use crate::orchestrator::tests::runtime_loop_scenarios::loop_scenarios::support::LoopScenarioHarness;

#[test]
fn research_to_execution_loop_scenario_shapes_ready_work_and_records_feedback() {
	let harness = LoopScenarioHarness::new();
	let contract = harness.assert_latent_research_stays_non_executable();
	let (contract, _policy, evaluation) = harness.promote_and_evaluate_program(contract);

	harness.assert_direct_dispatch_shaping(&evaluation);
	harness.record_review_guardrail_and_assert_harness_feedback(contract);
}
