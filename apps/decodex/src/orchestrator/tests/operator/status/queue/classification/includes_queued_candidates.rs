use crate::orchestrator::tests::operator::status::{
	self, FakeTracker, HashMap, StateStore, orchestrator,
};

#[test]
fn includes_queued_candidates() {
	let workflow_markdown =
		status::sample_workflow_markdown("pubfi", &[], "Follow the repository policy.", 1);
	let (_temp_dir, config, workflow) =
		status::temp_project_layout_with_workflow_markdown(&workflow_markdown);
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let ready_issue = status::sample_issue_with_sort_fields(
		"issue-ready",
		"PUB-101",
		"Todo",
		&[],
		Some(1),
		"2026-03-13T04:16:17.133Z",
	);
	let mut blocked_issue = status::sample_issue_with_sort_fields(
		"issue-blocked",
		"PUB-102",
		"Todo",
		&[],
		Some(2),
		"2026-03-13T05:16:17.133Z",
	);

	blocked_issue.description = String::from("```json\n{}\n```");

	let claimed_issue = status::sample_issue_with_sort_fields(
		"issue-claimed",
		"PUB-103",
		"Todo",
		&[],
		Some(3),
		"2026-03-13T06:16:17.133Z",
	);
	let closed_issue = status::sample_issue_with_sort_fields(
		"issue-closed",
		"PUB-104",
		"Done",
		&[],
		Some(4),
		"2026-03-13T07:16:17.133Z",
	);
	let canceled_issue = status::sample_issue_with_sort_fields(
		"issue-canceled",
		"PUB-105",
		"Canceled",
		&[],
		Some(5),
		"2026-03-13T08:16:17.133Z",
	);

	state_store
		.upsert_lease(config.service_id(), &claimed_issue.id, "run-claimed", "In Progress")
		.expect("lease should record");

	let tracker = FakeTracker::new(vec![
		claimed_issue.clone(),
		blocked_issue.clone(),
		closed_issue.clone(),
		canceled_issue.clone(),
		ready_issue.clone(),
	]);
	let snapshot = orchestrator::build_live_operator_status_snapshot(
		&tracker,
		&config,
		&workflow,
		&state_store,
		10,
	)
	.expect("snapshot should build");

	assert_eq!(snapshot.queued_candidates.len(), 3);

	let queued_by_issue = snapshot
		.queued_candidates
		.iter()
		.map(|candidate| (candidate.issue_identifier.as_str(), candidate))
		.collect::<HashMap<_, _>>();

	assert_eq!(
		queued_by_issue.get("PUB-101").expect("ready queued issue should exist").classification,
		"ready"
	);
	assert_eq!(
		queued_by_issue.get("PUB-101").expect("ready queued issue should exist").reason,
		"eligible_for_dispatch"
	);
	assert_eq!(
		queued_by_issue.get("PUB-102").expect("blocked queued issue should exist").classification,
		"blocked"
	);
	assert_eq!(
		queued_by_issue.get("PUB-102").expect("blocked queued issue should exist").reason,
		"missing_dispatch_briefing"
	);
	assert_eq!(
		queued_by_issue.get("PUB-103").expect("claimed queued issue should exist").classification,
		"claimed"
	);
	assert_eq!(
		queued_by_issue.get("PUB-103").expect("claimed queued issue should exist").reason,
		"shared_claim_present"
	);
	assert!(
		!queued_by_issue.contains_key("PUB-104"),
		"terminal queued echoes should not appear in operator intake candidates"
	);
	assert!(
		!queued_by_issue.contains_key("PUB-105"),
		"canceled queued echoes should not appear in operator intake candidates"
	);
}
