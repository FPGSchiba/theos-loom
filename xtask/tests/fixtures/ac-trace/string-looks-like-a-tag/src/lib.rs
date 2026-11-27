//! A string whose lines look like a tag with an id that does not exist.

const S: &str = "
// AC: CE-99.9
";

// AC: CE-90.1
#[test]
fn proves_the_first() {
    assert!(!S.is_empty());
}

// AC: CE-90.2
#[test]
fn proves_the_second() {}
