pub(in crate::agent::tracker_tool_bridge::tools::manual_attention::normalize) fn is_runtime_owned_manual_attention_error_class(
	error_class: &str,
) -> bool {
	matches!(
		error_class,
		"retryable_execution_failure"
			| "repo_gate_canonicalize_failed"
			| "repo_gate_verify_failed"
			| "repo_gate_baseline_failed"
			| "repo_gate_preexisting_baseline_failed"
			| "repo_gate_global_baseline_failed"
			| "repo_gate_tracked_rewrites_left"
			| "repo_gate_git_lock_contention"
			| "stalled_run_detected"
			| "app_server_zero_evidence_start_failed"
			| "app_server_plugin_list_timeout"
			| "app_server_preflight_timeout"
			| "app_server_transport_disconnected"
			| "phase_goal_terminal_path_missing"
			| "app_server_dynamic_tool_protocol_failure"
			| "app_server_dynamic_tool_failed"
			| "app_server_turn_failed"
			| "app_server_turn_missing_error_payload"
			| "app_server_usage_limit_exceeded"
	) || runtime_owned_baseline_error_class(error_class)
}

fn runtime_owned_baseline_error_class(error_class: &str) -> bool {
	[
		"baseline",
		"preexisting",
		"pre_existing",
		"repo_wide",
		"repository_wide",
		"global_baseline",
		"docs_gate",
	]
	.iter()
	.any(|pattern| error_class.contains(pattern))
}
