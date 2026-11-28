//! Token usage of a run.

use std::ops::Add;

use serde::{Deserialize, Serialize};

/// One kind of token a cap can apply to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenKind {
    /// Input tokens.
    Input,
    /// Output tokens.
    Output,
    /// Cache writes, of either lifetime.
    CacheWrite,
    /// Cache reads.
    CacheRead,
}

/// Token counts, as the CLI reports them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens.
    pub input: u64,
    /// Output tokens.
    pub output: u64,
    /// Cache-write tokens with the 5-minute lifetime.
    pub cache_write_5m: u64,
    /// Cache-write tokens with the 1-hour lifetime.
    pub cache_write_1h: u64,
    /// Cache-read tokens.
    pub cache_read: u64,
}

impl Usage {
    /// The count of one token kind; `CacheWrite` is both lifetimes together.
    #[must_use]
    pub fn get(&self, kind: TokenKind) -> u64 {
        match kind {
            TokenKind::Input => self.input,
            TokenKind::Output => self.output,
            TokenKind::CacheWrite => self.cache_write_5m.saturating_add(self.cache_write_1h),
            TokenKind::CacheRead => self.cache_read,
        }
    }
}

impl Add for Usage {
    type Output = Usage;

    fn add(self, rhs: Usage) -> Usage {
        Usage {
            input: self.input.saturating_add(rhs.input),
            output: self.output.saturating_add(rhs.output),
            cache_write_5m: self.cache_write_5m.saturating_add(rhs.cache_write_5m),
            cache_write_1h: self.cache_write_1h.saturating_add(rhs.cache_write_1h),
            cache_read: self.cache_read.saturating_add(rhs.cache_read),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_get_sums_both_cache_write_parts() {
        let usage = Usage {
            cache_write_5m: 3,
            cache_write_1h: 4,
            ..Usage::default()
        };
        assert_eq!(usage.get(TokenKind::CacheWrite), 7);
    }

    #[test]
    fn usage_adds_field_by_field() {
        let a = Usage {
            input: 1,
            output: 2,
            cache_write_5m: 3,
            cache_write_1h: 4,
            cache_read: 5,
        };
        let b = Usage {
            input: 10,
            output: 20,
            cache_write_5m: 30,
            cache_write_1h: 40,
            cache_read: 50,
        };
        let sum = a + b;
        assert_eq!(
            sum,
            Usage {
                input: 11,
                output: 22,
                cache_write_5m: 33,
                cache_write_1h: 44,
                cache_read: 55
            }
        );
        assert_eq!(sum.get(TokenKind::Input), 11);
        assert_eq!(sum.get(TokenKind::Output), 22);
        assert_eq!(sum.get(TokenKind::CacheRead), 55);
    }
}
