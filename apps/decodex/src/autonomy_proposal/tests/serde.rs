use crate::autonomy_proposal::{AutonomyProposalChallengeSource, AutonomyProposalIssueCandidate};

#[test]
fn autonomy_proposal_challenge_source_accepts_legacy_support_agent_alias() {
	let source: AutonomyProposalChallengeSource =
		serde_json::from_value(serde_json::json!("subagent"))
			.expect("canonical subagent source should parse");
	let legacy_source: AutonomyProposalChallengeSource =
		serde_json::from_value(serde_json::json!("support_agent"))
			.expect("legacy support_agent source should parse");

	assert_eq!(source, AutonomyProposalChallengeSource::Subagent);
	assert!(
		legacy_source == AutonomyProposalChallengeSource::Subagent,
		"legacy support_agent should canonicalize to Subagent"
	);
	assert_eq!(
		serde_json::to_value(legacy_source).expect("source should serialize"),
		serde_json::json!("subagent")
	);
}

#[test]
fn autonomy_proposal_issue_candidate_accepts_mcp_camel_case_fields() {
	let candidate: AutonomyProposalIssueCandidate = serde_json::from_value(serde_json::json!({
		"key": "evaluation-gate",
		"title": "Evaluate the proposed split.",
		"objective": "Prove the proposal split is useful before execution.",
		"stage": "eval",
		"dependencies": ["readback-contract"],
		"conflictDomains": ["module:autonomy"],
		"acceptance": ["Evaluation result is recorded."],
		"validation": ["cargo test -p decodex autonomy_proposal --lib"],
		"risk": ["False positives remain visible."],
		"queueIntent": "ready_to_queue"
	}))
	.expect("MCP-shaped issue candidate should parse");

	assert_eq!(candidate.conflict_domains, [String::from("module:autonomy")]);
	assert_eq!(candidate.queue_intent, "ready_to_queue");
}
