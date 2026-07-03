use crate::{
	orchestrator,
	orchestrator::tests::runtime_program_intake_dogfood::program_intake_dogfood::support::{
		self, DogfoodTracker,
	},
	program_intake::{self},
	state::StateStore,
};

#[test]
fn live_program_status_refreshes_terminal_label_clear_without_scheduler_pass() {
	let (_temp_dir, config, workflow) = super::temp_project_layout();
	let store = StateStore::open_in_memory().expect("state store should open");
	let stale_attention_issue = support::dogfood_issue(
		config.service_id(),
		"issue-pub-1597",
		"PUB-1597",
		"Todo",
		&["decodex:needs-attention"],
	);
	let tracker = DogfoodTracker::default().with_issues([stale_attention_issue.clone()]);
	let apply_report = program_intake::run_issue_batch_intake(
		&store,
		&tracker,
		&config,
		&workflow,
		vec![String::from("PUB-1597")],
		false,
		true,
	)
	.expect("issue-batch apply should persist the stale mapped issue facts");

	assert_eq!(apply_report.counts.blocked, 1);

	let mut completed_issue =
		support::dogfood_issue(config.service_id(), "issue-pub-1597", "PUB-1597", "Done", &[]);

	completed_issue.id.clone_from(&stale_attention_issue.id);
	tracker.upsert_issue(completed_issue);

	let snapshot =
		orchestrator::build_live_operator_status_snapshot(&tracker, &config, &workflow, &store, 10)
			.expect("live status should refresh mapped issue facts");
	let program = snapshot.execution_programs.first().expect("program status should surface");
	let rendered_status = orchestrator::render_operator_status(&snapshot);

	assert_eq!(program.program_id, apply_report.program_id);
	assert_eq!(program.status, "completed", "{rendered_status}");
	assert_eq!(program.completed_count, 1);
	assert_eq!(program.needs_attention_count, 0);
	assert_eq!(program.blocked_count, 0);
	assert!(rendered_status.contains("status=completed"), "{rendered_status}");
	assert!(rendered_status.contains("attention=0"), "{rendered_status}");
	assert!(rendered_status.contains("completed=1"), "{rendered_status}");
	assert!(rendered_status.contains("mapped_issues=PUB-1597"), "{rendered_status}");
	assert!(!rendered_status.contains("issue=PUB-1597 issue_state=Todo"));
	assert!(!rendered_status.contains("decodex:needs-attention"));

	let refreshed_program = store
		.list_execution_programs(config.service_id())
		.expect("programs should load")
		.into_iter()
		.find(|record| record.program_id() == apply_report.program_id)
		.expect("refreshed program should remain persisted");
	let refreshed_issue = refreshed_program
		.program()
		.nodes()
		.first()
		.and_then(|node| node.linear_issue())
		.expect("refreshed node should retain its issue mapping");

	assert_eq!(refreshed_issue.issue_state(), "Done");
	assert!(!refreshed_issue.has_needs_attention_label());
}
