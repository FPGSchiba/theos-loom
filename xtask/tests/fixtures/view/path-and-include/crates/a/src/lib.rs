//! Crate a, which takes code from outside `src/`.

#[path = "../extra/m.rs"]
mod m;

include!("../extra/inc.rs");

const DATA: &str = include_str!("../data.txt");

/// Uses all three.
pub fn all() -> usize {
    m::M + INC + DATA.len()
}
