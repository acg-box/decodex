//! Keep notifications above native composer windows without moving the editor.
use gpui::{
	self, AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Pixels,
	Point, Render, Subscription, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
	WindowOptions,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
	},
};

const WIDTH: f32 = 328.;

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
}
pub(super) struct StatusPanel {
	owner: Entity<Shell>,
	parent: AnyWindowHandle,
	focus: FocusHandle,
	surface: Option<MaterialPanel>,
	origin: Option<Point<Pixels>>,
	height: f32,
	open: bool,
	focus_pending: bool,
	_observation: Subscription,
}

impl StatusPanel {
	fn forward_navigation(&self, action: &dyn gpui::Action, cx: &mut App) {
		self.owner.update(cx, |s, cx| {
			s.status_open = false;
			cx.notify();
		});
		forward_shell_action(self.parent, action, cx);
	}

	fn place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(origin) = self.origin
			&& let Some(surface) = &mut self.surface
			&& surface
				.place(Bounds::new(origin, gpui::size(gpui::px(WIDTH), gpui::px(self.height))))
		{
			window.bounds_changed(cx);
		}
	}
}
impl Render for StatusPanel {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let owner = self.owner.clone();
		let parent = self.parent;
		let measured = cx.entity().downgrade();
		let panel = owner.update(cx, |s, cx| {
			s.render_status_panel(&shell::connection_presentation(s.connection), cx)
		});

		let content = gpui::div()
			.w_full()
			.map(ui_theme::menu_outline)
			.p(gpui::px(ui_theme::POPOVER_PADDING))
			.font_family(FONT_FAMILY)
			.text_color(ui_theme::rgb(TEXT))
			.track_focus(&self.focus)
			.capture_any_mouse_down(cx.listener(|panel, event, window, cx| {
				if shell::navigation::forward_mouse_navigation(panel.parent, event, window, cx) {
					panel.owner.update(cx, |s, cx| {
						s.status_open = false;
						cx.notify();
					});
				}
			}))
			.on_action(cx.listener(|panel, action: &shell::NavigateBack, _, cx| {
				panel.forward_navigation(action, cx);
			}))
			.on_action(cx.listener(|panel, action: &shell::NavigateForward, _, cx| {
				panel.forward_navigation(action, cx);
			}))
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
						cx.stop_propagation();
					}
				}
			})
			.child(panel);
		crate::ui_layout::intrinsic(gpui::px(WIDTH), content, move |size, window, cx| {
			let _ = measured.update(cx, |panel, cx| {
				panel.height = f32::from(size.height).max(48.);
				panel.place(window, cx);
				if panel.open
					&& let Some(surface) = &mut panel.surface
				{
					surface.refresh_material();
				}
				cx.defer_in(window, |panel, window, cx| {
					if let Some(surface) = &mut panel.surface {
						surface.set_open(panel.open);
						if panel.open && panel.focus_pending {
							panel.focus_pending = false;
							surface.focus_text();
							window.focus(&panel.focus, cx);
							window.activate_window();
						}
					}
				});
			});
		})
	}
}

impl Shell {
	pub(super) fn prepare_native_status(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let open = self.status_open;

		if let Some(child) = self.native_status.child {
			let parent = window.window_handle();
			let close = cx.entity().downgrade();
			let viewport = window.viewport_size();
			let origin = gpui::point(
				viewport.width - gpui::px(WIDTH + CONTROL_MARGIN),
				gpui::px(CONTROL_MARGIN + CONTROL_GROUP_HEIGHT + CONTROL_MARGIN),
			);

			cx.defer(move |cx| {
				if open {
					ui_motion::claim_popover(
						parent,
						child.into(),
						move |cx| {
							let _ = close.update(cx, |s, cx| {
								s.status_open = false;
								cx.notify();
							});
						},
						cx,
					);
				}
				let _ = child.update(cx, |s, window, cx| {
					if s.open != open {
						s.focus_pending = open;
					}
					if s.open != open || s.origin != Some(origin) {
						cx.notify();
					}
					s.open = open;
					s.origin = Some(origin);
					if open {
						s.place(window, cx);
					}
					if let Some(surface) = &mut s.surface {
						if open {
							surface.prepare_open();
						} else {
							surface.set_open(false);
						}
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
					gpui::size(gpui::px(WIDTH), gpui::px(240.)),
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

					StatusPanel {
						owner,
						parent,
						focus,
						surface: None,
						origin: None,
						height: 240.,
						open: false,
						focus_pending: false,
						_observation: observation,
					}
				})
			},
		)
		.ok()?;
	let installed = child
		.update(cx, |s, child_window, _| {
			s.surface = MaterialPanel::install_popover(window, child_window);

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
