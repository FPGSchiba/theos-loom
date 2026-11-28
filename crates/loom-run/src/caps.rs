//! Caps of a run: one per token kind, spend and wall time (D6).

use std::time::Duration;

use loom_core::run::{CapKind, StopCause, TokenKind};
use serde::{Deserialize, Deserializer};

use crate::prices::PriceBook;
use crate::usage::UsageLedger;

/// The caps of a run. A cap that is not set never stops the run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caps {
    /// Most input tokens.
    #[serde(default)]
    pub input_tokens: Option<u64>,
    /// Most output tokens.
    #[serde(default)]
    pub output_tokens: Option<u64>,
    /// Most cache-write tokens, both lifetimes together.
    #[serde(default)]
    pub cache_write_tokens: Option<u64>,
    /// Most cache-read tokens.
    #[serde(default)]
    pub cache_read_tokens: Option<u64>,
    /// Most spend in US dollars.
    #[serde(default)]
    pub spend_usd: Option<f64>,
    /// Longest wall time; in config it is the integer `wall_time_secs`.
    #[serde(
        default,
        rename = "wall_time_secs",
        deserialize_with = "seconds_option"
    )]
    pub wall_time: Option<Duration>,
}

fn seconds_option<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
    Ok(Option::<u64>::deserialize(d)?.map(Duration::from_secs))
}

impl Caps {
    /// The first cap the run has crossed, or `None`. Order: wall time, the four token kinds
    /// (input, output, cache write, cache read), then spend. A token or time cap is crossed when
    /// the value is greater than the cap. Spend is computed only when `spend_usd` is set; then a
    /// missing price (or no price book) is [`StopCause::NoPrice`] (R-g).
    #[must_use]
    pub fn check(
        &self,
        ledger: &UsageLedger,
        prices: Option<&PriceBook>,
        elapsed: Duration,
    ) -> Option<StopCause> {
        if self.wall_time.is_some_and(|cap| elapsed > cap) {
            return Some(StopCause::Cap(CapKind::WallTime));
        }
        let total = ledger.total();
        let token_caps = [
            (TokenKind::Input, self.input_tokens),
            (TokenKind::Output, self.output_tokens),
            (TokenKind::CacheWrite, self.cache_write_tokens),
            (TokenKind::CacheRead, self.cache_read_tokens),
        ];
        for (kind, cap) in token_caps {
            if cap.is_some_and(|cap| total.get(kind) > cap) {
                return Some(StopCause::Cap(CapKind::Tokens(kind)));
            }
        }
        let cap = self.spend_usd?;
        let by_model = ledger.by_model();
        let Some(prices) = prices else {
            return by_model.keys().next().map(|model| StopCause::NoPrice {
                model: model.clone(),
            });
        };
        match prices.spend(&by_model) {
            Ok(spend) if spend > cap => Some(StopCause::Cap(CapKind::Spend)),
            Ok(_) => None,
            Err(no_price) => Some(StopCause::NoPrice {
                model: no_price.model,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use loom_core::run::Usage;

    use super::*;

    fn ledger(usage: Usage) -> UsageLedger {
        let mut ledger = UsageLedger::default();
        ledger.record("m1", "model-a", usage);
        ledger
    }

    fn book() -> PriceBook {
        let json =
            r#"{"model-a": {"input_cost_per_token": 0.001, "output_cost_per_token": 0.002}}"#;
        PriceBook::from_litellm(json, &BTreeMap::new()).unwrap()
    }

    const NOW: Duration = Duration::from_secs(1);

    // AC: TU-03.4
    #[test]
    fn each_token_kind_has_its_own_cap() {
        let caps = Caps {
            input_tokens: Some(10),
            output_tokens: Some(10),
            cache_write_tokens: Some(10),
            cache_read_tokens: Some(10),
            ..Caps::default()
        };
        let at = Usage {
            input: 10,
            output: 10,
            cache_write_5m: 4,
            cache_write_1h: 6,
            cache_read: 10,
        };
        assert_eq!(caps.check(&ledger(at), None, NOW), None);
        let cases = [
            (Usage { input: 11, ..at }, TokenKind::Input),
            (Usage { output: 11, ..at }, TokenKind::Output),
            (
                Usage {
                    cache_write_1h: 7,
                    ..at
                },
                TokenKind::CacheWrite,
            ),
            (
                Usage {
                    cache_read: 11,
                    ..at
                },
                TokenKind::CacheRead,
            ),
        ];
        for (usage, kind) in cases {
            assert_eq!(
                caps.check(&ledger(usage), None, NOW),
                Some(StopCause::Cap(CapKind::Tokens(kind)))
            );
        }
    }

    // AC: TU-03.4
    #[test]
    fn spend_over_its_cap_stops() {
        let caps = Caps {
            spend_usd: Some(0.5),
            ..Caps::default()
        };
        let prices = book();
        let under = Usage {
            input: 400,
            ..Usage::default()
        };
        let over = Usage {
            input: 600,
            ..Usage::default()
        };
        assert_eq!(caps.check(&ledger(under), Some(&prices), NOW), None);
        assert_eq!(
            caps.check(&ledger(over), Some(&prices), NOW),
            Some(StopCause::Cap(CapKind::Spend))
        );
        let unknown = ledger(under);
        let none = caps.check(&unknown, None, NOW);
        assert_eq!(
            none,
            Some(StopCause::NoPrice {
                model: "model-a".into()
            })
        );
    }

    #[test]
    fn no_spend_cap_needs_no_prices() {
        let caps = Caps {
            input_tokens: Some(1_000),
            ..Caps::default()
        };
        let mut l = UsageLedger::default();
        l.record(
            "m",
            "unknown-model",
            Usage {
                input: 5,
                ..Usage::default()
            },
        );
        assert_eq!(caps.check(&l, None, NOW), None);
        assert_eq!(caps.check(&l, Some(&book()), NOW), None);
        let with_spend = Caps {
            spend_usd: Some(1.0),
            ..caps
        };
        assert_eq!(
            with_spend.check(&l, Some(&book()), NOW),
            Some(StopCause::NoPrice {
                model: "unknown-model".into()
            })
        );
    }

    // AC: TU-03.4
    #[test]
    fn wall_time_over_its_cap_stops() {
        let caps = Caps {
            wall_time: Some(Duration::from_secs(60)),
            ..Caps::default()
        };
        let l = UsageLedger::default();
        assert_eq!(caps.check(&l, None, Duration::from_secs(60)), None);
        assert_eq!(
            caps.check(&l, None, Duration::from_millis(60_001)),
            Some(StopCause::Cap(CapKind::WallTime))
        );
    }

    #[test]
    fn no_caps_never_stop() {
        let l = ledger(Usage {
            input: u64::MAX,
            ..Usage::default()
        });
        assert_eq!(Caps::default().check(&l, None, Duration::MAX), None);
    }

    #[test]
    fn caps_read_from_toml() {
        let caps: Caps = toml::from_str("output_tokens = 5\nwall_time_secs = 90\n").unwrap();
        assert_eq!(caps.output_tokens, Some(5));
        assert_eq!(caps.wall_time, Some(Duration::from_secs(90)));
        assert_eq!(caps.input_tokens, None);
    }
}
