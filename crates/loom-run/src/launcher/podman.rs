//! The Podman launcher (spec §5.3 steps 1, 4, 5 and 7): each run gets its own internal network,
//! its own proxy (the only way out, see [`crate::egress`]) and its own rootless container, alive
//! as long as the run (`sleep infinity` under `--init`); every command of the run is a
//! `podman exec` in it. Everything it creates carries the label `loom.run=<run id>`, so stopping
//! or aborting a run removes exactly that run's containers and networks. Linux only (D2).

use std::os::unix::fs::MetadataExt;
use std::time::Duration;

use loom_core::run::{RunEvent, RunId};
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout_at};

use super::child::Children;
use super::{BoxFuture, Exec, LaunchContext, LaunchError, Launcher, Session};
use crate::config::RunConfig;
use crate::egress::proxy::{Proxy, ProxySpec, proxy_name, proxy_url};
use crate::podman::{Podman, PodmanError, within};
use crate::workspace::run_dir;

/// The label key on everything of a run.
const RUN_LABEL: &str = "loom.run";
/// Where the run's clone is mounted, and the CLI's working directory.
const WORK_DIR: &str = "/work";
/// Where the kit is mounted, read-only.
const KIT_DIR: &str = "/kit";
/// How long `podman stop` waits after SIGTERM before it kills the run's container.
const STOP_GRACE: Duration = Duration::from_secs(3);
/// The same for the proxy: a moment for its last log lines to reach Loom.
const PROXY_STOP_GRACE: Duration = Duration::from_secs(1);
/// How long a whole stop may take, every Podman call included, so a run ends well within the
/// 10 s of TU-03.3 even when Podman hangs.
const STOP_LIMIT: Duration = Duration::from_secs(7);
/// The part of [`STOP_LIMIT`] kept for removing what is left.
const TEARDOWN_SHARE: Duration = Duration::from_secs(2);
/// How long a clean-up outside a stop (a failed start, an abort, a dropped session) may take.
const CLEANUP_LIMIT: Duration = Duration::from_secs(10);
/// The variables that point the run's tools at the proxy; both spellings, as tools differ.
const PROXY_VARS: [&str; 4] = ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"];

/// Runs each run in its own rootless Podman container, from the configured image, with the
/// network of T4: the public web through the run's proxy, nothing else.
///
/// Its `Debug` shows the names of the variables set with [`PodmanLauncher::with_env`], never
/// their values.
#[derive(Debug, Clone)]
pub struct PodmanLauncher {
    podman: Podman,
    config: RunConfig,
    /// Variables set with [`PodmanLauncher::with_env`], passed by name.
    env: Vec<String>,
    /// Extra `-v` specs.
    volumes: Vec<String>,
}

impl PodmanLauncher {
    /// A launcher with `config`'s Podman binary, image, proxy image, egress network and
    /// blocked ranges.
    #[must_use]
    pub fn new(config: &RunConfig) -> Self {
        Self {
            podman: Podman::new(config.podman.clone()),
            config: config.clone(),
            env: Vec::new(),
            volumes: Vec::new(),
        }
    }

    /// Sets an environment variable in every container. The value is set on the Podman process
    /// and passed as `--env NAME`, so it is never on a command line.
    #[must_use]
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let name = name.into();
        self.podman = self.podman.with_env(name.clone(), value);
        self.env.push(name);
        self
    }

    /// Mounts one more volume into every container: a `-v` spec, `<host>:<container>[:opts]`.
    #[must_use]
    pub fn with_volume(mut self, spec: impl Into<String>) -> Self {
        self.volumes.push(spec.into());
        self
    }

    /// Creates the run's network, proxy and container; returns the started session.
    async fn create(&self, ctx: &LaunchContext) -> Result<PodmanSession, LaunchError> {
        let label = run_label(ctx.run);
        let network = network_name(ctx.run);
        self.podman
            .network_create_internal(&network, &label)
            .await?;
        let conf = run_dir(&self.config, ctx.run).join("squid.conf");
        let mut proxy = Proxy::start(
            &self.podman,
            &ProxySpec {
                run: ctx.run,
                label: &label,
                network: &network,
                egress_network: &self.config.egress_network,
                image: &self.config.proxy_image,
                blocked: &self.config.blocked_ranges,
                conf: &conf,
            },
        )
        .await?;
        let owner = tokio::fs::metadata(&ctx.workspace).await.map_err(|e| {
            LaunchError::Other(format!("cannot read {}: {e}", ctx.workspace.display()))
        })?;
        let args = run_args(&RunArgs {
            ctx,
            label: &label,
            network: &network,
            user: (owner.uid(), owner.gid()),
            image: &self.config.image,
            env: &self.env,
            volumes: &self.volumes,
        })?;
        let id = self.podman.run_detached(&args).await?;
        let image = self.podman.image_id(&self.config.image).await?;
        Ok(PodmanSession {
            podman: self.podman.clone(),
            run: ctx.run,
            id,
            image,
            children: Children::new(),
            events: proxy.take_events(),
            proxy: Some(proxy),
            stopped: false,
        })
    }
}

impl Launcher for PodmanLauncher {
    /// Refuses unless Podman runs rootless (spec §5.3 step 1); on a failure after the check,
    /// removes whatever of the run it created before it returns the error.
    async fn start(&self, ctx: &LaunchContext) -> Result<Box<dyn Session>, LaunchError> {
        if !self.podman.is_rootless().await? {
            return Err(LaunchError::NotRootless);
        }
        match self.create(ctx).await {
            Ok(session) => Ok(Box::new(session)),
            Err(error) => {
                let deadline = Instant::now() + CLEANUP_LIMIT;
                if let Err(cleanup) = teardown(&self.podman, ctx.run, deadline).await {
                    tracing::warn!(error = %cleanup, run = %ctx.run, "could not clean up a failed start");
                }
                Err(error)
            }
        }
    }

    async fn abort(&self, run: RunId) -> Result<(), LaunchError> {
        let deadline = Instant::now() + CLEANUP_LIMIT;
        Ok(teardown(&self.podman, run, deadline).await?)
    }
}

/// What one `podman run` is made of.
struct RunArgs<'a> {
    ctx: &'a LaunchContext,
    label: &'a str,
    network: &'a str,
    user: (u32, u32),
    image: &'a str,
    env: &'a [String],
    volumes: &'a [String],
}

/// The arguments after `podman run -d` (spec §5.3 step 5; R-n). Credentials and other variables
/// go by name only: Podman copies their values from its own environment. The proxy's address is
/// no secret and goes by value.
fn run_args(a: &RunArgs<'_>) -> Result<Vec<String>, LaunchError> {
    let (uid, gid) = a.user;
    let mut args: Vec<String> = vec![
        "--init".into(),
        "--rm".into(),
        "--name".into(),
        worker_name(a.ctx.run),
        "--label".into(),
        a.label.into(),
        "--userns=keep-id".into(),
        "--user".into(),
        format!("{uid}:{gid}"),
        "--network".into(),
        a.network.into(),
        "-v".into(),
        format!("{}:{WORK_DIR}:z", a.ctx.workspace.display()),
        "-w".into(),
        WORK_DIR.into(),
        "-v".into(),
        format!("{}:{KIT_DIR}:ro,z", a.ctx.kit.display()),
    ];
    for volume in a.volumes {
        args.extend(["-v".into(), volume.clone()]);
    }
    args.extend(["--env".into(), format!("KIT_DIR={KIT_DIR}")]);
    let proxy = proxy_url(a.ctx.run);
    for var in PROXY_VARS {
        args.extend(["--env".into(), format!("{var}={proxy}")]);
    }
    for name in a.ctx.env.iter().chain(a.env) {
        if !is_env_name(name) {
            return Err(LaunchError::Other(format!(
                "{name:?} is not an environment variable name"
            )));
        }
        args.extend(["--env".into(), name.clone()]);
    }
    args.extend([a.image.into(), "sleep".into(), "infinity".into()]);
    Ok(args)
}

/// A plain variable name: `--env NAME=value` would put a value on the command line.
fn is_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn run_label(run: RunId) -> String {
    format!("{RUN_LABEL}={run}")
}

fn network_name(run: RunId) -> String {
    format!("loom-run-{run}")
}

fn worker_name(run: RunId) -> String {
    format!("loom-worker-{run}")
}

/// Removes every container and network of `run`, each call bounded by `deadline`. Tries them
/// all, also when a listing fails (then by their known names), and returns the first error.
/// Nothing of `run` existing is no error.
async fn teardown(podman: &Podman, run: RunId, deadline: Instant) -> Result<(), PodmanError> {
    let label = run_label(run);
    let mut first = None;
    let mut containers = vec![worker_name(run), proxy_name(run)];
    match within(deadline, "podman ps", podman.ls_by_label(&label)).await {
        Ok(ids) => containers.extend(ids),
        Err(error) => {
            first.get_or_insert(error);
        }
    }
    if let Err(error) = within(deadline, "podman rm", podman.rm_force(&containers)).await {
        first.get_or_insert(error);
    }
    let listed = within(
        deadline,
        "podman network ls",
        podman.networks_by_label(&label),
    )
    .await;
    let networks = match listed {
        Ok(names) => names,
        Err(error) => {
            first.get_or_insert(error);
            vec![network_name(run)]
        }
    };
    for name in networks {
        if let Err(error) = within(deadline, "podman network rm", podman.network_rm(&name)).await {
            first.get_or_insert(error);
        }
    }
    first.map_or(Ok(()), Err)
}

/// One run's container and proxy, and the `podman exec` clients it started.
struct PodmanSession {
    podman: Podman,
    run: RunId,
    id: String,
    image: String,
    children: Children,
    proxy: Option<Proxy>,
    /// The proxy's `Egress` events, until the runner takes them.
    events: Option<mpsc::Receiver<RunEvent>>,
    /// Set once `stop` ran; a session dropped before it is removed in the background.
    stopped: bool,
}

impl Session for PodmanSession {
    fn exec<'a>(
        &'a mut self,
        args: &'a [String],
        stdin: Option<String>,
    ) -> BoxFuture<'a, Result<Exec, LaunchError>> {
        let mut all = vec!["claude".to_owned()];
        all.extend(args.iter().cloned());
        let command = self.podman.exec(&self.id, &all, stdin.is_some());
        let exec = self.children.spawn(command, "podman exec", stdin);
        Box::pin(async move { exec })
    }

    /// Stops the container (`podman stop --time 3`; `--init` makes it reach every process)
    /// and the proxy, reads the proxy's log to its end, then removes everything of the run by
    /// force. All of it within [`STOP_LIMIT`], every Podman call bounded.
    fn stop(&mut self) -> BoxFuture<'_, Result<(), LaunchError>> {
        let children = self.children.stop();
        Box::pin(async move {
            let deadline = Instant::now() + STOP_LIMIT;
            let stops_end = deadline - TEARDOWN_SHARE;
            let proxy = proxy_name(self.run);
            let (worker, proxied) = tokio::join!(
                within(
                    stops_end,
                    "podman stop",
                    self.podman.stop(&self.id, STOP_GRACE)
                ),
                within(
                    stops_end,
                    "podman stop",
                    self.podman.stop(&proxy, PROXY_STOP_GRACE)
                ),
            );
            for error in [worker.err(), proxied.err()].into_iter().flatten() {
                tracing::warn!(%error, run = %self.run, "podman stop failed");
            }
            if let Some(proxy) = self.proxy.take() {
                proxy.finish(stops_end).await;
            }
            let removed = teardown(&self.podman, self.run, deadline).await;
            self.stopped = true;
            if let Ok(children) = timeout_at(deadline, children).await {
                children?;
            } else {
                tracing::warn!(run = %self.run, "the exec clients did not end in time");
            }
            Ok(removed?)
        })
    }

    fn take_events(&mut self) -> Option<mpsc::Receiver<RunEvent>> {
        self.events.take()
    }

    fn image_id(&self) -> Option<String> {
        Some(self.image.clone())
    }

    fn kit_dir(&self) -> String {
        KIT_DIR.to_owned()
    }
}

impl Drop for PodmanSession {
    fn drop(&mut self) {
        if self.stopped {
            return;
        }
        // Not stopped: remove the container in the background, when a runtime is there to do it.
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let podman = self.podman.clone();
            let run = self.run;
            runtime.spawn(async move {
                let deadline = Instant::now() + CLEANUP_LIMIT;
                if let Err(error) = teardown(&podman, run, deadline).await {
                    tracing::warn!(%error, %run, "could not remove a dropped session's container");
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn ctx(env: &[&str]) -> LaunchContext {
        LaunchContext {
            run: RunId::new(),
            workspace: PathBuf::from("/data/runs/r/work"),
            kit: PathBuf::from("/data/kits/k"),
            env: env.iter().map(|&n| n.to_owned()).collect(),
        }
    }

    fn args(ctx: &LaunchContext, env: &[String]) -> Result<Vec<String>, LaunchError> {
        run_args(&RunArgs {
            ctx,
            label: "loom.run=r",
            network: "loom-run-r",
            user: (1000, 1001),
            image: "img",
            env,
            volumes: &["/x:/y:ro".to_owned()],
        })
    }

    #[test]
    fn the_run_args_follow_the_image_contract() {
        let ctx = ctx(&["GH_TOKEN"]);
        let args = args(&ctx, &["FAKE_MODE".to_owned()]).expect("args");
        let text = args.join(" ");
        let run = ctx.run;
        assert!(
            text.starts_with(&format!(
                "--init --rm --name loom-worker-{run} --label loom.run=r --userns=keep-id \
                 --user 1000:1001"
            )),
            "{text}"
        );
        assert!(text.contains("--network loom-run-r"));
        assert!(text.contains("-v /data/runs/r/work:/work:z -w /work"));
        assert!(text.contains("-v /data/kits/k:/kit:ro,z -v /x:/y:ro"));
        let proxy = format!("http://loom-proxy-{run}:3128");
        assert!(
            text.contains(&format!(
                "--env KIT_DIR=/kit --env HTTP_PROXY={proxy} --env HTTPS_PROXY={proxy} \
                 --env http_proxy={proxy} --env https_proxy={proxy} --env GH_TOKEN --env FAKE_MODE"
            )),
            "{text}"
        );
        assert!(text.ends_with("img sleep infinity"));
    }

    #[test]
    fn an_env_name_with_a_value_is_refused() {
        assert!(args(&ctx(&["GH_TOKEN=abc"]), &[]).is_err());
        assert!(args(&ctx(&["1X"]), &[]).is_err());
        assert!(args(&ctx(&[]), &[String::new()]).is_err());
        assert!(is_env_name("_A1"));
    }

    #[test]
    fn debug_shows_env_names_never_values() {
        let config = RunConfig::from_toml("data_dir = 'd'\nimage = 'i'\nproxy_image = 'p'\n")
            .expect("config");
        let launcher = PodmanLauncher::new(&config).with_env("GH_TOKEN", "s3cr3t-value");
        let text = format!("{launcher:?}");
        assert!(text.contains("GH_TOKEN"), "{text}");
        assert!(!text.contains("s3cr3t"), "{text}");
    }
}
