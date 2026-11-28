//! Reads one output stream of the CLI: every byte goes to the raw stream store (R-h), and on
//! stdout every line is parsed into a [`RunEvent`] for the runner.

use loom_core::run::{RawStreamStore, RunEvent};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::chunks::ChunkWriter;
use crate::deadline::sleep_until_some;
use crate::launcher::Pipe;
use crate::stream::parse_line;

/// Starts a task that reads `pipe` to its end, or until `stop` fires. Each line is stored
/// byte for byte through `writer`; with `events`, its parsed event is sent there too.
pub(crate) fn spawn<S: RawStreamStore + 'static>(
    pipe: Pipe,
    writer: ChunkWriter<S>,
    events: Option<mpsc::Sender<RunEvent>>,
    stop: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(pump(pipe, writer, events, stop))
}

async fn pump<S: RawStreamStore>(
    pipe: Pipe,
    mut writer: ChunkWriter<S>,
    events: Option<mpsc::Sender<RunEvent>>,
    stop: CancellationToken,
) {
    let mut reader = BufReader::new(pipe);
    // Kept across iterations: `read_until` is cancel safe only as long as its buffer survives.
    let mut line = Vec::new();
    loop {
        let due = writer.deadline();
        tokio::select! {
            biased;
            () = stop.cancelled() => break,
            read = reader.read_until(b'\n', &mut line) => match read {
                Ok(0) => break,
                Ok(_) if line.ends_with(b"\n") => {
                    take_line(&mut line, &mut writer, events.as_ref()).await;
                }
                // The last line has no newline; the next read sees the end.
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "could not read the CLI's output");
                    break;
                }
            },
            () = sleep_until_some(due) => writer.flush_if_due().await,
        }
    }
    take_line(&mut line, &mut writer, events.as_ref()).await;
    writer.finish().await;
}

/// Stores `line` and sends its event; empties it.
async fn take_line<S: RawStreamStore>(
    line: &mut Vec<u8>,
    writer: &mut ChunkWriter<S>,
    events: Option<&mpsc::Sender<RunEvent>>,
) {
    if line.is_empty() {
        return;
    }
    writer.write(line).await;
    let text = line.strip_suffix(b"\n").unwrap_or(line);
    let text = text.strip_suffix(b"\r").unwrap_or(text);
    if let Some(events) = events
        && !text.trim_ascii().is_empty()
    {
        // The runner may have stopped listening; the bytes are stored all the same.
        let _ = events.send(parse_line(text)).await;
    }
    line.clear();
}
