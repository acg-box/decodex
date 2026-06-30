use serde_json::Value;

const DELIBERATION_PLUGIN_JSON: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/deliberation/.codex-plugin/plugin.json"
));
const DECODEX_PLUGIN_JSON: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/.codex-plugin/plugin.json"
));
const KNOWLEDGE_PLUGIN_JSON: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/knowledge/.codex-plugin/plugin.json"
));
const CODEBASE_PLUGIN_JSON: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/codebase/.codex-plugin/plugin.json"
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
const RESEARCH_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research/SKILL.md"
));
const RESEARCH_PROMOTE_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-promote/SKILL.md"
));
const LAND_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/land/SKILL.md"
));
const DELIBERATION_SKEPTIC_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/deliberation/skills/skeptic/SKILL.md"
));
const DELIBERATION_GRILL_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/deliberation/skills/grill/SKILL.md"
));
const DELIBERATION_SCOUT_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/deliberation/skills/scout/SKILL.md"
));
const DELIBERATION_GATE_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/deliberation/references/deliberation-gate.md"
));
const KNOWLEDGE_DOCS_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/knowledge/skills/docs/SKILL.md"
));
const KNOWLEDGE_DOCS_DRIFT_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/knowledge/skills/docs-drift/SKILL.md"
));
const KNOWLEDGE_OKF_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/knowledge/skills/okf/SKILL.md"
));
const KNOWLEDGE_REPO_MEMORY_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/knowledge/skills/repo-memory/SKILL.md"
));
const KNOWLEDGE_WRITEBACK_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/knowledge/skills/writeback/SKILL.md"
));
const CODEBASE_WORK_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/codebase/skills/work/SKILL.md"
));
const REPO_DEBUGGING_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/codebase/skills/debugging/SKILL.md"
));
const REPO_VERIFICATION_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/codebase/skills/verification/SKILL.md"
));
const REPO_REVIEW_FEEDBACK_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/codebase/skills/review-feedback/SKILL.md"
));
const REPO_DEPENDENCY_POLICY_SKILL: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/codebase/skills/dependency-policy/SKILL.md"
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
const RESEARCH_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research/agents/openai.yaml"
));
const RESEARCH_PROMOTE_AGENT_POLICY: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/skills/research-promote/agents/openai.yaml"
));
const ROUTING_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/routing.md"
));
const RESEARCH_LIFECYCLE_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/research-lifecycle.md"
));
const RESEARCH_CONTRACT_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/research-contract.md"
));
const RESEARCH_PROMOTION_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/decodex/references/research-promotion.md"
));
const KNOWLEDGE_DOCS_METHOD_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/knowledge/references/docs-method.md"
));
const KNOWLEDGE_DOCS_OKF_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/knowledge/references/docs-okf.md"
));
const KNOWLEDGE_DOCS_WIKI_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/knowledge/references/docs-wiki.md"
));
const KNOWLEDGE_DOCS_DRIFT_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/knowledge/references/docs-drift.md"
));
const KNOWLEDGE_OKF_LAYER_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/knowledge/references/okf-layer.md"
));
const CODEBASE_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/codebase/references/codebase.md"
));
const REPO_DEPENDENCY_POLICY_REF: &str = include_str!(concat!(
	env!("CARGO_MANIFEST_DIR"),
	"/../../plugins/codebase/references/dependency-policy.md"
));

#[test]
fn packaged_plugin_manifests_route_split_surface_owners() {
	let decodex_surface = manifest_interface_surface(DECODEX_PLUGIN_JSON);
	let knowledge_surface = manifest_interface_surface(KNOWLEDGE_PLUGIN_JSON);
	let codebase_surface = manifest_interface_surface(CODEBASE_PLUGIN_JSON);
	let deliberation_surface = manifest_interface_surface(DELIBERATION_PLUGIN_JSON);
	let routing_surface = format!("{ROUTING_REF}\n{KNOWLEDGE_OKF_LAYER_REF}\n{CODEBASE_REF}");

	assert_contains(&decodex_surface, "bounded research");
	assert_contains(&decodex_surface, "runtime ops");
	assert_contains(&decodex_surface, "companion plugins");
	assert_contains(&decodex_surface, "Research this with Decodex.");
	assert_contains(&decodex_surface, "Plan accepted Decodex work.");
	assert_contains(&decodex_surface, "Operate Decodex.");
	assert_not_contains(&decodex_surface, "Maintain Decodex docs.");
	assert_not_contains(&decodex_surface, "Work with an OKF bundle.");
	assert_contains(&knowledge_surface, "OKF and LLM Wiki");
	assert_contains(&knowledge_surface, "semantic drift audits");
	assert_contains(&knowledge_surface, "source-backed repository memory");
	assert_contains(&knowledge_surface, "automatic knowledge writeback");
	assert_contains(&knowledge_surface, "Maintain docs and OKF.");
	assert_contains(&knowledge_surface, "Audit semantic drift.");
	assert_contains(&knowledge_surface, "Write back stable repo knowledge.");
	assert_contains(&codebase_surface, "task-runner structure");
	assert_contains(&codebase_surface, "module-boundary defaults");
	assert_contains(&codebase_surface, "verification evidence");
	assert_contains(&codebase_surface, "root-cause debugging");
	assert_contains(&codebase_surface, "Apply codebase rules.");
	assert_contains(&codebase_surface, "Verify this change.");
	assert_contains(&codebase_surface, "Debug this failure.");
	assert_contains(&deliberation_surface, "read-only evidence scouting");
	assert_contains(&deliberation_surface, "decision grilling");
	assert_contains(&deliberation_surface, "evidence sufficiency");
	assert_contains(&deliberation_surface, "Scout this evidence.");
	assert_contains(&deliberation_surface, "Grill this plan.");
	assert_contains(&deliberation_surface, "Skeptic-review this claim.");
	assert_contains(&routing_surface, "$knowledge:docs");
	assert_contains(&routing_surface, "$knowledge:okf");
	assert_contains(&routing_surface, "$knowledge:repo-memory");
	assert_contains(&routing_surface, "$codebase:work");
	assert_contains(&routing_surface, "$deliberation:skeptic");
	assert_contains(&routing_surface, "$knowledge:docs-drift");
	assert_contains(&routing_surface, "$knowledge:writeback");
	assert_contains(&routing_surface, "Runtime: `docs/spec/` and `docs/runbook/`");
}

#[test]
fn packaged_decodex_skills_preserve_research_promotion_and_program_boundaries() {
	let skill_surface = format!(
		"{DECODEX_SKILL}\n{DECODEX_OPS_SKILL}\n{PLANNING_SKILL}\n{RESEARCH_SKILL}\n{RESEARCH_PROMOTE_SKILL}\n{ROUTING_REF}\n{RESEARCH_LIFECYCLE_REF}\n{RESEARCH_CONTRACT_REF}\n{RESEARCH_PROMOTION_REF}"
	);
	let planning_surface = format!("{PLANNING_SKILL}\n{ROUTING_REF}");

	assert_contains(&skill_surface, "Research/design");
	assert_contains(&skill_surface, "output is latent until promoted");
	assert_contains(&skill_surface, "$deliberation:skeptic");
	assert_contains(&skill_surface, "`research-promote`");
	assert_contains(
		&skill_surface,
		"Do not route Decodex research through external research skills",
	);
	assert_contains(&skill_surface, "contract-first Decision Contract");
	assert_contains(&skill_surface, "explicit acceptance");
	assert_contains_normalized(&skill_surface, "Research never queues work");
	assert_contains_normalized(&skill_surface, "Program Intake dispatches persisted Program nodes");
	assert_contains(&skill_surface, "queue labels are not scheduling");
	assert_contains(&skill_surface, "Ordinary intake starts");
	assert_contains(&skill_surface, "not queue-label polling");
	assert_contains(&skill_surface, "decodex:needs-attention");
	assert_contains(&skill_surface, "terminal_pending");
	assert_contains(&skill_surface, "Require promoted research");
	assert_contains_normalized(&skill_surface, "promotion is separate");
	assert_contains(&skill_surface, "after promotion or explicit execution instruction");
	assert_contains(&skill_surface, "runtime operations");
	assert_contains(&skill_surface, "service labels");
	assert_contains(&skill_surface, "$knowledge:docs");
	assert_contains(&skill_surface, "docs/evidence");
	assert_contains(&skill_surface, "LLM Wiki indexes");
	assert_contains(&skill_surface, "current truth");
	assert_contains_normalized(&skill_surface, "Do not queue work, mutate Linear");
	assert_contains(&skill_surface, "Program Intake");
	assert_contains(&planning_surface, "Decodex-native issue briefs");
	assert_contains(&skill_surface, "decodex://docs/spec/autonomy-control-plane");
	assert_contains(&skill_surface, "decodex://projects/{service_id}/autonomy");
	assert_contains(&skill_surface, "autonomy_submit_signal");
	assert_contains(&skill_surface, "autonomy_request_promotion");
	assert_contains_normalized(&skill_surface, "Auth and profile prove access only");
	assert_contains(&planning_surface, "cold-start lane");
	assert_contains(&planning_surface, "outcome");
	assert_contains(&planning_surface, "non-goals");
	assert_contains(&planning_surface, "landing zone");
	assert_contains(&planning_surface, "validation expectations");
	assert_contains(&planning_surface, "Do not invent modules");
	assert_contains(&planning_surface, "Do not replace `WORKFLOW.md`");
	assert_contains(&planning_surface, "do not call external delivery");
	assert_not_contains(&planning_surface, "Pair with delivery");
}

#[test]
fn packaged_research_and_skeptic_skills_encode_decodex_methodology() {
	let research_surface = format!(
		"{RESEARCH_SKILL}\n{RESEARCH_PROMOTE_SKILL}\n{DELIBERATION_SKEPTIC_SKILL}\n{DELIBERATION_SCOUT_SKILL}\n{DELIBERATION_GRILL_SKILL}\n{DELIBERATION_GATE_REF}\n{RESEARCH_LIFECYCLE_REF}\n{RESEARCH_CONTRACT_REF}\n{RESEARCH_PROMOTION_REF}"
	);

	assert_contains(&research_surface, "bounded Decodex research");
	assert_contains_normalized(
		&research_surface,
		"first-principles probe, scout evidence, options, judgment, skeptic review, decision",
	);
	assert_contains(&research_surface, "Deliberation Gate");
	assert_contains(&research_surface, "Inline exception");
	assert_contains(&research_surface, "$deliberation:skeptic");
	assert_contains_normalized(&research_surface, "No evidence, no claim");
	assert_contains(
		&research_surface,
		"Do not route Decodex research through external research skills",
	);
	assert_contains(&research_surface, "primary/rival hypotheses");
	assert_contains(&research_surface, "falsifiers");
	assert_contains(&research_surface, "contradictions");
	assert_contains(&research_surface, "external source");
	assert_contains(&research_surface, "repo source");
	assert_contains(&research_surface, "live readback");
	assert_contains(&research_surface, "status quo");
	assert_contains(&research_surface, "skeptic pass");
	assert_contains(&research_surface, "not_decision_ready");
	assert_contains_normalized(
		&research_surface,
		"unresolved material objections block `decision_ready`",
	);
	assert_contains(&research_surface, "Use exactly one");
	assert_contains(&research_surface, "Refuse unresolved decisions");
	assert_contains_normalized(&research_surface, "promotion is separate");
	assert_contains(&research_surface, "Promotion requires explicit acceptance");
	assert_contains(
		&research_surface,
		"research-only evidence versus durable knowledge candidates",
	);
	assert_contains(&research_surface, "OKF disposition");
	assert_contains(&research_surface, "promote_and_supersede");
	assert_contains(&research_surface, "promote_and_retire");
	assert_contains(&research_surface, "reject_or_deprecate");
	assert_contains(&research_surface, "docs/decisions");
	assert_contains(&research_surface, "docs/evidence");
	assert_contains(&research_surface, "Use `no_promotion` only when");
	assert_contains(&research_surface, "route execution to `planning`");
	assert_contains(&research_surface, "missing evidence");
	assert_contains(&research_surface, "premature success claims");
	assert_contains(&research_surface, "bounded read-only evidence gathering");
	assert_contains(&research_surface, "Pressure-test the shape of the work before execution");
}

#[test]
fn packaged_decodex_skills_route_review_evidence_and_handoff_recovery_to_runtime_authority() {
	let ops_surface = format!("{DECODEX_OPS_SKILL}\n{ROUTING_REF}");
	let research_surface = format!("{RESEARCH_SKILL}\n{ROUTING_REF}");
	let land_surface = format!("{LAND_SKILL}\n{ROUTING_REF}");

	assert_contains(&ops_surface, "missing_review_handoff_record");
	assert_contains(&ops_surface, "`decodex recover review-handoff diagnose <ISSUE> --json`");
	assert_contains(
		&ops_surface,
		"`decodex recover review-handoff rebind <ISSUE> --pr <URL> --dry-run`",
	);
	assert_contains(
		&ops_surface,
		"`decodex recover review-handoff adopt <ISSUE> --pr <URL> --dry-run`",
	);
	assert_contains(&ops_surface, "Decodex-owned retained lane PR");
	assert_contains(&ops_surface, "human-owned PR takeover");
	assert_contains(&ops_surface, "Do not infer PR lineage");
	assert_contains(
		&research_surface,
		"research compact loop is not runtime `compact_current_head_review`",
	);
	assert_contains(&research_surface, "issue_review_checkpoint");
	assert_contains(&research_surface, "`review_cost_control`");
	assert_contains(&research_surface, "`decodex evidence`");
	assert_contains_normalized(&research_surface, "not a skipped-review signal");
	assert_contains(&land_surface, "`decodex land --authority <ISSUE> --pr <URL> \"<summary>\"`");
	assert_contains(&land_surface, "Only `decodex land` lands a Decodex-owned PR");
	assert_contains_normalized(
		&land_surface,
		"Rebind restores or refreshes a Decodex-owned retained lane",
	);
	assert_contains_normalized(&land_surface, "adopt is for a human-owned PR takeover");
	assert_contains(&land_surface, "do not land the PR");
	assert_contains(&land_surface, "Do not use global `AGENTS.md`");
}

#[test]
fn packaged_knowledge_skills_encode_okf_wiki_and_drift_boundaries() {
	let docs_surface = format!(
		"{KNOWLEDGE_DOCS_SKILL}\n{KNOWLEDGE_DOCS_DRIFT_SKILL}\n{KNOWLEDGE_OKF_SKILL}\n{KNOWLEDGE_REPO_MEMORY_SKILL}\n{KNOWLEDGE_WRITEBACK_SKILL}\n{KNOWLEDGE_DOCS_METHOD_REF}\n{KNOWLEDGE_DOCS_OKF_REF}\n{KNOWLEDGE_DOCS_WIKI_REF}\n{KNOWLEDGE_DOCS_DRIFT_REF}\n{KNOWLEDGE_OKF_LAYER_REF}\n{ROUTING_REF}"
	);

	assert_contains(&docs_surface, "OKF");
	assert_contains(&docs_surface, "LLM Wiki");
	assert_contains(&docs_surface, "docs check");
	assert_contains(&docs_surface, "Markdown-only");
	assert_contains(&docs_surface, "Research Contract");
	assert_contains(&docs_surface, "Drift Audit");
	assert_contains(&docs_surface, "docs impact");
	assert_contains(&docs_surface, "research_required");
	assert_contains(&docs_surface, "one authoritative concept per claim");
	assert_contains_normalized(&docs_surface, "superseded research routes as provenance");
	assert_contains(&docs_surface, "`docs/evidence`");
	assert_contains(&docs_surface, "pass`, `fail`, or `needs-human");
	assert_contains(&docs_surface, "Do not create a parallel `wiki/` or `okf/` root");
	assert_not_contains(&docs_surface, "Okf");
	assert_contains(&docs_surface, "portable OKF");
	assert_contains(&docs_surface, "Decodex docs profile");
	assert_contains(&docs_surface, "Close the loop between implementation and durable knowledge");
	assert_contains(&docs_surface, "Prefer automatic writeback");
}

#[test]
fn packaged_okf_skills_preserve_portable_profile_boundary() {
	let okf_surface =
		format!("{KNOWLEDGE_OKF_SKILL}\n{KNOWLEDGE_REPO_MEMORY_SKILL}\n{KNOWLEDGE_OKF_LAYER_REF}");

	assert_contains(&okf_surface, "portable OKF");
	assert_contains(&okf_surface, "LLM Wiki");
	assert_contains(&okf_surface, "repo-memory");
	assert_contains(&okf_surface, "source-backed repository memory");
	assert_contains(&okf_surface, "The CLI does not replace the LLM");
	assert_contains(&okf_surface, "decodex okf init");
	assert_contains(&okf_surface, "decodex okf check");
	assert_contains(&okf_surface, "decodex okf find");
	assert_contains(&okf_surface, "decodex okf graph");
	assert_contains(&okf_surface, "core");
	assert_contains(&okf_surface, "wiki");
	assert_contains(&okf_surface, "repo-memory");
	assert_contains(&okf_surface, "decodex");
	assert_contains(&okf_surface, "source_refs");
	assert_contains(&okf_surface, "code_refs");
	assert_contains(&okf_surface, "related");
	assert_contains(&okf_surface, "drift_watch");
	assert_contains_normalized(&okf_surface, "Do not create or recommend `decodex docs okf ...`");
	assert_contains_normalized(&okf_surface, "do not inherit runtime lanes");
	assert_contains(&okf_surface, "docs-impact checkpoints");
}

#[test]
fn packaged_codebase_skills_preserve_command_and_verification_authority() {
	let repo_surface = format!(
		"{CODEBASE_WORK_SKILL}\n{REPO_DEBUGGING_SKILL}\n{REPO_VERIFICATION_SKILL}\n{REPO_REVIEW_FEEDBACK_SKILL}\n{REPO_DEPENDENCY_POLICY_SKILL}\n{CODEBASE_REF}\n{REPO_DEPENDENCY_POLICY_REF}"
	);

	assert_contains(&repo_surface, "Checked-in command authority used");
	assert_contains(&repo_surface, "Task Runner Structure");
	assert_contains(&repo_surface, "Prefer repo-native commands");
	assert_contains(&repo_surface, "Makefile.toml");
	assert_contains(&repo_surface, "Root-cause investigation");
	assert_contains_normalized(&repo_surface, "symptom -> owner boundary -> fresh baseline");
	assert_contains(&repo_surface, "Every positive claim must have evidence");
	assert_contains(&repo_surface, "Implemented, not fully verified");
	assert_contains(&repo_surface, "verified_actionable");
	assert_contains(&repo_surface, "Run `plugin-eval analyze <plugin-root> --format markdown`");
	assert_contains(&repo_surface, "Task-runner review checklist");
	assert_contains(&repo_surface, "External `uses: owner/action@ref`");
	assert_contains(&repo_surface, "whole discoverable dependency surface");
	assert_contains(&repo_surface, "open Dependabot PRs are authoritative candidates");
	assert_contains(&repo_surface, "residual dependency checks");
	assert_contains(&repo_surface, "requires-follow-up-migration");
	assert_contains(&repo_surface, "Do not duplicate this routing in host bootstrap files");
}

#[test]
fn decodex_lifecycle_specialist_skills_stay_explicit_only() {
	for policy in [
		COMMIT_AGENT_POLICY,
		DECODEX_OPS_AGENT_POLICY,
		LAND_AGENT_POLICY,
		PLANNING_AGENT_POLICY,
		RESEARCH_AGENT_POLICY,
		RESEARCH_PROMOTE_AGENT_POLICY,
	] {
		assert_contains(policy, "allow_implicit_invocation: false");
	}
}

#[test]
fn codebase_knowledge_and_deliberation_skills_allow_implicit_routing() {
	let implicit_surface = format!(
		"{CODEBASE_WORK_SKILL}\n{REPO_DEBUGGING_SKILL}\n{REPO_VERIFICATION_SKILL}\n{REPO_REVIEW_FEEDBACK_SKILL}\n{REPO_DEPENDENCY_POLICY_SKILL}\n{KNOWLEDGE_DOCS_SKILL}\n{KNOWLEDGE_DOCS_DRIFT_SKILL}\n{KNOWLEDGE_OKF_SKILL}\n{KNOWLEDGE_REPO_MEMORY_SKILL}\n{KNOWLEDGE_WRITEBACK_SKILL}\n{DELIBERATION_SKEPTIC_SKILL}\n{DELIBERATION_SCOUT_SKILL}\n{DELIBERATION_GRILL_SKILL}"
	);

	assert_not_contains(&implicit_surface, "allow_implicit_invocation: false");
	assert_contains(&implicit_surface, "description: Use when repository code work");
	assert_contains(&implicit_surface, "description: Use when repository docs");
	assert_contains(
		&implicit_surface,
		"description: Use when a task needs bounded read-only evidence gathering",
	);
	assert_contains(&implicit_surface, "description: Use when unclear intent");
}

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
