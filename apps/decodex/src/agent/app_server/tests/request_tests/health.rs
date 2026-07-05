use serde_json::{self};

use crate::agent::app_server::{self, CommandExecHealthCheck};

#[test]
fn command_exec_health_check_uses_bounded_standalone_request() {
	let health_check = CommandExecHealthCheck {
		command: vec![String::from("/bin/sh"), String::from("-c"), String::from("printf ok")],
		expected_stdout: String::from("ok"),
		timeout_ms: 1_000,
		output_bytes_cap: 128,
	};
	let params = app_server::build_command_exec_health_check_params(&health_check, "/tmp/worktree");
	let value = serde_json::to_value(&params).expect("command exec params should serialize");

	assert_eq!(value["command"], serde_json::json!(["/bin/sh", "-c", "printf ok"]));
	assert_eq!(value["cwd"], "/tmp/worktree");
	assert_eq!(value["timeoutMs"], 1_000);
	assert_eq!(value["outputBytesCap"], 128);
	assert!(value.get("threadId").is_none());
	assert!(value.get("sandboxPolicy").is_none());
	assert!(value.get("permissionProfile").is_none());
}
