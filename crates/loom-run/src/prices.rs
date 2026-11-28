//! The price book: USD per token for each model, loaded as data (T14, D7).
//!
//! The list is `LiteLLM`'s public price list. The HTTP client is `ureq` with rustls on `ring` and
//! the platform's root certificates (`rustls-native-certs`): the other rustls roots crates are
//! not on the licence allowlist.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use loom_core::run::{PriceListStore, Usage};
use serde_json::Value;
use thiserror::Error;
use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};

const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Prices of one model, USD per token. A missing price means "not priced".
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ModelPrice {
    /// Input tokens.
    pub input: Option<f64>,
    /// Output tokens.
    pub output: Option<f64>,
    /// Cache writes with the 5-minute lifetime.
    pub cache_write_5m: Option<f64>,
    /// Cache writes with the 1-hour lifetime.
    pub cache_write_1h: Option<f64>,
    /// Cache reads.
    pub cache_read: Option<f64>,
}

impl ModelPrice {
    fn from_litellm(entry: &serde_json::Map<String, Value>) -> Self {
        let get = |key: &str| entry.get(key).and_then(Value::as_f64);
        Self {
            input: get("input_cost_per_token"),
            output: get("output_cost_per_token"),
            cache_write_5m: get("cache_creation_input_token_cost"),
            cache_write_1h: get("cache_creation_input_token_cost_above_1hr"),
            cache_read: get("cache_read_input_token_cost"),
        }
    }

    /// This price with every field the override sets replaced.
    fn overridden_by(self, over: &ModelPrice) -> Self {
        Self {
            input: over.input.or(self.input),
            output: over.output.or(self.output),
            cache_write_5m: over.cache_write_5m.or(self.cache_write_5m),
            cache_write_1h: over.cache_write_1h.or(self.cache_write_1h),
            cache_read: over.cache_read.or(self.cache_read),
        }
    }

    /// The cost of `usage`, or `None` when a kind with tokens has no price.
    fn cost(&self, usage: &Usage) -> Option<f64> {
        let kinds = [
            (usage.input, self.input),
            (usage.output, self.output),
            (usage.cache_write_5m, self.cache_write_5m),
            (usage.cache_write_1h, self.cache_write_1h),
            (usage.cache_read, self.cache_read),
        ];
        let mut sum = 0.0;
        for (tokens, price) in kinds {
            if tokens > 0 {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "token counts stay far below 2^52"
                )]
                let tokens = tokens as f64;
                sum += tokens * price?;
            }
        }
        Some(sum)
    }
}

/// A model had tokens of a kind that has no price.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("no price for model {model}")]
pub struct NoPrice {
    /// The model name.
    pub model: String,
}

/// Why a price book could not be built.
#[derive(Debug, Error)]
pub enum PriceError {
    /// The list is not JSON, or not a JSON object.
    #[error("the price list is not a JSON object: {0}")]
    Parse(String),
    /// Downloading the list failed.
    #[error("downloading the price list failed: {0}")]
    Fetch(String),
    /// The price-list store failed.
    #[error("the price-list store failed: {0}")]
    Store(String),
    /// The download failed and no earlier copy is saved.
    #[error("no price list: the download failed and no copy is saved")]
    NoPriceList,
}

/// Prices by model name, from a `LiteLLM` list with the configured overrides applied.
#[derive(Debug, Clone, Default)]
pub struct PriceBook {
    prices: BTreeMap<String, ModelPrice>,
}

impl PriceBook {
    /// Parses a `LiteLLM` price list. Entries that are not objects (`_comment`, ...) are skipped;
    /// an override replaces the fields it sets, and adds models the list lacks.
    ///
    /// # Errors
    /// Fails when `json` is not a JSON object.
    pub fn from_litellm(
        json: &str,
        overrides: &BTreeMap<String, ModelPrice>,
    ) -> Result<Self, PriceError> {
        let value: Value =
            serde_json::from_str(json).map_err(|e| PriceError::Parse(e.to_string()))?;
        let Value::Object(entries) = value else {
            return Err(PriceError::Parse("top level is not an object".into()));
        };
        let mut prices: BTreeMap<String, ModelPrice> = entries
            .iter()
            .filter_map(|(name, entry)| {
                Some((name.clone(), ModelPrice::from_litellm(entry.as_object()?)))
            })
            .collect();
        for (name, over) in overrides {
            let base = prices.get(name).copied().unwrap_or_default();
            prices.insert(name.clone(), base.overridden_by(over));
        }
        Ok(Self { prices })
    }

    /// The price of a model: its exact name, then `anthropic/<name>`.
    fn price(&self, model: &str) -> Option<&ModelPrice> {
        self.prices
            .get(model)
            .or_else(|| self.prices.get(&format!("anthropic/{model}")))
    }

    /// The spend in USD of the usage per model.
    ///
    /// # Errors
    /// [`NoPrice`] for the first model that is unknown or has tokens of a kind without a price.
    pub fn spend(&self, by_model: &BTreeMap<String, Usage>) -> Result<f64, NoPrice> {
        let mut total = 0.0;
        for (model, usage) in by_model {
            let cost = self
                .price(model)
                .and_then(|price| price.cost(usage))
                .ok_or_else(|| NoPrice {
                    model: model.clone(),
                })?;
            total += cost;
        }
        Ok(total)
    }
}

fn fetch_blocking(url: &str) -> Result<String, String> {
    let mut tls = TlsConfig::builder().provider(TlsProvider::Rustls);
    if url.starts_with("https://") {
        let loaded = rustls_native_certs::load_native_certs();
        for error in &loaded.errors {
            tracing::warn!(%error, "a platform root certificate could not be loaded");
        }
        let certs: Vec<Certificate<'static>> = loaded
            .certs
            .iter()
            .map(|der| Certificate::from_der(der.as_ref()).to_owned())
            .collect();
        tls = tls.root_certs(RootCerts::Specific(Arc::new(certs)));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls.build())
        .timeout_global(Some(FETCH_TIMEOUT))
        .build()
        .into();
    let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())
}

async fn fetch(url: &str) -> Result<String, PriceError> {
    let url = url.to_owned();
    tokio::task::spawn_blocking(move || fetch_blocking(&url))
        .await
        .map_err(|e| PriceError::Fetch(e.to_string()))?
        .map_err(PriceError::Fetch)
}

/// Downloads the price list. When it parses, it is saved and used; otherwise the last saved copy
/// is used and a warning is logged.
///
/// # Errors
/// [`PriceError::NoPriceList`] when the download fails and no copy is saved;
/// [`PriceError::Store`] when the store fails.
pub async fn refresh<S: PriceListStore>(
    url: &str,
    store: &S,
    overrides: &BTreeMap<String, ModelPrice>,
) -> Result<PriceBook, PriceError> {
    let fresh = match fetch(url).await {
        Ok(body) => PriceBook::from_litellm(&body, overrides).map(|book| (body, book)),
        Err(error) => Err(error),
    };
    match fresh {
        Ok((body, book)) => {
            store
                .save(url, &body)
                .await
                .map_err(|e| PriceError::Store(e.to_string()))?;
            Ok(book)
        }
        Err(error) => {
            tracing::warn!(%error, url, "price list refresh failed; using the last saved copy");
            let saved = store
                .latest()
                .await
                .map_err(|e| PriceError::Store(e.to_string()))?;
            let body = saved.ok_or(PriceError::NoPriceList)?;
            PriceBook::from_litellm(&body, overrides)
        }
    }
}
