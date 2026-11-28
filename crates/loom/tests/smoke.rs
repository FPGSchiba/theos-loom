//! The `loom` binary starts and exits cleanly.

// AC: TU-00.1
#[test]
fn the_binary_runs_and_exits_zero() -> Result<(), Box<dyn std::error::Error>> {
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_loom")).status()?;
    assert!(status.success(), "loom exited with {status}");
    Ok(())
}
