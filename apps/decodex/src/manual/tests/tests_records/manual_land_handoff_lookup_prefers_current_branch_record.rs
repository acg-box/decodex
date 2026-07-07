#[rustfmt::skip]
use crate::manual::{self, tests};
use crate::state::{ReviewLifecycleHandoffFixture, StateStore};

#[test]
fn manual_land_handoff_lookup_prefers_current_branch_record() {
	let issue = tests::sample_issue("issue-1", "XY-225", true, &["decodex:active:pubfi"]);
	let state_store = StateStore::open_in_memory().expect("state store should open");

	state_store
		.upsert_review_lifecycle_handoff_fixture(
			"decodex",
			&issue.id,
			&ReviewLifecycleHandoffFixture::new(
				String::from("run-current"),
				2,
				String::from("xy-225"),
				String::from("https://github.com/hack-ink/decodex/pull/67"),
				String::from("main"),
				String::from("xy-225"),
				String::from("deadbeef"),
			),
		)
		.expect("runtime handoff should persist");
	state_store
		.upsert_review_lifecycle_handoff_fixture(
			"decodex",
			&issue.id,
			&ReviewLifecycleHandoffFixture::new(
				String::from("run-other"),
				3,
				String::from("xy-225-next"),
				String::from("https://github.com/hack-ink/decodex/pull/99"),
				String::from("main"),
				String::from("xy-225-next"),
				String::from("cafebabe"),
			),
		)
		.expect("unrelated runtime handoff should persist");

	let lifecycle_record =
		manual::read_manual_land_lifecycle(&state_store, "decodex", &issue.id, "xy-225")
			.expect("manual land lifecycle should read")
			.expect("current branch lifecycle should be found");

	assert_eq!(lifecycle_record.branch_name(), "xy-225");
	assert_eq!(lifecycle_record.pr_url(), "https://github.com/hack-ink/decodex/pull/67");
}
