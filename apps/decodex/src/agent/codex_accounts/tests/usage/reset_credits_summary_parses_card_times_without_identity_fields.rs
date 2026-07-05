use crate::agent::codex_accounts;

#[test]
fn reset_credits_summary_parses_card_times_without_identity_fields() {
	let payload = serde_json::json!({
		"available_count": 2,
		"total_earned_count": 7,
		"credits": [
			{
				"id": "reset-credit-unique-id",
				"profile_user_id": "user-unique-id",
				"profile_image_url": "https://example.invalid/avatar.png",
				"granted_at": "2026-06-25T05:51:30.432909Z",
				"expires_at": "2026-07-25T05:51:30.432909Z",
				"redeem_started_at": null,
				"redeemed_at": null,
				"status": "available",
				"title": "Reset",
				"description": "Reset",
				"reset_type": "codex"
			},
			{
				"id": "reset-credit-second-id",
				"granted_at": "2026-06-26T01:00:00Z",
				"expires_at": "2026-07-26T01:00:00Z",
				"status": "available"
			}
		]
	});
	let summary = codex_accounts::reset_credits_snapshot_from_payload(&payload, 1_800_000_000);

	assert_eq!(summary.available_count, Some(2));
	assert_eq!(summary.total_earned_count, Some(7));
	assert_eq!(summary.checked_at_unix_epoch, 1_800_000_000);
	assert_eq!(
		summary.credits,
		vec![
			codex_accounts::ResetCreditSummary {
				granted_at_unix_epoch: Some(1_782_366_690),
				expires_at_unix_epoch: Some(1_784_958_690),
				status: Some(String::from("available")),
			},
			codex_accounts::ResetCreditSummary {
				granted_at_unix_epoch: Some(1_782_435_600),
				expires_at_unix_epoch: Some(1_785_027_600),
				status: Some(String::from("available")),
			},
		]
	);
}
