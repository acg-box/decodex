mod common;
mod identity;
mod input;
mod paths;
mod refusals;

use crate::{
	autonomy_objective::AutonomyObjectiveContract,
	autonomy_proposal::{
		AutonomyProposal, AutonomyProposalCompileInput, AutonomyProposalRefusal,
		AutonomyProposalState,
	},
	autonomy_signal::AutonomySignal,
	prelude::Result,
};

pub(super) fn proposal_refusals(
	objective: Option<&AutonomyObjectiveContract>,
	signals: &[AutonomySignal],
	input: &AutonomyProposalCompileInput,
	contradictions: &[String],
) -> Vec<AutonomyProposalRefusal> {
	refusals::proposal_refusals(objective, signals, input, contradictions)
}

pub(super) fn derive_proposal_state(
	has_signals: bool,
	refusals: &[AutonomyProposalRefusal],
) -> AutonomyProposalState {
	refusals::derive_proposal_state(has_signals, refusals)
}

pub(super) fn normalize_repo_relative_path(value: &str) -> Option<String> {
	paths::normalize_repo_relative_path(value)
}

pub(super) fn autonomy_proposal_schema() -> String {
	identity::autonomy_proposal_schema()
}

pub(super) const fn autonomy_proposal_record_version() -> u16 {
	identity::autonomy_proposal_record_version()
}

pub(super) fn autonomy_proposal_id(fingerprint: &str) -> String {
	identity::autonomy_proposal_id(fingerprint)
}

pub(super) fn autonomy_proposal_fingerprint(proposal: &AutonomyProposal) -> Result<String> {
	identity::autonomy_proposal_fingerprint(proposal)
}

pub(super) fn validate_compile_input(input: &AutonomyProposalCompileInput) -> Result<()> {
	input::validate_compile_input(input)
}

pub(super) fn validate_proposed_issue_stage(key: &str, stage: &str) -> Result<()> {
	input::validate_proposed_issue_stage(key, stage)
}

pub(super) fn validate_proposed_issue_queue_intent(key: &str, queue_intent: &str) -> Result<()> {
	input::validate_proposed_issue_queue_intent(key, queue_intent)
}

pub(super) fn validate_required(name: &str, value: &str) -> Result<()> {
	common::validate_required(name, value)
}

pub(super) fn validate_optional_required(name: &str, value: Option<&str>) -> Result<()> {
	common::validate_optional_required(name, value)
}

pub(super) fn validate_string_list(name: &str, values: &[String]) -> Result<()> {
	common::validate_string_list(name, values)
}

pub(super) fn validate_sorted_unique(name: &str, values: &[String]) -> Result<()> {
	common::validate_sorted_unique(name, values)
}

pub(super) fn unique_sorted_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
	common::unique_sorted_strings(values)
}
