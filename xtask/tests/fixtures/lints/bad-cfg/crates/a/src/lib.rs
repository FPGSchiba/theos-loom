//! Synthetic fixture: cfg names that the gate refuses, one on each odd line from 3.

#[cfg(not(clippy))]
pub fn a() {}
#[cfg(debug_assertions)]
pub fn b() {}
#[cfg(miri)]
pub fn c() {}
#[cfg(coverage)]
pub fn d() {}
#[cfg(docsrs)]
pub fn e() {}
#[cfg(target_has_atomic_equal_alignment = "8")]
pub fn f() {}
#[cfg(false)]
pub fn g() {}
#[cfg_attr(not(clippy), inline)]
pub fn h() {}
macro_rules! m { () => { #[cfg(not(clippy))] fn g() {} } }
