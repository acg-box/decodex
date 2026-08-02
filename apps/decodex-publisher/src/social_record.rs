//! Atomic Content Manager recording and immutable publication identity.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
	SOCIAL_CANDIDATE_SCHEMA, SOCIAL_POST_SCHEMA,
	filesystem::PinnedPrivateJsonFile,
	prelude::{Result, eyre},
};

const MAX_STAGING_BYTES: u64 = 1024 * 1024;

#[derive(Debug)]
pub(crate) struct SocialRecordCandidateRequest {
	pub(crate) staging_path: PathBuf,
	pub(crate) staging_dir: PathBuf,
	pub(crate) candidates_dir: PathBuf,
	pub(crate) posts_dir: PathBuf,
	pub(crate) attempts_dir: PathBuf,
	pub(crate) locks_dir: PathBuf,
	pub(crate) run_id: String,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SocialRecordCandidateReport {
	pub(crate) status: String,
	pub(crate) decision: String,
	pub(crate) run_id: String,
	pub(crate) path: String,
	pub(crate) staging_cleaned: bool,
}

pub(crate) fn record_social_candidate(
	request: &SocialRecordCandidateRequest,
) -> Result<SocialRecordCandidateReport> {
	if !crate::social_publish::valid_run_id(&request.run_id) {
		eyre::bail!("run_id must be a lowercase UUID");
	}
	let root = crate::repo_root()?;
	let staging_dir = crate::resolve_against(&root, &request.staging_dir);
	let staging_path = crate::resolve_against(&root, &request.staging_path);
	crate::ensure_private_directory(&staging_dir)?;
	crate::require_contained_regular_file(&staging_path, &staging_dir)
		.map_err(|error| eyre::eyre!("staging artifact is invalid: {error}"))?;

	let candidates_dir = crate::resolve_against(&root, &request.candidates_dir);
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let attempts_dir = crate::resolve_against(&root, &request.attempts_dir);
	let _state_lock = crate::social_publish::scan::acquire_social_state_lock(&request.locks_dir)?;

	let staging = PinnedPrivateJsonFile::open(&staging_path, MAX_STAGING_BYTES)?;
	let mut candidate = staging.payload.clone();
	if candidate.get("schema").and_then(Value::as_str) != Some(SOCIAL_CANDIDATE_SCHEMA) {
		eyre::bail!("Content Manager staging accepts only {SOCIAL_CANDIDATE_SCHEMA}");
	}
	apply_publication_identity(&mut candidate)?;
	crate::validate_generated_social_artifact(&candidate)
		.map_err(|error| eyre::eyre!("staging artifact failed validation: {error}"))?;
	crate::social_evidence::validate_source_evidence(&candidate)
		.map_err(|error| eyre::eyre!("candidate evidence failed validation: {error}"))?;

	let destination = candidates_dir.join(format!("{}.json", request.run_id));
	if let Some(existing) = load_optional_json(&destination)? {
		if existing != candidate {
			eyre::bail!("refusing to overwrite an existing Content Manager effect");
		}
		staging.unlink()?;
		return report("already_recorded", request, &root, &destination, &candidate);
	}
	require_no_candidate_backpressure(&candidates_dir, &posts_dir, &attempts_dir)?;
	crate::write_new_json(&destination, &candidate)?;

	let recorded = crate::load_json(&destination)?;
	if recorded != candidate {
		eyre::bail!("authoritative Content Manager effect changed after write");
	}
	staging.unlink()?;
	report("recorded", request, &root, &destination, &candidate)
}

fn report(
	status: &str,
	request: &SocialRecordCandidateRequest,
	root: &Path,
	destination: &Path,
	candidate: &Value,
) -> Result<SocialRecordCandidateReport> {
	let decision = candidate
		.pointer("/decision/worthiness")
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("candidate decision is missing"))?;
	Ok(SocialRecordCandidateReport {
		status: status.into(),
		decision: decision.into(),
		run_id: request.run_id.clone(),
		path: crate::path_arg(root, destination),
		staging_cleaned: true,
	})
}

fn require_no_candidate_backpressure(
	candidates_dir: &Path,
	posts_dir: &Path,
	attempts_dir: &Path,
) -> Result<()> {
	validate_attempt_records(attempts_dir)?;
	let terminal_refs = existing_json_files(posts_dir)?
		.into_iter()
		.map(|path| crate::load_json(&path))
		.collect::<Result<Vec<_>>>()?
		.into_iter()
		.filter(|post| post.get("schema").and_then(Value::as_str) == Some(SOCIAL_POST_SCHEMA))
		.filter_map(|post| post.pointer("/source_refs/social_candidates").cloned())
		.filter_map(|refs| refs.as_array().cloned())
		.flatten()
		.filter_map(|value| value.as_str().map(str::to_owned))
		.collect::<std::collections::BTreeSet<_>>();
	let root = crate::repo_root()?;
	for path in existing_json_files(candidates_dir)? {
		let candidate = crate::load_json(&path)?;
		crate::validate_generated_social_artifact(&candidate)
			.map_err(|error| eyre::eyre!("existing candidate failed validation: {error}"))?;
		crate::social_evidence::validate_source_evidence(&candidate).map_err(|error| {
			eyre::eyre!("existing candidate evidence failed validation: {error}")
		})?;
		let candidate_ref = crate::path_arg(&root, &path);
		if terminal_refs.contains(&candidate_ref) {
			continue;
		}
		let publication_lineage_sha256 = publication_lineage_sha256(&candidate)?;
		if crate::social_xurl::publication_effect_conflict(
			attempts_dir,
			&publication_lineage_sha256,
			None,
		)?
		.is_none()
		{
			eyre::bail!("one Content Manager candidate is still pending: {candidate_ref}");
		}
	}
	Ok(())
}

fn validate_attempt_records(attempts_dir: &Path) -> Result<()> {
	for path in existing_json_files(attempts_dir)? {
		let payload = crate::load_json(&path)?;
		match payload.get("schema").and_then(Value::as_str) {
			Some(crate::social_xurl::model::ATTEMPT_SCHEMA) => {
				let attempt: crate::social_xurl::model::XurlAttempt =
					serde_json::from_value(payload).map_err(|_| {
						eyre::eyre!("{} is not a valid xurl publication attempt", path.display())
					})?;
				crate::social_xurl::ledger::validate_publication_cost_record(&attempt)?;
			},
			Some(crate::social_xurl::model::OBSERVATION_ATTEMPT_SCHEMA) => {
				let attempt: crate::social_xurl::model::XurlObservationAttempt =
					serde_json::from_value(payload).map_err(|_| {
						eyre::eyre!("{} is not a valid xurl observation attempt", path.display())
					})?;
				crate::social_xurl::ledger::validate_observation_cost_record(&attempt)?;
			},
			_ => eyre::bail!("{} has invalid xurl attempt state", path.display()),
		}
	}
	Ok(())
}

fn existing_json_files(path: &Path) -> Result<Vec<PathBuf>> {
	if !path.exists() {
		return Ok(Vec::new());
	}
	crate::collect_json_files(&[path.to_path_buf()])
}

fn load_optional_json(path: &Path) -> Result<Option<Value>> {
	if !path.exists() {
		return Ok(None);
	}
	Ok(Some(crate::load_json(path)?))
}

pub(crate) fn apply_publication_identity(candidate: &mut Value) -> Result<()> {
	let supplied =
		candidate.pointer("/decision/idempotency_key").and_then(Value::as_str).map(str::to_owned);
	let expected = publication_idempotency_key(candidate)?;
	if supplied.as_deref().is_some_and(|value| value != expected) {
		eyre::bail!("candidate idempotency_key does not match its immutable content evidence");
	}
	let decision = candidate
		.get_mut("decision")
		.and_then(Value::as_object_mut)
		.ok_or_else(|| eyre::eyre!("candidate decision is required"))?;
	decision.insert("idempotency_key".into(), Value::String(expected));
	Ok(())
}

pub(crate) fn publication_lineage_sha256(candidate: &Value) -> Result<String> {
	if candidate.get("schema").and_then(Value::as_str) != Some(SOCIAL_CANDIDATE_SCHEMA) {
		eyre::bail!("publication identity requires {SOCIAL_CANDIDATE_SCHEMA}");
	}
	let mut identity = candidate.clone();
	let decision = identity
		.get_mut("decision")
		.and_then(Value::as_object_mut)
		.ok_or_else(|| eyre::eyre!("candidate decision is required"))?;
	decision.remove("idempotency_key");
	let bytes = serde_json::to_vec(&identity)?;
	let mut digest = Sha256::new();
	digest.update(b"decodex-content-evidence-v1\0");
	digest.update(bytes);
	Ok(digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(crate) fn publication_idempotency_key(candidate: &Value) -> Result<String> {
	Ok(format!("content-publication:{}", publication_lineage_sha256(candidate)?))
}

pub(crate) fn validate_publication_identity(candidate: &Value) -> Result<()> {
	if candidate.get("schema").and_then(Value::as_str) != Some(SOCIAL_CANDIDATE_SCHEMA) {
		return Ok(());
	}
	let actual = candidate
		.pointer("/decision/idempotency_key")
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("candidate idempotency_key is required"))?;
	let expected = publication_idempotency_key(candidate)?;
	if actual != expected {
		eyre::bail!("candidate idempotency_key does not match its immutable content evidence");
	}
	Ok(())
}
