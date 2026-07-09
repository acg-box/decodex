use serde_json::{self, Value};

use crate::mcp::{
	McpError, McpServer, ReadResourceParams,
	resources::{templates, types::McpResource},
};

impl McpServer {
	pub(in crate::mcp) fn list_resources(&self) -> Result<Value, McpError> {
		let mut resources = self.context.openwiki_resources()?;

		resources.extend(self.context.decision_contract_resources()?);

		if let Some(project_id) = self.context.project_id() {
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/status"),
				format!("Project {project_id} status"),
				"Read-only local runtime status snapshot.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/status_live"),
				format!("Project {project_id} live status"),
				"Remote-safe status, activity, progress, and lane-control summary.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/activity_tail"),
				format!("Project {project_id} activity tail"),
				"Remote-safe current/recent run activity summary.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/lane-control"),
				format!("Project {project_id} lane-control readback"),
				"Read-only lane-control state for current and recent local lanes.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/pr_review_state"),
				format!("Project {project_id} PR/review state"),
				"Remote-safe PR and review-state readback.",
			));
			resources.push(McpResource::json(
				format!("decodex://projects/{project_id}/autonomy"),
				format!("Project {project_id} autonomy summaries"),
				"Read-only autonomy objective, signal, proposal, and evidence summaries.",
			));
		}

		Ok(serde_json::json!({ "resources": resources }))
	}

	pub(in crate::mcp) fn list_resource_templates(&self) -> Value {
		let mut resource_templates = templates::openwiki_resource_templates();

		resource_templates.extend(templates::runtime_resource_templates());

		serde_json::json!({
			"resourceTemplates": resource_templates
		})
	}

	pub(in crate::mcp) fn read_resource(&self, params: Option<Value>) -> Result<Value, McpError> {
		let params = params.ok_or_else(McpError::invalid_params)?;
		let params = serde_json::from_value::<ReadResourceParams>(params)
			.map_err(|_| McpError::invalid_params())?;
		let content = self.context.read_resource(&params.uri)?;

		Ok(serde_json::json!({
			"contents": [
				{
					"uri": content.uri,
					"mimeType": content.mime_type,
					"text": content.text
				}
			]
		}))
	}
}
