use super::{
	ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE, ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
	ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE, AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE,
	AUTHORITY_DECISION_REQUEST_EVENT_TYPE, AgentPrivateEvidenceRef, EvidenceRequest,
	HarnessImprovementCandidateSummary, OperatorRunStatus, PHASE_ACCEPTANCE_CHECK_EVENT_TYPE,
	PRIVATE_EVIDENCE_PAYLOAD_PREVIEW_LIMIT, PRIVATE_EVIDENCE_READBACK_SCHEMA, Path,
	PrivateEvidenceArchitectureRecoverySummary, PrivateEvidenceBoundaryCheckSummary,
	PrivateEvidenceDecisionRequestSummary, PrivateEvidencePayloadSummary,
	PrivateEvidencePhaseAcceptanceSummary, PrivateEvidenceReadback, PrivateEvidenceReadbackEvent,
	PrivateEvidenceRepoGateFailureSummary, PrivateEvidenceReviewCheckpointSummary,
	PrivateEvidenceReviewRouteCount, PrivateEvidenceTarget, ProjectRunStatus,
	REVIEW_CHECKPOINT_EVENT_TYPE, Result, ServiceConfig, StateStore, Value, collections, eyre,
	harness_improvement_candidates_from_private_events, operator_run_issue_identifier_from_fields,
	relative_worktree_path_for_path, state,
};

pub(in crate::orchestrator) fn render_private_evidence_reference(
	run: &OperatorRunStatus,
) -> String {
	let private_evidence = agent_private_evidence_ref(run);

	format!(
		"ref={} source={} default_view={} read=`{}`",
		private_evidence.evidence_ref,
		private_evidence.source,
		private_evidence.default_view,
		private_evidence.read_command
	)
}

pub(in crate::orchestrator) fn agent_private_evidence_ref(
	run: &OperatorRunStatus,
) -> AgentPrivateEvidenceRef {
	run.private_evidence.clone()
}

pub(in crate::orchestrator) fn private_evidence_ref_for_run_fields(
	project_id: &str,
	project_config_path: &Path,
	issue_id: &str,
	issue_identifier: Option<&str>,
	run_id: &str,
	attempt_number: i64,
) -> AgentPrivateEvidenceRef {
	AgentPrivateEvidenceRef {
		evidence_ref: private_evidence_ref_for_parts(project_id, issue_id, run_id, attempt_number),
		source: String::from("runtime_sqlite"),
		default_view: String::from("summarized_payloads"),
		read_command: private_evidence_read_command(
			project_config_path,
			issue_identifier.unwrap_or(issue_id),
			Some(run_id),
			Some(attempt_number),
			true,
			false,
		),
	}
}

fn private_evidence_read_command(
	project_config_path: &Path,
	issue_selector: &str,
	run_id: Option<&str>,
	attempt_number: Option<i64>,
	json: bool,
	include_payload: bool,
) -> String {
	let mut command = format!(
		"decodex evidence --config {} {}",
		shell_quote(&project_config_path.display().to_string()),
		shell_quote(issue_selector)
	);

	if let Some(run_id) = run_id {
		command.push_str(&format!(" --run-id {}", shell_quote(run_id)));
	}
	if let Some(attempt_number) = attempt_number {
		command.push_str(&format!(" --attempt {attempt_number}"));
	}

	if json {
		command.push_str(" --json");
	}
	if include_payload {
		command.push_str(" --include-payload");
	}

	command
}

pub(in crate::orchestrator) fn private_evidence_ref_for_parts(
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
) -> String {
	format!("private-evidence:{project_id}/{issue_id}/{run_id}/{attempt_number}")
}

pub(in crate::orchestrator) fn shell_quote(raw: &str) -> String {
	if !raw.is_empty()
		&& raw.bytes().all(|byte| {
			byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/' | b':')
		}) {
		return raw.to_owned();
	}

	format!("'{}'", raw.replace('\'', "'\\''"))
}

pub(in crate::orchestrator) fn build_private_evidence_readback(
	state_store: &StateStore,
	project: &ServiceConfig,
	request: &EvidenceRequest<'_>,
) -> Result<PrivateEvidenceReadback> {
	let target = resolve_private_evidence_target(
		state_store,
		project,
		request.issue,
		request.run_id,
		request.attempt_number,
	)?;
	let events = state_store.list_private_execution_events(
		project.service_id(),
		&target.issue_id,
		&target.run_id,
		target.attempt_number,
	)?;
	let latest_event = events.last();
	let warnings = if events.is_empty() {
		vec![String::from("private_execution_evidence_missing")]
	} else {
		Vec::new()
	};
	let issue_selector = target.issue_identifier.as_deref().unwrap_or(&target.issue_id).to_owned();
	let read_command = private_evidence_read_command(
		project.config_path(),
		&issue_selector,
		Some(&target.run_id),
		Some(target.attempt_number),
		true,
		request.include_payload,
	);

	Ok(PrivateEvidenceReadback {
		schema: PRIVATE_EVIDENCE_READBACK_SCHEMA,
		project_id: project.service_id().to_owned(),
		issue_selector: request.issue.to_owned(),
		issue_id: target.issue_id.clone(),
		issue_identifier: target.issue_identifier,
		run_id: target.run_id.clone(),
		attempt_number: target.attempt_number,
		source: "runtime_sqlite",
		evidence_ref: private_evidence_ref_for_parts(
			project.service_id(),
			&target.issue_id,
			&target.run_id,
			target.attempt_number,
		),
		read_command,
		payload_mode: if request.include_payload { "full_payloads" } else { "summarized_payloads" },
		event_count: events.len(),
		latest_event_type: latest_event.map(|event| event.event_type().to_owned()),
		latest_event_at: latest_event.map(|event| event.recorded_at().to_owned()),
		review_checkpoints: review_checkpoints_from_private_events(&events),
		repo_gate_failures: repo_gate_failures_from_private_events(&events),
		phase_acceptance_checks: phase_acceptance_checks_from_private_events(&events),
		boundary_checks: boundary_checks_from_private_events(&events),
		decision_requests: authority_decision_requests_from_private_events(&events),
		architecture_recoveries: architecture_recoveries_from_private_events(&events),
		improvement_candidates: harness_improvement_candidates_from_private_events(&events),
		events: events
			.iter()
			.map(|event| private_evidence_readback_event(event, request.include_payload))
			.collect(),
		warnings,
	})
}

fn resolve_private_evidence_target(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_selector: &str,
	run_id: Option<&str>,
	attempt_number: Option<i64>,
) -> Result<PrivateEvidenceTarget> {
	let (_, runs) = state_store.list_project_runs(project.service_id(), usize::MAX)?;
	let selector = issue_selector.trim();
	let matching_run = runs
		.iter()
		.filter(|run| private_evidence_run_matches_issue(project, run, selector))
		.filter(|run| run_id.is_none_or(|run_id| run.run_id() == run_id))
		.find(|run| attempt_number.is_none_or(|attempt| run.attempt_number() == attempt));

	if let Some(run) = matching_run {
		let branch_name = run.branch_name().map(str::to_owned);
		let worktree_path =
			run.worktree_path().map(|path| relative_worktree_path_for_path(project, path));
		let issue_identifier = operator_run_issue_identifier_from_fields(
			run.run_id(),
			branch_name.as_deref(),
			worktree_path.as_deref(),
		);

		return Ok(PrivateEvidenceTarget {
			issue_id: run.issue_id().to_owned(),
			issue_identifier,
			run_id: run.run_id().to_owned(),
			attempt_number: run.attempt_number(),
		});
	}
	if let (Some(run_id), Some(attempt_number)) = (run_id, attempt_number) {
		let events = state_store.list_private_execution_events_for_run_attempt(
			project.service_id(),
			run_id,
			attempt_number,
		)?;

		if let Some(issue_id) = private_evidence_direct_lookup_issue_id(&events, selector)? {
			return Ok(PrivateEvidenceTarget {
				issue_identifier: (issue_id != selector).then(|| selector.to_owned()),
				issue_id,
				run_id: run_id.to_owned(),
				attempt_number,
			});
		}

		return Ok(PrivateEvidenceTarget {
			issue_id: selector.to_owned(),
			issue_identifier: None,
			run_id: run_id.to_owned(),
			attempt_number,
		});
	}

	eyre::bail!(
		"No local run matched issue `{selector}` in project `{}`. Pass --run-id and --attempt for direct runtime-store lookup, or run `decodex status --json` to find local run ids.",
		project.service_id()
	)
}

fn private_evidence_direct_lookup_issue_id(
	events: &[state::PrivateExecutionEvent],
	selector: &str,
) -> Result<Option<String>> {
	let issue_ids = events
		.iter()
		.map(state::PrivateExecutionEvent::issue_id)
		.collect::<collections::BTreeSet<_>>();

	if issue_ids.is_empty() {
		return Ok(None);
	}
	if issue_ids.len() == 1 {
		return Ok(issue_ids.iter().next().map(|issue_id| (*issue_id).to_owned()));
	}
	if issue_ids.contains(selector) {
		return Ok(Some(selector.to_owned()));
	}

	eyre::bail!(
		"Direct private evidence lookup for issue `{selector}` matched multiple local issue ids for the supplied run and attempt; pass the local issue id from `decodex status --json`."
	)
}

fn review_checkpoints_from_private_events(
	events: &[state::PrivateExecutionEvent],
) -> Vec<PrivateEvidenceReviewCheckpointSummary> {
	events
		.iter()
		.filter(|event| event.event_type() == REVIEW_CHECKPOINT_EVENT_TYPE)
		.filter_map(review_checkpoint_from_private_event)
		.collect()
}

fn review_checkpoint_from_private_event(
	event: &state::PrivateExecutionEvent,
) -> Option<PrivateEvidenceReviewCheckpointSummary> {
	let payload = event.payload();
	let phase = payload.get("phase")?.as_str()?.to_owned();
	let status = payload.get("status")?.as_str()?.to_owned();
	let head_sha = payload.get("head_sha").and_then(Value::as_str).map(str::to_owned);
	let round =
		payload.get("nonclean_rounds").or_else(|| payload.get("round")).and_then(Value::as_u64);
	let (review_class, risk_class, compact_eligible, fallback_reason) =
		review_checkpoint_cost_control_summary(payload);
	let accepted_finding_count = payload
		.get("review")
		.and_then(|review| review.get("accepted_findings"))
		.or_else(|| payload.get("accepted_findings"))
		.and_then(Value::as_array)
		.map_or(0, Vec::len);
	let rejected_finding_count = payload
		.get("review")
		.and_then(|review| review.get("rejected_findings"))
		.or_else(|| payload.get("rejected_findings"))
		.and_then(Value::as_array)
		.map_or(0, Vec::len);
	let (active_fingerprints, stop_fingerprint) = review_checkpoint_fingerprint_summary(payload);
	let (route_counts, route_next_action) = review_checkpoint_route_summary(payload);
	let next_action = review_checkpoint_next_action(&status);

	Some(PrivateEvidenceReviewCheckpointSummary {
		phase,
		status,
		head_sha,
		round,
		review_class,
		risk_class,
		compact_eligible,
		fallback_reason,
		active_fingerprints,
		stop_fingerprint,
		accepted_finding_count,
		rejected_finding_count,
		route_counts,
		route_next_action,
		next_action,
	})
}

fn review_checkpoint_fingerprint_summary(payload: &Value) -> (Vec<String>, Option<String>) {
	let policy = payload.get("review").and_then(|review| review.get("finding_policy"));
	let active_source = payload
		.get("active_fingerprints")
		.or_else(|| policy.and_then(|policy| policy.get("active_fingerprints")));
	let active_fingerprints = active_source
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect();
	let stop_fingerprint = payload
		.get("stop_fingerprint")
		.and_then(Value::as_str)
		.or_else(|| {
			policy.and_then(|policy| policy.get("stop_fingerprint")).and_then(Value::as_str)
		})
		.map(str::to_owned);

	(active_fingerprints, stop_fingerprint)
}

fn review_checkpoint_cost_control_summary(
	payload: &Value,
) -> (Option<String>, Option<String>, Option<bool>, Option<String>) {
	let cost_control = payload.get("review").and_then(|review| review.get("review_cost_control"));
	let review_class = payload
		.get("review_class")
		.and_then(Value::as_str)
		.or_else(|| {
			cost_control
				.and_then(|cost_control| cost_control.get("review_class"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let risk_class = payload
		.get("risk_class")
		.and_then(Value::as_str)
		.or_else(|| {
			cost_control
				.and_then(|cost_control| cost_control.get("risk_class"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);
	let compact_eligible = payload.get("compact_eligible").and_then(Value::as_bool).or_else(|| {
		cost_control
			.and_then(|cost_control| cost_control.get("compact_eligible"))
			.and_then(Value::as_bool)
	});
	let fallback_reason = payload
		.get("review_fallback_reason")
		.and_then(Value::as_str)
		.or_else(|| {
			cost_control
				.and_then(|cost_control| cost_control.get("fallback_reason"))
				.and_then(Value::as_str)
		})
		.map(str::to_owned);

	(review_class, risk_class, compact_eligible, fallback_reason)
}

fn review_checkpoint_route_summary(
	payload: &Value,
) -> (Vec<PrivateEvidenceReviewRouteCount>, Option<String>) {
	let review = payload.get("review").unwrap_or(payload);
	let route_summary = review.get("finding_route_summary");
	let route_counts = payload
		.get("route_counts")
		.or_else(|| route_summary.and_then(|summary| summary.get("route_counts")))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(|count| {
			Some(PrivateEvidenceReviewRouteCount {
				route: count.get("route")?.as_str()?.to_owned(),
				count: usize::try_from(count.get("count")?.as_u64()?).ok()?,
			})
		})
		.collect();
	let route_next_action = payload
		.get("route_next_action")
		.and_then(Value::as_str)
		.or_else(|| {
			route_summary.and_then(|summary| summary.get("next_action")).and_then(Value::as_str)
		})
		.map(str::to_owned);

	(route_counts, route_next_action)
}

fn review_checkpoint_next_action(status: &str) -> String {
	match status {
		"clean" => String::from("Proceed with review handoff when repo gate evidence is current."),
		"findings" => String::from(
			"Repair accepted findings, rerun validation, and checkpoint the repaired head.",
		),
		"blocked" => String::from("Resolve the blocking review condition before continuing."),
		"needs_architecture_review" => {
			String::from("Escalate for an architecture decision before further repair churn.")
		},
		_ => String::from("Inspect the Decodex Review checkpoint summary before continuing."),
	}
}

fn repo_gate_failures_from_private_events(
	events: &[state::PrivateExecutionEvent],
) -> Vec<PrivateEvidenceRepoGateFailureSummary> {
	events.iter().filter_map(repo_gate_failure_from_private_event).collect()
}

fn repo_gate_failure_from_private_event(
	event: &state::PrivateExecutionEvent,
) -> Option<PrivateEvidenceRepoGateFailureSummary> {
	if event.event_type() != "phase_goal_transition" {
		return None;
	}

	let payload = event.payload();
	let transition_payload = payload.get("payload")?;
	let error_class = transition_payload.get("errorClass")?.as_str()?.to_owned();

	if !error_class.starts_with("repo_gate_") {
		return None;
	}

	let diagnostic = transition_payload.get("repoGateFailure");

	Some(PrivateEvidenceRepoGateFailureSummary {
		record_id: event.record_id(),
		phase: payload.get("phase")?.as_str()?.to_owned(),
		error_class,
		disposition: transition_payload.get("disposition")?.as_str()?.to_owned(),
		stage: diagnostic
			.and_then(|value| value.get("stage"))
			.and_then(Value::as_str)
			.map(str::to_owned),
		failed_command: diagnostic
			.and_then(|value| value.get("failed_command"))
			.and_then(Value::as_str)
			.map(str::to_owned),
		exit_status: diagnostic.and_then(|value| value.get("exit_status")).and_then(Value::as_i64),
		summary: diagnostic
			.and_then(|value| value.get("summary"))
			.and_then(Value::as_str)
			.map(str::to_owned),
		problem_lines: diagnostic
			.and_then(|value| value.get("problem_lines"))
			.and_then(Value::as_array)
			.into_iter()
			.flatten()
			.filter_map(Value::as_str)
			.map(str::to_owned)
			.collect(),
		output_excerpt: diagnostic
			.and_then(|value| value.get("output_excerpt"))
			.and_then(Value::as_str)
			.map(str::to_owned),
		output_truncated: diagnostic
			.and_then(|value| value.get("output_truncated"))
			.and_then(Value::as_bool),
	})
}

fn phase_acceptance_checks_from_private_events(
	events: &[state::PrivateExecutionEvent],
) -> Vec<PrivateEvidencePhaseAcceptanceSummary> {
	events
		.iter()
		.filter(|event| event.event_type() == PHASE_ACCEPTANCE_CHECK_EVENT_TYPE)
		.filter_map(phase_acceptance_check_from_private_event)
		.collect()
}

fn phase_acceptance_check_from_private_event(
	event: &state::PrivateExecutionEvent,
) -> Option<PrivateEvidencePhaseAcceptanceSummary> {
	let payload = event.payload();
	let phase = payload.get("phase")?.as_str()?.to_owned();
	let decision = payload.get("decision")?.as_str()?.to_owned();
	let reason_code = payload.get("reason_code")?.as_str()?.to_owned();
	let objective_covered = payload
		.get("objective_coverage")
		.and_then(|objective| objective.get("covered"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let effective_delta_present = payload
		.get("effective_delta")
		.and_then(|delta| delta.get("present"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let changed_surfaces = payload
		.get("effective_delta")
		.and_then(|delta| delta.get("changed_surfaces"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.map(str::to_owned)
		.collect();
	let non_goal_passed = payload
		.get("non_goal_check")
		.and_then(|check| check.get("passed"))
		.and_then(Value::as_bool)
		.unwrap_or(false);
	let validation_passed = payload
		.get("validation_evidence")
		.and_then(|evidence| evidence.get("repo_gate_passed"))
		.and_then(Value::as_bool)
		.unwrap_or(false);

	Some(PrivateEvidencePhaseAcceptanceSummary {
		phase,
		decision,
		reason_code,
		objective_covered,
		effective_delta_present,
		changed_surfaces,
		non_goal_passed,
		validation_passed,
		next_action: payload
			.get("next_action")
			.and_then(Value::as_str)
			.unwrap_or("inspect_phase_acceptance_check")
			.to_owned(),
	})
}

fn boundary_checks_from_private_events(
	events: &[state::PrivateExecutionEvent],
) -> Vec<PrivateEvidenceBoundaryCheckSummary> {
	events
		.iter()
		.filter(|event| event.event_type() == AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE)
		.filter_map(boundary_check_from_private_event)
		.collect()
}

fn boundary_check_from_private_event(
	event: &state::PrivateExecutionEvent,
) -> Option<PrivateEvidenceBoundaryCheckSummary> {
	let payload = event.payload();
	let disposition = payload.get("disposition")?.as_str()?.to_owned();
	let policy_decision = payload
		.get("policy_decision")
		.and_then(Value::as_str)
		.or_else(|| {
			payload.get("policy").and_then(|policy| policy.get("decision")).and_then(Value::as_str)
		})
		.map(str::to_owned)
		.unwrap_or_else(|| boundary_policy_decision_from_disposition(&disposition).to_owned());
	let reason = payload
		.get("final_disposition")
		.and_then(|final_disposition| final_disposition.get("reason"))
		.and_then(Value::as_str)
		.map(str::to_owned);
	let attempted_recovery_reason =
		payload.get("attempted_recovery_reason").and_then(Value::as_str).map(str::to_owned);
	let decision_contract_count =
		payload.get("decision_contract_ids").and_then(Value::as_array).map_or(0, Vec::len);
	let changed_surface_count =
		payload.get("changed_surfaces").and_then(Value::as_array).map_or(0, Vec::len);
	let improvement_signal_count =
		payload.get("improvement_signals").and_then(Value::as_array).map_or(0, Vec::len);
	let requires_enhanced_evidence = payload
		.get("policy")
		.and_then(|policy| policy.get("requires_enhanced_evidence"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| boundary_policy_requires_enhanced_evidence(&policy_decision));
	let blocks_landing = payload
		.get("policy")
		.and_then(|policy| policy.get("blocks_landing"))
		.and_then(Value::as_bool)
		.unwrap_or_else(|| boundary_policy_blocks_landing(&policy_decision));
	let next_action = boundary_check_next_action(&policy_decision);

	Some(PrivateEvidenceBoundaryCheckSummary {
		disposition,
		policy_decision,
		reason,
		attempted_recovery_reason,
		decision_contract_count,
		changed_surface_count,
		improvement_signal_count,
		requires_enhanced_evidence,
		blocks_landing,
		next_action,
	})
}

fn boundary_policy_decision_from_disposition(disposition: &str) -> &'static str {
	match disposition {
		"requires_human" | "insufficient_evidence" => "requires_human_decision",
		_ => "auto_continue",
	}
}

fn boundary_policy_requires_enhanced_evidence(policy_decision: &str) -> bool {
	matches!(policy_decision, "requires_enhanced_evidence" | "block_landing")
}

fn boundary_policy_blocks_landing(policy_decision: &str) -> bool {
	policy_decision == "block_landing"
}

fn boundary_check_next_action(policy_decision: &str) -> String {
	match policy_decision {
		"auto_continue" => {
			String::from("Continue autonomous architecture recovery inside the accepted boundary.")
		},
		"requires_enhanced_evidence" => String::from(
			"Continue recovery and preserve enhanced evidence before review handoff or landing.",
		),
		"block_landing" => String::from(
			"Continue recovery, but block landing until review or validation policy evidence is restored.",
		),
		"requires_human_decision" => {
			String::from("Stop for a human boundary decision before continuing.")
		},
		_ => String::from("Inspect the authority boundary summary before continuing."),
	}
}

fn authority_decision_requests_from_private_events(
	events: &[state::PrivateExecutionEvent],
) -> Vec<PrivateEvidenceDecisionRequestSummary> {
	events
		.iter()
		.filter(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE)
		.filter_map(authority_decision_request_from_private_event)
		.collect()
}

fn authority_decision_request_from_private_event(
	event: &state::PrivateExecutionEvent,
) -> Option<PrivateEvidenceDecisionRequestSummary> {
	let payload = event.payload();
	let decision_request_id = payload.get("decision_request_id")?.as_str()?.to_owned();
	let reason = payload.get("reason")?.as_str()?.to_owned();
	let boundary = payload.get("boundary")?.as_str()?.to_owned();
	let phase = payload.get("phase").and_then(Value::as_str).unwrap_or("human_required").to_owned();
	let next_action = payload
		.get("next_action")
		.or_else(|| payload.get("resume_condition"))?
		.as_str()?
		.to_owned();
	let recommendation = payload.get("recommendation").and_then(Value::as_str).map(str::to_owned);
	let resume_condition =
		payload.get("resume_condition").and_then(Value::as_str).map(str::to_owned);

	Some(PrivateEvidenceDecisionRequestSummary {
		decision_request_id,
		phase,
		reason,
		boundary,
		next_action,
		recommendation,
		resume_condition,
	})
}

fn architecture_recoveries_from_private_events(
	events: &[state::PrivateExecutionEvent],
) -> Vec<PrivateEvidenceArchitectureRecoverySummary> {
	events
		.iter()
		.filter(|event| {
			matches!(
				event.event_type(),
				ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE
					| ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE
					| ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE
			)
		})
		.filter_map(architecture_recovery_from_private_event)
		.collect()
}

fn architecture_recovery_from_private_event(
	event: &state::PrivateExecutionEvent,
) -> Option<PrivateEvidenceArchitectureRecoverySummary> {
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
				.map(boundary_policy_decision_from_disposition)
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
				.is_some_and(boundary_policy_requires_enhanced_evidence)
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
			boundary_policy_decision.as_deref().is_some_and(boundary_policy_blocks_landing)
		});
	let recovery_budget_attempt = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("attempt"))
		.and_then(Value::as_u64);
	let recovery_budget_max_attempts = payload
		.get("recovery_budget")
		.and_then(|budget| budget.get("max_attempts"))
		.and_then(Value::as_u64);
	let next_action = architecture_recovery_next_action(&reason_code);

	Some(PrivateEvidenceArchitectureRecoverySummary {
		reason_code,
		guardrail_reason,
		boundary_disposition,
		boundary_policy_decision,
		requires_enhanced_evidence,
		blocks_landing,
		recovery_budget_attempt,
		recovery_budget_max_attempts,
		next_action,
	})
}

fn architecture_recovery_next_action(reason_code: &str) -> String {
	match reason_code {
		"architecture_recovery_started" => String::from(
			"Retry with a materially different implementation strategy inside authority.",
		),
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

fn private_evidence_run_matches_issue(
	project: &ServiceConfig,
	run: &ProjectRunStatus,
	selector: &str,
) -> bool {
	if run.issue_id() == selector {
		return true;
	}

	let branch_name = run.branch_name().map(str::to_owned);
	let worktree_path =
		run.worktree_path().map(|path| relative_worktree_path_for_path(project, path));
	let issue_identifier = operator_run_issue_identifier_from_fields(
		run.run_id(),
		branch_name.as_deref(),
		worktree_path.as_deref(),
	);

	issue_identifier
		.as_deref()
		.is_some_and(|issue_identifier| issue_identifier.eq_ignore_ascii_case(selector))
}

fn private_evidence_readback_event(
	event: &state::PrivateExecutionEvent,
	include_payload: bool,
) -> PrivateEvidenceReadbackEvent {
	PrivateEvidenceReadbackEvent {
		record_id: event.record_id(),
		event_type: event.event_type().to_owned(),
		recorded_at: event.recorded_at().to_owned(),
		payload_summary: summarize_private_evidence_payload(event.payload()),
		payload: include_payload.then(|| event.payload().clone()),
	}
}

fn summarize_private_evidence_payload(payload: &Value) -> PrivateEvidencePayloadSummary {
	let encoded = serde_json::to_vec(payload).unwrap_or_default();
	let mut keys = Vec::new();
	let mut preview = Vec::new();
	let mut redacted_default_keys = Vec::new();
	let kind = match payload {
		Value::Object(object) => {
			for (key, value) in object {
				keys.push(key.clone());

				if private_evidence_payload_key_is_sensitive(key) {
					redacted_default_keys.push(key.clone());
					preview.push(format!("{key}=<redacted by default>"));
				} else {
					preview
						.push(format!("{key}={}", summarize_private_evidence_payload_value(value)));
				}
			}

			String::from("object")
		},
		Value::Array(values) => {
			preview.push(format!("array_len={}", values.len()));

			String::from("array")
		},
		Value::String(value) => {
			preview.push(truncate_private_evidence_payload_preview(value));

			String::from("string")
		},
		Value::Number(value) => {
			preview.push(value.to_string());

			String::from("number")
		},
		Value::Bool(value) => {
			preview.push(value.to_string());

			String::from("bool")
		},
		Value::Null => String::from("null"),
	};

	PrivateEvidencePayloadSummary {
		kind,
		byte_count: encoded.len(),
		keys,
		preview,
		redacted_default_keys,
	}
}

fn summarize_private_evidence_payload_value(value: &Value) -> String {
	match value {
		Value::Null => String::from("null"),
		Value::Bool(value) => value.to_string(),
		Value::Number(value) => value.to_string(),
		Value::String(value) => truncate_private_evidence_payload_preview(value),
		Value::Array(values) => format!("array(len={})", values.len()),
		Value::Object(object) => format!("object(keys={})", object.len()),
	}
}

fn private_evidence_payload_key_is_sensitive(key: &str) -> bool {
	let key = key.to_ascii_lowercase();

	key.contains("transcript")
		|| key.contains("message")
		|| key.contains("conversation")
		|| key.contains("raw")
		|| key.contains("stdout")
		|| key.contains("stderr")
		|| key.contains("log")
		|| key.contains("token")
		|| key.contains("secret")
}

fn truncate_private_evidence_payload_preview(value: &str) -> String {
	let mut preview = String::new();
	let mut truncated = false;

	for character in value.chars() {
		if preview.len() + character.len_utf8() > PRIVATE_EVIDENCE_PAYLOAD_PREVIEW_LIMIT {
			truncated = true;

			break;
		}

		preview.push(character);
	}

	if truncated {
		preview.push_str("...");
	}

	preview
}

pub(in crate::orchestrator) fn render_private_evidence_readback(
	readback: &PrivateEvidenceReadback,
) -> String {
	let mut output = String::new();

	append_private_evidence_readback_header(&mut output, readback);
	append_private_evidence_decision_requests(&mut output, &readback.decision_requests);
	append_private_evidence_review_checkpoints(&mut output, &readback.review_checkpoints);
	append_private_evidence_repo_gate_failures(&mut output, &readback.repo_gate_failures);
	append_private_evidence_phase_acceptance_checks(&mut output, &readback.phase_acceptance_checks);
	append_private_evidence_architecture_recoveries(&mut output, &readback.architecture_recoveries);
	append_private_evidence_boundary_checks(&mut output, &readback.boundary_checks);
	append_private_evidence_improvement_candidates(&mut output, &readback.improvement_candidates);
	append_private_evidence_events(&mut output, &readback.events);

	output
}

fn append_private_evidence_readback_header(
	output: &mut String,
	readback: &PrivateEvidenceReadback,
) {
	output.push_str(&format!("Project: {}\n", readback.project_id));
	output.push_str("Private Execution Evidence\n");
	output.push_str(&format!("issue_selector: {}\n", readback.issue_selector));
	output.push_str(&format!("issue_id: {}\n", readback.issue_id));
	output.push_str(&format!(
		"issue_identifier: {}\n",
		readback.issue_identifier.as_deref().unwrap_or("none")
	));
	output.push_str(&format!("run_id: {}\n", readback.run_id));
	output.push_str(&format!("attempt: {}\n", readback.attempt_number));
	output.push_str(&format!("source: {}\n", readback.source));
	output.push_str(&format!("evidence_ref: {}\n", readback.evidence_ref));
	output.push_str(&format!("payload_mode: {}\n", readback.payload_mode));
	output.push_str(&format!("event_count: {}\n", readback.event_count));
	output.push_str(&format!(
		"improvement_candidate_count: {}\n",
		readback.improvement_candidates.len()
	));
	output.push_str(&format!("decision_request_count: {}\n", readback.decision_requests.len()));
	output.push_str(&format!("review_checkpoint_count: {}\n", readback.review_checkpoints.len()));
	output.push_str(&format!(
		"architecture_recovery_count: {}\n",
		readback.architecture_recoveries.len()
	));
	output.push_str(&format!("boundary_check_count: {}\n", readback.boundary_checks.len()));
	output.push_str(&format!(
		"latest_event_type: {}\n",
		readback.latest_event_type.as_deref().unwrap_or("none")
	));
	output.push_str(&format!(
		"latest_event_at: {}\n",
		readback.latest_event_at.as_deref().unwrap_or("none")
	));

	if !readback.warnings.is_empty() {
		output.push_str(&format!("warnings: {}\n", readback.warnings.join(", ")));
	}
}

fn append_private_evidence_decision_requests(
	output: &mut String,
	decision_requests: &[PrivateEvidenceDecisionRequestSummary],
) {
	output.push_str("\nDecision Requests\n");

	if decision_requests.is_empty() {
		output.push_str("- none\n");
	} else {
		for request in decision_requests {
			output.push_str(&format!(
				"- id: {}\n  phase: {}\n  reason: {}\n  boundary: {}\n  next_action: {}\n",
				request.decision_request_id,
				request.phase,
				request.reason,
				request.boundary,
				request.next_action
			));
		}
	}
}

fn append_private_evidence_review_checkpoints(
	output: &mut String,
	review_checkpoints: &[PrivateEvidenceReviewCheckpointSummary],
) {
	output.push_str("\nReview Checkpoints\n");

	if review_checkpoints.is_empty() {
		output.push_str("- none\n");
	} else {
		for checkpoint in review_checkpoints {
			let active_fingerprints = if checkpoint.active_fingerprints.is_empty() {
				String::from("none")
			} else {
				checkpoint.active_fingerprints.join(", ")
			};
			let route_counts = if checkpoint.route_counts.is_empty() {
				String::from("none")
			} else {
				checkpoint
					.route_counts
					.iter()
					.map(|count| format!("{}={}", count.route, count.count))
					.collect::<Vec<_>>()
					.join(", ")
			};

			output.push_str(&format!(
				"- phase: {}\n  status: {}\n  head_sha: {}\n  round: {}\n  review_class: {}\n  risk_class: {}\n  compact_eligible: {}\n  review_fallback_reason: {}\n  active_fingerprints: {}\n  stop_fingerprint: {}\n  accepted_findings: {}\n  rejected_findings: {}\n  route_counts: {}\n  route_next_action: {}\n  next_action: {}\n",
				checkpoint.phase,
				checkpoint.status,
				checkpoint.head_sha.as_deref().unwrap_or("none"),
				checkpoint
					.round
					.map_or_else(|| String::from("none"), |round| round.to_string()),
				checkpoint.review_class.as_deref().unwrap_or("none"),
				checkpoint.risk_class.as_deref().unwrap_or("none"),
				checkpoint.compact_eligible.map_or("none", |eligible| {
					if eligible {
						"true"
					} else {
						"false"
					}
				}),
				checkpoint.fallback_reason.as_deref().unwrap_or("none"),
				active_fingerprints,
				checkpoint.stop_fingerprint.as_deref().unwrap_or("none"),
				checkpoint.accepted_finding_count,
				checkpoint.rejected_finding_count,
				route_counts,
				checkpoint.route_next_action.as_deref().unwrap_or("none"),
				checkpoint.next_action
			));
		}
	}
}

fn append_private_evidence_repo_gate_failures(
	output: &mut String,
	failures: &[PrivateEvidenceRepoGateFailureSummary],
) {
	if failures.is_empty() {
		return;
	}

	output.push_str("\nRepo Gate Failures\n");

	for failure in failures {
		let problem_lines = if failure.problem_lines.is_empty() {
			String::from("none")
		} else {
			failure.problem_lines.join(" | ")
		};

		output.push_str(&format!(
			"- record_id: {}\n  phase: {}\n  error_class: {}\n  disposition: {}\n  stage: {}\n  failed_command: {}\n  exit_status: {}\n  summary: {}\n  problem_lines: {}\n",
			failure.record_id,
			failure.phase,
			failure.error_class,
			failure.disposition,
			failure.stage.as_deref().unwrap_or("none"),
			failure.failed_command.as_deref().unwrap_or("none"),
			failure
				.exit_status
				.map_or_else(|| String::from("none"), |status| status.to_string()),
			failure.summary.as_deref().unwrap_or("none"),
			problem_lines
		));
	}
}

fn append_private_evidence_phase_acceptance_checks(
	output: &mut String,
	checks: &[PrivateEvidencePhaseAcceptanceSummary],
) {
	if checks.is_empty() {
		return;
	}

	output.push_str("Phase Acceptance Checks\n");

	for check in checks {
		let surfaces = if check.changed_surfaces.is_empty() {
			String::from("none")
		} else {
			check.changed_surfaces.join(",")
		};

		output.push_str(&format!(
			"- phase: {}\n  decision: {}\n  reason_code: {}\n  objective_covered: {}\n  effective_delta: {}\n  changed_surfaces: {}\n  non_goal_passed: {}\n  validation_passed: {}\n  next_action: {}\n",
			check.phase,
			check.decision,
			check.reason_code,
			check.objective_covered,
			check.effective_delta_present,
			surfaces,
			check.non_goal_passed,
			check.validation_passed,
			check.next_action
		));
	}
}

fn append_private_evidence_architecture_recoveries(
	output: &mut String,
	architecture_recoveries: &[PrivateEvidenceArchitectureRecoverySummary],
) {
	output.push_str("\nArchitecture Recoveries\n");

	if architecture_recoveries.is_empty() {
		output.push_str("- none\n");
	} else {
		for recovery in architecture_recoveries {
			output.push_str(&format!(
				"- reason_code: {}\n  guardrail_reason: {}\n  boundary_disposition: {}\n  boundary_policy: {}\n  enhanced_evidence: {}\n  blocks_landing: {}\n  budget: {}/{}\n  next_action: {}\n",
				recovery.reason_code,
				recovery.guardrail_reason.as_deref().unwrap_or("none"),
				recovery.boundary_disposition.as_deref().unwrap_or("none"),
				recovery
					.boundary_policy_decision
					.as_deref()
					.unwrap_or("none"),
				recovery.requires_enhanced_evidence,
				recovery.blocks_landing,
				recovery
					.recovery_budget_attempt
					.map_or_else(|| String::from("none"), |attempt| attempt.to_string()),
				recovery
					.recovery_budget_max_attempts
					.map_or_else(|| String::from("none"), |max_attempts| max_attempts.to_string()),
				recovery.next_action
			));
		}
	}
}

fn append_private_evidence_boundary_checks(
	output: &mut String,
	boundary_checks: &[PrivateEvidenceBoundaryCheckSummary],
) {
	output.push_str("\nBoundary Checks\n");

	if boundary_checks.is_empty() {
		output.push_str("- none\n");
	} else {
		for boundary in boundary_checks {
			output.push_str(&format!(
				"- disposition: {}\n  policy: {}\n  enhanced_evidence: {}\n  blocks_landing: {}\n  reason: {}\n  attempted_recovery: {}\n  decision_contracts: {}\n  changed_surfaces: {}\n  improvement_signals: {}\n  next_action: {}\n",
				boundary.disposition,
				boundary.policy_decision,
				boundary.requires_enhanced_evidence,
				boundary.blocks_landing,
				boundary.reason.as_deref().unwrap_or("none"),
				boundary
					.attempted_recovery_reason
					.as_deref()
					.unwrap_or("none"),
				boundary.decision_contract_count,
				boundary.changed_surface_count,
				boundary.improvement_signal_count,
				boundary.next_action
			));
		}
	}
}

fn append_private_evidence_improvement_candidates(
	output: &mut String,
	improvement_candidates: &[HarnessImprovementCandidateSummary],
) {
	output.push_str("\nImprovement Candidates\n");

	if improvement_candidates.is_empty() {
		output.push_str("- none\n");
	} else {
		for candidate in improvement_candidates {
			output.push_str(&format!(
				"- kind: {}\n  reason_code: {}\n  target: {}\n  source_event_count: {}\n  recommendation: {}\n",
				candidate.kind,
				candidate.reason_code,
				candidate.target,
				candidate.source_event_count,
				candidate.recommendation
			));
		}
	}
}

fn append_private_evidence_events(output: &mut String, events: &[PrivateEvidenceReadbackEvent]) {
	output.push_str("\nEvents\n");

	if events.is_empty() {
		output.push_str("- none\n");

		return;
	}

	for event in events {
		output.push_str(&format!(
			"- record_id: {}\n  event_type: {}\n  recorded_at: {}\n  payload: {}\n",
			event.record_id,
			event.event_type,
			event.recorded_at,
			render_private_evidence_payload_summary(&event.payload_summary)
		));

		if let Some(payload) = &event.payload {
			output.push_str(&format!("  full_payload: {}\n", payload));
		}
	}
}

pub(in crate::orchestrator) fn render_private_evidence_payload_summary(
	summary: &PrivateEvidencePayloadSummary,
) -> String {
	let keys = if summary.keys.is_empty() { String::from("none") } else { summary.keys.join(",") };
	let preview =
		if summary.preview.is_empty() { String::from("none") } else { summary.preview.join("; ") };
	let redacted = if summary.redacted_default_keys.is_empty() {
		String::from("none")
	} else {
		summary.redacted_default_keys.join(",")
	};

	format!(
		"kind={} bytes={} keys={} preview={} redacted_default_keys={}",
		summary.kind, summary.byte_count, keys, preview, redacted
	)
}
