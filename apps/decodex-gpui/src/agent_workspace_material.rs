//! Workspace panes keep one native glass surface across all layout modes.
use gpui::{AnyElement, MouseMoveEvent, prelude::*};

use super::workspace_size::Panel;
use crate::{
	shell::{
		self,
		agent_surface::{AgentSurface, Context, Window},
	},
	ui_layout,
	ui_motion::{self, PanelReveal},
	ui_theme,
};

fn material_id(panel: Panel) -> &'static str {
	if panel == Panel::Left {
		"workspace-navigation-material"
	} else {
		"workspace-activity-material"
	}
}

impl AgentSurface {
	pub(crate) fn hide_workspace_materials(&self, window: &Window, cx: &mut Context<Self>) {
		for panel in [Panel::Left, Panel::Right] {
			self.hide_workspace_material(panel, window, cx);
		}
	}

	pub(super) fn hide_workspace_material(
		&self,
		panel: Panel,
		window: &Window,
		cx: &mut Context<Self>,
	) {
		ui_motion::hide_material_surface(material_id(panel), window, cx);
	}

	pub(super) fn workspace_material(
		&self,
		panel: Panel,
		visible_width: f32,
		content_width: f32,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let height = (f32::from(window.viewport_size().height)
			- shell::WINDOW_CONTROLS_CLEARANCE
			- ui_theme::WORKSPACE_GUTTER)
			.max(0.);
		let clip_x =
			if panel == Panel::Right { (content_width - visible_width).max(0.) } else { 0. };
		let parent = window.window_handle();
		let rail = ui_theme::CONVERSATION_TAB_SIZE + 2. * ui_theme::SIDEBAR_INSET;
		if panel == Panel::Left {
			crate::frame_trace::sidebar_geometry(visible_width, content_width);
			let reveal = PanelReveal::new(
				self.workspace_sidebar_expanded(window),
				visible_width,
				rail,
				content_width,
			);
			let changed = self.workspace.sidebar_reveal.borrow().as_ref().is_none_or(|old| {
				old.expanded != reveal.expanded
					|| old.target_expanded != reveal.target_expanded
					|| old.content_width != reveal.content_width
			});
			if changed {
				self.workspace.sidebar_reveal.replace(Some(reveal));
				self.presentations.sidebar.update(cx, |_, cx| cx.notify());
			}
		}
		let owner = cx.entity().downgrade();
		let escape_owner = owner.clone();
		gpui::div()
			.debug_selector(move || material_id(panel).into())
			.relative()
			.size_full()
			.child(
				gpui::div()
					.absolute()
					.left(gpui::px(-clip_x))
					.top_0()
					.w(gpui::px(content_width))
					.h(gpui::px(height))
					.child(
						ui_motion::material_surface(
							material_id(panel),
							f64::from(ui_theme::WORKSPACE_RADIUS),
							move |window, cx| {
								owner
									.update(cx, |s, cx| {
										// The native window owns this cache for its whole lifetime.
										// Moving its clip must not rebuild the fixed content
										// canvas.
										let presentation = if panel == Panel::Left {
											s.presentations.sidebar.clone()
										} else {
											s.presentations.activity.clone()
										};
										let content = ui_layout::pane(presentation, window);
										let root = s
											.workspace_resize_root(cx)
											.w(gpui::px(content_width))
											.h(gpui::px(height))
											.capture_any_mouse_down(cx.listener(
												move |s, _, _, _| {
													s.workspace.focused_panel = Some(panel);
												},
											))
											.on_mouse_move(cx.listener(
												move |s, event: &MouseMoveEvent, _, cx| {
													if panel == Panel::Left
														&& event.pressed_button.is_none()
														&& !cx.has_active_drag()
													{
														s.sidebar_hover(true, cx);
													}
												},
											))
											.on_hover(cx.listener(
												move |s, hovered: &bool, _, cx| {
													if panel == Panel::Left && !hovered {
														s.sidebar_hover(false, cx);
													}
												},
											))
											.child(content)
											.when(
												panel == Panel::Right
													|| s.workspace.sidebar_visible,
												|root| {
													root.child(s.sidebar_resize_handle(panel, cx))
												},
											);
										// The child owns the material; the persistent main window
										// owns actions.
										#[cfg(any(target_os = "macos", test))]
										macro_rules! forward { ($root:expr, $($action:ty),+ $(,)?) => {
							$root$(.on_action(move |a: &$action, _, cx| shell::forward_shell_action(parent, a, cx)))+
						}; }
										#[cfg(any(target_os = "macos", test))]
										let root = forward!(
											root,
											shell::ActivateAgent,
											shell::ActivateHealth,
											shell::FindConversation,
											shell::ToggleSidebar,
											shell::ToggleInspector,
											shell::ToggleGraph,
											shell::OpenGlobalCommands,
											shell::ToggleContentFullscreen,
											shell::NavigateBack,
											shell::NavigateForward,
											shell::ActivateSettings,
											shell::GrowPanel,
											shell::ShrinkPanel,
											shell::ResetPanel,
											shell::GrowPanels,
											shell::ShrinkPanels,
											shell::ResetPanels,
										)
										.on_action(|_: &shell::FocusNext, window, cx| {
											window.focus_next(cx);
											cx.stop_propagation();
										})
										.on_action(|_: &shell::FocusPrevious, window, cx| {
											window.focus_prev(cx);
											cx.stop_propagation();
										})
										.on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
											if event.keystroke.key == "escape" {
												shell::forward_shell_action(
													parent,
													&shell::DismissStatus,
													cx,
												);
											}
										});
										root.into_any_element()
									})
									.unwrap_or_else(|_| gpui::div().into_any_element())
							},
							|_| {},
						)
						.workspace()
						.clip(gpui::Bounds::new(
							gpui::point(gpui::px(clip_x), gpui::px(0.)),
							gpui::size(gpui::px(visible_width), gpui::px(height)),
						))
						.on_escape(move |cx| {
							let owner = escape_owner.clone();
							let _ = parent.update(cx, |_, window, cx| {
								let _ = owner
									.update(cx, |s, cx| s.dismiss_workspace_overlays(window, cx));
							});
						}),
					),
			)
			.into_any_element()
	}
}
