//! Podman for the container tests: the pinned test image, a launcher that runs the fake CLI, and
//! a guard that removes everything labelled with a run's id, also when the test fails.
#![cfg(all(test, target_os = "linux"))]

use std::path::Path;
use std::process::Command;

use loom_core::run::RunId;
use loom_run::config::RunConfig;
use loom_run::launcher::PodmanLauncher;

/// The test image, pinned by the digest of its multi-arch index (R-m, plan ruling: a glibc
/// image runs the fake built on the test machine).
pub const TEST_IMAGE: &str = "docker.io/library/debian:trixie-slim@sha256:8aa5a7bf5a1955bddbe7d74fcdd0b91ab30e74362a8b73af672b0481b35b44a0";

/// The label the launcher puts on everything of `run`, as a `--filter` value.
pub fn label(run: RunId) -> String {
    format!("label=loom.run={run}")
}

/// A config whose data dir is `data_dir` and whose image is [`TEST_IMAGE`].
pub fn podman_config(data_dir: &Path) -> RunConfig {
    let mut config = crate::support::config(data_dir);
    config.image = TEST_IMAGE.to_owned();
    config
}

/// A launcher that mounts the fake CLI as `claude` and runs it in `mode`.
pub fn launcher(config: &RunConfig, mode: &str) -> PodmanLauncher {
    let fake = crate::fake::fake_claude();
    PodmanLauncher::new(config)
        .with_volume(format!("{}:/usr/local/bin/claude:ro", fake.display()))
        .with_env("FAKE_MODE", mode)
}

/// The lines `podman <args>` prints, or `None` when it fails.
fn podman_lines(args: &[&str]) -> Option<Vec<String>> {
    let out = Command::new("podman").args(args).output().ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    })
}

/// The containers of `run`, running or not.
pub fn containers(run: RunId) -> Vec<String> {
    podman_lines(&["ps", "-a", "-q", "--filter", &label(run)]).expect("podman ps runs")
}

/// The networks of `run`.
pub fn networks(run: RunId) -> Vec<String> {
    podman_lines(&["network", "ls", "-q", "--filter", &label(run)]).expect("podman network ls runs")
}

/// Podman's id of `image`.
pub fn image_id(image: &str) -> String {
    podman_lines(&["image", "inspect", "--format", "{{.Id}}", image])
        .and_then(|lines| lines.into_iter().next())
        .expect("podman image inspect runs")
}

/// Removes every container and network of its run when dropped; never panics.
pub struct Leftovers(pub RunId);

impl Drop for Leftovers {
    fn drop(&mut self) {
        let filter = label(self.0);
        for id in podman_lines(&["ps", "-a", "-q", "--filter", &filter]).unwrap_or_default() {
            let _ = Command::new("podman")
                .args(["rm", "--force", "--time", "0", &id])
                .output();
        }
        for name in podman_lines(&["network", "ls", "-q", "--filter", &filter]).unwrap_or_default()
        {
            let _ = Command::new("podman")
                .args(["network", "rm", "--force", &name])
                .output();
        }
    }
}
