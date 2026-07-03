use crate::plugin_surface_tests::{
	self, DECODEX_OPS_SKILL, DECODEX_SKILL, DELIBERATION_GATE_REF, DELIBERATION_GRILL_SKILL,
	DELIBERATION_SCOUT_SKILL, DELIBERATION_SKEPTIC_SKILL, LAND_SKILL, PLANNING_SKILL,
	RESEARCH_CONTRACT_REF, RESEARCH_LIFECYCLE_REF, RESEARCH_PROMOTE_SKILL, RESEARCH_PROMOTION_REF,
	RESEARCH_SKILL, ROUTING_REF,
};

#[test]
fn packaged_decodex_skills_preserve_research_promotion_and_program_boundaries() {
	let skill_surface = format!(
		"{DECODEX_SKILL}\n{DECODEX_OPS_SKILL}\n{PLANNING_SKILL}\n{RESEARCH_SKILL}\n{RESEARCH_PROMOTE_SKILL}\n{ROUTING_REF}\n{RESEARCH_LIFECYCLE_REF}\n{RESEARCH_CONTRACT_REF}\n{RESEARCH_PROMOTION_REF}"
	);
	let planning_surface = format!("{PLANNING_SKILL}\n{ROUTING_REF}");

	plugin_surface_tests::assert_contains(&skill_surface, "Research/design");
	plugin_surface_tests::assert_contains(&skill_surface, "output is latent until promoted");
	plugin_surface_tests::assert_contains(&skill_surface, "$deliberation:skeptic");
	plugin_surface_tests::assert_contains(&skill_surface, "`research-promote`");
	plugin_surface_tests::assert_contains(
		&skill_surface,
		"Do not route Decodex research through external research skills",
	);
	plugin_surface_tests::assert_contains(&skill_surface, "contract-first Decision Contract");
	plugin_surface_tests::assert_contains(&skill_surface, "explicit acceptance");
	plugin_surface_tests::assert_contains_normalized(&skill_surface, "Research never queues work");
	plugin_surface_tests::assert_contains_normalized(
		&skill_surface,
		"Program Intake dispatches persisted Program nodes",
	);
	plugin_surface_tests::assert_contains(&skill_surface, "queue labels are not scheduling");
	plugin_surface_tests::assert_contains(&skill_surface, "Ordinary intake starts");
	plugin_surface_tests::assert_contains(&skill_surface, "not queue-label polling");
	plugin_surface_tests::assert_contains(&skill_surface, "decodex:needs-attention");
	plugin_surface_tests::assert_contains(&skill_surface, "terminal_pending");
	plugin_surface_tests::assert_contains(&skill_surface, "Require promoted research");
	plugin_surface_tests::assert_contains_normalized(&skill_surface, "promotion is separate");
	plugin_surface_tests::assert_contains(
		&skill_surface,
		"after promotion or explicit execution instruction",
	);
	plugin_surface_tests::assert_contains(&skill_surface, "runtime operations");
	plugin_surface_tests::assert_contains(&skill_surface, "service labels");
	plugin_surface_tests::assert_contains(&skill_surface, "$knowledge:docs");
	plugin_surface_tests::assert_contains(&skill_surface, "docs/evidence");
	plugin_surface_tests::assert_contains(&skill_surface, "LLM Wiki indexes");
	plugin_surface_tests::assert_contains(&skill_surface, "current truth");
	plugin_surface_tests::assert_contains_normalized(
		&skill_surface,
		"Do not queue work, mutate Linear",
	);
	plugin_surface_tests::assert_contains(&skill_surface, "Program Intake");
	plugin_surface_tests::assert_contains(&planning_surface, "Decodex-native issue briefs");
	plugin_surface_tests::assert_contains(
		&skill_surface,
		"decodex://docs/spec/autonomy-control-plane",
	);
	plugin_surface_tests::assert_contains(
		&skill_surface,
		"decodex://projects/{service_id}/autonomy",
	);
	plugin_surface_tests::assert_contains(&skill_surface, "autonomy_submit_signal");
	plugin_surface_tests::assert_contains(&skill_surface, "autonomy_request_promotion");
	plugin_surface_tests::assert_contains_normalized(
		&skill_surface,
		"Auth and profile prove access only",
	);
	plugin_surface_tests::assert_contains(&planning_surface, "cold-start lane");
	plugin_surface_tests::assert_contains(&planning_surface, "outcome");
	plugin_surface_tests::assert_contains(&planning_surface, "non-goals");
	plugin_surface_tests::assert_contains(&planning_surface, "landing zone");
	plugin_surface_tests::assert_contains(&planning_surface, "validation expectations");
	plugin_surface_tests::assert_contains(&planning_surface, "Do not invent modules");
	plugin_surface_tests::assert_contains(&planning_surface, "Do not replace `WORKFLOW.md`");
	plugin_surface_tests::assert_contains(&planning_surface, "do not call external delivery");
	plugin_surface_tests::assert_not_contains(&planning_surface, "Pair with delivery");
}

#[test]
fn packaged_research_and_skeptic_skills_encode_decodex_methodology() {
	let research_surface = format!(
		"{RESEARCH_SKILL}\n{RESEARCH_PROMOTE_SKILL}\n{DELIBERATION_SKEPTIC_SKILL}\n{DELIBERATION_SCOUT_SKILL}\n{DELIBERATION_GRILL_SKILL}\n{DELIBERATION_GATE_REF}\n{RESEARCH_LIFECYCLE_REF}\n{RESEARCH_CONTRACT_REF}\n{RESEARCH_PROMOTION_REF}"
	);

	plugin_surface_tests::assert_contains(&research_surface, "bounded Decodex research");
	plugin_surface_tests::assert_contains_normalized(
		&research_surface,
		"first-principles probe, scout evidence, options, judgment, skeptic review, decision",
	);
	plugin_surface_tests::assert_contains(&research_surface, "Deliberation Gate");
	plugin_surface_tests::assert_contains(&research_surface, "Inline exception");
	plugin_surface_tests::assert_contains(&research_surface, "$deliberation:skeptic");
	plugin_surface_tests::assert_contains_normalized(&research_surface, "No evidence, no claim");
	plugin_surface_tests::assert_contains(
		&research_surface,
		"Do not route Decodex research through external research skills",
	);
	plugin_surface_tests::assert_contains(&research_surface, "primary/rival hypotheses");
	plugin_surface_tests::assert_contains(&research_surface, "falsifiers");
	plugin_surface_tests::assert_contains(&research_surface, "contradictions");
	plugin_surface_tests::assert_contains(&research_surface, "external source");
	plugin_surface_tests::assert_contains(&research_surface, "repo source");
	plugin_surface_tests::assert_contains(&research_surface, "live readback");
	plugin_surface_tests::assert_contains(&research_surface, "status quo");
	plugin_surface_tests::assert_contains(&research_surface, "skeptic pass");
	plugin_surface_tests::assert_contains(&research_surface, "not_decision_ready");
	plugin_surface_tests::assert_contains_normalized(
		&research_surface,
		"unresolved material objections block `decision_ready`",
	);
	plugin_surface_tests::assert_contains(&research_surface, "Use exactly one");
	plugin_surface_tests::assert_contains(&research_surface, "Refuse unresolved decisions");
	plugin_surface_tests::assert_contains_normalized(&research_surface, "promotion is separate");
	plugin_surface_tests::assert_contains(
		&research_surface,
		"Promotion requires explicit acceptance",
	);
	plugin_surface_tests::assert_contains(
		&research_surface,
		"research-only evidence versus durable knowledge candidates",
	);
	plugin_surface_tests::assert_contains(&research_surface, "OKF disposition");
	plugin_surface_tests::assert_contains(&research_surface, "promote_and_supersede");
	plugin_surface_tests::assert_contains(&research_surface, "promote_and_retire");
	plugin_surface_tests::assert_contains(&research_surface, "reject_or_deprecate");
	plugin_surface_tests::assert_contains(&research_surface, "docs/decisions");
	plugin_surface_tests::assert_contains(&research_surface, "docs/evidence");
	plugin_surface_tests::assert_contains(&research_surface, "Use `no_promotion` only when");
	plugin_surface_tests::assert_contains(&research_surface, "route execution to `planning`");
	plugin_surface_tests::assert_contains(&research_surface, "missing evidence");
	plugin_surface_tests::assert_contains(&research_surface, "premature success claims");
	plugin_surface_tests::assert_contains(
		&research_surface,
		"bounded read-only evidence gathering",
	);
	plugin_surface_tests::assert_contains(
		&research_surface,
		"Pressure-test the shape of the work before execution",
	);
}

#[test]
fn packaged_decodex_skills_route_review_evidence_and_handoff_recovery_to_runtime_authority() {
	let ops_surface = format!("{DECODEX_OPS_SKILL}\n{ROUTING_REF}");
	let research_surface = format!("{RESEARCH_SKILL}\n{ROUTING_REF}");
	let land_surface = format!("{LAND_SKILL}\n{ROUTING_REF}");

	plugin_surface_tests::assert_contains(&ops_surface, "missing_review_handoff_record");
	plugin_surface_tests::assert_contains(
		&ops_surface,
		"`decodex recover review-handoff diagnose <ISSUE> --json`",
	);
	plugin_surface_tests::assert_contains(
		&ops_surface,
		"`decodex recover review-handoff rebind <ISSUE> --pr <URL> --dry-run`",
	);
	plugin_surface_tests::assert_contains(
		&ops_surface,
		"`decodex recover review-handoff adopt <ISSUE> --pr <URL> --dry-run`",
	);
	plugin_surface_tests::assert_contains(&ops_surface, "Decodex-owned retained lane PR");
	plugin_surface_tests::assert_contains(&ops_surface, "human-owned PR takeover");
	plugin_surface_tests::assert_contains(&ops_surface, "Do not infer PR lineage");
	plugin_surface_tests::assert_contains(
		&research_surface,
		"research compact loop is not runtime `compact_current_head_review`",
	);
	plugin_surface_tests::assert_contains(&research_surface, "issue_review_checkpoint");
	plugin_surface_tests::assert_contains(&research_surface, "`review_cost_control`");
	plugin_surface_tests::assert_contains(&research_surface, "`decodex evidence`");
	plugin_surface_tests::assert_contains_normalized(
		&research_surface,
		"not a skipped-review signal",
	);
	plugin_surface_tests::assert_contains(
		&land_surface,
		"`decodex land --authority <ISSUE> --pr <URL> \"<summary>\"`",
	);
	plugin_surface_tests::assert_contains(
		&land_surface,
		"Only `decodex land` lands a Decodex-owned PR",
	);
	plugin_surface_tests::assert_contains_normalized(
		&land_surface,
		"Rebind restores or refreshes a Decodex-owned retained lane",
	);
	plugin_surface_tests::assert_contains_normalized(
		&land_surface,
		"adopt is for a human-owned PR takeover",
	);
	plugin_surface_tests::assert_contains(&land_surface, "do not land the PR");
	plugin_surface_tests::assert_contains(&land_surface, "Do not use global `AGENTS.md`");
}
