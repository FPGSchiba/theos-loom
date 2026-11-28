//! `loom-core`: the domain and its state machines.
//!
//! So far it holds the [`run`] module: the typed events, stop causes and usage of a Claude Code
//! run, and the storage traits the run runtime writes through.

pub mod run;
