//! Presentation of existing source receipts. No scheduler or transcript ownership.
use serde_json::Value;

use crate::application::{ProductStore, QueryResultPayload, ServiceApplication, deliveries};
use decodex_database::{AgentCollaborationRecord, AgentDisposition};
use decodex_protocol::{
	AgentAsyncQuestionDto, AgentExchangeAssessment, AgentExchangeDto, AgentExchangeKind,
	AgentExchangeQuestionDto, AgentExchangesResult, AgentSnapshotDto, AgentSnapshotResult,
	AgentWorkKindDto,
};

impl ServiceApplication {
	pub(super) async fn query_exchanges(&self, before: Option<i64>) -> QueryResultPayload {
		let unavailable = || QueryResultPayload::AgentExchanges(AgentExchangesResult::Unavailable);
		let ProductStore::Available(store) = &self.store else { return unavailable() };
		let QueryResultPayload::AgentSnapshot(AgentSnapshotResult::Available(snapshot)) =
			self.query_agent_snapshot().await
		else {
			return unavailable();
		};

		if snapshot.connection_initializing {
			return unavailable();
		}

		let Some(source) = snapshot.runtime_source.clone() else {
			return unavailable();
		};
		let visible: Vec<_> = snapshot
			.work_items
			.iter()
			.filter(|w| !snapshot.archived_work_ids.contains(&w.id))
			.map(|w| w.id.clone())
			.collect();
		let Ok(mut records) = store.read_agent_collaboration(visible.clone(), before).await else {
			return unavailable();
		};
		let mut next_before = (records.len() > 100).then(|| records[99].event.id);

		records.truncate(100);

		let mut entries = Vec::new();
		let mut bytes = 0_usize;

		for record in records {
			let Some(entry) = project(record, &snapshot) else {
				continue;
			};
			let size = serde_json::to_vec(&entry).map_or(usize::MAX, |value| value.len() + 1);

			if bytes.saturating_add(size) > 96 * 1_024 {
				next_before = entries.last().map(|entry: &AgentExchangeDto| entry.event_id);

				break;
			}

			bytes += size;

			entries.push(entry);
		}

		let Ok(mut saved) = store.read_agent_collaboration_questions(visible).await else {
			return unavailable();
		};
		let mut questions_truncated = saved.len() > 100;

		saved.truncate(100);

		let projected = saved
			.into_iter()
			.filter_map(|row| {
				let mut question: AgentAsyncQuestionDto =
					serde_json::from_str(&row.question.question_json).ok()?;
				let (title, truncated) = public_excerpt(&question.title);

				question.title = if truncated { format!("{title}…") } else { title };

				question.options.clear();

				Some(AgentExchangeQuestionDto {
					work_id: row.work,
					thread_id: row.question.thread_id,
					turn_id: row.question.turn_id,
					item_id: row.question.item_id,
					question,
					pending: row.pending,
				})
			})
			.collect::<Vec<_>>();
		let mut questions = Vec::new();
		let mut question_bytes = 0_usize;

		for question in projected {
			let size = serde_json::to_vec(&question).map_or(usize::MAX, |value| value.len() + 1);

			if question_bytes.saturating_add(size) > 64 * 1_024 {
				questions_truncated = true;

				break;
			}

			question_bytes += size;

			questions.push(question);
		}

		QueryResultPayload::AgentExchanges(AgentExchangesResult::Available {
			source,
			entries,
			questions,
			questions_truncated,
			next_before,
		})
	}
}

fn project(
	record: AgentCollaborationRecord,
	snapshot: &AgentSnapshotDto,
) -> Option<AgentExchangeDto> {
	let event = record.event;
	let value: Value = serde_json::from_str(&event.payload).ok()?;
	let (kind, text, other, thread, turn, item, outcome) = match event.event_kind.as_str() {
		"collaboration_refused" => (
			AgentExchangeKind::NotSent,
			value["prompt"].as_str().unwrap_or_default().to_owned(),
			value["targetWorkId"].as_str().map(str::to_owned),
			value["threadId"].as_str().map(str::to_owned),
			value["turnId"].as_str().map(str::to_owned),
			value["callId"].as_str().map(str::to_owned),
			None,
		),
		"work_instruction" => (
			AgentExchangeKind::Instruction,
			value["text"].as_str().unwrap_or_default().to_owned(),
			manager(snapshot, &event.work_item_id),
			value["threadId"].as_str().map(str::to_owned),
			event.delivered_turn_id.clone().filter(|id| !id.is_empty()),
			None,
			None,
		),
		"worker_turn_completed" => (
			AgentExchangeKind::Result,
			deliveries::result_text(&value),
			record.recipient_work_id.or_else(|| manager(snapshot, &event.work_item_id)),
			value.pointer("/terminal/threadId").and_then(Value::as_str).map(str::to_owned),
			value.pointer("/terminal/turn/id").and_then(Value::as_str).map(str::to_owned),
			None,
			value.pointer("/terminal/turn/status").and_then(Value::as_str).map(str::to_owned),
		),
		_ => return None,
	};
	let (excerpt, truncated) = public_excerpt(&text);
	let assessment = (kind == AgentExchangeKind::Result)
		.then_some(event.disposition)
		.flatten()
		.map(|d| match d {
			AgentDisposition::Resolved => AgentExchangeAssessment::Accepted,
			AgentDisposition::FollowUp => AgentExchangeAssessment::FollowUp,
			AgentDisposition::Wait => AgentExchangeAssessment::Wait,
			AgentDisposition::UserDecision => AgentExchangeAssessment::UserDecision,
		});
	let target_dispatch_state = (kind == AgentExchangeKind::NotSent)
		.then(|| {
			snapshot
				.work_items
				.iter()
				.find(|work| Some(&work.id) == other.as_ref())
				.map(|work| work.dispatch_state)
		})
		.flatten();

	Some(AgentExchangeDto {
		event_id: event.id,
		source_event_id: event.source_event_id,
		work_id: event.work_item_id,
		other_work_id: other,
		target_dispatch_state,
		thread_id: thread,
		turn_id: turn,
		item_id: item,
		instruction_event_id: record.instruction_event_id,
		excerpt,
		truncated,
		kind,
		delivered_turn_id: event.delivered_turn_id.filter(|id| !id.is_empty()),
		assessment,
		assessment_note: assessment.and(event.disposition_note).map(|text| public_excerpt(&text).0),
		outcome,
		created_at_micros: event.created_at_micros,
	})
}

fn manager(snapshot: &AgentSnapshotDto, work: &str) -> Option<String> {
	let mut current = snapshot.work_items.iter().find(|w| w.id == work)?;

	for _ in 0..snapshot.work_items.len() {
		current =
			snapshot.work_items.iter().find(|w| Some(&w.id) == current.parent_goal_id.as_ref())?;

		if current.kind == AgentWorkKindDto::Manager || current.parent_goal_id.is_none() {
			return Some(current.id.clone());
		}
	}

	None
}

fn public_excerpt(text: &str) -> (String, bool) {
	if decodex_core::contains_credential_material(text) {
		return ("Sensitive content omitted. Open the source conversation.".into(), true);
	}

	let mut chars = text.chars();
	let excerpt = chars.by_ref().take(640).collect();

	(excerpt, chars.next().is_some())
}

#[cfg(test)]
mod tests {
	use crate::application::collaboration::{
		self, AgentCollaborationRecord, AgentDisposition, AgentExchangeAssessment, AgentSnapshotDto,
	};
	use decodex_database::AgentInboxEvent;
	fn snapshot() -> AgentSnapshotDto {
		serde_json::from_value(serde_json::json!({"runtime_source":"test-source","connection_initializing":false,"projects":[],"work_items":[
   {"id":"main","parent_goal_id":null,"kind":"manager","title":"Main","codex_thread_id":"main-thread","active_turn_id":null,"dispatch_state":"idle","status":"open","next_check_at_micros":null,"created_at_micros":1,"updated_at_micros":1},
   {"id":"lead","parent_goal_id":"main","kind":"manager","title":"Lead","codex_thread_id":"lead-thread","active_turn_id":null,"dispatch_state":"idle","status":"open","next_check_at_micros":null,"created_at_micros":1,"updated_at_micros":1},
   {"id":"goal","parent_goal_id":"lead","kind":"goal","title":"Goal","codex_thread_id":null,"active_turn_id":null,"dispatch_state":"idle","status":"open","next_check_at_micros":null,"created_at_micros":1,"updated_at_micros":1},
   {"id":"worker","parent_goal_id":"goal","kind":"task","title":"Research","codex_thread_id":"worker-thread","active_turn_id":null,"dispatch_state":"idle","status":"open","next_check_at_micros":null,"created_at_micros":1,"updated_at_micros":1}
  ],"dependencies":[],"pending_events":[]})).unwrap()
	}
	fn record(kind: &str, disposition: Option<AgentDisposition>) -> AgentCollaborationRecord {
		AgentCollaborationRecord{instruction_event_id:Some(10),recipient_work_id:None,event:AgentInboxEvent{id:20,source_event_id:"exact-source".into(),work_item_id:"worker".into(),event_kind:kind.into(),payload:serde_json::json!({"text":"Use Maple","terminal":{"threadId":"worker-thread","turn":{"id":"worker-turn","status":"completed"}},"threadReadback":{"assistantMessages":[{"phase":"final_answer","text":"The label uses Maple."}]}}).to_string(),created_at_micros:20,disposition,disposition_note:disposition.map(|_|"Checked the returned label".into()),disposed_at_micros:disposition.map(|_|21),delivered_turn_id:Some("review-turn".into())}}
	}
	#[test]
	fn assessments_belong_to_exact_results_and_route_to_the_nearest_manager() {
		let snapshot = snapshot();
		let pending =
			collaboration::project(record("worker_turn_completed", None), &snapshot).unwrap();

		assert_eq!(pending.other_work_id.as_deref(), Some("lead"));
		assert_eq!(pending.assessment, None);
		assert_eq!(pending.instruction_event_id, Some(10));
		assert_eq!(pending.excerpt, "The label uses Maple.");
		assert_eq!(pending.thread_id.as_deref(), Some("worker-thread"));

		let accepted = collaboration::project(
			record("worker_turn_completed", Some(AgentDisposition::Resolved)),
			&snapshot,
		)
		.unwrap();

		assert_eq!(accepted.assessment, Some(AgentExchangeAssessment::Accepted));

		let instruction = collaboration::project(
			record("work_instruction", Some(AgentDisposition::Resolved)),
			&snapshot,
		)
		.unwrap();

		assert_eq!(
			instruction.assessment, None,
			"provider instruction acknowledgment is not result acceptance"
		);
		assert_eq!(snapshot.work_items[0].status, decodex_protocol::AgentWorkStatusDto::Open);
	}
	#[test]
	fn excerpts_are_bounded_and_credentials_are_omitted() {
		let text = "界".repeat(2_000);
		let (excerpt, truncated) = collaboration::public_excerpt(&text);

		assert_eq!(excerpt.chars().count(), 640);
		assert!(truncated);

		let sensitive = "Authorization: Bearer sk-proj-abcdefghijklmnopqrstuvwxyz0123456789";
		let (excerpt, truncated) = collaboration::public_excerpt(sensitive);

		assert!(truncated);
		assert!(!excerpt.contains("sk-proj"));
	}
}
