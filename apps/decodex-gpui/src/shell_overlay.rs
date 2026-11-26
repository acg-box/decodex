//! Shared presentation and focus ownership for the main workspace's modal surfaces.
use super::{
	ActivateSettings, CloseWindow, Context, Destination, DismissStatus, FocusNext, FocusPrevious,
	NavigateBack, NavigateForward, Shell,
	agent_surface::{AgentSurface, Catalog},
	ui_motion, ui_theme,
};
use gpui::{
	AnyElement, KeyDownEvent, Modifiers, MouseButton, Pixels, Role, Window, div, prelude::*, px,
};

const MATERIAL: &str = "workspace-overlay-material";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkspaceOverlay {
	Settings,
	Catalog(Catalog),
}

impl WorkspaceOverlay {
	fn title(self) -> &'static str {
		match self {
			Self::Settings => "Settings",
			Self::Catalog(Catalog::Archived) => "Archived",
			Self::Catalog(Catalog::Codex) => "Codex conversations",
		}
	}
}

impl Shell {
	pub(super) fn show_overlay(&mut self, overlay: WorkspaceOverlay, cx: &mut Context<Self>) {
		if self.overlay == Some(overlay) {
			return;
		}
		if matches!(self.overlay, Some(WorkspaceOverlay::Catalog(_)))
			&& overlay == WorkspaceOverlay::Settings
		{
			self.agent.update(cx, AgentSurface::close_catalog);
		}
		if self.overlay == Some(WorkspaceOverlay::Settings) {
			self.leave_settings_section();
		}
		if self.overlay.is_none() {
			self.overlay_capture_focus = true;
		}
		#[cfg(all(target_os = "macos", not(test)))]
		self.agent.update(cx, |agent, cx| agent.dismiss_native_menu(cx));
		self.status_open = false;
		self.overlay = Some(overlay);
		cx.notify();
	}

	pub(super) fn close_overlay(&mut self, cx: &mut Context<Self>) {
		let Some(previous) = self.overlay.take() else { return };
		match previous {
			WorkspaceOverlay::Settings => self.leave_settings_section(),
			WorkspaceOverlay::Catalog(_) => self.agent.update(cx, AgentSurface::close_catalog),
		}
		self.overlay_capture_focus = false;
		let parent = self.overlay_parent;
		let focus = self.overlay_return_focus.take().unwrap_or_else(|| self.root_focus.clone());
		cx.defer(move |cx| {
			let _ = parent.update(cx, |_, window, cx| {
				ui_motion::hide_material_surface(MATERIAL, window, cx);
				window.activate_window();
				window.focus(&focus, cx);
			});
		});
		cx.notify();
	}

	pub(super) fn escape_overlay(&mut self, cx: &mut Context<Self>) {
		if self.overlay == Some(WorkspaceOverlay::Catalog(Catalog::Archived))
			&& self.agent.update(cx, |agent, cx| agent.dismiss_catalog_selection(cx))
		{
			return;
		}
		self.close_overlay(cx);
	}

	pub(super) fn capture_overlay_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if !std::mem::take(&mut self.overlay_capture_focus) {
			return;
		}
		#[cfg(all(target_os = "macos", not(test)))]
		let origin = self
			.agent
			.update(cx, |agent, cx| agent.active_native_composer_focus(cx))
			.or_else(|| window.focused(cx));
		#[cfg(not(all(target_os = "macos", not(test))))]
		let origin = window.focused(cx);
		self.overlay_return_focus = origin;
		#[cfg(all(target_os = "macos", not(test)))]
		if crate::ui_theme::native_material_panel::available(
			crate::ui_theme::material::Surface::Popover,
		) {
			return;
		}
		window.focus(&self.overlay_focus, cx);
	}

	pub(super) fn render_overlay(
		&self,
		window: &Window,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let overlay = self.overlay?;
		let size = window.viewport_size();
		let width = (size.width - px(48.)).min(px(1000.)).max(px(1.));
		let height = (size.height - px(72.)).min(px(720.)).max(px(1.));
		let owner = cx.weak_entity();
		let escape = owner.clone();
		let surface = ui_motion::material_surface(
			MATERIAL,
			18.,
			move |window, cx| {
				owner
					.update(cx, |shell, cx| shell.overlay_content(overlay, height, window, cx))
					.unwrap_or_else(|_| div().into_any_element())
			},
			|_| {},
		)
		.with_modal_focus(self.overlay_focus.clone())
		.on_escape(move |cx| {
			let _ = escape.update(cx, |shell, cx| shell.escape_overlay(cx));
		});
		Some(
			div()
				.absolute()
				.inset_0()
				.flex()
				.items_center()
				.justify_center()
				.child(
					div()
						.id("workspace-overlay-backdrop")
						.absolute()
						.inset_0()
						.occlude()
						.bg(ui_theme::workspace_overlay_scrim())
						.on_mouse_down(
							MouseButton::Left,
							cx.listener(|shell, _, _, cx| {
								cx.stop_propagation();
								shell.close_overlay(cx);
							}),
						),
				)
				.child(div().w(width).h(height).occlude().child(surface))
				.into_any_element(),
		)
	}

	/// Tab traversal is window-wide in GPUI. Inline dialogs share that window
	/// with the workspace, so wrap at the dialog's own first and last tab stops.
	fn focus_overlay_tab(&self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
		let mut first_outside = None;
		loop {
			if forward {
				window.focus_next(cx);
			} else {
				window.focus_prev(cx);
			}
			let focused = window.focused(cx);
			if self.overlay_focus.contains_focused(window, cx) {
				return;
			}
			if focused == first_outside {
				window.focus(&self.overlay_focus, cx);
				return;
			}
			if first_outside.is_none() {
				first_outside = focused;
			}
		}
	}

	fn overlay_content(
		&mut self,
		overlay: WorkspaceOverlay,
		height: Pixels,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		// A control can disappear when a narrow catalog changes from its list to the detail view.
		// Keep keyboard input in the dialog when that removes its focused element.
		cx.defer_in(window, move |shell, window, cx| {
			if shell.overlay == Some(overlay) && !shell.overlay_focus.contains_focused(window, cx) {
				window.focus(&shell.overlay_focus, cx);
			}
		});
		let content = match overlay {
			WorkspaceOverlay::Settings => self.render_settings_content(window, cx),
			WorkspaceOverlay::Catalog(catalog) =>
				self.agent.update(cx, |agent, cx| agent.render_catalog(catalog, window, cx)),
		};
		let close = ui_theme::menu_row("workspace-overlay-close", true)
			.debug_selector(|| "workspace-overlay-close".into())
			.aria_label("Close")
			.tab_index(2)
			.w(px(26.))
			.h(px(26.))
			.absolute()
			.top(px(10.))
			.right(px(10.))
			.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
			.flex()
			.items_center()
			.justify_center()
			.cursor_pointer()
			.child(super::workspace_symbols::icon_sized(
				super::workspace_symbols::Symbol::Close,
				12.,
			))
			.on_click(cx.listener(|shell, _, _, cx| {
				shell.close_overlay(cx);
				cx.stop_propagation();
			}));
		div()
			.id("workspace-overlay-panel")
			.debug_selector(|| "workspace-overlay-panel".into())
			.role(Role::Dialog)
			.aria_label(overlay.title())
			.key_context("WorkspaceOverlay")
			.track_focus(&self.overlay_focus)
			.tab_group()
			.relative()
			.w_full()
			.h(height)
			.flex()
			.flex_col()
			.overflow_hidden()
			.rounded(px(18.))
			.text_color(ui_theme::rgb(ui_theme::TEXT))
			.child(super::navigation::mouse_navigation(cx.weak_entity(), Self::navigate_history))
			.on_action(cx.listener(|shell, _: &CloseWindow, _, cx| {
				shell.close_overlay(cx);
				cx.stop_propagation();
			}))
			.on_action(cx.listener(|shell, _: &DismissStatus, _, cx| {
				shell.escape_overlay(cx);
				cx.stop_propagation();
			}))
			.on_key_down(cx.listener(|shell, event: &KeyDownEvent, _, cx| {
				if event.keystroke.key == "escape"
					&& event.keystroke.modifiers == Modifiers::default()
				{
					shell.escape_overlay(cx);
					cx.stop_propagation();
				}
			}))
			.on_action(cx.listener(|shell, _: &FocusNext, window, cx| {
				shell.focus_overlay_tab(true, window, cx);
				cx.stop_propagation();
			}))
			.on_action(cx.listener(|shell, _: &FocusPrevious, window, cx| {
				shell.focus_overlay_tab(false, window, cx);
				cx.stop_propagation();
			}))
			.on_action(cx.listener(|shell, _: &ActivateSettings, _, cx| {
				shell.open_settings(Destination::Settings, cx);
				cx.stop_propagation();
			}))
			.on_action(cx.listener(|shell, _: &NavigateBack, window, cx| {
				shell.navigate_history(false, window, cx);
				cx.stop_propagation();
			}))
			.on_action(cx.listener(|shell, _: &NavigateForward, window, cx| {
				shell.navigate_history(true, window, cx);
				cx.stop_propagation();
			}))
			.child(div().flex_1().min_h_0().min_w_0().flex().child(content))
			.child(close)
			.into_any_element()
	}
}
