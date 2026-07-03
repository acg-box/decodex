use std::collections::BTreeSet;

use serde_json::Value;

pub(super) fn normalized_config_flags(
	raw_flags: Option<&Value>,
	known_features: &BTreeSet<String>,
) -> Vec<String> {
	let Some(raw_flags) = raw_flags.and_then(Value::as_array) else {
		return Vec::new();
	};
	let mut normalized = Vec::new();
	let mut seen = BTreeSet::new();

	for flag in raw_flags {
		let Some(raw_value) = flag.as_str() else {
			continue;
		};
		let mut value = raw_value.trim().to_owned();

		if value.is_empty() || seen.contains(&value) {
			continue;
		}

		if let Some(feature_name) = normalize_feature_flag(&value, known_features) {
			value = format!("features.{feature_name} = true");
		}

		if !(value.starts_with("--")
			|| value.contains('=')
			|| value.ends_with(".json")
			|| value.ends_with(".toml"))
		{
			continue;
		}

		seen.insert(value.clone());
		normalized.push(value);
	}

	normalized
}

fn normalize_feature_flag(value: &str, known_features: &BTreeSet<String>) -> Option<String> {
	let lower = value.to_ascii_lowercase();

	if let Some(candidate) = lower.strip_prefix("--enable ") {
		let candidate = candidate.trim();

		return known_feature_name(candidate, known_features);
	}

	let candidate = lower.strip_prefix("features.").unwrap_or(&lower);
	let (name, enabled) =
		candidate.split_once('=').map_or((candidate.trim(), true), |(name, enabled)| {
			(name.trim(), enabled.trim() == "true")
		});

	enabled.then(|| known_feature_name(name, known_features)).flatten()
}

fn known_feature_name(value: &str, known_features: &BTreeSet<String>) -> Option<String> {
	let valid = !value.is_empty()
		&& value.chars().all(|character| {
			character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
		});

	if valid && known_features.contains(value) { Some(value.to_owned()) } else { None }
}
