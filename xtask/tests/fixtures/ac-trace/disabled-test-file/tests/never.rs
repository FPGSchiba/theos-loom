//! An integration test of a feature nobody enables.
#![cfg(feature = "never")]

// AC: TU-90.2
#[test]
fn would_prove_the_second() {}
