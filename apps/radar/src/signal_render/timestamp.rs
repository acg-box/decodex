use serde_json::{Map, Value};

use crate::{
	prelude::{Result, eyre},
	signal_render::source_refs,
};

pub(super) fn pick_published_at(
	bundle: &Map<String, Value>,
	analysis: &Map<String, Value>,
	override_value: Option<&str>,
) -> Result<String> {
	if let Some(value) = override_value.filter(|value| !value.is_empty()) {
		return Ok(value.to_owned());
	}
	if let Some(value) =
		crate::string_field(analysis, "published_at").filter(|value| !value.is_empty())
	{
		return Ok(value.to_owned());
	}
	if let Some(value) = bundle
		.get("primary_pr")
		.and_then(Value::as_object)
		.and_then(|primary_pr| crate::string_field(primary_pr, "merged_at"))
		.filter(|value| !value.is_empty())
	{
		return Ok(value.to_owned());
	}

	let first_commit = source_refs::bundle_commits(bundle)?
		.into_iter()
		.next()
		.ok_or_else(|| eyre::eyre!("Bundle commits must be a non-empty list"))?;

	if let Some(value) =
		crate::string_field(first_commit, "committed_at").filter(|value| !value.is_empty())
	{
		Ok(value.to_owned())
	} else {
		crate::utc_now_iso()
	}
}
