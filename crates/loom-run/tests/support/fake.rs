//! Finds the fake Claude CLI (`examples/fake_claude.rs`). Cargo builds a package's examples when
//! it builds its tests, so the fake sits in `examples/` beside the test binary's `deps/`.
#![cfg(test)]

use std::path::PathBuf;

/// The path of the built fake CLI. Panics with a hint when it is not built.
pub fn fake_claude() -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary's path");
    let profile = exe
        .parent()
        .and_then(|deps| deps.parent())
        .expect("the test binary lives in <target>/<profile>/deps");
    let name = format!("fake_claude{}", std::env::consts::EXE_SUFFIX);
    let path = profile.join("examples").join(name);
    assert!(
        path.is_file(),
        "the fake CLI is not built at {}; run the tests with `cargo test` or `cargo nextest run` \
         (both build the examples) or build it with `cargo build -p loom-run --example fake_claude`",
        path.display()
    );
    path
}
