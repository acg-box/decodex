use crate::orchestrator::{
	self,
	tests::{self},
};

#[test]
fn no_eligible_issue_message_includes_operator_hint() {
	let (_temp_dir, config, workflow) = tests::temp_project_layout();
	let message = orchestrator::format_no_eligible_issue_message(&config, &workflow);

	assert!(message.contains("No eligible issue found for the configured project."));
	assert!(message.contains("`Todo`"));
	assert!(message.contains("`decodex:queued:<service-id>`"));
	assert!(message.contains("`decodex:queued:pubfi`"));
	assert!(message.contains("`decodex:manual-only`/`decodex:needs-attention`"));
	assert!(message.contains("non-terminal state"));
	assert!(message.contains("dependency blockers"));
	assert!(message.contains("no active issue claim"));
	assert!(message.contains("Program Intake"));
	assert!(message.contains("decodex status --live"));
	assert!(message.contains("decodex intake issues --project pubfi --apply <ISSUE>"));
}
