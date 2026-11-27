//! Both criteria are tagged, but one more tag is above a function that is not a test.

// AC: TU-90.1
#[test]
fn proves_the_first() {}

// AC: TU-90.2
#[test]
fn proves_the_second() {}

// AC: TU-90.1
fn helper() {}
