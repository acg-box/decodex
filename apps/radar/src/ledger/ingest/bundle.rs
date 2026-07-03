use crate::{
	ledger::{
		self, ArtifactLinkInput, BUNDLE_SCHEMA, CommitInput, Connection, Path, ReviewInput,
		SIGNAL_SCHEMA, Value, eyre,
		ingest::{signal, subject},
	},
	prelude::Result,
};

pub(crate) fn ingest_artifact_set(
	connection: &Connection,
	bundle_path: &Path,
	analysis_path: Option<&Path>,
	signal_path: Option<&Path>,
) -> Result<()> {
	let bundle = ledger::load_json(bundle_path)?;
	let signal_exists = signal_path.is_some_and(Path::exists);
	let (repo, subject_kind, subject_id) = record_bundle(
		connection,
		&bundle,
		bundle_path,
		if signal_exists { "signal" } else { "watch" },
		"Imported from generated Radar artifacts.",
	)?;

	if let Some(path) = analysis_path.filter(|path| path.exists()) {
		ledger::record_artifact(
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
		let signal_subjects = signal::record_signal_artifact(connection, path)?;

		if !signal_subjects.iter().any(|subject| {
			subject.repo == repo
				&& subject.subject_kind == subject_kind
				&& subject.subject_id == subject_id
		}) {
			ledger::record_artifact(
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

pub(super) fn signal_schema() -> &'static str {
	SIGNAL_SCHEMA
}

fn record_bundle(
	connection: &Connection,
	bundle: &Value,
	bundle_path: &Path,
	status: &str,
	reason: &str,
) -> Result<(String, String, String)> {
	let validation = ledger::validate_artifact(bundle);

	if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) || !validation.errors.is_empty() {
		let mut errors = validation.errors;

		if validation.schema.as_deref() != Some(BUNDLE_SCHEMA) {
			errors.insert(0, format!("schema must be {BUNDLE_SCHEMA}"));
		}

		eyre::bail!("Bundle validation failed:\n- {}", errors.join("\n- "));
	}

	let (repo, subject_kind, subject_id) = subject::subject_for_bundle(bundle)?;

	record_bundle_commits(connection, bundle, &repo)?;

	ledger::record_review(
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
	ledger::record_artifact(
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

fn record_bundle_commits(connection: &Connection, bundle: &Value, repo: &str) -> Result<()> {
	let bundle = ledger::object_value(bundle, "bundle")?;
	let pr_number = bundle
		.get("primary_pr")
		.and_then(Value::as_object)
		.and_then(|primary_pr| primary_pr.get("number"))
		.and_then(Value::as_i64);
	let commits = ledger::non_empty_array(bundle.get("commits"))
		.ok_or_else(|| eyre::eyre!("commits must be a non-empty list"))?;

	for commit in commits {
		let commit = ledger::object_value(commit, "commit")?;

		ledger::record_commit(
			connection,
			CommitInput {
				repo,
				sha: ledger::required_string(commit, "sha", "commit.sha")?,
				title: ledger::required_string(commit, "message", "commit.message")?,
				url: ledger::required_string(commit, "url", "commit.url")?,
				committed_at: ledger::optional_string(commit, "committed_at"),
				pr_number,
			},
		)?;
	}

	Ok(())
}
