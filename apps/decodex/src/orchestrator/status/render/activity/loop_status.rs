use crate::orchestrator::{OperatorLoopStatus, OperatorRunControlCapability};

pub(crate) fn render_loop_status_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(status) = status else {
		return String::from("none");
	};
	let next_action = status.next_action.as_deref().unwrap_or("none");
	let autonomy_objective = status
		.autonomy_objective
		.as_ref()
		.map(|objective| objective.source_ref.as_str())
		.unwrap_or("none");
	let autonomy_report =
		status.autonomy_report.as_ref().map(|report| report.authority.as_str()).unwrap_or("none");

	format!(
		"{}; review_level={}; autonomy={}; autonomy_objective={autonomy_objective}; autonomy_signals={}; autonomy_proposals={}; report={autonomy_report}; next_action={next_action}",
		status.summary,
		status.review_level,
		status.autonomy,
		status.autonomy_signals.len(),
		status.autonomy_proposals.len()
	)
}

pub(crate) fn render_loop_autonomy_signals_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(status) = status else {
		return String::from("none");
	};

	if status.autonomy_signals.is_empty() {
		return String::from("none");
	}

	status
		.autonomy_signals
		.iter()
		.map(|signal| {
			format!(
				"{}:{}@v{} freshness={} confidence={} privacy={} sources={} completeness={} gaps={} contradictions={}",
				signal.kind,
				signal.objective_id,
				signal.objective_version,
				signal.freshness,
				signal.confidence,
				signal.privacy,
				signal.source_refs.len(),
				signal.completeness,
				signal.gaps.len(),
				signal.contradictions.len()
			)
		})
		.collect::<Vec<_>>()
		.join(";")
}

pub(crate) fn render_loop_review_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(review) = status.and_then(|status| status.review.as_ref()) else {
		return String::from("none");
	};
	let checkpoint = review.checkpoint.as_ref().map_or_else(
		|| String::from("checkpoint=none"),
		|checkpoint| {
			format!(
				"checkpoint=head:{} round:{} review_class:{} risk_class:{} compact_eligible:{} fallback:{} updated:{}",
				checkpoint.head_sha,
				checkpoint.round,
				checkpoint.review_class.as_deref().unwrap_or("none"),
				checkpoint.risk_class.as_deref().unwrap_or("none"),
				checkpoint
					.compact_eligible
					.map_or("none", |eligible| if eligible { "true" } else { "false" }),
				checkpoint.fallback_reason.as_deref().unwrap_or("none"),
				checkpoint.updated_at
			)
		},
	);

	format!("phase={} status={} {checkpoint}", review.phase, review.status)
}

pub(crate) fn render_loop_architecture_recovery_summary(
	status: Option<&OperatorLoopStatus>,
) -> String {
	let Some(recovery) = status.and_then(|status| status.architecture_recovery.as_ref()) else {
		return String::from("none");
	};
	let budget = recovery.budget.as_ref().map_or_else(
		|| String::from("none"),
		|budget| format!("{}/{}", budget.attempt, budget.max_attempts),
	);

	format!(
		"status={} reason={} guardrail={} boundary={} policy={} enhanced_evidence={} blocks_landing={} budget={} next_action={}",
		recovery.status,
		recovery.reason_code,
		recovery.guardrail_reason.as_deref().unwrap_or("none"),
		recovery.boundary_disposition.as_deref().unwrap_or("none"),
		recovery.boundary_policy_decision.as_deref().unwrap_or("none"),
		recovery.requires_enhanced_evidence,
		recovery.blocks_landing,
		budget,
		recovery.next_action
	)
}

pub(crate) fn render_loop_boundary_summary(status: Option<&OperatorLoopStatus>) -> String {
	let Some(boundary) = status.and_then(|status| status.boundary.as_ref()) else {
		return String::from("none");
	};

	format!(
		"disposition={} policy={} enhanced_evidence={} blocks_landing={} reason={} attempted_recovery={} changed_surfaces={} improvement_signals={}",
		boundary.disposition,
		boundary.policy_decision,
		boundary.requires_enhanced_evidence,
		boundary.blocks_landing,
		boundary.reason.as_deref().unwrap_or("none"),
		boundary.attempted_recovery_reason.as_deref().unwrap_or("none"),
		boundary.changed_surface_count,
		boundary.improvement_signal_count
	)
}

pub(crate) fn render_control_capability_summary(
	capability: Option<&OperatorRunControlCapability>,
) -> String {
	let Some(capability) = capability else {
		return String::from("none");
	};
	let thread_id = capability.thread_id.as_deref().unwrap_or("none");
	let turn_id = capability.turn_id.as_deref().unwrap_or("none");

	format!(
		"status={}; transport={}; channel={}; thread_id={thread_id}; turn_id={turn_id}",
		capability.status, capability.transport, capability.channel_path
	)
}
