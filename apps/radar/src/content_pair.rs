//! Atomic persistence and discovery of source-backed content-review pairs.

use std::{
	collections::{BTreeMap, BTreeSet},
	path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use crate::{
	CACHE_MAX_BYTES_PER_COLLECTION, RadarBundleBuildReceipt, RadarContentEligibilityRequest,
	RadarContentPairCommitReport, RadarContentPairCommitRequest, UPSTREAM_IMPACT_SCHEMA,
	UPSTREAM_REVIEW_SCHEMA,
	content_eligibility::ValidatedContentPair,
	prelude::{Result, eyre},
	private_fs::{PrivateEntryKind, RadarCacheLock},
};

pub(crate) const PAIRS_RELATIVE_PATH: &str = "github/content-review-pairs";
pub(crate) const STAGING_RELATIVE_PATH: &str = "github/content-review-staging";
const BUNDLES_RELATIVE_PATH: &str = "github/bundles";
const STAGING_SCHEMA: &str = "radar_content_review_pair_staging/v2";
const COMMIT_REPORT_SCHEMA: &str = "radar_content_review_pair_commit/v1";
const REVIEW_FILE: &str = "review.json";
const IMPACT_FILE: &str = "impact.json";
const STAGING_REVIEW_DIGEST_SENTINEL: &str =
	"0000000000000000000000000000000000000000000000000000000000000000";
const MAX_STAGING_BYTES: u64 = 256 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StagingPair {
	schema: String,
	run_id: String,
	queue_sha256: String,
	selection_sha256: String,
	bundle_evidence_receipt: RadarBundleBuildReceipt,
	patch_anchor: Option<StagingPatchAnchor>,
	patch_anchor_limitation: Option<StagingPatchAnchorLimitation>,
	review: Value,
	impact: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StagingPatchAnchor {
	path: String,
	kind: PatchAnchorKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum PatchAnchorKind {
	Implementation,
	Test,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StagingPatchAnchorLimitation {
	reason: PatchAnchorLimitationReason,
	detail: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum PatchAnchorLimitationReason {
	NoPatchExcerpts,
	NoUsableImplementationOrTestAnchor,
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
	let current_run_id = crate::current_run_id()?;
	let cache = crate::private_fs::PrivateCache::open_existing(&request.cache_root)?;
	let lock = cache.lock()?;
	let staging_relative = lock.relative_path(&request.staging)?;

	validate_staging_location(&staging_relative)?;
	let expected_staging = Path::new(STAGING_RELATIVE_PATH).join(format!("{current_run_id}.json"));
	if staging_relative != expected_staging {
		eyre::bail!("Radar content-review staging path must match CODEX_THREAD_ID");
	}
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

	validate_staging(&staging, &staging_relative, &current_run_id)?;
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
	let pair_sha256 = content_pair_sha256(&review_raw, &impact_raw);
	let final_name = format!("{}--{staging_sha256}--{pair_sha256}", staging.run_id);
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
	validate_staged_bundle(&lock, &staging, &pair)?;
	let recovery =
		exact_recovery_pair(&lock, &final_relative, &queue_raw, request.max_age_hours, &pair)?;
	reject_conflicting_run_or_subject(&lock, &staging.run_id, &final_name, &pair)?;
	let selection = crate::content_review::review_next_under_lock(
		&lock,
		&queue_raw,
		request.max_age_hours,
		recovery.then_some(final_relative.as_path()),
	)?;
	validate_current_selection(&staging, &pair, &selection)?;

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
	handled_subjects_excluding(lock, queue_raw, None)
}

pub(crate) fn handled_subjects_excluding(
	lock: &RadarCacheLock,
	queue_raw: &[u8],
	excluded_pair: Option<&Path>,
) -> Result<BTreeSet<SubjectLineage>> {
	let mut handled = BTreeSet::new();
	let mut identities = BTreeMap::<(String, String, String, Vec<String>), SubjectLineage>::new();

	for directory in pair_directories(lock)? {
		if excluded_pair.is_some_and(|excluded| excluded == directory) {
			continue;
		}
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

fn exact_recovery_pair(
	lock: &RadarCacheLock,
	directory: &Path,
	queue_raw: &[u8],
	max_age_hours: u64,
	expected: &ValidatedContentPair,
) -> Result<bool> {
	match lock.cache().entry_kind(directory)? {
		None => Ok(false),
		Some(PrivateEntryKind::File) => {
			eyre::bail!("Radar committed pair destination is not a directory")
		},
		Some(PrivateEntryKind::Directory) => {
			let existing = read_committed_pair(lock, directory, queue_raw, max_age_hours)?;

			if &existing != expected {
				eyre::bail!("Radar exact retry pair does not match the staging payload");
			}

			Ok(true)
		},
	}
}

fn validate_current_selection(
	staging: &StagingPair,
	pair: &ValidatedContentPair,
	selection: &crate::RadarReviewNextReport,
) -> Result<()> {
	if selection.status != "needs_source_review" {
		eyre::bail!("Radar content-review staging has no current review-next selection");
	}
	if selection.selection_sha256.as_deref() != Some(staging.selection_sha256.as_str()) {
		eyre::bail!("Radar content-review staging selection_sha256 is not current");
	}
	let selected = selection
		.selected
		.as_ref()
		.ok_or_else(|| eyre::eyre!("Radar content-review staging selection is missing"))?;

	if selected.repo != pair.repo
		|| selected.subject_kind != pair.subject_kind
		|| selected.subject_id != pair.subject_id
		|| selected.slug != pair.slug
		|| selected.commit_shas != pair.commit_shas
	{
		eyre::bail!("Radar content-review pair must match the exact current review-next selection");
	}

	Ok(())
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
		let (existing_run_id, _, _) = parse_pair_directory_name(name)?;

		if existing_run_id == run_id && name != final_name {
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
	let name = directory
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("Radar committed pair directory name is invalid"))?;
	let (_, _, expected_pair_sha256) = parse_pair_directory_name(name)?;
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

	if content_pair_sha256(&review_raw, &impact_raw) != expected_pair_sha256 {
		eyre::bail!("Radar committed content-review pair digest does not match its artifacts");
	}

	Ok((review_raw, impact_raw))
}

pub(crate) fn read_private_eligibility_pair(
	lock: &RadarCacheLock,
	review: &Path,
	impact: &Path,
) -> Result<(Vec<u8>, Vec<u8>)> {
	if review.file_name() != Some(std::ffi::OsStr::new(REVIEW_FILE))
		|| impact.file_name() != Some(std::ffi::OsStr::new(IMPACT_FILE))
	{
		eyre::bail!("private content eligibility requires review.json and impact.json");
	}
	let directory = review.parent().ok_or_else(|| {
		eyre::eyre!("private content eligibility review path has no pair directory")
	})?;
	if impact.parent() != Some(directory)
		|| directory.parent() != Some(Path::new(PAIRS_RELATIVE_PATH))
	{
		eyre::bail!(
			"private content eligibility requires one strict committed content-review pair directory"
		);
	}
	let name = directory
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| eyre::eyre!("private content eligibility pair directory name is invalid"))?;
	validate_pair_directory_name(name)?;

	read_pair_artifacts(lock, directory)
}

fn validate_staging(staging: &StagingPair, relative: &Path, current_run_id: &str) -> Result<()> {
	if staging.schema != STAGING_SCHEMA {
		eyre::bail!("Radar content-review staging schema must be {STAGING_SCHEMA}");
	}
	crate::run_identity::validate_run_id(&staging.run_id)?;
	if staging.run_id != current_run_id {
		eyre::bail!("Radar content-review staging run_id must match CODEX_THREAD_ID");
	}
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
	if !is_lowercase_sha256(&staging.selection_sha256) {
		eyre::bail!("Radar content-review staging selection_sha256 must be lowercase SHA-256");
	}
	if staging.patch_anchor.as_ref().is_some_and(|anchor| {
		anchor.path.is_empty()
			|| anchor.path != anchor.path.trim()
			|| anchor.path.contains(['\r', '\n'])
	}) {
		eyre::bail!("Radar content-review patch_anchor.path must be one trimmed non-empty line");
	}
	if let Some(limitation) = &staging.patch_anchor_limitation {
		let detail = &limitation.detail;

		if detail.is_empty()
			|| detail != detail.trim()
			|| detail.contains(['\r', '\n'])
			|| detail.chars().count() > 512
		{
			eyre::bail!(
				"Radar content-review patch_anchor_limitation.detail must be 1-512 trimmed characters"
			);
		}
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

fn validate_staged_bundle(
	lock: &RadarCacheLock,
	staging: &StagingPair,
	pair: &ValidatedContentPair,
) -> Result<()> {
	let bundle_relative = Path::new(BUNDLES_RELATIVE_PATH).join(format!("{}.json", staging.run_id));
	let bundle_raw = lock.read_bounded(&bundle_relative, CACHE_MAX_BYTES_PER_COLLECTION)?;
	let (bundle, receipt) = crate::bundle_evidence_from_bytes(&bundle_raw)?;

	if receipt != staging.bundle_evidence_receipt {
		eyre::bail!("Radar content-review bundle evidence receipt does not match the run bundle");
	}
	let bundle = bundle
		.as_object()
		.ok_or_else(|| eyre::eyre!("Radar content-review run bundle must be an object"))?;

	validate_bundle_subject_binding(bundle, pair)?;
	validate_patch_evidence_contract(bundle, &receipt, staging, pair)
}

fn validate_bundle_subject_binding(
	bundle: &Map<String, Value>,
	pair: &ValidatedContentPair,
) -> Result<()> {
	let repo = crate::required_string(bundle, "repo", "run bundle repo")?;

	if repo != pair.repo {
		eyre::bail!("Radar content-review run bundle repo must match the selected queue subject");
	}
	let commits = bundle
		.get("commits")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Radar content-review run bundle commits must be a list"))?;
	let bundle_commits = normalized_bundle_commit_shas(commits)?;

	if bundle_commits != pair.commit_shas {
		eyre::bail!(
			"Radar content-review run bundle commit set must exactly match review and queue lineage"
		);
	}
	match crate::required_string(bundle, "analysis_mode", "run bundle analysis mode")? {
		"pr_first" => {
			if pair.subject_kind != "pr" {
				eyre::bail!("Radar pr_first run bundle requires a pull-request queue subject");
			}
			let number = bundle
				.get("primary_pr")
				.and_then(Value::as_object)
				.and_then(|primary_pr| primary_pr.get("number"))
				.and_then(Value::as_u64)
				.ok_or_else(|| {
					eyre::eyre!("Radar pr_first run bundle primary_pr.number must be an integer")
				})?;

			if number.to_string() != pair.subject_id {
				eyre::bail!(
					"Radar pr_first run bundle primary_pr.number must match the queue subject_id"
				);
			}
		},
		"commit_only" => {
			if pair.subject_kind != "commit" {
				eyre::bail!("Radar commit_only run bundle requires a commit queue subject");
			}
			if commits.len() != 1 {
				eyre::bail!("Radar commit_only run bundle must contain exactly one commit");
			}
			let commit_sha = commits[0]
				.get("sha")
				.and_then(Value::as_str)
				.ok_or_else(|| eyre::eyre!("Radar commit_only run bundle commit SHA is missing"))?;

			if commit_sha != pair.subject_id {
				eyre::bail!(
					"Radar commit_only run bundle commit SHA must match the queue subject_id"
				);
			}
		},
		_ => eyre::bail!("Radar content-review run bundle analysis mode is unsupported"),
	}

	Ok(())
}

fn normalized_bundle_commit_shas(commits: &[Value]) -> Result<Vec<String>> {
	let mut shas = commits
		.iter()
		.map(|commit| {
			commit
				.get("sha")
				.and_then(Value::as_str)
				.filter(|sha| !sha.is_empty())
				.map(str::to_ascii_lowercase)
				.ok_or_else(|| eyre::eyre!("Radar content-review run bundle commit SHA is missing"))
		})
		.collect::<Result<Vec<_>>>()?;

	shas.sort();
	shas.dedup();
	Ok(shas)
}

fn validate_patch_evidence_contract(
	bundle: &Map<String, Value>,
	receipt: &RadarBundleBuildReceipt,
	staging: &StagingPair,
	pair: &ValidatedContentPair,
) -> Result<()> {
	let patch_count = receipt.patch_excerpt_count;

	if patch_count == 0 {
		return match (&staging.patch_anchor, &staging.patch_anchor_limitation) {
			(None, Some(limitation))
				if limitation.reason == PatchAnchorLimitationReason::NoPatchExcerpts =>
				validate_patch_anchor_limitation(limitation, staging, pair),
			_ => eyre::bail!(
				"Radar zero-excerpt run bundle requires the no_patch_excerpts limitation"
			),
		};
	}
	match (&staging.patch_anchor, &staging.patch_anchor_limitation) {
		(Some(anchor), None) => validate_patch_anchor(bundle, anchor, staging),
		(None, Some(limitation)) => {
			if limitation.reason != PatchAnchorLimitationReason::NoUsableImplementationOrTestAnchor
			{
				eyre::bail!("Radar positive-excerpt run bundle uses the wrong limitation reason");
			}
			validate_patch_anchor_limitation(limitation, staging, pair)
		},
		(Some(_), Some(_)) => eyre::bail!(
			"Radar positive-excerpt staging must choose patch_anchor or patch_anchor_limitation"
		),
		(None, None) => eyre::bail!(
			"Radar positive-excerpt staging requires patch_anchor or a nonpublishable limitation"
		),
	}
}

fn validate_patch_anchor(
	bundle: &Map<String, Value>,
	anchor: &StagingPatchAnchor,
	staging: &StagingPair,
) -> Result<()> {
	let files = bundle
		.get("files")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Radar content-review run bundle files must be a list"))?;
	let file = files
		.iter()
		.find(|file| file.get("path").and_then(Value::as_str) == Some(anchor.path.as_str()));
	let Some(file) = file else {
		eyre::bail!("Radar content-review patch_anchor.path does not name a run bundle file");
	};
	if !file
		.get("patch_excerpt")
		.and_then(Value::as_str)
		.is_some_and(|excerpt| !excerpt.trim().is_empty())
	{
		eyre::bail!("Radar content-review patch_anchor.path has no non-empty patch excerpt");
	}
	validate_anchor_kind(bundle, anchor)?;
	for (label, payload) in
		[("Staged upstream review", &staging.review), ("Staged upstream impact", &staging.impact)]
	{
		if !has_exact_path_claim(payload, &anchor.path)? {
			eyre::bail!("{label} evidence must use exact '<patch_anchor.path>: <claim>' syntax");
		}
	}

	Ok(())
}

fn validate_anchor_kind(bundle: &Map<String, Value>, anchor: &StagingPatchAnchor) -> Result<()> {
	let docs_refs = bundle_string_set(bundle, "docs_refs")?;
	let examples_refs = bundle_string_set(bundle, "examples_refs")?;
	let is_documentation = docs_refs.contains(anchor.path.as_str())
		|| examples_refs.contains(anchor.path.as_str())
		|| is_documentation_or_example_path(&anchor.path);
	let is_test = is_conservative_test_path(&anchor.path);
	let is_allowlisted = is_allowlisted_anchor_path(&anchor.path);

	match anchor.kind {
		PatchAnchorKind::Implementation if is_documentation => eyre::bail!(
			"Radar implementation patch_anchor cannot use documentation or example paths"
		),
		PatchAnchorKind::Implementation if is_test => {
			eyre::bail!("Radar implementation patch_anchor cannot use a test path")
		},
		PatchAnchorKind::Implementation if !is_allowlisted => eyre::bail!(
			"Radar implementation patch_anchor must use an allowlisted source, protocol, or config path"
		),
		PatchAnchorKind::Test if is_documentation => {
			eyre::bail!("Radar test patch_anchor cannot use documentation or example paths")
		},
		PatchAnchorKind::Test if !is_test => {
			eyre::bail!("Radar test patch_anchor must use a conservative test path")
		},
		PatchAnchorKind::Test if !is_allowlisted => eyre::bail!(
			"Radar test patch_anchor must use an allowlisted source, protocol, or config path"
		),
		PatchAnchorKind::Implementation | PatchAnchorKind::Test => Ok(()),
	}
}

fn bundle_string_set<'a>(bundle: &'a Map<String, Value>, field: &str) -> Result<BTreeSet<&'a str>> {
	bundle
		.get(field)
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("Radar run bundle {field} must be a list"))?
		.iter()
		.map(|value| {
			value
				.as_str()
				.ok_or_else(|| eyre::eyre!("Radar run bundle {field} must contain strings"))
		})
		.collect()
}

fn is_conservative_test_path(path: &str) -> bool {
	let lower = path.to_ascii_lowercase();
	let components = lower.split('/').collect::<Vec<_>>();
	let file_name = components.last().copied().unwrap_or_default();
	let stem = file_name.rsplit_once('.').map_or(file_name, |(stem, _)| stem);
	let original_file_name = path.rsplit('/').next().unwrap_or_default();
	let original_stem =
		original_file_name.rsplit_once('.').map_or(original_file_name, |(stem, _)| stem);

	components.iter().any(|component| {
		matches!(
			*component,
			"test"
				| "tests" | "testing"
				| "__tests__"
				| "integration-test"
				| "integration-tests"
				| "integration_test"
				| "integration_tests"
				| "integrationtest"
				| "integrationtests"
				| "e2e" | "end-to-end"
				| "end_to_end"
				| "fixture" | "fixtures"
				| "snapshot" | "snapshots"
				| "testdata" | "test_data"
		)
	}) || matches!(
		stem,
		"test"
			| "tests" | "testing"
			| "integration-test"
			| "integration-tests"
			| "integration_test"
			| "integration_tests"
			| "integrationtest"
			| "integrationtests"
			| "e2e" | "end-to-end"
			| "end_to_end"
			| "fixture"
			| "fixtures"
			| "snapshot"
			| "snapshots"
			| "testdata"
			| "test_data"
	) || file_name.starts_with("test_")
		|| file_name.starts_with("test-")
		|| file_name.starts_with("integration_test_")
		|| file_name.starts_with("integration-test-")
		|| file_name.starts_with("e2e_")
		|| file_name.starts_with("e2e-")
		|| stem.ends_with("_test")
		|| stem.ends_with("_tests")
		|| stem.ends_with("-test")
		|| stem.ends_with("-tests")
		|| stem.ends_with("_spec")
		|| stem.ends_with("_specs")
		|| stem.ends_with("_integration_test")
		|| stem.ends_with("_integration_tests")
		|| stem.ends_with("-integration-test")
		|| stem.ends_with("-integration-tests")
		|| stem.ends_with("_e2e")
		|| stem.ends_with("-e2e")
		|| original_stem.ends_with("Test")
		|| original_stem.ends_with("Tests")
		|| original_stem.ends_with("IntegrationTest")
		|| original_stem.ends_with("IntegrationTests")
		|| original_stem.ends_with("E2E")
		|| original_stem.ends_with("E2ETest")
		|| original_stem.ends_with("E2ETests")
		|| original_stem.ends_with("Spec")
		|| original_stem.ends_with("Specs")
		|| file_name.contains(".test.")
		|| file_name.contains(".tests.")
		|| file_name.contains(".spec.")
		|| file_name.contains(".integration.test.")
		|| file_name.contains(".integration-test.")
		|| file_name.contains(".e2e.")
		|| file_name.ends_with(".snap")
}

fn is_documentation_or_example_path(path: &str) -> bool {
	let lower = path.to_ascii_lowercase();
	let components = lower.split('/').collect::<Vec<_>>();
	let file_name = components.last().copied().unwrap_or_default();

	components.iter().any(|component| {
		matches!(
			*component,
			"doc"
				| "docs" | "documentation"
				| "example" | "examples"
				| "website" | "websites"
				| "content" | "contents"
				| "guide" | "guides"
		)
	}) || file_name.starts_with("readme")
		|| file_name.starts_with("changelog")
		|| matches!(path_extension(file_name), Some("md" | "mdx" | "rst"))
		|| file_name.contains("example")
}

fn is_allowlisted_anchor_path(path: &str) -> bool {
	let file_name = path.rsplit('/').next().unwrap_or_default().to_ascii_lowercase();

	matches!(
		path_extension(&file_name),
		Some(
			"rs" | "toml"
				| "json" | "proto"
				| "yaml" | "yml"
				| "ini" | "conf"
				| "cfg" | "ts"
				| "tsx" | "js"
				| "jsx" | "mjs"
				| "cjs" | "py"
				| "pyi" | "go"
				| "swift" | "c"
				| "cc" | "cpp"
				| "h" | "hpp"
				| "java" | "kt"
				| "kts" | "sh"
				| "bash" | "zsh"
				| "fish" | "sql"
				| "graphql" | "gql"
		)
	) || matches!(file_name.as_str(), "dockerfile" | "makefile" | "justfile")
}

fn path_extension(file_name: &str) -> Option<&str> {
	file_name.rsplit_once('.').map(|(_, extension)| extension)
}

fn has_exact_path_claim(payload: &Value, path: &str) -> Result<bool> {
	let prefix = format!("{path}: ");
	let evidence = payload
		.get("evidence")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("staged evidence must be a list"))?;

	Ok(evidence.iter().any(|item| {
		item.as_str()
			.and_then(|item| item.strip_prefix(&prefix))
			.is_some_and(|claim| !claim.is_empty() && claim == claim.trim())
	}))
}

fn validate_patch_anchor_limitation(
	limitation: &StagingPatchAnchorLimitation,
	staging: &StagingPair,
	pair: &ValidatedContentPair,
) -> Result<()> {
	if !matches!(pair.public_signal_decision.as_str(), "defer" | "skip") {
		eyre::bail!("Radar patch-anchor limitation requires a defer or skip decision");
	}
	if staging.impact.get("publisher_angle").and_then(Value::as_str) != Some("none") {
		eyre::bail!("Radar patch-anchor limitation requires publisher_angle none");
	}
	let expected = format!("bundle patch limitation: {}", limitation.detail);

	for (label, payload) in
		[("Staged upstream review", &staging.review), ("Staged upstream impact", &staging.impact)]
	{
		let evidence = payload
			.get("evidence")
			.and_then(Value::as_array)
			.ok_or_else(|| eyre::eyre!("{label} evidence must be a list"))?;

		if evidence.len() != 1 || evidence[0].as_str() != Some(expected.as_str()) {
			eyre::bail!("{label} evidence must contain exactly the canonical patch limitation");
		}
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

fn validate_pair_directory_name(name: &str) -> Result<()> {
	let _ = parse_pair_directory_name(name)?;
	Ok(())
}

fn parse_pair_directory_name(name: &str) -> Result<(&str, &str, &str)> {
	let mut parts = name.split("--");
	let run_id = parts.next().unwrap_or_default();
	let staging_sha256 = parts.next().unwrap_or_default();
	let pair_sha256 = parts.next().unwrap_or_default();

	if parts.next().is_some() {
		eyre::bail!("Radar committed pair directory name is malformed");
	}
	crate::run_identity::validate_run_id(run_id)
		.map_err(|_| eyre::eyre!("Radar committed pair directory run_id is malformed"))?;
	if !is_lowercase_sha256(staging_sha256) || !is_lowercase_sha256(pair_sha256) {
		eyre::bail!("Radar committed pair directory digest is malformed");
	}

	Ok((run_id, staging_sha256, pair_sha256))
}

fn is_lowercase_sha256(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn pretty_json_bytes(value: &Value) -> Result<Vec<u8>> {
	let mut bytes = serde_json::to_vec_pretty(value)?;

	bytes.push(b'\n');
	Ok(bytes)
}

fn sha256_hex(payload: &[u8]) -> String {
	Sha256::digest(payload).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn content_pair_sha256(review_raw: &[u8], impact_raw: &[u8]) -> String {
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
	crate::content_eligibility::validate_decision_angle(
		crate::required_string(impact, "public_signal_decision", "upstream impact decision")?,
		crate::required_string(impact, "publisher_angle", "upstream impact publisher angle")?,
	)?;
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
