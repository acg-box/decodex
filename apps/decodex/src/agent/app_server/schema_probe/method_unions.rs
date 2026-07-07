use std::{collections::BTreeMap, fs, path::Path};

use serde_json::{self, Value};

use crate::{
	agent::app_server::schema_probe::constants::{
		APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS, APP_SERVER_REQUIRED_CLIENT_REQUESTS,
		APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS, APP_SERVER_REQUIRED_SERVER_REQUESTS,
	},
	prelude::{Result, eyre},
};

pub(in crate::agent::app_server::schema_probe) fn validate_generated_app_server_method_unions(
	out_dir: &Path,
) -> Result<()> {
	validate_generated_method_union(out_dir, "ClientRequest", APP_SERVER_REQUIRED_CLIENT_REQUESTS)?;
	validate_generated_method_union(out_dir, "ServerRequest", APP_SERVER_REQUIRED_SERVER_REQUESTS)?;
	validate_generated_method_union(
		out_dir,
		"ClientNotification",
		APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS,
	)?;
	validate_generated_method_union(
		out_dir,
		"ServerNotification",
		APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS,
	)?;

	Ok(())
}

fn validate_generated_method_union(
	out_dir: &Path,
	title: &'static str,
	required_methods: &[(&'static str, &'static str)],
) -> Result<()> {
	let Some(schema) = find_schema_by_title(out_dir, title)? else {
		eyre::bail!("Generated app-server schema was missing `{title}` method union.");
	};
	let method_refs = method_schema_refs(&schema);
	let missing_or_mismatched = required_methods
		.iter()
		.filter_map(|(method, expected_ref)| match method_refs.get(*method) {
			Some(actual_ref) if actual_ref.as_deref() == expected_ref_to_option(expected_ref) => {
				None
			},
			Some(actual_ref) => Some(format!(
				"{method} expected {} got {}",
				expected_ref_display(expected_ref),
				actual_ref.as_deref().unwrap_or("<no params>")
			)),
			None => Some(format!("{method} missing")),
		})
		.collect::<Vec<_>>();

	if !missing_or_mismatched.is_empty() {
		eyre::bail!(
			"Generated app-server `{title}` schema was missing or changed Decodex-owned methods: {}",
			missing_or_mismatched.join(", ")
		);
	}

	Ok(())
}

fn find_schema_by_title(out_dir: &Path, title: &str) -> Result<Option<Value>> {
	let direct_path = out_dir.join(format!("{title}.json"));

	if direct_path.is_file() {
		let schema = fs::read_to_string(&direct_path)?;

		return Ok(Some(serde_json::from_str(&schema)?));
	}

	let mut schema = None;

	collect_schema_by_title(out_dir, title, &mut schema)?;

	Ok(schema)
}

fn collect_schema_by_title(
	path: &Path,
	title: &str,
	matching_schema: &mut Option<Value>,
) -> Result<()> {
	for entry in fs::read_dir(path)? {
		let entry = entry?;
		let path = entry.path();

		if path.is_dir() {
			collect_schema_by_title(&path, title, matching_schema)?;
		} else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
			let schema = fs::read_to_string(&path)?;
			let value: Value = serde_json::from_str(&schema)?;

			if value.get("title").and_then(Value::as_str) == Some(title) {
				*matching_schema = Some(value);
			}
		}
	}

	Ok(())
}

fn method_schema_refs(schema: &Value) -> BTreeMap<String, Option<String>> {
	let Some(branches) = schema.get("oneOf").and_then(Value::as_array) else {
		return BTreeMap::new();
	};

	branches
		.iter()
		.filter_map(|branch| {
			let properties = branch.get("properties")?.as_object()?;
			let method =
				properties.get("method")?.get("enum")?.as_array()?.first()?.as_str()?.to_owned();
			let params_ref = properties
				.get("params")
				.and_then(|params| params.get("$ref"))
				.and_then(Value::as_str)
				.map(schema_ref_title);

			Some((method, params_ref))
		})
		.collect()
}

fn schema_ref_title(schema_ref: &str) -> String {
	schema_ref.rsplit('/').next().unwrap_or(schema_ref).to_owned()
}

fn expected_ref_to_option(expected_ref: &str) -> Option<&str> {
	(!expected_ref.is_empty()).then_some(expected_ref)
}

fn expected_ref_display(expected_ref: &str) -> &str {
	expected_ref_to_option(expected_ref).unwrap_or("<no params>")
}
