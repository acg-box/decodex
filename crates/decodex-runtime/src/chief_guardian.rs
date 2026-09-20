//! Bounded review projection; only the daemon can retrieve original approval payloads.
use decodex_database::{ChiefGuardianReview, SqliteStore};
use decodex_protocol::{
	ChiefGuardianReviewDto, ChiefGuardianReviewsResult, ChiefGuardianStatus as Status,
	ChiefGuardianSubmission as Submission,
};

pub(crate) async fn read(
	store: &SqliteStore,
	work: &str,
	before: Option<i64>,
	generation: Option<String>,
) -> ChiefGuardianReviewsResult {
	let Ok(owner) = store.get_chief_work_item(work.into()).await else {
		return ChiefGuardianReviewsResult::Unavailable;
	};
	let Ok(saved) = store.read_chief_guardian_reviews(work.into(), before, 9).await else {
		return ChiefGuardianReviewsResult::Unavailable;
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
			return ChiefGuardianReviewsResult::Unavailable;
		};
		if owner.active_turn_id.as_ref().is_some_and(|turn| turn != &row.turn_id)
			|| !matches!(
				owner.dispatch_state,
				decodex_database::ChiefDispatchState::Idle
					| decodex_database::ChiefDispatchState::Running
			) {
			review.can_approve = false;
			if review.status == Status::Denied {
				review.approval_unavailable =
					Some("The task has moved to another turn or is reconnecting.".into());
			}
		}
		let cost = serde_json::to_vec(&review).map_or(usize::MAX, |v| v.len());
		if bytes.saturating_add(cost) > 128 * 1024 {
			more = true;
			break;
		}
		bytes += cost;
		reviews.push(review);
	}
	let next_before = more.then(|| reviews.last().map(|r| r.row_id)).flatten();
	ChiefGuardianReviewsResult::Available { reviews, next_before }
}

fn project(row: &ChiefGuardianReview, generation: Option<&str>) -> Option<ChiefGuardianReviewDto> {
	let event = serde_json::from_str(&row.event_json).ok()?;
	let observed = decodex_codex::guardian::decode_review(
		if row.status == "inProgress" {
			"item/autoApprovalReview/started"
		} else {
			"item/autoApprovalReview/completed"
		},
		&event,
	)?;
	let status = match row.status.as_str() {
		"inProgress" => Status::InProgress,
		"approved" => Status::Approved,
		"denied" => Status::Denied,
		"timedOut" => Status::TimedOut,
		"aborted" => Status::Aborted,
		_ => return None,
	};
	let submission = match row.approval_state.as_deref() {
		Some("pending") => Some(Submission::Pending),
		Some("submitted") => Some(Submission::Submitted),
		Some("rejected") => Some(Submission::Rejected),
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
	let mut result = ChiefGuardianReviewDto {
		row_id: row.id,
		digest: row.digest(),
		action_label: label.into(),
		status,
		risk_level: event["review"]["riskLevel"].as_str().map(str::to_owned),
		user_authorization: event["review"]["userAuthorization"].as_str().map(str::to_owned),
		rationale: event["review"]["rationale"].as_str().map(str::to_owned),
		action_json: Some(serde_json::to_string_pretty(&event["action"]).ok()?),
		details_unavailable: None,
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
	} else if serde_json::to_vec(&result).ok()?.len() > 96 * 1024 {
		result.action_json = None;
		result.rationale = None;
		result.details_unavailable=Some("Review details exceed the display limit. Inspect the action in the native Codex conversation.".into());
	}
	if status == Status::Denied {
		result.approval_unavailable =
			if row.conflicted {
				Some("Conflicting review evidence was received. This saved action cannot be approved.".into())
			} else if matches!(submission, Some(Submission::Pending | Submission::Submitted)) {
				Some("An approval submission is already recorded.".into())
			} else if result.details_unavailable.is_some() {
				Some("Complete action details are required before approval.".into())
			} else if decodex_codex::guardian::core_denial_event(&observed).is_none() {
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
	use super::*;
	use serde_json::json;
	fn denial() -> ChiefGuardianReview {
		ChiefGuardianReview {id:1,thread_id:"thread".into(),turn_id:"turn".into(),review_id:"review".into(),
			connection_id:"connection".into(),generation_id:Some("generation".into()),status:"denied".into(),
			event_json:json!({"threadId":"thread","turnId":"turn","reviewId":"review","targetItemId":null,"startedAtMs":1,"completedAtMs":2,"decisionSource":"agent","review":{"status":"denied","riskLevel":"high","userAuthorization":"low","rationale":"User did not request this action."},"action":{"type":"command","source":"shell","command":"echo fixture","cwd":"/tmp"}}).to_string(),
			conflicted:false,approval_state:None,approval_key:None}
	}
	#[test]
	fn review_and_user_submission_are_separate_and_bound_to_exact_evidence() {
		let mut row = denial();
		let result = project(&row, Some("generation")).unwrap();
		assert!(result.can_approve && result.current_process);
		assert_eq!(result.digest, row.digest());
		for state in ["pending", "submitted", "rejected"] {
			row.approval_state = Some(state.into());
			row.approval_key = Some("exact-command".into());
			let result = project(&row, Some("new-generation")).unwrap();
			assert_eq!(result.status, Status::Denied);
			assert!(!result.current_process);
			assert_eq!(result.can_approve, state == "rejected");
			assert_eq!(result.submission_key.as_deref(), Some("exact-command"));
		}
		row.conflicted = true;
		assert!(!project(&row, None).unwrap().can_approve);
	}
	#[test]
	fn withheld_or_unknown_action_details_cannot_be_approved() {
		for command in ["sk-".to_owned() + &"A".repeat(60), "x".repeat(100_000)] {
			let mut row = denial();
			let mut event: serde_json::Value = serde_json::from_str(&row.event_json).unwrap();
			event["action"]["command"] = json!(command);
			row.event_json = event.to_string();
			let result = project(&row, None).unwrap();
			assert!(
				!result.can_approve
					&& result.action_json.is_none()
					&& result.details_unavailable.is_some()
			);
			assert!(serde_json::to_string(&result).unwrap().len() < 4096);
		}
		let mut row = denial();
		let mut event: serde_json::Value = serde_json::from_str(&row.event_json).unwrap();
		event["action"]["futureField"] = json!(true);
		row.event_json = event.to_string();
		let result = project(&row, None).unwrap();
		assert!(!result.can_approve && result.action_json.unwrap().contains("futureField"));
	}
}
