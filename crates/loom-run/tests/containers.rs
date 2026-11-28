//! Runs in rootless Podman containers (TU-03.1, TU-03.3). Linux only (D2): on other systems this
//! file compiles to nothing.
#![cfg(target_os = "linux")]
// Test code throughout, helpers included, as in `support/mod.rs`.
#![cfg(test)]

#[path = "support/fake.rs"]
mod fake;
#[path = "support/podman.rs"]
mod podman;
mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use loom_core::run::{RunEvent, RunId, StopCause};
use loom_run::config::{KitRef, RunConfig, RunSpec};
use loom_run::launcher::{LaunchContext, LaunchError, Launcher, PodmanLauncher};
use loom_run::runner::{RunHandle, Runner};
use loom_store::PgRawStreamStore;
use loom_store::testing::TestDb;
use podman::{Leftovers, containers, image_id, launcher, networks, podman_config};
use serde_json::Value;
use support::{git, make_repo, spec};
use tokio::sync::mpsc;

/// Long enough for a first pull of the test image.
const FIRST_EVENT: Duration = Duration::from_secs(300);

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
        let spec = spec(&repo, "story/ED-12", kit);
        let db = TestDb::create().await.expect("test database");
        Self { tmp, db, spec }
    }

    fn config(&self) -> RunConfig {
        podman_config(&self.tmp.path().join("data"))
    }

    /// Starts a run; the guard removes its containers and networks when the test ends.
    fn start(
        &self,
        config: RunConfig,
        launcher: PodmanLauncher,
    ) -> (RunHandle, mpsc::Receiver<RunEvent>, Leftovers) {
        let store = PgRawStreamStore::new(self.db.pool().clone());
        let runner = Runner::new(config, launcher, store, None);
        let (handle, rx) = runner.start(self.spec.clone()).expect("start");
        let guard = Leftovers(handle.id);
        (handle, rx, guard)
    }

    /// Runs to the end; returns the run's id, every event and the guard, which the test keeps
    /// until it has checked that nothing is left.
    async fn run(
        &self,
        config: RunConfig,
        launcher: PodmanLauncher,
    ) -> (RunId, Vec<RunEvent>, Leftovers) {
        let (handle, mut rx, guard) = self.start(config, launcher);
        let id = handle.id;
        let mut events = Vec::new();
        while let Some(event) = rx.recv().await {
            events.push(event);
        }
        handle.join().await;
        (id, events, guard)
    }

    async fn end(self) {
        self.db.drop_db().await.expect("drop the test database");
    }
}

fn ended(events: &[RunEvent]) -> &StopCause {
    match events.last() {
        Some(RunEvent::RunEnded { cause }) => cause,
        other => panic!("the last event is not RunEnded: {other:?}"),
    }
}

/// The fake's `report` line.
fn report(events: &[RunEvent]) -> Value {
    events
        .iter()
        .find_map(|event| match event {
            RunEvent::Unknown { raw } if raw.contains("fake_report") => {
                Some(serde_json::from_str(raw).expect("the report is JSON"))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no report in {events:#?}"))
}

/// Writes an executable shell script.
fn script(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write the script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

fn assert_nothing_left(run: RunId) {
    assert_eq!(containers(run), Vec::<String>::new(), "containers left");
    assert_eq!(networks(run), Vec::<String>::new(), "networks left");
}

// AC: TU-03.1
#[tokio::test(flavor = "multi_thread")]
async fn a_run_starts_in_a_rootless_container_with_its_clone_and_kit() {
    let world = World::new().await;
    let config = world.config();
    let (id, events, _guard) = world.run(config.clone(), launcher(&config, "report")).await;

    assert_eq!(ended(&events), &StopCause::Completed, "{events:#?}");
    let report = report(&events);
    let uid = report["uid"].as_u64().expect("the uid is known");
    assert_ne!(uid, 0, "the CLI runs as root");
    assert_eq!(report["cwd"], "/work");
    assert_eq!(report["branch"], "story/ED-12");
    assert_eq!(report["kit_version"], "1.2.3");
    let image = events.iter().find_map(|event| match event {
        RunEvent::Image { id } => Some(id.clone()),
        _ => None,
    });
    assert_eq!(image, Some(image_id(&config.image)));
    assert_nothing_left(id);
    world.end().await;
}

// AC: TU-03.1
#[tokio::test(flavor = "multi_thread")]
async fn two_concurrent_runs_of_one_story_never_share_a_worktree() {
    let world = World::new().await;
    let config = world.config();
    let marking = || launcher(&config, "report").with_env("FAKE_MARK", "1");
    let ((one, first, _g1), (two, second, _g2)) = tokio::join!(
        world.run(config.clone(), marking()),
        world.run(config.clone(), marking()),
    );

    assert_ne!(one, two);
    assert_ne!(
        loom_run::workspace::run_dir(&config, one),
        loom_run::workspace::run_dir(&config, two)
    );
    let mut own = Vec::new();
    for events in [&first, &second] {
        assert_eq!(ended(events), &StopCause::Completed, "{events:#?}");
        let report = report(events);
        assert_eq!(report["cwd"], "/work");
        let mark = report["mark"].as_str().expect("a mark").to_owned();
        let marks: Vec<&str> = report["files"]
            .as_array()
            .expect("files")
            .iter()
            .filter_map(Value::as_str)
            .filter(|name| name.starts_with(".fake-mark-"))
            .collect();
        // Each run sees the file it wrote in /work, and never the other's.
        assert_eq!(marks, vec![mark.as_str()]);
        own.push(mark);
    }
    assert_ne!(own[0], own[1]);
    assert_nothing_left(one);
    assert_nothing_left(two);
    world.end().await;
}

// AC: TU-03.1
#[tokio::test(flavor = "multi_thread")]
async fn a_podman_that_is_not_rootless_is_refused() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let log = tmp.path().join("calls.log");
    let stand_in = tmp.path().join("podman");
    script(
        &stand_in,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in info) echo false ;; esac\n",
            log.display()
        ),
    );
    let mut config = podman_config(&tmp.path().join("data"));
    config.podman = stand_in.display().to_string();
    let ctx = LaunchContext {
        run: RunId::new(),
        workspace: tmp.path().to_owned(),
        kit: tmp.path().to_owned(),
        env: Vec::new(),
    };

    let result = PodmanLauncher::new(&config).start(&ctx).await;

    assert!(
        matches!(result, Err(LaunchError::NotRootless)),
        "{:?}",
        result.map(|_| "a session")
    );
    let calls = std::fs::read_to_string(&log).expect("the stand-in ran");
    assert_eq!(
        calls.lines().collect::<Vec<_>>(),
        vec!["info --format {{.Host.Security.Rootless}}"],
        "nothing but the check ran"
    );
}

// AC: TU-03.3
#[tokio::test(flavor = "multi_thread")]
async fn cancelling_stops_the_process_tree_and_the_container_within_10_s() {
    let world = World::new().await;
    let config = world.config();
    let (handle, mut rx, _guard) = world.start(config.clone(), launcher(&config, "spawn"));
    let id = handle.id;
    let mut events = Vec::new();
    // Wait until the fake runs in the container and has started its children.
    loop {
        let event = tokio::time::timeout(FIRST_EVENT, rx.recv())
            .await
            .expect("the fake starts in time")
            .unwrap_or_else(|| panic!("the run ended early: {events:#?}"));
        let spawned = matches!(&event, RunEvent::Unknown { raw } if raw.contains("fake_spawn"));
        events.push(event);
        if spawned {
            break;
        }
    }
    assert_eq!(containers(id).len(), 1, "the container runs");

    let cancelled = Instant::now();
    handle.cancel();
    while let Some(event) = rx.recv().await {
        events.push(event);
    }
    let elapsed = cancelled.elapsed();

    assert_eq!(ended(&events), &StopCause::Cancelled);
    assert!(elapsed < Duration::from_secs(10), "took {elapsed:?}");
    assert_nothing_left(id);
    handle.join().await;
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_pass_by_name_never_by_value() {
    let world = World::new().await;
    let secret = format!("s3cr3t-{}", RunId::new());
    let log: PathBuf = world.tmp.path().join("podman-calls.log");
    let wrapper = world.tmp.path().join("podman-wrapper");
    // The wrapper is Loom's environment: it holds the secret and logs every podman call.
    script(
        &wrapper,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nLOOM_TEST_SECRET='{secret}' exec podman \"$@\"\n",
            log.display()
        ),
    );
    let mut config = world.config();
    config.podman = wrapper.display().to_string();
    config.credentials = vec!["LOOM_TEST_SECRET".to_owned()];
    let (_, events, _guard) = world.run(config.clone(), launcher(&config, "report")).await;

    assert_eq!(ended(&events), &StopCause::Completed, "{events:#?}");
    assert_eq!(report(&events)["env"]["LOOM_TEST_SECRET"], secret.as_str());
    let calls = std::fs::read_to_string(&log).expect("the wrapper ran");
    assert!(calls.contains("--env LOOM_TEST_SECRET "), "{calls}");
    assert!(
        !calls.contains("s3cr3t-"),
        "a value on a command line: {calls}"
    );
    for event in &events {
        let text = format!("{event:?}");
        if !text.contains("fake_report") {
            assert!(!text.contains("s3cr3t-"), "a value in an event: {text}");
        }
    }
    world.end().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_start_leaves_nothing_behind() {
    let world = World::new().await;
    let mut config = world.config();
    config.image = "localhost/loom-test/no-such-image:missing".to_owned();
    let (id, events, _guard) = world.run(config.clone(), launcher(&config, "report")).await;

    assert!(
        matches!(ended(&events), StopCause::Failed { .. }),
        "{events:#?}"
    );
    assert_nothing_left(id);
    world.end().await;
}
