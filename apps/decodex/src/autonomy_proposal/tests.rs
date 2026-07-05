mod bridge;
mod bridge_fixtures;
mod challenge;
mod compile;
mod compile_fixtures;
mod objective_fixtures;
mod persistence;
mod policy_fixtures;
mod serde;
mod signal_fixtures;

pub(crate) use self::{
	bridge_fixtures::{
		assert_autonomy_candidate_shape, bridge_authority, store_challenged_autonomy_candidate,
	},
	compile_fixtures::{compile_input, issue_candidate},
	objective_fixtures::{objective_fixture, store_accepted_objective},
	policy_fixtures::{
		accepted_project_policy, accepted_project_policy_fixture, decision_bridge_authority_input,
		runtime_policy_bridge_authority,
	},
	signal_fixtures::{runtime_signal, signal_input},
};

trait ExpectNone {
	fn expect_none(self, message: &str);
}

impl<T> ExpectNone for Option<T> {
	fn expect_none(self, message: &str) {
		assert!(self.is_none(), "{message}");
	}
}
