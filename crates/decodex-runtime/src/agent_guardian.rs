//! Bounded review projection; only the daemon can retrieve original approval payloads.
use serde_json::Value;

use decodex_codex::guardian;
use decodex_core::MAX_NATIVE_MESSAGE_BYTES;
use decodex_database::{AgentGuardianReview, SqliteStore};
use decodex_protocol::{
	AgentGuardianDetailResult, AgentGuardianReviewDto, AgentGuardianReviewsResult,
	AgentGuardianStatus, AgentGuardianSubmission, GUARDIAN_DETAIL_PAGE_BYTES,
};

/// Read complete details from the existing saved observation. No native request is sent.
pub(crate) async fn detail(
	store: &SqliteStore,
	work: &str,
	row_id: i64,
	digest: &str,
	offset: usize,
) -> AgentGuardianDetailResult {
	let Ok(Some(row)) = store.agent_guardian_review(work.into(), row_id).await else {
		return AgentGuardianDetailResult::Unavailable;
	};

	if row.digest() != digest {
		return AgentGuardianDetailResult::Unavailable;
	}

	let Ok(event) = serde_json::from_str::<Value>(&row.event_json) else {
		return AgentGuardianDetailResult::Unavailable;
	};
	let method = if row.status == "inProgress" {
		"item/autoApprovalReview/started"
	} else {
		"item/autoApprovalReview/completed"
	};

	if guardian::decode_review(method, &event).is_none() {
		return AgentGuardianDetailResult::Unavailable;
	}

	let action = match serde_json::to_string(&event["action"]) {
		Ok(action) => action,
		Err(_) => return AgentGuardianDetailResult::Unavailable,
	};
	let rationale = event["review"]["rationale"].as_str().unwrap_or("");

	if decodex_core::contains_credential_material(&action)
		|| decodex_core::contains_credential_material(rationale)
	{
		return AgentGuardianDetailResult::Unavailable;
	}

	let text = format!("Action\n{action}\n\nFindings\n{rationale}");

	if text.len() > MAX_NATIVE_MESSAGE_BYTES
		|| offset >= text.len()
		|| !text.is_char_boundary(offset)
	{
		return AgentGuardianDetailResult::Unavailable;
	}

	let mut end = offset.saturating_add(GUARDIAN_DETAIL_PAGE_BYTES).min(text.len());

	while !text.is_char_boundary(end) {
		end -= 1;
	}

	AgentGuardianDetailResult::Available {
		row_id,
		digest: digest.into(),
		offset,
		total_bytes: text.len(),
		text: text[offset..end].into(),
		next_offset: (end < text.len()).then_some(end),
	}
}

pub(crate) async fn read(
	store: &SqliteStore,
	work: &str,
	before: Option<i64>,
	generation: Option<String>,
) -> AgentGuardianReviewsResult {
	let Ok(owner) = store.get_agent_work_item(work.into()).await else {
		return AgentGuardianReviewsResult::Unavailable;
	};
	let Ok(saved) = store.read_agent_guardian_reviews(work.into(), before, 9).await else {
		return AgentGuardianReviewsResult::Unavailable;
	};
	let mut reviews = Vec::new();
	let mut bytes: usize = 0;
	let mut more = false;

	for row in saved {
		if reviews.len() == 8 {
			more = true;

			break;
		}

		let Some(mut review) = project(&row, generation.as_deref()) else {
			return AgentGuardianReviewsResult::Unavailable;
		};

		if owner.active_turn_id.as_ref().is_some_and(|turn| turn != &row.turn_id)
			|| !matches!(
				owner.dispatch_state,
				decodex_database::AgentDispatchState::Idle
					| decodex_database::AgentDispatchState::Running
			) {
			review.can_approve = false;

			if review.status == AgentGuardianStatus::Denied {
				review.approval_unavailable =
					Some("The task has moved to another turn or is reconnecting.".into());
			}
		}

		let cost = serde_json::to_vec(&review).map_or(usize::MAX, |v| v.len());

		if bytes.saturating_add(cost) > 128 * 1_024 {
			more = true;

			break;
		}

		bytes += cost;

		reviews.push(review);
	}

	let next_before = more.then(|| reviews.last().map(|r| r.row_id)).flatten();

	AgentGuardianReviewsResult::Available { reviews, next_before }
}

fn project(row: &AgentGuardianReview, generation: Option<&str>) -> Option<AgentGuardianReviewDto> {
	let event = serde_json::from_str(&row.event_json).ok()?;
	let observed = guardian::decode_review(
		if row.status == "inProgress" {
			"item/autoApprovalReview/started"
		} else {
			"item/autoApprovalReview/completed"
		},
		&event,
	)?;
	let status = match row.status.as_str() {
		"inProgress" => AgentGuardianStatus::InProgress,
		"approved" => AgentGuardianStatus::Approved,
		"denied" => AgentGuardianStatus::Denied,
		"timedOut" => AgentGuardianStatus::TimedOut,
		"aborted" => AgentGuardianStatus::Aborted,
		_ => return None,
	};
	let submission = match row.approval_state.as_deref() {
		Some("pending") => Some(AgentGuardianSubmission::Pending),
		Some("submitted") => Some(AgentGuardianSubmission::Submitted),
		Some("rejected") => Some(AgentGuardianSubmission::Rejected),
		None => None,
		_ => return None,
	};
	let label = match event["action"]["type"].as_str() {
		Some("command") => "Shell command",
		Some("execve") => "Child process",
		Some("writeStdin") => "Terminal input",
		Some("applyPatch") => "File changes",
		Some("networkAccess") => "Network access",
		Some("mcpToolCall") => "MCP tool",
		Some("requestPermissions") => "Permission request",
		_ => "Other action",
	};
	let mut result = AgentGuardianReviewDto {
		row_id: row.id,
		digest: row.digest(),
		action_label: label.into(),
		status,
		risk_level: event["review"]["riskLevel"].as_str().map(str::to_owned),
		user_authorization: event["review"]["userAuthorization"].as_str().map(str::to_owned),
		rationale: event["review"]["rationale"].as_str().map(str::to_owned),
		action_json: Some(serde_json::to_string(&event["action"]).ok()?),
		details_unavailable: None,
		details_paged: false,
		current_process: generation.is_some() && generation == row.generation_id.as_deref(),
		submission,
		submission_key: row.approval_key.clone(),
		can_approve: false,
		approval_unavailable: None,
	};

	if result.action_json.as_deref().is_some_and(decodex_core::contains_credential_material)
		|| result.rationale.as_deref().is_some_and(decodex_core::contains_credential_material)
	{
		result.action_json = None;
		result.rationale = None;
		result.details_unavailable =
			Some("Review details contain credential material and cannot be displayed here.".into());
	} else if serde_json::to_vec(&result).ok()?.len() > 96 * 1_024 {
		result.action_json = None;
		result.rationale = None;
		result.details_paged = true;
	}
	if status == AgentGuardianStatus::Denied {
		result.approval_unavailable =
			if row.conflicted {
				Some("Conflicting review evidence was received. This saved action cannot be approved.".into())
			} else if matches!(
				submission,
				Some(AgentGuardianSubmission::Pending | AgentGuardianSubmission::Submitted)
			) {
				Some("An approval submission is already recorded.".into())
			} else if result.details_unavailable.is_some() {
				Some("Complete action details are required before approval.".into())
			} else if guardian::core_denial_event(&observed).is_none() {
				Some("This action format cannot be submitted without losing review details.".into())
			} else {
				None
			};
		result.can_approve = result.approval_unavailable.is_none();
	}

	Some(result)
}

#[cfg(test)]
mod tests {

	use crate::agent_guardian::{self, AgentGuardianReview, AgentGuardianStatus};
	fn denial() -> AgentGuardianReview {
		AgentGuardianReview {id:1,thread_id:"thread".into(),turn_id:"turn".into(),review_id:"review".into(),
			connection_id:"connection".into(),generation_id:Some("generation".into()),status:"denied".into(),
			event_json:serde_json::json!({"threadId":"thread","turnId":"turn","reviewId":"review","targetItemId":null,"startedAtMs":1,"completedAtMs":2,"decisionSource":"agent","review":{"status":"denied","riskLevel":"high","userAuthorization":"low","rationale":"User did not request this action."},"action":{"type":"command","source":"shell","command":"echo fixture","cwd":"/tmp"}}).to_string(),
			conflicted:false,approval_state:None,approval_key:None}
	}
	#[test]
	fn review_and_user_submission_are_separate_and_bound_to_exact_evidence() {
		let mut row = denial();
		let result = agent_guardian::project(&row, Some("generation")).unwrap();

		assert!(result.can_approve && result.current_process);
		assert_eq!(result.digest, row.digest());

		for state in ["pending", "submitted", "rejected"] {
			row.approval_state = Some(state.into());
			row.approval_key = Some("exact-command".into());

			let result = agent_guardian::project(&row, Some("new-generation")).unwrap();

			assert_eq!(result.status, AgentGuardianStatus::Denied);
			assert!(!result.current_process);
			assert_eq!(result.can_approve, state == "rejected");
			assert_eq!(result.submission_key.as_deref(), Some("exact-command"));
		}

		row.conflicted = true;

		assert!(!agent_guardian::project(&row, None).unwrap().can_approve);
	}
	#[test]
	fn withheld_or_unknown_action_details_cannot_be_approved() {
		{
			let command = "sk-".to_owned() + &"A".repeat(60);
			let mut row = denial();
			let mut event: serde_json::Value = serde_json::from_str(&row.event_json).unwrap();

			event["action"]["command"] = serde_json::json!(command);
			row.event_json = event.to_string();

			let result = agent_guardian::project(&row, None).unwrap();

			assert!(
				!result.can_approve
					&& result.action_json.is_none()
					&& result.details_unavailable.is_some()
			);
			assert!(serde_json::to_string(&result).unwrap().len() < 4_096);
		}

		let mut row = denial();
		let mut event: serde_json::Value = serde_json::from_str(&row.event_json).unwrap();

		event["action"]["futureField"] = serde_json::json!(true);
		row.event_json = event.to_string();

		let result = agent_guardian::project(&row, None).unwrap();

		assert!(!result.can_approve && result.action_json.unwrap().contains("futureField"));
	}

	#[test]
	fn large_action_details_use_complete_paging() {
		for command in ["x".repeat(100_000), "界".repeat(100_000) + " exact-required-suffix"] {
			let mut row = denial();
			let mut event: serde_json::Value = serde_json::from_str(&row.event_json).unwrap();

			event["action"]["command"] = serde_json::json!(command);
			row.event_json = event.to_string();

			let result = agent_guardian::project(&row, None).unwrap();

			assert!(result.can_approve && result.details_paged);
			assert!(result.action_json.is_none() && result.rationale.is_none());
			assert!(result.details_unavailable.is_none());
			assert!(serde_json::to_vec(&result).unwrap().len() < 4_096);
		}
	}
}
