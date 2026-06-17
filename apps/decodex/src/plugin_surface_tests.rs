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
const OKF_SKILL: &str =
	include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/decodex/skills/okf/SKILL.md"));
const OKF_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/okf/agents/openai.yaml"
));
const OKF_QUERY_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/okf-query/SKILL.md"
));
const OKF_QUERY_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/okf-query/agents/openai.yaml"
));
const OKF_MAINTAIN_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/okf-maintain/SKILL.md"
));
const OKF_MAINTAIN_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/okf-maintain/agents/openai.yaml"
));
const DOCS_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/docs/SKILL.md"
));
const DOCS_OKF_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/docs-okf/SKILL.md"
));
const DOCS_OKF_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/docs-okf/agents/openai.yaml"
));
const DOCS_WIKI_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/docs-wiki/SKILL.md"
));
const DOCS_WIKI_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/docs-wiki/agents/openai.yaml"
));
const DOCS_DRIFT_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/docs-drift/SKILL.md"
));
const DOCS_DRIFT_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/docs-drift/agents/openai.yaml"
));
const RESEARCH_PROBE_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-probe/SKILL.md"
));
const RESEARCH_PROBE_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-probe/agents/openai.yaml"
));
const RESEARCH_EVIDENCE_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-evidence/SKILL.md"
));
const RESEARCH_EVIDENCE_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-evidence/agents/openai.yaml"
));
const RESEARCH_OPTIONS_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-options/SKILL.md"
));
const RESEARCH_OPTIONS_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-options/agents/openai.yaml"
));
const RESEARCH_JUDGMENT_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-judgment/SKILL.md"
));
const RESEARCH_JUDGMENT_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-judgment/agents/openai.yaml"
));
const RESEARCH_CHALLENGE_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-challenge/SKILL.md"
));
const RESEARCH_CHALLENGE_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-challenge/agents/openai.yaml"
));
const RESEARCH_DECISION_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-decision/SKILL.md"
));
const RESEARCH_DECISION_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-decision/agents/openai.yaml"
));
const RESEARCH_PROMOTE_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-promote/SKILL.md"
));
const RESEARCH_PROMOTE_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-promote/agents/openai.yaml"
));
const ROUTING_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/routing.md"
));
const ISSUE_BRIEFING_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/issue-briefing.md"
));
const DOCS_METHOD_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/docs-method.md"
));
const DOCS_OKF_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/docs-okf.md"
));
const DOCS_WIKI_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/docs-wiki.md"
));
const DOCS_DRIFT_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/docs-drift.md"
));
const OKF_LAYER_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/okf-layer.md"
));
const RESEARCH_LIFECYCLE_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/research-lifecycle.md"
));
const RESEARCH_EVIDENCE_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/research-evidence.md"
));
const RESEARCH_CONTRACT_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/research-contract.md"
));
const RESEARCH_PROMOTION_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/research-promotion.md"
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
	let manifest_surface = format!(
		"{long_description}\n{default_prompts}\n{ROUTING_REF}\n{ISSUE_BRIEFING_REF}\n{RESEARCH_LIFECYCLE_REF}\n{RESEARCH_CONTRACT_REF}"
	);

	assert_contains(&manifest_surface, "natural-language-first");
	assert_contains(&manifest_surface, "portable OKF");
	assert_contains(&manifest_surface, "OKF docs");
	assert_contains(&manifest_surface, "bounded research");
	assert_contains(&manifest_surface, "probe, evidence, options, judgment, challenge, decision");
	assert_contains(&manifest_surface, "`research X`");
	assert_contains(&manifest_surface, "latent Decision Contract");
	assert_contains(&manifest_surface, "\"arrange this\"");
	assert_contains(&manifest_surface, "\"推进\"");
	assert_contains(&manifest_surface, "ready mapped nodes directly");
	assert_contains(&manifest_surface, "DAG");
	assert_contains(&manifest_surface, "issue briefing");
	assert_contains(&default_prompts, "Maintain Decodex docs.");
	assert_contains(&default_prompts, "Work with an OKF bundle.");
	assert_contains(&default_prompts, "Research this with Decodex.");
}

#[test]
fn packaged_skills_preserve_research_promotion_and_queue_boundaries() {
	let skill_surface = format!(
		"{DECODEX_SKILL}\n{PLANNING_SKILL}\n{AUTOMATION_SKILL}\n{LABELS_SKILL}\n{RESEARCH_SKILL}\n{ROUTING_REF}\n{RESEARCH_LIFECYCLE_REF}\n{RESEARCH_PROMOTION_REF}"
	);
	let planning_surface = format!("{PLANNING_SKILL}\n{ISSUE_BRIEFING_REF}\n{ROUTING_REF}");

	assert_contains(&skill_surface, "## Natural-Language Research Routing");
	assert_contains(&skill_surface, "`research X`");
	assert_contains(&skill_surface, "`research-probe`");
	assert_contains(&skill_surface, "`research-promote`");
	assert_contains(&skill_surface, "external research skills");
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
	assert_contains_normalized(&skill_surface, "does not queue work, mutate Linear");
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
		"{RESEARCH_SKILL}\n{RESEARCH_PROBE_SKILL}\n{RESEARCH_EVIDENCE_SKILL}\n{RESEARCH_OPTIONS_SKILL}\n{RESEARCH_JUDGMENT_SKILL}\n{RESEARCH_CHALLENGE_SKILL}\n{RESEARCH_DECISION_SKILL}\n{RESEARCH_PROMOTE_SKILL}\n{RESEARCH_LIFECYCLE_REF}\n{RESEARCH_EVIDENCE_REF}\n{RESEARCH_CONTRACT_REF}\n{RESEARCH_PROMOTION_REF}"
	);

	assert_contains(&research_surface, "default research surface");
	assert_contains(&research_surface, "probe, evidence, options, judgment, challenge, decision");
	assert_contains_normalized(&research_surface, "No evidence, no claim");
	assert_contains_normalized(&research_surface, "runtime state");
	assert_contains(
		&research_surface,
		"Do not route Decodex research through external research skills",
	);
	assert_contains(&research_surface, "primary hypothesis");
	assert_contains(&research_surface, "rival hypotheses");
	assert_contains(&research_surface, "falsifiers");
	assert_contains(&research_surface, "No evidence, no claim");
	assert_contains(&research_surface, "observations");
	assert_contains(&research_surface, "contradictions");
	assert_contains(&research_surface, "external_source");
	assert_contains(&research_surface, "repo_source");
	assert_contains(&research_surface, "live_readback");
	assert_contains(&research_surface, "status quo");
	assert_contains(&research_surface, "evidence");
	assert_contains(&research_surface, "challenge-ready");
	assert_contains(&research_surface, "not_decision_ready");
	assert_contains(
		&research_surface,
		"Do not finalize `decision_ready` while material objections remain unresolved",
	);
	assert_contains_normalized(
		&research_surface,
		"Unresolved material objections block `decision_ready`",
	);
	assert_contains(&research_surface, "Use exactly one");
	assert_contains(&research_surface, "refuse promotion while unresolved decisions");
	assert_contains(&research_surface, "Promotion is a separate authority step");
	assert_contains(&research_surface, "Promotion requires explicit acceptance");
	assert_contains(&research_surface, "Do not infer acceptance");
	assert_contains(&research_surface, "Program Intake");
}

#[test]
fn packaged_docs_skills_encode_okf_wiki_and_drift_boundaries() {
	let docs_surface = format!(
		"{DOCS_SKILL}\n{DOCS_OKF_SKILL}\n{DOCS_WIKI_SKILL}\n{DOCS_DRIFT_SKILL}\n{DOCS_METHOD_REF}\n{DOCS_OKF_REF}\n{DOCS_WIKI_REF}\n{DOCS_DRIFT_REF}\n{ROUTING_REF}"
	);

	assert_contains(&docs_surface, "OKF");
	assert_contains(&docs_surface, "LLM Wiki");
	assert_contains(&docs_surface, "docs check");
	assert_contains(&docs_surface, "docs lint");
	assert_contains(&docs_surface, "Markdown-only");
	assert_contains(&docs_surface, "Research Contract");
	assert_contains(&docs_surface, "Drift Audit");
	assert_contains(&docs_surface, "docs impact");
	assert_contains(&docs_surface, "research_required");
	assert_contains(&docs_surface, "one authoritative concept per claim");
	assert_contains(&docs_surface, "pass`, `fail`, or `needs-human");
	assert_contains(&docs_surface, "Do not create a parallel `wiki/` or `okf/` root");
	assert_not_contains(&docs_surface, "Okf");
	assert_contains(&docs_surface, "portable OKF");
	assert_contains(&docs_surface, "Decodex profile");
}

#[test]
fn packaged_okf_skills_preserve_portable_profile_boundary() {
	let okf_surface =
		format!("{OKF_SKILL}\n{OKF_QUERY_SKILL}\n{OKF_MAINTAIN_SKILL}\n{OKF_LAYER_REF}");

	assert_contains(&okf_surface, "portable OKF");
	assert_contains(&okf_surface, "LLM Wiki");
	assert_contains(&okf_surface, "decodex okf check");
	assert_contains(&okf_surface, "decodex okf find");
	assert_contains(&okf_surface, "decodex okf graph");
	assert_contains(&okf_surface, "decodex okf route");
	assert_contains(&okf_surface, "core");
	assert_contains(&okf_surface, "wiki");
	assert_contains(&okf_surface, "repo-memory");
	assert_contains(&okf_surface, "decodex");
	assert_contains(&okf_surface, "source_refs");
	assert_contains(&okf_surface, "code_refs");
	assert_contains(&okf_surface, "related");
	assert_contains(&okf_surface, "drift_watch");
	assert_contains_normalized(&okf_surface, "Do not create or recommend `decodex docs okf ...`");
	assert_contains_normalized(&okf_surface, "do not inherit Decodex lanes");
	assert_contains(&okf_surface, "Do not require Decodex lanes or Linear workflow");
}

#[test]
fn narrow_lifecycle_and_specialist_skills_are_explicit_only() {
	for policy in [
		AUTOMATION_AGENT_POLICY,
		COMMIT_AGENT_POLICY,
		LABELS_AGENT_POLICY,
		LAND_AGENT_POLICY,
		MANUAL_CLI_AGENT_POLICY,
		OKF_AGENT_POLICY,
		OKF_QUERY_AGENT_POLICY,
		OKF_MAINTAIN_AGENT_POLICY,
		DOCS_OKF_AGENT_POLICY,
		DOCS_WIKI_AGENT_POLICY,
		DOCS_DRIFT_AGENT_POLICY,
		RESEARCH_PROBE_AGENT_POLICY,
		RESEARCH_EVIDENCE_AGENT_POLICY,
		RESEARCH_OPTIONS_AGENT_POLICY,
		RESEARCH_JUDGMENT_AGENT_POLICY,
		RESEARCH_CHALLENGE_AGENT_POLICY,
		RESEARCH_DECISION_AGENT_POLICY,
		RESEARCH_PROMOTE_AGENT_POLICY,
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
