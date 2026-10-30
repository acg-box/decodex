//! One current projection per conversation; never infer acceptance from a completed turn.
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use reqwest::Url;
use serde_json::Value;

use crate::application::{ProductStore, QueryResultPayload, ServiceApplication};
use decodex_database::DeliveryRecord;
use decodex_protocol::{
	AgentDeliveriesResult, AgentDeliveryArtifact, AgentDeliveryEntry, AgentDeliveryExtent,
	AgentDeliveryKind, AgentDeliveryQuery, AgentDeliveryUpdate, AgentDeliveryUpdateKind,
	AgentSnapshotResult,
};

impl ServiceApplication {
	pub(super) async fn query_deliveries(&self, query: &AgentDeliveryQuery) -> QueryResultPayload {
		let unavailable =
			|| QueryResultPayload::AgentDeliveries(AgentDeliveriesResult::Unavailable);
		let ProductStore::Available(store) = &self.store else { return unavailable() };
		let QueryResultPayload::AgentSnapshot(AgentSnapshotResult::Available(snapshot)) =
			self.query_agent_snapshot(false).await
		else {
			return unavailable();
		};

		if snapshot.connection_initializing {
			return unavailable();
		}

		let visible = snapshot
			.work_items
			.into_iter()
			.filter(|work| !snapshot.archived_work_ids.contains(&work.id))
			.map(|work| work.id)
			.collect();
		let Ok(page) = store
			.agent_deliveries(
				Some(visible),
				query.agent_id.as_ref().map(|p| p.as_str().to_owned()),
				query.after,
				query.before,
				query.limit,
			)
			.await
		else {
			return unavailable();
		};

		QueryResultPayload::AgentDeliveries(AgentDeliveriesResult::Available {
			entries: page.entries.into_iter().map(project).collect(),
			extent: page.extent.map(|(first, last)| AgentDeliveryExtent { first, last }),
		})
	}
}

pub(super) fn result_text(value: &Value) -> String {
	let messages = value.pointer("/threadReadback/assistantMessages");
	let parsed =
		messages.and_then(Value::as_str).and_then(|s| serde_json::from_str::<Value>(s).ok());
	let messages = parsed.as_ref().or(messages).and_then(Value::as_array);
	let text = messages
		.and_then(|items| {
			let nonempty = || {
				items
					.iter()
					.filter(|item| item["text"].as_str().is_some_and(|s| !s.trim().is_empty()))
			};

			nonempty()
				.rev()
				.find(|item| item["phase"] == "final_answer")
				.or_else(|| nonempty().next_back())
		})
		.and_then(|item| item["text"].as_str());

	text.map_or_else(|| super::agent_assistant_history(value, &mut false).1, str::to_owned)
}

fn project(row: DeliveryRecord) -> AgentDeliveryEntry {
	let parse = |value: Option<&str>| value.and_then(|s| serde_json::from_str::<Value>(s).ok());
	let value = parse(row.payload.as_deref());
	let milestone = parse(row.milestone.as_deref());
	let decision = parse(row.decision.as_deref());
	let outcome =
		value.as_ref().and_then(|v| v.pointer("/terminal/turn/status")).and_then(Value::as_str);
	let plan = milestone.as_ref().and_then(|v| v.get("progress"));
	let result = row
		.resolution
		.clone()
		.unwrap_or_else(|| value.as_ref().map(result_text).unwrap_or_default());
	let (_, artifacts) = reported_result(&result);
	let kind = if row.blocked || row.needs_input || row.status == "user_decision" {
		AgentDeliveryKind::NeedsInput
	} else if row.running {
		AgentDeliveryKind::Working
	} else if row.uncertain {
		AgentDeliveryKind::Unconfirmed
	} else if row.queued {
		AgentDeliveryKind::Waiting
	} else if row.status == "resolved" {
		AgentDeliveryKind::Accepted
	} else if outcome == Some("failed") {
		AgentDeliveryKind::Failed
	} else if outcome == Some("interrupted") {
		AgentDeliveryKind::Stopped
	} else if outcome == Some("completed") && !result.trim().is_empty() {
		AgentDeliveryKind::Result
	} else if row.status == "wait" || outcome == Some("completed") {
		AgentDeliveryKind::Waiting
	} else {
		AgentDeliveryKind::Requested
	};
	let step = plan.and_then(|p| p["summary"].as_str()).unwrap_or_default();
	let question = decision.as_ref().and_then(|q| q.get("title")).and_then(Value::as_str);
	let updates = recent_updates(&row.updates);
	let summary = match kind {
		AgentDeliveryKind::NeedsInput if row.blocked => "In use by another app".into(),
		AgentDeliveryKind::NeedsInput =>
			question.unwrap_or("A decision or approval is needed.").to_owned(),
		AgentDeliveryKind::Result | AgentDeliveryKind::Accepted => result,
		AgentDeliveryKind::Failed => "Open the task to review the failure.".into(),
		AgentDeliveryKind::Unconfirmed => "Check the task before continuing.".into(),
		AgentDeliveryKind::Working if row.resuming => "Continuing after your answer.".into(),
		_ => step.into(),
	};

	AgentDeliveryEntry {
		id: row.id,
		work_id: row.work_id,
		title: row.title,
		slot: row.slot,
		kind,
		summary,
		artifacts: if matches!(kind, AgentDeliveryKind::Result | AgentDeliveryKind::Accepted) {
			artifacts
		} else {
			Vec::new()
		},
		updates,
		updated_at_micros: row.updated_at_micros,
	}
}

fn recent_updates(source: &str) -> Vec<AgentDeliveryUpdate> {
	let rows: Vec<Value> = serde_json::from_str(source).unwrap_or_default();
	let mut updates: Vec<AgentDeliveryUpdate> = Vec::new();

	for row in rows.into_iter().rev() {
		let payload = &row["payload"];
		let (kind, text) = match row["kind"].as_str() {
			Some("plan_updated") => (
				AgentDeliveryUpdateKind::Progress,
				payload["progress"]["summary"].as_str().unwrap_or_default().to_owned(),
			),
			Some(kind @ ("async_question_answer" | "user_message"))
				if kind == "async_question_answer" || payload["asyncQuestionReply"] == true =>
			{
				let Some(replies) = decodex_protocol::parse_agent_async_question_replies(
					payload["text"].as_str().unwrap_or_default(),
				) else {
					continue;
				};

				(
					AgentDeliveryUpdateKind::Decision,
					replies
						.into_iter()
						.map(|reply| format!("{} — {}", reply.answer, reply.question))
						.collect::<Vec<_>>()
						.join("\n"),
				)
			},
			Some("goal_resolved") => (
				AgentDeliveryUpdateKind::Result,
				payload["summary"].as_str().unwrap_or_default().to_owned(),
			),
			Some("agent_turn_completed" | "worker_turn_completed")
				if payload.pointer("/terminal/turn/status").and_then(Value::as_str)
					== Some("completed") =>
				(AgentDeliveryUpdateKind::Result, result_text(payload)),
			_ => continue,
		};
		let summary = reported_result(&text).0;

		if summary.is_empty()
			|| updates.last().is_some_and(|last| last.kind == kind && last.summary == summary)
		{
			continue;
		}

		updates.push(AgentDeliveryUpdate {
			id: row["id"].as_i64().unwrap_or_default(),
			kind,
			summary: summary.chars().take(280).collect(),
			created_at_micros: row["at"].as_i64().unwrap_or_default(),
		});
	}

	updates
}

// Parse only the explicit task's final answer. Code, tool output, and intermediate
// prose never become a result or a link. Match the transcript's Markdown syntax.
fn reported_result(text: &str) -> (String, Vec<AgentDeliveryArtifact>) {
	let mut summary = String::new();
	let mut artifacts = Vec::new();
	let mut link: Option<(String, String)> = None;
	let mut code = false;
	let mut first_block = true;

	for event in Parser::new(text) {
		match event {
			Event::Start(Tag::CodeBlock(_)) => code = true,
			Event::End(TagEnd::CodeBlock) => {
				code = false;
			},
			Event::Start(Tag::Link { dest_url, .. }) =>
				link = Some((dest_url.to_string(), String::new())),
			Event::End(TagEnd::Link) =>
				if let Some((target, title)) = link.take() {
					let url = if target.starts_with('/') {
						Url::parse(&format!(
							"file://{}",
							target.split(":").next().unwrap_or(&target)
						))
						.ok()
					} else {
						Url::parse(&target).ok().filter(|u| {
							matches!(u.scheme(), "https" | "http")
								&& u.username().is_empty()
								&& u.password().is_none()
						})
					};

					if let Some(url) = url
						&& !title.trim().is_empty()
						&& artifacts.len() < 3
						&& !artifacts
							.iter()
							.any(|a: &AgentDeliveryArtifact| a.target == url.as_str())
					{
						artifacts.push(AgentDeliveryArtifact {
							title: title.chars().take(80).collect(),
							target: url.into(),
						});
					}
				},
			Event::Text(t) | Event::Code(t) if !code => {
				if first_block {
					summary.push_str(&t);
				}

				if let Some((_, title)) = &mut link {
					title.push_str(&t);
				}
			},
			Event::SoftBreak | Event::HardBreak if first_block => summary.push(' '),
			Event::End(TagEnd::Paragraph | TagEnd::Heading(_)) if !summary.is_empty() =>
				first_block = false,
			_ => {},
		}
	}

	(summary.split_whitespace().collect::<Vec<_>>().join(" "), artifacts)
}
#[cfg(test)]
mod tests {
	use crate::application::deliveries::{self, AgentDeliveryKind, AgentDeliveryUpdateKind};
	fn row() -> decodex_database::DeliveryRecord {
		decodex_database::DeliveryRecord{id:1,work_id:"work".into(),title:"Task".into(),slot:Some(1),updated_at_micros:1,status:"open".into(),running:false,resuming:false,needs_input:false,blocked:false,queued:false,uncertain:false,payload:Some(serde_json::json!({"terminal":{"turn":{"status":"completed"}},"threadReadback":{"assistantMessages":[{"phase":"commentary","text":"I will inspect."},{"phase":"final_answer","text":"Created the reading guide.\n\n[Open guide](/tmp/guide.md)"}]}}).to_string()),milestone:Some(serde_json::json!({"progress":{"summary":"Check the guide","complete":true}}).to_string()),decision:None,resolution:None,updates:"[]".into()}
	}
	#[test]
	fn conversations_prioritize_decisions_and_show_the_latest_reply_without_a_plan() {
		let ready = deliveries::project(row());

		assert_eq!(ready.kind, AgentDeliveryKind::Result);
		assert_eq!(ready.summary, "Created the reading guide.\n\n[Open guide](/tmp/guide.md)");
		assert_eq!(ready.artifacts.len(), 1);

		let mut r = row();

		r.milestone = Some(
			serde_json::json!({"progress":{"summary":"Check links","complete":false}}).to_string(),
		);

		assert_eq!(
			deliveries::project(r).kind,
			AgentDeliveryKind::Result,
			"an explicit result link survives a stale checklist"
		);

		let mut r = row();

		r.milestone = Some(r#"{"progress":{"summary":"Choose a novel","complete":false}}"#.into());
		r.payload = Some(r#"{"terminal":{"turn":{"status":"completed"}},"threadReadback":{"assistantMessages":[{"phase":"final_answer","text":"Please choose a format."}]}}"#.into());

		assert_eq!(deliveries::project(r).kind, AgentDeliveryKind::Result);

		let mut r = row();

		r.running = true;

		let working = deliveries::project(r);

		assert_eq!(working.summary, "Check the guide");
		assert!(working.artifacts.is_empty());

		let mut r = row();

		r.running = true;
		r.resuming = true;

		assert_eq!(deliveries::project(r).summary, "Continuing after your answer.");

		let mut r = row();

		r.running = true;
		r.needs_input = true;
		r.decision =
			Some(r#"{"id":"question","title":"Which format should I use?","options":[]}"#.into());

		let asking = deliveries::project(r);

		assert_eq!(asking.kind, AgentDeliveryKind::NeedsInput);
		assert_eq!(asking.summary, "Which format should I use?");
	}
	#[test]
	fn deliveries_link_only_explicit_markdown_and_skip_code_and_unsafe_targets() {
		let (summary, links) = deliveries::reported_result(
			"Ready.\n\n```md\n[Fake](https://example.com/fake)\n```\n[Guide](/tmp/my%20guide.md) [Unsafe](javascript:alert) [Password](https://name:password@example.com) [Review](https://github.com/acg-box/decodex/pull/1)",
		);

		assert_eq!(summary, "Ready.");
		assert_eq!(links.len(), 2);
		assert_eq!(links[0].target, "file:///tmp/my%20guide.md");
		assert_eq!(links[1].title, "Review");
	}
	#[test]
	fn retained_changes_show_choices_and_final_results_without_transport_or_tool_noise() {
		let answer = format!(
			"<send_user_message_question_reply>\n{}\n</send_user_message_question_reply>",
			serde_json::json!([{"questionItemId":"q","question":"Which outing?","answer":"A short walk"}])
		);
		let completed = serde_json::json!({"terminal":{"turn":{"status":"completed"}},"threadReadback":{"assistantMessages":[{"phase":"commentary","text":"I will check."},{"phase":"final_answer","text":"The route is ready.\n\n[Route](/tmp/route.md)"}]}});
		let changes = serde_json::json!([
			{"id":5,"kind":"user_message","at":50,"payload":{"text":answer.replace("A short walk","Reading"),"asyncQuestionReply":true}},
			{"id":4,"kind":"agent_turn_completed","at":40,"payload":completed},
			{"id":3,"kind":"async_question_answer","at":30,"payload":{"text":answer}},
			{"id":2,"kind":"activity_completed","at":20,"payload":{"text":"Running command"}},
			{"id":1,"kind":"plan_updated","at":10,"payload":{"progress":{"summary":"Check the short route"}}}
		]);
		let changes = deliveries::recent_updates(&changes.to_string());

		assert_eq!(changes.iter().map(|c| c.id).collect::<Vec<_>>(), vec![1, 3, 4, 5]);
		assert_eq!(changes[1].kind, AgentDeliveryUpdateKind::Decision);
		assert_eq!(changes[1].summary, "A short walk — Which outing?");
		assert_eq!(changes[2].summary, "The route is ready.");
		assert_eq!(changes[2].created_at_micros, 40);
		assert_eq!(changes[3].kind, AgentDeliveryUpdateKind::Decision);
		assert_eq!(changes[3].summary, "Reading — Which outing?");
	}
	#[test]
	fn completed_responses_keep_all_paragraphs_and_code() {
		for text in [
			format!("{}\n\nThe complete final paragraph.", "A useful detail. ".repeat(40)),
			"```python\ndef group_books(books):\n    return books\n```".to_owned(),
		] {
			let mut r = row();

			r.payload = Some(serde_json::json!({"terminal":{"turn":{"status":"completed"}},"threadReadback":{"assistantMessages":[{"phase":"final_answer","text":text}]}}).to_string());

			let projected = deliveries::project(r);

			assert_eq!(projected.kind, AgentDeliveryKind::Result);
			assert_eq!(projected.summary, text);
		}
	}
	#[test]
	fn writer_lock_uses_the_same_actionable_copy_as_the_composer() {
		let mut row = row();

		row.blocked = true;

		let entry = deliveries::project(row);

		assert_eq!(entry.kind, AgentDeliveryKind::NeedsInput);
		assert_eq!(entry.summary, "In use by another app");
		assert!(entry.artifacts.is_empty());
	}
}
