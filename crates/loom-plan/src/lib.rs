//! `loom-plan`: the planning repo reader and validation.

pub mod finding;
pub mod model;
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the planning repo reader of a later P01 task is the first caller"
    )
)]
pub(crate) mod parse;

pub use finding::{Finding, Rule, Severity};
