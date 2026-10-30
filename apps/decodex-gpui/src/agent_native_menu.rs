//! Share one anchored native popover for model, permission and speed choices.
use gpui::{
	AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Pixels, Render,
	Size, Subscription, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
	WindowOptions,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
	},
};

use crate::{
	shell::{
		FocusNext, FocusPrevious, ToggleContentFullscreen,
		agent_surface::{AgentSurface, composer},
		forward_shell_action,
	},
	ui_motion,
	ui_theme::{
		self, BODY_SIZE, FONT_FAMILY, TEXT, appearance, native_material_panel::MaterialPanel,
	},
};

#[derive(Default)]
pub(super) struct NativeMenu {
	child: Option<WindowHandle<ComposerMenu>>,
	creating: bool,
	pub(super) failed: bool,
}
impl NativeMenu {
	pub(super) fn notify(&self, cx: &mut App) {
		if let Some(child) = self.child {
			cx.defer(move |cx| {
				let _ = child.update(cx, |_, _, cx| cx.notify());
			});
		}
	}
}

fn content_surface(menu: Option<&str>) -> Option<&str> {
	match menu {
		Some("microphone") => Some("attachments"),
		other => other,
	}
}

#[derive(Clone, Copy, PartialEq)]
struct Placement {
	anchor: Bounds<Pixels>,
	viewport: Size<Pixels>,
	width: f32,
}

struct ComposerMenu {
	owner: Entity<AgentSurface>,
	parent: AnyWindowHandle,
	surface: Option<MaterialPanel>,
	focus: FocusHandle,
	open_menu: Option<&'static str>,
	focus_pending: bool,
	placement: Option<Placement>,
	height: f32,
	_observation: Subscription,
}
impl ComposerMenu {
	fn place(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(placement) = self.placement
			&& let Some(surface) = &mut self.surface
			&& surface.place(composer::composer_menu_bounds(
				placement.anchor,
				self.height,
				placement.width,
				placement.viewport,
			)) {
			window.bounds_changed(cx);
		}
	}
}
impl Render for ComposerMenu {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let active = ui_motion::application_active();
		let available = self.owner.read(cx).composer_unavailable_reason().is_none();
		let open = active && available && self.open_menu.is_some();
		crate::frame_trace::popup_event("render", self.open_menu, open);
		if !open && let Some(surface) = &mut self.surface {
			surface.set_open(false);
		}
		let owner = self.owner.clone();
		let parent = self.parent;
		let menu = cx.entity().downgrade();
		let content = owner.update(cx, |s, cx| {
			s.composer_options(cx).unwrap_or_else(|| gpui::div().into_any_element())
		});

		let content = gpui::div()
			.w_full()
			.map(ui_theme::menu_outline)
			.font_family(FONT_FAMILY)
			.text_size(gpui::px(BODY_SIZE))
			.text_color(ui_theme::rgb(TEXT))
			.on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
			.on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
			.on_action({
				let owner = owner.clone();

				move |action: &ToggleContentFullscreen, _, cx| {
					owner.update(cx, |s, cx| {
						s.dismiss_composer_menu(cx);
						s.focus_native_message(cx);
					});
					forward_shell_action(parent, action, cx);
				}
			})
			.on_key_down({
				let owner = owner.clone();

				move |event, window, cx| {
					if event.keystroke.key == "escape" {
						owner.update(cx, |s, cx| {
							s.dismiss_composer_menu(cx);
							s.focus_native_message(cx);
						});
						cx.stop_propagation();
					} else if event.keystroke.key == "down" {
						window.focus_next(cx);
						cx.stop_propagation();
					} else if event.keystroke.key == "up" {
						window.focus_prev(cx);
						cx.stop_propagation();
					}
				}
			})
			.id("native-composer-menu")
			.track_focus(&self.focus)
			.child(content);
		let width = self.placement.map_or(248., |placement| placement.width);
		crate::ui_layout::intrinsic(gpui::px(width), content, move |size, window, cx| {
			let _ = menu.update(cx, |panel, cx| {
				panel.height = f32::from(size.height).max(48.);
				panel.place(window, cx);
				if open && let Some(surface) = &mut panel.surface {
					surface.refresh_material();
				}
				crate::frame_trace::popup_event("prepared", panel.open_menu, open);
				// The frame now has its final drawable size. Reveal it after paint,
				// so a retained menu never stretches the previous menu's texture.
				cx.defer_in(window, move |panel, window, cx| {
					let open = open
						&& ui_motion::application_active()
						&& panel.open_menu.is_some()
						&& panel.placement.is_some();
					if let Some(surface) = &mut panel.surface {
						surface.set_open(open);
						crate::frame_trace::popup_event("present", panel.open_menu, open);
						if open && panel.focus_pending {
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

impl AgentSurface {
	pub(crate) fn dismiss_native_menu(&mut self, cx: &mut Context<Self>) {
		if self.native_controls.composer.enabled && self.composer_menu.is_some() {
			self.dismiss_composer_menu(cx);
		}
	}

	pub(crate) fn prepare_native_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let menu = self.composer_menu.or(self.composer_menu_content);
		let anchor = self
			.native_controls
			.composer
			.bounds
			.zip(
				menu.and_then(|menu| {
					self.menu_trigger_bounds.get(composer::composer_menu_trigger(menu))
				})
				.copied(),
			)
			.map(|(composer, trigger)| Bounds::new(composer.origin + trigger.origin, trigger.size));
		let active = ui_motion::application_active();
		let available = self.composer_unavailable_reason().is_none();
		let open = active
			&& available
			&& self.native_controls.composer.enabled
			&& composer::selection_menu(self.composer_menu)
			&& anchor.is_some();
		if let Some(child) = self.native_controls.menu.child {
			let placement = anchor.map(|anchor| Placement {
				anchor,
				viewport: window.viewport_size(),
				width: composer::composer_menu_width(menu),
			});

			cx.defer(move |cx| {
				let _ = child.update(cx, |panel, window, cx| {
					let selection = if open { menu } else { None };
					if panel.open_menu != selection {
						let changed_surface =
							content_surface(panel.open_menu) != content_surface(selection);
						if changed_surface && let Some(surface) = &mut panel.surface {
							surface.set_open(false);
						}
						panel.focus_pending = changed_surface && selection.is_some();
						cx.notify();
					}
					panel.open_menu = selection;
					if panel.placement != placement {
						cx.notify();
					}
					panel.placement = placement;
					if open {
						// The native window must be on-screen to receive its layout
						// frame. Its content remains transparent until that frame is ready.
						panel.place(window, cx);
						if let Some(surface) = &mut panel.surface {
							surface.prepare_open();
						}
					}
				});
			});

			return;
		}

		if !open || self.native_controls.menu.creating || self.native_controls.menu.failed {
			return;
		}

		self.native_controls.menu.creating = true;

		let owner = cx.entity();
		let parent = window.window_handle();

		cx.defer(move |cx| {
			let child = parent
				.update(cx, |_, window, cx| create(owner.clone(), parent, window, cx))
				.ok()
				.flatten();

			owner.update(cx, |s, cx| {
				s.native_controls.menu.child = child;
				s.native_controls.menu.creating = false;
				s.native_controls.menu.failed = child.is_none();

				s.notify_composer_controls(cx);
			});
		});
	}
}

fn create(
	owner: Entity<AgentSurface>,
	parent: AnyWindowHandle,
	window: &mut Window,
	cx: &mut App,
) -> Option<WindowHandle<ComposerMenu>> {
	let child = cx
		.open_window(
			WindowOptions {
				titlebar: None,
				window_bounds: Some(WindowBounds::Windowed(Bounds::new(
					gpui::point(gpui::px(0.), gpui::px(0.)),
					gpui::size(gpui::px(composer::composer_menu_width(None)), gpui::px(240.)),
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

					cx.observe_window_activation(window, |panel: &mut ComposerMenu, window, cx| {
						if !window.is_window_active() && !ui_motion::application_active() {
							panel.owner.update(cx, |s, cx| {
								if composer::selection_menu(s.composer_menu) {
									s.dismiss_composer_menu(cx);
								}
							});
						}
					})
					.detach();

					let observation = cx.observe(&owner, |_, _, cx| cx.notify());
					let focus = cx.focus_handle();

					window.focus(&focus, cx);

					ComposerMenu {
						owner,
						parent,
						surface: None,
						focus,
						open_menu: None,
						focus_pending: false,
						placement: None,
						height: 240.,
						_observation: observation,
					}
				})
			},
		)
		.ok()?;
	let installed = child
		.update(cx, |panel, child_window, _| {
			panel.surface = MaterialPanel::install_popover(window, child_window);

			if let Some(surface) = &panel.surface {
				surface.set_title("Decodex Session Settings");
				surface.focus_text();
			}

			panel.surface.is_some()
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
