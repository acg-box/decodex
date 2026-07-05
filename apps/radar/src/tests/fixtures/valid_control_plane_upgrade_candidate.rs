use serde_json::Value;

pub(crate) fn valid_control_plane_upgrade_candidate() -> Value {
	serde_json::json!({
		"schema": "control_plane_upgrade_candidate/v1",
		"slug": "openai-codex-pr-22414-control-plane",
		"repo": "openai/codex",
		"status": "proposed",
		"source_refs": {
			"upstream_reviews": [
				".agent/automations/radar/cache/github/reviews/openai-codex-pr-22414.review.json"
			],
			"upstream_impacts": [
				".agent/automations/radar/cache/github/impact/openai-codex-pr-22414.json"
			],
			"urls": ["https://github.com/openai/codex/pull/22414"]
		},
		"observed_change": "Remote Codex can use Unix socket endpoints.",
		"control_plane_impact": "compat_risk",
		"upgrade_path": "compat_risk_mitigation",
		"affected_surfaces": ["app-server protocol"],
		"target_codex": {
			"channel": "stable",
			"version": "0.142.2",
			"tag": "rust-v0.142.2",
			"release_url": "https://github.com/openai/codex/releases/tag/rust-v0.142.2",
			"compatibility_status": "needs_review",
			"matrix_ref": "docs/reference/codex-compatibility-matrix.md#codex-01422"
		},
		"authority": {
			"decision_contract_required": true,
			"program_intake_required": true,
			"mutation_allowed": false,
			"objective_id": "decodex-self-iteration"
		},
		"reason": "The upstream app-server transport change may affect Decodex Control Plane compatibility.",
		"validation_gates": ["decodex probe stdio://", "cargo test -p decodex app_server --lib"],
		"stop_conditions": ["Missing accepted Decision Contract", "Probe failure against the target Codex build"],
		"acceptance_criteria": [
			"Compatibility impact is proven or dismissed with source-backed evidence."
		]
	})
}
