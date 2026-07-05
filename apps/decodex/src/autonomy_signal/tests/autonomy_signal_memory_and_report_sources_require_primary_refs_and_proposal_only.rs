use crate::autonomy_signal::{AutonomySignal, AutonomySignalSourceType, tests};

#[test]
fn autonomy_signal_memory_and_report_sources_require_primary_refs_and_proposal_only() {
	for source_type in [AutonomySignalSourceType::Memory, AutonomySignalSourceType::Report] {
		let mut input = tests::signal_input();

		input.source_type = source_type;
		input.source_refs = vec![String::from("memory:summary:older-context")];
		input.primary_source_refs = Vec::new();
		input.proposal_only = false;

		assert!(AutonomySignal::docs_skill_drift(input.clone()).is_err());

		input.primary_source_refs = vec![String::from("docs/spec/runtime.md")];
		input.proposal_only = true;

		AutonomySignal::docs_skill_drift(input)
			.expect("memory/report signals with primary refs remain proposal-only");
	}
}
