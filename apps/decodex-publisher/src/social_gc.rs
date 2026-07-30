//! Bounded garbage collection for private social automation state.

mod inventory;
mod journal;
mod plan;

#[cfg(test)] mod tests;

use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	SocialGcReport, SocialGcRequest,
	prelude::{Result, eyre},
};

const DAILY_STRATEGY_KEEP: usize = 14;
const WEEKLY_STRATEGY_KEEP: usize = 8;
const MINIMUM_RETENTION_DAYS: i64 = 10;
const MAX_GC_ENTRIES: usize = 8_192;
const MAX_GC_FILES: usize = 4_096;
const MAX_GC_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
struct GcPolicy {
	daily_strategy_keep: usize,
	weekly_strategy_keep: usize,
	minimum_retention: Duration,
	max_entries: usize,
	max_files: usize,
	max_bytes: u64,
}
impl Default for GcPolicy {
	fn default() -> Self {
		Self {
			daily_strategy_keep: DAILY_STRATEGY_KEEP,
			weekly_strategy_keep: WEEKLY_STRATEGY_KEEP,
			minimum_retention: Duration::days(MINIMUM_RETENTION_DAYS),
			max_entries: MAX_GC_ENTRIES,
			max_files: MAX_GC_FILES,
			max_bytes: MAX_GC_BYTES,
		}
	}
}

#[derive(Debug)]
struct GcFailure(&'static str);
type GcResult<T> = std::result::Result<T, GcFailure>;

#[derive(Clone, Debug, Eq, PartialEq)]
enum GcJournalStep {
	BeforeJournalFileSync,
	AfterJournalFileSync,
	BeforeJournalPublish,
	AfterJournalPublish,
	BeforeJournalPublishDirectorySync,
	AfterJournalPublishDirectorySync,
	BeforePlannedFileUnlink(String),
	AfterPlannedFileUnlink(String),
	BeforeDataDirectorySync((u64, u64)),
	AfterDataDirectorySync((u64, u64)),
	BeforeJournalUnlink,
	AfterJournalUnlink,
	BeforeJournalRemovalDirectorySync,
	AfterJournalRemovalDirectorySync,
}

pub(crate) fn gc_social(request: &SocialGcRequest) -> Result<SocialGcReport> {
	gc_social_with(request, GcPolicy::default(), || {}).map_err(|failure| eyre::eyre!(failure.0))
}

pub(crate) fn validate_default_state(request: &SocialGcRequest) -> Result<usize> {
	let now = parse_now(request)?;
	let _state_lock = crate::social_publish::scan::acquire_social_state_lock(&request.locks_dir)
		.map_err(|_| eyre::eyre!("social_gc_lock_failed"))?;
	let mut no_fault = |_| Ok(());
	journal::recover(request, &mut no_fault).map_err(|failure| eyre::eyre!(failure.0))?;
	let inventory = inventory::scan(request, GcPolicy::default(), now)
		.map_err(|_| eyre::eyre!("social_gc_scan_invalid"))?;
	plan::build(&inventory, GcPolicy::default(), now)
		.map_err(|_| eyre::eyre!("social_gc_contract_invalid"))?;

	Ok(inventory.scanned_files)
}

fn gc_social_with(
	request: &SocialGcRequest,
	policy: GcPolicy,
	before_delete: impl FnOnce(),
) -> GcResult<SocialGcReport> {
	let mut no_fault = |_| Ok(());
	gc_social_with_hooks(request, policy, before_delete, &mut no_fault)
}

fn gc_social_with_hooks(
	request: &SocialGcRequest,
	policy: GcPolicy,
	before_delete: impl FnOnce(),
	hook: &mut impl FnMut(GcJournalStep) -> GcResult<()>,
) -> GcResult<SocialGcReport> {
	let now = parse_now(request).map_err(|_| GcFailure("social_gc_request_invalid"))?;
	let _state_lock = crate::social_publish::scan::acquire_social_state_lock(&request.locks_dir)
		.map_err(|_| GcFailure("social_gc_lock_failed"))?;
	journal::recover(request, hook)?;
	let inventory =
		inventory::scan(request, policy, now).map_err(|_| GcFailure("social_gc_scan_invalid"))?;
	let deletion_plan = plan::build(&inventory, policy, now)
		.map_err(|_| GcFailure("social_gc_contract_invalid"))?;

	before_delete();
	deletion_plan.preflight().map_err(|_| GcFailure("social_gc_delete_race"))?;
	journal::persist(request, &deletion_plan.files, hook)?;
	if !deletion_plan.files.is_empty() {
		journal::recover(request, hook)?;
	}

	let mut reason_codes = Vec::new();
	if deletion_plan.deleted_strategies > 0 {
		reason_codes.push("expired_strategies_pruned".into());
	}
	if deletion_plan.deleted_lineages > 0 {
		reason_codes.push("terminal_lineages_pruned".into());
	}
	if deletion_plan.retained_by_strategy {
		reason_codes.push("strategy_reference_retained".into());
	}
	if deletion_plan.retained_by_current_month {
		reason_codes.push("current_billing_month_retained".into());
	}
	if deletion_plan.retained_nonterminal {
		reason_codes.push("nonterminal_lineage_retained".into());
	}
	if deletion_plan.retained_by_window {
		reason_codes.push("retention_window_retained".into());
	}
	if reason_codes.is_empty() {
		reason_codes.push("nothing_eligible".into());
	}

	Ok(SocialGcReport {
		status: "complete".into(),
		reason_codes,
		scanned_files: inventory.scanned_files,
		deleted_lineages: deletion_plan.deleted_lineages,
		deleted_files: deletion_plan.files.len(),
		deleted_strategies: deletion_plan.deleted_strategies,
		retained_lineages: deletion_plan.retained_lineages,
		retained_strategies: deletion_plan.retained_strategies,
	})
}

fn parse_now(request: &SocialGcRequest) -> Result<OffsetDateTime> {
	OffsetDateTime::parse(&request.now, &Rfc3339)
		.map_err(|_| eyre::eyre!("social GC now must be an RFC3339 timestamp"))
}

fn current_billing_month(now: OffsetDateTime) -> String {
	format!("{:04}-{:02}", now.year(), u8::from(now.month()))
}

fn digest_hex(bytes: &[u8]) -> String {
	use sha2::{Digest as _, Sha256};

	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn one_string_ref(value: &serde_json::Value, object: &str, field: &str) -> Option<String> {
	value
		.get(object)
		.and_then(serde_json::Value::as_object)
		.and_then(|refs| refs.get(field))
		.and_then(serde_json::Value::as_array)
		.filter(|refs| refs.len() == 1)
		.and_then(|refs| refs.first())
		.and_then(serde_json::Value::as_str)
		.map(Into::into)
}

fn required_string<'a>(value: &'a serde_json::Value, field: &str) -> Option<&'a str> {
	value.get(field).and_then(serde_json::Value::as_str).filter(|value| !value.is_empty())
}
