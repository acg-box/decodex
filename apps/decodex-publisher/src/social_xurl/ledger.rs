use std::{
	collections::{BTreeMap, BTreeSet},
	fs,
	path::{Path, PathBuf},
};

use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::model::{
	ATTEMPT_SCHEMA, CREATE_COST_MICROUSD, IDENTITY_READ_COST_MICROUSD,
	IDENTITY_RECOVERY_EXHAUSTED_STATUS, NO_CREATE_RELEASED_STATUS,
	NORMAL_PUBLICATION_COST_MICROUSD, OBSERVATION_ATTEMPT_SCHEMA,
	PUBLICATION_LINEAGE_BUDGET_MICROUSD, READ_COST_MICROUSD, READ_RECOVERY_EXHAUSTED_STATUS,
	XurlAttempt, XurlCall, XurlObservationAttempt, XurlReconciliation,
};
use crate::{
	SOCIAL_MONTHLY_BUDGET_MICROUSD, SocialXurlCostReport,
	prelude::{Result, eyre},
};

#[derive(Default)]
struct CostTotals {
	reserved: u64,
	used: u64,
	publication_attempts: u64,
	observation_attempts: u64,
	identity_reads: u64,
	content_creates: u64,
	post_reads: u64,
	total_calls: u64,
}

pub(super) struct CallCompletion<'a> {
	pub(super) call_status: &'a str,
	pub(super) response_sha256: Option<String>,
	pub(super) status: &'a str,
	pub(super) updated_at: &'a str,
	pub(super) verified_user_id: Option<&'a str>,
	pub(super) post_id: Option<&'a str>,
	pub(super) published_url: Option<&'a str>,
}

pub(super) fn load_attempt(path: &Path) -> Result<XurlAttempt> {
	serde_json::from_value(crate::load_json(path)?)
		.map_err(|_| eyre::eyre!("xurl publication attempt is invalid"))
}

pub(super) fn load_observation_attempt(path: &Path) -> Result<XurlObservationAttempt> {
	serde_json::from_value(crate::load_json(path)?)
		.map_err(|_| eyre::eyre!("xurl observation attempt is invalid"))
}

pub(super) fn publication_effect_conflict(
	attempts_dir: &Path,
	publication_lineage_sha256: &str,
	excluded_attempt_path: Option<&Path>,
) -> Result<Option<PathBuf>> {
	if !attempts_dir.exists() {
		return Ok(None);
	}
	for path in crate::collect_json_files(&[attempts_dir.to_path_buf()])? {
		if excluded_attempt_path == Some(path.as_path()) {
			continue;
		}
		let payload = crate::load_json(&path)?;
		if payload.get("schema").and_then(Value::as_str) != Some(ATTEMPT_SCHEMA) {
			continue;
		}
		let attempt: XurlAttempt = serde_json::from_value(payload)
			.map_err(|_| eyre::eyre!("{} is not a valid xurl attempt", path.display()))?;
		if attempt.publication_lineage_sha256 != publication_lineage_sha256 {
			continue;
		}
		if publication_effect_started(&attempt) {
			return Ok(Some(path));
		}
	}

	Ok(None)
}

pub(super) fn daily_publication_effect_conflict(
	attempts_dir: &Path,
	day: &str,
) -> Result<Option<PathBuf>> {
	let day = OffsetDateTime::parse(&format!("{day}T00:00:00Z"), &Rfc3339)
		.map_err(|_| eyre::eyre!("publication day is invalid"))?
		.date();
	if !attempts_dir.exists() {
		return Ok(None);
	}
	for path in crate::collect_json_files(&[attempts_dir.to_path_buf()])? {
		let payload = crate::load_json(&path)?;
		match payload.get("schema").and_then(Value::as_str) {
			Some(OBSERVATION_ATTEMPT_SCHEMA) => continue,
			Some(ATTEMPT_SCHEMA) => {},
			_ => return Err(eyre::eyre!("{} has invalid xurl billing lineage", path.display())),
		}
		let attempt: XurlAttempt = serde_json::from_value(payload)
			.map_err(|_| eyre::eyre!("{} is not a valid xurl attempt", path.display()))?;
		validate_publication_cost_record(&attempt)?;
		let created_at = OffsetDateTime::parse(&attempt.created_at, &Rfc3339)
			.map_err(|_| eyre::eyre!("xurl publication attempt timestamp is invalid"))?;
		if created_at.to_offset(time::UtcOffset::UTC).date() == day
			&& publication_effect_started(&attempt)
		{
			return Ok(Some(path));
		}
	}

	Ok(None)
}

fn publication_effect_started(attempt: &XurlAttempt) -> bool {
	attempt.post_id.is_some()
		|| attempt.calls.iter().any(|call| {
			call.operation == "content_create"
				&& matches!(call.status.as_str(), "inflight" | "succeeded" | "uncertain")
		})
}

pub(super) fn observation_attempt_exists(
	attempts_dir: &Path,
	post_ref: &str,
	window: &str,
) -> Result<bool> {
	if !attempts_dir.exists() {
		return Ok(false);
	}
	for path in crate::collect_json_files(&[attempts_dir.to_path_buf()])? {
		let payload = crate::load_json(&path)?;
		if payload.get("schema").and_then(Value::as_str) != Some(OBSERVATION_ATTEMPT_SCHEMA) {
			continue;
		}
		let attempt: XurlObservationAttempt = serde_json::from_value(payload).map_err(|_| {
			eyre::eyre!("{} is not a valid xurl observation attempt", path.display())
		})?;
		if attempt.post_ref == post_ref && attempt.window == window {
			return Ok(true);
		}
	}

	Ok(false)
}

pub(super) fn monthly_reserved_cost(attempts_dir: &Path, billing_month: &str) -> Result<u64> {
	Ok(scan_costs(attempts_dir, billing_month, false)?.reserved)
}

pub(super) fn cost_report(
	attempts_dir: &Path,
	billing_month: &str,
) -> Result<SocialXurlCostReport> {
	let totals = scan_costs(attempts_dir, billing_month, true)?;
	let remaining = SOCIAL_MONTHLY_BUDGET_MICROUSD
		.checked_sub(totals.reserved)
		.ok_or_else(|| eyre::eyre!("monthly X budget ledger exceeds its hard cap"))?;

	Ok(SocialXurlCostReport {
		status: "ok".into(),
		billing_month: billing_month.into(),
		used_cost_ceiling_microusd: totals.used,
		reserved_cost_ceiling_microusd: totals.reserved,
		monthly_cap_microusd: SOCIAL_MONTHLY_BUDGET_MICROUSD,
		remaining_cost_ceiling_microusd: remaining,
		publication_attempt_count: totals.publication_attempts,
		observation_attempt_count: totals.observation_attempts,
		identity_read_call_count: totals.identity_reads,
		content_create_call_count: totals.content_creates,
		post_read_call_count: totals.post_reads,
		total_call_count: totals.total_calls,
	})
}

fn scan_costs(attempts_dir: &Path, billing_month: &str, strict: bool) -> Result<CostTotals> {
	if !valid_billing_month(billing_month) {
		return Err(eyre::eyre!("xurl billing month is invalid"));
	}
	if !attempts_dir.exists() {
		return Ok(CostTotals::default());
	}
	let metadata = fs::symlink_metadata(attempts_dir)?;
	if metadata.file_type().is_symlink() || !metadata.is_dir() {
		return Err(eyre::eyre!("xurl attempts path must be a directory"));
	}
	let mut totals = CostTotals::default();
	let mut lineage_reserved = BTreeMap::<String, u64>::new();
	for path in crate::collect_json_files(&[attempts_dir.to_path_buf()])? {
		let payload = crate::load_json(&path)?;
		let schema = payload.get("schema").and_then(Value::as_str);
		let (charges, calls, publication, publication_lineage_sha256, lineage_cost) = match schema {
			Some(ATTEMPT_SCHEMA) => {
				let attempt: XurlAttempt = serde_json::from_value(payload).map_err(|_| {
					eyre::eyre!("{} is not a valid xurl publication usage record", path.display())
				})?;
				validate_usage_path(&path, &attempt.billing_month)?;
				if strict {
					validate_publication_cost_record(&attempt)?;
				}
				let charges = publication_charges(&attempt)?;
				(
					charges,
					attempt.calls,
					true,
					attempt.publication_lineage_sha256,
					attempt.reserved_cost_ceiling_microusd,
				)
			},
			Some(OBSERVATION_ATTEMPT_SCHEMA) => {
				let attempt: XurlObservationAttempt =
					serde_json::from_value(payload).map_err(|_| {
						eyre::eyre!(
							"{} is not a valid xurl observation usage record",
							path.display()
						)
					})?;
				validate_usage_path(&path, &attempt.billing_month)?;
				if strict {
					validate_observation_cost_record(&attempt)?;
				}
				let charges = observation_charges(&attempt)?;
				(
					charges,
					attempt.calls,
					false,
					attempt.publication_lineage_sha256,
					attempt.reserved_cost_ceiling_microusd,
				)
			},
			_ => return Err(eyre::eyre!("{} has invalid xurl billing lineage", path.display())),
		};
		let lineage_total = lineage_reserved.entry(publication_lineage_sha256).or_default();
		*lineage_total = lineage_total
			.checked_add(lineage_cost)
			.ok_or_else(|| eyre::eyre!("publication lineage budget arithmetic overflowed"))?;
		if *lineage_total > PUBLICATION_LINEAGE_BUDGET_MICROUSD {
			return Err(eyre::eyre!("publication lineage budget ledger exceeds its hard cap"));
		}
		let charged_this_month = charges.iter().any(|(month, _)| month == billing_month);
		if charged_this_month {
			if publication {
				totals.publication_attempts = checked_increment(totals.publication_attempts)?;
			} else {
				totals.observation_attempts = checked_increment(totals.observation_attempts)?;
			}
		}
		for (charge_month, cost) in &charges {
			if charge_month == billing_month {
				totals.reserved = totals
					.reserved
					.checked_add(*cost)
					.ok_or_else(|| eyre::eyre!("monthly X budget arithmetic overflowed"))?;
			}
		}
		for call in calls {
			let call_month = call.billing_month.as_deref().unwrap_or_else(|| {
				charges.first().map(|(month, _)| month.as_str()).unwrap_or_default()
			});
			if call_month != billing_month {
				continue;
			}
			totals.used = totals
				.used
				.checked_add(call.recorded_cost_ceiling_microusd)
				.ok_or_else(|| eyre::eyre!("monthly X budget arithmetic overflowed"))?;
			totals.total_calls = checked_increment(totals.total_calls)?;
			match call.operation.as_str() {
				"identity_read" | "identity_read_reconcile" => {
					totals.identity_reads = checked_increment(totals.identity_reads)?;
				},
				"content_create" => {
					totals.content_creates = checked_increment(totals.content_creates)?;
				},
				_ => totals.post_reads = checked_increment(totals.post_reads)?,
			}
		}
	}
	if totals.reserved > SOCIAL_MONTHLY_BUDGET_MICROUSD || totals.used > totals.reserved {
		return Err(eyre::eyre!("monthly X budget ledger exceeds its hard cap"));
	}

	Ok(totals)
}

pub(crate) fn validate_publication_cost_record(attempt: &XurlAttempt) -> Result<()> {
	if attempt.xurl_version != super::auth_contract::APPROVED_XURL_VERSION
		|| attempt.schema != ATTEMPT_SCHEMA
		|| !crate::social_publish::valid_run_id(&attempt.run_id)
		|| !valid_billing_month(&attempt.billing_month)
		|| attempt.target_account != super::model::TARGET_ACCOUNT
		|| !lowercase_digest(&attempt.publication_lineage_sha256)
		|| attempt.idempotency_key
			!= format!("content-publication:{}", attempt.publication_lineage_sha256)
		|| attempt.pricing_policy_id.as_deref() != Some(super::model::PRICING_POLICY_ID)
		|| attempt
			.authorization_contract_sha256
			.as_deref()
			.is_none_or(|digest| !lowercase_digest(digest))
		|| attempt.calls.len() > 5
		|| !matches!(
			attempt.status.as_str(),
			"reserved"
				| "identity_inflight"
				| "identity_reconcile_inflight"
				| "identity_reconcile_halted"
				| "identity_reconciled"
				| NO_CREATE_RELEASED_STATUS
				| IDENTITY_RECOVERY_EXHAUSTED_STATUS
				| "identity_verified"
				| "create_inflight"
				| "create_uncertain"
				| "created" | "read_inflight"
				| "read_retry_inflight"
				| "read_retry_pending"
				| "read_reconcile_inflight"
				| "read_reconcile_halted"
				| READ_RECOVERY_EXHAUSTED_STATUS
				| "halted" | "verified"
				| "published"
		) || OffsetDateTime::parse(&attempt.created_at, &Rfc3339).is_err()
		|| OffsetDateTime::parse(&attempt.updated_at, &Rfc3339).is_err()
		|| matches!(
			attempt.status.as_str(),
			NO_CREATE_RELEASED_STATUS
				| IDENTITY_RECOVERY_EXHAUSTED_STATUS
				| READ_RECOVERY_EXHAUSTED_STATUS
		) && attempt.reconciliation.is_none()
	{
		return Err(eyre::eyre!("xurl publication usage authority is invalid"));
	}
	for call in &attempt.calls {
		validate_cost_call(call)?;
	}
	publication_charges(attempt)?;
	validate_publication_call_sequence(attempt)?;
	validate_publication_state(attempt)?;
	Ok(())
}

pub(crate) fn validate_observation_cost_record(attempt: &XurlObservationAttempt) -> Result<()> {
	if attempt.schema != OBSERVATION_ATTEMPT_SCHEMA
		|| !crate::social_publish::valid_run_id(&attempt.run_id)
		|| !valid_billing_month(&attempt.billing_month)
		|| !matches!(attempt.window.as_str(), "24h" | "7d")
		|| !lowercase_digest(&attempt.publication_lineage_sha256)
		|| attempt.pricing_policy_id.as_deref() != Some(super::model::PRICING_POLICY_ID)
		|| attempt
			.authorization_contract_sha256
			.as_deref()
			.is_none_or(|digest| !lowercase_digest(digest))
		|| !(1..=3).contains(&attempt.calls.len())
		|| attempt.calls.last() != Some(&attempt.call)
		|| !matches!(
			attempt.status.as_str(),
			"read_inflight"
				| "read_reconcile_inflight"
				| "read_reconcile_halted"
				| READ_RECOVERY_EXHAUSTED_STATUS
				| "halted" | "observed"
		) || OffsetDateTime::parse(&attempt.created_at, &Rfc3339).is_err()
		|| OffsetDateTime::parse(&attempt.updated_at, &Rfc3339).is_err()
		|| attempt.status == READ_RECOVERY_EXHAUSTED_STATUS && attempt.reconciliation.is_none()
	{
		return Err(eyre::eyre!("xurl observation usage authority is invalid"));
	}
	for call in &attempt.calls {
		validate_cost_call(call)?;
	}
	observation_charges(attempt)?;
	validate_observation_call_sequence(attempt)?;
	validate_observation_state(attempt)?;
	Ok(())
}

fn validate_cost_call(call: &XurlCall) -> Result<()> {
	if !matches!(
		call.status.as_str(),
		"inflight" | "succeeded" | "failed" | "invalid" | "uncertain"
	) || call.response_sha256.as_deref().is_some_and(|digest| !lowercase_digest(digest))
		|| matches!(call.status.as_str(), "succeeded" | "invalid") && call.response_sha256.is_none()
		|| call.status == "inflight" && call.response_sha256.is_some()
		|| call
			.operation_id
			.as_deref()
			.is_some_and(|operation_id| !crate::social_publish::valid_run_id(operation_id))
	{
		return Err(eyre::eyre!("xurl usage call is invalid"));
	}
	Ok(())
}

fn validate_publication_call_sequence(attempt: &XurlAttempt) -> Result<()> {
	let calls = &attempt.calls;
	if calls.is_empty() {
		return Ok(());
	}
	if calls[0].operation != "identity_read" || !initial_call_metadata(&calls[0]) {
		return Err(eyre::eyre!("xurl publication usage call sequence is invalid"));
	}
	if calls.len() == 1 {
		return Ok(());
	}

	match calls[1].operation.as_str() {
		"identity_read_reconcile" => validate_identity_recovery_sequence(attempt, calls)?,
		"content_create" => validate_create_and_read_sequence(attempt, calls)?,
		_ => return Err(eyre::eyre!("xurl publication usage call sequence is invalid")),
	}
	validate_unique_recovery_owners(&attempt.run_id, calls)
}

fn validate_identity_recovery_sequence(attempt: &XurlAttempt, calls: &[XurlCall]) -> Result<()> {
	if calls.len() > 3 || !interrupted_call(&calls[0]) {
		return Err(eyre::eyre!("xurl identity recovery sequence is invalid"));
	}
	for (index, call) in calls[1..].iter().enumerate() {
		if call.operation != "identity_read_reconcile"
			|| !recovery_call_metadata(call, false)
			|| index + 2 < calls.len() && !interrupted_call(call)
		{
			return Err(eyre::eyre!("xurl identity recovery sequence is invalid"));
		}
	}
	if attempt.post_id.is_some() || attempt.published_url.is_some() {
		return Err(eyre::eyre!("xurl identity recovery state has a public post identity"));
	}

	Ok(())
}

fn validate_create_and_read_sequence(attempt: &XurlAttempt, calls: &[XurlCall]) -> Result<()> {
	if calls[0].status != "succeeded"
		|| calls[1].operation != "content_create"
		|| !initial_call_metadata(&calls[1])
	{
		return Err(eyre::eyre!("xurl publication create sequence is invalid"));
	}
	if calls.len() == 2 {
		return Ok(());
	}
	if calls[1].status != "succeeded" {
		return Err(eyre::eyre!("xurl publication read sequence is invalid"));
	}

	let reads = &calls[2..];
	if reads.len() > 3 {
		return Err(eyre::eyre!("xurl publication read sequence is invalid"));
	}
	for (index, call) in reads.iter().enumerate() {
		let valid_operation = match index {
			0 => matches!(
				call.operation.as_str(),
				"post_read_initial" | "post_read_initial_reconcile"
			),
			1 => matches!(call.operation.as_str(), "post_read_retry" | "post_read_reconcile"),
			2 => call.operation == "post_read_reconcile",
			_ => false,
		};
		let valid_metadata = match call.operation.as_str() {
			"post_read_initial" => initial_call_metadata(call),
			"post_read_initial_reconcile" => recovery_call_metadata(call, true),
			"post_read_retry" => retry_call_metadata(call),
			"post_read_reconcile" => recovery_call_metadata(call, false),
			_ => false,
		};
		if !valid_operation
			|| !valid_metadata
			|| index > 0 && !interrupted_call(&reads[index - 1])
			|| index + 1 < reads.len() && !interrupted_call(call)
		{
			return Err(eyre::eyre!("xurl publication read sequence is invalid"));
		}
	}
	if attempt.verified_user_id.is_none() || attempt.post_id.is_none() {
		return Err(eyre::eyre!("xurl publication read state lacks its public post identity"));
	}

	Ok(())
}

fn validate_observation_call_sequence(attempt: &XurlObservationAttempt) -> Result<()> {
	let calls = &attempt.calls;
	if calls[0].operation != "outcome_read" || !initial_call_metadata(&calls[0]) {
		return Err(eyre::eyre!("xurl observation usage call sequence is invalid"));
	}
	for (index, call) in calls[1..].iter().enumerate() {
		if call.operation != "outcome_read_reconcile"
			|| !recovery_call_metadata(call, false)
			|| !interrupted_call(&calls[index])
		{
			return Err(eyre::eyre!("xurl observation recovery sequence is invalid"));
		}
	}
	validate_unique_recovery_owners(&attempt.run_id, calls)
}

fn validate_unique_recovery_owners(run_id: &str, calls: &[XurlCall]) -> Result<()> {
	let mut owners = BTreeSet::new();
	for call in calls {
		let Some(owner) = call.operation_id.as_deref() else {
			continue;
		};
		if owner == run_id || !owners.insert(owner) {
			return Err(eyre::eyre!("xurl usage recovery owner is invalid"));
		}
	}

	Ok(())
}

fn initial_call_metadata(call: &XurlCall) -> bool {
	call.operation_id.is_none() && call.billing_month.is_none()
}

fn recovery_call_metadata(call: &XurlCall, billing_month_optional: bool) -> bool {
	call.operation_id.as_deref().is_some_and(crate::social_publish::valid_run_id)
		&& (billing_month_optional || call.billing_month.is_some())
}

fn retry_call_metadata(call: &XurlCall) -> bool {
	call.operation_id.is_none() && call.billing_month.is_some()
}

fn interrupted_call(call: &XurlCall) -> bool {
	matches!(call.status.as_str(), "failed" | "invalid" | "uncertain")
}

fn validate_publication_state(attempt: &XurlAttempt) -> Result<()> {
	let last = attempt.calls.last();
	let valid = match attempt.status.as_str() {
		"reserved" => attempt.calls.is_empty(),
		"identity_inflight" => call_state(last, &["identity_read"], &["inflight"]),
		"identity_reconcile_inflight" =>
			call_state(last, &["identity_read_reconcile"], &["inflight"]),
		"identity_reconcile_halted" =>
			call_state(last, &["identity_read_reconcile"], &["failed", "invalid"]),
		"identity_reconciled" => call_state(last, &["identity_read_reconcile"], &["succeeded"]),
		NO_CREATE_RELEASED_STATUS =>
			(attempt.calls.is_empty()
				|| call_state(
					last,
					&["identity_read", "identity_read_reconcile"],
					&["succeeded", "failed", "invalid", "uncertain"],
				)) && attempt.calls.iter().all(|call| {
				matches!(call.operation.as_str(), "identity_read" | "identity_read_reconcile")
			}) && attempt.post_id.is_none()
				&& attempt.published_url.is_none(),
		IDENTITY_RECOVERY_EXHAUSTED_STATUS =>
			call_state(last, &["identity_read_reconcile"], &["failed", "invalid", "uncertain"])
				&& attempt.post_id.is_none()
				&& attempt.published_url.is_none(),
		"identity_verified" =>
			call_state(last, &["identity_read", "identity_read_reconcile"], &["succeeded"]),
		"create_inflight" => call_state(last, &["content_create"], &["inflight"]),
		"create_uncertain" => call_state(last, &["content_create"], &["uncertain"]),
		"created" => call_state(last, &["content_create"], &["succeeded"]),
		"read_inflight" => call_state(last, &["post_read_initial"], &["inflight"]),
		"read_retry_pending" =>
			call_state(last, &["post_read_initial"], &["failed", "invalid", "uncertain"]),
		"read_retry_inflight" => call_state(last, &["post_read_retry"], &["inflight"]),
		"read_reconcile_inflight" =>
			call_state(last, &["post_read_initial_reconcile", "post_read_reconcile"], &["inflight"]),
		"read_reconcile_halted" => call_state(
			last,
			&["post_read_initial_reconcile", "post_read_reconcile"],
			&["failed", "invalid"],
		),
		READ_RECOVERY_EXHAUSTED_STATUS => call_state(
			last,
			&[
				"post_read_initial",
				"post_read_initial_reconcile",
				"post_read_retry",
				"post_read_reconcile",
			],
			&["failed", "invalid", "uncertain"],
		),
		"halted" => last.is_some_and(|call| matches!(call.status.as_str(), "failed" | "invalid")),
		"verified" | "published" => call_state(
			last,
			&[
				"post_read_initial",
				"post_read_initial_reconcile",
				"post_read_reconcile",
				"post_read_retry",
			],
			&["succeeded"],
		),
		_ => false,
	};
	if !valid {
		return Err(eyre::eyre!("xurl publication usage state is invalid"));
	}

	Ok(())
}

fn validate_observation_state(attempt: &XurlObservationAttempt) -> Result<()> {
	let last = attempt.calls.last();
	let valid = match attempt.status.as_str() {
		"read_inflight" => call_state(last, &["outcome_read"], &["inflight"]),
		"read_reconcile_inflight" => call_state(last, &["outcome_read_reconcile"], &["inflight"]),
		"read_reconcile_halted" =>
			call_state(last, &["outcome_read_reconcile"], &["failed", "invalid"]),
		READ_RECOVERY_EXHAUSTED_STATUS => call_state(
			last,
			&["outcome_read", "outcome_read_reconcile"],
			&["failed", "invalid", "uncertain"],
		),
		"halted" =>
			call_state(last, &["outcome_read", "outcome_read_reconcile"], &["failed", "invalid"]),
		"observed" => call_state(last, &["outcome_read", "outcome_read_reconcile"], &["succeeded"]),
		_ => false,
	};
	if !valid {
		return Err(eyre::eyre!("xurl observation usage state is invalid"));
	}

	Ok(())
}

fn call_state(call: Option<&XurlCall>, operations: &[&str], statuses: &[&str]) -> bool {
	call.is_some_and(|call| {
		operations.contains(&call.operation.as_str()) && statuses.contains(&call.status.as_str())
	})
}

fn lowercase_digest(value: &str) -> bool {
	value.len() == 64
		&& value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn checked_increment(value: u64) -> Result<u64> {
	value.checked_add(1).ok_or_else(|| eyre::eyre!("xurl cost count overflowed"))
}

fn publication_charges(attempt: &XurlAttempt) -> Result<Vec<(String, u64)>> {
	let no_create_terminal = attempt.reconciliation.is_some()
		&& matches!(
			attempt.status.as_str(),
			"identity_reconciled" | NO_CREATE_RELEASED_STATUS | IDENTITY_RECOVERY_EXHAUSTED_STATUS
		);
	let base_reservation = if no_create_terminal {
		attempt.calls.iter().filter(|call| call.billing_month.is_none()).try_fold(
			0_u64,
			|total, call| {
				total
					.checked_add(call.recorded_cost_ceiling_microusd)
					.ok_or_else(|| eyre::eyre!("xurl publication usage arithmetic overflowed"))
			},
		)?
	} else {
		NORMAL_PUBLICATION_COST_MICROUSD
	};
	let mut charges = vec![(attempt.billing_month.clone(), base_reservation)];
	let mut reserved = base_reservation;
	for call in &attempt.calls {
		let expected_cost = match call.operation.as_str() {
			"identity_read" | "identity_read_reconcile" => IDENTITY_READ_COST_MICROUSD,
			"content_create" => CREATE_COST_MICROUSD,
			"post_read_initial"
			| "post_read_initial_reconcile"
			| "post_read_retry"
			| "post_read_reconcile" => READ_COST_MICROUSD,
			_ => return Err(eyre::eyre!("xurl publication usage operation is invalid")),
		};
		if call.recorded_cost_ceiling_microusd != expected_cost
			|| matches!(
				call.operation.as_str(),
				"identity_read" | "content_create" | "post_read_initial"
			) && call.billing_month.is_some()
			|| matches!(
				call.operation.as_str(),
				"identity_read_reconcile" | "post_read_retry" | "post_read_reconcile"
			) && call.billing_month.is_none()
		{
			return Err(eyre::eyre!("xurl publication usage charge is invalid"));
		}
		if let Some(month) = &call.billing_month {
			if !valid_billing_month(month) {
				return Err(eyre::eyre!("xurl publication call billing month is invalid"));
			}
			reserved = reserved
				.checked_add(call.recorded_cost_ceiling_microusd)
				.ok_or_else(|| eyre::eyre!("xurl publication usage arithmetic overflowed"))?;
			charges.push((month.clone(), call.recorded_cost_ceiling_microusd));
		}
	}
	if attempt.reserved_cost_ceiling_microusd != reserved {
		return Err(eyre::eyre!("xurl publication usage reservation is inconsistent"));
	}
	if reserved > PUBLICATION_LINEAGE_BUDGET_MICROUSD {
		return Err(eyre::eyre!("xurl publication lineage reservation exceeds its hard cap"));
	}

	Ok(charges)
}

fn observation_charges(attempt: &XurlObservationAttempt) -> Result<Vec<(String, u64)>> {
	let mut charges = vec![(attempt.billing_month.clone(), READ_COST_MICROUSD)];
	let mut reserved = READ_COST_MICROUSD;
	for call in &attempt.calls {
		if call.recorded_cost_ceiling_microusd != READ_COST_MICROUSD
			|| (call.operation == "outcome_read") != call.billing_month.is_none()
			|| !matches!(call.operation.as_str(), "outcome_read" | "outcome_read_reconcile")
		{
			return Err(eyre::eyre!("xurl observation usage charge is invalid"));
		}
		if let Some(month) = &call.billing_month {
			if !valid_billing_month(month) {
				return Err(eyre::eyre!("xurl observation call billing month is invalid"));
			}
			reserved = reserved
				.checked_add(call.recorded_cost_ceiling_microusd)
				.ok_or_else(|| eyre::eyre!("xurl observation usage arithmetic overflowed"))?;
			charges.push((month.clone(), call.recorded_cost_ceiling_microusd));
		}
	}
	if attempt.reserved_cost_ceiling_microusd != reserved {
		return Err(eyre::eyre!("xurl observation usage reservation is inconsistent"));
	}
	if reserved > PUBLICATION_LINEAGE_BUDGET_MICROUSD {
		return Err(eyre::eyre!("xurl observation lineage reservation exceeds its hard cap"));
	}

	Ok(charges)
}

fn validate_usage_path(path: &Path, billing_month: &str) -> Result<()> {
	if !valid_billing_month(billing_month)
		|| path.parent().and_then(Path::file_name).and_then(|value| value.to_str())
			!= Some(billing_month)
	{
		return Err(eyre::eyre!("{} has invalid xurl billing lineage", path.display()));
	}

	Ok(())
}

pub(super) fn valid_billing_month(value: &str) -> bool {
	let bytes = value.as_bytes();
	bytes.len() == 7
		&& bytes[4] == b'-'
		&& bytes[..4].iter().all(u8::is_ascii_digit)
		&& bytes[5..].iter().all(u8::is_ascii_digit)
		&& matches!(
			&bytes[5..],
			b"01"
				| b"02" | b"03"
				| b"04" | b"05"
				| b"06" | b"07"
				| b"08" | b"09"
				| b"10" | b"11"
				| b"12"
		)
}

pub(super) fn ensure_budget(
	attempts_dir: &Path,
	billing_month: &str,
	additional_microusd: u64,
) -> Result<u64> {
	let reserved = monthly_reserved_cost(attempts_dir, billing_month)?;
	let next = reserved
		.checked_add(additional_microusd)
		.ok_or_else(|| eyre::eyre!("monthly X budget arithmetic overflowed"))?;
	if next > SOCIAL_MONTHLY_BUDGET_MICROUSD {
		return Err(eyre::eyre!(
			"monthly X budget exhausted for {billing_month}: reserved={reserved}, next={additional_microusd}, limit={SOCIAL_MONTHLY_BUDGET_MICROUSD}"
		));
	}

	Ok(next)
}

pub(super) fn ensure_lineage_budget(
	attempts_dir: &Path,
	publication_lineage_sha256: &str,
	additional_microusd: u64,
) -> Result<u64> {
	if !lowercase_digest(publication_lineage_sha256) {
		return Err(eyre::eyre!("publication lineage digest is invalid"));
	}
	let reserved = lineage_reserved_cost(attempts_dir, publication_lineage_sha256)?;
	let next = reserved
		.checked_add(additional_microusd)
		.ok_or_else(|| eyre::eyre!("publication lineage budget arithmetic overflowed"))?;
	if next > PUBLICATION_LINEAGE_BUDGET_MICROUSD {
		return Err(eyre::eyre!(
			"publication lineage budget exhausted: reserved={reserved}, next={additional_microusd}, limit={PUBLICATION_LINEAGE_BUDGET_MICROUSD}"
		));
	}

	Ok(next)
}

fn lineage_reserved_cost(attempts_dir: &Path, publication_lineage_sha256: &str) -> Result<u64> {
	if !attempts_dir.exists() {
		return Ok(0);
	}
	let mut reserved = 0_u64;
	for path in crate::collect_json_files(&[attempts_dir.to_path_buf()])? {
		let payload = crate::load_json(&path)?;
		let (lineage, cost) = match payload.get("schema").and_then(Value::as_str) {
			Some(ATTEMPT_SCHEMA) => {
				let attempt: XurlAttempt = serde_json::from_value(payload).map_err(|_| {
					eyre::eyre!("{} is not a valid xurl publication usage record", path.display())
				})?;
				validate_publication_cost_record(&attempt)?;
				(attempt.publication_lineage_sha256, attempt.reserved_cost_ceiling_microusd)
			},
			Some(OBSERVATION_ATTEMPT_SCHEMA) => {
				let attempt: XurlObservationAttempt =
					serde_json::from_value(payload).map_err(|_| {
						eyre::eyre!(
							"{} is not a valid xurl observation usage record",
							path.display()
						)
					})?;
				validate_observation_cost_record(&attempt)?;
				(attempt.publication_lineage_sha256, attempt.reserved_cost_ceiling_microusd)
			},
			_ => return Err(eyre::eyre!("{} has invalid xurl billing lineage", path.display())),
		};
		if lineage == publication_lineage_sha256 {
			reserved = reserved
				.checked_add(cost)
				.ok_or_else(|| eyre::eyre!("publication lineage budget arithmetic overflowed"))?;
		}
	}
	if reserved > PUBLICATION_LINEAGE_BUDGET_MICROUSD {
		return Err(eyre::eyre!("publication lineage budget ledger exceeds its hard cap"));
	}

	Ok(reserved)
}

pub(super) fn remaining_lineage_budget(
	attempts_dir: &Path,
	publication_lineage_sha256: &str,
) -> Result<u64> {
	PUBLICATION_LINEAGE_BUDGET_MICROUSD
		.checked_sub(lineage_reserved_cost(attempts_dir, publication_lineage_sha256)?)
		.ok_or_else(|| eyre::eyre!("publication lineage budget ledger exceeds its hard cap"))
}

pub(super) fn append_call(
	path: &Path,
	attempt: &mut XurlAttempt,
	call: XurlCall,
	status: &str,
	updated_at: &str,
) -> Result<()> {
	let previous = serde_json::to_value(&*attempt)?;
	attempt.calls.push(call);
	attempt.status = status.into();
	attempt.updated_at = updated_at.into();
	replace(path, &previous, attempt)
}

pub(super) fn finish_last_call(
	path: &Path,
	attempt: &mut XurlAttempt,
	completion: CallCompletion<'_>,
) -> Result<()> {
	let previous = serde_json::to_value(&*attempt)?;
	let call =
		attempt.calls.last_mut().ok_or_else(|| eyre::eyre!("xurl attempt has no active call"))?;
	if call.status != "inflight" {
		return Err(eyre::eyre!("xurl attempt call is not inflight"));
	}
	call.status = completion.call_status.into();
	call.response_sha256 = completion.response_sha256;
	attempt.status = completion.status.into();
	attempt.updated_at = completion.updated_at.into();
	if let Some(value) = completion.verified_user_id {
		attempt.verified_user_id = Some(value.into());
	}
	if let Some(value) = completion.post_id {
		attempt.post_id = Some(value.into());
	}
	if let Some(value) = completion.published_url {
		attempt.published_url = Some(value.into());
	}
	replace(path, &previous, attempt)
}

pub(super) fn update_attempt(
	path: &Path,
	attempt: &mut XurlAttempt,
	status: &str,
	updated_at: &str,
) -> Result<()> {
	let previous = serde_json::to_value(&*attempt)?;
	attempt.status = status.into();
	attempt.updated_at = updated_at.into();
	replace(path, &previous, attempt)
}

pub(super) fn reconcile_attempt(
	path: &Path,
	attempt: &mut XurlAttempt,
	status: &str,
	updated_at: &str,
	reconciliation: XurlReconciliation,
) -> Result<()> {
	let previous = serde_json::to_value(&*attempt)?;
	if matches!(
		status,
		"identity_reconciled" | NO_CREATE_RELEASED_STATUS | IDENTITY_RECOVERY_EXHAUSTED_STATUS
	) {
		attempt.reserved_cost_ceiling_microusd =
			attempt.calls.iter().try_fold(0_u64, |total, call| {
				if !matches!(call.operation.as_str(), "identity_read" | "identity_read_reconcile") {
					return Err(eyre::eyre!(
						"identity-only recovery contains a non-identity reservation"
					));
				}
				total
					.checked_add(call.recorded_cost_ceiling_microusd)
					.ok_or_else(|| eyre::eyre!("identity-only recovery budget overflowed"))
			})?;
	}
	if matches!(
		status,
		NO_CREATE_RELEASED_STATUS
			| IDENTITY_RECOVERY_EXHAUSTED_STATUS
			| READ_RECOVERY_EXHAUSTED_STATUS
	) && let Some(call) = attempt.calls.last_mut()
		&& call.status == "inflight"
	{
		call.status = "uncertain".into();
	}
	attempt.status = status.into();
	attempt.updated_at = updated_at.into();
	attempt.reconciliation = Some(reconciliation);
	validate_publication_cost_record(attempt)?;
	replace(path, &previous, attempt)
}

pub(super) fn reserve_retry(
	path: &Path,
	attempt: &mut XurlAttempt,
	attempts_dir: &Path,
	call: XurlCall,
	updated_at: &str,
) -> Result<()> {
	let billing_month = call
		.billing_month
		.as_deref()
		.ok_or_else(|| eyre::eyre!("publication retry lacks a billing month"))?;
	ensure_budget(attempts_dir, billing_month, call.recorded_cost_ceiling_microusd)?;
	ensure_lineage_budget(
		attempts_dir,
		&attempt.publication_lineage_sha256,
		call.recorded_cost_ceiling_microusd,
	)?;
	let previous = serde_json::to_value(&*attempt)?;
	attempt.reserved_cost_ceiling_microusd = attempt
		.reserved_cost_ceiling_microusd
		.checked_add(call.recorded_cost_ceiling_microusd)
		.ok_or_else(|| eyre::eyre!("publication retry budget overflowed"))?;
	attempt.calls.push(call);
	attempt.status = "read_retry_inflight".into();
	attempt.updated_at = updated_at.into();
	replace(path, &previous, attempt)
}

pub(super) fn reserve_publication_reconcile_call(
	path: &Path,
	attempt: &mut XurlAttempt,
	attempts_dir: &Path,
	call: XurlCall,
	status: &str,
	updated_at: &str,
	reserve_additional: bool,
) -> Result<()> {
	if attempt.calls.len() >= 5 {
		return Err(eyre::eyre!("publication reconciliation paid-call sequence is exhausted"));
	}
	if reserve_additional {
		let billing_month = call
			.billing_month
			.as_deref()
			.ok_or_else(|| eyre::eyre!("publication reconciliation call lacks a billing month"))?;
		ensure_budget(attempts_dir, billing_month, call.recorded_cost_ceiling_microusd)?;
		ensure_lineage_budget(
			attempts_dir,
			&attempt.publication_lineage_sha256,
			call.recorded_cost_ceiling_microusd,
		)?;
	} else if call.billing_month.is_some() {
		return Err(eyre::eyre!(
			"publication reconciliation reused a reservation with a new billing charge"
		));
	} else {
		ensure_budget(attempts_dir, &attempt.billing_month, 0)?;
		ensure_lineage_budget(attempts_dir, &attempt.publication_lineage_sha256, 0)?;
	}
	let previous = serde_json::to_value(&*attempt)?;
	let prior = attempt
		.calls
		.last_mut()
		.ok_or_else(|| eyre::eyre!("xurl publication attempt has no prior call"))?;
	if prior.status == "inflight" {
		prior.status = "uncertain".into();
	}
	if reserve_additional {
		attempt.reserved_cost_ceiling_microusd = attempt
			.reserved_cost_ceiling_microusd
			.checked_add(call.recorded_cost_ceiling_microusd)
			.ok_or_else(|| eyre::eyre!("publication reconciliation budget overflowed"))?;
	}
	attempt.calls.push(call);
	attempt.status = status.into();
	attempt.updated_at = updated_at.into();
	replace(path, &previous, attempt)
}

pub(super) fn replace_observation(
	path: &Path,
	previous: &XurlObservationAttempt,
	next: &XurlObservationAttempt,
) -> Result<()> {
	crate::replace_existing_json(
		path,
		&serde_json::to_value(previous)?,
		&serde_json::to_value(next)?,
	)
}

pub(super) fn reserve_observation_reconcile_call(
	path: &Path,
	attempt: &mut XurlObservationAttempt,
	attempts_dir: &Path,
	call: XurlCall,
	updated_at: &str,
) -> Result<()> {
	if attempt.calls.len() >= 3 {
		return Err(eyre::eyre!("observation reconciliation paid-call sequence is exhausted"));
	}
	let billing_month = call
		.billing_month
		.as_deref()
		.ok_or_else(|| eyre::eyre!("observation reconciliation call lacks a billing month"))?;
	ensure_budget(attempts_dir, billing_month, call.recorded_cost_ceiling_microusd)?;
	ensure_lineage_budget(
		attempts_dir,
		&attempt.publication_lineage_sha256,
		call.recorded_cost_ceiling_microusd,
	)?;
	let previous = attempt.clone();
	let prior = attempt
		.calls
		.last_mut()
		.ok_or_else(|| eyre::eyre!("xurl observation attempt has no interrupted call"))?;
	if !matches!(prior.status.as_str(), "inflight" | "failed" | "invalid" | "uncertain") {
		return Err(eyre::eyre!("xurl observation attempt has no recoverable read"));
	}
	if prior.status == "inflight" {
		prior.status = "uncertain".into();
	}
	attempt.reserved_cost_ceiling_microusd = attempt
		.reserved_cost_ceiling_microusd
		.checked_add(call.recorded_cost_ceiling_microusd)
		.ok_or_else(|| eyre::eyre!("observation reconciliation budget overflowed"))?;
	attempt.call = call.clone();
	attempt.calls.push(call);
	attempt.status = "read_reconcile_inflight".into();
	attempt.updated_at = updated_at.into();
	replace_observation(path, &previous, attempt)
}

pub(super) fn finish_observation_call(
	path: &Path,
	attempt: &mut XurlObservationAttempt,
	call_status: &str,
	status: &str,
	updated_at: &str,
	response_sha256: Option<String>,
) -> Result<()> {
	let previous = attempt.clone();
	let call = attempt
		.calls
		.last_mut()
		.ok_or_else(|| eyre::eyre!("xurl observation attempt has no active call"))?;
	if call.status != "inflight" {
		return Err(eyre::eyre!("xurl observation attempt call is not inflight"));
	}
	call.status = call_status.into();
	call.response_sha256 = response_sha256;
	attempt.call = call.clone();
	attempt.status = status.into();
	attempt.updated_at = updated_at.into();
	replace_observation(path, &previous, attempt)
}

pub(super) fn reconcile_observation(
	path: &Path,
	attempt: &mut XurlObservationAttempt,
	updated_at: &str,
	response_sha256: &str,
	reconciliation: XurlReconciliation,
) -> Result<()> {
	let previous = attempt.clone();
	attempt.status = "observed".into();
	attempt.updated_at = updated_at.into();
	let call = attempt
		.calls
		.last_mut()
		.ok_or_else(|| eyre::eyre!("xurl observation attempt has no active call"))?;
	call.status = "succeeded".into();
	call.response_sha256 = Some(response_sha256.into());
	attempt.call = call.clone();
	attempt.reconciliation = Some(reconciliation);
	replace_observation(path, &previous, attempt)
}

pub(super) fn terminalize_observation(
	path: &Path,
	attempt: &mut XurlObservationAttempt,
	updated_at: &str,
	reconciliation: XurlReconciliation,
) -> Result<()> {
	let previous = attempt.clone();
	let call = attempt
		.calls
		.last_mut()
		.ok_or_else(|| eyre::eyre!("xurl observation attempt has no paid call"))?;
	if call.status == "inflight" {
		call.status = "uncertain".into();
	}
	attempt.call = call.clone();
	attempt.status = READ_RECOVERY_EXHAUSTED_STATUS.into();
	attempt.updated_at = updated_at.into();
	attempt.reconciliation = Some(reconciliation);
	validate_observation_cost_record(attempt)?;
	replace_observation(path, &previous, attempt)
}

fn replace(path: &Path, previous: &Value, attempt: &XurlAttempt) -> Result<()> {
	crate::replace_existing_json(path, previous, &serde_json::to_value(attempt)?)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn recovery_charges_are_attributed_to_the_call_month() {
		let temp = tempfile::tempdir().expect("tempdir");
		let attempts = temp.path().join("attempts");
		let publication = publication_attempt(
			"019fa400-0000-7000-8000-000000000001",
			vec![call("identity_read_reconcile", IDENTITY_READ_COST_MICROUSD, Some("2026-08"))],
		);
		crate::write_new_json(
			&attempts.join("2026-07/publication.json"),
			&serde_json::to_value(publication).expect("publication"),
		)
		.expect("publication usage");

		let initial = call("outcome_read", READ_COST_MICROUSD, None);
		let recovery = call("outcome_read_reconcile", READ_COST_MICROUSD, Some("2026-08"));
		let observation = XurlObservationAttempt {
			schema: OBSERVATION_ATTEMPT_SCHEMA.into(),
			run_id: "019fa400-0000-7000-8000-000000000002".into(),
			billing_month: "2026-07".into(),
			reserved_cost_ceiling_microusd: 10_000,
			status: "observed".into(),
			post_ref: "post.json".into(),
			post_id: "1".into(),
			publication_lineage_sha256:
				"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".into(),
			window: "24h".into(),
			created_at: "2026-07-28T00:00:00Z".into(),
			updated_at: "2026-08-01T00:00:00Z".into(),
			pricing_policy_id: Some(crate::social_xurl::model::PRICING_POLICY_ID.into()),
			authorization_contract_sha256: Some(
				"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
			),
			call: recovery.clone(),
			calls: vec![initial, recovery],
			reconciliation: None,
		};
		crate::write_new_json(
			&attempts.join("2026-07/observation.json"),
			&serde_json::to_value(observation).expect("observation"),
		)
		.expect("observation usage");

		assert_eq!(monthly_reserved_cost(&attempts, "2026-07").expect("July"), 35_000);
		assert_eq!(monthly_reserved_cost(&attempts, "2026-08").expect("August"), 15_000);
	}

	#[test]
	fn cost_report_separates_used_and_reserved_ceilings_without_payloads() {
		let temp = tempfile::tempdir().expect("tempdir");
		let attempts = temp.path().join("attempts");
		let publication = publication_attempt(
			"019fa400-0000-7000-8000-000000000001",
			vec![
				call("identity_read", IDENTITY_READ_COST_MICROUSD, None),
				call("content_create", CREATE_COST_MICROUSD, None),
			],
		);
		crate::write_new_json(
			&attempts.join("2026-07/publication.json"),
			&serde_json::to_value(publication).expect("publication"),
		)
		.expect("publication usage");
		let outcome_call = call("outcome_read", READ_COST_MICROUSD, None);
		let observation = XurlObservationAttempt {
			schema: OBSERVATION_ATTEMPT_SCHEMA.into(),
			run_id: "019fa400-0000-7000-8000-000000000002".into(),
			billing_month: "2026-07".into(),
			reserved_cost_ceiling_microusd: READ_COST_MICROUSD,
			status: "observed".into(),
			post_ref: "post.json".into(),
			post_id: "1".into(),
			publication_lineage_sha256:
				"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into(),
			window: "24h".into(),
			created_at: "2026-07-28T00:00:00Z".into(),
			updated_at: "2026-07-28T00:00:01Z".into(),
			pricing_policy_id: Some(crate::social_xurl::model::PRICING_POLICY_ID.into()),
			authorization_contract_sha256: Some(
				"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
			),
			call: outcome_call.clone(),
			calls: vec![outcome_call],
			reconciliation: None,
		};
		crate::write_new_json(
			&attempts.join("2026-07/observation.json"),
			&serde_json::to_value(observation).expect("observation"),
		)
		.expect("observation usage");

		let report = cost_report(&attempts, "2026-07").expect("bounded cost report");
		assert_eq!(report.used_cost_ceiling_microusd, 30_000);
		assert_eq!(report.reserved_cost_ceiling_microusd, 35_000);
		assert_eq!(report.remaining_cost_ceiling_microusd, 1_215_000);
		assert_eq!(report.publication_attempt_count, 1);
		assert_eq!(report.observation_attempt_count, 1);
		assert_eq!(report.identity_read_call_count, 1);
		assert_eq!(report.content_create_call_count, 1);
		assert_eq!(report.post_read_call_count, 1);
		assert_eq!(report.total_call_count, 3);
		let serialized = serde_json::to_string(&report).expect("cost report JSON");
		assert!(!serialized.contains("post.json"));
		assert!(!serialized.contains("response_sha256"));
	}

	#[test]
	fn cost_report_fails_closed_on_invalid_v4_usage() {
		let temp = tempfile::tempdir().expect("tempdir");
		let attempts = temp.path().join("attempts");
		let mut publication =
			publication_attempt("019fa400-0000-7000-8000-000000000001", Vec::new());
		publication.xurl_version = "1.3.2".into();
		crate::write_new_json(
			&attempts.join("2026-07/publication.json"),
			&serde_json::to_value(publication).expect("publication"),
		)
		.expect("invalid publication usage");

		let error = cost_report(&attempts, "2026-07")
			.expect_err("wrong runtime authority must stop cost reporting")
			.to_string();
		assert!(error.contains("usage authority is invalid"), "{error}");
	}

	#[test]
	fn monthly_cap_counts_every_base_reservation_before_an_added_read() {
		let temp = tempfile::tempdir().expect("tempdir");
		let attempts = temp.path().join("attempts");
		for index in 0..41 {
			let attempt =
				publication_attempt(&format!("019fa400-0000-7000-8000-{index:012}"), Vec::new());
			crate::write_new_json(
				&attempts.join("2026-07").join(format!("{index}.json")),
				&serde_json::to_value(attempt).expect("attempt"),
			)
			.expect("usage record");
		}

		let error = ensure_budget(&attempts, "2026-07", NORMAL_PUBLICATION_COST_MICROUSD)
			.expect_err("the next reservation must exceed the cap")
			.to_string();

		assert!(error.contains("monthly X budget exhausted"), "{error}");
	}

	#[test]
	fn reconciliation_reservation_rejects_a_stale_writer_race() {
		let temp = tempfile::tempdir().expect("tempdir");
		let attempts = temp.path().join("attempts");
		let path = attempts.join("2026-07/publication.json");
		let mut initial = call("identity_read", IDENTITY_READ_COST_MICROUSD, None);
		initial.status = "inflight".into();
		initial.response_sha256 = None;
		let mut attempt =
			publication_attempt("019fa400-0000-7000-8000-000000000001", vec![initial]);
		attempt.status = "identity_inflight".into();
		crate::write_new_json(&path, &serde_json::to_value(&attempt).expect("initial attempt"))
			.expect("attempt");
		let mut first = load_attempt(&path).expect("first reader");
		let mut stale = load_attempt(&path).expect("stale reader");
		let mut first_call =
			call("identity_read_reconcile", IDENTITY_READ_COST_MICROUSD, Some("2026-07"));
		first_call.status = "inflight".into();
		first_call.response_sha256 = None;
		reserve_publication_reconcile_call(
			&path,
			&mut first,
			&attempts,
			first_call,
			"identity_reconcile_inflight",
			"2026-07-01T00:01:00Z",
			true,
		)
		.expect("first reservation");
		let mut stale_call =
			call("identity_read_reconcile", IDENTITY_READ_COST_MICROUSD, Some("2026-07"));
		stale_call.operation_id = Some("019fa400-0000-7000-8000-000000000098".into());
		stale_call.status = "inflight".into();
		stale_call.response_sha256 = None;

		let _ = reserve_publication_reconcile_call(
			&path,
			&mut stale,
			&attempts,
			stale_call,
			"identity_reconcile_inflight",
			"2026-07-01T00:01:00Z",
			true,
		)
		.expect_err("stale writer must lose the compare-and-swap race");
		let durable = load_attempt(&path).expect("durable winner");
		assert_eq!(durable.calls.len(), 2);
		assert_eq!(
			durable.calls[1].operation_id.as_deref(),
			Some("019fa400-0000-7000-8000-000000000099")
		);
	}

	#[test]
	fn reused_read_reservation_rechecks_an_over_limit_ledger_before_mutation() {
		let temp = tempfile::tempdir().expect("tempdir");
		let attempts = temp.path().join("attempts");
		let path = attempts.join("2026-07/0.json");
		for index in 0..42 {
			let run_id = format!("019fa400-0000-7000-8000-{index:012}");
			let calls = if index == 0 {
				vec![
					call("identity_read", IDENTITY_READ_COST_MICROUSD, None),
					call("content_create", CREATE_COST_MICROUSD, None),
				]
			} else {
				Vec::new()
			};
			let attempt = publication_attempt(&run_id, calls);
			crate::write_new_json(
				&attempts.join("2026-07").join(format!("{index}.json")),
				&serde_json::to_value(attempt).expect("attempt"),
			)
			.expect("usage record");
		}
		let before = crate::load_json(&path).expect("durable attempt");
		let mut attempt = load_attempt(&path).expect("recovery attempt");
		let mut recovery_call = call("post_read_initial_reconcile", READ_COST_MICROUSD, None);
		recovery_call.status = "inflight".into();
		recovery_call.response_sha256 = None;

		let error = reserve_publication_reconcile_call(
			&path,
			&mut attempt,
			&attempts,
			recovery_call,
			"read_reconcile_inflight",
			"2026-07-01T00:01:00Z",
			false,
		)
		.expect_err("reused reservation must still validate the ledger")
		.to_string();

		assert!(error.contains("monthly X budget ledger exceeds its hard cap"), "{error}");
		assert_eq!(crate::load_json(&path).expect("unchanged attempt"), before);
	}

	fn publication_attempt(run_id: &str, calls: Vec<XurlCall>) -> XurlAttempt {
		let additional = calls
			.iter()
			.filter(|call| call.billing_month.is_some())
			.map(|call| call.recorded_cost_ceiling_microusd)
			.sum::<u64>();
		let publication_lineage_sha256 = crate::social_xurl::runtime::sha256(run_id.as_bytes());
		let status = match calls.last().map(|call| call.operation.as_str()) {
			Some("identity_read_reconcile") => "identity_reconciled",
			Some("content_create") => "created",
			_ => "reserved",
		};
		XurlAttempt {
			schema: ATTEMPT_SCHEMA.into(),
			run_id: run_id.into(),
			reservation_ref: "reservation.json".into(),
			candidate_ref: "candidate.json".into(),
			candidate_sha256: None,
			idempotency_key: format!("content-publication:{publication_lineage_sha256}"),
			publication_lineage_sha256,
			billing_month: "2026-07".into(),
			target_account: "decodexspace".into(),
			status: status.into(),
			created_at: "2026-07-01T00:00:00Z".into(),
			updated_at: "2026-07-01T00:00:00Z".into(),
			reserved_cost_ceiling_microusd: NORMAL_PUBLICATION_COST_MICROUSD + additional,
			xurl_version: "1.3.1".into(),
			pricing_policy_id: Some(crate::social_xurl::model::PRICING_POLICY_ID.into()),
			authorization_contract_sha256: Some(
				"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
			),
			calls,
			verified_user_id: None,
			post_id: None,
			published_url: None,
			reconciliation: None,
		}
	}

	fn call(operation: &str, cost: u64, billing_month: Option<&str>) -> XurlCall {
		XurlCall {
			operation: operation.into(),
			operation_id: operation
				.ends_with("_reconcile")
				.then(|| "019fa400-0000-7000-8000-000000000099".into()),
			billing_month: billing_month.map(str::to_owned),
			status: "succeeded".into(),
			recorded_cost_ceiling_microusd: cost,
			response_sha256: Some(
				"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
			),
		}
	}
}
