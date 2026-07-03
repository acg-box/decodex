mod program_intake_dogfood {
	mod goal_intake;
	mod issue_batch;
	mod live_status;
	mod support;

	pub(super) use crate::orchestrator::tests::temp_project_layout;
}
