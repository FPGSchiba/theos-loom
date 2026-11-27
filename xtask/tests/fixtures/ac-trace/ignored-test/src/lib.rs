//! The only test of the second criterion never runs.

// AC: CE-90.1
#[test]
fn proves_the_first() {}

// AC: CE-90.2
#[test]
#[ignore = "x"]
fn would_prove_the_second() {}
