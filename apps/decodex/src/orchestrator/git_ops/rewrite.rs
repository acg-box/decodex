mod model;
mod outcome;
mod snapshot;

pub(crate) use self::{
	model::{RepoGateCommandOutcome, RepoGateTrackedRewriteDecision},
	outcome::{repo_gate_diff_rewrite_outcome, repo_gate_scope_envelope_failure_or_source},
	snapshot::read_repo_gate_tracked_diff_snapshot,
};
