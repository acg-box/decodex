mod evidence;
mod lineage;
mod objective;
mod proposal;
mod report;
mod signal;

use std::collections::BTreeSet;

use crate::{
	config::ServiceConfig,
	orchestrator::{
		OperatorAutonomyLineageStatus, OperatorAutonomyObjectiveStatus,
		OperatorAutonomyProposalStatus, OperatorAutonomyReportReadbackStatus,
		OperatorAutonomySignalStatus,
	},
	state::ProjectLoopEvidenceSnapshot,
	tracker::public_text,
};

pub(in crate::orchestrator) fn operator_autonomy_objective_status(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Option<OperatorAutonomyObjectiveStatus> {
	objective::operator_autonomy_objective_status(project, loop_evidence)
}

pub(in crate::orchestrator) fn operator_autonomy_signal_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomySignalStatus> {
	signal::operator_autonomy_signal_statuses(loop_evidence)
}

pub(in crate::orchestrator) fn operator_autonomy_proposal_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomyProposalStatus> {
	proposal::operator_autonomy_proposal_statuses(loop_evidence)
}

pub(in crate::orchestrator) fn operator_autonomy_lineage_statuses(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
) -> Vec<OperatorAutonomyLineageStatus> {
	lineage::operator_autonomy_lineage_statuses(loop_evidence)
}

pub(in crate::orchestrator) fn operator_autonomy_report_status(
	objective: Option<&OperatorAutonomyObjectiveStatus>,
	signals: &[OperatorAutonomySignalStatus],
	proposals: &[OperatorAutonomyProposalStatus],
	lineage: &[OperatorAutonomyLineageStatus],
) -> Option<OperatorAutonomyReportReadbackStatus> {
	report::operator_autonomy_report_status(objective, signals, proposals, lineage)
}

fn operator_autonomy_objective_ref(objective_id: &str, objective_version: u64) -> String {
	format!("{objective_id}@v{objective_version}")
}

fn operator_autonomy_completeness(known_gaps: &[String]) -> String {
	if known_gaps.is_empty() { String::from("complete") } else { String::from("partial") }
}

fn public_autonomy_refs(refs: &[String]) -> (Vec<String>, bool) {
	let mut redacted = false;
	let refs = refs
		.iter()
		.filter_map(|value| {
			let Some(value) = public_autonomy_ref(value) else {
				redacted = true;

				return None;
			};

			Some(value)
		})
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect::<Vec<_>>();

	(refs, redacted)
}

fn public_autonomy_ref(value: &str) -> Option<String> {
	let value = value.trim();

	if value.is_empty()
		|| public_text::validate_public_text_field("autonomy source_ref", value).is_err()
	{
		return None;
	}

	Some(value.to_owned())
}

fn public_status_values(values: &[String]) -> (Vec<String>, bool) {
	let mut redacted = false;
	let values = values
		.iter()
		.map(|value| {
			let (value, value_redacted) = public_status_value(value);

			redacted |= value_redacted;

			value
		})
		.collect();

	(values, redacted)
}

fn public_or_redacted_status_value(value: &str) -> String {
	public_status_value(value).0
}

fn public_status_value(value: &str) -> (String, bool) {
	let value = value.trim();

	if value.is_empty() {
		return (String::from("none"), false);
	}
	if public_text::validate_public_text_field("autonomy status value", value).is_err() {
		return (String::from("redacted_sensitive_detail"), true);
	}

	(value.to_owned(), false)
}
