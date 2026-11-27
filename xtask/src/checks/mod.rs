//! The checks of `docs/gate.md` §1, one module each (or one per family).
//!
//! A check module has a pure core, inputs in and a [`Verdict`](crate::verdict::Verdict) out, and a
//! `run` function that gathers the real inputs. `main` and the gate call `run`.

pub mod ac_trace;
pub mod approval;
pub mod cargo_cmds;
pub mod commit_msg;
pub mod coverage;
pub mod engine_free;
pub mod lints;
pub mod pr_body;
pub mod pr_title;
pub mod slow;
