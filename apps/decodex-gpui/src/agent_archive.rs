//! Explicit restoration of a native archived task, using fresh desired-state readback.
use std::{rc::Rc, time::Duration};

use gpui::{AnyElement, KeyDownEvent};
use tokio::runtime::Builder;
use ui_theme::{BLUE, PANEL_HEADER_TINT, TEXT_MUTED};

#[cfg(test)] use crate::shell::agent_surface::AgentDispatchStateDto;
#[cfg(test)] use crate::shell::agent_surface::AgentSnapshotDto;
#[cfg(test)] use crate::shell::agent_surface::AgentSnapshotResult;
#[cfg(test)] use crate::shell::agent_surface::AgentWorkStatusDto;
use crate::shell::agent_surface::{
	self, AgentActionDto, AgentClient, AgentCommandResponse, AgentSurface, AgentWorkItemDto,
	Context, EntityId, FluentBuilder, IdempotencyKey, InteractiveElement, IntoElement,
	ParentElement, Role, StatefulInteractiveElement, Styled, Task, WireText, ui_theme,
};
use decodex_protocol::AgentArchiveResult;

#[derive(Default)]
pub(super) struct Panel {
	owner: Option<String>,
	result: Option<AgentArchiveResult>,
	epoch: u64,
	last_read: Option<std::time::Instant>,
	request: Option<Task<()>>,
	mutation: Option<Task<()>>,
	mutation_key: Option<String>,
	pub(super) feedback: String,
}
impl Panel {
	fn apply_read(&mut self, result: AgentArchiveResult, explicit: bool) {
		let failed = matches!(
			result,
			AgentArchiveResult::Unavailable
				| AgentArchiveResult::Unconfirmed
				| AgentArchiveResult::CapacityExceeded
		);

		if failed {
			if explicit {
				self.feedback = "Could not check archive status. Try again.".into();
			}
			// An incomplete background read must not replace the last confirmed state.
		} else {
			self.feedback.clear();

			self.result = Some(result);
		}
	}
}

impl AgentSurface {
	pub(super) fn archive_disconnected(&mut self) {
		self.archive.epoch = self.archive.epoch.wrapping_add(1);
		self.archive.result = None;
		self.archive.request = None;
		self.archive.mutation = None;
		self.archive.mutation_key = None;
		self.archive.last_read = None;

		self.archive.feedback.clear();
	}

	pub(crate) fn load_archive_state(&mut self, force: bool, cx: &mut Context<Self>) {
		if self.connection_initializing() {
			return;
		}
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
		if !force && self.archive.last_read.is_some_and(|at| at.elapsed() < Duration::from_secs(15))
		{
			return;
		}

		let Some(profile) = self.profile.clone() else {
			self.archive.result = Some(AgentArchiveResult::Unavailable);

			return;
		};
		let Ok(work_id) = EntityId::new(work.clone()) else {
			return;
		};
		// Ordinary snapshot refreshes do not invalidate an archive request.
		// The archive epoch changes only with its owner or service connection.
		let epoch = self.archive.epoch;

		self.archive.last_read = Some(std::time::Instant::now());

		// Keep the archived reading view stable while checking. The request guard
		// disables Unarchive until this read completes.
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime.block_on(AgentClient::new(profile).archive_state(work_id)).ok()
		});

		self.archive.request = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(AgentArchiveResult::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if !s.complete_archive_read(&work, epoch, result, force) {
					return;
				}

				cx.notify();
			});
		}));
	}

	fn complete_archive_read(
		&mut self,
		work: &str,
		epoch: u64,
		result: AgentArchiveResult,
		explicit: bool,
	) -> bool {
		if self.selected.as_deref() != Some(work)
			|| self.archive.owner.as_deref() != Some(work)
			|| self.archive.epoch != epoch
		{
			return false;
		}

		self.archive.request = None;

		self.archive.apply_read(result, explicit);

		true
	}

	fn restore_archive(&mut self, work: &str, thread: &str, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work)
			|| self.archive.owner.as_deref() != Some(work)
			|| self.archive.mutation.is_some()
			|| self.archive.request.is_some()
			|| !matches!(&self.archive.result,Some(AgentArchiveResult::Archived {thread_id}) if thread_id==thread)
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
		// Ordinary snapshot refreshes do not invalidate an archive request.
		// The archive epoch changes only with its owner or service connection.
		let epoch = self.archive.epoch;
		let work = work.to_owned();
		let key = agent_surface::unique_command();
		let command_key = IdempotencyKey::new(key.clone()).expect("bounded identity");

		self.archive.mutation_key = Some(key.clone());
		self.archive.result = None;
		self.archive.feedback = "Restoring the original task…".into();

		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let expected_thread = thread_id.as_str().to_owned();
			let result = runtime.block_on(client.execute(
				AgentActionDto::RestoreArchivedThread { work_id: work_id.clone(), thread_id },
				command_key,
			));
			let state = runtime
				.block_on(client.archive_state(work_id))
				.unwrap_or(AgentArchiveResult::Unavailable);

			Some((result, bind_readback(state, &expected_thread)))
		});

		self.archive.mutation = Some(cx.spawn(async move |surface, cx| {
			let completed = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.selected.as_ref() != Some(&work)
					|| s.archive.epoch != epoch
					|| s.archive.mutation_key.as_ref() != Some(&key)
				{
					return;
				}

				s.archive.mutation = None;
				s.archive.mutation_key = None;

				let (feedback, state) = match completed {
					Some((_, AgentArchiveResult::Active { thread_id })) => (
						"The original task is active. Previously queued work can continue.",
						AgentArchiveResult::Active { thread_id },
					),
					Some((Err(_), state)) =>
						("No restore request was sent. Check the service connection.", state),
					Some((Ok(AgentCommandResponse::Rejected { .. }), state)) => (
						"Restoration was not accepted. Review the refreshed state before trying again.",
						state,
					),
					Some((_, state)) => (
						"Restoration is unconfirmed. Refresh the state before another explicit attempt.",
						state,
					),
					None => (
						"Restoration is unconfirmed. Refresh the state before another explicit attempt.",
						AgentArchiveResult::Unavailable,
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

	pub(super) fn selected_is_archived(&self) -> bool {
		self.native_agents.selected.is_none()
			&& self.archive.owner == self.selected
			&& (matches!(self.archive.result, Some(AgentArchiveResult::Archived { .. }))
				|| self.archive.mutation.is_some())
	}

	pub(super) fn archive_panel(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		if self.archive.owner.as_deref() != Some(&work.id) {
			return gpui::div().into_any_element();
		}

		let restoring = self.archive.mutation.is_some();
		let thread = match &self.archive.result {
			Some(AgentArchiveResult::Archived { thread_id }) => Some(thread_id.clone()),
			_ if restoring => None,
			_ => return gpui::div().into_any_element(),
		};
		let work = work.id.clone();

		gpui::div()
			.w_full()
			.flex_none()
			.px(gpui::px(20.))
			.py(gpui::px(10.))
			.flex()
			.items_center()
			.justify_between()
			.gap_3()
			.bg(gpui::rgba(PANEL_HEADER_TINT))
			.text_size(gpui::px(11.))
			.child(
				gpui::div()
					.flex()
					.flex_col()
					.gap_1()
					.child(if restoring { "Unarchiving…" } else { "Archived" })
					.child(
						gpui::div()
							.text_color(gpui::rgb(TEXT_MUTED))
							.child("History is available. Unarchive to continue."),
					),
			)
			.when_some(thread.filter(|_| self.archive.request.is_none()), |panel, thread| {
				panel.child(button("archive-restore", "Unarchive", cx, move |s, cx| {
					s.restore_archive(&work, &thread, cx)
				}))
			})
			.into_any_element()
	}
}

fn bind_readback(state: AgentArchiveResult, expected: &str) -> AgentArchiveResult {
	match &state {
		AgentArchiveResult::Active { thread_id } | AgentArchiveResult::Archived { thread_id }
			if thread_id != expected =>
			AgentArchiveResult::Unconfirmed,
		_ => state,
	}
}

fn button(
	id: &'static str,
	label: &'static str,
	cx: &mut Context<AgentSurface>,
	action: impl Fn(&mut AgentSurface, &mut Context<AgentSurface>) + 'static,
) -> AnyElement {
	let action = Rc::new(action);
	let click = action.clone();

	gpui::div()
		.id(id)
		.debug_selector(move || id.to_owned())
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label)
		.cursor_pointer()
		.text_color(gpui::rgb(BLUE))
		.py_1()
		.on_click(cx.listener(move |s, _, _, cx| click(s, cx)))
		.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
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
	use std::{future, thread, time::Duration};

	use crate::shell::agent_surface::archive::{
		self, AgentArchiveResult, AgentSurface, AgentWorkItemDto, Panel,
	};
	#[cfg(test)]
	use crate::shell::agent_surface::archive::{
		AgentDispatchStateDto, AgentSnapshotDto, AgentSnapshotResult, AgentWorkStatusDto,
	};

	#[gpui::test]
	fn snapshot_refresh_does_not_strand_archive_read(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.selected = Some("root".into());
			s.archive.owner = s.selected.clone();

			let epoch = s.archive.epoch;

			s.archive.request = Some(cx.spawn(async |_, _| future::pending::<()>().await));
			s.generation += 1; // An unrelated snapshot request starts before the archive reply.

			assert!(s.complete_archive_read(
				"root",
				epoch,
				AgentArchiveResult::Active { thread_id: "thread".into() },
				false
			));
			assert!(s.archive.request.is_none());
			assert!(matches!(s.archive.result, Some(AgentArchiveResult::Active { .. })));

			s.archive_disconnected();

			assert!(!s.complete_archive_read(
				"root",
				epoch,
				AgentArchiveResult::Archived { thread_id: "thread".into() },
				false
			));
			assert!(s.archive.result.is_none());
		});
	}

	#[test]
	fn restored_state_cannot_be_attributed_to_a_rebound_native_thread() {
		for state in [
			AgentArchiveResult::Active { thread_id: "new".into() },
			AgentArchiveResult::Archived { thread_id: "new".into() },
		] {
			assert_eq!(archive::bind_readback(state, "original"), AgentArchiveResult::Unconfirmed);
		}

		let state = AgentArchiveResult::Active { thread_id: "original".into() };

		assert_eq!(archive::bind_readback(state.clone(), "original"), state);
	}

	#[gpui::test]
	fn archive_read_only_applies_only_to_the_selected_confirmed_thread(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.selected = Some("root".into());
			s.archive.owner = Some("root".into());
			s.archive.result = Some(AgentArchiveResult::Archived { thread_id: "thread".into() });

			assert!(s.selected_is_archived());

			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
			s.submit(cx);

			assert!(!s.sending);
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");

			s.selected = Some("other".into());

			assert!(!s.selected_is_archived());

			s.selected = Some("root".into());

			for state in [
				AgentArchiveResult::Active { thread_id: "thread".into() },
				AgentArchiveResult::Unavailable,
				AgentArchiveResult::Unconfirmed,
			] {
				s.archive.result = Some(state);

				assert!(!s.selected_is_archived());
			}
		});
	}

	#[gpui::test]
	fn restore_control_requires_fresh_archive_identity_and_explicit_click_or_keyboard(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
				connection_initializing: false,
				runtime_source: None,
				workspaces: vec![],
				dependencies: vec![],
				pending_events: vec![],
				work_items: vec![AgentWorkItemDto {
					id: "root".into(),
					parent_goal_id: None,
					kind: decodex_protocol::AgentWorkKindDto::Goal,
					title: "Agent".into(),
					codex_thread_id: Some("thread".into()),
					active_turn_id: None,
					dispatch_state: AgentDispatchStateDto::Idle,
					status: AgentWorkStatusDto::Open,
					next_check_at_micros: None,
					created_at_micros: 1,
					updated_at_micros: 1,
				}],
			})));

			s.archive = Panel {
				owner: Some("root".into()),
				result: Some(AgentArchiveResult::Archived { thread_id: "thread".into() }),
				..Default::default()
			};
		});

		visual.simulate_resize(gpui::size(gpui::px(1_180.), gpui::px(1_200.)));
		visual.update(|window, cx| {
			assert_eq!(window.viewport_size(), gpui::size(gpui::px(1_180.), gpui::px(1_200.)));

			window.draw(cx).clear();
		});

		// Read hit-test bounds after the workspace panels finish their 200 ms transition.
		thread::sleep(Duration::from_millis(240));

		visual.update(|window, cx| window.draw(cx).clear());

		let bounds = visual.debug_bounds("archive-restore").expect("explicit restore control");

		visual.simulate_mouse_down(
			bounds.center(),
			gpui::MouseButton::Left,
			gpui::Modifiers::default(),
		);
		visual.update(|window, cx| window.draw(cx).clear());

		assert_eq!(visual.debug_bounds("archive-restore"), Some(bounds));

		visual.simulate_mouse_up(
			bounds.center(),
			gpui::MouseButton::Left,
			gpui::Modifiers::default(),
		);
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
#[cfg(test)]
mod background_read_tests {
	use crate::shell::agent_surface::archive::{AgentArchiveResult, Panel};

	#[test]
	fn transient_read_does_not_flash_error_or_replace_known_state() {
		let mut panel = Panel::default();

		panel.apply_read(AgentArchiveResult::Active { thread_id: "thread".into() }, false);
		panel.apply_read(AgentArchiveResult::Unavailable, false);

		assert!(panel.feedback.is_empty());
		assert!(matches!(panel.result, Some(AgentArchiveResult::Active { .. })));

		for failure in [
			AgentArchiveResult::Unavailable,
			AgentArchiveResult::Unconfirmed,
			AgentArchiveResult::CapacityExceeded,
		] {
			panel.apply_read(failure, false);

			assert!(panel.feedback.is_empty(), "background polling must not insert a banner");
			assert!(matches!(panel.result, Some(AgentArchiveResult::Active { .. })));
		}

		panel.apply_read(AgentArchiveResult::Unavailable, true);

		assert_eq!(panel.feedback, "Could not check archive status. Try again.");

		panel.apply_read(AgentArchiveResult::Active { thread_id: "thread".into() }, false);

		assert!(panel.feedback.is_empty());
	}
}
