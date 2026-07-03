use crate::{
	orchestrator,
	orchestrator::tests::operator::status::running_lanes::{
		self,
		autonomy_lineage::{assertions, fixtures},
	},
	state::StateStore,
};

#[test]
fn operator_status_surfaces_autonomy_lineage_without_raw_payloads() {
	let (_temp_dir, config, workflow) = running_lanes::temp_project_layout();
	let state_store = StateStore::open_in_memory().expect("state store should open");
	let issue = running_lanes::sample_issue("Todo", &[]);
	let seeded = fixtures::seed_autonomy_lineage(&state_store, &config, &workflow, &issue);
	let snapshot = orchestrator::build_operator_status_snapshot(&config, &state_store, 10)
		.expect("snapshot should build");

	assertions::assert_autonomy_readback(&snapshot, &seeded);
}
