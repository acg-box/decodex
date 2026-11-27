//! Read participation and board posts from the selected native turn.
use super::*;
use crate::shell::agent_surface::work_board::board_topics;

impl AgentSurface {
	fn participant_task_label(&self, name: &str) -> String {
		if super::super::managed_task_name(name) {
			return self
				.work_board
				.collaboration
				.task_labels
				.get(name)
				.cloned()
				.unwrap_or_default();
		}
		activity::wording::purpose(&name.replace('_', " "))
	}

	pub(super) fn latest_thread_turn(&self, thread: &str) -> Option<&str> {
		self.timeline.native.thread_items(thread).rev().find_map(|entry| match entry {
			AgentTimelineContent::Item { turn_id, .. }
			| AgentTimelineContent::TurnBoundary { turn_id, .. } => Some(turn_id.as_str()),
			_ => None,
		})
	}

	/// A later solo follow-up must not displace the last collaboration.
	pub(super) fn latest_discussion_turn(&self, thread: &str) -> Option<&str> {
		self.timeline.native.thread_items(thread).rev().find_map(|entry| {
			let AgentTimelineContent::Item { turn_id, collaboration, .. } = entry else {
				return None;
			};
			(collaboration.as_ref().is_some_and(|call| {
				call.sender_thread_id == thread && !call.receiver_thread_ids.is_empty()
			}) || board_topics::from_entry(entry, thread).is_some())
			.then_some(turn_id.as_str())
		})
	}

	/// Only the owning conversation can state the outcome of this particular turn.
	pub(super) fn collaboration_outcome(
		&self,
		thread: &str,
		turn: &str,
	) -> Option<(&str, &str, Option<i64>)> {
		let mut completed = false;
		let mut ended_at = None;
		for content in self.timeline.native.thread_items(thread).rev() {
			match content {
				AgentTimelineContent::TurnBoundary {
					turn_id,
					completed: true,
					status,
					started_at_micros,
					duration_ms,
					..
				} if turn_id == turn => {
					if matches!(
						status.as_deref(),
						Some("failed" | "interrupted" | "cancelled" | "canceled")
					) {
						return None;
					}
					completed = true;
					ended_at = started_at_micros
						.zip(*duration_ms)
						.and_then(|(start, ms)| start.checked_add_unsigned(ms.checked_mul(1_000)?));
				},
				AgentTimelineContent::Item { turn_id, item_id, kind, phase, text, .. }
					if turn_id == turn
						&& kind == "agentMessage"
						&& !text.trim().is_empty()
						&& (phase.as_deref() == Some("final_answer")
							|| (completed && phase.is_none())) =>
				{
					return Some((item_id, text, ended_at));
				},
				_ => {},
			}
		}
		None
	}

	/// Build the history menu from named collaboration on this thread. No global agent list.
	pub(super) fn earlier_discussions(&self, thread: &str, current: &str) -> Vec<(String, String)> {
		let mut turns = BTreeMap::<&str, Vec<String>>::new();
		let mut prompts = BTreeMap::new();
		let mut order = Vec::new();
		for entry in self.timeline.native.thread_items(thread) {
			let AgentTimelineContent::Item { turn_id, kind, text, collaboration, activity, .. } =
				entry
			else {
				continue;
			};
			if turn_id == current {
				continue;
			}
			if kind == "userMessage" && !text.trim().is_empty() {
				prompts.entry(turn_id.as_str()).or_insert(text.as_str());
				continue;
			}
			let title = if let Some(call) = collaboration
				.as_ref()
				.filter(|c| c.sender_thread_id == thread && !c.receiver_thread_ids.is_empty())
			{
				activity
					.as_ref()
					.filter(|a| a.kind == "subAgentActivity")
					.and_then(|a| a.detail.rsplit('/').next())
					.map(|n| self.participant_task_label(n))
					.unwrap_or_else(|| activity::wording::purpose(&call.prompt))
			} else if let Some(post) = board_topics::from_entry(entry, thread) {
				format!("#{}", post.channel_name)
			} else {
				continue;
			};
			let names = turns.entry(turn_id).or_insert_with(|| {
				order.push(turn_id.as_str());
				Vec::new()
			});
			if !title.is_empty() && !names.contains(&title) {
				names.push(title);
			}
		}
		order
			.into_iter()
			.rev()
			.map(|turn| {
				let names = &turns[turn];
				let title = if let Some(prompt) = prompts.get(turn) {
					activity::wording::message(prompt)
				} else if names.is_empty() {
					"Agent discussion".into()
				} else {
					names.join(", ")
				};
				(turn.to_owned(), title)
			})
			.collect()
	}

	pub(super) fn turn_participants(
		&self,
		thread: &str,
		turn: &str,
		rows: &[Row],
	) -> BTreeMap<String, Participant> {
		let mut members = BTreeMap::new();
		let Some(binding) = self.timeline.native.binding.as_ref().filter(|b| b.thread == thread)
		else {
			return members;
		};
		let live = self.conversation_record(&binding.work).is_some_and(|w| {
			w.codex_thread_id.as_deref() == Some(thread)
				&& w.active_turn_id.as_deref() == Some(turn)
		});
		for entry in &self.timeline.native.entries {
			let AgentTimelineContent::Item { turn_id, collaboration: Some(call), activity, .. } =
				&entry.content
			else {
				continue;
			};
			if turn_id != turn || call.sender_thread_id != thread {
				continue;
			}
			for id in call.receiver_thread_ids.iter().filter(|id| id.as_str() != thread) {
				let member = members.entry(id.clone()).or_insert_with(|| {
					let row = rows
						.iter()
						.find(|r| r.work == binding.work && r.thread.as_ref() == Some(id));
					let path_name = activity
						.as_ref()
						.filter(|a| a.kind == "subAgentActivity")
						.and_then(|a| a.detail.rsplit('/').next())
						.filter(|name| !name.is_empty());
					let task_name =
						path_name.map(|n| self.participant_task_label(n)).unwrap_or_default();
					let name = row
						.map(participant_name)
						.filter(|s| !s.is_empty())
						.map(str::to_owned)
						.unwrap_or_else(|| {
							if task_name.is_empty() { "Agent".into() } else { task_name.clone() }
						});
					let task = if task_name.is_empty() {
						row.filter(|r| r.native && r.title != r.owner)
							.map(|r| r.title.clone())
							.unwrap_or_default()
					} else {
						task_name
					};

					Participant {
						spawned_here: false,
						timing: Default::default(),
						name,
						task,
						state: ParticipantState::Assigned,
						reply: String::new(),
					}
				});
				if let Some(at) = activity.as_ref().and_then(|a| a.native_timestamp_ms) {
					member.timing.record(&call.tool, at);
				}
				if !call.prompt.trim().is_empty()
					&& (member.task.is_empty()
						|| matches!(
							call.tool.as_str(),
							"spawnAgent" | "followupTask" | "sendInput" | "sendMessage"
						)) {
					member.task.clone_from(&call.prompt);
				}
				// A thread-level status snapshot has no receiving turn identity. After a
				// follow-up it can still contain the reply to the previous dispatch.
				match call.tool.as_str() {
					"spawnAgent" | "subAgentActivity/started" => member.spawned_here = true,
					// Codex also emits an interaction when the lead steers a child before
					// its first report. The child is still completing the original assignment.
					"subAgentActivity/interacted"
						if matches!(
							member.state,
							ParticipantState::Working | ParticipantState::Assigned
						) => {},
					"followupTask"
					| "sendInput"
					| "sendMessage"
					| "subAgentActivity/interacted" => member.spawned_here = false,
					_ => {},
				}
				match call.tool.as_str() {
					"subAgentActivity/started"
					| "subAgentActivity/interacted"
					| "spawnAgent"
					| "followupTask"
					| "sendInput"
					| "sendMessage" => {
						member.state = if live {
							ParticipantState::Working
						} else {
							ParticipantState::Assigned
						};
						member.reply.clear();
					},
					"subAgentActivity/completed" =>
						if member.reply.is_empty() {
							member.state = ParticipantState::Finished
						},
					"subAgentActivity/interrupted"
						if matches!(
							member.state,
							ParticipantState::Working | ParticipantState::Assigned
						) =>
						member.state = ParticipantState::Stopped,
					_ => {},
				}
				if member.spawned_here
					&& let Some(result) = call.results.iter().find(|r| &r.thread_id == id)
				{
					match result.status.as_str() {
						"completed" if !result.message.trim().is_empty() => {
							member.state = ParticipantState::Replied;
							member.reply.clone_from(&result.message);
						},
						"completed" if member.reply.is_empty() =>
							member.state = ParticipantState::Finished,
						"errored" | "failed" => {
							member.state = ParticipantState::Failed;
							member.reply.clone_from(&result.message);
						},
						"shutdown"
							if matches!(
								member.state,
								ParticipantState::Assigned | ParticipantState::Working
							) =>
							member.state = ParticipantState::Finished,
						"interrupted"
							if matches!(
								member.state,
								ParticipantState::Assigned | ParticipantState::Working
							) =>
							member.state = ParticipantState::Stopped,
						"running" | "pendingInit" if live =>
							member.state = ParticipantState::Working,
						_ => {},
					}
				}
			}
		}
		// A fresh spawn with exactly one verified child turn is unambiguous. A reused
		// child has no dispatch identity in a thread-level snapshot.
		for (id, member) in &mut members {
			// Use a live child state only for the current dispatch. A reused child's state
			// cannot be applied to an older turn.
			if matches!(member.state, ParticipantState::Working | ParticipantState::Assigned)
				&& (live || self.latest_thread_turn(thread) == Some(turn))
				&& let Some(agent) = self
					.native_agents
					.lists
					.get(&binding.work)
					.and_then(|agents| agents.iter().find(|agent| agent.thread_id == *id))
			{
				member.state = match agent.status.as_str() {
					"waitingOnApproval" => ParticipantState::NeedsApproval,
					"waitingOnUserInput" => ParticipantState::NeedsInput,
					"systemError" => ParticipantState::Failed,
					"active" | "running" => ParticipantState::Working,
					_ => member.state,
				};
			}
			if member.spawned_here
				&& member.state == ParticipantState::Finished
				&& let Some(row) =
					rows.iter().find(|r| r.work == binding.work && r.thread.as_ref() == Some(id))
				&& let Some(brief) = self
					.work_board
					.briefs
					.get(&row.key)
					.filter(|brief| brief.source_thread == row.thread)
				&& let Some(output) = brief.output.as_ref().filter(|o| o.only_turn == Some(true))
				&& self.work_output_complete(row, &output.turn)
			{
				member.state = ParticipantState::Replied;
				member.reply.clone_from(&output.text);
			}
		}
		members
	}

	pub(super) fn board_topics(
		&self,
		work: &str,
		thread: &str,
		historical_turn: Option<&str>,
		rows: &[Row],
	) -> Vec<Vec<board_topics::Post>> {
		let mut posts = BTreeMap::new();
		if historical_turn.is_none() {
			// Native descendants belong to this conversation's tree. An independent project
			// agent's board is never imported just because the two agents exchanged messages.
			let sources = self.board_sources(work);
			for row in rows
				.iter()
				.filter(|row| row.thread.as_deref().is_some_and(|id| sources.contains(id)))
			{
				if let Some(brief) = self
					.work_board
					.briefs
					.get(&row.key)
					.filter(|brief| brief.source_thread == row.thread)
					.and_then(|brief| brief.board.as_ref())
				{
					for post in brief.posts.values() {
						board_topics::merge(&mut posts, post.clone());
					}
				}
			}
		}
		if self
			.timeline
			.native
			.binding
			.as_ref()
			.is_some_and(|b| b.thread == thread && b.work == work)
		{
			for entry in &self.timeline.native.entries {
				if self
					.timeline
					.native
					.source_threads
					.get(&entry.position)
					.is_some_and(|source| source != thread)
				{
					continue;
				}
				if let AgentTimelineContent::Item { turn_id, .. } = &entry.content
					&& historical_turn.is_none_or(|turn| turn == turn_id)
					&& let Some(post) = board_topics::from_entry(&entry.content, thread)
				{
					board_topics::merge(&mut posts, post);
				}
			}
		}
		let mut topics = BTreeMap::<String, Vec<_>>::new();
		for post in posts.into_values() {
			topics.entry(post.thread_id.clone()).or_default().push(post);
		}
		let mut topics: Vec<_> = topics.into_values().collect();
		for topic in &mut topics {
			topic.sort_by(|a, b| {
				(a.message_id != a.thread_id, board_topics::order(a))
					.cmp(&(b.message_id != b.thread_id, board_topics::order(b)))
			});
		}
		topics.sort_by(|a, b| {
			b.last().map(board_topics::order).cmp(&a.last().map(board_topics::order))
		});
		topics
	}
}

#[derive(Clone)]
pub(super) struct Participant {
	spawned_here: bool,
	pub(super) timing: super::timing::Timing,
	pub(super) name: String,
	pub(super) task: String,
	pub(super) state: ParticipantState,
	pub(super) reply: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ParticipantState {
	NeedsInput,
	NeedsApproval,
	Working,
	Replied,
	Finished,
	Stopped,
	Failed,
	Assigned,
}
impl ParticipantState {
	pub(super) fn short_label(self) -> &'static str {
		match self {
			Self::NeedsApproval => "Approval needed",
			Self::NeedsInput => "Input needed",
			Self::Working => "Working",
			Self::Replied => "Replied",
			Self::Finished => "Finished",
			Self::Stopped => "Stopped",
			Self::Failed => "Failed",
			Self::Assigned => "Assigned",
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::shell::agent_surface::native_timeline::Binding;
	use decodex_protocol::AgentTimelineEntry;
	use gpui::{AppContext as _, TestAppContext};
	use serde_json::json;

	fn event(
		position: u64,
		turn: &str,
		tool: &str,
		receiver: &str,
		reply: &str,
	) -> AgentTimelineEntry {
		let results = if reply.is_empty() {
			vec![]
		} else {
			vec![json!({"thread_id": receiver, "status": "completed", "message": reply})]
		};
		serde_json::from_value(json!({
            "position": position,
            "content": {
                "type": "item", "turn_id": turn, "item_id": position.to_string(),
                "kind": "collabAgentToolCall", "text":"", "truncated": false, "attachments": [],
                "collaboration": {
                    "sender_thread_id":"ref-sage", "receiver_thread_ids":[receiver],
                    "tool":tool, "status":"completed", "prompt": if tool == "spawnAgent" {"Check the totals"} else {""}, "results": results
                }
            }
        })).unwrap()
	}

	#[gpui::test]
	fn participant_replies_belong_to_the_original_turn(cx: &mut TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1536.), gpui::px(1024.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_workbench_page("workbench", cx);
			s.workspace.graph_expanded = false;
			s.open_work_conversation("sage", None, cx);
			s.toggle_agent_tree(cx);
			s.timeline.native.binding = Some(Binding {
				work: "sage".into(),
				thread: "ref-sage".into(),
				account: "test".into(),
			});
			s.timeline.native.entries = vec![
				event(1, "first", "spawnAgent", "child-a", ""),
				event(2, "first", "wait", "child-a", "Total is 30."),
				event(3, "first", "spawnAgent", "child-b", ""),
				event(4, "next", "spawnAgent", "child-a", ""),
			];
			let rows = s.factory_rows();
			let first = s.turn_participants("ref-sage", "first", &rows);
			assert_eq!(first.len(), 2);
			assert_eq!(first["child-a"].reply, "Total is 30.");
			assert!(first["child-b"].reply.is_empty());
			assert!(s.turn_participants("ref-sage", "next", &rows)["child-a"].reply.is_empty());
			assert!(s.turn_participants("other", "first", &rows).is_empty());
			assert_eq!(
				s.earlier_discussions("ref-sage", "next"),
				vec![("first".into(), "Check the totals".into())]
			);
			s.timeline.native.entries.insert(
				0,
				serde_json::from_value(json!({
					"position": 0,
					"content": {"type":"item","turn_id":"first","item_id":"ask","kind":"userMessage",
						"text":"Can five guests attend? Check the venue.","truncated":false,"attachments":[]}
				}))
				.unwrap(),
			);
			assert_eq!(s.earlier_discussions("ref-sage", "next")[0].1, "Can five guests attend?");
			assert!(s.earlier_discussions("other", "next").is_empty());
		});
		visual.update(crate::ui_frame::draw_frame);
		let earlier = visual.debug_bounds("earlier-turn-first").unwrap();
		visual.simulate_click(earlier.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		surface.read_with(visual, |s, _| {
			assert!(matches!(&s.work_board.reference,
				ReferencePage::Discussion { turn, thread, title } if turn == "first" && thread == "ref-sage" && title == "Can five guests attend?"
			))
		});
		let report = visual.debug_bounds("contributor-first-child-a").unwrap();
		visual.simulate_click(report.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		surface.read_with(visual, |s, _| {
			assert!(
				matches!(&s.work_board.reference, ReferencePage::Participant { turn, thread, agent }
				if turn == "first" && thread == "ref-sage" && agent == "child-a")
			);
			assert_eq!(s.selected.as_deref(), Some("sage"));
		});
		assert!(visual.debug_bounds("earlier-turn-first").is_none());
		let back = visual.debug_bounds("reference-back").unwrap();
		visual.simulate_click(back.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("contributor-first-child-a").is_some());
		let back = visual.debug_bounds("reference-back").unwrap();
		visual.simulate_click(back.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("earlier-turn-first").is_some());
	}

	#[gpui::test]
	fn returning_after_a_solo_turn_keeps_the_same_lead_reply(cx: &mut TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, _| {
			s.timeline.native.binding = Some(Binding {
				work: "sage".into(),
				thread: "ref-sage".into(),
				account: "test".into(),
			});
			let reply = |position, turn, phase, text| {
				serde_json::from_value(json!({
					"position": position,
					"content": {
						"type": "item", "turn_id": turn, "item_id": format!("item-{position}"),
						"kind": "agentMessage", "phase": phase, "text": text,
						"truncated": false, "attachments": []
					}
				}))
				.unwrap()
			};
			let boundary = |position, status| {
				serde_json::from_value(json!({
					"position": position,
					"content": {
						"type": "turn_boundary", "turn_id": "workshop", "completed": true,
						"status": status, "started_at_micros": 1_000_000, "duration_ms": 42
					}
				}))
				.unwrap()
			};
			s.timeline.native.entries = vec![
				event(1, "workshop", "spawnAgent", "catering", ""),
				reply(2, "workshop", "commentary", "Checking lunch capacity."),
				reply(3, "workshop", "final_answer", "Invite three today."),
				boundary(4, "completed"),
				reply(5, "email", "final_answer", "Workshop reminder"),
				reply(6, "workshop", "final_answer", "A different conversation"),
			];
			s.timeline.native.source_threads.insert(6, "other".into());
			assert_eq!(s.latest_thread_turn("ref-sage"), Some("email"));
			assert_eq!(s.latest_discussion_turn("ref-sage"), Some("workshop"));
			assert_eq!(
				s.collaboration_outcome("ref-sage", "workshop"),
				Some(("item-3", "Invite three today.", Some(1_042_000)))
			);
			s.timeline.native.entries[3] = boundary(4, "interrupted");
			assert!(s.collaboration_outcome("ref-sage", "workshop").is_none());
			assert!(s.collaboration_outcome("other", "workshop").is_none());
		});
	}

	#[gpui::test]
	fn compact_completion_reads_only_the_first_result_of_its_own_spawn(cx: &mut TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_workbench_page("workbench", cx);
			s.open_work_conversation("sage", None, cx);
			s.timeline.native.binding = Some(Binding {
				work: "sage".into(),
				thread: "ref-sage".into(),
				account: "test".into(),
			});
			s.timeline.native.entries = vec![
				event(1, "first", "subAgentActivity/started", "ref-iris", ""),
				event(2, "first", "subAgentActivity/completed", "ref-iris", ""),
				event(3, "next", "subAgentActivity/interacted", "ref-iris", ""),
				event(4, "next", "subAgentActivity/completed", "ref-iris", ""),
			];
			let rows = s.factory_rows();
			let row = rows.iter().find(|r| r.thread.as_deref() == Some("ref-iris")).unwrap();
			let brief = s.work_board.briefs.get_mut(&row.key).unwrap();
			brief.source_thread = row.thread.clone();
			brief.output = Some(activity::Output {
				only_turn: Some(true),
				turn: "child-turn".into(),
				text: "24 cartons".into(),
				truncated: false,
			});
			brief.outcome = Some(("child-turn".into(), "completed".into()));
			assert_eq!(
				s.turn_participants("ref-sage", "first", &rows)["ref-iris"].reply,
				"24 cartons"
			);
			assert!(s.turn_participants("ref-sage", "next", &rows)["ref-iris"].reply.is_empty());
			// A message to a running child can be interleaved between its compact
			// spawn and completion in the same native turn.
			s.timeline
				.native
				.entries
				.insert(1, event(10, "first", "subAgentActivity/interacted", "ref-iris", ""));
			assert_eq!(
				s.turn_participants("ref-sage", "first", &rows)["ref-iris"].reply,
				"24 cartons"
			);
			s.timeline.native.entries.remove(1);
			s.timeline.native.entries.push(event(
				5,
				"first",
				"sendInput",
				"ref-iris",
				"24 cartons",
			));
			let reused = s.turn_participants("ref-sage", "first", &rows);
			assert!(reused["ref-iris"].reply.is_empty());
			assert!(matches!(
				reused["ref-iris"].state,
				ParticipantState::Assigned | ParticipantState::Working
			));
			s.timeline.native.entries.extend([
				event(6, "first", "subAgentActivity/completed", "ref-iris", ""),
				event(7, "first", "wait", "ref-iris", "24 cartons"),
			]);
			assert!(s.turn_participants("ref-sage", "first", &rows)["ref-iris"].reply.is_empty());
			s.timeline.native.entries.truncate(4);
			s.work_board.briefs.get_mut(&row.key).unwrap().output.as_mut().unwrap().only_turn =
				Some(false);
			assert!(s.turn_participants("ref-sage", "first", &rows)["ref-iris"].reply.is_empty());
		});
	}

	#[gpui::test]
	fn managed_task_numbers_are_resolved_only_by_registered_identity(cx: &mut TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, _| {
			let name = "work_9e88961932fdf33a592b65b3a6de04a1";
			assert_eq!(s.participant_task_label("work_outline"), "Work outline");
			assert_eq!(s.participant_task_label(name), "");
			s.work_board
				.collaboration
				.task_labels
				.insert(name.into(), "Transcript controls".into());
			assert_eq!(s.participant_task_label(name), "Transcript controls");
		});
	}

	#[gpui::test]
	fn participant_status_tracks_the_native_agent_until_its_report_arrives(
		cx: &mut TestAppContext,
	) {
		let surface = cx.new(|cx| {
			let mut s = AgentSurface::new(cx);
			s.visual_workspace_fixture(cx);
			s.visual_workbench_page("workbench", cx);
			s
		});
		surface.update(cx, |s, _| {
			s.timeline.native.binding = Some(Binding {
				work: "sage".into(),
				thread: "ref-sage".into(),
				account: "test".into(),
			});
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "sage")
				.unwrap()
				.active_turn_id = Some("now".into());
			s.timeline.native.entries =
				vec![event(1, "now", "subAgentActivity/started", "ref-iris", "")];
			let rows = s.factory_rows();
			for (status, section) in [
				("active", "Working"),
				("waitingOnUserInput", "Input needed"),
				("waitingOnApproval", "Approval needed"),
				("systemError", "Failed"),
			] {
				s.native_agents
					.lists
					.get_mut("sage")
					.unwrap()
					.iter_mut()
					.find(|agent| agent.thread_id == "ref-iris")
					.unwrap()
					.status = status.into();
				assert_eq!(
					s.turn_participants("ref-sage", "now", &rows)["ref-iris"].state.short_label(),
					section
				);
			}
			s.timeline.native.entries.extend([
				event(2, "now", "wait", "ref-iris", "Ready for review."),
				event(3, "now", "subAgentActivity/interrupted", "ref-iris", ""),
			]);
			let participants = s.turn_participants("ref-sage", "now", &rows);
			assert_eq!(participants["ref-iris"].reply, "Ready for review.");
			assert_eq!(participants["ref-iris"].state.short_label(), "Replied");
		});
	}

	#[gpui::test]
	fn graph_inspection_filters_citations_and_replaces_the_previous_agent(cx: &mut TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1536.), gpui::px(1024.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_workbench_page("workbench", cx);
			s.workspace.graph_expanded = false;
			s.open_work_conversation("sage", None, cx);
			s.timeline.native.binding = Some(Binding {
				work: "sage".into(),
				thread: "ref-sage".into(),
				account: "test".into(),
			});
			s.timeline.native.entries = vec![
				event(1, "first", "spawnAgent", "ref-iris", ""),
				event(
					2,
					"first",
					"wait",
					"ref-iris",
					"[Iris file](/tmp/iris.txt:4) and [shared](/tmp/common.txt:8)",
				),
				event(3, "first", "spawnAgent", "child-b", ""),
				event(
					4,
					"first",
					"wait",
					"child-b",
					"[Different file](/tmp/sibling.txt) and [shared](/tmp/common.txt:12)",
				),
			];
			let iris = s
				.factory_rows()
				.into_iter()
				.find(|r| r.thread.as_deref() == Some("ref-iris"))
				.unwrap();
			s.select_workbench_agent(&iris.key, cx);
		});
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("cited-file-/tmp/common.txt").is_some());
		assert!(visual.debug_bounds("cited-file-/tmp/iris.txt").is_some());
		assert!(visual.debug_bounds("cited-file-/tmp/sibling.txt").is_none());
		assert!(visual.debug_bounds("citation-/tmp/common.txt-child-b").is_some());
		surface.update(visual, |s, cx| {
			let peer = s
				.factory_rows()
				.into_iter()
				.find(|r| r.thread.as_deref() == Some("ref-lin"))
				.unwrap();
			s.select_workbench_agent(&peer.key, cx);
		});
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("cited-file-/tmp/iris.txt").is_none());
		let back = visual.debug_bounds("reference-back").unwrap();
		visual.simulate_click(back.center(), Default::default());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("sage"));
			assert_eq!(s.work_board.reference, ReferencePage::Overview);
			assert!(s.work_board.view.picked_agent.is_none());
		});
	}

	#[gpui::test]
	fn unread_parent_groups_sibling_posts_by_native_topic(cx: &mut TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1536.), gpui::px(1024.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_workbench_page("workbench", cx);
			s.workspace.graph_expanded = false;
			s.open_work_conversation("sage", None, cx);
			s.toggle_agent_tree(cx);
			let second = s.native_agents.lists["mira"]
				.iter()
				.find(|a| a.thread_id == "ref-lin")
				.unwrap()
				.clone();
			s.native_agents.lists.get_mut("sage").unwrap().push(second);
			// Registered rows can belong to a different work, while the native roster is
			// authoritative.
			let rows = s.factory_rows();
            let post = |source: &str, id: &str, topic: &str| -> board_topics::Post {
                serde_json::from_value(json!({"message_id":id, "thread_id":topic, "channel_name":"team",
                    "author":source, "created_at":"2026-11-26T12:00:00Z","text":id,"posted_by_thread":source})).unwrap()
            };
            // Durable posts arrive together on the root, even when its transcript is empty.
            let root = rows.iter().find(|r| r.thread.as_deref() == Some("ref-sage")).unwrap();
            let brief = s.work_board.briefs.get_mut(&root.key).unwrap();
            brief.source_thread = root.thread.clone();
            brief.board.get_or_insert_with(Default::default).observe_native(decodex_protocol::NativeBoardPage {
                root: "ref-sage".into(), after: 2, more: false,
                posts: vec![post("ref-iris","topic","topic"),post("ref-lin","answer","topic")],
            });
            // Existing child receipts can overlap the Board or belong to another tree.
            for entry in [post("ref-lin","answer","topic"), post("ref-lin","another","another"), post("ref-mira","unrelated","topic")] {
                let row = rows.iter().find(|r| r.thread == entry.posted_by_thread).unwrap();
                let brief = s.work_board.briefs.get_mut(&row.key).unwrap();
                brief.source_thread = row.thread.clone();
                board_topics::merge(&mut brief.board.get_or_insert_with(Default::default).posts, entry);
            }
			let topics = s.board_topics("sage", "ref-sage", None, &rows);
			assert_eq!(topics.len(), 2);
			let joined = topics.iter().find(|group| group[0].thread_id == "topic").unwrap();
			assert_eq!(
				joined.iter().map(|p| p.message_id.as_str()).collect::<Vec<_>>(),
				["topic", "answer"]
			);
			assert!(s.board_topics("sage", "ref-sage", Some("different-turn"), &rows).is_empty());
		});
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("inspect-post-answer").is_none());
		let topic = visual.debug_bounds("topic-topic").unwrap();
		visual.simulate_click(topic.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("inspect-post-topic").is_some());
		assert!(visual.debug_bounds("inspect-post-another").is_none());
		let answer = visual.debug_bounds("inspect-post-answer").unwrap();
		visual.simulate_click(answer.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		surface.read_with(visual, |s, _| {
			assert_eq!(s.selected.as_deref(), Some("sage"));
			assert!(
				matches!(&s.work_board.reference, ReferencePage::Conversation { thread, message: Some(id), .. }
				if thread == "ref-lin" && id == "answer")
			);
		});
		let back = visual.debug_bounds("reference-back").unwrap();
		visual.simulate_click(back.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("inspect-post-answer").is_some());
	}

	#[gpui::test]
	fn board_shows_only_posts_actually_disclosed_to_this_conversation(cx: &mut TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, _| {
            s.timeline.native.binding = Some(Binding {work:"sage".into(), thread:"ref-sage".into(), account:"test".into()});
            let item = |pos, tool, text, status| serde_json::from_value(json!({
                "position":pos,"content":{"type":"item", "turn_id":"one", "item_id":format!("post-{pos}"), "kind":"mcpToolCall",
                "text":json!({"message_id":"m1","thread_id":"m1","created_at":"2026-11-26T13:00:00Z","author":"/root/reviewer","channel_name":"design","text":text}).to_string(),
                "truncated":false,"attachments":[],"activity":{"turn_id":"one","item_id":format!("post-{pos}"),"kind":"mcpToolCall","label":"Using tool","detail":tool,"status":status}}})).unwrap();
            s.timeline.native.entries = vec![
                item(1,"codex_message_board/post","","completed"),
                item(2,"codex_message_board/read_post","Confirmed totals.","completed"),
                item(3,"other/read_post","Unrelated.","completed"),
                item(4,"codex_message_board/read_post","Not delivered.","failed"),
                item(5,"codex_message_board/read_post","Other thread.","completed"),
            ];
            s.timeline.native.source_threads.insert(5,"another".into());
            let topics=s.board_topics("sage","ref-sage",Some("one"), &[]);
            let posts=&topics[0];
            assert_eq!(posts.len(),1);
            assert_eq!(posts[0].text,"Confirmed totals.");
            assert!(s.board_topics("sage","ref-sage",Some("earlier"),&[]).is_empty());
            assert_eq!(s.earlier_discussions("ref-sage","next")[0].1, "#design");
        });
	}
}
