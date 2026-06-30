use serde_json::Value;

use super::support::{response_at, run_stdio, run_stdio_raw, test_repo};

#[test]
fn resources_read_rejects_invalid_resource_uri() {
	let repo = test_repo();
	let responses = run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"file:///etc/passwd"}}"#,
	);
	let error = response_at(&responses, 0).get("error").expect("error response");

	assert_eq!(error["code"], -32_602);
}

#[test]
fn stdio_output_contains_only_json_rpc_responses() {
	let repo = test_repo();
	let input = [
			r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
			r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
			r#"{"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}"#,
			r#"{"jsonrpc":"2.0","id":3,"method":"resources/templates/list","params":{}}"#,
			r#"{"jsonrpc":"2.0","id":4,"method":"prompts/list","params":{}}"#,
			r#"{"jsonrpc":"2.0","id":5,"method":"prompts/get","params":{"name":"decodex_validation_ready","arguments":{"issue":"XY-994"}}}"#,
			r#"{"jsonrpc":"2.0","id":6,"method":"tools/list","params":{}}"#,
			r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"decodex_plan","arguments":{"intent":"validation_ready"}}}"#,
		]
		.join("\n");
	let output = run_stdio_raw(repo.path(), &input);
	let lines = output.lines().collect::<Vec<_>>();

	assert_eq!(lines.len(), 7);

	for line in lines {
		let value = serde_json::from_str::<Value>(line).expect("stdout line should be JSON");

		assert_eq!(value["jsonrpc"], "2.0");
	}
}
