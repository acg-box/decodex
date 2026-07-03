use crate::plugin_surface_tests::{
	self, CODEBASE_REF, CODEBASE_WORK_SKILL, COMMIT_AGENT_POLICY, DECODEX_OPS_AGENT_POLICY,
	DELIBERATION_GRILL_SKILL, DELIBERATION_SCOUT_SKILL, DELIBERATION_SKEPTIC_SKILL,
	KNOWLEDGE_DOCS_DRIFT_SKILL, KNOWLEDGE_DOCS_SKILL, KNOWLEDGE_OKF_SKILL,
	KNOWLEDGE_REPO_MEMORY_SKILL, KNOWLEDGE_WRITEBACK_SKILL, LAND_AGENT_POLICY,
	PLANNING_AGENT_POLICY, REPO_DEBUGGING_SKILL, REPO_DEPENDENCY_POLICY_REF,
	REPO_DEPENDENCY_POLICY_SKILL, REPO_REVIEW_FEEDBACK_SKILL, REPO_VERIFICATION_SKILL,
	RESEARCH_AGENT_POLICY, RESEARCH_PROMOTE_AGENT_POLICY,
};

#[test]
fn packaged_codebase_skills_preserve_command_and_verification_authority() {
	let repo_surface = format!(
		"{CODEBASE_WORK_SKILL}\n{REPO_DEBUGGING_SKILL}\n{REPO_VERIFICATION_SKILL}\n{REPO_REVIEW_FEEDBACK_SKILL}\n{REPO_DEPENDENCY_POLICY_SKILL}\n{CODEBASE_REF}\n{REPO_DEPENDENCY_POLICY_REF}"
	);

	plugin_surface_tests::assert_contains(&repo_surface, "Checked-in command authority used");
	plugin_surface_tests::assert_contains(&repo_surface, "Task Runner Structure");
	plugin_surface_tests::assert_contains(&repo_surface, "Prefer repo-native commands");
	plugin_surface_tests::assert_contains(&repo_surface, "Makefile.toml");
	plugin_surface_tests::assert_contains(&repo_surface, "Root-cause investigation");
	plugin_surface_tests::assert_contains_normalized(
		&repo_surface,
		"symptom -> owner boundary -> fresh baseline",
	);
	plugin_surface_tests::assert_contains(&repo_surface, "Every positive claim must have evidence");
	plugin_surface_tests::assert_contains(&repo_surface, "Implemented, not fully verified");
	plugin_surface_tests::assert_contains(&repo_surface, "verified_actionable");
	plugin_surface_tests::assert_contains(
		&repo_surface,
		"Run `plugin-eval analyze <plugin-root> --format markdown`",
	);
	plugin_surface_tests::assert_contains(&repo_surface, "Task-runner review checklist");
	plugin_surface_tests::assert_contains(&repo_surface, "External `uses: owner/action@ref`");
	plugin_surface_tests::assert_contains(&repo_surface, "whole discoverable dependency surface");
	plugin_surface_tests::assert_contains(
		&repo_surface,
		"open Dependabot PRs are authoritative candidates",
	);
	plugin_surface_tests::assert_contains(&repo_surface, "residual dependency checks");
	plugin_surface_tests::assert_contains(&repo_surface, "requires-follow-up-migration");
	plugin_surface_tests::assert_contains(
		&repo_surface,
		"Do not duplicate this routing in host bootstrap files",
	);
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
		plugin_surface_tests::assert_contains(policy, "allow_implicit_invocation: false");
	}
}

#[test]
fn codebase_knowledge_and_deliberation_skills_allow_implicit_routing() {
	let implicit_surface = format!(
		"{CODEBASE_WORK_SKILL}\n{REPO_DEBUGGING_SKILL}\n{REPO_VERIFICATION_SKILL}\n{REPO_REVIEW_FEEDBACK_SKILL}\n{REPO_DEPENDENCY_POLICY_SKILL}\n{KNOWLEDGE_DOCS_SKILL}\n{KNOWLEDGE_DOCS_DRIFT_SKILL}\n{KNOWLEDGE_OKF_SKILL}\n{KNOWLEDGE_REPO_MEMORY_SKILL}\n{KNOWLEDGE_WRITEBACK_SKILL}\n{DELIBERATION_SKEPTIC_SKILL}\n{DELIBERATION_SCOUT_SKILL}\n{DELIBERATION_GRILL_SKILL}"
	);

	plugin_surface_tests::assert_not_contains(
		&implicit_surface,
		"allow_implicit_invocation: false",
	);
	plugin_surface_tests::assert_contains(
		&implicit_surface,
		"description: Use when repository code work",
	);
	plugin_surface_tests::assert_contains(
		&implicit_surface,
		"description: Use when repository docs",
	);
	plugin_surface_tests::assert_contains(
		&implicit_surface,
		"description: Use when a task needs bounded read-only evidence gathering",
	);
	plugin_surface_tests::assert_contains(
		&implicit_surface,
		"description: Use when unclear intent",
	);
}
