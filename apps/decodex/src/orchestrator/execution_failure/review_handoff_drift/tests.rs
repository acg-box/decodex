use crate::orchestrator::{
	execution_failure::review_handoff_drift::command,
	kernel::command::{CommandFact, CommandIntentKind},
};

#[test]
fn drift_recovery_lifecycle_authority_rebind_intent_preserves_kernel_contract() {
	let intent = command::rebind_lifecycle_authority_command_intent("PUB-101", "run-1");

	assert_eq!(intent.kind, CommandIntentKind::SyncReviewLifecycleAuthority);
	assert_eq!(
		intent.idempotency_key,
		"PUB-101:run-1:sync_review_lifecycle_authority:review_handoff_state_drift_lifecycle_authority_rebound",
	);
	assert!(intent.preconditions.contains(&CommandFact::PostReviewLifecyclePresent));
	assert_eq!(intent.expected_postconditions, vec![CommandFact::ReviewLifecycleAuthorityCurrent],);

	command::review_handoff_drift_command_adapter(
		intent,
		CommandIntentKind::SyncReviewLifecycleAuthority,
	)
	.expect("kernel-built drift recovery lifecycle authority sync intent should pass adapter");
}
