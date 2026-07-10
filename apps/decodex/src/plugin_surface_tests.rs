mod decodex_skills;
mod manifests;

use serde_json::Value;

const DECODEX_PLUGIN_JSON: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/.codex-plugin/plugin.json"
));
const DECODEX_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/decodex/SKILL.md"
));
const DECODEX_OPS_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/decodex-ops/SKILL.md"
));
const PLANNING_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/planning/SKILL.md"
));
const LAND_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/land/SKILL.md"
));
const COMMIT_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/commit/agents/openai.yaml"
));
const DECODEX_OPS_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/decodex-ops/agents/openai.yaml"
));
const LAND_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/land/agents/openai.yaml"
));
const PLANNING_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/planning/agents/openai.yaml"
));
const ROUTING_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/routing.md"
));

fn manifest_interface_surface(manifest_json: &str) -> String {
	let manifest: Value =
		serde_json::from_str(manifest_json).expect("plugin manifest should parse");
	let interface = manifest
		.get("interface")
		.and_then(Value::as_object)
		.expect("plugin interface should be an object");
	let long_description = interface
		.get("longDescription")
		.and_then(Value::as_str)
		.expect("longDescription should be a string");
	let default_prompts = interface
		.get("defaultPrompt")
		.and_then(Value::as_array)
		.expect("defaultPrompt should be an array")
		.iter()
		.filter_map(Value::as_str)
		.collect::<Vec<_>>()
		.join("\n");

	format!("{long_description}\n{default_prompts}")
}

fn assert_contains(haystack: &str, needle: &str) {
	assert!(haystack.contains(needle), "expected packaged plugin content to contain `{needle}`");
}
