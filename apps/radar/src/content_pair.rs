//! Atomic persistence and discovery of source-backed content-review pairs.

use std::{
	collections::{BTreeMap, BTreeSet},
	path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use crate::{
	RadarContentEligibilityRequest, RadarContentPairCommitReport, RadarContentPairCommitRequest,
	UPSTREAM_IMPACT_SCHEMA, UPSTREAM_REVIEW_SCHEMA,
	content_eligibility::ValidatedContentPair,
	prelude::{Result, eyre},
	private_fs::{PrivateEntryKind, RadarCacheLock},
};

pub(crate) const PAIRS_RELATIVE_PATH: &str = "github/content-review-pairs";
pub(crate) const STAGING_RELATIVE_PATH: &str = "github/content-review-staging";
const STAGING_SCHEMA: &str = "radar_content_review_pair_staging/v1";
const COMMIT_REPORT_SCHEMA: &str = "radar_content_review_pair_commit/v1";
const REVIEW_FILE: &str = "review.json";
const IMPACT_FILE: &str = "impact.json";
const STAGING_REVIEW_DIGEST_SENTINEL: &str =
	"0000000000000000000000000000000000000000000000000000000000000000";
const MAX_STAGING_BYTES: u64 = 256 * 1024;
const MAX_RUN_ID_CHARS: usize = 64;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StagingPair {
	schema: String,
	run_id: String,
	queue_sha256: String,
	review: Value,
	impact: Value,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(crate) struct SubjectLineage {
	pub(crate) repo: String,
	pub(crate) subject_kind: String,
	pub(crate) subject_id: String,
	pub(crate) commit_shas: Vec<String>,
}

pub(crate) fn handled_state_sha256(handled: &BTreeSet<SubjectLineage>) -> Result<String> {
	Ok(sha256_hex(&serde_json::to_vec(handled)?))
}

pub(crate) fn validate_committed_pair_directory(
	lock: &RadarCacheLock,
	directory: &Path,
) -> Result<()> {
	let name = directory
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("Radar committed pair directory name is invalid"))?;

	validate_pair_directory_name(name)?;
	let (review_raw, impact_raw) = read_pair_artifacts(lock, directory)?;

	validate_committed_pair_artifacts(&review_raw, &impact_raw)?;
	Ok(())
}

pub(crate) fn commit_content_pair(
	request: &RadarContentPairCommitRequest,
) -> Result<RadarContentPairCommitReport> {
	if request.max_age_hours == 0 {
		eyre::bail!("source freshness limit must be at least one hour");
	}
	let cache = crate::private_fs::PrivateCache::open_existing(&request.cache_root)?;
	let lock = cache.lock()?;
	let staging_relative = lock.relative_path(&request.staging)?;

	validate_staging_location(&staging_relative)?;
	let staging_identity = lock
		.cache()
		.metadata(&staging_relative)?
		.ok_or_else(|| eyre::eyre!("Radar content-review staging file does not exist"))?;
	let staging_raw = lock.read_bounded(&staging_relative, MAX_STAGING_BYTES)?;
	let current_identity = lock
		.cache()
		.metadata(&staging_relative)?
		.ok_or_else(|| eyre::eyre!("Radar content-review staging file disappeared"))?;
	if current_identity != staging_identity {
		eyre::bail!("Radar content-review staging identity changed during read");
	}
	let mut staging: StagingPair = serde_json::from_slice(&staging_raw)
		.map_err(|error| eyre::eyre!("Radar content-review staging JSON is invalid: {error}"))?;

	validate_staging(&staging, &staging_relative)?;
	let queue_relative = Path::new(crate::paths::REVIEW_QUEUE_RELATIVE_PATH);
	let queue_raw = lock.read(queue_relative)?;
	let queue_sha256 = sha256_hex(&queue_raw);
	if staging.queue_sha256 != queue_sha256 {
		eyre::bail!("Radar content-review staging queue_sha256 is not current");
	}

	let review_raw = pretty_json_bytes(&staging.review)?;
	materialize_impact_review_digest(&mut staging.impact, &review_raw)?;
	let impact_raw = pretty_json_bytes(&staging.impact)?;
	let staging_sha256 = sha256_hex(&staging_raw);
	let final_name = format!("{}--{}", staging.run_id, pair_sha256(&review_raw, &impact_raw));
	let final_relative = Path::new(PAIRS_RELATIVE_PATH).join(&final_name);
	let review_relative = final_relative.join(REVIEW_FILE);
	let impact_relative = final_relative.join(IMPACT_FILE);
	let validation_request = RadarContentEligibilityRequest {
		queue: request.cache_root.join(queue_relative),
		review: request.cache_root.join(&review_relative),
		impact: request.cache_root.join(&impact_relative),
		max_age_hours: request.max_age_hours,
	};
	let pair = crate::content_eligibility::validate_content_pair_raw(
		&validation_request,
		&queue_raw,
		&review_raw,
		&impact_raw,
	)?;

	reject_conflicting_run_or_subject(&lock, &staging.run_id, &final_name, &pair)?;
	let created = lock.create_directory_atomic(
		&final_relative,
		&[(REVIEW_FILE, &review_raw), (IMPACT_FILE, &impact_raw)],
	)?;
	let installed = read_committed_pair(&lock, &final_relative, &queue_raw, request.max_age_hours)?;
	if installed != pair {
		eyre::bail!("Radar committed content-review pair does not match the staging payload");
	}

	lock.remove_if_matches(&staging_relative, &staging_identity)?;

	Ok(RadarContentPairCommitReport {
		schema: COMMIT_REPORT_SCHEMA.to_owned(),
		status: if created { "committed" } else { "recovered" }.to_owned(),
		pair_dir: relative_ref(&final_relative),
		review_path: relative_ref(&review_relative),
		impact_path: relative_ref(&impact_relative),
		staging_sha256,
		review_sha256: pair.review_sha256,
		impact_sha256: pair.impact_sha256,
	})
}

pub(crate) fn handled_subjects(
	lock: &RadarCacheLock,
	queue_raw: &[u8],
) -> Result<BTreeSet<SubjectLineage>> {
	let mut handled = BTreeSet::new();
	let mut identities = BTreeMap::<(String, String, String, Vec<String>), SubjectLineage>::new();

	for directory in pair_directories(lock)? {
		let (review_raw, impact_raw) = read_pair_artifacts(lock, &directory)?;
		let lineage = validate_committed_pair_artifacts(&review_raw, &impact_raw)?;
		let key = (
			lineage.repo.clone(),
			lineage.subject_kind.clone(),
			lineage.subject_id.clone(),
			lineage.commit_shas.clone(),
		);

		if let Some(previous) = identities.insert(key, lineage.clone()) {
			if previous != lineage {
				eyre::bail!("Radar committed content-review handled state is ambiguous");
			}
			eyre::bail!("Radar committed content-review subject is duplicated");
		}
		if queue_contains_lineage(queue_raw, &lineage)? {
			handled.insert(lineage);
		}
	}

	Ok(handled)
}

fn reject_conflicting_run_or_subject(
	lock: &RadarCacheLock,
	run_id: &str,
	final_name: &str,
	pair: &ValidatedContentPair,
) -> Result<()> {
	for directory in pair_directories(lock)? {
		let name = directory
			.file_name()
			.and_then(|name| name.to_str())
			.ok_or_else(|| eyre::eyre!("Radar committed pair directory name is invalid"))?;
		let (review_raw, impact_raw) = read_pair_artifacts(lock, &directory)?;
		let existing = validate_committed_pair_artifacts(&review_raw, &impact_raw)?;

		if name.starts_with(&format!("{run_id}--")) && name != final_name {
			eyre::bail!("Radar content-review run_id already has a conflicting committed payload");
		}
		if name != final_name
			&& existing.repo == pair.repo
			&& existing.subject_kind == pair.subject_kind
			&& existing.subject_id == pair.subject_id
			&& existing.commit_shas == pair.commit_shas
		{
			eyre::bail!("Radar content-review subject already has a committed pair");
		}
	}

	Ok(())
}

fn pair_directories(lock: &RadarCacheLock) -> Result<Vec<PathBuf>> {
	let mut directories = Vec::new();

	for entry in lock.cache().entries_if_present(Path::new(PAIRS_RELATIVE_PATH))? {
		if entry.kind != PrivateEntryKind::Directory {
			eyre::bail!("Radar committed content-review root contains a non-directory entry");
		}
		let name = entry
			.name
			.to_str()
			.ok_or_else(|| eyre::eyre!("Radar committed pair directory name is not UTF-8"))?;

		validate_pair_directory_name(name)?;
		directories.push(Path::new(PAIRS_RELATIVE_PATH).join(name));
	}

	Ok(directories)
}

fn read_committed_pair(
	lock: &RadarCacheLock,
	directory: &Path,
	queue_raw: &[u8],
	max_age_hours: u64,
) -> Result<ValidatedContentPair> {
	let (review_raw, impact_raw) = read_pair_artifacts(lock, directory)?;
	let request = RadarContentEligibilityRequest {
		queue: lock.cache().root_path().join(crate::paths::REVIEW_QUEUE_RELATIVE_PATH),
		review: lock.cache().root_path().join(directory).join(REVIEW_FILE),
		impact: lock.cache().root_path().join(directory).join(IMPACT_FILE),
		max_age_hours,
	};

	crate::content_eligibility::validate_content_pair_raw(
		&request,
		queue_raw,
		&review_raw,
		&impact_raw,
	)
}

fn read_pair_artifacts(lock: &RadarCacheLock, directory: &Path) -> Result<(Vec<u8>, Vec<u8>)> {
	let entries = lock.cache().entries(directory)?;
	let names = entries
		.iter()
		.map(|entry| {
			if entry.kind != PrivateEntryKind::File {
				eyre::bail!("Radar committed content-review pair contains a nested directory");
			}
			entry
				.name
				.to_str()
				.map(ToOwned::to_owned)
				.ok_or_else(|| eyre::eyre!("Radar committed pair file name is not UTF-8"))
		})
		.collect::<Result<BTreeSet<_>>>()?;
	let expected = BTreeSet::from([IMPACT_FILE.to_owned(), REVIEW_FILE.to_owned()]);

	if names != expected {
		eyre::bail!("Radar committed content-review pair must contain exactly two artifacts");
	}
	let review_relative = directory.join(REVIEW_FILE);
	let impact_relative = directory.join(IMPACT_FILE);
	let review_raw = lock.read_bounded(&review_relative, MAX_STAGING_BYTES)?;
	let impact_raw = lock.read_bounded(&impact_relative, MAX_STAGING_BYTES)?;
	let expected_digest = directory
		.file_name()
		.and_then(|name| name.to_str())
		.and_then(|name| name.rsplit_once("--"))
		.map(|(_, digest)| digest)
		.ok_or_else(|| eyre::eyre!("Radar committed pair directory name is malformed"))?;

	if pair_sha256(&review_raw, &impact_raw) != expected_digest {
		eyre::bail!("Radar committed content-review pair digest does not match its directory");
	}

	Ok((review_raw, impact_raw))
}

fn validate_staging(staging: &StagingPair, relative: &Path) -> Result<()> {
	if staging.schema != STAGING_SCHEMA {
		eyre::bail!("Radar content-review staging schema must be {STAGING_SCHEMA}");
	}
	validate_run_id(&staging.run_id)?;
	let expected = Path::new(STAGING_RELATIVE_PATH).join(format!("{}.json", staging.run_id));
	if relative != expected {
		eyre::bail!("Radar content-review staging path must match its run_id");
	}
	if staging.queue_sha256.len() != 64
		|| !staging.queue_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
		|| staging.queue_sha256.bytes().any(|byte| byte.is_ascii_uppercase())
	{
		eyre::bail!("Radar content-review staging queue_sha256 must be lowercase SHA-256");
	}
	for (label, schema, payload) in [
		("Staged upstream review", UPSTREAM_REVIEW_SCHEMA, &staging.review),
		("Staged upstream impact", UPSTREAM_IMPACT_SCHEMA, &staging.impact),
	] {
		crate::validate_expected_schema(payload, schema, label)?;
		let errors = crate::validate_artifact_errors(payload);

		if !errors.is_empty() {
			eyre::bail!("{label} validation failed:\n- {}", errors.join("\n- "));
		}
	}
	let artifact_sha256 = staging
		.impact
		.get("review_lineage")
		.and_then(Value::as_object)
		.and_then(|lineage| lineage.get("artifact_sha256"))
		.and_then(Value::as_str)
		.ok_or_else(|| {
			eyre::eyre!(
				"Staged upstream impact review_lineage.artifact_sha256 must use the \
				 non-authoritative sentinel"
			)
		})?;
	if artifact_sha256 != STAGING_REVIEW_DIGEST_SENTINEL {
		eyre::bail!(
			"Staged upstream impact review_lineage.artifact_sha256 must use the \
			 non-authoritative sentinel"
		);
	}

	Ok(())
}

fn materialize_impact_review_digest(impact: &mut Value, review_raw: &[u8]) -> Result<()> {
	let lineage = impact
		.get_mut("review_lineage")
		.and_then(Value::as_object_mut)
		.ok_or_else(|| eyre::eyre!("Staged upstream impact review_lineage must be an object"))?;
	let artifact_sha256 = lineage
		.get_mut("artifact_sha256")
		.ok_or_else(|| eyre::eyre!("Staged upstream impact review digest sentinel is missing"))?;

	if artifact_sha256.as_str() != Some(STAGING_REVIEW_DIGEST_SENTINEL) {
		eyre::bail!("Staged upstream impact review digest sentinel changed before materialization");
	}
	*artifact_sha256 = Value::String(sha256_hex(review_raw));

	Ok(())
}

fn validate_staging_location(relative: &Path) -> Result<()> {
	let parent = relative.parent().unwrap_or_else(|| Path::new(""));

	if parent != Path::new(STAGING_RELATIVE_PATH)
		|| relative.extension().is_none_or(|extension| extension != "json")
	{
		eyre::bail!("Radar content-review staging must be a JSON file in the fixed staging root");
	}

	Ok(())
}

fn validate_run_id(run_id: &str) -> Result<()> {
	if run_id.is_empty()
		|| run_id.chars().count() > MAX_RUN_ID_CHARS
		|| !run_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
	{
		eyre::bail!("Radar content-review run_id must use 1-64 ASCII letters, digits, or hyphens");
	}

	Ok(())
}

fn validate_pair_directory_name(name: &str) -> Result<()> {
	let Some((run_id, digest)) = name.rsplit_once("--") else {
		eyre::bail!("Radar committed pair directory name is malformed");
	};

	validate_run_id(run_id)?;
	if digest.len() != 64
		|| !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
		|| digest.bytes().any(|byte| byte.is_ascii_uppercase())
	{
		eyre::bail!("Radar committed pair directory digest is malformed");
	}

	Ok(())
}

fn pretty_json_bytes(value: &Value) -> Result<Vec<u8>> {
	let mut bytes = serde_json::to_vec_pretty(value)?;

	bytes.push(b'\n');
	Ok(bytes)
}

fn sha256_hex(payload: &[u8]) -> String {
	Sha256::digest(payload).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn pair_sha256(review_raw: &[u8], impact_raw: &[u8]) -> String {
	let mut digest = Sha256::new();

	digest.update(b"radar-content-review-pair-v1");
	for payload in [review_raw, impact_raw] {
		digest.update(u64::try_from(payload.len()).unwrap_or(u64::MAX).to_be_bytes());
		digest.update(payload);
	}

	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn relative_ref(path: &Path) -> String {
	path.components()
		.map(|component| component.as_os_str().to_string_lossy())
		.collect::<Vec<_>>()
		.join("/")
}

pub(crate) fn validate_committed_pair_artifacts(
	review_raw: &[u8],
	impact_raw: &[u8],
) -> Result<SubjectLineage> {
	let review: Value = serde_json::from_slice(review_raw)
		.map_err(|error| eyre::eyre!("Committed upstream review JSON is invalid: {error}"))?;
	let impact: Value = serde_json::from_slice(impact_raw)
		.map_err(|error| eyre::eyre!("Committed upstream impact JSON is invalid: {error}"))?;

	crate::validate_expected_schema(&review, UPSTREAM_REVIEW_SCHEMA, "Upstream review")?;
	crate::validate_expected_schema(&impact, UPSTREAM_IMPACT_SCHEMA, "Upstream impact")?;
	for (label, payload) in [("Upstream review", &review), ("Upstream impact", &impact)] {
		let errors = crate::validate_artifact_errors(payload);

		if !errors.is_empty() {
			eyre::bail!("{label} validation failed:\n- {}", errors.join("\n- "));
		}
	}
	let review = crate::object_value(&review, "upstream review")?;
	let impact = crate::object_value(&impact, "upstream impact")?;
	let subject = review
		.get("subject")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("upstream review subject must be an object"))?;
	let repo = crate::required_string(review, "repo", "upstream review repo")?;
	let slug = crate::required_string(review, "slug", "upstream review slug")?;
	let subject_kind =
		crate::required_string(subject, "subject_kind", "upstream review subject kind")?;
	let subject_id = crate::required_string(subject, "subject_id", "upstream review subject id")?;
	let upstream_head = crate::required_string(review, "upstream_head", "upstream review head")?;
	let commit_shas = normalized_commit_shas(subject)?;

	if crate::required_string(impact, "repo", "upstream impact repo")? != repo
		|| crate::required_string(impact, "slug", "upstream impact slug")? != slug
	{
		eyre::bail!("Committed review and impact identity must match");
	}
	let lineage = impact
		.get("review_lineage")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("upstream impact review_lineage must be an object"))?;
	for (field, expected) in [
		("slug", slug),
		("subject_kind", subject_kind),
		("subject_id", subject_id),
		("upstream_head", upstream_head),
	] {
		if crate::required_string(lineage, field, "upstream impact review lineage")? != expected {
			eyre::bail!("Committed impact review_lineage.{field} must match the review");
		}
	}
	if normalized_commit_shas(lineage)? != commit_shas {
		eyre::bail!("Committed impact commit lineage must match the review");
	}
	if crate::required_string(lineage, "artifact_sha256", "review artifact digest")?
		!= sha256_hex(review_raw)
	{
		eyre::bail!("Committed impact artifact digest must match the review");
	}
	let review_urls = source_urls(review)?;
	let impact_urls = source_urls(impact)?;

	if review_urls.is_disjoint(&impact_urls) {
		eyre::bail!("Committed review and impact must cite one shared source URL");
	}
	let requests_impact =
		review.get("next_actions").and_then(Value::as_array).is_some_and(|actions| {
			actions
				.iter()
				.any(|action| action.get("type").and_then(Value::as_str) == Some("upstream_impact"))
		});
	if !requests_impact {
		eyre::bail!("Committed review must request an upstream_impact action");
	}

	Ok(SubjectLineage {
		repo: repo.to_owned(),
		subject_kind: subject_kind.to_owned(),
		subject_id: subject_id.to_owned(),
		commit_shas,
	})
}

fn source_urls(object: &Map<String, Value>) -> Result<BTreeSet<String>> {
	object
		.get("source_refs")
		.and_then(Value::as_object)
		.and_then(|refs| refs.get("items"))
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("content-review source_refs.items must be a list"))?
		.iter()
		.map(|item| {
			item.get("url")
				.and_then(Value::as_str)
				.map(ToOwned::to_owned)
				.ok_or_else(|| eyre::eyre!("content-review source reference must include a URL"))
		})
		.collect()
}

fn normalized_commit_shas(object: &Map<String, Value>) -> Result<Vec<String>> {
	let mut commits = object
		.get("commit_shas")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("content-review commit_shas must be a list"))?
		.iter()
		.map(|value| {
			value
				.as_str()
				.map(ToOwned::to_owned)
				.ok_or_else(|| eyre::eyre!("content-review commit_shas must contain strings"))
		})
		.collect::<Result<Vec<_>>>()?;

	commits.sort();
	commits.dedup();
	Ok(commits)
}

fn queue_contains_lineage(queue_raw: &[u8], lineage: &SubjectLineage) -> Result<bool> {
	let queue: Value = serde_json::from_slice(queue_raw)
		.map_err(|error| eyre::eyre!("Review queue JSON is invalid: {error}"))?;
	let queue = crate::object_value(&queue, "review queue")?;
	if crate::required_string(queue, "repo", "review queue repo")? != lineage.repo {
		return Ok(false);
	}
	let Some(subjects) = queue.get("subjects").and_then(Value::as_array) else {
		eyre::bail!("review queue subjects must be a list");
	};

	for subject in subjects {
		let Some(subject) = subject.as_object() else {
			eyre::bail!("review queue subject must be an object");
		};
		if crate::string_field(subject, "subject_kind") == Some(lineage.subject_kind.as_str())
			&& crate::string_field(subject, "subject_id") == Some(lineage.subject_id.as_str())
		{
			return Ok(normalized_commit_shas(subject)? == lineage.commit_shas);
		}
	}

	Ok(false)
}
