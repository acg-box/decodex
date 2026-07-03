use crate::{
	orchestrator::status::post_review::{
		AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE, AUTHORITY_DECISION_REQUEST_EVENT_TYPE,
		PostReviewLaneClassification, PostReviewLaneDecision, PostReviewLaneSnapshot,
		PostReviewRuntimeState, PrivateExecutionEvent, ReviewHandoffMarker, Value,
		operator_boundary_policy_blocks_landing,
		operator_boundary_policy_requires_enhanced_evidence,
	},
	prelude::Result,
};

pub(crate) fn apply_authority_boundary_landing_policy(
	snapshot: &PostReviewLaneSnapshot,
	classification: &mut PostReviewLaneClassification,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> Result<()> {
	if classification.decision != PostReviewLaneDecision::ReadyToLand {
		return Ok(());
	}

	let Some(reason) = authority_boundary_landing_requirement(snapshot, runtime_state)? else {
		return Ok(());
	};

	classification.decision = if reason == "authority_boundary_requires_human_decision" {
		PostReviewLaneDecision::Block
	} else {
		PostReviewLaneDecision::NeedsReviewRepair
	};
	classification.reason = reason.to_owned();

	Ok(())
}

pub(crate) fn authority_boundary_landing_requirement(
	snapshot: &PostReviewLaneSnapshot,
	runtime_state: Option<PostReviewRuntimeState<'_>>,
) -> Result<Option<&'static str>> {
	let Some(runtime_state) = runtime_state else {
		return Ok(None);
	};
	let events = runtime_state
		.state_store
		.list_private_execution_events_for_issue(runtime_state.project_id, &snapshot.issue.id)?;

	if events.iter().any(|event| event.event_type() == AUTHORITY_DECISION_REQUEST_EVENT_TYPE) {
		return Ok(Some("authority_boundary_requires_human_decision"));
	}
	if events.iter().rev().any(|event| {
		event.event_type() == AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE
			&& authority_boundary_event_requires_human_decision(event.payload())
	}) {
		return Ok(Some("authority_boundary_requires_human_decision"));
	}

	let latest_clean_review_record_id = events
		.iter()
		.rev()
		.find(|event| authority_boundary_clearance_review_checkpoint(event, snapshot))
		.map_or(0, PrivateExecutionEvent::record_id);

	for event in events.iter().rev() {
		if event.record_id() <= latest_clean_review_record_id
			|| event.event_type() != AUTHORITY_BOUNDARY_CHECK_EVENT_TYPE
		{
			continue;
		}

		if let Some(reason) = authority_boundary_event_landing_requirement(event.payload()) {
			return Ok(Some(reason));
		}
	}

	Ok(None)
}

pub(crate) fn authority_boundary_clearance_review_checkpoint(
	event: &PrivateExecutionEvent,
	snapshot: &PostReviewLaneSnapshot,
) -> bool {
	if event.event_type() != "review_checkpoint"
		|| event.payload().get("status").and_then(Value::as_str) != Some("clean")
	{
		return false;
	}

	let Some(checkpoint_head) = event.payload().get("head_sha").and_then(Value::as_str) else {
		return false;
	};
	let expected_head = snapshot
		.local_head_oid
		.as_deref()
		.or_else(|| snapshot.review_handoff.as_ref().map(ReviewHandoffMarker::pr_head_oid));

	expected_head == Some(checkpoint_head)
}

pub(crate) fn authority_boundary_event_blocks_landing(payload: &Value) -> bool {
	payload
		.get("policy")
		.and_then(|policy| policy.get("blocks_landing"))
		.and_then(Value::as_bool)
		.or_else(|| payload.get("blocks_landing").and_then(Value::as_bool))
		.unwrap_or_else(|| {
			authority_boundary_event_policy_decision(payload)
				.is_some_and(operator_boundary_policy_blocks_landing)
		})
}

pub(crate) fn authority_boundary_event_requires_enhanced_evidence(payload: &Value) -> bool {
	payload
		.get("policy")
		.and_then(|policy| policy.get("requires_enhanced_evidence"))
		.and_then(Value::as_bool)
		.or_else(|| payload.get("requires_enhanced_evidence").and_then(Value::as_bool))
		.unwrap_or_else(|| {
			authority_boundary_event_policy_decision(payload)
				.is_some_and(operator_boundary_policy_requires_enhanced_evidence)
		})
}

pub(crate) fn authority_boundary_event_landing_requirement(
	payload: &Value,
) -> Option<&'static str> {
	if authority_boundary_event_blocks_landing(payload) {
		return Some("authority_boundary_blocks_landing");
	}
	if authority_boundary_event_requires_enhanced_evidence(payload) {
		return Some("authority_boundary_requires_enhanced_evidence");
	}

	None
}

pub(crate) fn authority_boundary_event_requires_human_decision(payload: &Value) -> bool {
	authority_boundary_event_policy_decision(payload)
		.is_some_and(|policy_decision| policy_decision == "requires_human_decision")
		|| payload
			.get("policy")
			.and_then(|policy| policy.get("requires_human_decision"))
			.and_then(Value::as_bool)
			.unwrap_or(false)
		|| matches!(
			payload.get("disposition").and_then(Value::as_str).or_else(|| {
				payload
					.get("final_disposition")
					.and_then(|final_disposition| final_disposition.get("disposition"))
					.and_then(Value::as_str)
			}),
			Some("requires_human" | "insufficient_evidence")
		)
}

pub(crate) fn authority_boundary_event_policy_decision(payload: &Value) -> Option<&str> {
	payload.get("policy_decision").and_then(Value::as_str).or_else(|| {
		payload.get("policy").and_then(|policy| policy.get("decision")).and_then(Value::as_str)
	})
}
