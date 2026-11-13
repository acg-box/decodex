//! Conversation lifecycle actions retain exact native ownership and deletion cleanup.
use crate::{
	shell::{
		agent_surface::{AgentSurface, Context},
		workspace_symbols::{self, Symbol},
	},
	ui_motion::{self},
	ui_theme,
};
use decodex_protocol::{
	AgentActionDto, AgentClient, AgentCommandResponse, EntityId, IdempotencyKey, WireText,
};
use gpui::{
	AnyElement, AppContext as _, Bounds, Pixels, Role, SharedString,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};
use std::{cell::Cell, rc::Rc};
use tokio::runtime::Builder;

impl AgentSurface {
	pub(super) fn navigation_row_actions(
		&self,
		item: &super::Item,
		key: &str,
		opacity: f32,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut row = gpui::div()
			.absolute()
			.right(gpui::px(1.))
			.top(gpui::px(2.))
			.h(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
			.flex()
			.items_center()
			.opacity(opacity);
		for (action, label, glyph) in [
			(
				"pin",
				if self.workspace.navigation.preferences.pinned.contains(&item.id) {
					"Unpin"
				} else {
					"Pin"
				},
				super::Glyph::Pin,
			),
			(
				if item.archived { "restore" } else { "archive" },
				if item.archived { "Restore" } else { "Archive" },
				if item.archived { super::Glyph::Restore } else { super::Glyph::Archive },
			),
			("delete", "Delete", super::Glyph::Trash),
		] {
			let target = item.clone();
			let keyboard = target.clone();
			let row_key = key.to_owned();
			let key_target = row_key.clone();
			let debug = format!("navigation-{action}-{key}");
			let confirming = action == "delete"
				&& self.workspace.navigation.confirm_delete
				&& self.workspace.navigation.menu.as_deref() == Some(key);
			let idle = self
				.snapshot
				.as_ref()
				.and_then(|s| s.work_items.iter().find(|w| w.id == item.id))
				.is_none_or(|w| {
					w.active_turn_id.is_none()
						&& w.dispatch_state == decodex_protocol::AgentDispatchStateDto::Idle
				});
			let enabled = action == "pin"
				|| (self.workspace.navigation.mutation.is_none()
					&& idle
					&& (action != "archive" || !item.draft));
			let tip = if !enabled {
				if action == "archive" && item.draft {
					"This conversation has no messages to archive."
				} else if !idle {
					"Wait for the current turn to finish."
				} else {
					"Wait for the current change to finish."
				}
				.to_owned()
			} else if confirming {
				"Click again to delete permanently.".to_owned()
			} else {
				format!("{label} {}", item.title)
			};
			row = row.child(
				gpui::div()
					.id(SharedString::from(debug.clone()))
					.debug_selector(move || debug.clone())
					.role(Role::Button)
					.tab_index(if enabled { 0 } else { -1 })
					.aria_label(format!("{label} {}", item.title))
					.size(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.rounded(gpui::px(4.))
					.opacity(if enabled { 1. } else { 0.35 })
					.when(enabled, |button| button.cursor_pointer())
					.text_color(if confirming {
						ui_theme::rgb(ui_theme::ERROR)
					} else if action == "pin"
						&& self.workspace.navigation.preferences.pinned.contains(&item.id)
					{
						super::style::section_color(super::Section::Pin)
					} else {
						ui_theme::rgb(ui_theme::TEXT_MUTED)
					})
					.hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.focus_visible(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.tooltip(move |_, cx| {
						cx.new(|_| crate::shell::ControlTooltip(tip.clone())).into()
					})
					.child(ui_motion::ConfirmationNudge {
						started: if confirming {
							self.workspace.navigation.confirm_started
						} else {
							None
						},
						child: super::Icon { glyph, size: 13. }.into_any_element(),
					})
					.on_click(cx.listener(move |s, _, _, cx| {
						cx.stop_propagation();
						if enabled {
							s.navigation_row_command(&target, &row_key, action, cx);
						}
					}))
					.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
						if enabled
							&& !e.is_held
							&& ["enter", "space"].contains(&e.keystroke.key.as_str())
						{
							s.navigation_row_command(&keyboard, &key_target, action, cx);
							cx.stop_propagation();
						}
					})),
			);
		}
		row.into_any_element()
	}

	fn navigation_row_command(
		&mut self,
		item: &super::Item,
		key: &str,
		action: &str,
		cx: &mut Context<Self>,
	) {
		if action == "delete"
			&& !(self.workspace.navigation.confirm_delete
				&& self.workspace.navigation.menu.as_deref() == Some(key))
		{
			self.workspace.navigation.menu = Some(key.into());
			self.workspace.navigation.confirm_delete = true;
			self.workspace.navigation.confirm_started = Some(std::time::Instant::now());
			cx.notify();
			return;
		}
		self.workspace.navigation.confirm_delete = false;
		self.workspace.navigation.menu = None;
		match action {
			"pin" => {
				if !self.workspace.navigation.preferences.pinned.remove(&item.id) {
					self.workspace.navigation.preferences.pinned.insert(item.id.clone());
				}
				self.workspace.navigation.preferences.save();
			},
			"restore" => self.restore_navigation_archive(&item.id, cx),
			"delete" if item.archived => self.delete_navigation_archive(&item.id, cx),
			"archive" | "delete" =>
				self.mutate_navigation_conversation(&item.id, action == "delete", cx),
			_ => {},
		}
		cx.notify();
	}

	pub(super) fn navigation_options(
		&self,
		section: super::Section,
		opacity: f32,
		_window: &mut gpui::Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let open = self.workspace.navigation.options == Some(section);
		let bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
		let measured = bounds.clone();
		let owner = cx.entity().downgrade();
		let dismiss = owner.clone();
		gpui::div()
			.relative()
			.size(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
			.flex_none()
			.child(
				gpui::div()
					.id(SharedString::from(format!("navigation-options-{}", section.key())))
					.debug_selector(move || format!("navigation-options-{}", section.key()))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("{} options", section.label()))
					.aria_expanded(open)
					.size_full()
					.opacity(if open { 1. } else { opacity })
					.focus_visible(|s| s.opacity(1.))
					.flex()
					.items_center()
					.justify_center()
					.rounded(gpui::px(4.))
					.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
					.cursor_pointer()
					.hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.child(
						gpui::canvas(move |b, _, _| measured.set(Some(b)), |_, _, _, _| {})
							.absolute()
							.inset_0(),
					)
					.child(super::Icon { glyph: super::Glyph::Sort, size: 12. })
					.on_click(cx.listener(move |s, _, _, cx| {
						cx.stop_propagation();
						s.workspace.navigation.options = if open { None } else { Some(section) };
						cx.notify();
					}))
					.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.workspace.navigation.options = Some(section);
							cx.stop_propagation();
							cx.notify();
						}
					})),
			)
			.child(
				gpui::deferred(
					gpui::div().absolute().top(gpui::px(26.)).right_0().w(gpui::px(176.)).child(
						ui_motion::popover_lazy(
							SharedString::from(format!("navigation-sort-menu-{}", section.key())),
							open,
							move |_, cx| {
								owner
									.update(cx, |s, cx| s.navigation_options_menu(section, cx))
									.unwrap_or_else(|_| gpui::div().into_any_element())
							},
						)
						.trigger(bounds)
						.on_dismiss(move |cx| {
							let _ = dismiss.update(cx, |s, cx| {
								s.workspace.navigation.options = None;
								cx.notify();
							});
						}),
					),
				)
				.priority(3),
			)
			.into_any_element()
	}

	fn navigation_options_menu(
		&self,
		section: super::Section,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let pref = self.workspace.navigation.preferences.section(section);
		let mut menu = ui_theme::menu_outline(gpui::div())
			.p(gpui::px(4.))
			.flex()
			.flex_col()
			.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE));
		for (index, label, checked) in [
			(0, "Last updated", pref.sort == super::Sort::Updated),
			(1, "Alphabetical", pref.sort == super::Sort::Alphabetical),
			(2, "Always scroll titles", pref.auto_scroll),
		] {
			if index == 2 {
				menu = menu.child(
					gpui::div()
						.my(gpui::px(4.))
						.mx(gpui::px(3.))
						.h(gpui::px(1.))
						.bg(ui_theme::rgba(ui_theme::PANEL_DIVIDER)),
				);
			}
			menu = menu.child(
				gpui::div()
					.id(SharedString::from(format!("navigation-option-{}-{index}", section.key())))
					.debug_selector(move || format!("navigation-option-{}-{index}", section.key()))
					.role(if index == 2 { Role::MenuItemCheckBox } else { Role::MenuItemRadio })
					.aria_label(label)
					.aria_toggled(if checked { gpui::Toggled::True } else { gpui::Toggled::False })
					.tab_index(0)
					.h(gpui::px(26.))
					.px(gpui::px(7.))
					.flex()
					.items_center()
					.rounded(gpui::px(3.))
					.hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.focus_visible(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.child(gpui::div().flex_1().child(label))
					.when(checked, |s| s.child(workspace_symbols::icon_sized(Symbol::Confirm, 12.)))
					.on_click(cx.listener(move |s, _, _, cx| {
						s.select_navigation_preference(section, index, cx);
						cx.stop_propagation();
					}))
					.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, window, cx| {
						match e.keystroke.key.as_str() {
							"enter" | "space" if !e.is_held =>
								s.select_navigation_preference(section, index, cx),
							"down" => window.focus_next(cx),
							"up" => window.focus_prev(cx),
							"escape" => {
								s.workspace.navigation.options = None;
								cx.notify();
							},
							_ => return,
						}
						cx.stop_propagation();
					})),
			);
		}
		menu.into_any_element()
	}

	fn select_navigation_preference(
		&mut self,
		section: super::Section,
		index: i32,
		cx: &mut Context<Self>,
	) {
		let mut pref = self.workspace.navigation.preferences.section(section);
		match index {
			0 => pref.sort = super::Sort::Updated,
			1 => pref.sort = super::Sort::Alphabetical,
			_ => pref.auto_scroll = !pref.auto_scroll,
		}
		self.workspace.navigation.preferences.sections.insert(section, pref);
		self.workspace.navigation.preferences.save();
		if index < 2 {
			self.workspace.navigation.options = None;
		}
		cx.notify();
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
