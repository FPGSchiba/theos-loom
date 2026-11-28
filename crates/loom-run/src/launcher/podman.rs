//! The Podman launcher (spec §5.3 steps 1, 5 and 7): each run gets its own rootless container,
//! alive as long as the run (`sleep infinity` under `--init`), and every command of the run is a
//! `podman exec` in it. Everything it creates carries the label `loom.run=<run id>`, so stopping
//! or aborting a run removes exactly that run's containers and networks. Linux only (D2).

use std::os::unix::fs::MetadataExt;
use std::time::Duration;

use loom_core::run::RunId;
use tokio::time::timeout;

use super::child::Children;
use super::{BoxFuture, Exec, LaunchContext, LaunchError, Launcher, Session};
use crate::config::RunConfig;
use crate::podman::{Podman, PodmanError};

/// The label key on everything of a run.
const RUN_LABEL: &str = "loom.run";
/// Where the run's clone is mounted, and the CLI's working directory.
const WORK_DIR: &str = "/work";
/// Where the kit is mounted, read-only.
const KIT_DIR: &str = "/kit";
/// How long `podman stop` waits after SIGTERM before it kills.
const STOP_GRACE: Duration = Duration::from_secs(3);
/// How long the `podman stop` call may take in all; then the container is removed by force,
/// so a stop ends well within the 10 s of TU-03.3.
const STOP_LIMIT: Duration = Duration::from_secs(6);

/// Runs each run in its own rootless Podman container, from the configured image.
#[derive(Debug, Clone)]
pub struct PodmanLauncher {
    podman: Podman,
    image: String,
    /// Variables set with [`PodmanLauncher::with_env`], passed by name.
    env: Vec<String>,
    /// Extra `-v` specs.
    volumes: Vec<String>,
}

impl PodmanLauncher {
    /// A launcher with `config`'s Podman binary and image.
    #[must_use]
    pub fn new(config: &RunConfig) -> Self {
        Self {
            podman: Podman::new(config.podman.clone()),
            image: config.image.clone(),
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

    /// Creates the run's network and container; returns the started session.
    async fn create(&self, ctx: &LaunchContext) -> Result<PodmanSession, LaunchError> {
        let label = run_label(ctx.run);
        let network = network_name(ctx.run);
        self.podman
            .network_create_internal(&network, &label)
            .await?;
        let owner = tokio::fs::metadata(&ctx.workspace).await.map_err(|e| {
            LaunchError::Other(format!("cannot read {}: {e}", ctx.workspace.display()))
        })?;
        let args = run_args(&RunArgs {
            ctx,
            label: &label,
            network: &network,
            user: (owner.uid(), owner.gid()),
            image: &self.image,
            env: &self.env,
            volumes: &self.volumes,
        })?;
        let id = self.podman.run_detached(&args).await?;
        let image = self.podman.image_id(&self.image).await?;
        Ok(PodmanSession {
            podman: self.podman.clone(),
            run: ctx.run,
            id,
            image,
            children: Children::new(),
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
                if let Err(cleanup) = teardown(&self.podman, ctx.run).await {
                    tracing::warn!(error = %cleanup, run = %ctx.run, "could not clean up a failed start");
                }
                Err(error)
            }
        }
    }

    async fn abort(&self, run: RunId) -> Result<(), LaunchError> {
        Ok(teardown(&self.podman, run).await?)
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
/// go by name only: Podman copies their values from its own environment.
fn run_args(a: &RunArgs<'_>) -> Result<Vec<String>, LaunchError> {
    let (uid, gid) = a.user;
    let mut args: Vec<String> = vec![
        "--init".into(),
        "--rm".into(),
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

/// Removes every container and network of `run`; tries them all and returns the first error.
/// Nothing of `run` existing is no error.
async fn teardown(podman: &Podman, run: RunId) -> Result<(), PodmanError> {
    let label = run_label(run);
    let mut first = None;
    for id in podman.ls_by_label(&label).await? {
        if let Err(error) = podman.rm_force(&id).await {
            first.get_or_insert(error);
        }
    }
    for name in podman.networks_by_label(&label).await? {
        if let Err(error) = podman.network_rm(&name).await {
            first.get_or_insert(error);
        }
    }
    first.map_or(Ok(()), Err)
}

/// One run's container, and the `podman exec` clients it started.
struct PodmanSession {
    podman: Podman,
    run: RunId,
    id: String,
    image: String,
    children: Children,
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

    /// `podman stop --time 3`, at most [`STOP_LIMIT`]; then everything of the run is removed
    /// by force. `--init` makes the stop reach every process in the container.
    fn stop(&mut self) -> BoxFuture<'_, Result<(), LaunchError>> {
        let children = self.children.stop();
        Box::pin(async move {
            match timeout(STOP_LIMIT, self.podman.stop(&self.id, STOP_GRACE)).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(%error, run = %self.run, "podman stop failed"),
                Err(_) => tracing::warn!(run = %self.run, "podman stop took too long"),
            }
            let removed = teardown(&self.podman, self.run).await;
            self.stopped = true;
            children.await?;
            Ok(removed?)
        })
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
                if let Err(error) = teardown(&podman, run).await {
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
        let args = args(&ctx(&["GH_TOKEN"]), &["FAKE_MODE".to_owned()]).expect("args");
        let text = args.join(" ");
        assert!(
            text.starts_with("--init --rm --label loom.run=r --userns=keep-id --user 1000:1001")
        );
        assert!(text.contains("--network loom-run-r"));
        assert!(text.contains("-v /data/runs/r/work:/work:z -w /work"));
        assert!(text.contains("-v /data/kits/k:/kit:ro,z -v /x:/y:ro"));
        assert!(text.contains("--env KIT_DIR=/kit --env GH_TOKEN --env FAKE_MODE"));
        assert!(text.ends_with("img sleep infinity"));
    }

    #[test]
    fn an_env_name_with_a_value_is_refused() {
        assert!(args(&ctx(&["GH_TOKEN=abc"]), &[]).is_err());
        assert!(args(&ctx(&["1X"]), &[]).is_err());
        assert!(args(&ctx(&[]), &[String::new()]).is_err());
        assert!(is_env_name("_A1"));
    }
}
