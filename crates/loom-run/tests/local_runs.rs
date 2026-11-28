//! Whole runs through the local launcher and the fake CLI (TU-03.2, TU-03.4, TU-03.6).
// Test code throughout, helpers included, as in `support/mod.rs`.
#![cfg(test)]

#[path = "support/fake.rs"]
mod fake;
mod support;

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use loom_core::run::{CapKind, RawStreamStore, RunEvent, RunId, StopCause, StreamKind, TokenKind};
use loom_run::caps::Caps;
use loom_run::config::{KitRef, RunSpec};
use loom_run::launcher::{LaunchContext, LaunchError, Launcher, LocalLauncher, Session};
use loom_run::prices::PriceBook;
use loom_run::runner::{PreludeLimits, Runner};
use loom_run::stream::parse_line;
use loom_store::PgRawStreamStore;
use loom_store::testing::TestDb;
use support::{config, git, make_repo, spec};

const MODEL: &str = "claude-haiku-5-5";
/// What the fixture's usage costs at these prices is within 5 % of its `total_cost_usd`.
const FAIR_PRICES: &str = r#"{"claude-haiku-5-5": {
    "input_cost_per_token": 1e-7, "output_cost_per_token": 5e-7,
    "cache_creation_input_token_cost": 1.25e-7, "cache_creation_input_token_cost_above_1hr": 2e-7,
    "cache_read_input_token_cost": 1e-8}}"#;
/// Ten times [`FAIR_PRICES`].
const HIGH_PRICES: &str = r#"{"claude-haiku-5-5": {
    "input_cost_per_token": 1e-6, "output_cost_per_token": 5e-6,
    "cache_creation_input_token_cost": 1.25e-6, "cache_creation_input_token_cost_above_1hr": 2e-6,
    "cache_read_input_token_cost": 1e-7}}"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/streams")
        .join(name)
}

fn book(json: &str) -> PriceBook {
    PriceBook::from_litellm(json, &BTreeMap::new()).expect("prices parse")
}

/// One test's world: a repository, a kit at `v1`, a data dir and a throwaway database.
struct World {
    tmp: tempfile::TempDir,
    db: TestDb,
    spec: RunSpec,
}

impl World {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let repo = tmp.path().join("repo");
        make_repo(&repo, &[("a.txt", "a")]);
        let kit = tmp.path().join("kit");
        make_repo(&kit, &[("VERSION", "1.2.3\n")]);
        git(&kit, &["tag", "v1"]);
        let kit = KitRef {
            repository: kit.display().to_string(),
            version: "v1".to_owned(),
        };
        let mut spec = spec(&repo, "story/ED-12", kit);
        spec.prompt = "Reply with hi.\nSecond line.".to_owned();
        let db = TestDb::create().await.expect("test database");
        Self { tmp, db, spec }
    }

    fn data_dir(&self) -> PathBuf {
        self.tmp.path().join("data")
    }

    fn store(&self) -> PgRawStreamStore {
        PgRawStreamStore::new(self.db.pool().clone())
    }

    /// Runs `spec` to its end; returns the run's id, every event and the cause `join` gives.
    async fn run(
        &self,
        launcher: LocalLauncher,
        spec: RunSpec,
        prices: Option<PriceBook>,
    ) -> (RunId, Vec<RunEvent>, StopCause) {
        let runner = Runner::new(config(&self.data_dir()), launcher, self.store(), prices);
        let (handle, mut rx) = runner.start(spec).expect("start");
        let id = handle.id;
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        (id, events, handle.join().await)
    }

    async fn end(self) {
        self.db.drop_db().await.expect("drop the test database");
    }
}

fn replay(stream: &str) -> LocalLauncher {
    LocalLauncher::new(fake::fake_claude())
        .with_env("FAKE_MODE", "replay")
        .with_env("FAKE_STREAM", fixture(stream).display().to_string())
}

fn ended(events: &[RunEvent]) -> &StopCause {
    match events.last() {
        Some(RunEvent::RunEnded { cause }) => cause,
        other => panic!("the last event is not RunEnded: {other:?}"),
    }
}

fn position(events: &[RunEvent], wanted: impl Fn(&RunEvent) -> bool) -> usize {
    events
        .iter()
        .position(wanted)
        .unwrap_or_else(|| panic!("event missing in {events:#?}"))
}

/// The events after the prelude (kit, image, update, version) and before the end.
fn stream_events(events: &[RunEvent]) -> &[RunEvent] {
    let start = position(events, |e| matches!(e, RunEvent::CliVersion { .. })) + 1;
    let end = events.len() - 1;
    &events[start..end]
}

fn with_caps(mut spec: RunSpec, caps: Caps) -> RunSpec {
    spec.caps = caps;
    spec
}

// AC: TU-03.2
#[tokio::test(flavor = "multi_thread")]
async fn a_replayed_run_yields_typed_events_and_its_raw_stream() {
    let world = World::new().await;
    let stderr = "warning: something on stderr\nsecond line\n";
    let launcher = replay("normal_run.jsonl").with_env("FAKE_STDERR", stderr);

    let (id, events, cause) = world.run(launcher, world.spec.clone(), None).await;

    let bytes = std::fs::read(fixture("normal_run.jsonl")).expect("fixture");
    let expected: Vec<RunEvent> = bytes
        .split_inclusive(|b| *b == b'\n')
        .map(|line| parse_line(line.strip_suffix(b"\n").unwrap_or(line)))
        .collect();
    assert_eq!(stream_events(&events), expected.as_slice());
    assert_eq!(ended(&events), &StopCause::Completed);
    assert_eq!(cause, StopCause::Completed);
    let store = world.store();
    assert_eq!(
        store.read(id, StreamKind::Stdout).await.expect("stdout"),
        bytes
    );
    assert_eq!(
        store.read(id, StreamKind::Stderr).await.expect("stderr"),
        stderr.as_bytes()
    );
    assert!(
        !world.data_dir().join("runs").join(id.to_string()).exists(),
        "the run directory is removed"
    );
    world.end().await;
}

// AC: TU-03.2
#[tokio::test(flavor = "multi_thread")]
async fn the_cli_is_updated_and_its_version_recorded() {
    let world = World::new().await;

    let (_, events, _) = world
        .run(
            replay("normal_run.jsonl"),
            world.spec.clone(),
            Some(book(FAIR_PRICES)),
        )
        .await;

    let kit = position(
        &events,
        |e| matches!(e, RunEvent::Kit { version, .. } if version == "v1"),
    );
    let version = position(
        &events,
        |e| matches!(e, RunEvent::CliVersion { version } if version == "9.9.9 (Fake)"),
    );
    let first = position(&events, |e| matches!(e, RunEvent::Init { .. }));
    assert!(kit < version && version < first, "{events:#?}");
    assert!(
        !events.iter().any(|e| matches!(
            e,
            RunEvent::CliUpdateFailed { .. } | RunEvent::SpendMismatch { .. }
        )),
        "{events:#?}"
    );
    assert_eq!(ended(&events), &StopCause::Completed);
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_update_still_runs() {
    let world = World::new().await;
    let launcher = replay("normal_run.jsonl").with_env("FAKE_UPDATE_FAILS", "1");

    let (_, events, _) = world.run(launcher, world.spec.clone(), None).await;

    let failed = position(&events, |e| matches!(e, RunEvent::CliUpdateFailed { .. }));
    let version = position(&events, |e| matches!(e, RunEvent::CliVersion { .. }));
    assert!(failed < version);
    assert_eq!(ended(&events), &StopCause::Completed);
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_prompt_goes_on_stdin() {
    let world = World::new().await;
    let launcher = LocalLauncher::new(fake::fake_claude()).with_env("FAKE_MODE", "report");
    let mut spec = world.spec.clone();
    spec.model = Some("model-x".to_owned());
    spec.extra_args = vec!["--max-turns".to_owned(), "3".to_owned()];

    let (_, events, _) = world.run(launcher, spec.clone(), None).await;

    let report = events
        .iter()
        .find_map(|e| match e {
            RunEvent::Unknown { raw } if raw.contains("fake_report") => {
                Some(serde_json::from_str::<serde_json::Value>(raw).expect("report is JSON"))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no report in {events:#?}"));
    assert_eq!(report["stdin"], spec.prompt.as_str());
    let args: Vec<&str> = report["args"]
        .as_array()
        .expect("args")
        .iter()
        .filter_map(|a| a.as_str())
        .collect();
    assert_eq!(
        &args[..5],
        [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--dangerously-skip-permissions"
        ]
    );
    let plugin = args
        .iter()
        .position(|a| *a == "--plugin-dir")
        .expect("--plugin-dir");
    let kit_dir = Path::new(args[plugin + 1]);
    assert_eq!(
        // Trimmed: git may check the file out with CRLF on Windows.
        std::fs::read_to_string(kit_dir.join("VERSION"))
            .expect("the kit is there")
            .trim(),
        "1.2.3"
    );
    assert_eq!(report["kit_version"], "1.2.3");
    assert_eq!(report["branch"], "story/ED-12");
    assert_eq!(
        &args[plugin + 2..],
        ["--model", "model-x", "--max-turns", "3"]
    );
    assert!(!args.iter().any(|a| a.contains("Reply with hi")));
    assert_eq!(ended(&events), &StopCause::Completed);
    world.end().await;
}

// AC: TU-03.4
#[tokio::test(flavor = "multi_thread")]
async fn a_token_cap_stops_the_run_with_its_cause() {
    let world = World::new().await;
    let launcher = replay("normal_run.jsonl").with_env("FAKE_THEN", "sleep");
    let caps = Caps {
        cache_read_tokens: Some(17_000),
        ..Caps::default()
    };

    let (_, events, cause) = world
        .run(launcher, with_caps(world.spec.clone(), caps), None)
        .await;

    let want = StopCause::Cap(CapKind::Tokens(TokenKind::CacheRead));
    assert_eq!(ended(&events), &want);
    assert_eq!(cause, want);
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, RunEvent::RunEnded { .. }))
            .count(),
        1
    );
    assert!(
        matches!(events[events.len() - 2], RunEvent::Assistant { .. }),
        "the event that crossed the cap is the last before the end: {events:#?}"
    );
    world.end().await;
}

// AC: TU-03.4
#[tokio::test(flavor = "multi_thread")]
async fn a_spend_cap_stops_the_run() {
    let world = World::new().await;
    let launcher = replay("normal_run.jsonl").with_env("FAKE_THEN", "sleep");
    let caps = Caps {
        spend_usd: Some(0.001),
        ..Caps::default()
    };

    let (_, events, _) = world
        .run(
            launcher,
            with_caps(world.spec.clone(), caps),
            Some(book(FAIR_PRICES)),
        )
        .await;

    assert_eq!(ended(&events), &StopCause::Cap(CapKind::Spend));
    world.end().await;
}

// AC: TU-03.4
#[tokio::test(flavor = "multi_thread")]
async fn a_model_without_a_price_stops_a_run_with_a_spend_cap() {
    let world = World::new().await;
    let launcher = replay("normal_run.jsonl").with_env("FAKE_THEN", "sleep");
    let caps = Caps {
        spend_usd: Some(100.0),
        ..Caps::default()
    };
    let other_models = book(r#"{"model-a": {"input_cost_per_token": 1e-6}}"#);

    let (_, events, _) = world
        .run(
            launcher,
            with_caps(world.spec.clone(), caps),
            Some(other_models),
        )
        .await;

    assert_eq!(
        ended(&events),
        &StopCause::NoPrice {
            model: MODEL.to_owned()
        }
    );
    world.end().await;
}

// AC: TU-03.4
#[tokio::test(flavor = "multi_thread")]
async fn a_silent_run_stops_at_its_time_cap() {
    let world = World::new().await;
    let launcher = LocalLauncher::new(fake::fake_claude()).with_env("FAKE_MODE", "sleep");
    let caps = Caps {
        wall_time: Some(Duration::from_secs(2)),
        ..Caps::default()
    };
    let runner = Runner::new(config(&world.data_dir()), launcher, world.store(), None);
    let (handle, mut rx) = runner
        .start(with_caps(world.spec.clone(), caps))
        .expect("start");

    let mut started = None;
    let mut last = None;
    while let Some(event) = rx.recv().await {
        if matches!(event, RunEvent::CliVersion { .. }) {
            started = Some(Instant::now());
        }
        last = Some(event);
    }
    let started = started.expect("the CLI version is recorded");

    assert!(
        started.elapsed() < Duration::from_secs(4),
        "stopped after {:?}",
        started.elapsed()
    );
    assert_eq!(
        last,
        Some(RunEvent::RunEnded {
            cause: StopCause::Cap(CapKind::WallTime)
        })
    );
    assert_eq!(handle.join().await, StopCause::Cap(CapKind::WallTime));
    world.end().await;
}

// AC: TU-03.6
#[tokio::test(flavor = "multi_thread")]
async fn a_rate_limit_ends_the_run_as_rate_limited() {
    let world = World::new().await;
    let launcher = replay("rate_limit_rejected.jsonl").with_env("FAKE_THEN", "sleep");
    let begun = Instant::now();

    let (_, events, cause) = world.run(launcher, world.spec.clone(), None).await;

    let want = StopCause::RateLimited {
        resets_at: Some(1_795_891_800),
    };
    assert_eq!(ended(&events), &want);
    assert_eq!(cause, want);
    assert!(
        matches!(events[events.len() - 2], RunEvent::RateLimited { .. }),
        "{events:#?}"
    );
    assert!(
        begun.elapsed() < Duration::from_secs(30),
        "at once, not after the CLI's sleep"
    );
    world.end().await;
}

// AC: TU-03.6
#[tokio::test(flavor = "multi_thread")]
async fn a_429_result_ends_the_run_as_rate_limited() {
    let world = World::new().await;
    let launcher = replay("result_429.jsonl").with_env("FAKE_EXIT", "1");

    let (_, events, _) = world.run(launcher, world.spec.clone(), None).await;

    assert!(
        matches!(ended(&events), StopCause::RateLimited { .. }),
        "{events:#?}"
    );
    world.end().await;
}

// AC: TU-03.6
#[tokio::test(flavor = "multi_thread")]
async fn an_auth_failure_ends_the_run_as_auth_failed() {
    let world = World::new().await;
    let launcher = replay("auth_failure.jsonl").with_env("FAKE_EXIT", "1");

    let (_, events, cause) = world.run(launcher, world.spec.clone(), None).await;

    assert_eq!(ended(&events), &StopCause::AuthFailed);
    assert_eq!(cause, StopCause::AuthFailed);
    assert!(
        matches!(events[events.len() - 2], RunEvent::AuthFailed { .. }),
        "{events:#?}"
    );
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_cli_ends_as_failed() {
    let world = World::new().await;
    let launcher = replay("normal_run.jsonl").with_env("FAKE_EXIT", "3");

    let (_, events, _) = world.run(launcher, world.spec.clone(), None).await;

    assert_eq!(ended(&events), &StopCause::Failed { exit_code: Some(3) });
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_run_ends_as_cancelled() {
    let world = World::new().await;
    let launcher = LocalLauncher::new(fake::fake_claude()).with_env("FAKE_MODE", "sleep");
    let runner = Runner::new(config(&world.data_dir()), launcher, world.store(), None);
    let (handle, mut rx) = runner.start(world.spec.clone()).expect("start");
    let id = handle.id;

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        let init = matches!(event, RunEvent::Init { .. });
        events.push(event);
        if init {
            handle.cancel();
        }
    }

    assert_eq!(ended(&events), &StopCause::Cancelled);
    assert_eq!(handle.join().await, StopCause::Cancelled);
    assert!(!world.data_dir().join("runs").join(id.to_string()).exists());
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_cancelled_before_it_starts_cleans_up() {
    let world = World::new().await;
    let launcher = replay("normal_run.jsonl");
    let runner = Runner::new(config(&world.data_dir()), launcher, world.store(), None);
    let (handle, mut rx) = runner.start(world.spec.clone()).expect("start");
    let id = handle.id;
    handle.cancel();

    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    assert_eq!(ended(&events), &StopCause::Cancelled);
    assert!(!world.data_dir().join("runs").join(id.to_string()).exists());
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_spend_mismatch_is_recorded() {
    let world = World::new().await;

    let (_, events, _) = world
        .run(
            replay("normal_run.jsonl"),
            world.spec.clone(),
            Some(book(HIGH_PRICES)),
        )
        .await;

    let mismatch = events
        .iter()
        .find_map(|e| match e {
            RunEvent::SpendMismatch {
                computed_usd,
                reported_usd,
            } => Some((*computed_usd, *reported_usd)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no mismatch in {events:#?}"));
    assert!((mismatch.1 - 0.004_479_76).abs() < 1e-12, "{mismatch:?}");
    assert!(mismatch.0 > 0.04, "{mismatch:?}");
    assert!(matches!(
        events[events.len() - 2],
        RunEvent::SpendMismatch { .. }
    ));
    assert_eq!(ended(&events), &StopCause::Completed);
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_kept_run_dir_stays() {
    let world = World::new().await;
    let mut config = config(&world.data_dir());
    config.keep_run_dirs = true;
    let runner = Runner::new(config, replay("normal_run.jsonl"), world.store(), None);
    let (handle, mut rx) = runner.start(world.spec.clone()).expect("start");
    while rx.recv().await.is_some() {}
    let id = handle.id;

    assert_eq!(handle.join().await, StopCause::Completed);
    assert!(
        world
            .data_dir()
            .join("runs")
            .join(id.to_string())
            .join("work")
            .is_dir()
    );
    world.end().await;
}

/// A launcher whose start never finishes (or fails at once), and which records `abort`.
#[derive(Clone, Default)]
struct StuckLauncher {
    fails: bool,
    entered: Arc<tokio::sync::Notify>,
    aborted: Arc<AtomicBool>,
}

impl Launcher for StuckLauncher {
    fn start(
        &self,
        _ctx: &LaunchContext,
    ) -> impl Future<Output = Result<Box<dyn Session>, LaunchError>> + Send {
        let this = self.clone();
        async move {
            this.entered.notify_one();
            if !this.fails {
                tokio::time::sleep(Duration::from_secs(3600)).await;
            }
            Err(LaunchError::Other("the start failed".to_owned()))
        }
    }

    fn abort(&self, _run: RunId) -> impl Future<Output = Result<(), LaunchError>> + Send {
        self.aborted.store(true, Ordering::SeqCst);
        std::future::ready(Ok(()))
    }
}

async fn run_stuck(world: &World, launcher: StuckLauncher, cancel: bool) -> (RunId, Vec<RunEvent>) {
    let runner = Runner::new(
        config(&world.data_dir()),
        launcher.clone(),
        world.store(),
        None,
    );
    let (handle, mut rx) = runner.start(world.spec.clone()).expect("start");
    let id = handle.id;
    launcher.entered.notified().await;
    if cancel {
        handle.cancel();
    }
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }
    handle.join().await;
    (id, events)
}

fn ends(events: &[RunEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, RunEvent::RunEnded { .. }))
        .count()
}

// The container launcher relies on this to leave nothing of a cancelled start (TU-03.3).
#[tokio::test(flavor = "multi_thread")]
async fn a_cancel_during_the_start_aborts_it() {
    let world = World::new().await;
    let launcher = StuckLauncher::default();

    let (id, events) = run_stuck(&world, launcher.clone(), true).await;

    assert_eq!(ended(&events), &StopCause::Cancelled);
    assert_eq!(ends(&events), 1);
    assert!(launcher.aborted.load(Ordering::SeqCst), "abort was called");
    assert!(!world.data_dir().join("runs").join(id.to_string()).exists());
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_start_is_aborted() {
    let world = World::new().await;
    let launcher = StuckLauncher {
        fails: true,
        ..StuckLauncher::default()
    };

    let (_, events) = run_stuck(&world, launcher.clone(), false).await;

    assert_eq!(ended(&events), &StopCause::Failed { exit_code: None });
    assert_eq!(ends(&events), 1);
    assert!(launcher.aborted.load(Ordering::SeqCst), "abort was called");
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_never_launched_is_not_aborted() {
    let world = World::new().await;
    let launcher = StuckLauncher::default();
    // A run whose clone fails never reaches the launcher: nothing to abort.
    let mut spec = world.spec.clone();
    spec.repo_url = world.tmp.path().join("missing").display().to_string();
    let runner = Runner::new(
        config(&world.data_dir()),
        launcher.clone(),
        world.store(),
        None,
    );
    let (handle, mut rx) = runner.start(spec).expect("start");
    while rx.recv().await.is_some() {}

    assert_eq!(handle.join().await, StopCause::Failed { exit_code: None });
    assert!(!launcher.aborted.load(Ordering::SeqCst));
    world.end().await;
}

fn short_limits() -> PreludeLimits {
    PreludeLimits {
        update: Duration::from_millis(500),
        version: Duration::from_millis(500),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hung_update_is_given_up_and_the_run_goes_on() {
    let world = World::new().await;
    let launcher = replay("normal_run.jsonl").with_env("FAKE_UPDATE_HANGS", "1");
    let runner = Runner::new(config(&world.data_dir()), launcher, world.store(), None)
        .with_prelude_limits(short_limits());
    let (handle, mut rx) = runner.start(world.spec.clone()).expect("start");
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    let failed = position(
        &events,
        |e| matches!(e, RunEvent::CliUpdateFailed { message } if message.contains("did not end")),
    );
    let version = position(&events, |e| matches!(e, RunEvent::CliVersion { .. }));
    assert!(failed < version);
    assert_eq!(ended(&events), &StopCause::Completed);
    assert_eq!(handle.join().await, StopCause::Completed);
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hung_version_fails_the_run() {
    let world = World::new().await;
    let launcher = replay("normal_run.jsonl").with_env("FAKE_VERSION_HANGS", "1");
    let runner = Runner::new(config(&world.data_dir()), launcher, world.store(), None)
        .with_prelude_limits(short_limits());
    let (handle, mut rx) = runner.start(world.spec.clone()).expect("start");
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    assert!(
        !events
            .iter()
            .any(|e| matches!(e, RunEvent::CliVersion { .. }))
    );
    assert_eq!(ended(&events), &StopCause::Failed { exit_code: None });
    assert_eq!(handle.join().await, StopCause::Failed { exit_code: None });
    world.end().await;
}
