use serde_json::Value;

use super::{
	McpCapabilityProfile, McpTool, TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
	TOOL_AUTONOMY_CHALLENGE_PROPOSAL, TOOL_AUTONOMY_COMPILE_PROPOSAL,
	TOOL_AUTONOMY_DRAFT_OBJECTIVE, TOOL_AUTONOMY_REQUEST_PROMOTION, TOOL_AUTONOMY_SUBMIT_SIGNAL,
	TOOL_INTAKE_GOAL, TOOL_LANE_CONTROL, TOOL_OBSERVE, TOOL_PLAN, TOOL_PROJECT_CONTROL,
	TOOL_RESEARCH_COMPILE, TOOL_RESEARCH_PROMOTE,
};

pub(super) fn mcp_tools() -> Vec<McpTool> {
	let mut tools = mcp_foundation_tools();

	tools.extend(mcp_autonomy_tools());
	tools.extend(mcp_operator_tools());

	tools
}

fn mcp_foundation_tools() -> Vec<McpTool> {
	vec![
		mcp_tool_entry(
			McpCapabilityProfile::Observe,
			TOOL_OBSERVE,
			"Decodex Observe",
			"Read public-safe local Decodex runtime observability without private evidence payloads.",
			observe_tool_input_schema(),
			observe_tool_output_schema(),
			true,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_PLAN,
			"Decodex Plan",
			"Return the Decodex prompt/resource route for a requested workflow intent.",
			plan_tool_input_schema(),
			plan_tool_output_schema(),
			true,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_RESEARCH_COMPILE,
			"Decodex Research Compile",
			"Validate or persist a latent Decodex Decision Contract from bounded research input.",
			research_compile_tool_input_schema(),
			research_compile_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_RESEARCH_PROMOTE,
			"Decodex Research Promote",
			"Inspect or explicitly promote a latent Decision Contract through Decodex authority checks.",
			research_promote_tool_input_schema(),
			research_promote_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_INTAKE_GOAL,
			"Decodex Goal Intake",
			"Dry-run or explicitly apply promoted-goal Program Intake through Decodex authority gates.",
			intake_goal_tool_input_schema(),
			intake_goal_tool_output_schema(),
			false,
		),
	]
}

fn mcp_autonomy_tools() -> Vec<McpTool> {
	vec![
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_DRAFT_OBJECTIVE,
			"Decodex Autonomy Draft Objective",
			"Validate or persist a draft Objective Contract without granting acceptance authority.",
			autonomy_draft_objective_tool_input_schema(),
			autonomy_objective_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_ACCEPT_OBJECTIVE,
			"Decodex Autonomy Accept Objective",
			"Accept a draft Objective Contract version as project-level autonomy authority without starting execution.",
			autonomy_accept_objective_tool_input_schema(),
			autonomy_objective_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_SUBMIT_SIGNAL,
			"Decodex Autonomy Submit Signal",
			"Validate or persist proposal-only autonomy signal evidence under an accepted objective.",
			autonomy_submit_signal_tool_input_schema(),
			autonomy_signal_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_COMPILE_PROPOSAL,
			"Decodex Autonomy Compile Proposal",
			"Compile or persist non-executable autonomy proposal evidence from accepted objective-bound signals.",
			autonomy_compile_proposal_tool_input_schema(),
			autonomy_proposal_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_CHALLENGE_PROPOSAL,
			"Decodex Autonomy Challenge Proposal",
			"Dry-run or record challenge evidence for an autonomy proposal without making it acceptance authority.",
			autonomy_challenge_proposal_tool_input_schema(),
			autonomy_challenge_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Plan,
			TOOL_AUTONOMY_REQUEST_PROMOTION,
			"Decodex Autonomy Request Promotion",
			"Inspect or explicitly accept an autonomy proposal into a latent Decision Contract candidate.",
			autonomy_request_promotion_tool_input_schema(),
			autonomy_promotion_request_tool_output_schema(),
			false,
		),
	]
}

fn mcp_operator_tools() -> Vec<McpTool> {
	vec![
		mcp_tool_entry(
			McpCapabilityProfile::Operate,
			TOOL_LANE_CONTROL,
			"Decodex Lane Control",
			"Inspect a lane or request guarded soft lane-control actions with explicit authority.",
			lane_control_tool_input_schema(),
			lane_control_tool_output_schema(),
			false,
		),
		mcp_tool_entry(
			McpCapabilityProfile::Admin,
			TOOL_PROJECT_CONTROL,
			"Decodex Project Control",
			"Pause or resume future project dispatch through the registered project enablement guard.",
			project_control_tool_input_schema(),
			project_control_tool_output_schema(),
			false,
		),
	]
}

fn mcp_tool_entry(
	profile: McpCapabilityProfile,
	name: &str,
	title: &str,
	description: &str,
	input_schema: Value,
	output_schema: Value,
	read_only: bool,
) -> McpTool {
	McpTool {
		required_profile: profile,
		value: mcp_tool_value(
			name,
			title,
			description,
			profile,
			input_schema,
			output_schema,
			read_only,
		),
	}
}

fn mcp_tool_value(
	name: &str,
	title: &str,
	description: &str,
	profile: McpCapabilityProfile,
	input_schema: Value,
	output_schema: Value,
	read_only: bool,
) -> Value {
	serde_json::json!({
		"name": name,
		"title": title,
		"description": description,
		"inputSchema": input_schema,
		"outputSchema": output_schema,
		"annotations": {
			"readOnlyHint": read_only,
			"destructiveHint": false,
			"idempotentHint": read_only,
			"openWorldHint": false
		},
		"_meta": {
			"decodex/capabilityProfile": profile.as_str()
		}
	})
}

pub(super) fn tool_required_profile(name: &str) -> Option<McpCapabilityProfile> {
	match name {
		TOOL_OBSERVE => Some(McpCapabilityProfile::Observe),
		TOOL_PLAN => Some(McpCapabilityProfile::Plan),
		TOOL_RESEARCH_COMPILE
		| TOOL_RESEARCH_PROMOTE
		| TOOL_INTAKE_GOAL
		| TOOL_AUTONOMY_DRAFT_OBJECTIVE
		| TOOL_AUTONOMY_ACCEPT_OBJECTIVE
		| TOOL_AUTONOMY_SUBMIT_SIGNAL
		| TOOL_AUTONOMY_COMPILE_PROPOSAL
		| TOOL_AUTONOMY_CHALLENGE_PROPOSAL
		| TOOL_AUTONOMY_REQUEST_PROMOTION => Some(McpCapabilityProfile::Plan),
		TOOL_LANE_CONTROL => Some(McpCapabilityProfile::Operate),
		TOOL_PROJECT_CONTROL => Some(McpCapabilityProfile::Admin),
		_ => None,
	}
}

fn observe_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"issue": {
				"type": "string",
				"description": "Optional issue identifier or tracker id to inspect one lane."
			},
			"runId": {
				"type": "string",
				"description": "Optional run id used with issue-scoped lane inspection."
			},
			"limit": {
				"type": "integer",
				"minimum": 1,
				"description": "Maximum recent run count for project observability."
			}
		}
	})
}

fn plan_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"intent": {
				"type": "string",
				"enum": ["research", "validation_ready", "handoff", "lane_control"],
				"description": "Decodex workflow intent to route."
			},
			"issue": {
				"type": "string",
				"description": "Optional issue identifier for lane-scoped prompts."
			},
			"contractId": {
				"type": "string",
				"description": "Optional Decision Contract id for research or intake planning."
			}
		},
		"required": ["intent"]
	})
}

fn research_compile_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run validates without persistence; apply persists a latent Decision Contract."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"input": {
				"type": "object",
				"additionalProperties": true,
				"description": "Structured Decodex research/design input."
			},
			"intent": {
				"type": "string",
				"description": "Minimal natural-language research/design intent."
			},
			"sourceIssue": {
				"type": "string",
				"description": "Optional source tracker issue identifier for minimal intent intake."
			},
			"outcome": {
				"type": "string",
				"enum": ["decision_ready", "not_decision_ready", "blocked", "needs_human_decision"]
			},
			"authority": planning_authority_input_schema()
		}
	})
}

fn research_promote_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run inspects readiness; apply records explicit acceptance."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"contractId": {
				"type": "string",
				"description": "Decision Contract identifier to inspect or promote."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["contractId"]
	})
}

fn intake_goal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run previews generated issues; apply materializes only with explicit authority."
			},
			"contractId": {
				"type": "string",
				"description": "Promoted Decision Contract identifier to materialize."
			},
			"teamIssueIdentifier": {
				"type": "string",
				"description": "Optional source issue used to anchor generated issue team/state on apply."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["contractId"]
	})
}

fn autonomy_draft_objective_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run validates the Objective Contract; apply persists a draft only."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"objective": {
				"type": "object",
				"additionalProperties": true,
				"description": "decodex.autonomy_objective/1 payload with state=draft."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["objective"]
	})
}

fn autonomy_accept_objective_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run inspects the draft acceptance target; apply accepts the draft Objective Contract version."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"objectiveId": {
				"type": "string",
				"description": "Objective Contract id to accept."
			},
			"objectiveVersion": {
				"type": "integer",
				"minimum": 1,
				"description": "Draft Objective Contract version to accept."
			},
			"authority": {
				"type": "object",
				"additionalProperties": false,
				"description": "Explicit human/operator objective acceptance authority. Runtime-policy acceptance requires trusted Decodex state and is not accepted from caller-supplied fields.",
				"properties": {
					"acceptedBy": {
						"type": "string",
						"description": "Human or operator actor accepting the Objective Contract."
					},
					"acceptedByKind": {
						"type": "string",
						"enum": ["user"],
						"description": "Only direct user/operator acceptance is accepted through this tool until trusted runtime-policy resolution exists."
					},
					"acceptedAt": {
						"type": "string",
						"description": "Optional RFC3339 acceptance timestamp; Decodex fills the current time when omitted."
					},
					"acceptanceSource": {
						"type": "string",
						"description": "Source of the explicit acceptance, such as conversation or operator command."
					}
				},
				"required": ["acceptedBy", "acceptedByKind", "acceptanceSource"]
			}
		},
		"required": ["objectiveId", "objectiveVersion"]
	})
}

fn autonomy_submit_signal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run validates the signal; apply persists proposal-only signal evidence."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"kind": {
				"type": "string",
				"enum": [
					"runtime_health",
					"validation_regression",
					"review_feedback_cluster",
					"user_feedback_cluster",
					"spec_drift",
					"protocol_drift",
					"metric_regression",
					"execution_friction",
					"docs_skill_drift"
				]
			},
			"signal": {
				"type": "object",
				"additionalProperties": true,
				"description": "Signal input without derived id/fingerprint; Decodex derives stable identity."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["kind", "signal"]
	})
}

fn autonomy_compile_proposal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run compiles non-executable proposal evidence; apply persists it."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"proposal": {
				"type": "object",
				"additionalProperties": true,
				"description": "Autonomy proposal compile input."
			},
			"signalIds": {
				"type": "array",
				"items": { "type": "string" },
				"description": "Persisted autonomy signal ids to bind into the proposal."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["proposal"]
	})
}

fn autonomy_challenge_proposal_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run previews the challenge effect; apply records challenge evidence."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"proposalId": {
				"type": "string",
				"description": "Stable autonomy proposal id."
			},
			"challenge": {
				"type": "object",
				"additionalProperties": true,
				"description": "Challenge evidence. It is not acceptance authority."
			},
			"authority": planning_authority_input_schema()
		},
		"required": ["proposalId", "challenge"]
	})
}

fn autonomy_request_promotion_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"mode": {
				"type": "string",
				"enum": ["dry_run", "apply"],
				"description": "dry_run explains required authority; apply creates a latent Decision Contract candidate only with explicit proposal acceptance authority."
			},
			"projectId": {
				"type": "string",
				"description": "Optional Decodex service id when the MCP context is not project-scoped."
			},
			"proposalId": {
				"type": "string",
				"description": "Stable autonomy proposal id."
			},
			"authority": {
				"type": "object",
				"additionalProperties": true,
				"description": "Explicit proposal acceptance authority, including acceptedBy, acceptedByKind, acceptanceSource, reason, proposalActor, and proposalActorKind. acceptedProjectPolicy payloads are refused because trusted policy authority must be resolved from Decodex state."
			}
		},
		"required": ["proposalId"]
	})
}

fn planning_authority_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"source": {
				"type": "string",
				"description": "Explicit remote client or operator source for an apply-style call."
			},
			"reason": {
				"type": "string",
				"description": "Explicit reason authorizing an apply-style call."
			},
			"acceptedBy": {
				"type": "string",
				"description": "Actor accepting a Decision Contract promotion."
			},
			"acceptedAt": {
				"type": "string",
				"description": "Optional RFC3339 acceptance timestamp."
			},
			"acceptanceSource": {
				"type": "string",
				"description": "Conversation, issue, or policy source for explicit promotion authority."
			},
			"runId": {
				"type": "string",
				"description": "Current lane run id when a future planning mutation is lane-scoped."
			},
			"expectedTurnId": {
				"type": "string",
				"description": "Current lane turn id when a future planning mutation is lane-scoped."
			}
		}
	})
}

fn lane_control_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"action": {
				"type": "string",
				"enum": ["inspect", "interrupt", "steer", "manual_attention", "retained_resume"]
			},
			"projectId": {
				"type": "string",
				"description": "Optional project id precondition. When supplied, it must match the MCP gateway project context."
			},
			"issue": {
				"type": "string",
				"description": "Issue identifier or tracker issue id."
			},
			"runId": {
				"type": "string",
				"description": "Current run id observed through inspect."
			},
			"expectedTurnId": {
				"type": "string",
				"description": "Current turn id required for steer."
			},
			"message": {
				"type": "string",
				"description": "Operator-supplied steer message."
			},
			"force": {
				"type": "boolean",
				"description": "Hard interrupt fallback is not exposed through MCP and is refused when true."
			},
			"authority": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"reason": {
						"type": "string",
						"description": "Explicit operator reason for a mutating lane-control request."
					},
					"source": {
						"type": "string",
						"description": "Remote client or operator source identifier."
					},
					"inspectedRunId": {
						"type": "string",
						"description": "Run id observed through a prior inspect call."
					},
					"expectedTurnId": {
						"type": "string",
						"description": "Turn id observed through inspect and required for steer."
					},
					"allowHardFallback": {
						"type": "boolean",
						"description": "Explicit acknowledgement required with force=true before hard interrupt fallback can run."
					}
				}
			}
		},
		"required": ["action"]
	})
}

fn project_control_tool_input_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"action": {
				"type": "string",
				"enum": ["status", "pause", "resume", "scan"],
				"description": "Project-control action. Pause/resume only affect future dispatch."
			},
			"projectId": {
				"type": "string",
				"description": "Registered Decodex project id. Optional only when the gateway was started with a project config."
			},
			"authority": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"reason": {
						"type": "string",
						"description": "Explicit operator reason for pause or resume."
					},
					"source": {
						"type": "string",
						"description": "Remote client or operator source identifier."
					},
					"acknowledgeFutureDispatchOnly": {
						"type": "boolean",
						"description": "Must be true for pause/resume; active lanes are not killed."
					}
				}
			}
		},
		"required": ["action"]
	})
}

fn observe_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.observe_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["ok"]
			},
			"capability_profile": {
				"type": "string",
				"enum": ["observe"]
			},
			"observability": {
				"type": "object",
				"additionalProperties": true
			}
		},
		"required": ["schema", "status", "capability_profile", "observability"]
	}))
}

fn plan_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.plan_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["ok"]
			},
			"intent": {
				"type": "string",
				"enum": ["research", "validation_ready", "handoff", "lane_control"]
			},
			"prompt": {
				"type": "string"
			},
			"resource": {
				"type": "string"
			},
			"next_action": {
				"type": "string"
			},
			"issue": {
				"type": ["string", "null"]
			},
			"contract_id": {
				"type": ["string", "null"]
			}
		},
		"required": ["schema", "status", "intent", "prompt", "resource", "next_action"]
	}))
}

fn research_compile_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.research_compile_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"contract_id": { "type": "string" },
			"contract_status": {
				"type": "string",
				"enum": ["draft_latent", "accepted_promoted", "rejected_superseded", "needs_human_decision"]
			},
			"ready_for_issue_shaping": { "type": "boolean" },
			"issue_generation_ready_after_promotion": { "type": "boolean" },
			"execution_authority_granted": { "type": "boolean" },
			"proposed_issue_count": { "type": "integer", "minimum": 0 },
			"promotion_targets": { "type": "array", "items": { "type": "string" } },
			"conflict_domains": { "type": "array", "items": { "type": "string" } },
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"contract_id",
			"contract_status",
			"ready_for_issue_shaping",
			"issue_generation_ready_after_promotion",
			"execution_authority_granted",
			"proposed_issue_count",
			"promotion_targets",
			"conflict_domains",
			"next_action"
		]
	}))
}

fn research_promote_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.research_promote_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"contract_id": { "type": "string" },
			"contract_status": {
				"type": "string",
				"enum": ["draft_latent", "accepted_promoted", "rejected_superseded", "needs_human_decision"]
			},
			"execution_authority_granted": { "type": "boolean" },
			"ready_for_issue_shaping": { "type": "boolean" },
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"contract_id",
			"contract_status",
			"execution_authority_granted",
			"ready_for_issue_shaping",
			"next_action"
		]
	}))
}

fn intake_goal_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.intake_goal_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"service_id": { "type": "string" },
			"contract_id": { "type": "string" },
			"dry_run": { "type": "boolean" },
			"applied": { "type": "boolean" },
			"persisted": { "type": "boolean" },
			"issue_count": { "type": "integer", "minimum": 0 },
			"issues": {
				"type": "array",
				"items": {
					"type": "object",
					"additionalProperties": false,
					"properties": {
						"title": { "type": "string" },
						"objective": { "type": "string" },
						"issue_identifier": { "type": ["string", "null"] },
						"action": { "type": "string" },
						"dependencies": { "type": "array", "items": { "type": "string" } },
						"conflict_domains": { "type": "array", "items": { "type": "string" } },
						"acceptance": { "type": "array", "items": { "type": "string" } },
						"validation": { "type": "array", "items": { "type": "string" } },
						"reasons": { "type": "array", "items": { "type": "string" } }
					},
					"required": [
						"title",
						"objective",
						"issue_identifier",
						"action",
						"dependencies",
						"conflict_domains",
						"acceptance",
						"validation",
						"reasons"
					]
				}
			},
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"service_id",
			"contract_id",
			"dry_run",
			"applied",
			"persisted",
			"issue_count",
			"issues",
			"next_action"
		]
	}))
}

fn autonomy_objective_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_objective_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"objective": { "type": "object", "additionalProperties": true },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"objective",
			"authority_effect",
			"next_action"
		]
	}))
}

fn autonomy_signal_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_signal_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"signal": { "type": "object", "additionalProperties": true },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"signal",
			"authority_effect",
			"next_action"
		]
	}))
}

fn autonomy_proposal_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_proposal_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"proposal": { "type": "object", "additionalProperties": true },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"proposal",
			"authority_effect",
			"next_action"
		]
	}))
}

fn autonomy_challenge_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_challenge_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"proposal": { "type": "object", "additionalProperties": true },
			"challenge_evidence_count": { "type": "integer", "minimum": 0 },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" },
			"updated_at": { "type": ["string", "null"] }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"proposal",
			"challenge_evidence_count",
			"authority_effect",
			"next_action"
		]
	}))
}

fn autonomy_promotion_request_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": { "type": "string", "enum": ["decodex.mcp.autonomy_promotion_request_result/1"] },
			"status": { "type": "string", "enum": ["ok"] },
			"mode": { "type": "string", "enum": ["dry_run", "apply"] },
			"persisted": { "type": "boolean" },
			"project_id": { "type": "string" },
			"proposal": { "type": "object", "additionalProperties": true },
			"decision_contract_id": { "type": ["string", "null"] },
			"execution_authority_granted": { "type": "boolean" },
			"required_authority": { "type": "array", "items": { "type": "string" } },
			"authority_effect": { "type": "string" },
			"next_action": { "type": "string" }
		},
		"required": [
			"schema",
			"status",
			"mode",
			"persisted",
			"project_id",
			"proposal",
			"execution_authority_granted",
			"required_authority",
			"authority_effect",
			"next_action"
		]
	}))
}

fn lane_control_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.lane_control_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["ok", "queued", "refused"]
			},
			"reason": {
				"type": "string"
			},
			"message": {
				"type": "string"
			},
			"capability_profile": {
				"type": "string",
				"enum": ["operate"]
			},
			"action": {
				"type": "string",
				"enum": ["inspect", "interrupt", "steer", "manual_attention", "retained_resume"]
			},
			"project_id": {
				"type": ["string", "null"]
			},
			"issue": {
				"type": ["string", "null"]
			},
			"run_id": {
				"type": ["string", "null"]
			},
			"preconditions": {
				"type": "object",
				"additionalProperties": false,
				"properties": {
					"project_id_present": { "type": "boolean" },
					"issue_present": { "type": "boolean" },
					"run_id_present": { "type": "boolean" },
					"expected_turn_id_present": { "type": "boolean" },
					"message_present": { "type": "boolean" },
					"force_requested": { "type": "boolean" },
					"authority_reason_present": { "type": "boolean" },
					"authority_source_present": { "type": "boolean" },
					"authority_inspected_run_id_present": { "type": "boolean" },
					"authority_expected_turn_id_present": { "type": "boolean" },
					"authority_allow_hard_fallback": { "type": "boolean" }
				},
				"required": [
					"project_id_present",
					"issue_present",
					"run_id_present",
					"expected_turn_id_present",
					"message_present",
					"force_requested",
					"authority_reason_present",
					"authority_source_present",
					"authority_inspected_run_id_present",
					"authority_expected_turn_id_present",
					"authority_allow_hard_fallback"
				]
			},
			"result": {
				"type": "object",
				"additionalProperties": true
			}
		},
		"required": [
			"schema",
			"status",
			"reason",
			"message",
			"capability_profile",
			"action",
			"preconditions",
			"result"
		]
	}))
}

fn project_control_tool_output_schema() -> Value {
	tool_output_schema(serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.project_control_result/1"]
			},
			"status": {
				"type": "string",
				"enum": ["ok", "refused"]
			},
			"reason": {
				"type": "string"
			},
			"message": {
				"type": "string"
			},
			"capability_profile": {
				"type": "string",
				"enum": ["admin"]
			},
			"action": {
				"type": "string",
				"enum": ["status", "pause", "resume", "scan"]
			},
			"project_id": {
				"type": ["string", "null"]
			},
			"future_dispatch_only": {
				"type": "boolean"
			},
			"result": {
				"type": "object",
				"additionalProperties": true
			}
		},
		"required": [
			"schema",
			"status",
			"reason",
			"message",
			"capability_profile",
			"action",
			"future_dispatch_only",
			"result"
		]
	}))
}

fn tool_output_schema(primary_schema: Value) -> Value {
	serde_json::json!({
		"oneOf": [
			primary_schema,
			tool_refusal_output_schema(),
			tool_validation_error_output_schema()
		]
	})
}

fn tool_refusal_output_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.refusal/1"]
			},
			"status": {
				"type": "string",
				"enum": ["refused"]
			},
			"reason": {
				"type": "string"
			},
			"message": {
				"type": "string"
			},
			"tool": {
				"type": "string"
			},
			"capability_profile": {
				"type": "string",
				"enum": ["observe", "plan", "operate", "admin"]
			},
			"required_capability_profile": {
				"type": "string",
				"enum": ["observe", "plan", "operate", "admin"]
			}
		},
		"required": ["schema", "status", "reason", "message"]
	})
}

fn tool_validation_error_output_schema() -> Value {
	serde_json::json!({
		"type": "object",
		"additionalProperties": false,
		"properties": {
			"schema": {
				"type": "string",
				"enum": ["decodex.mcp.tool_validation_error/1"]
			},
			"status": {
				"type": "string",
				"enum": ["refused"]
			},
			"reason": {
				"type": "string",
				"enum": ["invalid_arguments"]
			},
			"tool": {
				"type": "string"
			},
			"message": {
				"type": "string"
			}
		},
		"required": ["schema", "status", "reason", "tool", "message"]
	})
}
