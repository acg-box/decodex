//! Keep notifications above native composer windows without moving the editor.
use gpui::{
	self, AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Render,
	Subscription, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowOptions,
	prelude::{InteractiveElement as _, IntoElement, ParentElement as _, Styled as _},
};

use crate::{
	shell::{self, Shell, ToggleContentFullscreen, forward_shell_action},
	ui_motion,
	ui_theme::{
		self, CONTROL_GROUP_HEIGHT, CONTROL_MARGIN, FONT_FAMILY, TEXT, appearance,
		native_material_panel::MaterialPanel,
	},
};

#[derive(Default)]
pub(super) struct NativeStatus {
	pub(super) child: Option<WindowHandle<StatusPanel>>,
	creating: bool,
	failed: bool,
	height: f32,
}
pub(super) struct StatusPanel {
	owner: Entity<Shell>,
	parent: AnyWindowHandle,
	focus: FocusHandle,
	surface: Option<MaterialPanel>,
	_observation: Subscription,
}
impl Render for StatusPanel {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let owner = self.owner.clone();
		let parent = self.parent;
		let panel = owner.update(cx, |s, cx| {
			s.render_status_panel(&shell::connection_presentation(s.connection), cx)
		});

		gpui::div()
			.w_full()
			.p(gpui::px(12.))
			.font_family(FONT_FAMILY)
			.text_color(ui_theme::rgb(TEXT))
			.track_focus(&self.focus)
			.on_action({
				let owner = owner.clone();

				move |action: &ToggleContentFullscreen, _, cx| {
					owner.update(cx, |s, cx| {
						s.status_open = false;
						cx.notify();
					});
					forward_shell_action(parent, action, cx);
				}
			})
			.on_key_down({
				let owner = owner.clone();

				move |event, _, cx| {
					if event.keystroke.key == "escape" {
						owner.update(cx, |s, cx| {
							s.status_open = false;

							cx.notify();
						});
					}
				}
			})
			.on_children_prepainted(move |bounds, _, cx| {
				if let Some(bounds) = bounds.first() {
					let height = f32::from(bounds.size.height) + 24.;

					owner.update(cx, |s, cx| {
						if (s.native_status.height - height).abs() > 0.5 {
							s.native_status.height = height;

							cx.notify();
						}
					});
				}
			})
			.child(
				gpui::div()
					.w_full()
					.rounded(gpui::px(14.))
					.bg(ui_theme::rgb(0x29292d))
					.shadow(vec![gpui::BoxShadow {
						inset: false,
						color: ui_theme::rgba(0x00000024).into(),
						offset: gpui::point(gpui::px(0.), gpui::px(4.)),
						blur_radius: gpui::px(12.),
						spread_radius: gpui::px(-3.),
					}])
					.child(panel),
			)
	}
}

impl Shell {
	pub(super) fn prepare_native_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		// Drive the fade from the visible parent: AppKit stops the display link
		// of a fully transparent or hidden child window.
		let opacity =
			ui_motion::native_presence("status-native-opacity", self.status_open, window, cx);

		if let Some(child) = self.native_status.child {
			let viewport = window.viewport_size();
			let height = self.native_status.height.max(48.);
			let bounds = Bounds::new(
				gpui::point(
					viewport.width - gpui::px(328. + CONTROL_MARGIN - 12.),
					gpui::px(CONTROL_MARGIN + CONTROL_GROUP_HEIGHT + CONTROL_MARGIN - 12.),
				),
				gpui::size(gpui::px(328.), gpui::px(height)),
			);

			cx.defer(move |cx| {
				let _ = child.update(cx, |s, window, cx| {
					if let Some(surface) = &mut s.surface {
						if surface.place(bounds) {
							window.bounds_changed(cx);
						}

						surface.set_opacity(opacity);
						surface.set_visible(opacity > 0.001);
					}
				});
			});

			return;
		}

		if !self.status_open || self.native_status.creating || self.native_status.failed {
			return;
		}

		self.native_status.creating = true;

		let owner = cx.entity();
		let parent = window.window_handle();

		cx.defer(move |cx| {
			let child = parent
				.update(cx, |_, window, cx| create(owner.clone(), parent, window, cx))
				.ok()
				.flatten();

			owner.update(cx, |s, cx| {
				s.native_status.child = child;
				s.native_status.creating = false;
				s.native_status.failed = child.is_none();

				cx.notify();
			});
		});
	}
}
fn create(
	owner: Entity<Shell>,
	parent: AnyWindowHandle,
	window: &mut Window,
	cx: &mut App,
) -> Option<WindowHandle<StatusPanel>> {
	let child = cx
		.open_window(
			WindowOptions {
				titlebar: None,
				window_bounds: Some(WindowBounds::Windowed(Bounds::new(
					gpui::point(gpui::px(0.), gpui::px(0.)),
					gpui::size(gpui::px(328.), gpui::px(240.)),
				))),
				window_background: WindowBackgroundAppearance::Transparent,
				show: false,
				focus: false,
				..ui_motion::window_options()
			},
			move |window, cx| {
				cx.new(|cx| {
					ui_motion::observe_activation(window, cx);
					appearance::observe(window, cx);

					let observation = cx.observe(&owner, |_, _, cx| cx.notify());
					let focus = cx.focus_handle();

					window.focus(&focus, cx);

					StatusPanel { owner, parent, focus, surface: None, _observation: observation }
				})
			},
		)
		.ok()?;
	let installed = child
		.update(cx, |s, child_window, _| {
			s.surface = MaterialPanel::install_overlay(window, child_window);

			s.surface.is_some()
		})
		.unwrap_or(false);

	if !installed {
		let _ = child.update(cx, |_, window, _| window.remove_window());

		return None;
	}

	cx.on_window_closed(move |cx, id| {
		if id == parent.window_id() {
			let _ = child.update(cx, |_, window, _| window.remove_window());
		}
	})
	.detach();

	Some(child)
}
