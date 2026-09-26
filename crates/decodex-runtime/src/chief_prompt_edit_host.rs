//! Public commands retain opaque reviews in the existing Chief actor. Queries are read-only.
use super::{ChiefCoordinator, ChiefHost, ChiefHostError};
use crate::PromptEditReview;
use decodex_database::ChiefPromptEditAttempt;
use decodex_protocol::{
	ChiefActionDto as Action, EntityId, PromptEditEvidence, PromptEditPhase as Phase,
	PromptEditStatus, WireText,
};
use std::{
	collections::BTreeMap,
	sync::Arc,
	time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub(super) type Reviews = Arc<Mutex<BTreeMap<String, (Instant, PromptEditReview)>>>;
const REVIEW_LIFETIME: Duration = Duration::from_secs(600);

impl ChiefHost {
	pub(super) async fn handle_prompt_edit(
		&self,
		key: &str,
		action: Action,
		chief: Option<&mut ChiefCoordinator>,
	) -> Result<String, ChiefHostError> {
		let chief = chief.ok_or("Task connection is unavailable")?;
		match action {
			Action::PreparePromptEdit { work_id, thread_id, turn_id, item_id } => {
				let pending = self
					.store
					.chief_prompt_edit_receipt(work_id.as_str().into(), thread_id.as_str().into())
					.await
					.map_err(|_| "Edit receipt is unavailable")?;
				if pending.is_some_and(|r| matches!(r.state.as_str(), "reserved" | "applied")) {
					return Err("Recover the existing prompt edit first".into());
				}
				let review = chief
					.prepare_prompt_edit(
						work_id.as_str(),
						thread_id.as_str(),
						turn_id.as_str(),
						item_id.as_str(),
						key,
					)
					.await
					.map_err(|_| "Prompt review failed")?
					.ok_or("This input cannot be edited")?;
				let mut reviews = self.prompt_edits.lock().await;
				reviews.retain(|_, (when, review)| {
					when.elapsed() < REVIEW_LIFETIME && review.is_live()
				});
				if reviews.len() >= 8 && !reviews.contains_key(work_id.as_str()) {
					return Err("Too many active prompt reviews".into());
				}
				reviews.insert(work_id.as_str().into(), (Instant::now(), review));
				Ok(work_id.as_str().into())
			},
			Action::ConfirmPromptEdit { work_id, thread_id, review_token } => {
				if let Some(receipt) = self
					.store
					.chief_prompt_edit_receipt(work_id.as_str().into(), thread_id.as_str().into())
					.await
					.map_err(|_| "Edit receipt is unavailable")?
					&& receipt.attempt.review_token == review_token.as_str()
				{
					return Ok(work_id.as_str().into()); // Lost command reply: report the receipt, never replay.
				}
				let review = {
					let mut reviews = self.prompt_edits.lock().await;
					let (when, review) =
						reviews.get(work_id.as_str()).ok_or("Prompt review expired")?;
					if when.elapsed() >= REVIEW_LIFETIME
						|| review.evidence().thread != thread_id.as_str()
						|| review.evidence().review_token != review_token.as_str()
					{
						return Err("Prompt review changed".into());
					}
					reviews.remove(work_id.as_str()).ok_or("Prompt review expired")?.1
				};
				chief.confirm_prompt_edit(review).await.map_err(|_| {
					ChiefHostError::Unknown("Prompt edit could not be confirmed; read its receipt")
				})?;
				Ok(work_id.as_str().into())
			},
			Action::RecoverPromptEdit { work_id, thread_id } => {
				chief.recover_prompt_edit(work_id.as_str(), thread_id.as_str()).await.map_err(
					|_| ChiefHostError::Unknown("Prompt edit history could not be reloaded"),
				)?;
				Ok(work_id.as_str().into())
			},
			Action::AcknowledgePromptEditDraft { work_id, thread_id, receipt_id, review_token } => {
				let receipt = self
					.store
					.chief_prompt_edit_receipt(work_id.as_str().into(), thread_id.as_str().into())
					.await
					.map_err(|_| "Edit receipt is unavailable")?
					.ok_or("Edit receipt is unavailable")?;
				if receipt.id != receipt_id || receipt.attempt.review_token != review_token.as_str()
				{
					return Err("Draft receipt changed".into());
				}
				if receipt.state == "draft_restored" {
					return Ok(work_id.as_str().into());
				}
				let current = chief
					.recover_prompt_edit(work_id.as_str(), thread_id.as_str())
					.await
					.map_err(|_| "Native edit recovery is incomplete")?
					.ok_or("Edit receipt is unavailable")?;
				if current.id != receipt_id || current.state != "applied" {
					return Err("Native edit recovery is incomplete".into());
				}
				let generation = chief.native_generation().map(|g| g.as_str().to_owned());
				if !self
					.store
					.release_chief_prompt_edit_draft(receipt_id, generation)
					.await
					.map_err(|_| "Draft acknowledgement failed")?
				{
					return Err("Draft acknowledgement changed".into());
				}
				Ok(work_id.as_str().into())
			},
			_ => Err("Unsupported prompt edit command".into()),
		}
	}

	pub(crate) async fn prompt_edit_status(
		&self,
		work: EntityId,
		thread: WireText,
		review: Option<&WireText>,
		offset: u64,
	) -> PromptEditStatus {
		let mut result = PromptEditStatus {
			work_id: work.clone(),
			thread_id: thread.clone(),
			phase: Phase::Idle,
			evidence: None,
		};
		let receipt = match self
			.store
			.chief_prompt_edit_receipt(work.as_str().into(), thread.as_str().into())
			.await
		{
			Ok(receipt) => receipt,
			Err(_) => {
				result.phase = Phase::Unavailable;
				return result;
			},
		};
		let prepared = {
			let reviews = self.prompt_edits.lock().await;
			reviews
				.get(work.as_str())
				.filter(|(when, r)| {
					when.elapsed() < REVIEW_LIFETIME
						&& r.is_live() && r.evidence().thread == thread.as_str()
				})
				.map(|(_, r)| r.evidence().clone())
		};
		let (attempt, id, phase) = if let Some(receipt) = receipt
			.filter(|r| matches!(r.state.as_str(), "reserved" | "applied") || prepared.is_none())
		{
			let phase = match receipt.state.as_str() {
				"reserved" => Phase::Uncertain,
				"applied" => Phase::Applied,
				"not_submitted" | "unchanged" => Phase::Unchanged,
				"draft_restored" => Phase::Restored,
				_ => {
					result.phase = Phase::Unavailable;
					return result;
				},
			};
			(receipt.attempt, Some(receipt.id), phase)
		} else if let Some(prepared) = prepared {
			if !self
				.store
				.chief_thread_is_owned(
					work.as_str().into(),
					thread.as_str().into(),
					prepared.generation.clone(),
				)
				.await
				.unwrap_or(false)
			{
				result.phase = Phase::Unavailable;
				return result;
			}
			(prepared, None, Phase::Review)
		} else {
			return result;
		};
		if (offset > 0 && review.is_none())
			|| review.is_some_and(|r| r.as_str() != attempt.review_token)
		{
			result.phase = Phase::Unavailable;
			return result;
		}
		result.evidence = fragment(&attempt, id, offset);
		result.phase = if result.evidence.is_some() { phase } else { Phase::Unavailable };
		result
	}
}

fn fragment(
	a: &ChiefPromptEditAttempt,
	id: Option<i64>,
	offset: u64,
) -> Option<PromptEditEvidence> {
	let text = serde_json::to_string(&a.content).ok()?;
	let start = usize::try_from(offset).ok()?;
	if start >= text.len() || !text.is_char_boundary(start) {
		return None;
	}
	let mut end = (start + 64 * 1024).min(text.len());
	while !text.is_char_boundary(end) {
		end -= 1;
	}
	Some(PromptEditEvidence {
		review_token: WireText::new(a.review_token.clone()).ok()?,
		receipt_id: id,
		before_turn_id: WireText::new(a.before_turn_id.clone()).ok()?,
		item_id: WireText::new(a.item_id.clone()).ok()?,
		removed_turns: u32::try_from(
			a.turn_ids.len() - a.turn_ids.iter().position(|t| t == &a.before_turn_id)?,
		)
		.ok()?,
		content_bytes: text.len() as u64,
		offset,
		fragment: text[start..end].into(),
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn canonical_fragments_preserve_unicode_and_fit_the_transport() {
		let a = ChiefPromptEditAttempt {
			work: "work".into(),
			thread: "thread".into(),
			generation: None,
			review_token: "a".repeat(64),
			attempt_id: "review".into(),
			before_turn_id: "selected".into(),
			item_id: "input".into(),
			turn_ids: vec!["prefix".into(), "selected".into(), "suffix".into()],
			content: vec![
				serde_json::json!({"type":"text","text":"界\\\"".repeat(40000),"text_elements":[]}),
				serde_json::json!({"type":"image","fileId":"native-file"}),
			],
		};
		let expected = serde_json::to_string(&a.content).unwrap();
		let mut restored = String::new();
		while restored.len() < expected.len() {
			let page = fragment(&a, None, restored.len() as u64).unwrap();
			assert_eq!(page.removed_turns, 2);
			assert!(
				serde_json::to_vec(&page).unwrap().len() < 200 * 1024,
				"leave room in the 256KiB wire envelope"
			);
			restored.push_str(&page.fragment);
		}
		assert_eq!(restored, expected);
		assert!(fragment(&a, None, (expected.find('界').unwrap() + 1) as u64).is_none());
		assert!(fragment(&a, None, expected.len() as u64).is_none());
	}
}
