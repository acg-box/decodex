use std::{
	fs::OpenOptions,
	io::Read as _,
	os::unix::fs::{MetadataExt as _, OpenOptionsExt as _},
	path::Path,
};

use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

use crate::{
	OffsetDateTime, RadarContentEligibilityReport, RadarContentEligibilityRequest,
	UPSTREAM_IMPACT_SCHEMA, UPSTREAM_REVIEW_QUEUE_SCHEMA, UPSTREAM_REVIEW_SCHEMA,
	prelude::{Result, eyre},
};

const CONTENT_ELIGIBILITY_SCHEMA: &str = "radar_content_eligibility/v1";
const MAX_CONTENT_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ValidatedContentPair {
	pub(crate) repo: String,
	pub(crate) subject_kind: String,
	pub(crate) subject_id: String,
	pub(crate) slug: String,
	pub(crate) upstream_head: String,
	pub(crate) commit_shas: Vec<String>,
	pub(crate) queue_sha256: String,
	pub(crate) review_sha256: String,
	pub(crate) impact_sha256: String,
	pub(crate) public_signal_decision: String,
	pub(crate) publisher_angle: String,
}

pub(crate) fn content_eligibility(
	request: &RadarContentEligibilityRequest,
) -> Result<RadarContentEligibilityReport> {
	if request.max_age_hours == 0 {
		eyre::bail!("source freshness limit must be at least one hour");
	}

	let mut artifacts = read_content_artifacts(request)?.into_iter();
	let queue_raw =
		artifacts.next().ok_or_else(|| eyre::eyre!("review queue bytes are missing"))?;
	let review_raw =
		artifacts.next().ok_or_else(|| eyre::eyre!("upstream review bytes are missing"))?;
	let impact_raw =
		artifacts.next().ok_or_else(|| eyre::eyre!("upstream impact bytes are missing"))?;
	let pair = validate_content_pair_raw(request, &queue_raw, &review_raw, &impact_raw)?;

	if pair.public_signal_decision != "publish" {
		eyre::bail!("upstream impact public_signal_decision must be publish");
	}
	if pair.publisher_angle == "none" {
		eyre::bail!("upstream impact publisher_angle must be a content angle");
	}
	let lineage_sha256 = eligibility_lineage_sha256(
		&pair.repo,
		&pair.subject_kind,
		&pair.subject_id,
		&pair.slug,
		&pair.upstream_head,
		&pair.commit_shas,
		&pair.queue_sha256,
		&pair.review_sha256,
		&pair.impact_sha256,
	);

	Ok(RadarContentEligibilityReport {
		schema: CONTENT_ELIGIBILITY_SCHEMA.to_owned(),
		repo: pair.repo,
		subject_kind: pair.subject_kind,
		subject_id: pair.subject_id,
		slug: pair.slug,
		upstream_head: pair.upstream_head,
		commit_shas: pair.commit_shas,
		queue_sha256: pair.queue_sha256,
		review_sha256: pair.review_sha256,
		impact_sha256: pair.impact_sha256,
		lineage_sha256,
	})
}

pub(crate) fn validate_content_pair_raw(
	request: &RadarContentEligibilityRequest,
	queue_raw: &[u8],
	review_raw: &[u8],
	impact_raw: &[u8],
) -> Result<ValidatedContentPair> {
	if request.max_age_hours == 0 {
		eyre::bail!("source freshness limit must be at least one hour");
	}

	let queue_digest = sha256_hex(queue_raw);
	let review_digest = sha256_hex(review_raw);
	let impact_digest = sha256_hex(impact_raw);
	let queue = parse_artifact("Review queue", queue_raw)?;
	let review = parse_artifact("Upstream review", review_raw)?;
	let impact = parse_artifact("Upstream impact", impact_raw)?;

	crate::validate_expected_schema(&queue, UPSTREAM_REVIEW_QUEUE_SCHEMA, "Review queue")?;
	crate::validate_expected_schema(&review, UPSTREAM_REVIEW_SCHEMA, "Upstream review")?;
	crate::validate_expected_schema(&impact, UPSTREAM_IMPACT_SCHEMA, "Upstream impact")?;
	validate_artifact("Review queue", &queue)?;
	validate_artifact("Upstream review", &review)?;
	validate_artifact("Upstream impact", &impact)?;

	validate_content_freshness(request, &queue, &review, &impact)?;

	let queue = crate::object_value(&queue, "review queue")?;
	let review = crate::object_value(&review, "upstream review")?;
	let impact = crate::object_value(&impact, "upstream impact")?;
	let repo = crate::required_string(review, "repo", "upstream review repo")?;
	let slug = crate::required_string(review, "slug", "upstream review slug")?;
	let subject = review
		.get("subject")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("upstream review subject must be an object"))?;
	let subject_kind =
		crate::required_string(subject, "subject_kind", "upstream review subject_kind")?;
	let subject_id = crate::required_string(subject, "subject_id", "upstream review subject_id")?;
	let review_head = crate::required_string(review, "upstream_head", "upstream review head")?;

	require_equal(
		crate::required_string(queue, "repo", "review queue repo")?,
		repo,
		"review queue repo",
	)?;
	require_equal(
		crate::required_string(impact, "repo", "upstream impact repo")?,
		repo,
		"upstream impact repo",
	)?;
	require_equal(
		crate::required_string(impact, "slug", "upstream impact slug")?,
		slug,
		"upstream impact slug",
	)?;

	let queue_subject = matching_queue_subject(queue, subject_kind, subject_id)?;
	let source_url = crate::required_string(queue_subject, "url", "queue subject URL")?;
	let queue_head = queue
		.get("source")
		.and_then(Value::as_object)
		.ok_or_else(|| eyre::eyre!("review queue source must be an object"))
		.and_then(|source| {
			crate::required_string(source, "upstream_head", "review queue upstream head")
		})?;
	require_equal(review_head, queue_head, "upstream review head")?;
	let queue_commits = normalized_commit_shas(queue_subject, "queue subject commit_shas")?;
	let review_commits = normalized_commit_shas(subject, "upstream review subject.commit_shas")?;
	if queue_commits != review_commits {
		eyre::bail!("upstream review commit_shas must exactly match the selected queue subject");
	}
	validate_impact_review_lineage(
		impact,
		&review_digest,
		slug,
		subject_kind,
		subject_id,
		review_head,
		&review_commits,
	)?;

	if !has_source_url(review, source_url) {
		eyre::bail!("upstream review must cite the selected queue subject URL");
	}
	if !has_source_url(impact, source_url) {
		eyre::bail!("upstream impact must cite the selected queue subject URL");
	}
	if !review_requests_upstream_impact(review) {
		eyre::bail!("upstream review must request an upstream_impact next action");
	}
	let public_signal_decision =
		crate::required_string(impact, "public_signal_decision", "upstream impact decision")?;
	let publisher_angle =
		crate::required_string(impact, "publisher_angle", "upstream impact publisher angle")?;

	Ok(ValidatedContentPair {
		repo: repo.to_owned(),
		subject_kind: subject_kind.to_owned(),
		subject_id: subject_id.to_owned(),
		slug: slug.to_owned(),
		upstream_head: review_head.to_owned(),
		commit_shas: review_commits,
		queue_sha256: queue_digest,
		review_sha256: review_digest,
		impact_sha256: impact_digest,
		public_signal_decision: public_signal_decision.to_owned(),
		publisher_angle: publisher_angle.to_owned(),
	})
}

fn validate_content_freshness(
	request: &RadarContentEligibilityRequest,
	queue: &Value,
	review: &Value,
	impact: &Value,
) -> Result<()> {
	let now = OffsetDateTime::now_utc();
	let mut errors = Vec::new();

	for (path, payload) in [
		(request.queue.as_path(), queue),
		(request.review.as_path(), review),
		(request.impact.as_path(), impact),
	] {
		crate::validate_source_freshness(path, payload, request.max_age_hours, now, &mut errors);
	}
	if !errors.is_empty() {
		eyre::bail!("Content eligibility freshness failed:\n- {}", errors.join("\n- "));
	}

	Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_impact_review_lineage(
	impact: &Map<String, Value>,
	review_digest: &str,
	slug: &str,
	subject_kind: &str,
	subject_id: &str,
	upstream_head: &str,
	commit_shas: &[String],
) -> Result<()> {
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
		require_equal(
			crate::required_string(lineage, field, "upstream impact review lineage")?,
			expected,
			&format!("upstream impact review_lineage.{field}"),
		)?;
	}

	let impact_commits =
		normalized_commit_shas(lineage, "upstream impact review_lineage.commit_shas")?;
	if impact_commits != commit_shas {
		eyre::bail!(
			"upstream impact review_lineage.commit_shas must exactly match the upstream review"
		);
	}

	require_equal(
		crate::required_string(
			lineage,
			"artifact_sha256",
			"upstream impact review artifact digest",
		)?,
		review_digest,
		"upstream impact review_lineage.artifact_sha256",
	)
}

fn normalized_commit_shas(entry: &Map<String, Value>, label: &str) -> Result<Vec<String>> {
	let mut commits = entry
		.get("commit_shas")
		.and_then(Value::as_array)
		.ok_or_else(|| eyre::eyre!("{label} must be a list"))?
		.iter()
		.map(|value| {
			value
				.as_str()
				.filter(|value| !value.is_empty())
				.map(str::to_ascii_lowercase)
				.ok_or_else(|| eyre::eyre!("{label} must contain non-empty strings"))
		})
		.collect::<Result<Vec<_>>>()?;

	commits.sort();
	commits.dedup();

	Ok(commits)
}

fn read_content_artifacts(request: &RadarContentEligibilityRequest) -> Result<Vec<Vec<u8>>> {
	let paths = [request.queue.as_path(), request.review.as_path(), request.impact.as_path()];
	let private_count = paths.iter().filter(|path| crate::is_radar_cache_path(path)).count();

	if private_count == paths.len() {
		return crate::read_private_files(&paths);
	}
	if private_count != 0 {
		eyre::bail!(
			"content eligibility inputs must all share one Radar cache root or all be external"
		);
	}

	paths.iter().map(|path| read_regular_file(path)).collect()
}

fn read_regular_file(path: &Path) -> Result<Vec<u8>> {
	read_regular_file_bounded_with(path, MAX_CONTENT_ARTIFACT_BYTES, || {})
}

fn read_regular_file_bounded_with(
	path: &Path,
	max_bytes: u64,
	after_metadata: impl FnOnce(),
) -> Result<Vec<u8>> {
	let mut file = OpenOptions::new()
		.read(true)
		.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
		.open(path)?;
	let initial = file.metadata()?;

	if !initial.is_file() {
		eyre::bail!("content eligibility input must be a regular non-symlink file");
	}
	if initial.len() > max_bytes {
		eyre::bail!("content eligibility input exceeds the bounded read limit");
	}
	after_metadata();

	let mut payload = Vec::with_capacity(initial.len() as usize);
	let read_limit = max_bytes
		.checked_add(1)
		.ok_or_else(|| eyre::eyre!("content eligibility read limit is too large"))?;

	file.by_ref().take(read_limit).read_to_end(&mut payload)?;
	if u64::try_from(payload.len()).unwrap_or(u64::MAX) > max_bytes {
		eyre::bail!("content eligibility input exceeds the bounded read limit");
	}
	let final_metadata = file.metadata()?;
	if (initial.dev(), initial.ino(), initial.mtime(), initial.mtime_nsec(), initial.len())
		!= (
			final_metadata.dev(),
			final_metadata.ino(),
			final_metadata.mtime(),
			final_metadata.mtime_nsec(),
			final_metadata.len(),
		) {
		eyre::bail!("content eligibility input identity changed during read");
	}

	Ok(payload)
}

#[cfg(test)]
pub(crate) fn read_regular_file_bounded_after_metadata(
	path: &Path,
	max_bytes: u64,
	after_metadata: impl FnOnce(),
) -> Result<Vec<u8>> {
	read_regular_file_bounded_with(path, max_bytes, after_metadata)
}

fn parse_artifact(label: &str, payload: &[u8]) -> Result<Value> {
	serde_json::from_slice(payload)
		.map_err(|error| eyre::eyre!("{label} contains invalid JSON: {error}"))
}

fn sha256_hex(payload: &[u8]) -> String {
	let digest = Sha256::digest(payload);
	let mut encoded = String::with_capacity(64);

	for byte in digest {
		use std::fmt::Write as _;

		write!(&mut encoded, "{byte:02x}").expect("writing into a String must not fail");
	}

	encoded
}

#[allow(clippy::too_many_arguments)]
fn eligibility_lineage_sha256(
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
		("schema", CONTENT_ELIGIBILITY_SCHEMA),
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

	sha256_digest_hex(digest.finalize())
}

fn update_digest_field(digest: &mut Sha256, name: &str, value: &str) {
	for bytes in [name.as_bytes(), value.as_bytes()] {
		digest.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
		digest.update(bytes);
	}
}

fn sha256_digest_hex(digest: impl AsRef<[u8]>) -> String {
	let mut encoded = String::with_capacity(64);

	for byte in digest.as_ref() {
		use std::fmt::Write as _;

		write!(&mut encoded, "{byte:02x}").expect("writing into a String must not fail");
	}

	encoded
}

fn validate_artifact(label: &str, payload: &Value) -> Result<()> {
	let errors = crate::validate_artifact_errors(payload);

	if errors.is_empty() {
		Ok(())
	} else {
		eyre::bail!("{label} validation failed:\n- {}", errors.join("\n- "))
	}
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

				(crate::string_field(subject, "subject_kind") == Some(subject_kind)
					&& crate::string_field(subject, "subject_id") == Some(subject_id))
				.then_some(subject)
			})
		})
		.ok_or_else(|| {
			eyre::eyre!(
				"review queue does not contain selected subject {subject_kind}:{subject_id}"
			)
		})
}

fn has_source_url(entry: &Map<String, Value>, source_url: &str) -> bool {
	entry
		.get("source_refs")
		.and_then(Value::as_object)
		.and_then(|refs| refs.get("items"))
		.and_then(Value::as_array)
		.is_some_and(|items| {
			items.iter().any(|item| {
				item.get("url").and_then(Value::as_str).is_some_and(|url| url == source_url)
			})
		})
}

fn review_requests_upstream_impact(review: &Map<String, Value>) -> bool {
	review.get("next_actions").and_then(Value::as_array).is_some_and(|actions| {
		actions
			.iter()
			.any(|action| action.get("type").and_then(Value::as_str) == Some("upstream_impact"))
	})
}

fn require_equal(actual: &str, expected: &str, label: &str) -> Result<()> {
	if actual == expected { Ok(()) } else { eyre::bail!("{label} must match the upstream review") }
}
