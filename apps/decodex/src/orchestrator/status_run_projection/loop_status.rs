#[allow(clippy::wildcard_imports)] use super::*;

pub(in crate::orchestrator) fn operator_loop_status_for_run(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
	lifecycle_summary: Option<String>,
) -> crate::prelude::Result<OperatorLoopStatus> {
	let loop_evidence = state_store.project_loop_evidence_snapshot(project.service_id())?;

	operator_loop_status_for_run_with_evidence(
		project,
		&loop_evidence,
		issue_id,
		run_id,
		attempt_number,
		default_review_phase,
		lifecycle_summary,
	)
}

pub(in crate::orchestrator) fn operator_loop_status_for_run_with_evidence(
	project: &ServiceConfig,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
	lifecycle_summary: Option<String>,
) -> crate::prelude::Result<OperatorLoopStatus> {
	let review_level = project.codex().review_level();
	let review = operator_review_loop_status(
		review_level,
		loop_evidence,
		issue_id,
		run_id,
		attempt_number,
		default_review_phase,
	)?;
	let events = loop_evidence.private_events(issue_id, run_id, attempt_number);
	let architecture_recovery =
		events.iter().rev().find_map(operator_architecture_recovery_status_from_event);
	let boundary = events.iter().rev().find_map(operator_boundary_status_from_event);
	let decision_request = events
		.iter()
		.rev()
		.find(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE)
		.and_then(operator_authority_decision_request_status_from_event);
	let autonomy_objective = operator_autonomy_objective_status(project, loop_evidence);
	let autonomy_signals = operator_autonomy_signal_statuses(loop_evidence);
	let autonomy_proposals = operator_autonomy_proposal_statuses(loop_evidence);
	let autonomy_lineage = operator_autonomy_lineage_statuses(loop_evidence);
	let autonomy_report = operator_autonomy_report_status(
		autonomy_objective.as_ref(),
		&autonomy_signals,
		&autonomy_proposals,
		&autonomy_lineage,
	);
	let autonomy = operator_loop_autonomy(
		boundary.as_ref(),
		architecture_recovery.as_ref(),
		decision_request.as_ref(),
	);
	let summary = operator_loop_status_summary(
		review.as_ref(),
		architecture_recovery.as_ref(),
		boundary.as_ref(),
		decision_request.as_ref(),
		autonomy,
		lifecycle_summary.as_deref(),
	);
	let next_action = operator_loop_status_next_action(
		review.as_ref(),
		architecture_recovery.as_ref(),
		boundary.as_ref(),
		decision_request.as_ref(),
	);

	Ok(OperatorLoopStatus {
		review_level: review_level.as_str().to_owned(),
		autonomy: autonomy.to_owned(),
		summary,
		next_action,
		autonomy_objective,
		autonomy_signals,
		autonomy_proposals,
		autonomy_lineage,
		autonomy_report,
		review,
		architecture_recovery,
		boundary,
		decision_request,
	})
}

pub(in crate::orchestrator) fn operator_review_loop_status(
	review_level: ReviewLevel,
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	default_review_phase: Option<&str>,
) -> crate::prelude::Result<Option<OperatorReviewLoopStatus>> {
	if let Some(checkpoint) = operator_latest_review_checkpoint_event_status(
		loop_evidence,
		issue_id,
		run_id,
		attempt_number,
	) {
		return Ok(Some(checkpoint));
	}

	let latest_checkpoint = ["handoff", "repair"]
		.into_iter()
		.filter_map(|phase| {
			loop_evidence.review_policy_checkpoint(issue_id, run_id, attempt_number, phase)
		})
		.max_by(|left, right| {
			left.updated_at_unix()
				.cmp(&right.updated_at_unix())
				.then_with(|| left.phase().cmp(right.phase()))
		});

	if let Some(checkpoint) = latest_checkpoint {
		let nonclean_rounds = checkpoint.nonclean_rounds();
		let summary = operator_review_checkpoint_summary_fields(checkpoint.details_json());

		return Ok(Some(OperatorReviewLoopStatus {
			phase: checkpoint.phase().to_owned(),
			status: checkpoint.status().to_owned(),
			checkpoint: Some(OperatorReviewCheckpointStatus {
				head_sha: checkpoint.head_sha().to_owned(),
				round: nonclean_rounds,
				nonclean_rounds,
				review_class: summary.review_class,
				risk_class: summary.risk_class,
				compact_eligible: summary.compact_eligible,
				fallback_reason: summary.fallback_reason,
				active_fingerprints: summary.active_fingerprints,
				stop_fingerprint: summary.stop_fingerprint,
				route_counts: summary.route_counts,
				route_next_action: summary.route_next_action,
				updated_at: checkpoint.updated_at().to_owned(),
			}),
		}));
	}

	if review_level.requires_review_checkpoint()
		&& let Some(default_review_phase) = default_review_phase
	{
		return Ok(Some(OperatorReviewLoopStatus {
			phase: default_review_phase.to_owned(),
			status: String::from("pending"),
			checkpoint: None,
		}));
	}

	Ok(None)
}

pub(in crate::orchestrator) fn operator_latest_review_checkpoint_event_status(
	loop_evidence: &ProjectLoopEvidenceSnapshot,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) -> Option<OperatorReviewLoopStatus> {
	loop_evidence.private_events(issue_id, run_id, attempt_number).iter().rev().find_map(|event| {
		let payload = event.payload();

		if event.event_type() != "review_checkpoint" {
			return None;
		}

		let phase = payload.get("phase").and_then(Value::as_str)?;
		let status = payload.get("status").and_then(Value::as_str)?;
		let head_sha = payload.get("head_sha").and_then(Value::as_str)?;
		let nonclean_rounds = payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or(0);
		let checkpoint =
			loop_evidence.review_policy_checkpoint(issue_id, run_id, attempt_number, phase)?;

		if checkpoint.status() != status
			|| checkpoint.head_sha() != head_sha
			|| checkpoint.nonclean_rounds() != nonclean_rounds
		{
			return None;
		}

		let details_json = payload.get("review").unwrap_or(payload).to_string();
		let summary = operator_review_checkpoint_summary_fields(&details_json);

		Some(OperatorReviewLoopStatus {
			phase: phase.to_owned(),
			status: status.to_owned(),
			checkpoint: Some(OperatorReviewCheckpointStatus {
				head_sha: head_sha.to_owned(),
				round: nonclean_rounds,
				nonclean_rounds,
				review_class: summary.review_class,
				risk_class: summary.risk_class,
				compact_eligible: summary.compact_eligible,
				fallback_reason: summary.fallback_reason,
				active_fingerprints: summary.active_fingerprints,
				stop_fingerprint: summary.stop_fingerprint,
				route_counts: summary.route_counts,
				route_next_action: summary.route_next_action,
				updated_at: checkpoint.updated_at().to_owned(),
			}),
		})
	})
}

pub(in crate::orchestrator) fn operator_review_checkpoint_summary_fields(
	details_json: &str,
) -> OperatorReviewCheckpointSummaryFields {
	let Ok(details) = serde_json::from_str::<Value>(details_json) else {
		return OperatorReviewCheckpointSummaryFields {
			review_class: None,
			risk_class: None,
			compact_eligible: None,
			fallback_reason: None,
			active_fingerprints: Vec::new(),
			stop_fingerprint: None,
			route_counts: Vec::new(),
			route_next_action: None,
		};
	};
	let policy = details.get("finding_policy");
	let cost_control = details.get("review_cost_control");
	let review_class = cost_control
		.and_then(|cost_control| cost_control.get("review_class"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let risk_class = cost_control
		.and_then(|cost_control| cost_control.get("risk_class"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let compact_eligible = cost_control
		.and_then(|cost_control| cost_control.get("compact_eligible"))
		.and_then(Value::as_bool);
	let fallback_reason = cost_control
		.and_then(|cost_control| cost_control.get("fallback_reason"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let active_fingerprints = policy
		.and_then(|policy| policy.get("active_fingerprints"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect();
	let stop_fingerprint = policy
		.and_then(|policy| policy.get("stop_fingerprint"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let route_summary = details.get("finding_route_summary");
	let route_counts = route_summary
		.and_then(|summary| summary.get("route_counts"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|count| {
			Some(OperatorReviewRouteCount {
				route: count.get("route")?.as_str()?.to_owned(),
				count: usize::try_from(count.get("count")?.as_u64()?).ok()?,
			})
		})
		.collect();
	let route_next_action = route_summary
		.and_then(|summary| summary.get("next_action"))
		.and_then(Value::as_str)
		.map(str::to_owned);

	OperatorReviewCheckpointSummaryFields {
		review_class,
		risk_class,
		compact_eligible,
		fallback_reason,
		active_fingerprints,
		stop_fingerprint,
		route_counts,
		route_next_action,
	}
}

pub(in crate::orchestrator) fn operator_architecture_recovery_status_from_event(
	event: &PrivateExecutionEvent,
) -> Option<OperatorArchitectureRecoveryStatus> {
	if !matches!(
		event.event_type(),
		ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE
			| ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE
			| ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE
	) {
		return None;
	}

	let payload = event.payload();
	let reason_code = payload.get("reason_code")?.as_str()?.to_owned();
	let guardrail_reason = payload
		.get("guardrail_reason")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("loop_guardrail")
				.and_then(|guardrail| guardrail.get("reason"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let boundary_disposition = payload
		.get("boundary_disposition")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("disposition"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let boundary_policy_decision = payload
		.get("boundary_policy_decision")
		.and_then(Value::as_str)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("policy_decision"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned)
		.or_else(|| {
			boundary_disposition
				.as_deref()
				.map(operator_boundary_policy_decision_from_disposition)
				.map(str::to_owned)
		});
	let requires_enhanced_evidence = payload
		.get("requires_enhanced_evidence")
		.and_then(Value::as_bool)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("requires_enhanced_evidence"))
				.and_then(Value::as_bool)
		})
		.unwrap_or_else(|| {
			boundary_policy_decision
				.as_deref()
				.is_some_and(operator_boundary_policy_requires_enhanced_evidence)
		});
	let blocks_landing = payload
		.get("blocks_landing")
		.and_then(Value::as_bool)
		.or_else(|| {
			payload
				.get("authority_boundary_check")
				.and_then(|boundary| boundary.get("blocks_landing"))
				.and_then(Value::as_bool)
		})
		.unwrap_or_else(|| {
			boundary_policy_decision.as_deref().is_some_and(operator_boundary_policy_blocks_landing)
		});
	let recovery_budget_attempt = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("attempt"))
		.and_then(Value::as_u64);
	let recovery_budget_max_attempts = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("max_attempts"))
		.and_then(Value::as_u64);
	let budget = recovery_budget_attempt
		.zip(recovery_budget_max_attempts)
		.map(|(attempt, max_attempts)| OperatorRecoveryBudgetStatus { attempt, max_attempts });
	let next_action = operator_architecture_recovery_next_action(
		&reason_code,
		boundary_policy_decision.as_deref(),
		requires_enhanced_evidence,
		blocks_landing,
	);

	Some(OperatorArchitectureRecoveryStatus {
		status: operator_architecture_recovery_status_for_reason(&reason_code).to_owned(),
		reason_code,
		guardrail_reason,
		boundary_disposition,
		boundary_policy_decision,
		requires_enhanced_evidence,
		blocks_landing,
		round: recovery_budget_attempt,
		budget,
		next_action,
	})
}

pub(in crate::orchestrator) fn operator_architecture_recovery_status_for_reason(
	reason_code: &str,
) -> &'static str {
	match reason_code {
		"architecture_recovery_started" => "active",
		"architecture_recovery_exhausted" => "exhausted",
		"contract_boundary_required" | "external_dependency_required" => "human_required",
		_ => "terminal",
	}
}

pub(in crate::orchestrator) fn operator_architecture_recovery_next_action(
	reason_code: &str,
	policy_decision: Option<&str>,
	requires_enhanced_evidence: bool,
	blocks_landing: bool,
) -> String {
	match reason_code {
		"architecture_recovery_started" => {
			match (policy_decision, blocks_landing, requires_enhanced_evidence) {
				(Some(policy), true, _) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`; keep landing blocked until validation or review-policy evidence is restored."
				),
				(Some(policy), false, true) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`; preserve enhanced evidence before review handoff or landing."
				),
				(Some(policy), false, false) => format!(
					"Retry with a materially different implementation strategy under authority policy `{policy}`."
				),
				(None, true, _) => String::from(
					"Retry with a materially different implementation strategy; keep landing blocked until validation or review-policy evidence is restored.",
				),
				(None, false, true) => String::from(
					"Retry with a materially different implementation strategy; preserve enhanced evidence before review handoff or landing.",
				),
				(None, false, false) => String::from(
					"Retry with a materially different implementation strategy inside authority.",
				),
			}
		},
		"architecture_recovery_exhausted" => String::from(
			"Require a new accepted recovery strategy or architecture decision before retrying.",
		),
		"external_dependency_required" => String::from(
			"Resolve the dependency or Execution Program readiness blocker before retrying.",
		),
		"contract_boundary_required" => String::from(
			"Resolve the Decision Contract or Authority Envelope boundary before retrying.",
		),
		_ => String::from("Inspect the Architecture Recovery Packet before retrying."),
	}
}

pub(in crate::orchestrator) fn operator_boundary_policy_decision_from_disposition(
	disposition: &str,
) -> &'static str {
	match disposition {
		"requires_human" | "insufficient_evidence" => "requires_human_decision",
		_ => "auto_continue",
	}
}

pub(in crate::orchestrator) fn operator_boundary_policy_requires_enhanced_evidence(
	policy_decision: &str,
) -> bool {
	matches!(policy_decision, "requires_enhanced_evidence" | "block_landing")
}

pub(in crate::orchestrator) fn operator_boundary_policy_blocks_landing(
	policy_decision: &str,
) -> bool {
	policy_decision == "block_landing"
}

pub(in crate::orchestrator) fn operator_boundary_status_from_event(
	event: &PrivateExecutionEvent,
) -> Option<OperatorBoundaryStatus> {
	if event.event_type() != AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE {
		return None;
	}

	let payload = event.payload();
	let disposition = payload
		.get("final_disposition")
		.and_then(|final_disposition| final_disposition.get("disposition"))
		.and_then(Value::as_str)
		.or_else(|| payload.get("disposition").and_then(Value::as_str))?
		.to_owned();
	let reason = payload
		.get("final_disposition")
		.and_then(|final_disposition| final_disposition.get("reason"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let policy_decision = payload
		.get("policy_decision")
		.and_then(Value::as_str)
		.or_else(|| {
			payload.get("policy").and_then(|policy| policy.get("decision")).and_then(Value::as_str)
		})
		.map(str::to_owned)
		.unwrap_or_else(|| {
			operator_boundary_policy_decision_from_disposition(&disposition).to_owned()
		});
	let attempted_recovery_reason =
		payload.get("attempted_recovery_reason").and_then(Value::as_str).map(str::to_owned);
	let changed_surface_count =
		payload.get("changed_surfaces").and_then(Value::as_array).map_or(0, Vec::len);
	let improvement_signal_count =
		payload.get("improvement_signals").and_then(Value::as_array).map_or(0, Vec::len);
	let requires_enhanced_evidence = payload
		.get("policy")
		.and_then(|policy| policy.get("requires_enhanced_evidence"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| operator_boundary_policy_requires_enhanced_evidence(&policy_decision));
	let blocks_landing = payload
		.get("policy")
		.and_then(|policy| policy.get("blocks_landing"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| operator_boundary_policy_blocks_landing(&policy_decision));

	Some(OperatorBoundaryStatus {
		disposition,
		policy_decision,
		reason,
		attempted_recovery_reason,
		changed_surface_count,
		improvement_signal_count,
		requires_enhanced_evidence,
		blocks_landing,
	})
}

pub(in crate::orchestrator) fn operator_loop_autonomy(
	boundary: Option<&OperatorBoundaryStatus>,
	architecture_recovery: Option<&OperatorArchitectureRecoveryStatus>,
	decision_request: Option<&OperatorAuthorityDecisionRequestStatus>,
) -> &'static str {
	if decision_request.is_some() {
		return "human_required";
	}
	if boundary.is_some_and(|boundary| boundary.policy_decision == "requires_human_decision") {
		return "human_required";
	}
	if architecture_recovery.is_some_and(|recovery| recovery.status != "active") {
		return "human_required";
	}

	"autonomous"
}

pub(in crate::orchestrator) fn operator_loop_status_summary(
	review: Option<&OperatorReviewLoopStatus>,
	architecture_recovery: Option<&OperatorArchitectureRecoveryStatus>,
	boundary: Option<&OperatorBoundaryStatus>,
	decision_request: Option<&OperatorAuthorityDecisionRequestStatus>,
	autonomy: &str,
	lifecycle_summary: Option<&str>,
) -> String {
	if let Some(request) = decision_request {
		return format!("human-required boundary stop: {} on {}", request.reason, request.boundary);
	}
	if let Some(recovery) = architecture_recovery {
		return format!("architecture recovery {}: {}", recovery.status, recovery.reason_code);
	}
	if let Some(review) = review {
		if let Some(fingerprint) =
			review.checkpoint.as_ref().and_then(|checkpoint| checkpoint.stop_fingerprint.as_ref())
		{
			return format!(
				"review {}: {} stopped on fingerprint {}",
				review.phase, review.status, fingerprint
			);
		}

		return format!("review {}: {}", review.phase, review.status);
	}
	if let Some(boundary) = boundary {
		return format!("boundary check: {}", boundary.disposition);
	}
	if let Some(lifecycle_summary) = lifecycle_summary {
		return lifecycle_summary.to_owned();
	}

	format!("loop autonomy: {autonomy}")
}

pub(in crate::orchestrator) fn operator_loop_status_next_action(
	review: Option<&OperatorReviewLoopStatus>,
	architecture_recovery: Option<&OperatorArchitectureRecoveryStatus>,
	boundary: Option<&OperatorBoundaryStatus>,
	decision_request: Option<&OperatorAuthorityDecisionRequestStatus>,
) -> Option<String> {
	if let Some(request) = decision_request {
		return Some(request.next_action.clone());
	}
	if let Some(recovery) = architecture_recovery {
		return Some(recovery.next_action.clone());
	}
	if let Some(boundary) = boundary {
		return match boundary.policy_decision.as_str() {
			"requires_human_decision" =>
				Some(String::from("Resolve the Authority Boundary Check before retrying the lane.")),
			"block_landing" => Some(String::from(
				"Continue recovery, but block landing until review or validation policy evidence is restored.",
			)),
			"requires_enhanced_evidence" => Some(String::from(
				"Continue recovery and preserve enhanced evidence before review handoff or landing.",
			)),
			_ => None,
		};
	}

	review.and_then(|review| {
		if review.status != "clean"
			&& let Some(route_next_action) = review
				.checkpoint
				.as_ref()
				.and_then(|checkpoint| checkpoint.route_next_action.clone())
		{
			return Some(route_next_action);
		}

		match review.status.as_str() {
			"clean" if review.phase == "handoff" => Some(String::from(
				"Push or update the PR and record review handoff for the clean current lane head.",
			)),
			"clean" if review.phase == "repair" => Some(String::from(
				"Record a fresh current-head handoff review checkpoint for the repaired lane head.",
			)),
			"pending" => Some(String::from(
				"Record the independent Decodex Review checkpoint for the current lane head.",
			)),
			"findings" => Some(String::from(
				"Repair validated review findings and record a fresh checkpoint.",
			)),
			"blocked" =>
				Some(String::from("Resolve the blocked Decodex Review before continuing.")),
			"needs_architecture_review" =>
				Some(String::from("Get architecture direction before continuing review repair.")),
			_ => None,
		}
	})
}
