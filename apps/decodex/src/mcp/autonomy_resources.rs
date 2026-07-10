mod resources;
mod summaries;

pub(super) use self::{
	resources::{
		mcp_autonomy_current_objective_resource, mcp_autonomy_evidence_resource,
		mcp_autonomy_objective_version_resource, mcp_autonomy_project_resource,
		mcp_autonomy_proposal_by_affected_identifier_resource, mcp_autonomy_proposal_resource,
		mcp_autonomy_proposals_resource, mcp_autonomy_signal_resource,
		mcp_autonomy_signals_resource,
	},
	summaries::{
		mcp_autonomy_objective_summary, mcp_autonomy_proposal_summary, mcp_autonomy_signal_summary,
	},
};
