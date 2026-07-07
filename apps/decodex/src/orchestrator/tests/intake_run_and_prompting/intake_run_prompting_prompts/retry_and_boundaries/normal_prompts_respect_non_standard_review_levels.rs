use crate::{
	config::ReviewLevel,
	orchestrator::tests::{self, intake_run_and_prompting},
};

#[test]
fn normal_prompts_respect_non_standard_review_levels() {
	for (mode, expected) in [(ReviewLevel::Off, "[codex].review = \"off\"")] {
		let (_temp_dir, config, workflow) = tests::temp_project_layout();
		let config = tests::service_config_with_review_level(&config, mode);
		let prompts = intake_run_and_prompting::build_normal_prompt_surfaces(&config, &workflow);

		for prompt in prompts.all() {
			assert!(prompt.contains(expected), "{mode:?} prompt should contain `{expected}`");
			assert!(!prompt.contains("Follow the repo-native bounded review method"));
			assert!(!prompt.contains("Decodex Review: request"));
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
