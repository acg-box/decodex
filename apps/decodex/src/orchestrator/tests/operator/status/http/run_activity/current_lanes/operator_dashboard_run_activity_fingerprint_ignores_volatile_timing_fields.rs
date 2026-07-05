use crate::orchestrator::tests::operator::status::http::orchestrator;

#[test]
fn operator_dashboard_run_activity_fingerprint_ignores_volatile_timing_fields() {
	let mut first = serde_json::json!({
		"accountControl": {
			"mode": "balanced",
			"account_selector": null,
		},
		"accounts": [],
		"currentLanes": [
			{
				"run_id": "run-1",
				"status": "running",
				"phase": "executing",
				"idle_for_seconds": 4,
				"protocol_idle_for_seconds": 3,
				"child_agent_activity": {
					"current_bucket": "model",
					"current_elapsed_seconds": 2,
					"buckets": [
						{
							"bucket": "model",
							"wall_seconds": 2,
							"event_count": 7,
						},
					],
				},
			},
		],
		"currentLanesComplete": true,
		"currentLaneScope": "complete",
	});
	let mut second = serde_json::json!({
		"accountControl": {
			"mode": "balanced",
			"account_selector": null,
		},
		"accounts": [],
		"currentLanes": [
			{
				"run_id": "run-1",
				"status": "running",
				"phase": "executing",
				"idle_for_seconds": 5,
				"protocol_idle_for_seconds": 4,
				"child_agent_activity": {
					"current_bucket": "model",
					"current_elapsed_seconds": 3,
					"buckets": [
						{
							"bucket": "model",
							"wall_seconds": 3,
							"event_count": 7,
						},
					],
				},
			},
		],
		"currentLanesComplete": true,
		"currentLaneScope": "complete",
	});

	orchestrator::strip_dashboard_run_activity_volatile_fields(&mut first);
	orchestrator::strip_dashboard_run_activity_volatile_fields(&mut second);

	assert_eq!(first, second);
	assert_eq!(first["currentLanes"][0]["run_id"], "run-1");
	assert_eq!(first["currentLanes"][0]["child_agent_activity"]["buckets"][0]["event_count"], 7);
	assert!(first["currentLanes"][0].get("idle_for_seconds").is_none());
	assert!(
		first["currentLanes"][0]["child_agent_activity"].get("current_elapsed_seconds").is_none()
	);
	assert!(
		first["currentLanes"][0]["child_agent_activity"]["buckets"][0]
			.get("wall_seconds")
			.is_none()
	);
}
