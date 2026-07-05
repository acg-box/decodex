use crate::orchestrator::tests::operator::status::{eyre, orchestrator};

#[test]
fn runtime_recovery_warning_keeps_safe_error_class() {
	assert_eq!(
		orchestrator::runtime_recovery_warning(
			"runtime_recovery_unavailable",
			&eyre::eyre!("Linear tracker request failed"),
		),
		"runtime_recovery_unavailable:tracker"
	);
	assert_eq!(
		orchestrator::runtime_recovery_warning(
			"runtime_recovery_unavailable",
			&eyre::eyre!("worktree scan failed"),
		),
		"runtime_recovery_unavailable:worktree"
	);
	assert_eq!(
		orchestrator::runtime_recovery_warning(
			"runtime_recovery_unavailable",
			&eyre::eyre!("sqlite runtime store locked"),
		),
		"runtime_recovery_unavailable:runtime_store"
	);
}
