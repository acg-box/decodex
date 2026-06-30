use serde_json::Value;

use crate::mcp::observability::sanitizer::sensitive_text;

pub(in crate::mcp) fn sanitize_mcp_observability_value(value: &mut Value) {
	match value {
		Value::Object(object) => {
			for key in [
				"worktreePath",
				"worktree_path",
				"channelPath",
				"channel_path",
				"requestPath",
				"request_path",
				"configPath",
				"config_path",
				"repoRoot",
				"repo_root",
				"effectiveCwd",
				"effective_cwd",
				"cwd",
				"privateEvidence",
				"private_evidence",
				"privateEvidenceRef",
				"private_evidence_ref",
				"privateEvidenceRefs",
				"private_evidence_refs",
				"executionProgramId",
				"execution_program_id",
				"executionProgramNodeIds",
				"execution_program_node_ids",
				"graphId",
				"graph_id",
				"nodeId",
				"node_id",
				"programId",
				"program_id",
				"readCommand",
				"read_command",
				"githubCliAuthority",
				"github_cli_authority",
				"githubCommandPath",
				"github_command_path",
				"ghCommandPath",
				"gh_command_path",
				"githubTokenEnvVar",
				"github_token_env_var",
				"path",
			] {
				object.remove(key);
			}
			for child in object.values_mut() {
				sanitize_mcp_observability_value(child);
			}
		},
		Value::String(text) => {
			if sensitive_text::observability_string_contains_sensitive_text(text) {
				*text = String::from("redacted_sensitive_detail");
			}
		},
		Value::Array(items) => {
			for item in items {
				sanitize_mcp_observability_value(item);
			}
		},
		_ => {},
	}
}

pub(in crate::mcp) fn mcp_sanitized_value(mut value: Value) -> Value {
	sanitize_mcp_observability_value(&mut value);

	value
}
