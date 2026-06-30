use super::{
	ARCHITECTURE_RECOVERY_BUDGET, ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE,
	ARCHITECTURE_RECOVERY_PACKET_SCHEMA, ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
	ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE, ArchitectureRecoveryStart,
	AuthorityBoundaryChangedSurface, AuthorityBoundaryCheckInput, AuthorityBoundaryDisposition,
	AuthorityBoundaryImprovementSignal, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface,
	AuthorityDecisionOption, AuthorityDecisionRequestInput, ExecutionProgramRecord, IssueRunPlan,
	LOOP_GUARDRAIL_CONVERGENCE_BUDGET, LoopGuardrailReason, LoopGuardrailRecoveryDecision,
	LoopGuardrailStopRequested, Path, RepoGateFailure, RepoGateFailureDisposition, Report, Result,
	ServiceConfig, StateStore, Value, git_guardrail_output, json, loop_guardrail_effective_status,
	loop_guardrail_worktree_fingerprint, record_authority_boundary_check_private_event,
	record_authority_decision_request_private_event, truncate_private_diagnostic_text,
};

use crate::state::DecisionContractRecord;

struct ArchitectureRecoveryBoundary {
	disposition: AuthorityBoundaryDisposition,
	policy_decision: AuthorityBoundaryPolicyDecision,
	final_reason: &'static str,
	boundary_type: AuthorityBoundarySurface,
}

struct ArchitectureRecoveryPacketInput<'a> {
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
	loop_guardrail_stop: &'a LoopGuardrailStopRequested,
	error: &'a Report,
	contracts: &'a [DecisionContractRecord],
	boundary_check_record_id: i64,
	boundary_disposition: AuthorityBoundaryDisposition,
	boundary_policy_decision: AuthorityBoundaryPolicyDecision,
	boundary_final_reason: &'a str,
	reason_code: &'a str,
	recovery_attempt_number: usize,
	prior_started_count: usize,
}

struct ArchitectureRecoveryTerminalEventInput<'a> {
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
	stop: &'a LoopGuardrailStopRequested,
	boundary_check_record_id: i64,
	boundary_disposition: AuthorityBoundaryDisposition,
	boundary_policy_decision: AuthorityBoundaryPolicyDecision,
	boundary_final_reason: &'a str,
	reason_code: &'a str,
	recovery_attempt_number: usize,
}

pub(super) fn loop_guardrail_architecture_recovery_decision(
	project: &ServiceConfig,
	state_store: &StateStore,
	issue_run: &IssueRunPlan,
	mut loop_guardrail_stop: LoopGuardrailStopRequested,
	error: &Report,
) -> Result<LoopGuardrailRecoveryDecision> {
	let prior_started_count = architecture_recovery_started_count(state_store, project, issue_run)?;
	let recovery_attempt_number = prior_started_count.saturating_add(1);
	let boundary = classify_loop_guardrail_authority_boundary(&loop_guardrail_stop, error);
	let changed_surfaces =
		architecture_recovery_changed_surfaces(&boundary, &issue_run.worktree.path);
	let policy_decision = architecture_recovery_policy_decision(&changed_surfaces);
	let disposition = policy_decision.disposition();
	let final_reason = architecture_recovery_final_reason(&boundary, policy_decision);
	let contracts = architecture_recovery_contracts_for_issue(state_store, project, issue_run)?;
	let decision_contract_ids =
		contracts.iter().map(|contract| contract.contract_id().to_owned()).collect::<Vec<_>>();
	let decision_contract_id_refs =
		decision_contract_ids.iter().map(String::as_str).collect::<Vec<_>>();
	let boundary_event = record_authority_boundary_check_private_event(
		state_store,
		AuthorityBoundaryCheckInput {
			project_id: project.service_id(),
			issue_id: &issue_run.issue.id,
			issue_identifier: &issue_run.issue.identifier,
			run_id: &issue_run.run_id,
			attempt_number: issue_run.attempt_number,
			decision_contract_ids: decision_contract_id_refs,
			attempted_recovery_reason: loop_guardrail_stop.reason.error_class(),
			changed_surfaces,
			policy_decision,
			disposition,
			final_disposition_reason: final_reason,
			improvement_signals: architecture_recovery_improvement_signals(
				loop_guardrail_stop.reason,
				&boundary,
			),
		},
	)?;
	let budget_exhausted = prior_started_count >= ARCHITECTURE_RECOVERY_BUDGET;
	let reason_code =
		architecture_recovery_reason_code(&boundary, policy_decision, budget_exhausted);

	record_architecture_recovery_packet(
		state_store,
		ArchitectureRecoveryPacketInput {
			project,
			issue_run,
			loop_guardrail_stop: &loop_guardrail_stop,
			error,
			contracts: &contracts,
			boundary_check_record_id: boundary_event.record_id(),
			boundary_disposition: disposition,
			boundary_policy_decision: policy_decision,
			boundary_final_reason: final_reason,
			reason_code,
			recovery_attempt_number,
			prior_started_count,
		},
	)?;

	if budget_exhausted || !policy_decision.allows_autonomous_recovery() {
		loop_guardrail_stop.architecture_recovery_reason_code = Some(reason_code.to_owned());

		record_architecture_recovery_terminal_outcome(
			state_store,
			ArchitectureRecoveryTerminalEventInput {
				project,
				issue_run,
				stop: &loop_guardrail_stop,
				boundary_check_record_id: boundary_event.record_id(),
				boundary_disposition: disposition,
				boundary_policy_decision: policy_decision,
				boundary_final_reason: final_reason,
				reason_code,
				recovery_attempt_number,
			},
		)?;

		return Ok(LoopGuardrailRecoveryDecision::HumanRequired(loop_guardrail_stop));
	}

	state_store.clear_loop_guardrail_checkpoint(
		project.service_id(),
		&issue_run.issue.id,
		loop_guardrail_stop.reason.error_class(),
	)?;

	record_architecture_recovery_started_event(
		state_store,
		project,
		issue_run,
		&loop_guardrail_stop,
		boundary_event.record_id(),
		policy_decision,
		recovery_attempt_number,
	)?;

	Ok(LoopGuardrailRecoveryDecision::Start(ArchitectureRecoveryStart {
		attempt_number: recovery_attempt_number,
		max_attempts: ARCHITECTURE_RECOVERY_BUDGET,
		policy_decision,
		detail: architecture_recovery_goal_detail(
			&loop_guardrail_stop,
			recovery_attempt_number,
			policy_decision,
		),
	}))
}

fn classify_loop_guardrail_authority_boundary(
	stop: &LoopGuardrailStopRequested,
	error: &Report,
) -> ArchitectureRecoveryBoundary {
	let source_is_repo_gate =
		stop.source_error_class.as_deref().is_some_and(|class| class.starts_with("repo_gate_"))
			|| error.downcast_ref::<RepoGateFailure>().is_some_and(|failure| {
				failure.disposition() == RepoGateFailureDisposition::ContinueRepair
			});

	match stop.reason {
		LoopGuardrailReason::ValidationRepeat | LoopGuardrailReason::RemainingDeltaUnchanged
			if source_is_repo_gate =>
		{
			ArchitectureRecoveryBoundary {
				disposition: AuthorityBoundaryDisposition::WithinAuthority,
				policy_decision: AuthorityBoundaryPolicyDecision::AutoContinue,
				final_reason: "Repo-gate convergence failed on an engineering implementation problem; architecture recovery may change implementation strategy without weakening validation.",
				boundary_type: AuthorityBoundarySurface::ImplementationStrategy,
			}
		},
		LoopGuardrailReason::NoEffectiveDiff if source_is_repo_gate => {
			ArchitectureRecoveryBoundary {
				disposition: AuthorityBoundaryDisposition::WithinAuthority,
				policy_decision: AuthorityBoundaryPolicyDecision::AutoContinue,
				final_reason: "No-effective-diff convergence followed repo-gate repair work; architecture recovery may replace the ineffective implementation strategy.",
				boundary_type: AuthorityBoundarySurface::ImplementationStrategy,
			}
		},
		LoopGuardrailReason::ReviewChurn => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::WithinAuthority,
			policy_decision: AuthorityBoundaryPolicyDecision::BlockLanding,
			final_reason: "Review churn can be recovered autonomously only by changing implementation architecture while preserving accepted behavior and review standards.",
			boundary_type: AuthorityBoundarySurface::ReviewPolicy,
		},
		LoopGuardrailReason::DependencyProgramStale => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "The next viable action changes dependency or Execution Program readiness and requires accepted authority.",
			boundary_type: AuthorityBoundarySurface::ExternalDependency,
		},
		LoopGuardrailReason::UncoveredDirection => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "Execution uncovered missing direction that changes the accepted Decision Contract.",
			boundary_type: AuthorityBoundarySurface::Objective,
		},
		LoopGuardrailReason::AmbiguousRetainedProgress => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "Retained progress ownership is underspecified, so Decodex lacks evidence that recovery is inside authority.",
			boundary_type: AuthorityBoundarySurface::RetainedOwnership,
		},
		_ => ArchitectureRecoveryBoundary {
			disposition: AuthorityBoundaryDisposition::RequiresHuman,
			policy_decision: AuthorityBoundaryPolicyDecision::RequiresHumanDecision,
			final_reason: "Guardrail evidence is insufficient to prove autonomous recovery stays inside the Authority Envelope.",
			boundary_type: AuthorityBoundarySurface::AuthorityEvidence,
		},
	}
}

fn architecture_recovery_started_count(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
) -> Result<usize> {
	Ok(state_store
		.list_private_execution_events_for_issue(project.service_id(), &issue_run.issue.id)?
		.iter()
		.filter(|event| event.event_type() == ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE)
		.count())
}

fn architecture_recovery_contracts_for_issue(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
) -> Result<Vec<DecisionContractRecord>> {
	let mut records = Vec::new();

	for issue_id in [&issue_run.issue.id, &issue_run.issue.identifier] {
		for record in
			state_store.list_decision_contracts_for_issue(project.service_id(), issue_id)?
		{
			if records.iter().all(|existing: &DecisionContractRecord| {
				existing.contract_id() != record.contract_id()
			}) {
				records.push(record);
			}
		}
	}

	records.sort_by(|left, right| left.contract_id().cmp(right.contract_id()));

	Ok(records)
}

fn architecture_recovery_changed_surfaces(
	boundary: &ArchitectureRecoveryBoundary,
	worktree_path: &Path,
) -> Vec<AuthorityBoundaryChangedSurface<'static>> {
	let mut surfaces = Vec::new();

	push_architecture_recovery_changed_surface(
		&mut surfaces,
		boundary.boundary_type,
		"Replace the non-converging guardrail repair strategy with a materially different architecture recovery strategy.",
		boundary.policy_decision,
		boundary.disposition,
	);

	if let Ok(Some(diff_paths)) =
		git_guardrail_output(worktree_path, &["diff", "--name-only", "HEAD", "--"])
	{
		for relative_path in diff_paths.lines().filter(|path| !path.trim().is_empty()) {
			for surface in architecture_recovery_surfaces_for_path(relative_path) {
				push_architecture_recovery_changed_surface(
					&mut surfaces,
					surface,
					architecture_recovery_surface_summary(surface),
					surface.policy_decision(),
					surface.policy_decision().disposition(),
				);
			}
		}
	}

	surfaces
}

fn push_architecture_recovery_changed_surface(
	surfaces: &mut Vec<AuthorityBoundaryChangedSurface<'static>>,
	surface: AuthorityBoundarySurface,
	change_summary: &'static str,
	policy_decision: AuthorityBoundaryPolicyDecision,
	legacy_disposition: AuthorityBoundaryDisposition,
) {
	if surfaces.iter().any(|existing| existing.surface == surface) {
		return;
	}

	surfaces.push(AuthorityBoundaryChangedSurface {
		surface,
		change_summary,
		policy_decision,
		legacy_disposition,
	});
}

fn architecture_recovery_surfaces_for_path(relative_path: &str) -> Vec<AuthorityBoundarySurface> {
	let normalized = relative_path.replace('\\', "/");
	let lower = normalized.to_ascii_lowercase();
	let mut surfaces = Vec::new();

	if lower.starts_with("docs/") {
		surfaces.push(AuthorityBoundarySurface::Docs);

		return surfaces;
	}
	if architecture_recovery_path_is_test(&lower) {
		surfaces.push(AuthorityBoundarySurface::Tests);

		return surfaces;
	}
	if architecture_recovery_path_is_config(&lower) {
		surfaces.push(AuthorityBoundarySurface::Config);

		return surfaces;
	}
	if architecture_recovery_path_is_public_api(&lower) {
		surfaces.push(AuthorityBoundarySurface::PublicApi);
	}
	if architecture_recovery_path_is_security(&lower) {
		surfaces.push(AuthorityBoundarySurface::Security);
	}
	if architecture_recovery_path_is_privacy(&lower) {
		surfaces.push(AuthorityBoundarySurface::Privacy);
	}
	if architecture_recovery_path_is_data(&lower) {
		surfaces.push(AuthorityBoundarySurface::Data);
	}
	if architecture_recovery_path_is_billing(&lower) {
		surfaces.push(AuthorityBoundarySurface::Billing);
	}
	if architecture_recovery_path_is_validation(&lower) {
		surfaces.push(AuthorityBoundarySurface::Validation);
	}
	if architecture_recovery_path_is_review_policy(&lower) {
		surfaces.push(AuthorityBoundarySurface::ReviewPolicy);
	}
	if surfaces.is_empty() && architecture_recovery_path_is_runtime(&lower) {
		surfaces.push(AuthorityBoundarySurface::Runtime);
	}

	surfaces
}

fn architecture_recovery_path_is_test(path: &str) -> bool {
	path.starts_with("tests/")
		|| path.contains("/tests/")
		|| path.ends_with("_test.rs")
		|| path.ends_with("tests.rs")
		|| path.contains("/test_")
}

fn architecture_recovery_path_is_config(path: &str) -> bool {
	path == "cargo.toml"
		|| path == "cargo.lock"
		|| path == "makefile.toml"
		|| path == "clippy.toml"
		|| path == "rust-toolchain.toml"
		|| path == "decodex.example.toml"
		|| path.starts_with(".github/")
		|| path.ends_with(".toml")
		|| path.ends_with(".yaml")
		|| path.ends_with(".yml")
		|| path.ends_with(".json")
		|| path.ends_with(".env")
}

fn architecture_recovery_path_is_public_api(path: &str) -> bool {
	architecture_recovery_path_has_segment(path, "cli")
		|| architecture_recovery_path_has_segment(path, "mcp")
		|| architecture_recovery_path_has_segment(path, "protocol")
		|| architecture_recovery_path_has_segment(path, "api")
		|| path.contains("tracker_tool_bridge")
		|| path.contains("app_bridge")
}

fn architecture_recovery_path_is_security(path: &str) -> bool {
	path.contains("auth")
		|| path.contains("credential")
		|| path.contains("secret")
		|| path.contains("security")
		|| path.contains("signing")
		|| path.contains("token")
}

fn architecture_recovery_path_is_privacy(path: &str) -> bool {
	path.contains("privacy") || path.contains("public_text") || path.contains("redact")
}

fn architecture_recovery_path_is_data(path: &str) -> bool {
	path.contains("database")
		|| path.contains("migration")
		|| path.contains("payload")
		|| path.contains("record")
		|| path.contains("sqlite")
		|| path.contains("state")
}

fn architecture_recovery_path_is_billing(path: &str) -> bool {
	path.contains("account")
		|| path.contains("billing")
		|| path.contains("credit")
		|| path.contains("invoice")
		|| path.contains("usage")
}

fn architecture_recovery_path_is_validation(path: &str) -> bool {
	path.contains("repo_gate")
		|| path.contains("validation")
		|| path.contains("validator")
		|| path.contains("verify")
}

fn architecture_recovery_path_is_review_policy(path: &str) -> bool {
	path.contains("review_policy") || path.contains("review_landing") || path.contains("landing")
}

fn architecture_recovery_path_is_runtime(path: &str) -> bool {
	path.starts_with("apps/") || path.starts_with("scripts/") || path.starts_with("dev/")
}

fn architecture_recovery_path_has_segment(path: &str, segment: &str) -> bool {
	path.split('/')
		.any(|part| part == segment || part.strip_suffix(".rs").is_some_and(|stem| stem == segment))
}

fn architecture_recovery_surface_summary(surface: AuthorityBoundarySurface) -> &'static str {
	match surface {
		AuthorityBoundarySurface::ImplementationStrategy => {
			"Replace the non-converging guardrail repair strategy with a materially different architecture recovery strategy."
		},
		AuthorityBoundarySurface::Runtime => {
			"Runtime implementation files changed during recovery."
		},
		AuthorityBoundarySurface::Tests => "Test files changed during recovery.",
		AuthorityBoundarySurface::Docs => "Documentation files changed during recovery.",
		AuthorityBoundarySurface::PublicApi => {
			"Public API or command surface files changed during recovery."
		},
		AuthorityBoundarySurface::Config => "Configuration files changed during recovery.",
		AuthorityBoundarySurface::Security => {
			"Security-sensitive implementation files changed during recovery."
		},
		AuthorityBoundarySurface::Data => {
			"Data or state persistence files changed during recovery."
		},
		AuthorityBoundarySurface::Billing => "Billing or usage files changed during recovery.",
		AuthorityBoundarySurface::Privacy => "Privacy-sensitive files changed during recovery.",
		AuthorityBoundarySurface::Validation => {
			"Validation or repository-gate files changed during recovery."
		},
		AuthorityBoundarySurface::ReviewPolicy => {
			"Review policy or landing policy files changed during recovery."
		},
		AuthorityBoundarySurface::Objective => {
			"Objective-changing recovery requires an explicit human decision."
		},
		AuthorityBoundarySurface::NonGoal => {
			"Non-goal-changing recovery requires an explicit human decision."
		},
		AuthorityBoundarySurface::ExternalDependency => {
			"External dependency recovery requires accepted authority."
		},
		AuthorityBoundarySurface::RetainedOwnership => {
			"Retained ownership evidence changed during recovery."
		},
		AuthorityBoundarySurface::AuthorityEvidence => {
			"Authority evidence changed or is insufficient during recovery."
		},
	}
}

fn architecture_recovery_policy_decision(
	surfaces: &[AuthorityBoundaryChangedSurface<'_>],
) -> AuthorityBoundaryPolicyDecision {
	surfaces.iter().fold(AuthorityBoundaryPolicyDecision::AutoContinue, |decision, surface| {
		AuthorityBoundaryPolicyDecision::max(decision, surface.policy_decision)
	})
}

fn architecture_recovery_final_reason(
	boundary: &ArchitectureRecoveryBoundary,
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	if policy_decision == boundary.policy_decision {
		return boundary.final_reason;
	}

	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue => boundary.final_reason,
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence => {
			"Changed high-risk surfaces can continue recovery autonomously, but require enhanced evidence before review handoff or landing."
		},
		AuthorityBoundaryPolicyDecision::BlockLanding => {
			"Changed validation or review-policy surfaces can continue recovery autonomously, but block landing until the required evidence is restored."
		},
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision => boundary.final_reason,
	}
}

fn architecture_recovery_improvement_signals(
	reason: LoopGuardrailReason,
	boundary: &ArchitectureRecoveryBoundary,
) -> Vec<AuthorityBoundaryImprovementSignal<'static>> {
	match boundary.disposition {
		AuthorityBoundaryDisposition::WithinAuthority => match reason {
			LoopGuardrailReason::ValidationRepeat
			| LoopGuardrailReason::RemainingDeltaUnchanged => {
				vec![AuthorityBoundaryImprovementSignal {
					kind: "missing_validator",
					reason_code: "validation_guardrail_repeated",
					target: "validator:repo_gate",
					recommendation: "Promote the repeated repo-gate failure into an earlier deterministic validator or fixture.",
				}]
			},
			_ => vec![AuthorityBoundaryImprovementSignal {
				kind: "weak_prompt",
				reason_code: "architecture_recovery_strategy_needed",
				target: "prompt:phase_goal_repair",
				recommendation: "Prompt recovery agents to replace the ineffective strategy instead of repeating patch-only repair.",
			}],
		},
		AuthorityBoundaryDisposition::RequiresHuman => vec![AuthorityBoundaryImprovementSignal {
			kind: "underspecified_decision_contract",
			reason_code: "contract_boundary_required",
			target: "decision_contract:authority_envelope",
			recommendation: "Record explicit accepted authority before retrying autonomous recovery.",
		}],
		AuthorityBoundaryDisposition::InsufficientEvidence => {
			vec![AuthorityBoundaryImprovementSignal {
				kind: "underspecified_decision_contract",
				reason_code: "authority_evidence_missing",
				target: "issue_template:loop_recovery",
				recommendation: "Capture retained ownership, validation, and Decision Contract evidence before recovery.",
			}]
		},
	}
}

fn architecture_recovery_reason_code(
	boundary: &ArchitectureRecoveryBoundary,
	policy_decision: AuthorityBoundaryPolicyDecision,
	budget_exhausted: bool,
) -> &'static str {
	if budget_exhausted {
		"architecture_recovery_exhausted"
	} else if boundary.boundary_type == AuthorityBoundarySurface::ExternalDependency {
		"external_dependency_required"
	} else if policy_decision.allows_autonomous_recovery() {
		"architecture_recovery_started"
	} else {
		"contract_boundary_required"
	}
}

fn record_architecture_recovery_packet(
	state_store: &StateStore,
	input: ArchitectureRecoveryPacketInput<'_>,
) -> Result<()> {
	let programs = architecture_recovery_programs_for_contracts(
		state_store,
		input.project.service_id(),
		input.contracts,
	)?;
	let retained = architecture_recovery_retained_worktree(&input.issue_run.worktree.path)?;
	let review =
		architecture_recovery_review_findings(state_store, input.project, input.issue_run)?;

	state_store
		.append_private_execution_event(
			input.project.service_id(),
			&input.issue_run.issue.id,
			&input.issue_run.run_id,
			input.issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_PACKET_EVENT_TYPE,
			json!({
				"schema": ARCHITECTURE_RECOVERY_PACKET_SCHEMA,
				"record_version": 1,
				"state": input.reason_code,
				"reason_code": input.reason_code,
				"issue": architecture_recovery_issue_payload(input.issue_run),
				"run": architecture_recovery_run_payload(input.issue_run),
				"decision_contract_context": input.contracts
					.iter()
					.map(architecture_recovery_contract_payload)
					.collect::<Vec<_>>(),
				"execution_program_context": programs
					.iter()
					.map(architecture_recovery_program_payload)
					.collect::<Vec<_>>(),
				"retained_worktree": retained,
				"validation_failures": architecture_recovery_validation_failures(
					input.loop_guardrail_stop,
					input.error,
				),
				"review_findings": review,
				"prior_recovery_attempts": {
					"started_count": input.prior_started_count,
				},
				"recovery_budget": {
					"attempt": input.recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
				"loop_guardrail": {
					"reason": input.loop_guardrail_stop.reason.error_class(),
					"consecutive_count": input.loop_guardrail_stop.consecutive_count,
					"threshold": LOOP_GUARDRAIL_CONVERGENCE_BUDGET,
					"fingerprint": input.loop_guardrail_stop.fingerprint.as_str(),
					"source_error_class": input.loop_guardrail_stop.source_error_class.as_deref(),
				},
				"authority_boundary_check": {
					"record_id": input.boundary_check_record_id,
					"disposition": input.boundary_disposition.as_str(),
					"policy_decision": input.boundary_policy_decision.as_str(),
					"requires_enhanced_evidence": input
						.boundary_policy_decision
						.requires_enhanced_evidence(),
					"blocks_landing": input.boundary_policy_decision.blocks_landing(),
					"reason": input.boundary_final_reason,
				},
			}),
		)
		.map(|_| ())
}

fn architecture_recovery_programs_for_contracts(
	state_store: &StateStore,
	project_id: &str,
	contracts: &[DecisionContractRecord],
) -> Result<Vec<ExecutionProgramRecord>> {
	let mut programs = Vec::new();

	for contract in contracts {
		for program in
			state_store.list_execution_programs_for_contract(project_id, contract.contract_id())?
		{
			if programs.iter().all(|existing: &ExecutionProgramRecord| {
				existing.program_id() != program.program_id()
			}) {
				programs.push(program);
			}
		}
	}

	programs.sort_by(|left, right| left.program_id().cmp(right.program_id()));

	Ok(programs)
}

fn architecture_recovery_retained_worktree(worktree_path: &Path) -> Result<Value> {
	let fingerprint = loop_guardrail_worktree_fingerprint(worktree_path)?;
	let tracked_status =
		git_guardrail_output(worktree_path, &["status", "--porcelain", "--untracked-files=no"])?;
	let raw_status = git_guardrail_output(worktree_path, &["status", "--porcelain"])?;
	let effective_status = raw_status.as_deref().map(loop_guardrail_effective_status);
	let diff_stat =
		git_guardrail_output(worktree_path, &["diff", "--stat", "--no-ext-diff", "HEAD", "--"])?;

	Ok(json!({
		"head_sha": fingerprint.as_ref().map(|value| value.head_sha.as_str()),
		"tracked_status_hash": fingerprint
			.as_ref()
			.map(|value| value.tracked_status_hash.as_str()),
		"tracked_diff_hash": fingerprint.as_ref().map(|value| value.tracked_diff_hash.as_str()),
		"effective_status_hash": fingerprint
			.as_ref()
			.map(|value| value.effective_status_hash.as_str()),
		"effective_delta_present": fingerprint
			.as_ref()
			.map(|value| value.effective_delta_present),
		"tracked_status": tracked_status.unwrap_or_default(),
		"effective_status": effective_status.unwrap_or_default(),
		"diff_stat": diff_stat.unwrap_or_default(),
	}))
}

fn architecture_recovery_review_findings(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
) -> Result<Value> {
	let events = state_store
		.list_private_execution_events_for_issue(project.service_id(), &issue_run.issue.id)?;
	let latest_review = events
		.iter()
		.rev()
		.find(|event| event.event_type() == "review_checkpoint")
		.map(|event| event.payload());
	let Some(payload) = latest_review else {
		return Ok(json!({
			"latest_status": null,
			"accepted_finding_count": 0,
			"rejected_finding_count": 0,
		}));
	};
	let review = payload.get("review").unwrap_or(payload);
	let route_summary = review.get("finding_route_summary");

	Ok(json!({
		"latest_status": payload.get("status").and_then(Value::as_str),
		"accepted_finding_count": review
			.get("accepted_findings")
			.and_then(Value::as_array)
			.map_or(0, Vec::len),
		"rejected_finding_count": review
			.get("rejected_findings")
			.and_then(Value::as_array)
			.map_or(0, Vec::len),
		"route_counts": route_summary
			.and_then(|summary| summary.get("route_counts"))
			.cloned()
			.unwrap_or_else(|| json!([])),
		"route_next_action": route_summary
			.and_then(|summary| summary.get("next_action"))
			.and_then(Value::as_str),
		"nonclean_rounds": payload.get("nonclean_rounds").and_then(Value::as_i64).unwrap_or(0),
	}))
}

fn architecture_recovery_issue_payload(issue_run: &IssueRunPlan) -> Value {
	json!({
		"id": issue_run.issue.id.as_str(),
		"identifier": issue_run.issue.identifier.as_str(),
		"title": issue_run.issue.title.as_str(),
	})
}

fn architecture_recovery_run_payload(issue_run: &IssueRunPlan) -> Value {
	json!({
		"run_id": issue_run.run_id.as_str(),
		"attempt_number": issue_run.attempt_number,
		"branch": issue_run.worktree.branch_name.as_str(),
		"dispatch_mode": issue_run.dispatch_mode.as_str(),
	})
}

fn architecture_recovery_contract_payload(record: &DecisionContractRecord) -> Value {
	json!({
		"contract_id": record.contract_id(),
		"source_issue_id": record.source_issue_id(),
		"status": record.status().as_str(),
		"updated_at": record.updated_at(),
	})
}

fn architecture_recovery_program_payload(record: &ExecutionProgramRecord) -> Value {
	json!({
		"program_id": record.program_id(),
		"source_contract_id": record.source_contract_id(),
		"updated_at": record.updated_at(),
	})
}

fn architecture_recovery_validation_failures(
	stop: &LoopGuardrailStopRequested,
	error: &Report,
) -> Value {
	json!({
		"guardrail_reason": stop.reason.error_class(),
		"source_error_class": stop.source_error_class.as_deref(),
		"error_summary": truncate_private_diagnostic_text(&error.to_string()),
	})
}

fn record_architecture_recovery_started_event(
	state_store: &StateStore,
	project: &ServiceConfig,
	issue_run: &IssueRunPlan,
	stop: &LoopGuardrailStopRequested,
	boundary_check_record_id: i64,
	boundary_policy_decision: AuthorityBoundaryPolicyDecision,
	recovery_attempt_number: usize,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			project.service_id(),
			&issue_run.issue.id,
			&issue_run.run_id,
			issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_STARTED_EVENT_TYPE,
			json!({
				"schema": "decodex.architecture_recovery_started/1",
				"record_version": 1,
				"reason_code": "architecture_recovery_started",
				"guardrail_reason": stop.reason.error_class(),
				"authority_boundary_check_record_id": boundary_check_record_id,
				"boundary_policy_decision": boundary_policy_decision.as_str(),
				"requires_enhanced_evidence": boundary_policy_decision.requires_enhanced_evidence(),
				"blocks_landing": boundary_policy_decision.blocks_landing(),
				"recovery_budget": {
					"attempt": recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
				"next_strategy": "materially_different_architecture_recovery",
			}),
		)
		.map(|_| ())
}

fn record_architecture_recovery_terminal_outcome(
	state_store: &StateStore,
	input: ArchitectureRecoveryTerminalEventInput<'_>,
) -> Result<()> {
	record_architecture_recovery_terminal_event(state_store, &input)?;

	if input.boundary_policy_decision.allows_autonomous_recovery() {
		return Ok(());
	}

	let decision_request_id = format!(
		"{}-{}-{}-{}",
		input.issue_run.issue.identifier,
		input.issue_run.run_id,
		input.issue_run.attempt_number,
		input.reason_code
	);

	record_authority_decision_request_private_event(
		state_store,
		architecture_recovery_decision_request_input(
			input.project,
			input.issue_run,
			input.stop,
			input.boundary_check_record_id,
			&decision_request_id,
			input.reason_code,
			input.boundary_final_reason,
		),
	)
	.map(|_| ())
}

fn record_architecture_recovery_terminal_event(
	state_store: &StateStore,
	input: &ArchitectureRecoveryTerminalEventInput<'_>,
) -> Result<()> {
	state_store
		.append_private_execution_event(
			input.project.service_id(),
			&input.issue_run.issue.id,
			&input.issue_run.run_id,
			input.issue_run.attempt_number,
			ARCHITECTURE_RECOVERY_TERMINAL_EVENT_TYPE,
			json!({
				"schema": "decodex.architecture_recovery_terminal/1",
				"record_version": 1,
				"reason_code": input.reason_code,
				"guardrail_reason": input.stop.reason.error_class(),
				"authority_boundary_check_record_id": input.boundary_check_record_id,
				"boundary_disposition": input.boundary_disposition.as_str(),
				"boundary_policy_decision": input.boundary_policy_decision.as_str(),
				"requires_enhanced_evidence": input
					.boundary_policy_decision
					.requires_enhanced_evidence(),
				"blocks_landing": input.boundary_policy_decision.blocks_landing(),
				"recovery_budget": {
					"attempt": input.recovery_attempt_number,
					"max_attempts": ARCHITECTURE_RECOVERY_BUDGET,
				},
			}),
		)
		.map(|_| ())
}

fn architecture_recovery_decision_request_input<'a>(
	project: &'a ServiceConfig,
	issue_run: &'a IssueRunPlan,
	stop: &'a LoopGuardrailStopRequested,
	boundary_check_record_id: i64,
	decision_request_id: &'a str,
	reason_code: &'a str,
	final_reason: &'a str,
) -> AuthorityDecisionRequestInput<'a> {
	AuthorityDecisionRequestInput {
		project_id: project.service_id(),
		issue_id: &issue_run.issue.id,
		issue_identifier: &issue_run.issue.identifier,
		run_id: &issue_run.run_id,
		attempt_number: issue_run.attempt_number,
		boundary_check_record_id,
		decision_request_id,
		reason_code,
		boundary_type: "architecture_recovery",
		proposed_change: "Continue loop recovery with a materially different architecture strategy.",
		why_exceeds_authority: final_reason,
		options: vec![
			AuthorityDecisionOption {
				label: "Authorize recovery",
				description: "Update the issue, Decision Contract, or policy to allow this recovery.",
			},
			AuthorityDecisionOption {
				label: "Keep stopped",
				description: "Leave the lane in manual attention until the boundary is resolved.",
			},
		],
		recommendation: "Resolve the authority boundary before requeueing the lane.",
		resume_condition: "Accept, reject, or revise the requested authority in the issue, Decision Contract, or project policy before clearing needs-attention.",
		retained_worktree_evidence: vec![issue_run.worktree.branch_name.as_str()],
		retained_diff_evidence: vec![stop.fingerprint.as_str()],
		recovery_attempt_context: vec![stop.reason.error_class()],
	}
}

fn architecture_recovery_goal_detail(
	stop: &LoopGuardrailStopRequested,
	recovery_attempt_number: usize,
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> String {
	format!(
		"Loop guardrail `{}` stopped the current ineffective strategy after {} matching observations. Decodex recorded an Architecture Recovery Packet and an Authority Boundary Check with policy `{}`; use autonomous architecture recovery attempt {} of {}. Start a materially different implementation strategy, preserve the accepted Decision Contract and all validation/review gates, and {}.",
		stop.reason.error_class(),
		stop.consecutive_count,
		policy_decision.as_str(),
		recovery_attempt_number,
		ARCHITECTURE_RECOVERY_BUDGET,
		architecture_recovery_policy_recovery_guidance(policy_decision)
	)
}

fn architecture_recovery_policy_recovery_guidance(
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue => {
			"request human attention only if the next viable action would change product behavior, public API/config contract, security, data, credential, billing, validation standards, or accepted authority"
		},
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence => {
			"preserve enhanced evidence for the changed high-risk surfaces before review handoff or landing"
		},
		AuthorityBoundaryPolicyDecision::BlockLanding => {
			"keep landing blocked until validation or review-policy evidence is restored"
		},
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision => {
			"request human attention before continuing recovery"
		},
	}
}

pub(super) fn architecture_recovery_retry_next_action(
	policy_decision: AuthorityBoundaryPolicyDecision,
) -> &'static str {
	match policy_decision {
		AuthorityBoundaryPolicyDecision::AutoContinue => {
			"decodex recorded authority policy `auto_continue` and will retry with a materially different architecture recovery strategy"
		},
		AuthorityBoundaryPolicyDecision::RequiresEnhancedEvidence => {
			"decodex recorded authority policy `requires_enhanced_evidence` and will retry with a materially different architecture recovery strategy while preserving enhanced evidence before review handoff or landing"
		},
		AuthorityBoundaryPolicyDecision::BlockLanding => {
			"decodex recorded authority policy `block_landing` and will retry with a materially different architecture recovery strategy while landing remains blocked until validation or review-policy evidence is restored"
		},
		AuthorityBoundaryPolicyDecision::RequiresHumanDecision => {
			"decodex recorded authority policy `requires_human_decision` and requires human attention before retrying"
		},
	}
}
