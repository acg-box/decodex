//! Conversation lifecycle actions retain exact native ownership and deletion cleanup.
use crate::{
	shell::{
		agent_surface::{AgentSurface, Context},
		workspace_symbols::{self, Symbol},
	},
	ui_motion::{self, SmoothControl as _},
	ui_theme,
};
use decodex_protocol::{
	AgentActionDto, AgentClient, AgentCommandResponse, EntityId, IdempotencyKey, WireText,
};
use gpui::{
	AnyElement, Bounds, Pixels, Role, SharedString,
	prelude::{
		InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};
use std::{cell::Cell, rc::Rc};
use tokio::runtime::Builder;

impl AgentSurface {
	pub(super) fn navigation_conversation_actions(
		&self,
		id: &str,
		group: &SharedString,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let open = self.workspace.navigation.menu.as_deref() == Some(id);
		let bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
		let measured = bounds.clone();
		let target = id.to_owned();
		let keyboard = target.clone();
		let owner = cx.entity().downgrade();
		let dismiss = owner.clone();
		let menu_id = id.to_owned();
		let popup_id = SharedString::from(format!("navigation-menu-{id}"));
		let dismissed_id = id.to_owned();
		gpui::div()
			.relative()
			.flex_none()
			.w(gpui::px(24.))
			.h(gpui::px(24.))
			.child(
				gpui::div()
					.id(SharedString::from(format!("conversation-actions-{id}")))
					.debug_selector({
						let id = id.to_owned();
						move || format!("conversation-actions-{id}")
					})
					.role(Role::Button)
					.tab_index(0)
					.aria_label("Conversation actions")
					.aria_expanded(open)
					.size_full()
					.flex()
					.items_center()
					.justify_center()
					.rounded(gpui::px(5.))
					.cursor_pointer()
					.opacity(if open { 1. } else { 0. })
					.group_hover(group.clone(), |s| s.opacity(1.))
					.focus_visible(|s| s.opacity(1.))
					.hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.child(
						gpui::canvas(move |b, _, _| measured.set(Some(b)), |_, _, _, _| {})
							.absolute()
							.inset_0(),
					)
					.child(gpui::div().text_size(gpui::px(16.)).child("…"))
					.on_click(cx.listener(move |s, _, _, cx| {
						cx.stop_propagation();
						s.toggle_navigation_menu(&target, cx);
					}))
					.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.toggle_navigation_menu(&keyboard, cx);
							cx.stop_propagation();
						}
					})),
			)
			.child(
				gpui::deferred(
					gpui::div().absolute().right_0().top(gpui::px(26.)).w(gpui::px(216.)).child(
						ui_motion::popover_lazy(popup_id, open, move |_, cx| {
							owner
								.update(cx, |s, cx| s.navigation_conversation_menu(&menu_id, cx))
								.unwrap_or_else(|_| gpui::div().into_any_element())
						})
						.trigger(bounds)
						.on_dismiss(move |cx| {
							let _ = dismiss.update(cx, |s, cx| {
								if s.workspace.navigation.menu.as_deref() == Some(&dismissed_id) {
									s.workspace.navigation.menu = None;
									s.workspace.navigation.confirm_delete = false;
									cx.notify();
								}
							});
						}),
					),
				)
				.priority(3),
			)
			.into_any_element()
	}

	fn toggle_navigation_menu(&mut self, id: &str, cx: &mut Context<Self>) {
		self.workspace.navigation.menu = if self.workspace.navigation.menu.as_deref() == Some(id) {
			None
		} else {
			Some(id.into())
		};
		self.workspace.navigation.confirm_delete = false;
		cx.notify();
	}

	fn navigation_conversation_menu(&self, id: &str, cx: &mut Context<Self>) -> AnyElement {
		let Some(work) =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == id))
		else {
			return gpui::div().into_any_element();
		};
		let mut menu = ui_theme::menu_outline(gpui::div())
			.p(gpui::px(ui_theme::POPOVER_CONTENT_PADDING))
			.flex()
			.flex_col()
			.gap_1();
		if self.workspace.navigation.confirm_delete {
			let explanation = if self.is_assistant_conversation(id) {
				"Conversation history will be removed. Learned Memory stays."
			} else {
				"Conversation history will be permanently removed."
			};
			menu =
				menu.child(gpui::div().px_2().py_1().text_size(gpui::px(11.)).child(explanation));
			menu = menu.child(self.navigation_menu_action(
				id,
				"delete",
				"Delete permanently",
				Symbol::Trash,
				cx,
			));
			menu =
				menu.child(self.navigation_menu_action(id, "cancel", "Cancel", Symbol::Close, cx));
		} else {
			if work.codex_thread_id.is_some() {
				menu = menu.child(self.navigation_menu_action(
					id,
					"archive",
					"Archive conversation",
					Symbol::Archive,
					cx,
				));
			}
			let label = if work.codex_thread_id.is_none() {
				"Discard draft"
			} else {
				"Delete conversation"
			};
			menu = menu.child(self.navigation_menu_action(id, "confirm", label, Symbol::Trash, cx));
		}
		menu.into_any_element()
	}

	fn navigation_menu_action(
		&self,
		id: &str,
		action: &'static str,
		label: &'static str,
		symbol: Symbol,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let target = id.to_owned();
		let keyboard = target.clone();
		let enabled = self.workspace.navigation.mutation.is_none();
		gpui::div()
			.id(SharedString::from(format!("conversation-{action}-{id}")))
			.debug_selector({
				let id = id.to_owned();
				move || format!("conversation-{action}-{id}")
			})
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label)
			.min_h(gpui::px(28.))
			.px_2()
			.flex()
			.items_center()
			.gap_2()
			.rounded(gpui::px(5.))
			.text_size(gpui::px(12.))
			.text_color(ui_theme::rgb(if action == "delete" {
				ui_theme::ERROR
			} else {
				ui_theme::TEXT
			}))
			.cursor_pointer()
			.hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.child(workspace_symbols::icon_sized(symbol, 14.))
			.child(label)
			.on_click(cx.listener(move |s, _, _, cx| {
				cx.stop_propagation();
				if enabled {
					s.navigation_menu_command(&target, action, cx);
				}
			}))
			.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
				if enabled && !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					s.navigation_menu_command(&keyboard, action, cx);
					cx.stop_propagation();
				}
			}))
			.smooth()
			.enabled(enabled)
			.into_any_element()
	}

	fn navigation_menu_command(&mut self, id: &str, action: &str, cx: &mut Context<Self>) {
		match action {
			"confirm" => {
				self.workspace.navigation.confirm_delete = true;
				cx.notify();
			},
			"cancel" => {
				self.workspace.navigation.menu = None;
				self.workspace.navigation.confirm_delete = false;
				cx.notify();
			},
			"archive" | "delete" => self.mutate_navigation_conversation(id, action == "delete", cx),
			_ => {},
		}
	}

	fn mutate_navigation_conversation(&mut self, id: &str, delete: bool, cx: &mut Context<Self>) {
		if self.workspace.navigation.mutation.is_some() {
			return;
		}
		let Some(work) =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == id)).cloned()
		else {
			return;
		};
		let canonical = self.assistant_id().as_deref() == Some(id);
		if work.codex_thread_id.is_none()
			&& (work.status != decodex_protocol::AgentWorkStatusDto::Wait
				|| work.dispatch_state != decodex_protocol::AgentDispatchStateDto::Idle
				|| !self.send_previews(id).is_empty())
		{
			self.workspace.navigation.feedback = Some(
				"This conversation is starting. Wait for it to finish before deleting it.".into(),
			);
			cx.notify();
			return;
		}
		if delete && canonical && work.codex_thread_id.is_none() {
			self.forget_deleted_conversation(id, cx);
			if self.selected.as_deref() == Some(id) {
				self.composer.update(cx, |input, cx| input.clear(cx));
			}
			self.workspace.navigation.menu = None;
			cx.notify();
			return;
		}
		let owner = EntityId::new(id).expect("saved conversation identity");
		let action = match work.codex_thread_id {
			Some(thread) => {
				let thread_id = WireText::new(thread).expect("saved native identity");
				if delete {
					AgentActionDto::DeleteConversation { work_id: owner, thread_id }
				} else {
					AgentActionDto::ArchiveConversation { work_id: owner, thread_id }
				}
			},
			None if delete => AgentActionDto::DiscardEmptyWork { work_id: owner },
			None => return,
		};
		let Some(profile) = self.profile.clone() else { return };
		let connection = profile.clone();
		let epoch = self.command_epoch;
		let id = id.to_owned();
		self.workspace.navigation.menu = None;
		self.workspace.navigation.feedback = None;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "Could not connect.".to_owned())?;
			runtime
				.block_on(
					AgentClient::new(connection).execute(
						action,
						IdempotencyKey::new(super::super::unique_command())
							.expect("generated command identity"),
					),
				)
				.map_err(|e| e.to_string())
		});
		self.workspace.navigation.mutation = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.command_epoch != epoch || s.profile.as_ref() != Some(&profile) {
					return;
				}
				s.workspace.navigation.mutation = None;
				if matches!(result, Ok(AgentCommandResponse::Accepted { .. })) {
					if delete {
						s.forget_deleted_conversation(&id, cx);
					}
					s.workspace.pages.retain(|page| page != &id);
					if s.selected.as_deref() == Some(&id) {
						s.close_native_agent(cx);
						s.history = None;
						s.selected = None;
						s.composer_manager = None;
						s.composer.update(cx, |input, cx| input.clear(cx));
						if let Some(other) = s
							.snapshot
							.as_ref()
							.and_then(|snapshot| {
								snapshot.work_items.iter().find(|w| {
									w.id != id && !snapshot.archived_work_ids.contains(&w.id)
								})
							})
							.map(|w| w.id.clone())
						{
							s.open_navigation_conversation(&other, cx);
						}
					}
					s.refresh(cx);
				} else {
					s.workspace.navigation.feedback = Some(
						"The change was not confirmed. Refresh the conversation before retrying."
							.into(),
					);
				}
				cx.notify();
			});
		}));
		cx.notify();
	}
}
