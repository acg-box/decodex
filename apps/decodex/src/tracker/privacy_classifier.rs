use std::time::Duration;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::{config::ProjectPrivacyClassifierConfig, prelude::Result};

pub(crate) static DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER:
	DisabledPublicProjectionPrivacyClassifier = DisabledPublicProjectionPrivacyClassifier;

/// Local-only classifier boundary for text already selected for public Linear projection.
pub(crate) trait PublicProjectionPrivacyClassifier {
	/// Classify one public projection field.
	fn classify_public_projection_text(
		&self,
		field_name: &str,
		text: &str,
	) -> PublicProjectionPrivacyClassification;
}

/// Classification verdict for one public Linear projection text field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PublicProjectionPrivacyClassification {
	/// The text is safe to publish.
	Allow,
	/// The classifier identified possible private information.
	Suspicious { reason: String },
	/// The configured classifier could not return a trustworthy answer.
	Unavailable { reason: String },
}

/// Runtime-selected local classifier implementation.
pub(crate) enum ConfiguredPublicProjectionPrivacyClassifier {
	/// No classifier is configured.
	Disabled,
	/// Loopback HTTP adapter for an operator-managed local classifier runtime.
	LocalHttp(LocalHttpPublicProjectionPrivacyClassifier),
}
impl ConfiguredPublicProjectionPrivacyClassifier {
	/// Build the runtime classifier selected by project config.
	pub(crate) fn from_config(config: &ProjectPrivacyClassifierConfig) -> Result<Self> {
		let Some(endpoint) = config.endpoint() else {
			return Ok(Self::Disabled);
		};

		Ok(Self::LocalHttp(LocalHttpPublicProjectionPrivacyClassifier::new(
			endpoint,
			config.timeout_ms(),
		)?))
	}
}

impl PublicProjectionPrivacyClassifier for ConfiguredPublicProjectionPrivacyClassifier {
	fn classify_public_projection_text(
		&self,
		field_name: &str,
		text: &str,
	) -> PublicProjectionPrivacyClassification {
		match self {
			Self::Disabled => DISABLED_PUBLIC_PROJECTION_PRIVACY_CLASSIFIER
				.classify_public_projection_text(field_name, text),
			Self::LocalHttp(classifier) => {
				classifier.classify_public_projection_text(field_name, text)
			},
		}
	}
}

/// Default classifier used when no local runtime is configured.
pub(crate) struct DisabledPublicProjectionPrivacyClassifier;
impl PublicProjectionPrivacyClassifier for DisabledPublicProjectionPrivacyClassifier {
	fn classify_public_projection_text(
		&self,
		_field_name: &str,
		_text: &str,
	) -> PublicProjectionPrivacyClassification {
		PublicProjectionPrivacyClassification::Allow
	}
}

/// Loopback HTTP adapter for an operator-managed local classifier runtime.
pub(crate) struct LocalHttpPublicProjectionPrivacyClassifier {
	client: Client,
	endpoint: String,
}
impl LocalHttpPublicProjectionPrivacyClassifier {
	fn new(endpoint: &str, timeout_ms: u64) -> Result<Self> {
		let client = Client::builder().timeout(Duration::from_millis(timeout_ms)).build()?;

		Ok(Self { client, endpoint: endpoint.to_owned() })
	}
}

impl PublicProjectionPrivacyClassifier for LocalHttpPublicProjectionPrivacyClassifier {
	fn classify_public_projection_text(
		&self,
		field_name: &str,
		text: &str,
	) -> PublicProjectionPrivacyClassification {
		let request = LocalClassifierRequest { field_name, text };
		let response = match self.client.post(&self.endpoint).json(&request).send() {
			Ok(response) => response,
			Err(error) => {
				return PublicProjectionPrivacyClassification::Unavailable {
					reason: format!("local classifier request failed: {error}"),
				};
			},
		};

		if !response.status().is_success() {
			return PublicProjectionPrivacyClassification::Unavailable {
				reason: format!("local classifier returned HTTP {}", response.status()),
			};
		}

		let response = match response.json::<LocalClassifierResponse>() {
			Ok(response) => response,
			Err(error) => {
				return PublicProjectionPrivacyClassification::Unavailable {
					reason: format!("local classifier response was invalid: {error}"),
				};
			},
		};

		response.into_classification()
	}
}

#[derive(Serialize)]
struct LocalClassifierRequest<'a> {
	field_name: &'a str,
	text: &'a str,
}

#[derive(Deserialize)]
struct LocalClassifierResponse {
	verdict: String,
	reason: Option<String>,
}
impl LocalClassifierResponse {
	fn into_classification(self) -> PublicProjectionPrivacyClassification {
		let reason = || {
			self.reason
				.clone()
				.filter(|reason| !reason.trim().is_empty())
				.unwrap_or_else(|| String::from("local classifier did not provide a reason"))
		};

		match self.verdict.trim().to_ascii_lowercase().as_str() {
			"allow" => PublicProjectionPrivacyClassification::Allow,
			"suspicious" | "block" | "blocked" => {
				PublicProjectionPrivacyClassification::Suspicious { reason: reason() }
			},
			"unavailable" => {
				PublicProjectionPrivacyClassification::Unavailable { reason: reason() }
			},
			other => PublicProjectionPrivacyClassification::Unavailable {
				reason: format!("local classifier returned unsupported verdict `{other}`"),
			},
		}
	}
}
