//! GitHub source payload normalization for Radar bundle artifacts.

use super::{
	BUNDLE_SCHEMA, Map, OnceLock, Regex, Value, eyre, first_line, object_value, required_string,
	serde_json, validate_artifact,
};

pub(super) fn build_pr_bundle_from_sources(
	repo: &str,
	pr: &Value,
	commits: &[Value],
	files: &[Value],
	default_branch: &str,
	notes: &[String],
) -> crate::prelude::Result<Value> {
	let pr = object_value(pr, "pull request")?;
	let commit_items =
		commits.iter().map(commit_bundle_item).collect::<crate::prelude::Result<Vec<_>>>()?;
	let file_items =
		files.iter().map(file_bundle_item).collect::<crate::prelude::Result<Vec<_>>>()?;
	let docs_refs = collect_docs_refs(files);
	let examples_refs = collect_examples_refs(files);
	let all_patch_text = files
		.iter()
		.filter_map(|file| file.get("patch").and_then(Value::as_str))
		.collect::<Vec<_>>()
		.join("\n");
	let all_commit_text = commits
		.iter()
		.filter_map(|commit| {
			commit
				.get("commit")
				.and_then(Value::as_object)
				.and_then(|commit| commit.get("message"))
				.and_then(Value::as_str)
		})
		.collect::<Vec<_>>()
		.join("\n");
	let mut bundle_notes =
		vec!["Built from GitHub pull-request, commits, files, and repo endpoints.".to_owned()];

	bundle_notes.extend(notes.iter().cloned());

	let primary_pr = serde_json::json!({
		"number": required_u64(pr, "number", "primary_pr.number")?,
		"title": required_string(pr, "title", "primary_pr.title")?,
		"body": pr.get("body").and_then(Value::as_str).unwrap_or(""),
		"state": pr
			.get("merged_at")
			.and_then(Value::as_str)
			.filter(|value| !value.is_empty())
			.map_or_else(
				|| required_string(pr, "state", "primary_pr.state").map(str::to_owned),
				|_| Ok("merged".to_owned()),
			)?,
		"merged_at": pr.get("merged_at").cloned().unwrap_or(Value::Null),
		"labels": pr_labels(pr),
		"url": required_string(pr, "html_url", "primary_pr.url")?,
	});
	let bundle = serde_json::json!({
		"schema": BUNDLE_SCHEMA,
		"repo": repo,
		"analysis_mode": "pr_first",
		"default_branch": default_branch,
		"primary_pr": primary_pr,
		"commits": commit_items,
		"files": file_items,
		"linked_issues": collect_issue_refs(
			&[pr.get("body").and_then(Value::as_str).unwrap_or(""), &all_commit_text]
		)?,
		"extracted_flags": collect_flags(&[
			pr.get("body").and_then(Value::as_str).unwrap_or(""),
			&all_commit_text,
			&all_patch_text,
		])?,
		"docs_refs": docs_refs,
		"examples_refs": examples_refs,
		"notes": bundle_notes,
	});

	validate_bundle_value(&bundle)?;

	Ok(bundle)
}

pub(super) fn build_commit_bundle_from_sources(
	repo: &str,
	commit: &Value,
	default_branch: &str,
	notes: &[String],
) -> crate::prelude::Result<Value> {
	let commit = object_value(commit, "commit")?;
	let files = commit.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
	let commit_payload = object_field(commit, "commit", "commit.commit")?;
	let commit_message = required_string(commit_payload, "message", "commit.commit.message")?;
	let all_patch_text = files
		.iter()
		.filter_map(|file| file.get("patch").and_then(Value::as_str))
		.collect::<Vec<_>>()
		.join("\n");
	let mut bundle_notes = vec!["Built from GitHub commit endpoint without PR context.".to_owned()];

	bundle_notes.extend(notes.iter().cloned());

	let bundle = serde_json::json!({
		"schema": BUNDLE_SCHEMA,
		"repo": repo,
		"analysis_mode": "commit_only",
		"default_branch": default_branch,
		"commits": [commit_bundle_item(&Value::Object(commit.clone()))?],
		"files": files
			.iter()
			.map(file_bundle_item)
			.collect::<crate::prelude::Result<Vec<_>>>()?,
		"linked_issues": collect_issue_refs(&[commit_message])?,
		"extracted_flags": collect_flags(&[commit_message, &all_patch_text])?,
		"docs_refs": collect_docs_refs(&files),
		"examples_refs": collect_examples_refs(&files),
		"notes": bundle_notes,
	});

	validate_bundle_value(&bundle)?;

	Ok(bundle)
}

fn commit_bundle_item(commit: &Value) -> crate::prelude::Result<Value> {
	let commit = object_value(commit, "commit")?;
	let payload = object_field(commit, "commit", "commit.commit")?;
	let author = object_field(payload, "author", "commit.commit.author").ok();
	let author_name = commit
		.get("author")
		.and_then(Value::as_object)
		.and_then(|author| author.get("login"))
		.and_then(Value::as_str)
		.or_else(|| author.and_then(|author| author.get("name")).and_then(Value::as_str));
	let committed_at = author.and_then(|author| author.get("date")).cloned().unwrap_or(Value::Null);

	Ok(serde_json::json!({
		"sha": required_string(commit, "sha", "commit.sha")?,
		"message": first_line(required_string(payload, "message", "commit.commit.message")?),
		"url": required_string(commit, "html_url", "commit.html_url")?,
		"author": author_name,
		"committed_at": committed_at,
	}))
}

fn file_bundle_item(file: &Value) -> crate::prelude::Result<Value> {
	let file = object_value(file, "file")?;

	Ok(serde_json::json!({
		"path": required_string(file, "filename", "file.filename")?,
		"status": required_string(file, "status", "file.status")?,
		"additions": required_i64(file, "additions", "file.additions")?,
		"deletions": required_i64(file, "deletions", "file.deletions")?,
		"patch_excerpt": file
			.get("patch")
			.and_then(Value::as_str)
			.and_then(truncate_patch),
	}))
}

fn validate_bundle_value(bundle: &Value) -> crate::prelude::Result<()> {
	let validation = validate_artifact(bundle);

	if validation.errors.is_empty() && validation.schema.as_deref() == Some(BUNDLE_SCHEMA) {
		Ok(())
	} else {
		let mut errors = validation.errors;

		if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) {
			errors.insert(0, format!("schema must be {BUNDLE_SCHEMA}"));
		}

		eyre::bail!("Bundle validation failed:\n- {}", errors.join("\n- "))
	}
}

fn object_field<'a>(
	object: &'a Map<String, Value>,
	field: &str,
	label: &str,
) -> crate::prelude::Result<&'a Map<String, Value>> {
	object
		.get(field)
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("{label} must be an object"))
}

fn required_u64(
	object: &Map<String, Value>,
	field: &str,
	label: &str,
) -> crate::prelude::Result<u64> {
	object
		.get(field)
		.and_then(Value::as_u64)
		.ok_or_else(|| eyre::eyre!("{label} must be an unsigned integer"))
}

fn required_i64(
	object: &Map<String, Value>,
	field: &str,
	label: &str,
) -> crate::prelude::Result<i64> {
	object
		.get(field)
		.and_then(Value::as_i64)
		.ok_or_else(|| eyre::eyre!("{label} must be an integer"))
}

fn pr_labels(pr: &Map<String, Value>) -> Vec<String> {
	pr.get("labels")
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|label| {
			label
				.as_object()
				.and_then(|label| label.get("name"))
				.and_then(Value::as_str)
				.map(str::to_owned)
		})
		.collect()
}

fn collect_docs_refs(files: &[Value]) -> Vec<String> {
	files
		.iter()
		.filter_map(file_name)
		.filter(|filename| filename.starts_with("docs/") || filename.ends_with("README.md"))
		.map(str::to_owned)
		.collect()
}

fn collect_examples_refs(files: &[Value]) -> Vec<String> {
	files
		.iter()
		.filter_map(file_name)
		.filter(|filename| {
			filename.to_lowercase().contains("example") || filename.contains("examples/")
		})
		.map(str::to_owned)
		.collect()
}

fn file_name(file: &Value) -> Option<&str> {
	file.as_object()?.get("filename")?.as_str()
}

fn collect_issue_refs(texts: &[&str]) -> crate::prelude::Result<Vec<String>> {
	collect_regex_matches(issue_ref_regex()?, texts)
}

fn collect_flags(texts: &[&str]) -> crate::prelude::Result<Vec<String>> {
	collect_regex_matches(flag_regex()?, texts)
}

fn collect_regex_matches(regex: &Regex, texts: &[&str]) -> crate::prelude::Result<Vec<String>> {
	let mut found = Vec::new();

	for text in texts {
		for captures in regex.captures_iter(text) {
			let Some(value) = captures.get(1).map(|matched| matched.as_str()) else {
				continue;
			};

			if !found.iter().any(|found_value| found_value == value) {
				found.push(value.to_owned());
			}
		}
	}

	Ok(found)
}

fn issue_ref_regex() -> crate::prelude::Result<&'static Regex> {
	static ISSUE_REF_RE: OnceLock<std::result::Result<Regex, regex::Error>> = OnceLock::new();

	ISSUE_REF_RE
		.get_or_init(|| Regex::new(r"(?:^|[^\w])((?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#\d+)"))
		.as_ref()
		.map_err(|error| eyre::eyre!("Failed to compile issue reference regex: {error}"))
}

fn flag_regex() -> crate::prelude::Result<&'static Regex> {
	static FLAG_RE: OnceLock<std::result::Result<Regex, regex::Error>> = OnceLock::new();

	FLAG_RE
		.get_or_init(|| {
			Regex::new(r"(?:^|[^\w-])(--[a-zA-Z0-9][\w-]*|[A-Z][A-Z0-9_]{2,}(?:=[^\s,`]+)?)")
		})
		.as_ref()
		.map_err(|error| eyre::eyre!("Failed to compile flag regex: {error}"))
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
