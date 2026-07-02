use crate::orchestrator::{
	execution_failure::review_handoff_drift::command,
	kernel::command::{CommandFact, CommandIntentKind},
};

#[test]
fn drift_recovery_marker_rebind_intent_preserves_kernel_contract() {
	let intent = command::review_handoff_drift_marker_rebind_command_intent("PUB-101", "run-1");

	assert_eq!(intent.kind, CommandIntentKind::SyncReviewOrchestrationMarker);
	assert_eq!(
		intent.idempotency_key,
		"PUB-101:run-1:sync_review_orchestration_marker:review_handoff_state_drift_orchestration_rebound",
	);
	assert!(intent.preconditions.contains(&CommandFact::PostReviewLifecyclePresent));
	assert_eq!(intent.expected_postconditions, vec![CommandFact::ReviewOrchestrationMarkerCurrent],);

	command::review_handoff_drift_command_adapter(
		intent,
		CommandIntentKind::SyncReviewOrchestrationMarker,
	)
	.expect("kernel-built drift recovery marker sync intent should pass adapter");
}
