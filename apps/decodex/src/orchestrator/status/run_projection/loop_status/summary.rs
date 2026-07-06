use crate::orchestrator::{
	OperatorArchitectureRecoveryStatus, OperatorAuthorityDecisionRequestStatus,
	OperatorBoundaryStatus, OperatorReviewLoopStatus,
};

pub(crate) fn operator_loop_autonomy(
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

pub(crate) fn operator_loop_status_summary(
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

pub(crate) fn operator_loop_status_next_action(
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
