use crate::plugin_surface_tests::{
	self, COMMIT_AGENT_POLICY, DECODEX_OPS_AGENT_POLICY, DECODEX_OPS_SKILL, DECODEX_SKILL,
	LAND_AGENT_POLICY, LAND_SKILL, PLANNING_AGENT_POLICY, PLANNING_SKILL, ROUTING_REF,
};

#[test]
fn packaged_decodex_skills_are_runtime_operator_only() {
	let skill_surface = format!(
		"{DECODEX_SKILL}\n{DECODEX_OPS_SKILL}\n{PLANNING_SKILL}\n{LAND_SKILL}\n{ROUTING_REF}"
	);

	plugin_surface_tests::assert_contains(&skill_surface, "runtime ops");
	plugin_surface_tests::assert_contains(&skill_surface, "lane");
	plugin_surface_tests::assert_contains(&skill_surface, "Program Intake");
	plugin_surface_tests::assert_contains(&skill_surface, "decodex land");
	plugin_surface_tests::assert_contains(&skill_surface, "review-handoff diagnose");
	plugin_surface_tests::assert_contains(&skill_surface, "review-handoff rebind");
	plugin_surface_tests::assert_contains(&skill_surface, "review-handoff adopt");
	plugin_surface_tests::assert_contains(&skill_surface, "superseded-closeout");
	plugin_surface_tests::assert_contains(&skill_surface, "Only `decodex land` lands");
	plugin_surface_tests::assert_contains(&skill_surface, "Do not bypass Decodex authority");
	plugin_surface_tests::assert_contains(&skill_surface, "raw Git");
	plugin_surface_tests::assert_contains(&skill_surface, "GitHub UI");
	plugin_surface_tests::assert_contains(&skill_surface, "gh pr merge");
	plugin_surface_tests::assert_contains(&skill_surface, "external installed `codebase`");
	plugin_surface_tests::assert_not_contains(&skill_surface, "research-promote");
	plugin_surface_tests::assert_not_contains(&skill_surface, "deliberation:");
	plugin_surface_tests::assert_not_contains(&skill_surface, "$knowledge:docs");
}

#[test]
fn decodex_lifecycle_specialist_skills_stay_explicit_only() {
	for policy in
		[COMMIT_AGENT_POLICY, DECODEX_OPS_AGENT_POLICY, LAND_AGENT_POLICY, PLANNING_AGENT_POLICY]
	{
		plugin_surface_tests::assert_contains(policy, "allow_implicit_invocation: false");
	}
}
