//! Source-bound assignments and operations. Replies belong to the conversation and timeline.
use decodex_protocol::{AgentTimelineContent, AgentTimelineEntry};
use pulldown_cmark::{Event, Options, Parser, TagEnd};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Preview {
	pub turn: String,
	pub task: String,
	pub text: String,
	pub running: Option<String>,
	pub recent: Vec<Operation>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Operation {
	pub id: String,
	pub text: String,
}

fn recent(entries: &[AgentTimelineEntry], turn: &str) -> Vec<Operation> {
	let mut seen = std::collections::BTreeSet::new();
	let mut operations = Vec::new();
	for entry in entries.iter().rev() {
		let AgentTimelineContent::Item {
			turn_id,
			item_id,
			activity: Some(activity),
			collaboration,
			..
		} = &entry.content
		else {
			continue;
		};
		if turn_id != turn || !seen.insert(item_id) {
			continue;
		}
		let label =
			super::super::native_timeline::activity_label(activity, collaboration.as_ref(), &[]);
		let source = if !activity.detail.trim().is_empty()
			&& collaboration.is_none()
			&& label != activity.detail
		{
			format!("{label} · {}", activity.detail)
		} else {
			label
		};
		let label = text(&source);
		if label.is_empty() {
			continue;
		}
		operations.push(Operation { id: format!("{turn}:{item_id}"), text: label });
		if operations.len() == 3 {
			break;
		}
	}
	operations.reverse();
	operations
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Output {
	pub turn: String,
	pub text: String,
	pub truncated: bool,
}

pub(super) struct OutputRef<'a> {
	pub turn: &'a str,
	pub text: &'a str,
	pub truncated: bool,
}

impl Output {
	pub fn borrowed(&self) -> OutputRef<'_> {
		OutputRef { turn: &self.turn, text: &self.text, truncated: self.truncated }
	}
}

/// Retain the returned answer for in-place reading; cards derive their own excerpt.
pub(super) fn output(entries: &[AgentTimelineEntry]) -> Option<Output> {
	let latest = latest_output(entries)?;
	// Terminal briefs prefer the native final answer. Commentary remains in the live preview.
	// Missing phases retain the legacy display path; an explicit commentary phase is not a result.
	let message = |phase: Option<&str>| {
		entries.iter().rev().find_map(|entry| match &entry.content {
			AgentTimelineContent::Item {
				turn_id, kind, text, truncated, phase: observed, ..
			} if turn_id == latest.turn
				&& kind == "agentMessage"
				&& !text.trim().is_empty()
				&& observed.as_deref() == phase =>
				Some(OutputRef { turn: turn_id, text, truncated: *truncated }),
			_ => None,
		})
	};
	message(Some("final_answer")).or_else(|| message(None)).map(|output| Output {
		turn: output.turn.into(),
		text: output.text.into(),
		truncated: output.truncated,
	})
}

pub(super) fn latest_output(entries: &[AgentTimelineEntry]) -> Option<OutputRef<'_>> {
	let turn = entries.iter().rev().find_map(|entry| match &entry.content {
		AgentTimelineContent::Item { turn_id, .. }
		| AgentTimelineContent::TurnBoundary { turn_id, .. } => Some(turn_id.as_str()),
		_ => None,
	})?;
	entries.iter().rev().find_map(|entry| match &entry.content {
		AgentTimelineContent::Item { turn_id, kind, text: source, truncated, .. }
			if turn_id == turn && kind == "agentMessage" && !source.trim().is_empty() =>
			Some(OutputRef { turn, text: source, truncated: *truncated }),
		_ => None,
	})
}

pub(super) fn assignments(
	entries: &[AgentTimelineEntry],
	sender: &str,
) -> BTreeMap<String, String> {
	let mut names = BTreeMap::new();
	for entry in entries {
		let AgentTimelineContent::Item { activity, collaboration: Some(call), .. } = &entry.content
		else {
			continue;
		};
		if call.sender_thread_id != sender {
			continue;
		}
		let explicit = matches!(
			call.tool.as_str(),
			"spawnAgent" | "sendInput" | "sendMessage" | "followupTask"
		) && !call.prompt.trim().is_empty();
		let label = if explicit {
			text(&call.prompt)
		} else if let Some(activity) =
			activity.as_ref().filter(|activity| activity.kind == "subAgentActivity")
		{
			// Current native events expose a task path, not the encrypted input body.
			let Some((_, name)) = activity.detail.rsplit_once('/') else { continue };
			text(&name.replace('_', " "))
		} else {
			continue;
		};
		if label.is_empty() {
			continue;
		}
		for receiver in &call.receiver_thread_ids {
			if explicit {
				names.insert(receiver.clone(), label.clone());
			} else {
				names.entry(receiver.clone()).or_insert_with(|| label.clone());
			}
		}
	}
	names
}

pub(super) fn text(source: &str) -> String {
	// Bound markdown work even when the underlying message contains a large document.
	let bounded: String = source.chars().take(640).collect();
	let plain: String = Parser::new_ext(&bounded, Options::ENABLE_TABLES)
		.filter_map(|event| match event {
			Event::Text(text) | Event::Code(text) => Some(text.into_string()),
			Event::SoftBreak | Event::HardBreak | Event::Rule => Some(" ".into()),
			Event::End(
				TagEnd::Paragraph
				| TagEnd::Heading(_)
				| TagEnd::Item
				| TagEnd::CodeBlock
				| TagEnd::TableCell,
			) => Some(" ".into()),
			_ => None,
		})
		.collect();
	let compact = plain.split_whitespace().collect::<Vec<_>>().join(" ");
	let mut chars = compact.chars();
	let mut preview: String = chars.by_ref().take(180).collect();
	if chars.next().is_some() {
		preview = preview.trim_end().to_owned();
		preview.push('…');
	}
	preview
}

pub(super) fn latest(entries: &[AgentTimelineEntry], turn: Option<&str>) -> Option<Preview> {
	let turn = turn.or_else(|| {
		entries.iter().rev().find_map(|entry| match &entry.content {
			AgentTimelineContent::Item { turn_id, .. }
			| AgentTimelineContent::TurnBoundary { turn_id, .. } => Some(turn_id.as_str()),
			_ => None,
		})
	})?;
	let task = entries.iter().rev().find_map(|entry| match &entry.content {
		AgentTimelineContent::Item { turn_id, kind, text, .. }
			if turn_id == turn
				&& matches!(kind.as_str(), "userMessage" | "agentInput")
				&& !text.trim().is_empty() =>
			Some(text),
		_ => None,
	});
	let error = entries
		.iter()
		.rev()
		.find_map(|entry| match &entry.content {
			AgentTimelineContent::TurnBoundary { turn_id, error, .. } if turn_id == turn =>
				Some(error.as_ref().map(|error| error.message.as_str())),
			_ => None,
		})
		.flatten();
	let operation = || {
		entries.iter().rev().find_map(|entry| match &entry.content {
			AgentTimelineContent::Item {
				turn_id, activity: Some(activity), collaboration, ..
			} if turn_id == turn && activity.kind != "subAgentActivity" => {
				let source = if activity.detail.trim().is_empty() {
					super::super::native_timeline::activity_label(
						activity,
						collaboration.as_ref(),
						&[],
					)
				} else {
					activity.detail.clone()
				};
				let text = text(&source);
				(!text.is_empty()).then_some(text)
			},
			_ => None,
		})
	};
	let task = task.map(|source| text(source)).unwrap_or_default();
	let preview_text = error.map(text).or_else(operation).unwrap_or_default();
	let completed = entries.iter().any(|entry| {
		matches!(
			&entry.content,
			AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } if turn_id == turn
		)
	});
	let running = (!completed)
		.then(|| {
			entries.iter().rev().find_map(|entry| match &entry.content {
				AgentTimelineContent::Item {
					turn_id,
					activity: Some(activity),
					collaboration,
					..
				} if turn_id == turn
					&& matches!(activity.status.as_str(), "running" | "inProgress") =>
					Some(if collaboration.is_some() || activity.detail.trim().is_empty() {
						text(&super::super::native_timeline::activity_label(
							activity,
							collaboration.as_ref(),
							&[],
						))
					} else {
						text(&activity.detail)
					}),
				_ => None,
			})
		})
		.flatten();
	(!task.is_empty() || !preview_text.is_empty() || running.is_some()).then(|| Preview {
		turn: turn.into(),
		task,
		text: preview_text,
		running,
		recent: recent(entries, turn),
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn item(turn: &str, kind: &str, text: &str) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position: 0,
			content: AgentTimelineContent::Item {
				turn_id: turn.into(),
				item_id: kind.into(),
				kind: kind.into(),
				text: text.into(),
				phase: None,
				truncated: false,
				activity: None,
				collaboration: None,
				app_ui: false,
				attachments: vec![],
			},
		}
	}

	#[test]
	fn terminal_briefs_prefer_the_complete_final_answer_over_commentary() {
		let mut final_answer = item("turn", "agentMessage", "Complete result.\n\nFinal paragraph.");
		if let AgentTimelineContent::Item { phase, .. } = &mut final_answer.content {
			*phase = Some("final_answer".into());
		}
		let mut commentary = item("turn", "agentMessage", "Finishing the check");
		if let AgentTimelineContent::Item { phase, .. } = &mut commentary.content {
			*phase = Some("commentary".into());
		}
		let entries = vec![final_answer, commentary.clone()];
		assert_eq!(output(&entries).unwrap().text, "Complete result.\n\nFinal paragraph.");
		assert_eq!(
			latest_output(&entries).unwrap().text,
			"Finishing the check",
			"live preview keeps its existing behavior"
		);
		assert!(
			output(&[commentary]).is_none(),
			"an explicit commentary phase is not a returned result"
		);
		assert_eq!(
			output(&[item("legacy", "agentMessage", "Legacy result")]).unwrap().text,
			"Legacy result"
		);
	}

	#[test]
	fn output_retains_the_returned_answer_and_excludes_other_turns_and_reasoning() {
		let mut entries = vec![item("old", "agentMessage", "Previous result")];
		assert_eq!(output(&entries).unwrap().text, "Previous result");
		entries.push(item("next", "userMessage", "Run the new check"));
		assert!(output(&entries).is_none());
		entries.push(item("next", "reasoning", "Private reasoning"));
		assert!(output(&entries).is_none());
		entries.push(item("next", "agentMessage", "**Compatibility checks passed.**"));
		assert_eq!(
			output(&entries),
			Some(Output {
				turn: "next".into(),
				text: "**Compatibility checks passed.**".into(),
				truncated: false
			})
		);
		entries.push(item("next", "agentMessage", &"result ".repeat(1000)));
		let answer = output(&entries).unwrap();
		assert_eq!(answer.text, "result ".repeat(1000));
		assert!(text(&answer.text).chars().count() <= 181);
		if let AgentTimelineContent::Item { truncated, .. } =
			&mut entries.last_mut().unwrap().content
		{
			*truncated = true;
		}
		assert!(output(&entries).unwrap().truncated);
	}

	#[test]
	fn replies_and_reasoning_do_not_replace_the_concrete_operation() {
		let mut entries = vec![
			item("old", "agentMessage", "Earlier reply"),
			item("now", "agentMessage", "Checking the change"),
		];
		let mut command = item("now", "commandExecution", "");
		if let AgentTimelineContent::Item { activity, .. } = &mut command.content {
			*activity = Some(decodex_protocol::AgentActivityDto {
				turn_id: "now".into(),
				item_id: "cmd".into(),
				kind: "commandExecution".into(),
				status: "running".into(),
				label: "Running command".into(),
				detail: "cargo test".into(),
				plugin_id: None,
				read_only_hint: None,
				native_timestamp_ms: None,
				duration_ms: None,
			});
		}
		entries.push(command);
		assert_eq!(latest(&entries, Some("now")).unwrap().text, "cargo test");
		assert_eq!(latest(&entries, Some("now")).unwrap().running.as_deref(), Some("cargo test"));
		if let AgentTimelineContent::Item { activity: Some(activity), .. } = &mut entries[2].content
		{
			activity.status = "completed".into();
		}
		assert!(latest(&entries, Some("now")).unwrap().running.is_none());
		entries.push(item("now", "agentMessage", "**All checks passed.**"));
		entries.push(item("now", "reasoning", "The reply is ready."));
		assert_eq!(latest(&entries, None).unwrap().text, "cargo test");
		assert!(
			latest(&entries, Some("next")).is_none(),
			"a new turn cannot show the previous reply as current activity"
		);
	}

	#[test]
	fn new_assignment_has_no_invented_activity_or_previous_turn_content() {
		let entries = vec![
			item("old", "agentMessage", "Old output"),
			item("next", "userMessage", "Review the scroll behavior"),
		];
		let preview = latest(&entries, None).unwrap();
		assert_eq!(preview.task, "Review the scroll behavior");
		assert!(preview.text.is_empty());
		assert!(latest(&[item("next", "agentMessage", "Only a reply")], None).is_none());
	}

	#[test]
	fn final_answers_stay_in_the_conversation_and_errors_keep_their_assignment() {
		let mut answer = item("turn", "agentMessage", "Created the reading guide.");
		if let AgentTimelineContent::Item { phase, .. } = &mut answer.content {
			*phase = Some("final_answer".into());
		}
		let completed = AgentTimelineEntry {
			position: 1,
			content: AgentTimelineContent::TurnBoundary {
				turn_id: "turn".into(),
				completed: true,
				status: Some("completed".into()),
				duration_ms: None,
				usage_summary: None,
				usage: None,
				error: None,
			},
		};
		let receipt = item("turn", "reasoning", "Late saved activity");
		let assignment = item("turn", "agentInput", "Check the reading time");
		let mut entries = vec![assignment, answer, completed, receipt];
		let preview = latest(&entries, None).unwrap();
		assert_eq!(preview.task, "Check the reading time");
		assert!(preview.text.is_empty());
		if let AgentTimelineContent::TurnBoundary { error, .. } = &mut entries[2].content {
			*error = Some(decodex_protocol::AgentTimelineError {
				message: "Permission denied".into(),
				truncated: false,
			});
		}
		assert_eq!(latest(&entries, None).unwrap().text, "Permission denied");
	}

	#[test]
	fn native_task_paths_identify_the_exact_recipient_without_inventing_a_prompt() {
		let mut event = item("turn", "subAgentActivity", "");
		if let AgentTimelineContent::Item { activity, collaboration, .. } = &mut event.content {
			*activity = Some(decodex_protocol::AgentActivityDto {
				turn_id: "turn".into(),
				item_id: "spawn".into(),
				kind: "subAgentActivity".into(),
				status: "started".into(),
				label: "Subagent started".into(),
				detail: "/root/soup_idea".into(),
				plugin_id: None,
				read_only_hint: None,
				native_timestamp_ms: None,
				duration_ms: None,
			});
			*collaboration = Some(decodex_protocol::AgentCollaborationDto {
				sender_thread_id: "parent".into(),
				receiver_thread_ids: vec!["child".into()],
				tool: "subAgentActivity/started".into(),
				status: "started".into(),
				prompt: String::new(),
				results: vec![],
			});
		}
		assert_eq!(
			assignments(&[event.clone()], "parent"),
			BTreeMap::from([("child".into(), "soup idea".into())])
		);
		assert!(assignments(&[event.clone()], "other-parent").is_empty());
		let mut explicit = event.clone();
		if let AgentTimelineContent::Item { collaboration: Some(call), .. } = &mut explicit.content
		{
			call.tool = "sendInput".into();
			call.prompt = "Review the soup ingredients".into();
		}
		assert_eq!(
			assignments(&[explicit, event], "parent")["child"],
			"Review the soup ingredients"
		);
	}

	#[test]
	fn recent_operations_keep_three_distinct_native_items_and_latest_revision() {
		let mut entries = Vec::new();
		for (id, label) in [
			("a", "Read Cargo.toml"),
			("b", "Run cargo check"),
			("c", "Inspect failure"),
			("d", "Run cargo test"),
			("d", "Tests passed"),
		] {
			let mut entry = item("now", "commandExecution", "");
			if let AgentTimelineContent::Item { item_id, activity, .. } = &mut entry.content {
				*item_id = id.into();
				*activity = Some(decodex_protocol::AgentActivityDto {
					turn_id: "now".into(),
					item_id: id.into(),
					kind: "commandExecution".into(),
					status: "completed".into(),
					label: "Command".into(),
					detail: label.into(),
					plugin_id: None,
					read_only_hint: None,
					native_timestamp_ms: None,
					duration_ms: None,
				});
			}
			entries.push(entry);
		}
		let preview = latest(&entries, Some("now")).unwrap();
		assert_eq!(
			preview.recent.iter().map(|op| op.id.as_str()).collect::<Vec<_>>(),
			["now:b", "now:c", "now:d"]
		);
		assert_eq!(preview.recent[2].text, "Command · Tests passed");
		assert!(latest(&entries, Some("next")).is_none());
	}

	#[test]
	fn preview_is_plain_bounded_text_without_losing_unicode() {
		assert_eq!(
			text("## Result\n\nUse **Markdown** with [links](https://example.com)."),
			"Result Use Markdown with links."
		);
		let preview = text(&"图".repeat(1000));
		assert_eq!(preview.chars().count(), 181);
		assert!(preview.ends_with('…'));
	}
}
