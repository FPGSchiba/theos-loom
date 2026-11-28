//! One run's proxy container (R-b): Squid on the run's internal network and on the egress
//! network, with the generated config; its log followed with `podman logs --follow` and read
//! into `Egress` events. Linux only (D2).

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use loom_core::run::{RunEvent, RunId};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader, Lines};
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout, timeout_at};

use super::access_log::parse_access_line;
use super::squid_conf::{PROXY_PORT, check_range, squid_conf};
use crate::launcher::LaunchError;
use crate::podman::Podman;

/// What Squid logs once it accepts connections; the run's container starts only after it.
const READY_MARK: &str = "Accepting HTTP Socket connections";
/// How long Squid may take to get ready once its container runs.
const READY_LIMIT: Duration = Duration::from_secs(30);
/// `Egress` events buffered before the log reader waits for the runner.
const EVENT_BUFFER: usize = 1024;
/// Where the config is mounted.
const CONF_IN_CONTAINER: &str = "/etc/squid/squid.conf";

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
    /// Where the config file is written, in the run's directory.
    pub(crate) conf: &'a Path,
}

/// A started proxy: the task that reads its log, and the events it reads.
pub(crate) struct Proxy {
    reader: JoinHandle<()>,
    events: Option<mpsc::Receiver<RunEvent>>,
}

impl Proxy {
    /// Writes the config, starts the proxy and waits until it accepts connections.
    ///
    /// # Errors
    /// A bad blocked range, the config not written, a Podman call's error, or a proxy that
    /// does not get ready within [`READY_LIMIT`].
    pub(crate) async fn start(podman: &Podman, spec: &ProxySpec<'_>) -> Result<Self, LaunchError> {
        for range in spec.blocked {
            check_range(range).map_err(LaunchError::Other)?;
        }
        tokio::fs::write(spec.conf, squid_conf(spec.blocked))
            .await
            .map_err(|e| {
                LaunchError::Other(format!("cannot write {}: {e}", spec.conf.display()))
            })?;
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
            format!("{}:{CONF_IN_CONTAINER}:ro,z", spec.conf.display()),
            spec.image.into(),
        ];
        podman.run_detached(&args).await?;
        let mut logs = podman.logs_follow(&name);
        logs.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = logs.spawn().map_err(|source| LaunchError::Spawn {
            program: "podman logs".to_owned(),
            source,
        })?;
        let (ready_tx, ready_rx) = oneshot::channel();
        let (events_tx, events) = mpsc::channel(EVENT_BUFFER);
        let reader = tokio::spawn(read_log(child, ready_tx, events_tx));
        let proxy = Self {
            reader,
            events: Some(events),
        };
        match timeout(READY_LIMIT, ready_rx).await {
            Ok(Ok(())) => Ok(proxy),
            Ok(Err(_)) => Err(LaunchError::Other(
                "the proxy ended before it accepted connections".to_owned(),
            )),
            Err(_) => Err(LaunchError::Other(format!(
                "the proxy did not accept connections within {READY_LIMIT:?}"
            ))),
        }
    }

    /// The `Egress` events, once.
    pub(crate) fn take_events(&mut self) -> Option<mpsc::Receiver<RunEvent>> {
        self.events.take()
    }

    /// Waits until the log is read to its end (the proxy is gone), at most until `deadline`;
    /// then stops reading.
    pub(crate) async fn finish(mut self, deadline: Instant) {
        if timeout_at(deadline, &mut self.reader).await.is_err() {
            tracing::warn!("the proxy's log did not end in time");
        }
        // Dropping the task's future also kills `podman logs`.
        self.reader.abort();
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

/// Reads the proxy's output to its end: the first [`READY_MARK`] fires `ready`, each access
/// line becomes an event. Squid's own log lines are skipped.
async fn read_log(mut child: Child, ready: oneshot::Sender<()>, events: mpsc::Sender<RunEvent>) {
    let mut ready = Some(ready);
    let mut out = child.stdout.take().map(|o| BufReader::new(o).lines());
    let mut err = child.stderr.take().map(|e| BufReader::new(e).lines());
    while out.is_some() || err.is_some() {
        let line = tokio::select! {
            line = next(&mut out), if out.is_some() => line,
            line = next(&mut err), if err.is_some() => line,
        };
        let Some(line) = line else { continue };
        if line.contains(READY_MARK)
            && let Some(ready) = ready.take()
        {
            // Nobody waiting any more is no error.
            let _ = ready.send(());
        }
        if let Some(event) = parse_access_line(&line) {
            tracing::debug!(?event, "egress");
            // The runner may have stopped listening; the log is still read to its end.
            let _ = events.send(event).await;
        }
    }
    if let Err(error) = child.wait().await {
        tracing::debug!(%error, "waiting for `podman logs` failed");
    }
}

/// The next line of `lines`; at its end (or on an error) `lines` becomes `None`.
async fn next<R: AsyncRead + Unpin>(lines: &mut Option<Lines<BufReader<R>>>) -> Option<String> {
    let reader = lines.as_mut()?;
    match reader.next_line().await {
        Ok(Some(line)) => Some(line),
        Ok(None) => {
            *lines = None;
            None
        }
        Err(error) => {
            tracing::warn!(%error, "could not read the proxy's log");
            *lines = None;
            None
        }
    }
}
