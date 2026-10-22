//! Explicit restoration of a native archived task, using fresh desired-state readback.
use std::{rc::Rc, time::Duration};

use gpui::{AnyElement, KeyDownEvent};
use tokio::runtime::Builder;
use ui_theme::{BLUE, PANEL_HEADER_TINT, TEXT_MUTED};

#[cfg(test)] use crate::shell::agent_surface::AgentDispatchStateDto;
#[cfg(test)] use crate::shell::agent_surface::AgentSnapshotDto;
#[cfg(test)] use crate::shell::agent_surface::AgentSnapshotResult;
#[cfg(test)] use crate::shell::agent_surface::AgentWorkStatusDto;
use crate::{
	shell::agent_surface::{
		self, AgentActionDto, AgentClient, AgentCommandResponse, AgentSurface, AgentWorkItemDto,
		Context, EntityId, FluentBuilder, IdempotencyKey, InteractiveElement, IntoElement,
		ParentElement, Role, StatefulInteractiveElement, Styled, Task, WireText, ui_theme,
	},
	ui_motion::SmoothControl as _,
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
		// Snapshot refreshes do not invalidate this read. Selection, reconnection
		// and an explicit restore each supersede its archive epoch.
		let epoch = self.archive.epoch;

		self.archive.last_read = Some(std::time::Instant::now());

		// Keep the confirmed banner and action stable during background checks.
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
		// An explicit restore supersedes a background check. Cancel its UI task
		// and reject any late reply before starting the desired-state readback.
		self.archive.epoch = self.archive.epoch.wrapping_add(1);
		self.archive.request = None;
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

	pub(super) fn work_is_archived(&self, id: &str) -> bool {
		self.snapshot.as_ref().is_some_and(|s| s.archived_work_ids.contains(id))
	}

	pub(super) fn selected_is_archived(&self) -> bool {
		self.native_agents.selected.is_none()
			&& (self.selected.as_ref().is_some_and(|id| self.work_is_archived(id))
				|| self.archive.owner == self.selected
					&& (matches!(self.archive.result, Some(AgentArchiveResult::Archived { .. }))
						|| self.archive.mutation.is_some()))
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
			.debug_selector(|| "archive-banner".into())
			.w_full()
			.flex_none()
			.px(gpui::px(20.))
			// Leave a separate hit target for the conversation fullscreen control.
			.pr(gpui::px(48.))
			.py(gpui::px(10.))
			.flex()
			.items_center()
			.justify_between()
			.gap_3()
			.bg(crate::ui_theme::rgba(PANEL_HEADER_TINT))
			.text_size(gpui::px(11.))
			.child(
				gpui::div()
					.flex()
					.flex_col()
					.gap_1()
					.child(if restoring { "Unarchiving…" } else { "Archived" })
					.child(
						gpui::div()
							.text_color(crate::ui_theme::rgb(TEXT_MUTED))
							.child("History is available. Unarchive to continue."),
					),
			)
			.when_some(thread, |panel, thread| {
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
		.text_color(crate::ui_theme::rgb(BLUE))
		.h(gpui::px(ui_theme::CONTROL_SIZE))
		.px_2()
		.flex_none()
		.flex()
		.items_center()
		.justify_center()
		.rounded(gpui::px(5.))
		.hover(|button| button.bg(crate::ui_theme::rgba(ui_theme::HOVER_FILL)))
		.on_click(cx.listener(move |s, _, _, cx| click(s, cx)))
		.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
			if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				cx.stop_propagation();

				action(s, cx);
			}
		}))
		.child(label)
		.smooth()
		.into_any_element()
}

#[cfg(test)]
mod tests {
	use gpui::AppContext as _;
	use std::{future, thread, time::Duration};

	use crate::shell::agent_surface::archive::{
		self, AgentArchiveResult, AgentSurface, AgentWorkItemDto, Panel,
	};
	#[cfg(test)]
	use crate::shell::agent_surface::archive::{
		AgentDispatchStateDto, AgentSnapshotDto, AgentSnapshotResult, AgentWorkStatusDto,
	};

	#[gpui::test]
	fn background_archive_check_keeps_restore_control_in_place(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.selected = Some("agent".into());
			s.archive = Panel {
				owner: s.selected.clone(),
				result: Some(AgentArchiveResult::Archived { thread_id: "thread".into() }),
				..Default::default()
			};
			cx.notify();
		});
		visual.simulate_resize(gpui::size(gpui::px(1180.), gpui::px(1200.)));
		visual.update(crate::ui_frame::draw_frame);
		thread::sleep(Duration::from_millis(240));
		visual.update(crate::ui_frame::draw_frame);
		let bounds = visual.debug_bounds("archive-restore").expect("restore action");
		let epoch = surface.update(visual, |s, cx| {
			s.archive.request = Some(cx.spawn(async |_, _| future::pending::<()>().await));
			cx.notify();
			s.archive.epoch
		});
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(visual.debug_bounds("archive-restore"), Some(bounds));
		visual.simulate_click(bounds.center(), Default::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.archive.feedback, "No service profile is configured.");
			assert!(s.complete_archive_read(
				"agent",
				epoch,
				AgentArchiveResult::Archived { thread_id: "thread".into() },
				false
			));
		});
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(visual.debug_bounds("archive-restore"), Some(bounds));
	}

	#[gpui::test]
	fn explicit_restore_supersedes_an_old_background_read(cx: &mut gpui::TestAppContext) {
		let (_root, profile, _) = super::super::drafts::tests::profiles();
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.profile = Some(profile);
			s.selected = Some("agent".into());
			s.archive = Panel {
				owner: s.selected.clone(),
				result: Some(AgentArchiveResult::Archived { thread_id: "thread".into() }),
				..Default::default()
			};
			s.archive.request = Some(cx.spawn(async |_, _| future::pending::<()>().await));
			let old_epoch = s.archive.epoch;
			s.restore_archive("agent", "thread", cx);
			assert!(s.archive.request.is_none());
			assert!(s.archive.mutation.is_some());
			let key = s.archive.mutation_key.clone();
			assert!(key.is_some());
			assert!(!s.complete_archive_read(
				"agent",
				old_epoch,
				AgentArchiveResult::Archived { thread_id: "thread".into() },
				false
			));
			assert!(s.archive.result.is_none());
			s.restore_archive("agent", "thread", cx);
			assert_eq!(s.archive.mutation_key, key, "one explicit restoration at a time");
		});
	}

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
				archived_work_ids: Default::default(),
				agent_slots: Default::default(),
				context_references: vec![],
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

			window.draw(cx).clear(cx);
		});

		// Read hit-test bounds after the workspace panels finish their 200 ms transition.
		thread::sleep(Duration::from_millis(240));

		visual.update(|window, cx| window.draw(cx).clear(cx));

		let bounds = visual.debug_bounds("archive-restore").expect("explicit restore control");
		let fullscreen = visual.debug_bounds("chat-expand").expect("conversation fullscreen");
		assert!(
			bounds.right() + gpui::px(8.) <= fullscreen.left(),
			"archive actions have separate hit targets"
		);

		visual.simulate_mouse_down(
			bounds.center(),
			gpui::MouseButton::Left,
			gpui::Modifiers::default(),
		);
		visual.update(|window, cx| window.draw(cx).clear(cx));

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
			window.draw(cx).clear(cx);
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

#[cfg(test)]
mod visibility_tests {
	use crate::shell::agent_surface::{
		AgentSnapshotResult, AgentSurface, Context, drafts::tests::profiles,
	};
	use decodex_protocol::{AgentAttachmentDto, AgentTaskReferenceDto, AgentWorkKindDto};

	fn fixture(s: &mut AgentSurface, cx: &mut Context<AgentSurface>) {
		s.visual_workspace_fixture(cx);
		let snapshot = s.snapshot.as_mut().unwrap();
		snapshot.work_items.retain(|w| matches!(w.id.as_str(), "agent" | "release"));
		snapshot.dependencies.clear();
		snapshot.pending_events.clear();
		for work in &mut snapshot.work_items {
			work.codex_thread_id = Some(format!("thread-{}", work.id));
			work.kind = AgentWorkKindDto::Manager;
		}
	}

	#[gpui::test]
	fn archived_selection_parks_exact_draft_and_restores_it_on_unarchive(
		cx: &mut gpui::TestAppContext,
	) {
		let (_service, profile, _) = profiles();
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.bind_drafts(Some(&profile), cx);
			fixture(s, cx);
			s.open_page("agent", cx);
			s.composer.update(cx, |input, cx| input.set_content("Main draft", cx));
			s.open_page("release", cx);
			s.composer.update(cx, |input, cx| input.set_content("Release draft", cx));
			let file = AgentAttachmentDto {
				path: decodex_protocol::ConversationWorkingDirectory::new("/tmp/kept.png").unwrap(),
				image: true,
				skill_name: None,
			};
			let reference = AgentTaskReferenceDto {
				work_id: decodex_protocol::EntityId::new("agent").unwrap(),
				thread_id: decodex_protocol::WireText::new("thread-agent").unwrap(),
				title: decodex_protocol::WireText::new("Main").unwrap(),
			};
			s.attachments = vec![file.clone()];
			s.task_references = vec![reference.clone()];
			let original = s.snapshot.clone().unwrap();
			let scope = original
				.work_items
				.iter()
				.find(|work| work.id == "release")
				.unwrap()
				.parent_goal_id
				.clone();
			assert!(
				crate::shell::agent_surface::graph::Layout::new(&original, scope.as_deref())
					.nodes
					.iter()
					.any(|node| node.id == "release")
			);
			let mut active = original.clone();
			active.work_items.retain(|w| w.id != "release");
			s.apply_refreshed_snapshot(Ok(AgentSnapshotResult::Available(active)), cx);
			assert!(
				!crate::shell::agent_surface::graph::Layout::new(
					s.snapshot.as_ref().unwrap(),
					scope.as_deref()
				)
				.nodes
				.iter()
				.any(|node| node.id == "release")
			);
			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert_eq!(s.composer_manager.as_deref(), Some("agent"));
			assert_eq!(s.composer.read(cx).content(), "Main draft");
			assert!(s.attachments.is_empty() && s.task_references.is_empty());
			assert_eq!(s.draft_profiles.texts["release"], "Release draft");
			assert_eq!(s.draft_profiles.files["release"].as_slice(), std::slice::from_ref(&file));
			assert_eq!(
				s.draft_profiles.tasks["release"].as_slice(),
				std::slice::from_ref(&reference)
			);
			assert!(!s.conversation_pages().iter().any(|(id, _, _)| id == "release"));
			s.apply_refreshed_snapshot(Ok(AgentSnapshotResult::Available(original)), cx);
			assert!(
				crate::shell::agent_surface::graph::Layout::new(
					s.snapshot.as_ref().unwrap(),
					scope.as_deref()
				)
				.nodes
				.iter()
				.any(|node| node.id == "release")
			);
			s.open_page("release", cx);
			assert_eq!(s.composer.read(cx).content(), "Release draft");
			assert_eq!(s.attachments, [file]);
			assert_eq!(s.task_references, [reference]);
			assert!(s.draft_owner_available());
		});
	}

	#[gpui::test]
	fn archived_root_cannot_retarget_an_initial_editor(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			fixture(s, cx);
			s.composer_manager = None;
			s.composer.update(cx, |input, cx| input.set_content("Root draft", cx));
			let mut active = s.snapshot.clone().unwrap();
			active.work_items.retain(|w| w.id == "release");
			active.work_items[0].parent_goal_id = None;
			s.apply_refreshed_snapshot(Ok(AgentSnapshotResult::Available(active)), cx);
			assert_eq!(s.selected.as_deref(), Some("release"));
			assert_eq!(s.composer_manager.as_deref(), Some("release"));
			assert!(s.composer.read(cx).content().is_empty());
			assert_eq!(s.draft_profiles.texts["agent"], "Root draft");
		});
	}

	#[gpui::test]
	fn a_new_conversation_after_archiving_all_owners_parks_the_original_draft(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			fixture(s, cx);
			s.open_page("agent", cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep the archived draft", cx));
			let original = s.snapshot.clone().unwrap();
			let mut empty = original.clone();
			empty.work_items.clear();
			s.apply_refreshed_snapshot(Ok(AgentSnapshotResult::Available(empty)), cx);
			assert!(s.selected.is_none());
			assert_eq!(s.composer_manager.as_deref(), Some("agent"));
			let mut fresh = original;
			fresh.work_items.retain(|w| w.id == "release");
			fresh.work_items[0].parent_goal_id = None;
			s.apply_refreshed_snapshot(Ok(AgentSnapshotResult::Available(fresh)), cx);
			assert_eq!(s.selected.as_deref(), Some("release"));
			assert_eq!(s.composer_manager.as_deref(), Some("release"));
			assert!(s.composer.read(cx).content().is_empty());
			assert_eq!(s.draft_profiles.texts["agent"], "Keep the archived draft");
			assert!(s.composer_unavailable_reason().is_none());
		});
	}

	#[gpui::test]
	fn no_active_conversation_keeps_draft_without_starting_replacement(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			fixture(s, cx);
			s.composer_manager = None;
			s.composer.update(cx, |input, cx| input.set_content("Keep root draft", cx));
			let original = s.snapshot.clone().unwrap();
			s.apply_refreshed_snapshot(Err(()), cx);
			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert_eq!(s.composer.read(cx).content(), "Keep root draft");
			let mut active = original.clone();
			active.work_items.clear();
			s.apply_refreshed_snapshot(Ok(AgentSnapshotResult::Available(active)), cx);
			assert!(s.selected.is_none());
			assert_eq!(s.composer_manager.as_deref(), Some("agent"));
			assert_eq!(s.composer.read(cx).content(), "Keep root draft");
			assert!(s.composer_unavailable_reason().is_some());
			assert!(s.build_submission("Keep root draft", cx).is_err());
			s.apply_refreshed_snapshot(Ok(AgentSnapshotResult::Available(original)), cx);
			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert_eq!(s.composer.read(cx).content(), "Keep root draft");
			assert!(s.draft_owner_available());
		});
	}
}
