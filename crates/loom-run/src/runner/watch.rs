//! The read loop (spec §5.3 step 6–7): every stdout event is sent on, counted and checked
//! against the caps; rate limits and sign-in failures stop at once (R-o); a timer checks the
//! caps while the CLI is silent; the CLI's exit decides the rest.

use std::time::Duration;

use loom_core::run::{RawStreamStore, RunEvent, StopCause};
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior, interval};

use super::run::{Run, StepError};
use crate::deadline::sleep_until_some;
use crate::launcher::{BoxFuture, LaunchError, Launcher};

/// How often the caps are checked when no event arrives.
const CAP_TICK: Duration = Duration::from_millis(250);
/// How long the output may stay open after the CLI exited (a child may hold it).
const EXIT_GRACE: Duration = Duration::from_secs(2);

impl<L: Launcher + 'static, S: RawStreamStore + 'static> Run<L, S> {
    /// Reads until a stop cause: one from an event or a cap, or the CLI's exit once its stdout
    /// is read to the end.
    pub(super) async fn watch(
        &mut self,
        mut lines: mpsc::Receiver<RunEvent>,
        mut exit: BoxFuture<'static, Result<Option<i32>, LaunchError>>,
        started: Instant,
    ) -> Result<StopCause, StepError> {
        let mut tick = interval(CAP_TICK);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut stdout_open = true;
        let mut exited: Option<Option<i32>> = None;
        let mut grace: Option<Instant> = None;
        loop {
            tokio::select! {
                biased;
                line = lines.recv(), if stdout_open => match line {
                    Some(event) => {
                        if let Some(cause) = self.on_event(event, started).await {
                            return Ok(cause);
                        }
                    }
                    None => stdout_open = false,
                },
                code = &mut exit, if exited.is_none() => {
                    exited = Some(code?);
                    grace = Some(Instant::now() + EXIT_GRACE);
                }
                () = sleep_until_some(grace), if stdout_open && grace.is_some() => {
                    tracing::warn!(run = %self.id, "the CLI exited but its output stayed open");
                    stdout_open = false;
                }
                _ = tick.tick() => {
                    if let Some(cause) = self.check_caps(started) {
                        return Ok(cause);
                    }
                }
            }
            if let (false, Some(code)) = (stdout_open, exited) {
                return Ok(match code {
                    Some(0) => StopCause::Completed,
                    exit_code => StopCause::Failed { exit_code },
                });
            }
        }
    }

    /// Counts and sends one event; returns the cause it stops the run with, if any.
    async fn on_event(&mut self, event: RunEvent, started: Instant) -> Option<StopCause> {
        let stop = match &event {
            RunEvent::Assistant {
                message_id,
                model,
                usage: Some(usage),
                ..
            } => {
                self.ledger.record(message_id, model, *usage);
                None
            }
            RunEvent::Result {
                total_cost_usd: Some(cost),
                ..
            } => {
                self.reported_usd = Some(*cost);
                None
            }
            RunEvent::RateLimited { resets_at, .. } => Some(StopCause::RateLimited {
                resets_at: *resets_at,
            }),
            RunEvent::AuthFailed { .. } => Some(StopCause::AuthFailed),
            _ => None,
        };
        self.emit(event).await;
        stop.or_else(|| self.check_caps(started))
    }

    fn check_caps(&self, started: Instant) -> Option<StopCause> {
        self.spec
            .caps
            .check(&self.ledger, self.prices.as_deref(), started.elapsed())
    }
}
