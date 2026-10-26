//! One cancellable observation connection for the visible managed conversation.
use std::{
	collections::BTreeSet,
	time::{Duration, Instant},
};

use tokio::sync::watch;

use crate::shell::agent_surface::{
	AgentClient, AgentSurface, AgentWorkItemDto, Context, EntityId, Task,
};
use decodex_protocol::{AgentLiveMessageDto, AgentOutputResult, AgentTimelineContent};

#[derive(Default)]
pub(super) struct OutputStream {
	owner: Option<String>,
	thread: Option<String>,
	task: Option<Task<()>>,
	ready: bool,
	messages: Vec<AgentLiveMessageDto>,
	retry_after: Option<Instant>,
}

impl AgentSurface {
	pub(super) fn observe_visible_output(&mut self, cx: &mut Context<Self>) {
		let Some(owner) = self.selected.clone().filter(|_| self.native_agents.selected.is_none())
		else {
			self.output_stream = OutputStream::default();

			return;
		};
		let thread = self.conversation_work().and_then(|work| work.codex_thread_id);

		if self.output_stream.owner.as_ref() != Some(&owner) || self.output_stream.thread != thread
		{
			self.output_stream =
				OutputStream { owner: Some(owner.clone()), thread, ..Default::default() };
		}
		if self.output_stream.task.is_some()
			|| self.output_stream.retry_after.is_some_and(|t| t > Instant::now())
		{
			return;
		}

		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Ok(work) = EntityId::new(owner.clone()) else {
			return;
		};
		let (sender, mut receiver) = watch::channel(None);

		if std::thread::Builder::new()
			.name("agent-output-io".into())
			.spawn(move || {
				if let Ok(runtime) =
					tokio::runtime::Builder::new_current_thread().enable_all().build()
				{
					let _ =
						runtime.block_on(AgentClient::new(profile).observe_output(work, sender));
				}
			})
			.is_err()
		{
			self.output_stream.retry_after = Some(Instant::now() + Duration::from_secs(2));

			return;
		}

		self.output_stream.task = Some(cx.spawn(async move |surface, cx| {
			while receiver.changed().await.is_ok() {
				// The latest-value channel coalesces bursts without an unbounded delta queue.
				cx.background_executor().timer(Duration::from_millis(8)).await;

				let update = receiver.borrow_and_update().clone();
				let Some(AgentOutputResult::Available { work_id, messages, .. }) = update else {
					continue;
				};
				let _ = surface.update(cx, |s, cx| {
					if s.selected.as_deref() != Some(work_id.as_str())
						|| s.output_stream.owner.as_ref() != Some(&owner)
					{
						return;
					}
					// The stream closes before the native page reaches this window.
					// Keep its last paint until that page acknowledges the same turn.
					if messages.is_empty()
						&& !s.output_stream.messages.is_empty()
						&& s.conversation_work().is_some_and(|work| s.native_history_active(&work))
					{
						s.retire_native_output();
						s.refresh_open_native_history(cx);
						cx.notify();

						return;
					}

					let changed = !s.output_stream.ready || s.output_stream.messages != messages;

					s.output_stream.ready = true;

					if changed {
						let needs_state = messages.first().is_some_and(|message| {
							s.snapshot
								.as_ref()
								.and_then(|snapshot| {
									snapshot.work_items.iter().find(|work| work.id == owner)
								})
								.and_then(|work| work.active_turn_id.as_ref())
								!= Some(&message.turn_id)
						});

						s.output_stream.messages = messages;

						if needs_state {
							s.refresh(cx);
						}

						cx.notify();
					}
				});
			}

			let _ = surface.update(cx, |s, cx| {
				if s.output_stream.owner.as_ref() == Some(&owner) {
					s.output_stream.task = None;
					s.output_stream.ready = false;
					s.output_stream.retry_after = Some(Instant::now() + Duration::from_secs(2));

					cx.notify();
				}
			});
		}));
	}

	pub(super) fn streamed_output<'a>(
		&'a self,
		work: &AgentWorkItemDto,
	) -> Option<&'a [AgentLiveMessageDto]> {
		(self.output_stream.ready
			&& self.output_stream.owner.as_ref() == Some(&work.id)
			&& (work.active_turn_id.as_ref().map_or_else(
				|| {
					self.native_history_active(work)
						&& self.output_stream.thread == work.codex_thread_id
				},
				|turn| self.output_stream.messages.iter().any(|m| &m.turn_id == turn),
			)))
		.then_some(self.output_stream.messages.as_slice())
	}

	pub(super) fn retire_native_output(&mut self) -> bool {
		let Some(binding) = &self.timeline.native.binding else { return false };

		if self.output_stream.owner.as_ref() != Some(&binding.work)
			|| self.output_stream.thread.as_ref() != Some(&binding.thread)
		{
			return false;
		}

		let completed: BTreeSet<_> = self
			.timeline
			.native
			.entries
			.iter()
			.filter_map(|entry| match &entry.content {
				AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } =>
					Some(turn_id),
				_ => None,
			})
			.collect();
		let before = self.output_stream.messages.len();

		self.output_stream.messages.retain(|m| !completed.contains(&m.turn_id));

		before != self.output_stream.messages.len()
	}
}

#[cfg(test)]
mod tests {
	use gpui::AppContext as _;

	use crate::shell::agent_surface::output_stream::{
		AgentLiveMessageDto, AgentSurface, OutputStream,
	};
	use decodex_protocol::{AgentHistoryResult, AgentTimelineContent, AgentTimelineEntry};

	#[gpui::test]
	fn live_output_has_one_row_until_its_exact_saved_item_arrives(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let mut work = s
				.snapshot
				.as_ref()
				.unwrap()
				.work_items
				.iter()
				.find(|w| w.id == "agent")
				.unwrap()
				.clone();

			work.active_turn_id = Some("current".into());
			work.codex_thread_id = Some("thread".into());
			s.timeline.native.binding = Some(super::super::native_timeline::Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			});

			let message = AgentLiveMessageDto {
				kind: Default::default(),
				turn_id: "current".into(),
				item_id: "answer".into(),
				text: "A single live answer.".into(),
				truncated: false,
			};

			if let Some((_, AgentHistoryResult::Available { live, .. })) = &mut s.history {
				live.push(message.clone());
			}

			s.output_stream = OutputStream {
				owner: Some(work.id.clone()),
				thread: work.codex_thread_id.clone(),
				ready: true,
				messages: vec![message],
				..Default::default()
			};

			assert_eq!(
				s.native_live_rows(&work).len(),
				1,
				"history and the output stream share one live row"
			);

			s.timeline.native.entries.push(saved_output("older", "answer"));
			s.timeline.native.entries.push(saved_output("current", "commentary"));

			assert_eq!(s.native_live_rows(&work).len(), 1, "other items do not hide this answer");

			work.active_turn_id = None;

			if let Some((_, AgentHistoryResult::Available { live, .. })) = &mut s.history {
				live.clear();
			}

			s.retire_native_output();

			assert_eq!(
				s.native_live_rows(&work).len(),
				1,
				"idle work must retain the displayed answer until native history takes over"
			);

			s.timeline.native.entries.push(saved_output("current", "answer"));

			assert!(
				s.native_live_rows(&work).is_empty(),
				"saved item takes over without a second copy"
			);

			let mut other = work.clone();

			other.codex_thread_id = Some("other-thread".into());

			assert!(
				s.streamed_output(&other).is_none(),
				"a new source cannot reuse the pending handoff"
			);

			s.timeline.native.entries.push(AgentTimelineEntry {
				position: 4,
				content: AgentTimelineContent::TurnBoundary {
					turn_id: "current".into(),
					completed: true,
					status: Some("completed".into()),
					duration_ms: None,
					usage_summary: None,
					usage: None,
					error: None,
				},
			});
			s.retire_native_output();

			assert!(
				s.output_stream.messages.is_empty(),
				"saved completion retires the stream instead of retaining a second history"
			);
		});
	}

	fn saved_output(turn: &str, item: &str) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position: 0,
			content: AgentTimelineContent::Item {
				turn_id: turn.into(),
				item_id: item.into(),
				kind: "agentMessage".into(),
				text: "A single live answer.".into(),
				truncated: false,
				phase: None,
				app_ui: false,
				activity: None,
				attachments: vec![],
				collaboration: None,
			},
		}
	}

	#[gpui::test]
	fn output_is_visible_only_for_its_owner_and_current_turn(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let mut work = s
				.snapshot
				.as_ref()
				.unwrap()
				.work_items
				.iter()
				.find(|w| w.id == "agent")
				.unwrap()
				.clone();

			work.active_turn_id = Some("current".into());
			s.output_stream = OutputStream {
				owner: Some(work.id.clone()),
				ready: true,
				messages: vec![AgentLiveMessageDto {
					kind: Default::default(),
					turn_id: "old".into(),
					item_id: "answer".into(),
					text: "old output".into(),
					truncated: false,
				}],
				..Default::default()
			};

			assert!(s.streamed_output(&work).is_none());

			s.output_stream.messages[0].turn_id = "current".into();

			assert_eq!(s.streamed_output(&work).unwrap().len(), 1);

			s.output_stream.owner = Some("other-agent".into());

			assert!(s.streamed_output(&work).is_none());

			s.output_stream.owner = Some(work.id.clone());
			work.active_turn_id = None;

			assert!(
				s.streamed_output(&work).is_none(),
				"completed output must come from saved history"
			);
		});
	}
}
