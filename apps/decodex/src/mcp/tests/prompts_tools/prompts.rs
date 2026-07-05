use serde_json::Value;

use crate::mcp::tests::support::{self};

#[test]
fn prompts_list_and_get_return_prompt_messages() {
	let repo = support::test_repo();
	let list_responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"prompts/list","params":{}}"#,
	);
	let prompts = support::response_at(&list_responses, 0)["result"]["prompts"]
		.as_array()
		.expect("prompts array");
	let prompt_names = prompts
		.iter()
		.filter_map(|prompt| prompt.get("name").and_then(Value::as_str))
		.collect::<Vec<_>>();

	assert!(prompt_names.contains(&"decodex_validation_ready"));

	let get_responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":2,"method":"prompts/get","params":{"name":"decodex_validation_ready","arguments":{"issue":"XY-994"}}}"#,
	);
	let messages = support::response_at(&get_responses, 0)["result"]["messages"]
		.as_array()
		.expect("messages array");
	let text = messages[0]["content"]["text"].as_str().expect("prompt text");

	assert!(text.contains("XY-994"));
	assert!(text.contains("validation-ready"));
}

#[test]
fn prompts_get_rejects_missing_required_arguments() {
	let repo = support::test_repo();
	let responses = support::run_stdio(
		repo.path(),
		r#"{"jsonrpc":"2.0","id":1,"method":"prompts/get","params":{"name":"decodex_validation_ready","arguments":{}}}"#,
	);
	let error = support::response_at(&responses, 0).get("error").expect("error response");

	assert_eq!(error["code"], -32_602);
}
