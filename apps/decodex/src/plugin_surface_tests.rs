use serde_json::Value;

const PLUGIN_JSON: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/.codex-plugin/plugin.json"
));
const DECODEX_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/decodex/SKILL.md"
));
const PLANNING_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/planning/SKILL.md"
));
const AUTOMATION_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/automation/SKILL.md"
));
const LABELS_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/labels/SKILL.md"
));

#[test]
fn packaged_plugin_manifest_routes_natural_language_research_to_decodex() {
	let manifest: Value = serde_json::from_str(PLUGIN_JSON).expect("plugin manifest should parse");
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

	assert_contains(long_description, "natural-language-first");
	assert_contains(long_description, "\"research X\"");
	assert_contains(long_description, "latent Decision Contracts");
	assert_contains(long_description, "\"arrange this\"");
	assert_contains(long_description, "\"推进\"");
	assert_contains(long_description, "queues only ready nodes");
	assert_contains(long_description, "graph, DAG, goal, and queue mechanics backstage");
	assert_contains(&default_prompts, "Research how Decodex should handle this.");
	assert_contains(
		&default_prompts,
		"Arrange the accepted Decodex research contract into executable issues.",
	);
}

#[test]
fn packaged_skills_preserve_research_promotion_and_queue_boundaries() {
	assert_contains(DECODEX_SKILL, "## Natural-Language Research Routing");
	assert_contains(DECODEX_SKILL, "`research X`");
	assert_contains(DECODEX_SKILL, "latent Decision Contract");
	assert_contains(DECODEX_SKILL, "`arrange this`");
	assert_contains(DECODEX_SKILL, "`推进`");
	assert_contains(DECODEX_SKILL, "Do not queue work");
	assert_contains(DECODEX_SKILL, "dispatches ready mapped nodes directly");
	assert_contains(PLANNING_SKILL, "accepted Decision Contract");
	assert_contains(PLANNING_SKILL, "Do not use planning to turn a plain `research X`");
	assert_contains_normalized(PLANNING_SKILL, "scheduler directly dispatch nodes");
	assert_contains(PLANNING_SKILL, "Promotion is a separate authority boundary");
	assert_contains(AUTOMATION_SKILL, "Automation starts only after execution authority exists");
	assert_contains_normalized(
		AUTOMATION_SKILL,
		"latent research must not dispatch retained lanes",
	);
	assert_contains(AUTOMATION_SKILL, "accepted/promoted Decision Contract");
	assert_contains_normalized(AUTOMATION_SKILL, "directly dispatch ready mapped nodes");
	assert_contains(AUTOMATION_SKILL, "Blocked, stale, paused, active, terminal");
	assert_contains_normalized(LABELS_SKILL, "not the user-facing research/design workflow");
	assert_contains(LABELS_SKILL, "accepted/promoted Decision");
	assert_contains(LABELS_SKILL, "Do not ask ordinary users to apply queue labels");
}

fn assert_contains(haystack: &str, needle: &str) {
	assert!(haystack.contains(needle), "expected packaged plugin content to contain `{needle}`");
}

fn assert_contains_normalized(haystack: &str, needle: &str) {
	let normalized_haystack = haystack.split_whitespace().collect::<Vec<_>>().join(" ");
	let normalized_needle = needle.split_whitespace().collect::<Vec<_>>().join(" ");

	assert_contains(&normalized_haystack, &normalized_needle);
}
