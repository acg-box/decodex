//! Group native speech by call occurrence; never change the stored timeline.
use decodex_protocol::{AgentTimelineContent as Content, AgentTimelineEntry};

pub(crate) struct Conversation {
	pub indices: Vec<usize>,
	pub session: String,
	pub text: String,
	pub ended: bool,
	pub failed: bool,
}

impl Conversation {
	pub fn identity(&self, entries: &[AgentTimelineEntry]) -> String {
		// The closing boundary is present on the newest page and remains stable as older
		// speech is prepended. Session IDs alone can be reused by later calls.
		let entry = self.indices.iter().rev().map(|i| &entries[*i]).find(|entry| {
            matches!(&entry.content, Content::VoiceBoundary { kind, .. } if kind == "realtimeSessionClosed")
        }).unwrap_or(&entries[self.indices[0]]);
		serde_json::json!([self.session, crate::shell::agent_surface::native_timeline::key(entry)])
			.to_string()
	}

	pub fn hidden(&self) -> bool {
		self.ended && !self.failed && self.text.trim().is_empty()
	}

	pub fn empty_status(&self) -> &'static str {
		if self.failed {
			"Voice conversation failed"
		} else {
			"Voice conversation · Awaiting transcript"
		}
	}
}

pub(crate) fn groups(entries: &[AgentTimelineEntry]) -> Vec<Conversation> {
	let mut result: Vec<Conversation> = Vec::new();
	let mut latest: Option<usize> = None;
	for (index, entry) in entries.iter().enumerate() {
		let (session, start) = match &entry.content {
			Content::VoiceBoundary { session_id, kind, .. } =>
				(session_id, kind == "realtimeSessionStarted"),
			Content::Speech { session_id, .. } => (session_id, false),
			Content::Item { kind, text, attachments, .. } if kind == "userMessage" => {
				if attachments.is_empty()
					&& text.contains("<source>transcript_tail_flush</source>")
					&& let Some(tail) = super::handoff(text)
					&& let Some(group) = latest.map(|i| &mut result[i])
				{
					merge_tail(&mut group.text, &tail);
					group.indices.push(index);
				} else {
					latest = None;
				}
				continue;
			},
			_ => continue,
		};
		let group = if !start {
			result.iter().rposition(|g| g.session == *session && !g.ended)
		} else {
			None
		};
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
			Content::Speech { role, text, truncated, .. } => {
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
			Content::VoiceBoundary { kind, outcome, .. } if kind != "realtimeSessionStarted" => {
				group.ended = true;
				group.failed = outcome.as_deref() == Some("failed");
			},
			_ => {},
		}
	}
	result
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
	use super::*;
	fn entry(content: Content) -> AgentTimelineEntry {
		AgentTimelineEntry { position: 0, content }
	}
	fn boundary(id: &str, start: bool) -> AgentTimelineEntry {
		entry(Content::VoiceBoundary {
			item_id: id.into(),
			session_id: "call".into(),
			kind: if start { "realtimeSessionStarted" } else { "realtimeSessionClosed" }.into(),
			outcome: None,
		})
	}
	fn speech(role: &str, text: &str) -> AgentTimelineEntry {
		entry(Content::Speech {
			item_id: text.into(),
			session_id: "call".into(),
			role: role.into(),
			text: text.into(),
			truncated: false,
		})
	}
	fn message(text: &str) -> AgentTimelineEntry {
		entry(Content::Item {
			turn_id: "turn".into(),
			item_id: text.into(),
			kind: "userMessage".into(),
			phase: None,
			text: text.into(),
			truncated: false,
			activity: None,
			app_ui: false,
			attachments: vec![],
		})
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
		assert_eq!(groups(initial)[0].identity(initial), groups(&entries)[0].identity(&entries));
		let later = vec![
			boundary("start-again", true),
			speech("user", "Again"),
			boundary("end-again", false),
		];
		assert_ne!(groups(&entries)[0].identity(&entries), groups(&later)[0].identity(&later));
	}

	#[test]
	fn empty_calls_hide_only_after_successful_end() {
		let empty =
			groups(&[boundary("start", true), speech("user", "   "), boundary("end", false)]);
		assert!(empty[0].hidden());
		let pending = groups(&[boundary("start", true)]);
		assert!(!pending[0].hidden());
		assert_eq!(pending[0].empty_status(), "Voice conversation · Awaiting transcript");
		let mut failure = boundary("failure", false);
		if let Content::VoiceBoundary { outcome, .. } = &mut failure.content {
			*outcome = Some("failed".into());
		}
		let failed = groups(&[boundary("start", true), failure]);
		assert!(!failed[0].hidden());
		assert_eq!(failed[0].empty_status(), "Voice conversation failed");
		let spoken = groups(&[speech("user", "Hello"), boundary("end", false)]);
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
		let groups = groups(&entries);
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
		let groups = groups(&entries);
		assert_eq!(groups.len(), 1);
		assert_eq!(groups[0].indices, vec![0, 1, 2, 3, 4]);
		assert_eq!(groups[0].text, "user: Hi\nassistant: Hello");
		let mut text = "user: Hi".into();
		merge_tail(&mut text, "user: Hi\nassistant: Hello");
		assert_eq!(text, "user: Hi\nassistant: Hello");
	}
	#[test]
	fn partial_pages_and_reused_session_ids_remain_separate() {
		let groups = groups(&[
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
