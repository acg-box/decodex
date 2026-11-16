//! Group native speech by call occurrence; never change the stored timeline.
use std::collections::{BTreeMap, BTreeSet};

use crate::shell::agent_surface::native_timeline;
use decodex_protocol::{AgentHistoryEntryDto, AgentTimelineContent, AgentTimelineEntry};

pub(crate) struct Conversation {
	pub indices: Vec<usize>,
	pub session: String,
	pub text: String,
	pub ended: bool,
	pub failed: bool,
}
impl Conversation {
	pub fn anchor_index(&self, entries: &[AgentTimelineEntry]) -> usize {
		// Keep the outer history anchor and disclosure state on the same stable boundary.
		self.indices.iter().rev().copied().find(|i| {
            matches!(&entries[*i].content, AgentTimelineContent::VoiceBoundary { kind, .. } if kind == "realtimeSessionClosed")
        }).unwrap_or(self.indices[0])
	}

	pub fn identity(&self, entries: &[AgentTimelineEntry]) -> String {
		serde_json::json!([
			self.session,
			native_timeline::key(&entries[self.anchor_index(entries)])
		])
		.to_string()
	}

	pub fn hidden(&self) -> bool {
		self.ended && self.text.trim().is_empty()
	}

	pub fn empty_status(&self) -> &'static str {
		if self.failed {
			"Voice conversation failed"
		} else {
			"Voice conversation (awaiting transcript)"
		}
	}
}

pub(crate) fn groups(entries: &[AgentTimelineEntry]) -> Vec<Conversation> {
	let mut result: Vec<Conversation> = Vec::new();
	let mut latest: Option<usize> = None;

	for (index, entry) in entries.iter().enumerate() {
		let (session, start) = match &entry.content {
			AgentTimelineContent::VoiceBoundary { session_id, kind, .. } =>
				(session_id, kind == "realtimeSessionStarted"),
			AgentTimelineContent::Speech { session_id, .. } => (session_id, false),
			AgentTimelineContent::Item { kind, text, attachments, .. } if kind == "userMessage" => {
				let current = latest.or_else(|| result.iter().rposition(|group| !group.ended));

				if attachments.is_empty()
					&& let Some(delegation) = super::delegation(text)
					&& let Some(group) = current.map(|i| &mut result[i])
					&& (!group.ended || delegation.source == "transcript_tail_flush")
				{
					// Native speech is the transcript. Handoff input is a task instruction;
					// it must not become a second conversation or an extra spoken sentence.
					if let Some(transcript) = delegation.transcript {
						merge_tail(&mut group.text, &super::unescape(transcript));
					}

					group.indices.push(index);
				} else {
					latest = None;
				}

				continue;
			},
			_ => continue,
		};
		let group = if !start { result.iter().rposition(|g| g.session == *session) } else { None };
		let group = group.unwrap_or_else(|| {
			result.push(Conversation {
				indices: vec![],
				session: session.clone(),
				text: String::new(),
				ended: false,
				failed: false,
			});

			result.len() - 1
		});

		latest = Some(group);

		let group = &mut result[group];

		group.indices.push(index);

		match &entry.content {
			AgentTimelineContent::Speech { role, text, truncated, .. } => {
				if text.trim().is_empty() && !truncated {
					continue;
				}
				if !group.text.is_empty() {
					group.text.push('\n');
				}

				group.text.push_str(&format!("{role}: {text}"));

				if *truncated {
					group.text.push_str(" [Transcript shortened]");
				}
			},
			AgentTimelineContent::VoiceBoundary { kind, outcome, .. }
				if kind != "realtimeSessionStarted" =>
			{
				group.ended = true;
				group.failed = outcome.as_deref() == Some("failed");
			},
			_ => {},
		}
	}

	result
}

/// Group the saved fallback with the same call boundary as native history.
/// Interleaved task output and typed messages do not split an identified call.
pub(crate) fn saved_groups(entries: &[&AgentHistoryEntryDto]) -> Vec<(Vec<usize>, String)> {
	let mut groups: Vec<(Vec<usize>, String)> = Vec::new();
	let mut sessions = BTreeMap::new();

	for (index, entry) in entries.iter().enumerate() {
		let voice = entry.receipt.as_ref().filter(|receipt| {
			matches!(
				(receipt.event_kind.as_str(), entry.kind.as_str()),
				("voice_user", "user") | ("voice_assistant", "assistant")
			)
		});

		if let Some(id) = voice.and_then(|receipt| receipt.voice_session_id.as_deref()) {
			let group = *sessions.entry(id).or_insert_with(|| {
				groups.push((Vec::new(), String::new()));

				groups.len() - 1
			});

			let (indices, text) = &mut groups[group];

			indices.push(index);

			if !text.is_empty() {
				text.push('\n');
			}

			if !entry.text.trim().is_empty() {
				text.push_str(&format!("{}: {}", entry.kind, entry.text));
			}
		}
	}

	groups
}

pub(crate) fn tail_flush_turns(entries: &[AgentTimelineEntry]) -> BTreeSet<&str> {
	entries
		.iter()
		.filter_map(|entry| match &entry.content {
			AgentTimelineContent::Item { turn_id, kind, text, attachments, .. }
				if kind == "userMessage"
					&& attachments.is_empty()
					&& text.contains("<source>transcript_tail_flush</source>")
					&& super::handoff(text).is_some() =>
				Some(turn_id.as_str()),
			_ => None,
		})
		.collect()
}

fn normalized(text: &str) -> String {
	text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn merge_tail(text: &mut String, tail: &str) {
	let existing: Vec<_> = text.lines().collect();
	let incoming: Vec<_> = tail.lines().collect();

	// A bounded upstream tail may contain only a suffix of the committed transcript.
	if !tail.is_empty() && normalized(text).ends_with(&normalized(tail)) {
		return;
	}

	let overlap = (0..=existing.len().min(incoming.len()))
		.rev()
		.find(|&n| {
			existing[existing.len() - n..]
				.iter()
				.zip(&incoming[..n])
				.all(|(a, b)| normalized(a) == normalized(b))
		})
		.unwrap_or(0);
	let remainder = incoming[overlap..].join("\n");

	if !text.is_empty() && !remainder.is_empty() {
		text.push('\n');
	}

	text.push_str(&remainder);
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::{
		AgentSurface,
		native_timeline::{Binding, Timeline},
		voice::history::conversations::{self, AgentTimelineContent, AgentTimelineEntry},
	};

	fn entry(content: AgentTimelineContent) -> AgentTimelineEntry {
		AgentTimelineEntry { position: 0, content }
	}
	fn boundary(id: &str, start: bool) -> AgentTimelineEntry {
		entry(AgentTimelineContent::VoiceBoundary {
			item_id: id.into(),
			session_id: "call".into(),
			kind: if start { "realtimeSessionStarted" } else { "realtimeSessionClosed" }.into(),
			outcome: None,
		})
	}
	fn speech(role: &str, text: &str) -> AgentTimelineEntry {
		entry(AgentTimelineContent::Speech {
			item_id: text.into(),
			session_id: "call".into(),
			role: role.into(),
			text: text.into(),
			truncated: false,
		})
	}
	fn message(text: &str) -> AgentTimelineEntry {
		entry(AgentTimelineContent::Item {
			collaboration: None,
			turn_id: "turn".into(),
			item_id: text.into(),
			kind: "userMessage".into(),
			phase: None,
			text: text.into(),
			truncated: false,
			activity: None,

			attachments: vec![],
		})
	}
	#[test]
	fn speech_received_after_close_stays_in_the_same_complete_call() {
		let calls = conversations::groups(&[
			boundary("start", true),
			speech("user", "Question"),
			boundary("end", false),
			speech("assistant", "Late final words"),
			boundary("end", false),
		]);
		assert_eq!(calls.len(), 1);
		assert!(calls[0].ended);
		assert_eq!(calls[0].text, "user: Question\nassistant: Late final words");
		let repeated = conversations::groups(&[
			boundary("start", true),
			speech("user", "First"),
			boundary("end", false),
			boundary("start-again", true),
			speech("user", "Second"),
			boundary("end-again", false),
		]);
		assert_eq!(
			repeated.len(),
			2,
			"a new start is a new call even when a provider reuses its session ID"
		);
	}
	#[test]
	fn saved_voice_groups_keep_calls_separate_without_grouping_normal_messages() {
		let make = |id, kind: &str, session: Option<&str>| decodex_protocol::AgentHistoryEntryDto {
			id,
			kind: kind.into(),
			text: format!("message {id}"),
			created_at_micros: id,
			native_source: None,
			turn_id: None,
			weather: vec![],
			activity: None,
			usage: None,
			duration_ms: None,
			receipt: session.map(|session| decodex_protocol::AgentHistoryReceiptDto {
				voice: None,
				unsent_reason: None,
				voice_session_id: Some(session.into()),
				event_kind: format!("voice_{kind}"),
				delivered_turn_id: None,
				disposed: true,
			}),
		};
		let entries = [
			make(1, "user", Some("a")),
			make(2, "system", None),
			make(3, "assistant", Some("a")),
			make(4, "user", None),
			make(5, "assistant", Some("a")),
			make(6, "user", Some("b")),
			make(7, "assistant", Some("b")),
		];
		let groups = conversations::saved_groups(&entries.iter().collect::<Vec<_>>());

		assert_eq!(
			groups,
			vec![
				(
					vec![0, 2, 4],
					"user: message 1\nassistant: message 3\nassistant: message 5".into()
				),
				(vec![5, 6], "user: message 6\nassistant: message 7".into())
			]
		);
	}

	#[gpui::test]
	fn expanded_call_stays_open_when_its_first_page_is_prepended(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_000.)));

		let mut entries = [
			boundary("start", true),
			speech("user", "Hi"),
			speech("assistant", "Hello"),
			boundary("end", false),
		];

		for (i, e) in entries.iter_mut().enumerate() {
			e.position = i as u64;
		}

		let page = |entries, cursor| decodex_protocol::AgentTimelinePage {
			source_threads: Default::default(),
			thread_id: "thread".into(),
			entries,
			next_cursor: cursor,
			weather: Default::default(),
			thread_token_total: None,
			thread_token_breakdown: None,
			thread_metrics: None,
			safety_buffering_turn_id: None,
			observed_activities: Vec::new(),
			active_realtime_session_at_page_start: None,
		};
		let binding = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| Some(&w.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("thread".into());

			let binding = Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			};
			let mut timeline = Timeline::default();

			assert!(
				timeline
					.replace(binding.clone(), page(entries[2..].to_vec(), Some("older".into())))
			);

			s.timeline.native = timeline;

			cx.notify();

			binding
		});

		visual.update(crate::ui_frame::draw_current_frame);

		let marks = surface.read_with(visual, |s, _| {
			assert_eq!(s.timeline.marks.len(), 1, "one call has one rail node");

			s.timeline.marks.keys().cloned().collect::<Vec<_>>()
		});
		let button = visual.debug_bounds("voice-history-block").unwrap();

		visual.simulate_click(button.center(), Default::default());
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("voice-history-transcript").is_some());

		surface.read_with(visual, |s, _| {
			assert_eq!(s.timeline.marks.keys().cloned().collect::<Vec<_>>(), marks);
			assert_eq!(s.timeline.selected.as_ref(), marks.first());
			assert!(s.timeline.follow_paused.contains(&binding.work));
			assert!(s.timeline.latest_follow_work.is_none());
			assert!(s.timeline.navigation.is_none());
		});
		surface.update(visual, |s, cx| {
			assert!(s.prepend_native_history(&binding, "older", page(entries[..2].to_vec(), None)));

			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);

		assert!(
			visual.debug_bounds("voice-history-transcript").is_some(),
			"pagination collapsed the existing transcript"
		);
	}

	#[test]
	fn loading_earlier_speech_preserves_the_conversation_identity() {
		let entries = vec![
			boundary("start", true),
			speech("user", "Hi"),
			speech("assistant", "Hello"),
			boundary("end", false),
		];
		let initial = &entries[2..];

		assert_eq!(
			conversations::groups(initial)[0].identity(initial),
			conversations::groups(&entries)[0].identity(&entries)
		);

		let later = vec![
			boundary("start-again", true),
			speech("user", "Again"),
			boundary("end-again", false),
		];

		assert_ne!(
			conversations::groups(&entries)[0].identity(&entries),
			conversations::groups(&later)[0].identity(&later)
		);
	}

	#[test]
	fn ended_empty_calls_are_hidden_including_failures() {
		let empty = conversations::groups(&[
			boundary("start", true),
			speech("user", "   "),
			boundary("end", false),
		]);

		assert!(empty[0].hidden());

		let pending = conversations::groups(&[boundary("start", true)]);

		assert!(!pending[0].hidden());
		assert_eq!(pending[0].empty_status(), "Voice conversation (awaiting transcript)");

		let mut failure = boundary("failure", false);

		if let AgentTimelineContent::VoiceBoundary { outcome, .. } = &mut failure.content {
			*outcome = Some("failed".into());
		}

		let failed = conversations::groups(&[boundary("start", true), failure]);

		assert!(failed[0].hidden(), "An empty failed call is not retained in history");
		assert_eq!(failed[0].empty_status(), "Voice conversation failed");

		let spoken = conversations::groups(&[speech("user", "Hello"), boundary("end", false)]);

		assert!(!spoken[0].hidden());
	}

	#[test]
	fn one_group_per_call_preserves_speakers_and_keeps_typed_messages_outside() {
		let entries = vec![
			boundary("start", true),
			speech("user", "Hi"),
			message("Typed message"),
			speech("assistant", "Hello"),
			boundary("end", false),
			boundary("second", true),
			speech("user", "Again"),
		];
		let groups = conversations::groups(&entries);

		assert_eq!(groups.len(), 2);
		assert_eq!(groups[0].indices, vec![0, 1, 3, 4]);
		assert_eq!(groups[0].text, "user: Hi\nassistant: Hello");
		assert!(groups[0].ended);
		assert!(!groups[1].ended);
	}
	#[test]
	fn tail_is_merged_once_and_new_text_is_preserved() {
		let tail = "<realtime_delegation><source>transcript_tail_flush</source><input>Internal instruction</input><transcript_delta>user: Hi\nassistant: Hello</transcript_delta></realtime_delegation>";
		let entries = vec![
			boundary("start", true),
			speech("user", "Hi"),
			speech("assistant", "Hello"),
			boundary("end", false),
			message(tail),
		];
		let groups = conversations::groups(&entries);

		assert_eq!(groups.len(), 1);
		assert_eq!(groups[0].indices, vec![0, 1, 2, 3, 4]);
		assert_eq!(groups[0].text, "user: Hi\nassistant: Hello");

		let mut text = "user: Hi".into();

		conversations::merge_tail(&mut text, "user: Hi\nassistant: Hello");

		assert_eq!(text, "user: Hi\nassistant: Hello");
	}

	#[test]
	fn handoffs_remain_in_one_complete_call_without_internal_instructions() {
		let handoff = "<realtime_delegation><input>Internal task instruction</input><transcript_delta>user: Hi\nassistant: Hello</transcript_delta></realtime_delegation>";
		let next = "<realtime_delegation><input>Another task instruction</input><transcript_delta>assistant: Hello\nuser: Check the tests</transcript_delta></realtime_delegation>";
		let entries = vec![
			boundary("start", true),
			speech("user", "Hi"),
			speech("assistant", "Hello"),
			message(handoff),
			message("Typed message outside the transcript"),
			message(next),
			speech("assistant", "Tests passed"),
			boundary("end", false),
		];
		let groups = conversations::groups(&entries);

		assert_eq!(groups.len(), 1);
		assert_eq!(groups[0].indices, vec![0, 1, 2, 3, 5, 6, 7]);
		assert_eq!(
			groups[0].text,
			"user: Hi\nassistant: Hello\nuser: Check the tests\nassistant: Tests passed"
		);
		assert_eq!(groups[0].anchor_index(&entries), 7);
	}
	#[test]
	fn partial_pages_and_reused_session_ids_remain_separate() {
		let groups = conversations::groups(&[
			speech("user", "Earlier page"),
			boundary("end", false),
			boundary("new", true),
			speech("user", "New call"),
		]);

		assert_eq!(groups.len(), 2);
		assert_eq!(groups[0].text, "user: Earlier page");
		assert_eq!(groups[1].text, "user: New call");
	}
}
