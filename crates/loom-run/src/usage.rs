//! Token usage of a run, summed per model.

use std::collections::BTreeMap;

use loom_core::run::Usage;

/// Token usage per message id; a message the CLI reports more than once counts once.
#[derive(Debug, Clone, Default)]
pub struct UsageLedger {
    messages: BTreeMap<String, (String, Usage)>,
}

impl UsageLedger {
    /// Records the usage of one message; the last value of a message id wins.
    pub fn record(&mut self, message_id: &str, model: &str, usage: Usage) {
        self.messages
            .insert(message_id.to_owned(), (model.to_owned(), usage));
    }

    /// The usage of all messages together.
    #[must_use]
    pub fn total(&self) -> Usage {
        self.messages
            .values()
            .fold(Usage::default(), |sum, (_, usage)| sum + *usage)
    }

    /// The usage of all messages, per model name.
    #[must_use]
    pub fn by_model(&self) -> BTreeMap<String, Usage> {
        let mut out: BTreeMap<String, Usage> = BTreeMap::new();
        for (model, usage) in self.messages.values() {
            let slot = out.entry(model.clone()).or_default();
            *slot = *slot + *usage;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, output: u64) -> Usage {
        Usage {
            input,
            output,
            ..Usage::default()
        }
    }

    #[test]
    fn a_repeated_message_counts_once() {
        let mut ledger = UsageLedger::default();
        ledger.record("m1", "model-a", usage(1, 1));
        ledger.record("m1", "model-a", usage(5, 7));
        assert_eq!(ledger.total(), usage(5, 7));
    }

    #[test]
    fn usage_sums_across_messages_and_models() {
        let mut ledger = UsageLedger::default();
        ledger.record("m1", "model-a", usage(1, 2));
        ledger.record("m2", "model-a", usage(10, 20));
        ledger.record("m3", "model-b", usage(100, 200));
        assert_eq!(ledger.total(), usage(111, 222));
        let by_model = ledger.by_model();
        assert_eq!(by_model["model-a"], usage(11, 22));
        assert_eq!(by_model["model-b"], usage(100, 200));
    }
}
