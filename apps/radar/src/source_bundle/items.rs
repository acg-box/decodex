use crate::{Value, prelude::Result, source_bundle::fields};

pub(super) fn commit_bundle_item(commit: &Value) -> Result<Value> {
	let commit = crate::object_value(commit, "commit")?;
	let payload = fields::object_field(commit, "commit", "commit.commit")?;
	let author = fields::object_field(payload, "author", "commit.commit.author").ok();
	let author_name = commit
		.get("author")
		.and_then(Value::as_object)
		.and_then(|author| author.get("login"))
		.and_then(Value::as_str)
		.or_else(|| author.and_then(|author| author.get("name")).and_then(Value::as_str));
	let committed_at = author.and_then(|author| author.get("date")).cloned().unwrap_or(Value::Null);

	Ok(serde_json::json!({
		"sha": crate::required_string(commit, "sha", "commit.sha")?,
		"message": crate::first_line(crate::required_string(payload, "message", "commit.commit.message")?),
		"url": crate::required_string(commit, "html_url", "commit.html_url")?,
		"author": author_name,
		"committed_at": committed_at,
	}))
}

pub(super) fn file_bundle_item(file: &Value) -> Result<Value> {
	let file = crate::object_value(file, "file")?;

	Ok(serde_json::json!({
		"path": crate::required_string(file, "filename", "file.filename")?,
		"status": crate::required_string(file, "status", "file.status")?,
		"additions": fields::required_i64(file, "additions", "file.additions")?,
		"deletions": fields::required_i64(file, "deletions", "file.deletions")?,
		"patch_excerpt": file
			.get("patch")
			.and_then(Value::as_str)
			.and_then(truncate_patch),
	}))
}

fn truncate_patch(value: &str) -> Option<String> {
	let compact = value.trim();

	if compact.is_empty() {
		return None;
	}
	if compact.chars().count() > 900 {
		let mut truncated = compact.chars().take(900).collect::<String>();

		truncated.push_str("...");

		Some(truncated)
	} else {
		Some(compact.into())
	}
}
