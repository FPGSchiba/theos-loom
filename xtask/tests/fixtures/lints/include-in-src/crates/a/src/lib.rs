//! Synthetic fixture: library code that includes a file.

include!("part.rs");

/// Data is not code.
pub const DATA: &str = include_str!("../data.txt");
