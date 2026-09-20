//! Exact service-tier identity shared by client, runtime, and provider boundaries.
use serde::{Deserialize, Deserializer, Serialize};

/// A bounded provider tier identifier. Construction does not prove account availability.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ServiceTier(String);

/// A service-tier identifier is malformed. Rejected input is never included.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidServiceTier;
impl std::fmt::Display for InvalidServiceTier {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("invalid service tier")
	}
}
impl std::error::Error for InvalidServiceTier {}

impl ServiceTier {
	/// Validate an exact identifier without mapping unknown tiers to a known tier.
	pub fn new(value: impl Into<String>) -> Result<Self, InvalidServiceTier> {
		let value = value.into();
		if value.is_empty()
			|| value.len() > 64
			|| !value.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
		{
			return Err(InvalidServiceTier);
		}
		Ok(Self(value))
	}

	/// Explicit standard speed, distinct from inheriting an existing thread setting.
	pub fn standard() -> Self {
		Self("default".into())
	}

	/// Preserve the meaning of older Fast controls and durable messages.
	pub fn from_fast(fast: bool) -> Self {
		Self(if fast { "priority" } else { "default" }.into())
	}

	/// Exact native service-tier request value.
	pub fn as_str(&self) -> &str {
		&self.0
	}

	/// The legacy thread setting uses null to clear a previous explicit tier.
	pub fn thread_value(&self) -> Option<&str> {
		(self.as_str() != "default").then_some(self.as_str())
	}
}

impl<'de> Deserialize<'de> for ServiceTier {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
	}
}
