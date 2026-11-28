//! A crate whose attributes all say why.

#[expect(dead_code, reason = "the example keeps it for a later task")]
fn unused() {}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "needs the network"]
    fn slow() {}
}

#[cfg(not(test))]
pub fn not_a_test() {}

#[cfg(windows)]
pub fn on_windows() {}

#[cfg(target_os = "macos")]
pub fn on_macos() {}

#[cfg(all(unix, not(target_env = "musl")))]
pub fn on_unix() {}

#[cfg(feature = "extra")]
pub fn extra() {}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
pub struct Plain;

macro_rules! m {
    ($(#[$m:meta])* $v:vis fn $n:ident()) => {
        $(#[$m])* $v fn $n() {}
    };
}

m! { #[inline] fn h() }

macro_rules! p {
    ($p:meta) => {
        #[cfg($p)]
        fn k() {}
    };
}

p!(windows);
