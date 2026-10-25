//! Safe native startup requirements and explicit diagnostic source identity.
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Native sign-in methods reported by the current startup requirements snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeStartupLoginMethod {
	/// ChatGPT sign-in.
	Chatgpt,
	/// API key authentication.
	Api,
}

/// Safe configuration location. Native error messages and source excerpts are not retained.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeStartupConfigLocation {
	/// Absolute configuration path, with credential-like or control text excluded.
	pub file: String,
	/// One-based native line, when available.
	pub line: Option<u32>,
	/// One-based native column, when available.
	pub column: Option<u32>,
}

/// Result of one native startup configuration check, separate from service and account health.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeStartupConfigStatus {
	/// Native configuration loaded and returned a requirements snapshot.
	Loaded {
		/// Version returned by the exact selected native executable.
		version: String,
		/// Allowed sign-in methods; absent means native did not report them.
		login_methods: Option<Vec<NativeStartupLoginMethod>>,
		/// Native returned a non-null requirements object.
		requirements_present: bool,
	},
	/// Native rejected local configuration or could not load cloud policy.
	Rejected {
		/// Available safe configuration location.
		location: Option<NativeStartupConfigLocation>,
		/// Native reported a cloud configuration bundle failure.
		cloud_policy: bool,
	},
	/// The component, folder, bounded command or response could not be checked.
	Unavailable,
}

/// A sanitized snapshot from the shared native sign-in, not a selected task account override.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeStartupConfiguration {
	/// Exact checked directory, when it is safe to display.
	pub directory: Option<String>,
	/// Actual executable digest after component verification, when available.
	pub codex_sha256: Option<String>,
	/// Configuration result; it does not change the service health report.
	pub status: NativeStartupConfigStatus,
}
impl NativeStartupConfiguration {
	/// Return a short description of the configuration result.
	pub const fn summary(&self) -> &'static str {
		match &self.status {
			NativeStartupConfigStatus::Loaded { .. } => "Codex configuration loaded",
			NativeStartupConfigStatus::Rejected { cloud_policy: true, .. } =>
				"Codex cloud policy could not load",
			NativeStartupConfigStatus::Rejected { .. } => "Codex configuration was rejected",
			NativeStartupConfigStatus::Unavailable => "Codex configuration check unavailable",
		}
	}

	/// Format only safe fields for display and explicit copying.
	pub fn report_text(&self) -> String {
		let mut text = format!(
			"{}\nUses the shared Codex sign-in. Account overrides for a task are not checked.",
			self.summary()
		);

		if let Some(directory) = &self.directory {
			text.push_str(&format!("\nDirectory: {directory}"));
		}

		match &self.status {
			NativeStartupConfigStatus::Loaded { version, login_methods, requirements_present } => {
				let methods = login_methods.as_ref().map_or_else(
					|| "Not reported".into(),
					|methods| {
						if methods.is_empty() {
							return "None".into();
						}

						methods
							.iter()
							.map(|m| match m {
								NativeStartupLoginMethod::Chatgpt => "ChatGPT",
								NativeStartupLoginMethod::Api => "API key",
							})
							.collect::<Vec<_>>()
							.join(", ")
					},
				);

				text.push_str(&format!(
					"\nNative version: {version}\nAllowed sign-in methods: {methods}\nNative requirements: {}",
					if *requirements_present { "Present" } else { "None reported" }
				));
			},
			NativeStartupConfigStatus::Rejected { location: Some(location), .. } => {
				text.push_str(&format!("\nConfiguration: {}", location.file));

				if let Some(line) = location.line {
					text.push_str(&format!("\nLine: {line}"));
				}
				if let Some(column) = location.column {
					text.push_str(&format!("\nColumn: {column}"));
				}
			},
			_ => {},
		}

		if let Some(digest) = &self.codex_sha256 {
			text.push_str(&format!("\nCodex SHA-256: {digest}"));
		}

		text
	}
}

impl NativeStartupConfiguration {
	/// Construct an unavailable result for a checked directory without private error text.
	pub fn unavailable(directory: &str) -> Self {
		Self {
			directory: valid_path(directory).then(|| directory.to_owned()),
			codex_sha256: None,
			status: NativeStartupConfigStatus::Unavailable,
		}
	}

	/// Validate the bounded safe fields before presentation or copying.
	pub fn is_valid(&self) -> bool {
		self.directory.as_ref().is_none_or(|v| valid_path(v))
			&& self
				.codex_sha256
				.as_ref()
				.is_none_or(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
			&& match &self.status {
				NativeStartupConfigStatus::Loaded { version, login_methods, .. } =>
					self.directory.is_some()
						&& self.codex_sha256.is_some()
						&& !version.is_empty()
						&& version.len() <= 128
						&& version
							.bytes()
							.all(|b| b.is_ascii_alphanumeric() || b".-+_".contains(&b))
						&& login_methods.as_ref().is_none_or(|v| v.len() <= 2),
				NativeStartupConfigStatus::Rejected { location, .. } =>
					self.directory.is_some()
						&& self.codex_sha256.is_some()
						&& location.as_ref().is_none_or(|v| {
							valid_path(&v.file)
								&& v.line.is_none_or(|n| n > 0)
								&& v.column.is_none_or(|n| n > 0)
						}),
				NativeStartupConfigStatus::Unavailable => true,
			}
	}
}
fn valid_path(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= 4_096
		&& Path::new(value).is_absolute()
		&& !value.chars().any(char::is_control)
		&& !decodex_core::contains_credential_material(value)
}

#[cfg(test)]
mod tests {
	use crate::native_startup_config::{NativeStartupConfigStatus, NativeStartupConfiguration};

	#[test]
	fn successful_startup_configuration_requires_exact_source_and_artifact_identity() {
		let mut report = NativeStartupConfiguration {
			directory: Some("/tmp/project".into()),
			codex_sha256: None,
			status: NativeStartupConfigStatus::Loaded {
				version: "0.0.0".into(),
				login_methods: None,
				requirements_present: false,
			},
		};

		assert!(!report.is_valid());

		report.codex_sha256 = Some("a".repeat(64));

		assert!(report.is_valid());

		report.directory = None;

		assert!(!report.is_valid());
	}
}
