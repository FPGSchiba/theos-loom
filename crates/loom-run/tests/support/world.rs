//! One container test's world: a repository, a kit at `v1`, a data dir and a throwaway
//! database; runs started in it, and readers for their events.
#![cfg(all(test, target_os = "linux"))]

use loom_core::run::{RunEvent, RunId, StopCause};
use loom_run::config::{KitRef, RunConfig, RunSpec};
use loom_run::launcher::PodmanLauncher;
use loom_run::runner::{RunHandle, Runner};
use loom_store::PgRawStreamStore;
use loom_store::testing::TestDb;
use serde_json::Value;
use tokio::sync::mpsc;

use crate::podman::{Leftovers, podman_config};
use crate::support::{git, make_repo, spec};

/// One test's world.
pub struct World {
    pub tmp: tempfile::TempDir,
    pub db: TestDb,
    pub spec: RunSpec,
}

impl World {
    pub async fn new() -> Self {
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

    pub fn config(&self) -> RunConfig {
        podman_config(&self.tmp.path().join("data"))
    }

    /// Starts a run; the guard removes its containers and networks when the test ends.
    pub fn start(
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
    pub async fn run(
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

    pub async fn end(self) {
        self.db.drop_db().await.expect("drop the test database");
    }
}

/// The run's stop cause, from its last event.
pub fn ended(events: &[RunEvent]) -> &StopCause {
    match events.last() {
        Some(RunEvent::RunEnded { cause }) => cause,
        other => panic!("the last event is not RunEnded: {other:?}"),
    }
}

/// The fake's first line of type `kind` (`fake_report`, `fake_request`, ...), as JSON.
pub fn fake_line(events: &[RunEvent], kind: &str) -> Value {
    events
        .iter()
        .find_map(|event| match event {
            RunEvent::Unknown { raw } => serde_json::from_str::<Value>(raw)
                .ok()
                .filter(|value| value["type"] == kind),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {kind} line in {events:#?}"))
}
