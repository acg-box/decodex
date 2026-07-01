use reqwest::Url;
use serde::Deserialize;

use crate::prelude::{Result, eyre};

/// Optional local-only classifier for public Linear projection text.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectPrivacyClassifierConfig {
	endpoint: Option<String>,
	#[serde(default = "default_privacy_classifier_timeout_ms")]
	timeout_ms: u64,
}
impl ProjectPrivacyClassifierConfig {
	/// Loopback HTTP endpoint for an operator-managed local classifier runtime.
	pub fn endpoint(&self) -> Option<&str> {
		self.endpoint.as_deref()
	}

	/// Per-field local classifier request timeout.
	pub fn timeout_ms(&self) -> u64 {
		self.timeout_ms
	}

	pub(super) fn validate(&self) -> Result<()> {
		if self.timeout_ms == 0 {
			eyre::bail!("`privacy_classifier.timeout_ms` must be greater than zero.");
		}
		if self.timeout_ms > 30_000 {
			eyre::bail!("`privacy_classifier.timeout_ms` must be 30000 or less.");
		}

		if let Some(endpoint) = self.endpoint.as_deref() {
			validate_local_privacy_classifier_endpoint(endpoint)?;
		}

		Ok(())
	}
}

impl Default for ProjectPrivacyClassifierConfig {
	fn default() -> Self {
		Self { endpoint: None, timeout_ms: default_privacy_classifier_timeout_ms() }
	}
}

fn default_privacy_classifier_timeout_ms() -> u64 {
	1_000
}

fn validate_local_privacy_classifier_endpoint(endpoint: &str) -> Result<()> {
	let url = Url::parse(endpoint)
		.map_err(|error| eyre::eyre!("`privacy_classifier.endpoint` must be a URL: {error}"))?;

	if url.scheme() != "http" {
		eyre::bail!("`privacy_classifier.endpoint` must use `http` on a loopback host.");
	}
	if !url.username().is_empty() || url.password().is_some() {
		eyre::bail!("`privacy_classifier.endpoint` must not contain credentials.");
	}

	let Some(host) = url.host_str() else {
		eyre::bail!("`privacy_classifier.endpoint` must include a loopback host.");
	};

	if !matches!(host, "localhost" | "127.0.0.1" | "::1") {
		eyre::bail!("`privacy_classifier.endpoint` must point to a loopback host, not `{host}`.");
	}

	Ok(())
}
