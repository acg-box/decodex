//! The native Pinned section is shared. Only the click destination depends on registration.
use super::{AgentSurface, Context, Item};
use decodex_protocol::{
	AgentActionDto, AgentClient, AgentCommandResponse, EntityId, IdempotencyKey,
	NativeThreadCandidate,
};
use gpui::{Subscription, Task, Window};
use tokio::runtime::Builder;

#[derive(Default)]
pub(super) struct Pins {
	pub(super) rows: Option<Vec<NativeThreadCandidate>>,
	task: Option<Task<()>>,
	saving: bool,
	focus: Option<Subscription>,
	pub(super) error: Option<String>,
}

impl AgentSurface {
	pub(in crate::shell::agent_surface) fn observe_navigation_pins(
		&mut self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if self.workspace.navigation.pins.focus.is_some()
			|| self.profile.is_none()
			|| self.snapshot.is_none()
		{
			return;
		}
		self.workspace.navigation.pins.focus =
			Some(cx.observe_window_activation(window, |s, window, cx| {
				if window.is_window_active() {
					s.refresh_navigation_pins(cx);
				}
			}));
		self.refresh_navigation_pins(cx);
	}

	pub(in crate::shell::agent_surface) fn native_pin_state(&self, thread: &str) -> Option<bool> {
		let pins = &self.workspace.navigation.pins;
		pins.rows.as_ref().map(|rows| rows.iter().any(|c| c.thread_id == thread))
	}

	pub(in crate::shell::agent_surface) fn pin_error(&self) -> Option<&str> {
		self.workspace.navigation.pins.error.as_deref()
	}

	pub(in crate::shell::agent_surface) fn pins_ready(&self) -> bool {
		let pins = &self.workspace.navigation.pins;
		!pins.saving && pins.rows.is_some() && pins.error.is_none()
	}

	pub(in crate::shell::agent_surface) fn refresh_navigation_pins(
		&mut self,
		cx: &mut Context<Self>,
	) {
		self.sync_navigation_pins(None, cx);
	}

	pub(in crate::shell::agent_surface) fn change_native_pin(
		&mut self,
		thread: &str,
		pinned: bool,
		cx: &mut Context<Self>,
	) {
		if self.pins_ready()
			&& let Ok(id) = EntityId::new(thread)
		{
			self.sync_navigation_pins(
				Some(AgentActionDto::SetThreadPinned { thread_id: id, pinned }),
				cx,
			);
		}
	}

	fn sync_navigation_pins(&mut self, action: Option<AgentActionDto>, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else { return };
		let pins = &mut self.workspace.navigation.pins;
		if pins.saving || (pins.task.is_some() && action.is_none()) {
			return;
		}
		// A click supersedes a background read. Its own read observes the completed write.
		pins.saving = action.is_some();
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			Some(runtime.block_on(async {
				let client = AgentClient::new(profile);
				let failure = if let Some(action) = action {
					let key = IdempotencyKey::new(super::super::unique_command())
						.expect("generated command identity");
					match client.execute(action, key).await {
						Ok(AgentCommandResponse::Accepted { .. }) => None,
						Ok(AgentCommandResponse::Rejected { error }) =>
							Some(super::super::command_rejection_message(&error)),
						_ =>
							Some("Could not confirm the change to the pinned conversation.".into()),
					}
				} else {
					None
				};
				(client.pinned_threads().await.ok(), failure)
			}))
		});
		self.workspace.navigation.pins.task = Some(cx.spawn(async move |owner, cx| {
			let (rows, failure) = request.await.unwrap_or_default();
			let _ = owner.update(cx, |s, cx| {
				let pins = &mut s.workspace.navigation.pins;
				pins.task = None;
				pins.saving = false;
				pins.error = if let Some(rows) = rows {
					pins.rows = Some(rows);
					failure
				} else {
					Some("Could not load pinned conversations from Codex.".into())
				};
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn item_thread<'a>(&'a self, item: &'a Item) -> Option<&'a str> {
		if let Some(candidate) = &item.external {
			return Some(&candidate.thread_id);
		}
		self.conversation_record(&item.id)?.codex_thread_id.as_deref()
	}

	pub(super) fn pinned_items(&self) -> Vec<Item> {
		self.workspace
			.navigation
			.pins
			.rows
			.iter()
			.flatten()
			.filter_map(|candidate| {
				if let Some(snapshot) = &self.snapshot
					&& let Some(work) = snapshot
						.work_items
						.iter()
						.find(|w| w.codex_thread_id.as_ref() == Some(&candidate.thread_id))
				{
					if snapshot.side_chat_sources.contains_key(&work.id) {
						return None;
					}
					return Some(self.local_navigation_item(
						snapshot,
						&work.id,
						self.conversation_label(work),
						Some(work),
					));
				}
				Some(Item {
					id: candidate.thread_id.clone(),
					title: candidate.title.clone(),
					location: "Codex preview".into(),
					project: None,
					updated: candidate.updated_at_micros,
					draft: false,
					assistant: false,
					archived: false,
					state: "Idle".into(),
					external: Some(candidate.clone()),
				})
			})
			.collect()
	}
}
