//! Both criteria are tagged, but one more tag is above a function that is not a test.

// AC: CE-90.1
#[test]
fn proves_the_first() {}

// AC: CE-90.2
#[test]
fn proves_the_second() {}

// AC: CE-90.1
fn helper() {}
