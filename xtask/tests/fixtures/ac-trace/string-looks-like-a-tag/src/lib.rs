//! A string whose lines look like a tag with an id that does not exist.

const S: &str = "
// AC: TU-99.9
";

// AC: TU-90.1
#[test]
fn proves_the_first() {
    assert!(!S.is_empty());
}

// AC: TU-90.2
#[test]
fn proves_the_second() {}
