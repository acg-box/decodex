//! Keep notifications above native composer windows without moving the editor.
use super::{Shell, connection_presentation};
use crate::ui_theme::{self, native_glass_panel::GlassPanel};
use gpui::{
	AnyWindowHandle, Bounds, Context, Entity, Render, Subscription, Window,
	WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowOptions, div, point, prelude::*,
	px, size,
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
	surface: Option<GlassPanel>,
	_observation: Subscription,
}
impl Shell {
	pub(super) fn prepare_native_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		// Drive the fade from the visible parent: AppKit stops the display link
		// of a fully transparent or hidden child window.
		let opacity = crate::ui_motion::native_presence(
			"status-native-opacity",
			self.status_open,
			window,
			cx,
		);
		if let Some(child) = self.native_status.child {
			let viewport = window.viewport_size();
			let height = self.native_status.height.max(48.);
			let bounds = Bounds::new(
				point(
					viewport.width - px(328. + ui_theme::CONTROL_MARGIN - 12.),
					viewport.height
						- px(ui_theme::CONTROL_MARGIN
							+ ui_theme::CONTROL_GROUP_HEIGHT
							+ ui_theme::CONTROL_MARGIN
							- 12. + height),
				),
				size(px(328.), px(height)),
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
impl Render for StatusPanel {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let owner = self.owner.clone();
		let panel = owner
			.update(cx, |s, cx| s.render_status_panel(&connection_presentation(s.connection), cx));
		div()
			.w_full()
			.p(px(12.))
			.font_family(ui_theme::FONT_FAMILY)
			.text_color(gpui::rgb(ui_theme::TEXT))
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
				div()
					.w_full()
					.rounded(px(14.))
					.bg(gpui::rgb(0x29292d))
					.shadow(vec![gpui::BoxShadow {
						inset: false,
						color: gpui::rgba(0x00000024).into(),
						offset: point(px(0.), px(4.)),
						blur_radius: px(12.),
						spread_radius: px(-3.),
					}])
					.child(panel),
			)
	}
}
fn create(
	owner: Entity<Shell>,
	parent: AnyWindowHandle,
	window: &mut Window,
	cx: &mut gpui::App,
) -> Option<WindowHandle<StatusPanel>> {
	let child = cx
		.open_window(
			WindowOptions {
				titlebar: None,
				window_bounds: Some(WindowBounds::Windowed(Bounds::new(
					point(px(0.), px(0.)),
					size(px(328.), px(240.)),
				))),
				window_background: WindowBackgroundAppearance::Transparent,
				show: false,
				focus: false,
				..Default::default()
			},
			move |_, cx| {
				cx.new(|cx| {
					let observation = cx.observe(&owner, |_, _, cx| cx.notify());
					StatusPanel { owner, surface: None, _observation: observation }
				})
			},
		)
		.ok()?;
	let installed = child
		.update(cx, |s, child_window, _| {
			s.surface = GlassPanel::install_overlay(window, child_window);
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
