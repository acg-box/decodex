use serde_json::{self, Value};

pub(super) fn mcp_autonomy_authority_boundary() -> Value {
	serde_json::json!({
		"mcp_authentication": "access_boundary_only",
		"capability_profile": "tool_visibility_boundary_only",
		"acceptance_authority": "explicit_human_or_trusted_accepted_project_policy_required",
		"execution_authority": "Decision Contract promotion and Program Intake remain separate"
	})
}
