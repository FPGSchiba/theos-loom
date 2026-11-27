//! Includes a `.rs` data file that is not Rust: rustc never parses it, the view does.

const DATA: &str = include_str!("data.rs");

// AC: CE-90.1
#[test]
fn proves_the_first() {
    assert!(!DATA.is_empty());
}
