//! Follows the proxy's access log, a file in the run's directory, into `Egress` events: lines
//! are read as they are written, and after the proxy stopped the rest is read to the end, so
//! even the tunnels Squid logs only when they close are recorded.
//!
//! The file is polled. A crate that watches files (`notify`) would add a dependency and a
//! platform layer for one file in one directory; a read at its end every [`POLL`] is enough.

use std::path::Path;
use std::time::Duration;

use loom_core::run::RunEvent;
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, sleep, timeout_at};
use tokio_util::sync::CancellationToken;

use super::access_log::parse_access_line;

/// How often the end of the file is checked for new lines.
pub const POLL: Duration = Duration::from_millis(200);

/// A followed access log.
#[derive(Debug)]
pub struct LogFollower {
    done: CancellationToken,
    task: JoinHandle<()>,
}

impl LogFollower {
    /// Follows the file at `path`, which must exist; each access line becomes an event on
    /// `events`.
    ///
    /// # Errors
    /// The file cannot be opened.
    pub async fn start(path: &Path, events: mpsc::Sender<RunEvent>) -> std::io::Result<Self> {
        let file = File::open(path).await?;
        let done = CancellationToken::new();
        let task = tokio::spawn(follow(file, events, done.clone()));
        Ok(Self { done, task })
    }

    /// Reads the rest of the file (the writer is gone) and stops, at most until `deadline`.
    pub async fn finish(mut self, deadline: Instant) {
        self.done.cancel();
        if timeout_at(deadline, &mut self.task).await.is_err() {
            tracing::warn!("the proxy's access log was not read to its end in time");
        }
        self.task.abort();
    }
}

impl Drop for LogFollower {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn follow(file: File, events: mpsc::Sender<RunEvent>, done: CancellationToken) {
    let mut reader = BufReader::new(file);
    // Kept across reads: a line written in two parts is completed by the next read.
    let mut line = Vec::new();
    loop {
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) if done.is_cancelled() => break,
            Ok(0) => {
                tokio::select! {
                    () = sleep(POLL) => {}
                    // One more read to the end follows.
                    () = done.cancelled() => {}
                }
            }
            Ok(_) if line.ends_with(b"\n") => take(&mut line, &events).await,
            // Part of a line; the rest comes with a later read.
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "could not read the proxy's access log");
                break;
            }
        }
    }
    take(&mut line, &events).await;
}

/// Sends the event of the line in `line`, which it empties. A line that is no access line is
/// logged: the file holds nothing else.
async fn take(line: &mut Vec<u8>, events: &mpsc::Sender<RunEvent>) {
    let text = String::from_utf8_lossy(line);
    let text = text.trim();
    if !text.is_empty() {
        match parse_access_line(text) {
            // The runner may have stopped listening; that is no error.
            Some(event) => drop(events.send(event).await),
            None => {
                tracing::warn!(line = %text, "a line of the proxy's access log is not understood");
            }
        }
    }
    line.clear();
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn egress(host: &str, allowed: bool) -> RunEvent {
        RunEvent::Egress {
            host: host.to_owned(),
            port: Some(443),
            allowed,
        }
    }

    // AC: TU-03.5
    #[tokio::test]
    async fn lines_written_before_and_after_the_end_are_all_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("access.log");
        let mut file = std::fs::File::create(&path).expect("create");
        writeln!(file, "1 TCP_TUNNEL 200 CONNECT a.example:443").expect("write");
        let (tx, mut rx) = mpsc::channel(16);
        let follower = LogFollower::start(&path, tx).await.expect("start");

        assert_eq!(rx.recv().await, Some(egress("a.example", true)));
        // A line in two parts, the second after the follower saw the end of the file.
        write!(file, "2 TCP_DENIED 403 CON").expect("write");
        file.flush().expect("flush");
        sleep(POLL * 2).await;
        writeln!(file, "NECT 10.0.0.1:443").expect("write");
        writeln!(file, "not an access line").expect("write");
        // The last line, written just before the writer is gone, with no newline.
        write!(file, "3 TCP_TUNNEL 200 CONNECT b.example:443").expect("write");
        drop(file);
        follower
            .finish(Instant::now() + Duration::from_secs(5))
            .await;

        let mut rest = Vec::new();
        while let Some(event) = rx.recv().await {
            rest.push(event);
        }
        assert_eq!(
            rest,
            vec![egress("10.0.0.1", false), egress("b.example", true)]
        );
    }

    #[tokio::test]
    async fn a_missing_file_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (tx, _rx) = mpsc::channel(1);
        assert!(
            LogFollower::start(&dir.path().join("none"), tx)
                .await
                .is_err()
        );
    }
}
