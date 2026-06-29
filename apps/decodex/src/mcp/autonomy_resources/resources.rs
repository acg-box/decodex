mod authority;
mod evidence;
mod objective;
mod project;
mod proposal;
mod signal;

pub(in crate::mcp) use self::{
	evidence::mcp_autonomy_evidence_resource,
	objective::{mcp_autonomy_current_objective_resource, mcp_autonomy_objective_version_resource},
	project::mcp_autonomy_project_resource,
	proposal::{mcp_autonomy_proposal_resource, mcp_autonomy_proposals_resource},
	signal::{mcp_autonomy_signal_resource, mcp_autonomy_signals_resource},
};
