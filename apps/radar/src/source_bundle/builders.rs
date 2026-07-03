use crate::{
	BUNDLE_SCHEMA, Value,
	prelude::Result,
	source_bundle::{extraction, fields, items, refs, validation},
};

pub(super) fn build_pr_bundle_from_sources(
	repo: &str,
	pr: &Value,
	commits: &[Value],
	files: &[Value],
	default_branch: &str,
	notes: &[String],
) -> Result<Value> {
	let pr = crate::object_value(pr, "pull request")?;
	let commit_items = commits.iter().map(items::commit_bundle_item).collect::<Result<Vec<_>>>()?;
	let file_items = files.iter().map(items::file_bundle_item).collect::<Result<Vec<_>>>()?;
	let docs_refs = refs::collect_docs_refs(files);
	let examples_refs = refs::collect_examples_refs(files);
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
		"number": fields::required_u64(pr, "number", "primary_pr.number")?,
		"title": crate::required_string(pr, "title", "primary_pr.title")?,
		"body": pr.get("body").and_then(Value::as_str).unwrap_or(""),
		"state": pr
			.get("merged_at")
			.and_then(Value::as_str)
			.filter(|value| !value.is_empty())
			.map_or_else(
				|| crate::required_string(pr, "state", "primary_pr.state").map(str::to_owned),
				|_| Ok("merged".to_owned()),
			)?,
		"merged_at": pr.get("merged_at").cloned().unwrap_or(Value::Null),
		"labels": fields::pr_labels(pr),
		"url": crate::required_string(pr, "html_url", "primary_pr.url")?,
	});
	let bundle = serde_json::json!({
		"schema": BUNDLE_SCHEMA,
		"repo": repo,
		"analysis_mode": "pr_first",
		"default_branch": default_branch,
		"primary_pr": primary_pr,
		"commits": commit_items,
		"files": file_items,
		"linked_issues": extraction::collect_issue_refs(
			&[pr.get("body").and_then(Value::as_str).unwrap_or(""), &all_commit_text]
		)?,
		"extracted_flags": extraction::collect_flags(&[
			pr.get("body").and_then(Value::as_str).unwrap_or(""),
			&all_commit_text,
			&all_patch_text,
		])?,
		"docs_refs": docs_refs,
		"examples_refs": examples_refs,
		"notes": bundle_notes,
	});

	validation::validate_bundle_value(&bundle)?;

	Ok(bundle)
}

pub(super) fn build_commit_bundle_from_sources(
	repo: &str,
	commit: &Value,
	default_branch: &str,
	notes: &[String],
) -> Result<Value> {
	let commit = crate::object_value(commit, "commit")?;
	let files = commit.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
	let commit_payload = fields::object_field(commit, "commit", "commit.commit")?;
	let commit_message =
		crate::required_string(commit_payload, "message", "commit.commit.message")?;
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
		"commits": [items::commit_bundle_item(&Value::Object(commit.clone()))?],
		"files": files
			.iter()
			.map(items::file_bundle_item)
			.collect::<Result<Vec<_>>>()?,
		"linked_issues": extraction::collect_issue_refs(&[commit_message])?,
		"extracted_flags": extraction::collect_flags(&[commit_message, &all_patch_text])?,
		"docs_refs": refs::collect_docs_refs(&files),
		"examples_refs": refs::collect_examples_refs(&files),
		"notes": bundle_notes,
	});

	validation::validate_bundle_value(&bundle)?;

	Ok(bundle)
}
