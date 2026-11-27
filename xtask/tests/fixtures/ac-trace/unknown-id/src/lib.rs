//! Both criteria of P90 are tagged, and one more tag names a criterion that does not exist.

// AC: CE-90.1
#[test]
fn proves_the_first() {}

// AC: CE-90.2
#[test]
fn proves_the_second() {}

// AC: CE-99.9
#[test]
fn proves_something_that_does_not_exist() {}
