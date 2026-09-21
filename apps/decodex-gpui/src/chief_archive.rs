//! Explicit restoration of a native archived task, using fresh desired-state readback.
use super::*;
use decodex_protocol::ChiefArchiveResult as State;

#[derive(Default)]
pub(super) struct Panel {
	owner: Option<String>,
	result: Option<State>,
	expanded: bool,
	epoch: u64,
	last_read: Option<std::time::Instant>,
	request: Option<Task<()>>,
	mutation: Option<Task<()>>,
	mutation_key: Option<String>,
	pub(super) feedback: String,
}

impl ChiefSurface {
	pub(super) fn archive_disconnected(&mut self) {
		self.archive.epoch = self.archive.epoch.wrapping_add(1);
		self.archive.result = None;
		self.archive.request = None;
		self.archive.mutation = None;
		self.archive.mutation_key = None;
		self.archive.last_read = None;
	}

	pub(super) fn load_archive_state(&mut self, force: bool, cx: &mut Context<Self>) {
		let Some(work) = self.selected.clone() else {
			return;
		};
		if self.archive.owner.as_ref() != Some(&work) {
			self.archive = Panel {
				owner: Some(work.clone()),
				epoch: self.archive.epoch.wrapping_add(1),
				..Default::default()
			};
		}
		if self.archive.request.is_some() || self.archive.mutation.is_some() {
			return;
		}
		if !force
			&& self.archive.last_read.is_some_and(|at| {
				!self.archive.expanded || at.elapsed() < std::time::Duration::from_secs(15)
			}) {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			self.archive.result = Some(State::Unavailable);
			return;
		};
		let Ok(work_id) = EntityId::new(work.clone()) else {
			return;
		};
		let generation = self.generation;
		let epoch = self.archive.epoch;
		self.archive.last_read = Some(std::time::Instant::now());
		// Disable restoration while inspecting; never reuse a stale archived result.
		self.archive.result = None;
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).archive_state(work_id)).ok()
		});
		self.archive.request = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(State::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation
					|| s.selected.as_ref() != Some(&work)
					|| s.archive.epoch != epoch
				{
					return;
				}
				s.archive.request = None;
				s.archive.feedback.clear();
				if matches!(result, State::Archived { .. }) {
					s.archive.expanded = true;
				}
				s.archive.result = Some(result);
				cx.notify();
			});
		}));
	}

	fn restore_archive(&mut self, work: &str, thread: &str, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work)
			|| self.archive.owner.as_deref() != Some(work)
			|| self.archive.mutation.is_some()
			|| self.archive.request.is_some()
			|| !matches!(&self.archive.result,Some(State::Archived {thread_id}) if thread_id==thread)
		{
			return;
		}
		if self.profile.is_none() {
			self.archive.feedback = "No service profile is configured.".into();
			cx.notify();
			return;
		}
		let (Some(profile), Ok(work_id), Ok(thread_id)) = (
			self.profile.clone(),
			EntityId::new(work.to_owned()),
			WireText::new(thread.to_owned()),
		) else {
			return;
		};
		let generation = self.generation;
		let epoch = self.archive.epoch;
		let work = work.to_owned();
		let key = unique_command();
		let command_key = IdempotencyKey::new(key.clone()).expect("bounded identity");
		self.archive.mutation_key = Some(key.clone());
		self.archive.result = None;
		self.archive.feedback = "Restoring the original task…".into();
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
			let expected_thread = thread_id.as_str().to_owned();
			let result = runtime.block_on(client.execute(
				ChiefActionDto::RestoreArchivedThread { work_id: work_id.clone(), thread_id },
				command_key,
			));
			let state =
				runtime.block_on(client.archive_state(work_id)).unwrap_or(State::Unavailable);
			Some((result, bind_readback(state, &expected_thread)))
		});
		self.archive.mutation = Some(cx.spawn(async move |surface, cx| {
			let completed = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation
					|| s.selected.as_ref() != Some(&work)
					|| s.archive.epoch != epoch
					|| s.archive.mutation_key.as_ref() != Some(&key)
				{
					return;
				}
				s.archive.mutation = None;
				s.archive.mutation_key = None;
				let (feedback, state) = match completed {
					Some((_, State::Active { thread_id })) => (
						"The original task is active. Previously queued work can continue.",
						State::Active { thread_id },
					),
					Some((Err(_), state)) =>
						("No restore request was sent. Check the service connection.", state),
					Some((Ok(ChiefCommandResponse::Rejected { .. }), state)) => (
						"Restoration was not accepted. Review the refreshed state before trying again.",
						state,
					),
					Some((_, state)) => (
						"Restoration is unconfirmed. Refresh the state before another explicit attempt.",
						state,
					),
					None => (
						"Restoration is unconfirmed. Refresh the state before another explicit attempt.",
						State::Unavailable,
					),
				};
				s.archive.feedback = feedback.into();
				s.archive.result = Some(state);
				s.archive.last_read = Some(std::time::Instant::now());
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn archive_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		if self.archive.owner.as_deref() != Some(&work.id) {
			return div().into_any_element();
		}
		let mut panel = div().flex().flex_col().gap_2().child(button(
			"archive-toggle",
			"Session recovery",
			cx,
			|s, cx| {
				s.archive.expanded = !s.archive.expanded;
				if s.archive.expanded {
					s.load_archive_state(true, cx);
				}
				cx.notify();
			},
		));
		if !self.archive.expanded {
			return panel.into_any_element();
		}
		let status = match &self.archive.result {
			None if self.archive.mutation.is_some() => "Waiting for restore confirmation…",
			None => "Checking the original session…",
			Some(State::Active { .. }) => "This session is active in Codex.",
			Some(State::Archived { .. }) =>
				"This session is archived in Codex. Restore it to continue with its original history. Previously queued work can then continue.",
			Some(State::Unbound) => "This task does not have a Codex session yet.",
			Some(State::Unconfirmed) =>
				"The session state is unconfirmed. It may have changed in another client.",
			Some(State::Unsupported) => "This Codex provider cannot report archive state.",
			Some(State::CapacityExceeded) =>
				"The session list exceeds the inspection limit. Archive state is unconfirmed.",
			Some(State::Unavailable) =>
				"Session state is unavailable. Reconnect and refresh before restoring.",
		};
		panel = panel.child(status);
		if self.archive.request.is_none() && self.archive.mutation.is_none() {
			panel = panel.child(button("archive-refresh", "Refresh session state", cx, |s, cx| {
				s.load_archive_state(true, cx)
			}));
			if let Some(State::Archived { thread_id }) = &self.archive.result {
				let work = work.id.clone();
				let thread = thread_id.clone();
				panel = panel.child(button(
					"archive-restore",
					"Restore original session",
					cx,
					move |s, cx| s.restore_archive(&work, &thread, cx),
				));
			}
		}
		panel.into_any_element()
	}
}

fn bind_readback(state: State, expected: &str) -> State {
	match &state {
		State::Active { thread_id } | State::Archived { thread_id } if thread_id != expected =>
			State::Unconfirmed,
		_ => state,
	}
}

fn button(
	id: &'static str,
	label: &'static str,
	cx: &mut Context<ChiefSurface>,
	action: impl Fn(&mut ChiefSurface, &mut Context<ChiefSurface>) + 'static,
) -> gpui::AnyElement {
	let action = std::rc::Rc::new(action);
	let click = action.clone();
	div()
		.id(id)
		.debug_selector(move || id.to_owned())
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label)
		.cursor_pointer()
		.text_color(rgb(ui_theme::BLUE))
		.py_1()
		.on_click(cx.listener(move |s, _, _, cx| click(s, cx)))
		.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
			if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				cx.stop_propagation();
				action(s, cx);
			}
		}))
		.child(label)
		.into_any_element()
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn restored_state_cannot_be_attributed_to_a_rebound_native_thread() {
		for state in
			[State::Active { thread_id: "new".into() }, State::Archived { thread_id: "new".into() }]
		{
			assert_eq!(bind_readback(state, "original"), State::Unconfirmed);
		}
		let state = State::Active { thread_id: "original".into() };
		assert_eq!(bind_readback(state.clone(), "original"), state);
	}
	#[gpui::test]
	fn restore_control_requires_fresh_archive_identity_and_explicit_click_or_keyboard(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, _| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				workspaces: vec![],
				dependencies: vec![],
				pending_events: vec![],
				work_items: vec![ChiefWorkItemDto {
					id: "root".into(),
					parent_goal_id: None,
					kind: decodex_protocol::ChiefWorkKindDto::Goal,
					title: "Chief".into(),
					codex_thread_id: Some("thread".into()),
					active_turn_id: None,
					dispatch_state: ChiefDispatchStateDto::Idle,
					status: ChiefWorkStatusDto::Open,
					next_check_at_micros: None,
					created_at_micros: 1,
					updated_at_micros: 1,
				}],
			})));
			s.archive = Panel {
				owner: Some("root".into()),
				result: Some(State::Archived { thread_id: "thread".into() }),
				expanded: true,
				..Default::default()
			};
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.), px(1200.)));
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("archive-restore").expect("explicit restore control");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert_eq!(s.archive.feedback, "No service profile is configured.");
			s.archive.feedback.clear();
			cx.notify();
		});
		visual.simulate_keystrokes("space");
		surface.update(visual, |s, cx| {
			assert_eq!(s.archive.feedback, "No service profile is configured.");
			s.archive.feedback.clear();
			s.restore_archive("root", "old-thread", cx);
			assert!(s.archive.feedback.is_empty());
			s.archive_disconnected();
			s.restore_archive("root", "thread", cx);
			assert!(s.archive.feedback.is_empty());
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("archive-restore").is_none());
	}
}
