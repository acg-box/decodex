use std::{
	collections::BTreeMap,
	ffi::{OsStr, OsString},
	path::{Path, PathBuf},
};

use serde_json::Value;
use time::{Date, Duration, Month, OffsetDateTime, format_description::well_known::Rfc3339};

use super::{GcPolicy, digest_hex};
use crate::{
	SOCIAL_CANDIDATE_SCHEMA, SOCIAL_OUTCOME_SCHEMA, SOCIAL_POST_SCHEMA,
	SOCIAL_PUBLISH_RESERVATION_SCHEMA, SOCIAL_STRATEGY_SCHEMA, SocialGcRequest,
	filesystem::{PinnedPrivateDirectory, PrivateFileIdentity},
	social_validation::SocialValidationState,
	social_xurl::{
		auth_contract::APPROVED_XURL_VERSION,
		ledger,
		model::{
			ATTEMPT_SCHEMA, OBSERVATION_ATTEMPT_SCHEMA, PUBLICATION_LINEAGE_BUDGET_MICROUSD,
			TARGET_ACCOUNT, XurlAttempt, XurlCall, XurlObservationAttempt,
		},
	},
};

const MAX_JSON_BYTES: u64 = 1024 * 1024;
const MAX_CLOCK_SKEW: Duration = Duration::minutes(5);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum ArtifactKind {
	Candidate,
	Outcome,
	Post,
	Reservation,
	Strategy,
}
impl ArtifactKind {
	fn schema(self) -> &'static str {
		match self {
			Self::Candidate => SOCIAL_CANDIDATE_SCHEMA,
			Self::Outcome => SOCIAL_OUTCOME_SCHEMA,
			Self::Post => SOCIAL_POST_SCHEMA,
			Self::Reservation => SOCIAL_PUBLISH_RESERVATION_SCHEMA,
			Self::Strategy => SOCIAL_STRATEGY_SCHEMA,
		}
	}
}

#[derive(Clone)]
pub(super) struct StoredFile {
	pub(super) directory: PinnedPrivateDirectory,
	pub(super) name: OsString,
	pub(super) identity: PrivateFileIdentity,
	pub(super) display_path: PathBuf,
	pub(super) key: String,
	pub(super) raw_sha256: String,
}
impl StoredFile {
	pub(super) fn path(&self) -> PathBuf {
		self.display_path.clone()
	}

	pub(super) fn filename(&self) -> Option<&str> {
		self.name.to_str()
	}

	pub(super) fn preflight(&self) -> crate::prelude::Result<()> {
		self.directory.verify_file(&self.name, self.identity)
	}

	pub(super) fn journal_snapshot(&self) -> crate::prelude::Result<(String, String)> {
		let (_value, identity, raw) = self.directory.read_json(&self.name, MAX_JSON_BYTES)?;
		let raw_sha256 = digest_hex(&raw);
		if identity != self.identity || raw_sha256 != self.raw_sha256 {
			return Err(crate::prelude::eyre::eyre!("social GC file changed after scan"));
		}

		Ok((raw_sha256, private_identity_sha256(identity)))
	}
}

pub(super) struct ArtifactRecord {
	pub(super) kind: ArtifactKind,
	pub(super) file: StoredFile,
	pub(super) value: Value,
}

pub(super) enum AttemptValue {
	Publish(XurlAttempt),
	Observe(XurlObservationAttempt),
}

pub(super) struct AttemptRecord {
	pub(super) file: StoredFile,
	pub(super) value: AttemptValue,
}
impl AttemptRecord {
	pub(super) fn billing_month(&self) -> &str {
		match &self.value {
			AttemptValue::Publish(value) => &value.billing_month,
			AttemptValue::Observe(value) => &value.billing_month,
		}
	}

	pub(super) fn uses_billing_month(&self, billing_month: &str) -> bool {
		self.billing_month() == billing_month
			|| match &self.value {
				AttemptValue::Publish(value) => value
					.calls
					.iter()
					.any(|call| call.billing_month.as_deref() == Some(billing_month)),
				AttemptValue::Observe(value) => value
					.calls
					.iter()
					.any(|call| call.billing_month.as_deref() == Some(billing_month)),
			}
	}

	pub(super) fn updated_at(&self) -> &str {
		match &self.value {
			AttemptValue::Publish(value) => &value.updated_at,
			AttemptValue::Observe(value) => &value.updated_at,
		}
	}
}

pub(super) struct Inventory {
	pub(super) artifacts: Vec<ArtifactRecord>,
	pub(super) attempts: Vec<AttemptRecord>,
	pub(super) scanned_files: usize,
}

struct ScanLimits {
	max_entries: usize,
	max_files: usize,
	max_bytes: u64,
	entries: usize,
	files: usize,
	bytes: u64,
}
impl ScanLimits {
	fn new(policy: GcPolicy) -> Self {
		Self {
			max_entries: policy.max_entries,
			max_files: policy.max_files,
			max_bytes: policy.max_bytes,
			entries: 0,
			files: 0,
			bytes: 0,
		}
	}

	fn add_entry(&mut self) -> Result<(), ()> {
		self.entries = self.entries.checked_add(1).ok_or(())?;
		if self.entries > self.max_entries {
			return Err(());
		}

		Ok(())
	}

	fn remaining_entries(&self) -> usize {
		self.max_entries.saturating_sub(self.entries)
	}

	fn add_file(&mut self, bytes: u64) -> Result<(), ()> {
		self.files = self.files.checked_add(1).ok_or(())?;
		self.bytes = self.bytes.checked_add(bytes).ok_or(())?;
		if self.files > self.max_files || self.bytes > self.max_bytes {
			return Err(());
		}

		Ok(())
	}
}

pub(super) fn scan(
	request: &SocialGcRequest,
	policy: GcPolicy,
	now: OffsetDateTime,
) -> Result<Inventory, ()> {
	let root = crate::repo_root().map_err(|_| ())?;
	let mut limits = ScanLimits::new(policy);
	let mut artifacts = Vec::new();
	scan_flat_artifacts(
		&root,
		&request.candidates_dir,
		ArtifactKind::Candidate,
		&mut limits,
		&mut artifacts,
	)
	.map_err(|_| ())?;
	scan_nested_artifacts(
		&root,
		&request.reservations_dir,
		ArtifactKind::Reservation,
		DirectoryNameKind::Day,
		&mut limits,
		&mut artifacts,
	)
	.map_err(|_| ())?;
	scan_flat_artifacts(&root, &request.posts_dir, ArtifactKind::Post, &mut limits, &mut artifacts)
		.map_err(|_| ())?;
	scan_flat_artifacts(
		&root,
		&request.outcomes_dir,
		ArtifactKind::Outcome,
		&mut limits,
		&mut artifacts,
	)
	.map_err(|_| ())?;
	scan_flat_artifacts(
		&root,
		&request.strategies_dir,
		ArtifactKind::Strategy,
		&mut limits,
		&mut artifacts,
	)
	.map_err(|_| ())?;
	artifacts.sort_by(|left, right| left.file.key.cmp(&right.file.key));
	validate_artifacts(&artifacts)?;

	let attempts = scan_attempts(&root, &request.attempts_dir, &mut limits, now)?;

	Ok(Inventory { artifacts, attempts, scanned_files: limits.files })
}

fn scan_flat_artifacts(
	root: &Path,
	raw_dir: &Path,
	kind: ArtifactKind,
	limits: &mut ScanLimits,
	artifacts: &mut Vec<ArtifactRecord>,
) -> Result<(), ()> {
	let display_directory = crate::resolve_against(root, raw_dir);
	let Some(directory) = open_directory(root, raw_dir)? else {
		return Ok(());
	};
	for name in directory.entries_bounded(limits.remaining_entries()).map_err(|_| ())? {
		limits.add_entry()?;
		if !is_artifact_filename(&name) {
			return Err(());
		}
		artifacts.push(read_artifact(root, &display_directory, &directory, name, kind, limits)?);
	}

	Ok(())
}

enum DirectoryNameKind {
	Day,
}

fn scan_nested_artifacts(
	root: &Path,
	raw_dir: &Path,
	kind: ArtifactKind,
	directory_name_kind: DirectoryNameKind,
	limits: &mut ScanLimits,
	artifacts: &mut Vec<ArtifactRecord>,
) -> Result<(), ()> {
	let display_directory = crate::resolve_against(root, raw_dir);
	let Some(directory) = open_directory(root, raw_dir)? else {
		return Ok(());
	};
	for child_name in directory.entries_bounded(limits.remaining_entries()).map_err(|_| ())? {
		limits.add_entry()?;
		let child = child_name.to_str().ok_or(())?;
		let valid = match directory_name_kind {
			DirectoryNameKind::Day => is_day(child),
		};
		if !valid {
			return Err(());
		}
		let child_directory = directory.open_child_directory(&child_name).map_err(|_| ())?;
		let child_display_directory = display_directory.join(&child_name);
		for name in child_directory.entries_bounded(limits.remaining_entries()).map_err(|_| ())? {
			limits.add_entry()?;
			if !is_digest_filename(&name) {
				return Err(());
			}
			let record = read_artifact(
				root,
				&child_display_directory,
				&child_directory,
				name,
				kind,
				limits,
			)?;
			if record.value.get("day").and_then(Value::as_str) != Some(child) {
				return Err(());
			}
			artifacts.push(record);
		}
	}

	Ok(())
}

fn read_artifact(
	root: &Path,
	display_directory: &Path,
	directory: &PinnedPrivateDirectory,
	name: OsString,
	kind: ArtifactKind,
	limits: &mut ScanLimits,
) -> Result<ArtifactRecord, ()> {
	let (value, identity, raw) = directory.read_json(&name, MAX_JSON_BYTES).map_err(|_| ())?;
	limits.add_file(identity.len())?;
	if value.get("schema").and_then(Value::as_str) != Some(kind.schema()) {
		return Err(());
	}
	let path = display_directory.join(&name);

	Ok(ArtifactRecord {
		kind,
		file: StoredFile {
			directory: directory.clone(),
			name,
			identity,
			display_path: path.clone(),
			key: crate::path_arg(root, &path),
			raw_sha256: digest_hex(&raw),
		},
		value,
	})
}

fn validate_artifacts(artifacts: &[ArtifactRecord]) -> Result<(), ()> {
	let mut state = SocialValidationState::new();
	let mut errors = Vec::new();
	for artifact in artifacts {
		let validation = crate::social_validation::validate_social_artifact_for_path(
			&artifact.file.path(),
			&artifact.value,
		);
		if !validation.errors.is_empty() {
			return Err(());
		}
		crate::social_validation::validate_social_cross_file_constraints(
			&artifact.file.path(),
			&artifact.value,
			&mut state,
			&mut errors,
		);
	}
	state.finish(&mut errors);
	if !errors.is_empty() {
		return Err(());
	}

	Ok(())
}

fn scan_attempts(
	root: &Path,
	raw_dir: &Path,
	limits: &mut ScanLimits,
	now: OffsetDateTime,
) -> Result<Vec<AttemptRecord>, ()> {
	let display_directory = crate::resolve_against(root, raw_dir);
	let Some(directory) = open_directory(root, raw_dir)? else {
		return Ok(Vec::new());
	};
	let mut attempts = Vec::new();
	for month_name in directory.entries_bounded(limits.remaining_entries()).map_err(|_| ())? {
		limits.add_entry()?;
		let month = month_name.to_str().filter(|value| is_month(value)).ok_or(())?;
		let month_directory = directory.open_child_directory(&month_name).map_err(|_| ())?;
		let month_display_directory = display_directory.join(&month_name);
		for name in month_directory.entries_bounded(limits.remaining_entries()).map_err(|_| ())? {
			limits.add_entry()?;
			if !is_attempt_filename(&name) {
				return Err(());
			}
			let (payload, identity, raw) =
				month_directory.read_json(&name, MAX_JSON_BYTES).map_err(|_| ())?;
			limits.add_file(identity.len())?;
			let value = match payload.get("schema").and_then(Value::as_str) {
				Some(ATTEMPT_SCHEMA) => {
					let attempt: XurlAttempt = serde_json::from_value(payload).map_err(|_| ())?;
					validate_publish_attempt(&attempt, month, &name, now)?;
					AttemptValue::Publish(attempt)
				},
				Some(OBSERVATION_ATTEMPT_SCHEMA) => {
					let attempt: XurlObservationAttempt =
						serde_json::from_value(payload).map_err(|_| ())?;
					validate_observation_attempt(&attempt, month, &name, now)?;
					AttemptValue::Observe(attempt)
				},
				_ => return Err(()),
			};
			let path = month_display_directory.join(&name);
			attempts.push(AttemptRecord {
				file: StoredFile {
					directory: month_directory.clone(),
					name,
					identity,
					display_path: path.clone(),
					key: crate::path_arg(root, &path),
					raw_sha256: digest_hex(&raw),
				},
				value,
			});
		}
	}
	attempts.sort_by(|left, right| left.file.key.cmp(&right.file.key));
	validate_attempt_lineage_budgets(&attempts)?;

	Ok(attempts)
}

fn validate_attempt_lineage_budgets(attempts: &[AttemptRecord]) -> Result<(), ()> {
	let mut totals = BTreeMap::<&str, u64>::new();
	for attempt in attempts {
		let (lineage, cost) = match &attempt.value {
			AttemptValue::Publish(value) =>
				(value.publication_lineage_sha256.as_str(), value.reserved_cost_ceiling_microusd),
			AttemptValue::Observe(value) =>
				(value.publication_lineage_sha256.as_str(), value.reserved_cost_ceiling_microusd),
		};
		let total = totals.entry(lineage).or_default();
		*total = total.checked_add(cost).ok_or(())?;
		if *total > PUBLICATION_LINEAGE_BUDGET_MICROUSD {
			return Err(());
		}
	}

	Ok(())
}

pub(super) fn validate_publish_attempt(
	attempt: &XurlAttempt,
	month: &str,
	name: &OsStr,
	now: OffsetDateTime,
) -> Result<(), ()> {
	ledger::validate_publication_cost_record(attempt).map_err(|_| ())?;
	let created_at = parse_bounded_time(&attempt.created_at, now)?;
	let updated_at = parse_bounded_time(&attempt.updated_at, now)?;
	if attempt.schema != ATTEMPT_SCHEMA
		|| attempt.billing_month != month
		|| month_for(created_at) != month
		|| updated_at < created_at
		|| attempt.target_account != TARGET_ACCOUNT
		|| !crate::social_publish::valid_run_id(&attempt.run_id)
		|| name.to_str() != Some(&format!("{}.json", attempt.run_id))
		|| attempt.reservation_ref.is_empty()
		|| attempt.candidate_ref.is_empty()
		|| attempt.idempotency_key.is_empty()
		|| attempt.xurl_version != APPROVED_XURL_VERSION
	{
		return Err(());
	}
	if matches!(attempt.status.as_str(), "verified" | "published")
		&& (!successful_publish_calls(&attempt.calls)
			|| !attempt.verified_user_id.as_deref().is_some_and(is_decimal)
			|| !attempt.post_id.as_deref().is_some_and(is_decimal)
			|| attempt.published_url.as_deref()
				!= attempt
					.post_id
					.as_deref()
					.map(|post_id| format!("https://x.com/decodexspace/status/{post_id}"))
					.as_deref())
	{
		return Err(());
	}

	Ok(())
}

fn successful_publish_calls(calls: &[XurlCall]) -> bool {
	calls.iter().any(|call| {
		matches!(call.operation.as_str(), "identity_read" | "identity_read_reconcile")
			&& call.status == "succeeded"
	}) && calls.iter().any(|call| call.operation == "content_create" && call.status == "succeeded")
		&& calls
			.iter()
			.any(|call| call.operation.starts_with("post_read") && call.status == "succeeded")
		&& calls.iter().all(|call| !matches!(call.status.as_str(), "inflight" | "uncertain"))
}

pub(super) fn validate_observation_attempt(
	attempt: &XurlObservationAttempt,
	month: &str,
	name: &OsStr,
	now: OffsetDateTime,
) -> Result<(), ()> {
	ledger::validate_observation_cost_record(attempt).map_err(|_| ())?;
	let created_at = parse_bounded_time(&attempt.created_at, now)?;
	let updated_at = parse_bounded_time(&attempt.updated_at, now)?;
	let expected_name = format!(
		"observe-{}.json",
		digest_hex(format!("{}\0{}", attempt.post_ref, attempt.window).as_bytes())
	);
	if attempt.schema != OBSERVATION_ATTEMPT_SCHEMA
		|| attempt.billing_month != month
		|| month_for(created_at) != month
		|| updated_at < created_at
		|| !crate::social_publish::valid_run_id(&attempt.run_id)
		|| name.to_str() != Some(expected_name.as_str())
		|| attempt.post_ref.is_empty()
		|| !is_decimal(&attempt.post_id)
		|| !matches!(attempt.window.as_str(), "24h" | "7d")
		|| attempt.status == "observed" && attempt.call.status != "succeeded"
		|| matches!(attempt.status.as_str(), "read_inflight" | "read_reconcile_inflight")
			&& attempt.call.status != "inflight"
		|| matches!(attempt.status.as_str(), "halted" | "read_reconcile_halted")
			&& !matches!(attempt.call.status.as_str(), "failed" | "invalid")
	{
		return Err(());
	}

	Ok(())
}

pub(super) fn private_identity_sha256(identity: PrivateFileIdentity) -> String {
	digest_hex(format!("{identity:?}").as_bytes())
}

fn open_directory(root: &Path, raw_dir: &Path) -> Result<Option<PinnedPrivateDirectory>, ()> {
	let path = crate::resolve_against(root, raw_dir);
	crate::open_existing_exact_private_directory(&path).map_err(|_| ())
}

fn parse_bounded_time(value: &str, now: OffsetDateTime) -> Result<OffsetDateTime, ()> {
	let parsed = OffsetDateTime::parse(value, &Rfc3339).map_err(|_| ())?;
	if parsed > now + MAX_CLOCK_SKEW {
		return Err(());
	}

	Ok(parsed)
}

fn month_for(value: OffsetDateTime) -> String {
	format!("{:04}-{:02}", value.year(), u8::from(value.month()))
}

pub(super) fn is_artifact_filename(name: &OsStr) -> bool {
	is_uuid_filename(name) || is_digest_filename(name)
}

pub(super) fn is_attempt_filename(name: &OsStr) -> bool {
	is_uuid_filename(name)
		|| name.to_str().is_some_and(|value| {
			value
				.strip_prefix("observe-")
				.and_then(|value| value.strip_suffix(".json"))
				.is_some_and(|value| is_hex(value, 64))
		})
}

fn is_uuid_filename(name: &OsStr) -> bool {
	name.to_str()
		.and_then(|value| value.strip_suffix(".json"))
		.is_some_and(crate::social_publish::valid_run_id)
}

pub(super) fn is_digest_filename(name: &OsStr) -> bool {
	name.to_str()
		.and_then(|value| value.strip_suffix(".json"))
		.is_some_and(|value| is_hex(value, 64))
}

pub(super) fn is_day(value: &str) -> bool {
	let bytes = value.as_bytes();
	if bytes.len() != 10
		|| bytes[4] != b'-'
		|| bytes[7] != b'-'
		|| !bytes
			.iter()
			.enumerate()
			.all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
	{
		return false;
	}
	let Ok(year) = value[..4].parse() else {
		return false;
	};
	let Ok(month) = value[5..7].parse::<u8>() else {
		return false;
	};
	let Ok(day) = value[8..].parse() else {
		return false;
	};

	Date::from_calendar_date(
		year,
		match Month::try_from(month) {
			Ok(month) => month,
			Err(_) => return false,
		},
		day,
	)
	.is_ok()
}

pub(super) fn is_month(value: &str) -> bool {
	let bytes = value.as_bytes();
	if bytes.len() != 7
		|| bytes[4] != b'-'
		|| !bytes.iter().enumerate().all(|(index, byte)| index == 4 || byte.is_ascii_digit())
	{
		return false;
	}
	let Ok(year) = value[..4].parse() else {
		return false;
	};
	let Ok(month) = value[5..].parse::<u8>() else {
		return false;
	};

	Date::from_calendar_date(
		year,
		match Month::try_from(month) {
			Ok(month) => month,
			Err(_) => return false,
		},
		1,
	)
	.is_ok()
}

fn is_hex(value: &str, length: usize) -> bool {
	value.len() == length
		&& value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn is_decimal(value: &str) -> bool {
	!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}
