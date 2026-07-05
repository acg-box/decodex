mod kind;
mod model;
mod output;
mod problem;

pub(crate) use self::{
	kind::repo_gate_failure_kind_for_output,
	model::RepoGateFailureDiagnostic,
	output::{repo_gate_git_output_lines, repo_gate_output_text},
};
