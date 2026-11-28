//! Short, stable directory names derived from URLs.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

/// The first 16 hex digits of the SHA-256 of `text`.
pub(crate) fn short_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    let mut out = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        // Writing to a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::short_hash;

    #[test]
    fn the_hash_is_16_hex_digits_and_stable() {
        let one = short_hash("https://example.com/a.git");
        assert_eq!(one.len(), 16);
        assert!(one.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(one, short_hash("https://example.com/a.git"));
        assert_ne!(one, short_hash("https://example.com/b.git"));
    }
}
