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
const AUTOMATION_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/automation/agents/openai.yaml"
));
const COMMIT_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/commit/agents/openai.yaml"
));
const LABELS_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/labels/SKILL.md"
));
const LABELS_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/labels/agents/openai.yaml"
));
const LAND_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/land/agents/openai.yaml"
));
const MANUAL_CLI_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/manual-cli/agents/openai.yaml"
));
const RESEARCH_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research/SKILL.md"
));
const RESEARCH_PROBE_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-probe/SKILL.md"
));
const RESEARCH_EVIDENCE_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-evidence/SKILL.md"
));
const RESEARCH_OPTIONS_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-options/SKILL.md"
));
const RESEARCH_JUDGMENT_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-judgment/SKILL.md"
));
const RESEARCH_CHALLENGE_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-challenge/SKILL.md"
));
const RESEARCH_DECISION_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-decision/SKILL.md"
));
const RESEARCH_PROMOTE_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-promote/SKILL.md"
));
const ROUTING_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/routing.md"
));
const ISSUE_BRIEFING_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/issue-briefing.md"
));
const RESEARCH_METHOD_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/research-method.md"
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
	let manifest_surface =
		format!("{long_description}\n{default_prompts}\n{ROUTING_REF}\n{RESEARCH_METHOD_REF}");

	assert_contains(&manifest_surface, "natural-language-first");
	assert_contains(&manifest_surface, "bounded research");
	assert_contains(&manifest_surface, "probe, evidence, options, judgment, challenge, decision");
	assert_contains(&manifest_surface, "`research X`");
	assert_contains(&manifest_surface, "latent Decision Contract");
	assert_contains(&manifest_surface, "\"arrange this\"");
	assert_contains(&manifest_surface, "\"推进\"");
	assert_contains(&manifest_surface, "ready mapped nodes directly");
	assert_contains(&manifest_surface, "DAG");
	assert_contains(&manifest_surface, "issue briefing");
	assert_contains(&default_prompts, "Research this with Decodex.");
	assert_contains(&default_prompts, "Arrange accepted Decodex work.");
	assert_contains(&default_prompts, "Inspect this Decodex lane.");
}

#[test]
fn packaged_skills_preserve_research_promotion_and_queue_boundaries() {
	let skill_surface = format!(
		"{DECODEX_SKILL}\n{PLANNING_SKILL}\n{AUTOMATION_SKILL}\n{LABELS_SKILL}\n{RESEARCH_SKILL}\n{ROUTING_REF}\n{RESEARCH_METHOD_REF}"
	);
	let planning_surface = format!("{PLANNING_SKILL}\n{ISSUE_BRIEFING_REF}\n{ROUTING_REF}");

	assert_contains(&skill_surface, "## Natural-Language Research Routing");
	assert_contains(&skill_surface, "`research X`");
	assert_contains(&skill_surface, "`research-probe`");
	assert_contains(&skill_surface, "`research-promote`");
	assert_contains(&skill_surface, "legacy external `$research`");
	assert_contains(&skill_surface, "latent Decision Contract");
	assert_contains(&skill_surface, "\"arrange this\"");
	assert_contains(&skill_surface, "\"推进\"");
	assert_contains_normalized(&skill_surface, "never queues work");
	assert_contains(&skill_surface, "dispatches ready mapped nodes directly");
	assert_contains(&skill_surface, "accepted Decision Contract");
	assert_contains(&skill_surface, "Do not replace `WORKFLOW.md`");
	assert_contains_normalized(&skill_surface, "Program Intake dispatches ready mapped nodes");
	assert_contains(&skill_surface, "Promotion is a separate authority step");
	assert_contains(&skill_surface, "after execution authority exists");
	assert_contains_normalized(&skill_surface, "never queues work, mutates Linear");
	assert_contains(&skill_surface, "ordinary non-Program issue intake");
	assert_contains(&skill_surface, "not queue-label polling");
	assert_contains(&skill_surface, "Require promoted research");
	assert_contains(&planning_surface, "Decodex-native issue briefs");
	assert_contains(&planning_surface, "generic dispatch briefing");
	assert_contains(&planning_surface, "one outcome");
	assert_contains(&planning_surface, "explicit non-goals");
	assert_contains(&planning_surface, "current-tree landing zone");
	assert_contains(&planning_surface, "validation expectations");
	assert_contains(
		&planning_surface,
		"Do not route Decodex issue briefing through an external delivery workflow",
	);
	assert_not_contains(&planning_surface, "Pair with delivery");
}

#[test]
fn packaged_research_skills_encode_decodex_methodology() {
	let research_surface = format!(
		"{RESEARCH_SKILL}\n{RESEARCH_PROBE_SKILL}\n{RESEARCH_EVIDENCE_SKILL}\n{RESEARCH_OPTIONS_SKILL}\n{RESEARCH_JUDGMENT_SKILL}\n{RESEARCH_CHALLENGE_SKILL}\n{RESEARCH_DECISION_SKILL}\n{RESEARCH_PROMOTE_SKILL}\n{RESEARCH_METHOD_REF}"
	);

	assert_contains(&research_surface, "default research surface");
	assert_contains(&research_surface, "probe, evidence, options, judgment, challenge, decision");
	assert_contains_normalized(&research_surface, "No evidence, no claim");
	assert_contains_normalized(&research_surface, "runtime state");
	assert_contains(
		&research_surface,
		"Do not route Decodex research through the legacy external `$research`",
	);
	assert_contains(&research_surface, "primary hypothesis");
	assert_contains(&research_surface, "rival hypotheses");
	assert_contains(&research_surface, "falsifiers");
	assert_contains(&research_surface, "`probe_completed`");
	assert_contains(&research_surface, "No evidence, no claim");
	assert_contains(
		&research_surface,
		"Separate observations, contradictions, inferences, and missing evidence",
	);
	assert_contains(&research_surface, "`research_evidence`");
	assert_contains(&research_surface, "status quo");
	assert_contains(&research_surface, "evidence");
	assert_contains(&research_surface, "`research_options`");
	assert_contains(&research_surface, "challenge-ready");
	assert_contains(&research_surface, "stable judgment id or hash");
	assert_contains(&research_surface, "not_decision_ready");
	assert_contains(
		&research_surface,
		"Do not finalize `decision_ready` while material objections remain unresolved",
	);
	assert_contains(&research_surface, "skeptic worker");
	assert_contains(&research_surface, "Use exactly one terminal outcome");
	assert_contains(&research_surface, "No unresolved decisions, evidence gaps, or blockers");
	assert_contains(&research_surface, "Promotion is a separate authority step");
	assert_contains_normalized(&research_surface, "research-to-planning authority boundary");
	assert_contains(&research_surface, "Do not infer acceptance");
	assert_contains(&research_surface, "Program Intake");
}

#[test]
fn narrow_lifecycle_skills_are_explicit_only() {
	for policy in [
		AUTOMATION_AGENT_POLICY,
		COMMIT_AGENT_POLICY,
		LABELS_AGENT_POLICY,
		LAND_AGENT_POLICY,
		MANUAL_CLI_AGENT_POLICY,
	] {
		assert_contains(policy, "allow_implicit_invocation: false");
	}
}

fn assert_contains(haystack: &str, needle: &str) {
	assert!(haystack.contains(needle), "expected packaged plugin content to contain `{needle}`");
}

fn assert_contains_normalized(haystack: &str, needle: &str) {
	let normalized_haystack = haystack.split_whitespace().collect::<Vec<_>>().join(" ");
	let normalized_needle = needle.split_whitespace().collect::<Vec<_>>().join(" ");

	assert_contains(&normalized_haystack, &normalized_needle);
}

fn assert_not_contains(haystack: &str, needle: &str) {
	assert!(
		!haystack.contains(needle),
		"expected packaged plugin content not to contain `{needle}`"
	);
}
