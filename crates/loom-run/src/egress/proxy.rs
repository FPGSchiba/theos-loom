//! One run's proxy container (R-b): Squid on the run's internal network and on the egress
//! network, with the generated config. Its access log is a file in the run's directory, mounted
//! into the container and followed into `Egress` events ([`super::log_file`]); Squid's own
//! output only tells when it is ready. Linux only (D2).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use loom_core::run::{RunEvent, RunId};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Child;
use tokio::sync::mpsc;
use tokio::time::{Instant, timeout};

use super::log_file::LogFollower;
use super::squid_conf::{ACCESS_LOG, PROXY_PORT, check_range, squid_conf};
use crate::launcher::LaunchError;
use crate::podman::Podman;

/// What Squid prints once it accepts connections; the run's container starts only after it.
const READY_MARK: &str = "Accepting HTTP Socket connections";
/// How long Squid may take to get ready once its container runs.
const READY_LIMIT: Duration = Duration::from_secs(30);
/// `Egress` events buffered before the log follower waits for the runner.
const EVENT_BUFFER: usize = 1024;
/// Where the config is mounted.
const CONF_IN_CONTAINER: &str = "/etc/squid/squid.conf";
/// The access log's mode on the host: Loom reads it, Squid (its `proxy` user, which is no user
/// of Loom's host) may only append to it.
const ACCESS_LOG_MODE: u32 = 0o602;

/// The proxy container's name, which is also its host name on the run's network.
#[must_use]
pub(crate) fn proxy_name(run: RunId) -> String {
    format!("loom-proxy-{run}")
}

/// The proxy's URL as the run's container sees it (`HTTP_PROXY`, `HTTPS_PROXY`).
#[must_use]
pub(crate) fn proxy_url(run: RunId) -> String {
    format!("http://{}:{PROXY_PORT}", proxy_name(run))
}

/// What one run's proxy is made of.
pub(crate) struct ProxySpec<'a> {
    /// The run.
    pub(crate) run: RunId,
    /// The run's label, `loom.run=<id>`.
    pub(crate) label: &'a str,
    /// The run's internal network.
    pub(crate) network: &'a str,
    /// The network the proxy reaches the outside through; created when missing.
    pub(crate) egress_network: &'a str,
    /// The Squid image.
    pub(crate) image: &'a str,
    /// Extra blocked ranges, from the run config.
    pub(crate) blocked: &'a [String],
    /// The directory for the config and the access log, in the run's directory.
    pub(crate) dir: &'a Path,
}

/// A started proxy: the follower of its access log, and the events it reads.
pub(crate) struct Proxy {
    log: LogFollower,
    events: Option<mpsc::Receiver<RunEvent>>,
}

impl Proxy {
    /// Writes the config and an empty access log, starts the proxy and waits until it accepts
    /// connections.
    ///
    /// # Errors
    /// A bad blocked range, a file not written, a Podman call's error, or a proxy that does not
    /// get ready within [`READY_LIMIT`].
    pub(crate) async fn start(podman: &Podman, spec: &ProxySpec<'_>) -> Result<Self, LaunchError> {
        for range in spec.blocked {
            check_range(range).map_err(LaunchError::Other)?;
        }
        let (conf, log) = write_files(spec).await?;
        podman.network_ensure(spec.egress_network).await?;
        let name = proxy_name(spec.run);
        let args: Vec<String> = vec![
            "--rm".into(),
            "--name".into(),
            name.clone(),
            // A log file Podman reads back on every host; journald may not be readable rootless.
            "--log-driver".into(),
            "k8s-file".into(),
            "--label".into(),
            spec.label.into(),
            "--network".into(),
            spec.network.into(),
            "--network".into(),
            spec.egress_network.into(),
            "-v".into(),
            format!("{}:{CONF_IN_CONTAINER}:ro,z", conf.display()),
            "-v".into(),
            format!("{}:{ACCESS_LOG}:z", log.display()),
            // Squid itself is the container's first process, so `podman stop` reaches it and it
            // logs its open tunnels as it ends; `-d 1` prints when it is ready.
            "--entrypoint".into(),
            "/usr/sbin/squid".into(),
            spec.image.into(),
            "-f".into(),
            CONF_IN_CONTAINER.into(),
            "-N".into(),
            "-d".into(),
            "1".into(),
        ];
        podman.run_detached(&args).await?;
        wait_ready(podman, &name).await?;
        let (events_tx, events) = mpsc::channel(EVENT_BUFFER);
        let log = LogFollower::start(&log, events_tx)
            .await
            .map_err(|e| LaunchError::Other(format!("cannot read {}: {e}", log.display())))?;
        Ok(Self {
            log,
            events: Some(events),
        })
    }

    /// The `Egress` events, once.
    pub(crate) fn take_events(&mut self) -> Option<mpsc::Receiver<RunEvent>> {
        self.events.take()
    }

    /// Reads the rest of the access log, once the proxy is stopped, at most until `deadline`.
    pub(crate) async fn finish(self, deadline: Instant) {
        self.log.finish(deadline).await;
    }
}

/// Writes the config and the empty access log into the spec's directory.
async fn write_files(spec: &ProxySpec<'_>) -> Result<(PathBuf, PathBuf), LaunchError> {
    let failed = |path: &Path, e: std::io::Error| {
        LaunchError::Other(format!("cannot write {}: {e}", path.display()))
    };
    tokio::fs::create_dir_all(spec.dir)
        .await
        .map_err(|e| failed(spec.dir, e))?;
    let conf = spec.dir.join("squid.conf");
    tokio::fs::write(&conf, squid_conf(spec.blocked))
        .await
        .map_err(|e| failed(&conf, e))?;
    let log = spec.dir.join("access.log");
    tokio::fs::write(&log, "")
        .await
        .map_err(|e| failed(&log, e))?;
    tokio::fs::set_permissions(&log, std::fs::Permissions::from_mode(ACCESS_LOG_MODE))
        .await
        .map_err(|e| failed(&log, e))?;
    Ok((conf, log))
}

/// Waits until the proxy prints [`READY_MARK`], at most [`READY_LIMIT`].
async fn wait_ready(podman: &Podman, name: &str) -> Result<(), LaunchError> {
    let mut logs = podman.logs_follow(name);
    logs.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = logs.spawn().map_err(|source| LaunchError::Spawn {
        program: "podman logs".to_owned(),
        source,
    })?;
    // Dropping `child` at the end kills `podman logs`.
    match timeout(READY_LIMIT, find_mark(child)).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(LaunchError::Other(
            "the proxy ended before it accepted connections".to_owned(),
        )),
        Err(_) => Err(LaunchError::Other(format!(
            "the proxy did not accept connections within {READY_LIMIT:?}"
        ))),
    }
}

/// Whether the output of `child` (either stream) shows [`READY_MARK`] before it ends.
async fn find_mark(mut child: Child) -> bool {
    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        return false;
    };
    let out = has_mark(out);
    let err = has_mark(err);
    tokio::pin!(out, err);
    let (mut out_ended, mut err_ended) = (false, false);
    while !(out_ended && err_ended) {
        let found = tokio::select! {
            found = &mut out, if !out_ended => { out_ended = true; found }
            found = &mut err, if !err_ended => { err_ended = true; found }
        };
        if found {
            return true;
        }
    }
    false
}

async fn has_mark<R: AsyncRead + Unpin>(pipe: R) -> bool {
    let mut lines = BufReader::new(pipe).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line.contains(READY_MARK) {
            return true;
        }
    }
    false
}
