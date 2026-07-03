use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use crate::{
	orchestrator::{self, OperatorRunStatus, RUN_OPERATION_IDLE},
	prelude::Result,
};

pub(crate) const OPERATOR_PRESENTATION_SCHEMA: &str = "decodex.operator.presentation/1";

#[derive(Serialize)]
pub(crate) struct OperatorSnapshotPresentation<'a> {
	schema: &'static str,
	#[serde(rename = "current_lane_cards")]
	current_lane_cards: Vec<OperatorCurrentLaneCard<'a>>,
}

#[derive(Serialize)]
pub(crate) struct OperatorCurrentLaneCard<'a> {
	id: &'a str,
	#[serde(rename = "run_id")]
	run_id: &'a str,
	#[serde(rename = "project_id")]
	project_id: &'a str,
	#[serde(rename = "issue_id")]
	issue_id: &'a str,
	#[serde(rename = "issue_identifier")]
	issue_identifier: &'a Option<String>,
	title: String,
	detail: String,
	tone: &'static str,
	#[serde(rename = "counts_as_running")]
	counts_as_running: bool,
	#[serde(rename = "needs_attention")]
	needs_attention: bool,
	#[serde(rename = "is_waiting")]
	is_waiting: bool,
	#[serde(rename = "assigned_account_fingerprints")]
	assigned_account_fingerprints: Vec<String>,
	#[serde(rename = "assigned_account_emails")]
	assigned_account_emails: Vec<String>,
	run: &'a OperatorRunStatus,
}

pub(crate) fn operator_snapshot_presentation_value(
	current_lanes: &[OperatorRunStatus],
) -> Result<Value> {
	Ok(serde_json::to_value(operator_snapshot_presentation(current_lanes))?)
}

pub(crate) fn operator_snapshot_presentation(
	current_lanes: &[OperatorRunStatus],
) -> OperatorSnapshotPresentation<'_> {
	OperatorSnapshotPresentation {
		schema: OPERATOR_PRESENTATION_SCHEMA,
		current_lane_cards: current_lanes.iter().map(operator_current_lane_card).collect(),
	}
}

pub(crate) fn operator_current_lane_card(run: &OperatorRunStatus) -> OperatorCurrentLaneCard<'_> {
	OperatorCurrentLaneCard {
		id: run.run_id.as_str(),
		run_id: run.run_id.as_str(),
		project_id: run.project_id.as_str(),
		issue_id: run.issue_id.as_str(),
		issue_identifier: &run.issue_identifier,
		title: operator_current_lane_card_title(run),
		detail: operator_current_lane_card_detail(run),
		tone: operator_current_lane_card_tone(run),
		counts_as_running: orchestrator::operator_run_counts_as_running(run),
		needs_attention: orchestrator::operator_run_counts_as_attention(run),
		is_waiting: orchestrator::operator_run_counts_as_waiting(run),
		assigned_account_fingerprints: operator_run_assigned_account_fingerprints(run),
		assigned_account_emails: operator_run_assigned_account_emails(run),
		run,
	}
}

pub(crate) fn operator_current_lane_card_title(run: &OperatorRunStatus) -> String {
	trimmed_operator_presentation_text(run.issue_identifier.as_deref())
		.or_else(|| trimmed_operator_presentation_text(run.title.as_deref()))
		.unwrap_or("Run")
		.to_owned()
}

pub(crate) fn operator_current_lane_card_detail(run: &OperatorRunStatus) -> String {
	trimmed_operator_presentation_text(
		run.child_agent_activity.as_ref().and_then(|activity| activity.current_detail.as_deref()),
	)
	.or_else(|| {
		trimmed_operator_presentation_text(
			run.child_agent_activity
				.as_ref()
				.and_then(|activity| activity.current_bucket.as_deref()),
		)
	})
	.or_else(|| trimmed_operator_presentation_text(run.wait_reason.as_deref()))
	.or_else(|| {
		trimmed_operator_presentation_text(Some(run.current_operation.as_str()))
			.filter(|operation| *operation != RUN_OPERATION_IDLE)
	})
	.or_else(|| trimmed_operator_presentation_text(Some(run.run_phase.as_str())))
	.or_else(|| trimmed_operator_presentation_text(Some(run.phase.as_str())))
	.or_else(|| trimmed_operator_presentation_text(run.thread_status.as_deref()))
	.unwrap_or("Active")
	.to_owned()
}

pub(crate) fn operator_current_lane_card_tone(run: &OperatorRunStatus) -> &'static str {
	if orchestrator::operator_run_counts_as_attention(run) {
		"attention"
	} else if orchestrator::operator_run_counts_as_waiting(run) {
		"waiting"
	} else {
		"running"
	}
}

pub(crate) fn operator_run_assigned_account_fingerprints(run: &OperatorRunStatus) -> Vec<String> {
	let mut fingerprints = BTreeSet::new();

	if let Some(account) = run.account.as_ref() {
		insert_non_empty_operator_presentation_text(
			&mut fingerprints,
			&account.account_fingerprint,
		);
	}

	for account in
		run.accounts.iter().filter(|account| account.status.eq_ignore_ascii_case("selected"))
	{
		insert_non_empty_operator_presentation_text(
			&mut fingerprints,
			&account.account_fingerprint,
		);
	}

	fingerprints.into_iter().collect()
}

pub(crate) fn operator_run_assigned_account_emails(run: &OperatorRunStatus) -> Vec<String> {
	let mut emails = BTreeSet::new();

	if let Some(account) = run.account.as_ref()
		&& let Some(email) = account.email.as_deref()
	{
		insert_non_empty_operator_presentation_text(&mut emails, email);
	}

	for account in
		run.accounts.iter().filter(|account| account.status.eq_ignore_ascii_case("selected"))
	{
		if let Some(email) = account.email.as_deref() {
			insert_non_empty_operator_presentation_text(&mut emails, email);
		}
	}

	emails.into_iter().collect()
}

pub(crate) fn insert_non_empty_operator_presentation_text(
	values: &mut BTreeSet<String>,
	value: &str,
) {
	if let Some(value) = trimmed_operator_presentation_text(Some(value)) {
		values.insert(value.to_owned());
	}
}

pub(crate) fn trimmed_operator_presentation_text(value: Option<&str>) -> Option<&str> {
	value.map(str::trim).filter(|value| !value.is_empty())
}
