//! Configuration of the runtime and the description of one run (spec §5.2).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;
use thiserror::Error;

use crate::caps::Caps;
use crate::prices::ModelPrice;

const DEFAULT_PRICE_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";

/// The configuration could not be read.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// The text is not valid configuration: bad TOML, a missing key or an unknown key.
    #[error("invalid run configuration: {0}")]
    Invalid(#[from] toml::de::Error),
}

/// Configuration per Loom installation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    /// Mirrors, kit checkouts and run directories live under it.
    pub data_dir: PathBuf,
    /// Path of the Podman binary.
    #[serde(default = "default_podman")]
    pub podman: String,
    /// The worker image.
    pub image: String,
    /// The proxy (Squid) image, by digest.
    pub proxy_image: String,
    /// The network the proxy uses to reach the outside; created when missing.
    #[serde(default = "default_egress_network")]
    pub egress_network: String,
    /// Address ranges (CIDR text) to block on top of the fixed private ones.
    #[serde(default)]
    pub blocked_ranges: Vec<String>,
    /// Names of environment variables passed into the container; never their values.
    #[serde(default)]
    pub credentials: Vec<String>,
    /// The price list and its overrides.
    #[serde(default)]
    pub prices: PricesConfig,
    /// Keep the run directory after the run ends (R-j).
    #[serde(default)]
    pub keep_run_dirs: bool,
}

/// Where prices come from.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PricesConfig {
    /// URL of the public price list.
    #[serde(default = "default_price_url")]
    pub url: String,
    /// Prices that replace the list's, by model name.
    #[serde(default)]
    pub overrides: BTreeMap<String, ModelPrice>,
}

impl Default for PricesConfig {
    fn default() -> Self {
        Self {
            url: default_price_url(),
            overrides: BTreeMap::new(),
        }
    }
}

fn default_podman() -> String {
    "podman".to_owned()
}

fn default_egress_network() -> String {
    "loom-egress".to_owned()
}

fn default_price_url() -> String {
    DEFAULT_PRICE_URL.to_owned()
}

impl RunConfig {
    /// Reads the configuration from TOML.
    ///
    /// # Errors
    /// [`ConfigError::Invalid`] for bad TOML, a missing required key or an unknown key.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(text)?)
    }
}

/// A pinned version of the workflow kit.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KitRef {
    /// The kit's git repository.
    pub repository: String,
    /// The tag or branch to check out.
    pub version: String,
}

/// Everything one run needs to know.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSpec {
    /// The story's key, such as `ED-12`.
    pub story_key: String,
    /// The component repository.
    pub repo_url: String,
    /// The story branch; created from the default branch when it does not exist.
    pub story_branch: String,
    /// The repository's default branch.
    pub default_branch: String,
    /// The workflow kit.
    pub kit: KitRef,
    /// The prompt, given to the CLI on stdin.
    pub prompt: String,
    /// The model, or the CLI's default.
    #[serde(default)]
    pub model: Option<String>,
    /// Extra CLI arguments.
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// The caps of the run.
    #[serde(default)]
    pub caps: Caps,
}

#[cfg(test)]
mod tests {
    use super::RunConfig;

    const MINIMAL: &str = "data_dir = 'd'\nimage = 'i'\nproxy_image = 'p'\n";

    #[test]
    fn defaults_apply_to_a_minimal_config() {
        let config = RunConfig::from_toml(MINIMAL).expect("parses");
        assert_eq!(config.podman, "podman");
        assert_eq!(config.egress_network, "loom-egress");
        assert!(!config.keep_run_dirs);
        assert!(
            config
                .prices
                .url
                .contains("model_prices_and_context_window.json")
        );
    }

    #[test]
    fn prices_overrides_are_read() {
        let text = format!("{MINIMAL}[prices.overrides.\"my-model\"]\ninput = 0.5\n");
        let config = RunConfig::from_toml(&text).expect("parses");
        assert_eq!(config.prices.overrides["my-model"].input, Some(0.5));
    }

    #[test]
    fn an_unknown_config_key_is_an_error() {
        assert!(RunConfig::from_toml(&format!("{MINIMAL}colour = 'red'\n")).is_err());
        assert!(
            RunConfig::from_toml(&format!("{MINIMAL}[prices]\nurl = 'u'\nbogus = 1\n")).is_err()
        );
    }
}
