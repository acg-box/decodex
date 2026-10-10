//! Native receipt presentation; Dock handoff acknowledgement keeps its own meaning.
use crate::shell::agent_surface::{
	AgentActionDto, AgentClient, AgentCommandResponse, AgentSnapshotDto, AgentSurface, Context,
	EntityId, IdempotencyKey, IntoElement, ParentElement, Styled, Task, unique_command,
};
use decodex_protocol::{AgentReadStateResult, AgentUnreadPosition};
use gpui::AnyElement;
use std::time::Instant;
use tokio::runtime::Builder;

#[derive(Default)]
pub(super) struct Panel {
	target: Option<(String, String)>,
	result: Option<AgentReadStateResult>,
	task: Option<Task<()>>,
	epoch: u64,
	read_at: Option<Instant>,
	feedback: String,
}
impl AgentSurface {
	pub(super) fn reset_read_state(&mut self) {
		self.read_state =
			Panel { epoch: self.read_state.epoch.wrapping_add(1), ..Default::default() };
	}

	fn read_state_target(&self) -> Option<(String, String)> {
		let target = self.native_goal_target()?;
		let owner = self.snapshot.as_ref()?.work_items.iter().find(|w| w.id == target.0)?;
		(owner.codex_thread_id.as_deref() == Some(target.1.as_str())).then_some(target)
	}

	pub(super) fn invalidate_read_state(&mut self, next: &AgentSnapshotDto) {
		if self.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref())
			!= next.runtime_source.as_ref()
			|| self.read_state.target.as_ref().is_some_and(|(work, thread)| {
				!next
					.work_items
					.iter()
					.any(|w| &w.id == work && w.codex_thread_id.as_ref() == Some(thread))
			}) {
			self.reset_read_state();
		}
	}

	pub(super) fn refresh_read_state(&mut self, cx: &mut Context<Self>) {
		if self.read_state.target != self.read_state_target() {
			self.reset_read_state();
		}
		if self.read_state.read_at.is_none_or(|at| at.elapsed().as_secs() >= 5) {
			self.load_read_state(None, cx);
		}
	}

	fn load_read_state(&mut self, mark: Option<bool>, cx: &mut Context<Self>) {
		if self.read_state.task.is_some() || !self.command_connection_ready() {
			return;
		}
		let (Some(target), Some(profile), Some(source)) = (
			self.read_state_target(),
			self.profile.clone(),
			self.snapshot.as_ref().and_then(|s| s.runtime_source.clone()),
		) else {
			return;
		};
		let (Ok(work_id), Ok(thread_id)) =
			(EntityId::new(target.0.clone()), EntityId::new(target.1.clone()))
		else {
			return;
		};
		let action = if let Some(read) = mark {
			let Some(AgentReadStateResult::Available { revision, review_token, .. }) =
				&self.read_state.result
			else {
				return;
			};
			if self.read_state.target.as_ref() != Some(&target) {
				return;
			}
			Some(AgentActionDto::SetThreadReadState {
				work_id: work_id.clone(),
				thread_id: thread_id.clone(),
				revision: revision.clone(),
				review_token: review_token.clone(),
				read,
			})
		} else {
			None
		};
		self.read_state.target = Some(target.clone());
		self.read_state.epoch = self.read_state.epoch.wrapping_add(1);
		let epoch = self.read_state.epoch;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let applied = action.map(|action| {
				runtime
					.block_on(client.execute(
						action,
						IdempotencyKey::new(unique_command()).expect("command identity"),
					))
					.is_ok_and(|v| matches!(v, AgentCommandResponse::Accepted { .. }))
			});
			let read = runtime
				.block_on(client.read_state(work_id, thread_id))
				.unwrap_or(AgentReadStateResult::Unavailable);
			Some((applied, read))
		});
		self.read_state.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.read_state.epoch != epoch {
					return;
				}
				s.read_state.task = None;
				if s.read_state_target() != Some(target)
					|| s.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref()) != Some(&source)
				{
					s.reset_read_state();
					cx.notify();
					return;
				}
				let (applied, read) =
					result.unwrap_or((mark.map(|_| false), AgentReadStateResult::Unavailable));
				s.read_state.result = Some(read);
				s.read_state.read_at = Some(Instant::now());
				if let Some(applied) = applied {
					s.read_state.feedback = if applied {
						"Native read status saved."
					} else {
						"The mark was rejected or unconfirmed. Check the refreshed status before trying again."
					}
					.into();
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn read_state_panel(&self, cx: &mut Context<Self>) -> AnyElement {
		if !self.command_connection_ready() || self.read_state_target().is_none() {
			return gpui::div().into_any_element();
		}
		let current = (self.read_state.target == self.read_state_target())
			.then_some(self.read_state.result.as_ref())
			.flatten();
		let mut panel = gpui::div().flex().flex_col().gap_2().child(read_text(current));
		if self.read_state.task.is_none() {
			panel = panel.child(self.workspace_action(
				"native-read-state-refresh".into(),
				"Refresh read status".into(),
				|s, cx| s.load_read_state(None, cx),
				cx,
			));
			if let Some(AgentReadStateResult::Available { first_unread, .. }) = current {
				let read = first_unread.is_some();
				panel = panel.child(self.workspace_action(
					"native-read-state-mark".into(),
					if read { "Mark as read" } else { "Mark as unread" }.into(),
					move |s, cx| s.load_read_state(Some(read), cx),
					cx,
				));
			}
		}
		panel.child(self.read_state.feedback.clone()).into_any_element()
	}
}
fn read_text(result: Option<&AgentReadStateResult>) -> &'static str {
	match result {
		Some(AgentReadStateResult::Available { first_unread: None, .. }) => "Conversation read",
		Some(AgentReadStateResult::Available {
			first_unread: Some(AgentUnreadPosition::ThreadStart),
			..
		}) => "Conversation marked unread",
		Some(AgentReadStateResult::Available {
			first_unread: Some(AgentUnreadPosition::Turn { .. }),
			..
		}) => "Conversation has unread results",
		Some(AgentReadStateResult::Unavailable) => "Native read status unavailable",
		None => "Reading conversation status…",
	}
}

#[cfg(test)]
#[path = "agent_read_state_tests.rs"]
mod tests;
