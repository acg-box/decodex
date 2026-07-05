use crate::{
	config::ReviewLevel,
	orchestrator::{
		ISSUE_REVIEW_CHECKPOINT_TOOL_NAME,
		tests::{self, intake_run_and_prompting},
	},
};

#[test]
fn normal_prompts_respect_non_standard_review_levels() {
	for (mode, expected, forbidden_checkpoint) in [
		(ReviewLevel::Off, "[codex].review = \"off\"", None),
		(
			ReviewLevel::Basic,
			"Self Check: Review your work repeatedly and fix any logic bugs until no new issues are found.",
			Some(ISSUE_REVIEW_CHECKPOINT_TOOL_NAME),
		),
	] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let config = tests::service_config_with_review_level(&config, mode);
		let prompts = intake_run_and_prompting::build_normal_prompt_surfaces(&config, &workflow);

		for prompt in prompts.all() {
			assert!(prompt.contains(expected), "{mode:?} prompt should contain `{expected}`");
			assert!(!prompt.contains("Follow the repo-native bounded review method"));

			if let Some(forbidden_checkpoint) = forbidden_checkpoint {
				assert!(!prompt.contains(forbidden_checkpoint));
			}

			assert!(!prompt.contains("only after the latest `issue_review_checkpoint`"));
		}

		assert!(
			prompts
				.developer_instructions
				.contains("Call `issue_review_handoff` after the branch is pushed")
		);
		assert!(prompts.user_input.contains("required validation has passed"));
		assert!(prompts.continuation_input.contains("after required validation has passed"));
	}
}
