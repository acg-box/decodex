//! Artifact ingestion and schema-specific ledger extraction.

use std::path::Path;

use rusqlite::Connection;
use serde_json::Value;

use crate::ledger::{
	records::{self, ArtifactLinkInput, CommitInput, ReviewInput},
	subjects::{self, RadarSubject},
};
use crate::{
	BUNDLE_SCHEMA, SIGNAL_SCHEMA,
	prelude::{Result, eyre},
};

pub(super) fn ingest_artifact_set(
	connection: &Connection,
	bundle_path: &Path,
	analysis_path: Option<&Path>,
	signal_path: Option<&Path>,
) -> Result<()> {
	let bundle = crate::load_json(bundle_path)?;
	let signal_exists = signal_path.is_some_and(Path::exists);
	let (repo, subject_kind, subject_id) = record_bundle(
		connection,
		&bundle,
		bundle_path,
		if signal_exists { "signal" } else { "watch" },
		"Imported from generated Radar artifacts.",
	)?;

	if let Some(path) = analysis_path.filter(|path| path.exists()) {
		records::record_artifact(
			connection,
			ArtifactLinkInput {
				repo: &repo,
				subject_kind: &subject_kind,
				subject_id: &subject_id,
				artifact_kind: "analysis",
				path,
			},
		)?;
	}
	if let Some(path) = signal_path.filter(|path| path.exists()) {
		let signal_subjects = record_signal_artifact(connection, path)?;

		if !signal_subjects.iter().any(|subject| {
			subject.repo == repo
				&& subject.subject_kind == subject_kind
				&& subject.subject_id == subject_id
		}) {
			records::record_artifact(
				connection,
				ArtifactLinkInput {
					repo: &repo,
					subject_kind: &subject_kind,
					subject_id: &subject_id,
					artifact_kind: "signal",
					path,
				},
			)?;
		}
	}

	Ok(())
}

pub(super) fn record_signal_artifact(
	connection: &Connection,
	signal_path: &Path,
) -> Result<Vec<RadarSubject>> {
	let signal = crate::load_json(signal_path)?;
	let validation = crate::validate_artifact(&signal);

	if validation.schema.as_deref() != Some(SIGNAL_SCHEMA) || !validation.errors.is_empty() {
		let mut errors = validation.errors;

		if validation.schema.as_deref() != Some(SIGNAL_SCHEMA) {
			errors.insert(0, format!("schema must be {SIGNAL_SCHEMA}"));
		}

		eyre::bail!(
			"Signal validation failed for {}:\n- {}",
			signal_path.display(),
			errors.join("\n- ")
		);
	}

	let signal = crate::object_value(&signal, "signal")?;
	let slug = crate::required_string(signal, "slug", "slug")?;
	let confidence = crate::required_string(signal, "confidence", "confidence")?;
	let subjects = subjects::subject_refs_for_signal(signal);

	for subject in &subjects {
		records::record_review(
			connection,
			ReviewInput {
				repo: &subject.repo,
				subject_kind: &subject.subject_kind,
				subject_id: &subject.subject_id,
				status: "signal",
				reason: &format!("Published signal_entry/v1: {slug}"),
				confidence: Some(confidence),
			},
		)?;
		records::record_artifact(
			connection,
			ArtifactLinkInput {
				repo: &subject.repo,
				subject_kind: &subject.subject_kind,
				subject_id: &subject.subject_id,
				artifact_kind: "signal",
				path: signal_path,
			},
		)?;
	}

	Ok(subjects)
}

fn record_bundle(
	connection: &Connection,
	bundle: &Value,
	bundle_path: &Path,
	status: &str,
	reason: &str,
) -> Result<(String, String, String)> {
	let validation = crate::validate_artifact(bundle);

	if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) || !validation.errors.is_empty() {
		let mut errors = validation.errors;

		if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) {
			errors.insert(0, format!("schema must be {BUNDLE_SCHEMA}"));
		}

		eyre::bail!("Bundle validation failed:\n- {}", errors.join("\n- "));
	}

	let (repo, subject_kind, subject_id) = subject_for_bundle(bundle)?;
	let bundle = crate::object_value(bundle, "bundle")?;
	let pr_number = bundle
		.get("primary_pr")
		.and_then(Value::as_object)
		.and_then(|primary_pr| primary_pr.get("number"))
		.and_then(Value::as_i64);
	let commits = crate::non_empty_array(bundle.get("commits"))
		.ok_or_else(|| eyre::eyre!("commits must be a non-empty list"))?;

	for commit in commits {
		let commit = crate::object_value(commit, "commit")?;

		records::record_commit(
			connection,
			CommitInput {
				repo: &repo,
				sha: crate::required_string(commit, "sha", "commit.sha")?,
				title: crate::required_string(commit, "message", "commit.message")?,
				url: crate::required_string(commit, "url", "commit.url")?,
				committed_at: crate::optional_string(commit, "committed_at"),
				pr_number,
			},
		)?;
	}

	records::record_review(
		connection,
		ReviewInput {
			repo: &repo,
			subject_kind: &subject_kind,
			subject_id: &subject_id,
			status,
			reason,
			confidence: if status == "signal" { Some("confirmed") } else { None },
		},
	)?;
	records::record_artifact(
		connection,
		ArtifactLinkInput {
			repo: &repo,
			subject_kind: &subject_kind,
			subject_id: &subject_id,
			artifact_kind: "bundle",
			path: bundle_path,
		},
	)?;

	Ok((repo, subject_kind, subject_id))
}

fn subject_for_bundle(bundle: &Value) -> Result<(String, String, String)> {
	let bundle = crate::object_value(bundle, "bundle")?;
	let repo = crate::required_string(bundle, "repo", "repo")?.to_owned();

	if let Some(number) = bundle
		.get("primary_pr")
		.and_then(Value::as_object)
		.and_then(|primary_pr| primary_pr.get("number"))
		.and_then(Value::as_u64)
	{
		return Ok((repo, "pr".into(), number.to_string()));
	}

	let commits = crate::non_empty_array(bundle.get("commits"))
		.ok_or_else(|| eyre::eyre!("commits must be a non-empty list"))?;
	let first_commit = crate::object_value(&commits[0], "commits[0]")?;
	let sha = crate::required_string(first_commit, "sha", "commits[0].sha")?;

	Ok((repo, "commit".into(), sha.to_owned()))
}
