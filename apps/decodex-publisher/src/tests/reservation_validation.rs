use std::fs;

use crate::tests::support::{self};

#[test]
fn validates_social_reservation_and_rejects_bad_timestamp() {
	let mut reservation = support::valid_social_publish_reservation();

	support::assert_social_errors(&reservation, []);

	reservation["reserved_at"] = serde_json::json!("not-a-date");

	support::assert_social_errors(&reservation, ["reserved_at must be an RFC3339 timestamp"]);
}

#[test]
fn rejects_duplicate_active_social_publish_reservation_idempotency_keys() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let first = temp_dir.path().join("reservations/one.json");
	let second = temp_dir.path().join("reservations/two.json");

	fs::create_dir_all(first.parent().expect("fixture should have parent"))
		.expect("fixture directory should be created");
	fs::write(&first, support::valid_social_publish_reservation().to_string())
		.expect("fixture should be written");
	fs::write(&second, support::valid_social_publish_reservation().to_string())
		.expect("fixture should be written");

	let error = crate::validate_social(&[temp_dir.path().to_path_buf()])
		.expect_err("duplicate active reservations should be rejected")
		.to_string();

	assert!(error.contains("duplicate active social_publish_reservation"));
}

#[test]
fn social_reserve_publish_writes_active_reservation_once() {
	let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
	let request = support::social_reserve_request(temp_dir.path(), false);
	let report = crate::reserve_social_publish(&request).expect("reservation should pass");

	assert_eq!(report.status, "reserved");
	assert!(
		temp_dir.path().join("reservations/2026-06-02/openai-codex-pr-22414.json").exists(),
		"reservation should be written"
	);

	let duplicate = crate::reserve_social_publish(&request)
		.expect_err("duplicate reservation should fail closed")
		.to_string();

	assert!(duplicate.contains("idempotency_key already has an active reservation"));
}
