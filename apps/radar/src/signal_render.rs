//! Signal artifact rendering helpers for Radar.

mod config_flags;
mod source_refs;
mod timestamp;

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::{
	SIGNAL_SCHEMA,
	prelude::{Result, eyre},
};

pub(super) fn rendered_signal(
	bundle: &Value,
	analysis: &Value,
	published_at_override: Option<&str>,
	config_flags: Vec<String>,
) -> Result<Value> {
	let bundle = crate::object_value(bundle, "Bundle")?;
	let analysis = crate::object_value(analysis, "Analysis draft")?;
	let title = crate::required_string(analysis, "title", "analysis draft")?;
	let slug = crate::string_field(analysis, "slug")
		.filter(|value| !value.is_empty())
		.map(str::to_owned)
		.unwrap_or_else(|| crate::slugify(title));
	let mut signal = Map::new();

	signal.insert("schema".into(), serde_json::json!(SIGNAL_SCHEMA));
	signal.insert("slug".into(), serde_json::json!(slug));
	signal.insert("lane".into(), serde_json::json!("github"));

	for field in ["kind", "title", "summary", "why_it_matters", "confidence", "impact"] {
		signal.insert(
			field.into(),
			serde_json::json!(crate::required_string(analysis, field, "analysis draft")?),
		);
	}

	signal.insert(
		"published_at".into(),
		serde_json::json!(timestamp::pick_published_at(bundle, analysis, published_at_override)?),
	);
	signal.insert("config_flags".into(), serde_json::json!(config_flags));
	signal.insert(
		"proof_points".into(),
		analysis
			.get("proof_points")
			.cloned()
			.ok_or_else(|| eyre::eyre!("analysis draft proof_points is required"))?,
	);
	signal.insert("source_refs".into(), rendered_source_refs(bundle)?);

	for field in ["how_to_try", "expected_effect", "caveats", "watch_state"] {
		if crate::is_truthy_json_value(analysis.get(field)) {
			signal.insert(
				field.into(),
				analysis
					.get(field)
					.cloned()
					.ok_or_else(|| eyre::eyre!("analysis draft {field} is required"))?,
			);
		}
	}

	Ok(Value::Object(signal))
}

pub(super) fn rendered_config_flags(
	bundle: &Value,
	analysis: &Value,
	known_features: &BTreeSet<String>,
) -> Vec<String> {
	let raw_flags = analysis
		.get("config_flags")
		.filter(|value| !value.is_null())
		.or_else(|| bundle.get("extracted_flags"));

	config_flags::normalized_config_flags(raw_flags, known_features)
}

fn rendered_source_refs(bundle: &Map<String, Value>) -> Result<Value> {
	source_refs::rendered_source_refs(bundle)
}
