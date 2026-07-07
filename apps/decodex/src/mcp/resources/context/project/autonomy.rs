use serde_json::Value;

use crate::{
	mcp::{self, McpContext, McpError, autonomy_resources},
	prelude::Result,
};

impl McpContext {
	pub(super) fn read_autonomy_project_resource(
		&self,
		project_id: &str,
		rest: &[String],
	) -> Result<Value, McpError> {
		let Some(state_store) = self.state_store.as_ref() else {
			return Err(McpError::resource_not_found());
		};

		match rest {
			[] => autonomy_resources::mcp_autonomy_project_resource(state_store, project_id),
			[resource] if resource == "signals" => {
				autonomy_resources::mcp_autonomy_signals_resource(state_store, project_id)
			},
			[resource, signal_id]
				if resource == "signals" && mcp::safe_autonomy_record_identifier(signal_id) =>
			{
				autonomy_resources::mcp_autonomy_signal_resource(state_store, project_id, signal_id)
			},
			[resource] if resource == "proposals" => {
				autonomy_resources::mcp_autonomy_proposals_resource(state_store, project_id)
			},
			[resource, proposal_id]
				if resource == "proposals" && mcp::safe_autonomy_record_identifier(proposal_id) =>
			{
				autonomy_resources::mcp_autonomy_proposal_resource(
					state_store,
					project_id,
					proposal_id,
				)
			},
			[resource] if resource == "evidence" => {
				autonomy_resources::mcp_autonomy_evidence_resource(state_store, project_id)
			},
			[resource, objective_id, selector]
				if resource == "objectives"
					&& mcp::safe_runtime_identifier(objective_id)
					&& selector == "current" =>
			{
				autonomy_resources::mcp_autonomy_current_objective_resource(
					state_store,
					project_id,
					objective_id,
				)
			},
			[resource, objective_id, version]
				if resource == "objectives" && mcp::safe_runtime_identifier(objective_id) =>
			{
				autonomy_resources::mcp_autonomy_objective_version_resource(
					state_store,
					project_id,
					objective_id,
					version,
				)
			},
			_ => Err(McpError::resource_not_found()),
		}
	}
}
