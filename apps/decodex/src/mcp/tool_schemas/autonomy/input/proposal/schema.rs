use serde_json::{self, Value};

pub(in crate::mcp::tool_schemas::autonomy::input::proposal) fn autonomy_compile_proposal_payload_schema()
-> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"objectiveId": {
				"type": "string",
				"description": "Accepted Objective Contract id."
			},
			"objectiveVersion": {
				"type": "integer",
				"minimum": 1,
				"description": "Accepted Objective Contract version."
			},
			"sourceFamily": {
				"type": "string",
				"description": "Signal family that motivated the proposal."
			},
			"intendedSurface": {
				"type": "string",
				"description": "Repo, docs, runtime, or workflow surface the proposal may affect."
			},
			"affectedIdentifiers": {
				"type": "array",
				"items": { "type": "string" },
				"description": "Optional issue, module, command, or artifact identifiers affected by the proposal."
			},
			"summary": {
				"type": "string",
				"description": "Operator-readable proposal summary."
			},
			"challengeRequirements": {
				"type": "array",
				"items": { "type": "string" },
				"description": "Independent review or challenge evidence required before promotion."
			},
			"rejectedAlternatives": {
				"type": "array",
				"items": { "type": "string" },
				"description": "Alternatives considered and rejected."
			},
			"rollbackPath": {
				"type": "string",
				"description": "How to revert or abandon the proposal safely."
			},
			"weakenedValidationOrReview": {
				"type": "array",
				"items": { "type": "string" },
				"description": "Validation or review gates that would be weakened; non-empty values refuse the proposal."
			},
			"issueCandidates": {
				"type": "array",
				"items": autonomy_issue_candidate_schema(),
				"description": "Optional explicit issue DAG. Dependencies refer to candidate keys and must be known and acyclic."
			},
			"createdAt": {
				"type": "string",
				"description": "Optional RFC3339 proposal timestamp. Defaults to MCP runtime time."
			}
		},
		"required": [
			"objectiveId",
			"objectiveVersion",
			"sourceFamily",
			"intendedSurface",
			"summary",
			"rollbackPath"
		]
	})
}

fn autonomy_issue_candidate_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"key": {
				"type": "string",
				"description": "Stable candidate key used by dependencies."
			},
			"title": {
				"type": "string",
				"description": "Issue title."
			},
			"objective": {
				"type": "string",
				"description": "Concrete sub-goal for this candidate issue."
			},
			"stage": {
				"type": "string",
				"enum": ["design", "spec", "schema", "runtime", "plugin", "eval", "handoff"],
				"description": "Execution stage for the candidate."
			},
			"dependencies": {
				"type": "array",
				"items": { "type": "string" },
				"description": "Candidate keys that must complete before this candidate."
			},
			"conflictDomains": {
				"type": "array",
				"items": { "type": "string" },
				"description": "Surfaces that should not run concurrently with conflicting work."
			},
			"acceptance": {
				"type": "array",
				"items": { "type": "string" },
				"minItems": 1,
				"description": "Acceptance criteria for the candidate issue."
			},
			"validation": {
				"type": "array",
				"items": { "type": "string" },
				"minItems": 1,
				"description": "Validation expected before completion."
			},
			"risk": {
				"type": "array",
				"items": { "type": "string" },
				"description": "Known risks or stop conditions."
			},
			"queueIntent": {
				"type": "string",
				"enum": ["not_ready", "ready_to_queue", "queued", "active", "paused", "done", "canceled"],
				"description": "Whether this candidate can enter normal intake after explicit acceptance."
			}
		},
		"required": [
			"key",
			"title",
			"objective",
			"stage",
			"acceptance",
			"validation",
			"queueIntent"
		]
	})
}
