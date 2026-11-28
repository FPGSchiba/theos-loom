//! Synthetic fixture: attributes that rule 3 refuses, one finding on each of lines 4, 6, 9, 12,
//! 15, 16, 20, 22, 24, 35 and 38. The attributes of lines 28 and 32 pass.

#![allow(missing_docs)]

#[allow(dead_code)]
fn silenced() {}

#[cfg_attr(test, allow(unused))]
fn cfg_attr_allow() {}

mod m { #![allow(unused, reason = "in one line") ] }

macro_rules! q { () => {
    #[expect(dead_code)] fn a() {}
    #[test] #[ignore] fn b() {}
} }

macro_rules! fwd { ($(#[$m:meta])* fn $n:ident()) => { $(#[$m])* fn $n() {} } }
fwd! { #[allow(dead_code)] fn c() }

#[expect(clippy::ignore_without_reason, reason = "try")]
#[test]
#[ignore]
fn guarded() {}

// These two pass.
#[expect(dead_code, reason = "kept")]
fn kept() {}

#[test]
#[ignore = "slow"]
fn slow() {}

#[warn(clippy::unwrap_used)]
fn lowered() {}

#[cfg_attr(test, warn(unused))]
fn lowered_in_cfg_attr() {}
