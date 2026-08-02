//! Machine-owned Content Manager writes and Radar lineage verification.

use std::{
	collections::BTreeSet,
	ffi::OsStr,
	fs,
	path::{Component, Path, PathBuf},
};

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	SOCIAL_CANDIDATE_SCHEMA, SOCIAL_POST_SCHEMA, SOCIAL_STRATEGY_SCHEMA,
	filesystem::PinnedPrivateJsonFile,
	prelude::{Result, eyre},
	social_validation::SocialValidationState,
};

const MAX_STAGING_BYTES: u64 = 1024 * 1024;
const MAX_RADAR_AGE: Duration = Duration::hours(12);
const MAX_FUTURE_SKEW: Duration = Duration::minutes(5);
const RADAR_CACHE_ROOT: &str = ".agent/automations/radar/cache";
const RADAR_QUEUE_PATH: &str =
	".agent/automations/radar/cache/github/review-queue/openai-codex-latest.json";
const RADAR_PAIR_PREFIX: &str = ".agent/automations/radar/cache/github/content-review-pairs";
const RADAR_ELIGIBILITY_SCHEMA: &str = "radar_content_eligibility/v1";
const RADAR_QUEUE_SCHEMA: &str = "upstream_review_queue/v1";
const RADAR_REVIEW_SCHEMA: &str = "upstream_review/v1";
const RADAR_IMPACT_SCHEMA: &str = "upstream_impact/v1";

#[derive(Debug)]
pub(crate) struct SocialRecordManagerRequest {
	pub(crate) staging_path: PathBuf,
	pub(crate) staging_dir: PathBuf,
	pub(crate) candidates_dir: PathBuf,
	pub(crate) strategies_dir: PathBuf,
	pub(crate) reservations_dir: PathBuf,
	pub(crate) posts_dir: PathBuf,
	pub(crate) outcomes_dir: PathBuf,
	pub(crate) locks_dir: PathBuf,
	pub(crate) run_id: String,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SocialRecordManagerReport {
	pub(crate) status: String,
	pub(crate) kind: String,
	pub(crate) run_id: String,
	pub(crate) path: String,
	pub(crate) staging_cleaned: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg(test)]
pub(crate) enum SocialRecordHookPoint {
	Locked,
	AuthoritativeWritten,
}

pub(crate) fn record_social_manager(
	request: &SocialRecordManagerRequest,
) -> Result<SocialRecordManagerReport> {
	record_social_manager_body(request, &mut |_| Ok(()))
}

#[cfg(test)]
pub(crate) fn record_social_manager_with_hook(
	request: &SocialRecordManagerRequest,
	mut hook: impl FnMut(SocialRecordHookPoint) -> Result<()>,
) -> Result<SocialRecordManagerReport> {
	record_social_manager_body(request, &mut |point| hook(point.into()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HookPoint {
	Locked,
	AuthoritativeWritten,
}

#[cfg(test)]
impl From<HookPoint> for SocialRecordHookPoint {
	fn from(value: HookPoint) -> Self {
		match value {
			HookPoint::Locked => Self::Locked,
			HookPoint::AuthoritativeWritten => Self::AuthoritativeWritten,
		}
	}
}

fn record_social_manager_body(
	request: &SocialRecordManagerRequest,
	hook: &mut impl FnMut(HookPoint) -> Result<()>,
) -> Result<SocialRecordManagerReport> {
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
	let strategies_dir = crate::resolve_against(&root, &request.strategies_dir);
	let reservations_dir = crate::resolve_against(&root, &request.reservations_dir);
	let posts_dir = crate::resolve_against(&root, &request.posts_dir);
	let outcomes_dir = crate::resolve_against(&root, &request.outcomes_dir);
	let _state_lock = crate::social_publish::scan::acquire_social_state_lock(&request.locks_dir)?;
	hook(HookPoint::Locked)?;

	let staging = PinnedPrivateJsonFile::open(&staging_path, MAX_STAGING_BYTES)?;
	let schema = staging
		.payload
		.get("schema")
		.and_then(Value::as_str)
		.ok_or_else(|| eyre::eyre!("staging artifact schema is required"))?;
	let (kind, destination_dir) = match schema {
		SOCIAL_CANDIDATE_SCHEMA => ("candidate", &candidates_dir),
		SOCIAL_STRATEGY_SCHEMA => ("strategy", &strategies_dir),
		_ => eyre::bail!(
			"Content Manager staging accepts only {SOCIAL_CANDIDATE_SCHEMA} or \
			 {SOCIAL_STRATEGY_SCHEMA}"
		),
	};
	let destination = destination_dir.join(format!("{}.json", request.run_id));
	let candidate_destination = candidates_dir.join(format!("{}.json", request.run_id));
	let strategy_destination = strategies_dir.join(format!("{}.json", request.run_id));

	crate::validate_generated_social_artifact(&staging.payload)
		.map_err(|error| eyre::eyre!("staging artifact failed validation: {error}"))?;
	if schema == SOCIAL_CANDIDATE_SCHEMA {
		crate::social_evidence::validate_internal_evidence_files(&staging.payload)
			.map_err(|error| eyre::eyre!("candidate evidence failed validation: {error}"))?;
	}

	let candidate_existing = load_optional_json(&candidate_destination)?;
	let strategy_existing = load_optional_json(&strategy_destination)?;
	let (target_existing, other_existing) = if schema == SOCIAL_CANDIDATE_SCHEMA {
		(candidate_existing.as_ref(), strategy_existing.as_ref())
	} else {
		(strategy_existing.as_ref(), candidate_existing.as_ref())
	};
	if other_existing.is_some() {
		eyre::bail!("run_id already owns a different Content Manager effect");
	}
	if let Some(existing) = target_existing {
		if existing != &staging.payload {
			eyre::bail!("refusing to overwrite an existing Content Manager effect");
		}
		validate_state_snapshot(
			&candidates_dir,
			&strategies_dir,
			&reservations_dir,
			&posts_dir,
			&outcomes_dir,
			None,
		)?;
		staging.unlink()?;

		return Ok(SocialRecordManagerReport {
			status: "already_recorded".into(),
			kind: kind.into(),
			run_id: request.run_id.clone(),
			path: crate::path_arg(&root, &destination),
			staging_cleaned: true,
		});
	}

	if schema == SOCIAL_CANDIDATE_SCHEMA {
		require_no_candidate_backpressure(&candidates_dir, &posts_dir)?;
	}
	validate_state_snapshot(
		&candidates_dir,
		&strategies_dir,
		&reservations_dir,
		&posts_dir,
		&outcomes_dir,
		Some((&destination, &staging.payload)),
	)?;

	crate::write_new_json(&destination, &staging.payload)?;
	hook(HookPoint::AuthoritativeWritten)?;
	let postvalidation = (|| {
		let recorded = crate::load_json(&destination)?;
		if recorded != staging.payload {
			eyre::bail!("authoritative Content Manager effect changed after write");
		}
		validate_state_snapshot(
			&candidates_dir,
			&strategies_dir,
			&reservations_dir,
			&posts_dir,
			&outcomes_dir,
			None,
		)
	})();
	if let Err(error) = postvalidation {
		let cleanup = PinnedPrivateJsonFile::open(&destination, MAX_STAGING_BYTES)
			.and_then(PinnedPrivateJsonFile::unlink);
		if cleanup.is_err() {
			return Err(eyre::eyre!(
				"Content Manager postvalidation failed and rollback was not safe: {error}"
			));
		}

		return Err(error);
	}
	staging.unlink()?;

	Ok(SocialRecordManagerReport {
		status: "recorded".into(),
		kind: kind.into(),
		run_id: request.run_id.clone(),
		path: crate::path_arg(&root, &destination),
		staging_cleaned: true,
	})
}

struct RadarSources {
	review_ref: String,
	impact_ref: String,
	queue: Map<String, Value>,
	review: Map<String, Value>,
	impact: Map<String, Value>,
	queue_sha256: String,
	review_sha256: String,
	impact_sha256: String,
}

struct RadarReviewIdentity<'a> {
	repo: &'a str,
	slug: &'a str,
	upstream_head: &'a str,
	subject_kind: &'a str,
	subject_id: &'a str,
	commit_shas: Vec<String>,
}

struct RadarPairSource {
	cache_root: PathBuf,
	digest: String,
}

pub(crate) fn validate_candidate_eligibility(candidate: &Value) -> Result<()> {
	if candidate.get("schema").and_then(Value::as_str) != Some(SOCIAL_CANDIDATE_SCHEMA) {
		return Ok(());
	}
	validate_optional_candidate_pair_paths(candidate)?;
	let Some(receipt) = candidate_eligibility_receipt(candidate)? else {
		return Ok(());
	};
	validate_eligibility_receipt_contract(receipt)?;

	let sources = load_radar_sources(candidate, receipt)?;
	let identity = radar_review_identity(&sources.review)?;
	validate_receipt_and_candidate(candidate, receipt, &identity)?;
	validate_queue_lineage(&sources.queue, &identity)?;
	validate_impact_lineage(candidate, &sources, &identity)?;
	validate_candidate_source_bindings(candidate, &sources)?;
	validate_lineage_digest(receipt, &sources, &identity)
}

fn validate_optional_candidate_pair_paths(candidate: &Value) -> Result<()> {
	let Some(source_refs) = candidate.get("source_refs").and_then(Value::as_object) else {
		return Ok(());
	};
	let reviews = source_refs.get("upstream_reviews").and_then(Value::as_array);
	let impacts = source_refs.get("upstream_impacts").and_then(Value::as_array);
	let has_pair_ref = reviews.is_some_and(|values| !values.is_empty())
		|| impacts.is_some_and(|values| !values.is_empty());
	if !has_pair_ref {
		return Ok(());
	}
	let review = reviews
		.filter(|values| values.len() == 1)
		.and_then(|values| values[0].as_str())
		.ok_or_else(|| {
		eyre::eyre!("source_refs.upstream_reviews must contain one strict Radar pair path")
	})?;
	let impact = impacts
		.filter(|values| values.len() == 1)
		.and_then(|values| values[0].as_str())
		.ok_or_else(|| {
		eyre::eyre!("source_refs.upstream_impacts must contain one strict Radar pair path")
	})?;
	let _ = validate_radar_pair_sources(review, impact)?;

	Ok(())
}

pub(crate) fn publication_lineage_sha256(candidate: &Value) -> Result<String> {
	let receipt = candidate_eligibility_receipt(candidate)?
		.ok_or_else(|| eyre::eyre!("publish candidate requires radar_eligibility"))?;
	let repo = required_string(receipt, "repo", "radar_eligibility repo")?;
	let subject_kind = required_string(receipt, "subject_kind", "radar_eligibility subject_kind")?;
	let subject_id = required_string(receipt, "subject_id", "radar_eligibility subject_id")?;
	let mut digest = Sha256::new();
	digest.update(b"decodex-radar-publication-lineage-v1");
	for (name, value) in
		[("repo", repo), ("subject_kind", subject_kind), ("subject_id", subject_id)]
	{
		update_digest_field(&mut digest, name, value);
	}

	Ok(hex_digest(digest.finalize()))
}

pub(crate) fn publication_idempotency_key(candidate: &Value) -> Result<String> {
	Ok(format!("radar-publication:{}", publication_lineage_sha256(candidate)?))
}

pub(crate) fn validate_publication_identity(candidate: &Value) -> Result<()> {
	if candidate.get("schema").and_then(Value::as_str) != Some(SOCIAL_CANDIDATE_SCHEMA) {
		return Ok(());
	}
	let decision = candidate
		.get("decision")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("candidate decision is required"))?;
	if decision.get("worthiness").and_then(Value::as_str) != Some("publish") {
		return Ok(());
	}
	let actual = required_string(decision, "idempotency_key", "candidate idempotency_key")?;
	let expected = publication_idempotency_key(candidate)?;
	if actual != expected {
		eyre::bail!(
			"publish candidate idempotency_key must be derived from its immutable Radar subject"
		);
	}

	Ok(())
}

fn candidate_eligibility_receipt(candidate: &Value) -> Result<Option<&Map<String, Value>>> {
	let worthiness = candidate
		.get("decision")
		.and_then(Value::as_object)
		.and_then(|decision| decision.get("worthiness"))
		.and_then(Value::as_str);
	let Some(receipt) = candidate.get("radar_eligibility") else {
		if worthiness == Some("publish") {
			eyre::bail!("publish candidate requires radar_eligibility");
		}

		return Ok(None);
	};
	object(receipt, "radar_eligibility").map(Some)
}

fn validate_eligibility_receipt_contract(receipt: &Map<String, Value>) -> Result<()> {
	require_exact_keys(
		receipt,
		"radar_eligibility",
		&[
			"commit_shas",
			"impact_sha256",
			"lineage_sha256",
			"queue_sha256",
			"repo",
			"review_sha256",
			"schema",
			"slug",
			"subject_id",
			"subject_kind",
			"upstream_head",
		],
	)?;
	require_equal(
		required_string(receipt, "schema", "radar_eligibility schema")?,
		RADAR_ELIGIBILITY_SCHEMA,
		"radar_eligibility schema",
	)
}

fn load_radar_sources(candidate: &Value, receipt: &Map<String, Value>) -> Result<RadarSources> {
	let source_refs = object(
		candidate
			.get("radar_source_refs")
			.ok_or_else(|| eyre::eyre!("radar_source_refs is required"))?,
		"radar_source_refs",
	)?;
	require_exact_keys(source_refs, "radar_source_refs", &["impact", "queue", "review"])?;
	let queue_ref = required_string(source_refs, "queue", "radar queue source")?;
	let review_ref = required_string(source_refs, "review", "radar review source")?;
	let impact_ref = required_string(source_refs, "impact", "radar impact source")?;
	let queue_root = validate_radar_queue_source_path(queue_ref)?;
	let pair = validate_radar_pair_sources(review_ref, impact_ref)?;
	if queue_root != pair.cache_root {
		eyre::bail!("Radar queue, review, and impact sources must use one private cache root");
	}

	let root = crate::repo_root()?;
	let (queue, queue_sha256) = crate::load_json_with_sha256(&root.join(queue_ref))?;
	let (review, review_raw, review_sha256) =
		crate::load_json_bytes_with_sha256(&root.join(review_ref))?;
	let (impact, impact_raw, impact_sha256) =
		crate::load_json_bytes_with_sha256(&root.join(impact_ref))?;
	require_equal(
		&pair.digest,
		&radar_content_pair_sha256(&review_raw, &impact_raw),
		"Radar content-review pair digest",
	)?;
	require_equal(
		required_string(receipt, "queue_sha256", "radar queue digest")?,
		&queue_sha256,
		"radar queue digest",
	)?;
	require_equal(
		required_string(receipt, "review_sha256", "radar review digest")?,
		&review_sha256,
		"radar review digest",
	)?;
	require_equal(
		required_string(receipt, "impact_sha256", "radar impact digest")?,
		&impact_sha256,
		"radar impact digest",
	)?;

	let queue = owned_object(queue, "Radar queue")?;
	let review = owned_object(review, "Radar review")?;
	let impact = owned_object(impact, "Radar impact")?;
	require_schema(&queue, RADAR_QUEUE_SCHEMA, "Radar queue")?;
	require_schema(&review, RADAR_REVIEW_SCHEMA, "Radar review")?;
	require_schema(&impact, RADAR_IMPACT_SCHEMA, "Radar impact")?;
	let now = OffsetDateTime::now_utc();
	require_fresh(&queue, "generated_at", "Radar queue", now)?;
	require_fresh(&review, "reviewed_at", "Radar review", now)?;
	require_fresh(&impact, "reviewed_at", "Radar impact", now)?;

	Ok(RadarSources {
		review_ref: review_ref.to_owned(),
		impact_ref: impact_ref.to_owned(),
		queue,
		review,
		impact,
		queue_sha256,
		review_sha256,
		impact_sha256,
	})
}

fn radar_review_identity(review: &Map<String, Value>) -> Result<RadarReviewIdentity<'_>> {
	let repo = required_string(review, "repo", "Radar review repo")?;
	let slug = required_string(review, "slug", "Radar review slug")?;
	let upstream_head = required_string(review, "upstream_head", "Radar review upstream head")?;
	let subject = object(
		review.get("subject").ok_or_else(|| eyre::eyre!("Radar review subject is required"))?,
		"Radar review subject",
	)?;
	let subject_kind = required_string(subject, "subject_kind", "Radar review subject kind")?;
	let subject_id = required_string(subject, "subject_id", "Radar review subject id")?;
	let commit_shas = normalized_commit_shas(subject, "Radar review commit_shas")?;

	Ok(RadarReviewIdentity { repo, slug, upstream_head, subject_kind, subject_id, commit_shas })
}

fn validate_receipt_and_candidate(
	candidate: &Value,
	receipt: &Map<String, Value>,
	identity: &RadarReviewIdentity<'_>,
) -> Result<()> {
	for (field, expected) in [
		("repo", identity.repo),
		("slug", identity.slug),
		("subject_kind", identity.subject_kind),
		("subject_id", identity.subject_id),
		("upstream_head", identity.upstream_head),
	] {
		require_equal(
			required_string(receipt, field, &format!("radar_eligibility {field}"))?,
			expected,
			&format!("radar_eligibility {field}"),
		)?;
	}
	if normalized_commit_shas(receipt, "radar_eligibility commit_shas")? != identity.commit_shas {
		eyre::bail!("radar_eligibility commit_shas do not match the review");
	}
	require_equal(
		candidate.get("repo").and_then(Value::as_str).unwrap_or_default(),
		identity.repo,
		"candidate repo",
	)?;
	require_equal(
		candidate.get("slug").and_then(Value::as_str).unwrap_or_default(),
		identity.slug,
		"candidate slug",
	)
}

fn validate_queue_lineage(
	queue: &Map<String, Value>,
	identity: &RadarReviewIdentity<'_>,
) -> Result<()> {
	require_equal(
		required_string(queue, "repo", "Radar queue repo")?,
		identity.repo,
		"Radar queue repo",
	)?;
	let queue_source = object(
		queue.get("source").ok_or_else(|| eyre::eyre!("Radar queue source is required"))?,
		"Radar queue source",
	)?;
	require_equal(
		required_string(queue_source, "upstream_head", "Radar queue upstream head")?,
		identity.upstream_head,
		"Radar queue upstream head",
	)?;
	let queue_subject = matching_queue_subject(queue, identity.subject_kind, identity.subject_id)?;
	if normalized_commit_shas(queue_subject, "Radar queue commit_shas")? != identity.commit_shas {
		eyre::bail!("Radar queue commit_shas do not match the review");
	}

	Ok(())
}

fn validate_impact_lineage(
	candidate: &Value,
	sources: &RadarSources,
	identity: &RadarReviewIdentity<'_>,
) -> Result<()> {
	let impact = &sources.impact;
	require_equal(
		required_string(impact, "repo", "Radar impact repo")?,
		identity.repo,
		"Radar impact repo",
	)?;
	require_equal(
		required_string(impact, "slug", "Radar impact slug")?,
		identity.slug,
		"Radar impact slug",
	)?;
	let impact_lineage = object(
		impact
			.get("review_lineage")
			.ok_or_else(|| eyre::eyre!("Radar impact review_lineage is required"))?,
		"Radar impact review_lineage",
	)?;
	for (field, expected) in [
		("slug", identity.slug),
		("subject_kind", identity.subject_kind),
		("subject_id", identity.subject_id),
		("upstream_head", identity.upstream_head),
	] {
		require_equal(
			required_string(impact_lineage, field, &format!("Radar impact {field}"))?,
			expected,
			&format!("Radar impact {field}"),
		)?;
	}
	if normalized_commit_shas(impact_lineage, "Radar impact commit_shas")? != identity.commit_shas {
		eyre::bail!("Radar impact commit_shas do not match the review");
	}
	require_equal(
		required_string(impact_lineage, "artifact_sha256", "Radar impact review artifact digest")?,
		&sources.review_sha256,
		"Radar impact review artifact digest",
	)?;
	require_equal(
		required_string(impact, "public_signal_decision", "Radar impact decision")?,
		"publish",
		"Radar impact decision",
	)?;
	let publisher_angle =
		required_string(impact, "publisher_angle", "Radar impact publisher angle")?;
	if publisher_angle == "none" {
		eyre::bail!("Radar impact publisher angle must be publishable");
	}
	require_equal(
		candidate.get("mode").and_then(Value::as_str).unwrap_or_default(),
		publisher_angle,
		"candidate mode",
	)?;
	if !review_requests_impact(&sources.review) {
		eyre::bail!("Radar review does not request an upstream impact");
	}

	Ok(())
}

fn validate_candidate_source_bindings(candidate: &Value, sources: &RadarSources) -> Result<()> {
	let source_refs = object(
		candidate
			.get("source_refs")
			.ok_or_else(|| eyre::eyre!("candidate source_refs are required"))?,
		"candidate source_refs",
	)?;
	require_single_ref(source_refs, "upstream_reviews", &sources.review_ref)?;
	require_single_ref(source_refs, "upstream_impacts", &sources.impact_ref)?;
	let evidence_digests = object(
		candidate
			.get("evidence_digests")
			.ok_or_else(|| eyre::eyre!("candidate evidence_digests are required"))?,
		"candidate evidence_digests",
	)?;
	require_equal(
		evidence_digests.get(&sources.review_ref).and_then(Value::as_str).unwrap_or_default(),
		&sources.review_sha256,
		"candidate review evidence digest",
	)?;
	require_equal(
		evidence_digests.get(&sources.impact_ref).and_then(Value::as_str).unwrap_or_default(),
		&sources.impact_sha256,
		"candidate impact evidence digest",
	)
}

fn validate_lineage_digest(
	receipt: &Map<String, Value>,
	sources: &RadarSources,
	identity: &RadarReviewIdentity<'_>,
) -> Result<()> {
	let expected_lineage = eligibility_lineage_sha256(
		identity.repo,
		identity.subject_kind,
		identity.subject_id,
		identity.slug,
		identity.upstream_head,
		&identity.commit_shas,
		&sources.queue_sha256,
		&sources.review_sha256,
		&sources.impact_sha256,
	);
	require_equal(
		required_string(receipt, "lineage_sha256", "radar lineage digest")?,
		&expected_lineage,
		"radar lineage digest",
	)
}

fn owned_object(value: Value, label: &str) -> Result<Map<String, Value>> {
	match value {
		Value::Object(object) => Ok(object),
		_ => eyre::bail!("{label} must be an object"),
	}
}

fn validate_state_snapshot(
	candidates_dir: &Path,
	strategies_dir: &Path,
	reservations_dir: &Path,
	posts_dir: &Path,
	outcomes_dir: &Path,
	injected: Option<(&Path, &Value)>,
) -> Result<()> {
	let mut files = crate::collect_json_files(&[
		candidates_dir.to_path_buf(),
		strategies_dir.to_path_buf(),
		reservations_dir.to_path_buf(),
		posts_dir.to_path_buf(),
		outcomes_dir.to_path_buf(),
	])?;
	files.sort();
	let injected_path = injected.map(|(path, _)| path);
	let mut state = SocialValidationState::new();
	let mut errors = Vec::new();

	for path in files {
		if injected_path == Some(path.as_path()) {
			continue;
		}
		let payload = crate::load_json(&path)?;
		let validation =
			crate::social_validation::validate_social_artifact_for_path(&path, &payload);
		for error in validation.errors {
			errors.push(format!("{}: {error}", path.display()));
		}
		crate::social_validation::validate_social_cross_file_constraints(
			&path,
			&payload,
			&mut state,
			&mut errors,
		);
	}
	if let Some((path, payload)) = injected {
		let validation = crate::social_validation::validate_social_artifact_for_path(path, payload);
		for error in validation.errors {
			errors.push(format!("{}: {error}", path.display()));
		}
		crate::social_validation::validate_social_cross_file_constraints(
			path,
			payload,
			&mut state,
			&mut errors,
		);
	}
	state.finish(&mut errors);
	if !errors.is_empty() {
		eyre::bail!("Content Manager state validation failed:\n- {}", errors.join("\n- "));
	}

	Ok(())
}

fn require_no_candidate_backpressure(candidates_dir: &Path, posts_dir: &Path) -> Result<()> {
	let root = crate::repo_root()?;
	let candidates = crate::collect_json_files(&[candidates_dir.to_path_buf()])?;
	if candidates.is_empty() {
		return Ok(());
	}
	let mut terminal = BTreeSet::new();
	for post_path in crate::collect_json_files(&[posts_dir.to_path_buf()])? {
		let post = crate::load_json(&post_path)?;
		if post.get("schema").and_then(Value::as_str) != Some(SOCIAL_POST_SCHEMA)
			|| !matches!(
				post.get("status").and_then(Value::as_str),
				Some("blocked" | "published" | "skipped")
			) {
			continue;
		}
		for reference in post
			.get("source_refs")
			.and_then(Value::as_object)
			.and_then(|refs| refs.get("social_candidates"))
			.and_then(Value::as_array)
			.into_iter()
			.flatten()
			.filter_map(Value::as_str)
		{
			terminal.insert(reference.to_owned());
		}
	}
	let unresolved = candidates
		.iter()
		.map(|path| crate::path_arg(&root, path))
		.filter(|reference| !terminal.contains(reference))
		.count();
	if unresolved > 0 {
		eyre::bail!("candidate backpressure is active: {unresolved} unresolved candidate(s)");
	}

	Ok(())
}

fn load_optional_json(path: &Path) -> Result<Option<Value>> {
	match fs::symlink_metadata(path) {
		Ok(_) => crate::load_json(path).map(Some),
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(error) => Err(error.into()),
	}
}

fn validate_radar_queue_source_path(reference: &str) -> Result<PathBuf> {
	let path = Path::new(reference);
	if path.is_absolute()
		|| path.components().any(|component| !matches!(component, Component::Normal(_)))
	{
		eyre::bail!("radar queue source must be the exact canonical private Radar queue path");
	}
	if path == Path::new(RADAR_QUEUE_PATH) {
		return Ok(PathBuf::from(RADAR_CACHE_ROOT));
	}
	#[cfg(test)]
	{
		let parts = path.iter().collect::<Vec<_>>();
		let suffix = [
			OsStr::new("github"),
			OsStr::new("review-queue"),
			OsStr::new("openai-codex-latest.json"),
		];
		if path.starts_with("target")
			&& let Some(index) = parts.windows(suffix.len()).position(|parts| parts == suffix)
			&& index + suffix.len() == parts.len()
		{
			return Ok(parts[..index].iter().collect());
		}
	}
	eyre::bail!("radar queue source must be the exact canonical private Radar queue path");
}

fn validate_radar_pair_sources(review_ref: &str, impact_ref: &str) -> Result<RadarPairSource> {
	let (review_root, review_pair, review_digest) =
		radar_pair_source(review_ref, "review.json", "review")?;
	let (impact_root, impact_pair, impact_digest) =
		radar_pair_source(impact_ref, "impact.json", "impact")?;
	if review_root != impact_root || review_pair != impact_pair || review_digest != impact_digest {
		eyre::bail!(
			"Radar review and impact sources must share one canonical content-review pair directory"
		);
	}

	Ok(RadarPairSource { cache_root: review_root, digest: review_digest })
}

fn radar_pair_source(
	reference: &str,
	expected_file: &str,
	label: &str,
) -> Result<(PathBuf, String, String)> {
	let path = Path::new(reference);
	if path.is_absolute()
		|| path.components().any(|component| !matches!(component, Component::Normal(_)))
		|| path.file_name().and_then(OsStr::to_str) != Some(expected_file)
	{
		eyre::bail!("radar {label} source must be a canonical private Radar pair path");
	}
	let parts = path.iter().collect::<Vec<_>>();
	let pair_root_index = if path.starts_with(RADAR_PAIR_PREFIX) {
		Path::new(RADAR_PAIR_PREFIX).components().count()
	} else {
		#[cfg(test)]
		{
			parts
				.windows(2)
				.position(|parts| {
					parts[0] == OsStr::new("github")
						&& parts[1] == OsStr::new("content-review-pairs")
				})
				.map(|index| index + 2)
				.ok_or_else(|| {
					eyre::eyre!("radar {label} source must be a canonical private Radar pair path")
				})?
		}
		#[cfg(not(test))]
		{
			eyre::bail!("radar {label} source must be a canonical private Radar pair path");
		}
	};
	if parts.len() != pair_root_index + 2 {
		eyre::bail!("radar {label} source must be a canonical private Radar pair path");
	}
	let pair = parts[pair_root_index]
		.to_str()
		.ok_or_else(|| eyre::eyre!("Radar content-review pair directory is not UTF-8"))?;
	let mut pair_parts = pair.split("--");
	let run_id = pair_parts.next().unwrap_or_default();
	let staging_sha256 = pair_parts.next().unwrap_or_default();
	let pair_sha256 = pair_parts.next().unwrap_or_default();
	if pair_parts.next().is_some()
		|| !crate::social_publish::valid_run_id(run_id)
		|| !lowercase_sha256(staging_sha256)
		|| !lowercase_sha256(pair_sha256)
	{
		eyre::bail!("Radar content-review pair directory is malformed");
	}
	let cache_root = if path.starts_with(RADAR_PAIR_PREFIX) {
		PathBuf::from(RADAR_CACHE_ROOT)
	} else {
		parts[..pair_root_index - 2].iter().collect()
	};

	Ok((cache_root, pair.to_owned(), pair_sha256.to_owned()))
}

pub(crate) fn radar_content_pair_sha256(review_raw: &[u8], impact_raw: &[u8]) -> String {
	let mut digest = Sha256::new();
	digest.update(b"radar-content-review-pair-v1");
	for payload in [review_raw, impact_raw] {
		digest.update(u64::try_from(payload.len()).unwrap_or(u64::MAX).to_be_bytes());
		digest.update(payload);
	}
	hex_digest(digest.finalize())
}

fn lowercase_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn require_schema(object: &Map<String, Value>, expected: &str, label: &str) -> Result<()> {
	require_equal(
		required_string(object, "schema", &format!("{label} schema"))?,
		expected,
		&format!("{label} schema"),
	)
}

fn require_fresh(
	object: &Map<String, Value>,
	field: &str,
	label: &str,
	now: OffsetDateTime,
) -> Result<()> {
	let timestamp =
		required_string(object, field, &format!("{label} {field}")).and_then(|value| {
			OffsetDateTime::parse(value, &Rfc3339)
				.map_err(|_| eyre::eyre!("{label} {field} must be an RFC3339 timestamp"))
		})?;
	if timestamp > now + MAX_FUTURE_SKEW {
		eyre::bail!("{label} is from the future");
	}
	if now - timestamp > MAX_RADAR_AGE {
		eyre::bail!("{label} is older than the 12-hour eligibility window");
	}

	Ok(())
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>> {
	value.as_object().ok_or_else(|| eyre::eyre!("{label} must be an object"))
}

fn required_string<'a>(
	object: &'a Map<String, Value>,
	field: &str,
	label: &str,
) -> Result<&'a str> {
	object
		.get(field)
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.ok_or_else(|| eyre::eyre!("{label} must be a non-empty string"))
}

fn require_equal(actual: &str, expected: &str, label: &str) -> Result<()> {
	if actual != expected {
		eyre::bail!("{label} does not match the verified Radar lineage");
	}

	Ok(())
}

fn require_exact_keys(object: &Map<String, Value>, label: &str, allowed: &[&str]) -> Result<()> {
	let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
	let expected = allowed.iter().copied().collect::<BTreeSet<_>>();
	if actual != expected {
		eyre::bail!("{label} must contain exactly the reviewed contract fields");
	}

	Ok(())
}

fn normalized_commit_shas(object: &Map<String, Value>, label: &str) -> Result<Vec<String>> {
	let commits = object
		.get("commit_shas")
		.and_then(Value::as_array)
		.filter(|values| !values.is_empty())
		.ok_or_else(|| eyre::eyre!("{label} must be a non-empty list"))?
		.iter()
		.map(|value| {
			value
				.as_str()
				.filter(|value| valid_git_oid(value))
				.map(str::to_ascii_lowercase)
				.ok_or_else(|| eyre::eyre!("{label} must contain Git object IDs"))
		})
		.collect::<Result<Vec<_>>>()?;
	let mut normalized = commits.clone();
	normalized.sort();
	normalized.dedup();
	if normalized != commits {
		eyre::bail!("{label} must be unique and lexicographically sorted");
	}

	Ok(normalized)
}

fn valid_git_oid(value: &str) -> bool {
	matches!(value.len(), 40 | 64)
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn matching_queue_subject<'a>(
	queue: &'a Map<String, Value>,
	subject_kind: &str,
	subject_id: &str,
) -> Result<&'a Map<String, Value>> {
	queue
		.get("subjects")
		.and_then(Value::as_array)
		.and_then(|subjects| {
			subjects.iter().find_map(|subject| {
				let subject = subject.as_object()?;
				(subject.get("subject_kind").and_then(Value::as_str) == Some(subject_kind)
					&& subject.get("subject_id").and_then(Value::as_str) == Some(subject_id))
				.then_some(subject)
			})
		})
		.ok_or_else(|| eyre::eyre!("Radar queue does not contain the reviewed subject"))
}

fn review_requests_impact(review: &Map<String, Value>) -> bool {
	review.get("next_actions").and_then(Value::as_array).is_some_and(|actions| {
		actions
			.iter()
			.any(|action| action.get("type").and_then(Value::as_str) == Some("upstream_impact"))
	})
}

fn require_single_ref(source_refs: &Map<String, Value>, field: &str, expected: &str) -> Result<()> {
	let values = source_refs
		.get(field)
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("source_refs.{field} must be a list"))?;
	if values.len() != 1 || values.first().and_then(Value::as_str) != Some(expected) {
		eyre::bail!("source_refs.{field} must contain exactly the verified Radar source");
	}

	Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn eligibility_lineage_sha256(
	repo: &str,
	subject_kind: &str,
	subject_id: &str,
	slug: &str,
	upstream_head: &str,
	commit_shas: &[String],
	queue_sha256: &str,
	review_sha256: &str,
	impact_sha256: &str,
) -> String {
	let mut digest = Sha256::new();

	digest.update(b"radar-content-eligibility-lineage-v1");
	for (name, value) in [
		("schema", RADAR_ELIGIBILITY_SCHEMA),
		("repo", repo),
		("subject_kind", subject_kind),
		("subject_id", subject_id),
		("slug", slug),
		("upstream_head", upstream_head),
		("queue_sha256", queue_sha256),
		("review_sha256", review_sha256),
		("impact_sha256", impact_sha256),
	] {
		update_digest_field(&mut digest, name, value);
	}
	digest.update(u64::try_from(commit_shas.len()).unwrap_or(u64::MAX).to_be_bytes());
	for commit in commit_shas {
		update_digest_field(&mut digest, "commit_sha", commit);
	}

	hex_digest(digest.finalize())
}

fn update_digest_field(digest: &mut Sha256, name: &str, value: &str) {
	for bytes in [name.as_bytes(), value.as_bytes()] {
		digest.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
		digest.update(bytes);
	}
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
	digest.as_ref().iter().map(|byte| format!("{byte:02x}")).collect()
}
