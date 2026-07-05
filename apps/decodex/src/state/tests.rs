mod autonomy_fixtures;
mod decision_fixtures;
mod fd_helpers;
mod leases;
mod persistent_events;
mod review_fixtures;
mod review_lifecycle;
mod run_activity;
mod run_control;
mod runtime_records;

pub(crate) use self::{
	autonomy_fixtures::{
		autonomy_objective_fixture, autonomy_proposal_fixture, sample_objective_acceptance,
	},
	decision_fixtures::{
		assert_decision_contract_retargeted, latent_decision_contract_fixture,
		sample_decision_promotion, sample_execution_program,
	},
	review_fixtures::{
		sample_pub_101_review_handoff, sample_pub_101_review_orchestration,
		seed_dropped_review_marker_tables, upsert_handoff_review_policy_checkpoint,
	},
};
#[cfg(unix)] pub(crate) use fd_helpers::fd_has_close_on_exec;

pub(crate) const IN_PROGRESS_STATE: &str = "In Progress";
