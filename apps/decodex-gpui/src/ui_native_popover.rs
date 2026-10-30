//! One transient material host per parent window. Content and actions retain their owners.
use std::{collections::HashMap, rc::Rc};

use gpui::{
	AnyElement, AnyWindowHandle, App, AppContext as _, Bounds, Context, ElementId, FocusHandle,
	Global, IntoElement, MouseDownEvent, Pixels, Point, Render, Size, Window,
	WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowId, WindowOptions,
	prelude::{FluentBuilder as _, InteractiveElement as _, ParentElement as _, Styled as _},
};

use super::{Popover, PopoverContent};
use crate::{
	ui_frame,
	ui_theme::{
		self, appearance,
		material::Surface,
		native_material_panel::{self, MaterialPanel},
	},
};

#[derive(Default)]
struct Hosts {
	children: HashMap<WindowId, WindowHandle<Panel>>,
	accessories: HashMap<(WindowId, ElementId), WindowHandle<Panel>>,
	active: HashMap<WindowId, ActivePopup>,
}
impl Global for Hosts {}

struct ActivePopup {
	id: WindowId,
	dismiss: Rc<dyn Fn(&mut App)>,
}

struct Content {
	id: ElementId,
	render: Rc<PopoverContent>,
	dismiss: Rc<dyn Fn(&mut App)>,
	trigger: Option<Bounds<Pixels>>,
	origin: Point<Pixels>,
	viewport: Size<Pixels>,
	width: Pixels,
	above: bool,
	preview: bool,
	attached: bool,
	radius: f64,
	focus: Option<FocusHandle>,
}

struct Panel {
	parent: AnyWindowHandle,
	surface: Option<MaterialPanel>,
	content: Option<Content>,
	focus: FocusHandle,
	focus_pending: bool,
	height: Pixels,
}

impl Panel {
	fn close(&mut self, cx: &mut App) {
		if let Some(content) = self.content.take() {
			(content.dismiss)(cx);
		}
		if let Some(surface) = &mut self.surface {
			surface.set_open(false);
		}
	}

	fn place(&mut self, window: &mut Window, cx: &mut App) {
		let Some(content) = &self.content else { return };
		let x = f32::from(content.origin.x)
			.clamp(8., (f32::from(content.viewport.width - content.width) - 8.).max(8.));
		let y = if content.above {
			content.trigger.map_or(content.origin.y, |trigger| trigger.top())
				- self.height
				- gpui::px(ui_theme::POPOVER_GAP)
		} else {
			content.origin.y
		};
		let y =
			f32::from(y).clamp(8., (f32::from(content.viewport.height - self.height) - 8.).max(8.));
		if let Some(surface) = &mut self.surface {
			surface.set_radius(content.radius);
			if surface.place(Bounds::new(
				gpui::point(gpui::px(x), gpui::px(y)),
				gpui::size(content.width, self.height),
			)) {
				window.bounds_changed(cx);
			}
		}
	}
}

impl Render for Panel {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		if !ui_frame::application_active() && self.content.as_ref().is_some_and(|c| !c.attached) {
			self.close(cx);
		}
		let Some(content) = &self.content else { return gpui::div().into_any_element() };
		let width = content.width;
		let focus = content.focus.clone().unwrap_or_else(|| self.focus.clone());
		let foreground = (content.render)(window, cx);
		let measured = cx.entity().downgrade();
		let root = gpui::div()
			.w_full()
			.when(!content.attached, ui_theme::menu_outline)
			.font_family(ui_theme::FONT_FAMILY)
			.text_size(gpui::px(12.))
			.line_height(gpui::px(18.))
			.text_color(ui_theme::rgb(ui_theme::TEXT))
			.track_focus(&self.focus)
			.on_key_down(cx.listener(|panel, event: &gpui::KeyDownEvent, window, cx| {
				match event.keystroke.key.as_str() {
					"escape" => {
						if let Some(content) = panel.content.as_ref().filter(|c| c.attached) {
							(content.dismiss)(cx);
						} else {
							panel.close(cx);
						}
						let parent = panel.parent;
						cx.defer(move |cx| {
							let _ = parent.update(cx, |_, window, _| window.activate_window());
						});
					},
					"down" => window.focus_next(cx),
					"up" => window.focus_prev(cx),
					_ => return,
				}
				cx.stop_propagation();
			}))
			.child(foreground);
		crate::ui_layout::intrinsic(width, root, move |size, window, cx| {
			let _ = measured.update(cx, |panel, cx| {
				if panel.content.as_ref().is_some_and(|content| !content.attached) {
					panel.height = size.height.max(gpui::px(1.));
					panel.place(window, cx);
				}
				if let Some(surface) = &mut panel.surface {
					surface.refresh_material();
				}
				let focus = focus.clone();
				cx.defer_in(window, move |panel, window, cx| {
					if panel.content.is_none() {
						return;
					}
					if let Some(surface) = &mut panel.surface {
						surface.set_open(true);
						if panel.focus_pending {
							panel.focus_pending = false;
							surface.focus_text();
							window.focus(&focus, cx);
							window.activate_window();
						}
					}
				});
			});
		})
		.into_any_element()
	}
}

pub(super) fn available() -> bool {
	native_material_panel::available(Surface::Popover)
}

pub(super) fn render(mut popover: Popover, window: &mut Window, cx: &mut App) -> AnyElement {
	let parent = window.window_handle();
	initialize(cx);
	if !popover.visible {
		let id = popover.id;
		cx.defer(move |cx| dismiss(parent, id, cx));
		return gpui::div().into_any_element();
	}
	let viewport = window.viewport_size();
	let content = popover.content.take().expect("native popover content");
	gpui::canvas(
		move |bounds, _, cx| {
			let inset = if popover.preview { 0. } else { ui_theme::POPOVER_PADDING };
			let request = Content {
				id: popover.id,
				render: content,
				dismiss: popover.dismiss,
				trigger: popover.trigger.get(),
				origin: bounds.origin
					+ gpui::point(
						gpui::px(inset),
						gpui::px(if popover.above { 0. } else { ui_theme::POPOVER_GAP }),
					),
				viewport,
				width: bounds.size.width - gpui::px(inset * 2.),
				above: popover.above,
				preview: popover.preview,
				attached: false,
				radius: f64::from(ui_theme::POPOVER_RADIUS),
				focus: popover.focus,
			};
			cx.defer(move |cx| present(parent, request, cx));
		},
		move |_, _, window, _| {
			window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
				if !phase.capture() {
					return;
				}
				if let Some(child) = cx.global::<Hosts>().children.get(&parent.window_id()).copied()
				{
					let _ = child.update(cx, |panel, _, cx| {
						if panel.content.as_ref().is_some_and(|c| {
							!c.trigger.is_some_and(|trigger| trigger.contains(&event.position))
						}) {
							panel.close(cx);
						}
					});
				}
			});
		},
	)
	.w_full()
	.h(gpui::px(1.))
	.into_any_element()
}

fn present(parent: AnyWindowHandle, request: Content, cx: &mut App) {
	let key = (parent.window_id(), request.id.clone());
	let existing = if request.attached {
		cx.global::<Hosts>().accessories.get(&key).copied()
	} else {
		cx.global::<Hosts>().children.get(&parent.window_id()).copied()
	};
	let child = existing
		.or_else(|| parent.update(cx, |_, window, cx| create(parent, window, cx)).ok().flatten());
	let Some(child) = child else { return };
	if request.attached {
		cx.global_mut::<Hosts>().accessories.insert(key, child);
	} else {
		cx.global_mut::<Hosts>().children.insert(parent.window_id(), child);
		claim(
			parent,
			child.into(),
			Rc::new(move |cx| {
				let _ = child.update(cx, |panel, _, cx| panel.close(cx));
			}),
			cx,
		);
	}
	let _ = child.update(cx, |panel, window, cx| {
		let changed = panel.content.as_ref().is_none_or(|old| old.id != request.id);
		if changed {
			panel.close(cx);
			panel.focus_pending = !request.preview && !request.attached;
		}
		if request.attached
			&& let Some(bounds) = request.trigger
		{
			// Attached surfaces use their parent's complete layout. Avoid resizing
			// Metal with the new width and the previous frame's height first.
			panel.height = bounds.size.height.max(gpui::px(1.));
		}
		panel.content = Some(request);
		panel.place(window, cx);
		if let Some(surface) = &mut panel.surface {
			surface.prepare_open();
		}
		cx.notify();
	});
}

pub(super) fn render_surface(
	id: ElementId,
	radius: f64,
	content: Rc<PopoverContent>,
	dismiss: Rc<dyn Fn(&mut App)>,
	window: &mut Window,
	cx: &mut App,
) -> AnyElement {
	initialize(cx);
	let parent = window.window_handle();
	let viewport = window.viewport_size();
	let outside_id = id.clone();
	gpui::canvas(
		move |bounds, _, cx| {
			let request = Content {
				id,
				render: content,
				dismiss,
				trigger: Some(bounds),
				origin: bounds.origin,
				viewport,
				width: bounds.size.width,
				above: false,
				preview: true,
				attached: true,
				radius,
				focus: None,
			};
			cx.defer(move |cx| present(parent, request, cx));
		},
		move |_, _, window, _| {
			let key = (parent.window_id(), outside_id);
			window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
				if phase.capture()
					&& let Some(child) = cx.global::<Hosts>().accessories.get(&key).copied()
				{
					let _ = child.update(cx, |panel, _, cx| {
						if let Some(content) = &panel.content {
							(content.dismiss)(cx);
						}
					});
				}
			});
		},
	)
	.size_full()
	.into_any_element()
}

pub(super) fn hide_surface(parent: AnyWindowHandle, id: ElementId, cx: &mut App) {
	if cx.has_global::<Hosts>()
		&& let Some(child) =
			cx.global::<Hosts>().accessories.get(&(parent.window_id(), id)).copied()
	{
		let _ = child.update(cx, |panel, _, cx| panel.close(cx));
	}
}

fn initialize(cx: &mut App) {
	if cx.has_global::<Hosts>() {
		return;
	}
	cx.set_global(Hosts::default());
	cx.on_window_closed(|cx, id| {
		cx.global_mut::<Hosts>().active.remove(&id);
		let keys: Vec<_> = cx
			.global::<Hosts>()
			.accessories
			.keys()
			.filter(|(parent, _)| *parent == id)
			.cloned()
			.collect();
		for key in keys {
			if let Some(child) = cx.global_mut::<Hosts>().accessories.remove(&key) {
				let _ = child.update(cx, |panel, window, cx| {
					panel.close(cx);
					window.remove_window();
				});
			}
		}
		if let Some(child) = cx.global_mut::<Hosts>().children.remove(&id) {
			let _ = child.update(cx, |panel, window, cx| {
				panel.close(cx);
				window.remove_window();
			});
		}
	})
	.detach();
}

fn create(
	parent: AnyWindowHandle,
	window: &mut Window,
	cx: &mut App,
) -> Option<WindowHandle<Panel>> {
	let child = cx
		.open_window(
			WindowOptions {
				titlebar: None,
				show: false,
				focus: false,
				window_bounds: Some(WindowBounds::Windowed(Bounds::new(
					gpui::point(gpui::px(0.), gpui::px(0.)),
					gpui::size(gpui::px(260.), gpui::px(120.)),
				))),
				window_background: WindowBackgroundAppearance::Transparent,
				..ui_frame::window_options()
			},
			|window, cx| {
				cx.new(|cx| {
					ui_frame::observe_activation(window, cx);
					appearance::observe(window, cx);
					cx.observe_window_activation(window, |panel: &mut Panel, window, cx| {
						if !window.is_window_active()
							&& !ui_frame::application_active()
							&& panel.content.as_ref().is_some_and(|content| !content.attached)
						{
							panel.close(cx);
						}
					})
					.detach();
					Panel {
						parent,
						surface: None,
						content: None,
						focus: cx.focus_handle(),
						focus_pending: false,
						height: gpui::px(120.),
					}
				})
			},
		)
		.ok()?;
	let installed = child
		.update(cx, |panel, child_window, _| {
			panel.surface = MaterialPanel::install_popover(window, child_window);
			panel.surface.is_some()
		})
		.unwrap_or(false);
	if !installed {
		let _ = child.update(cx, |_, window, _| window.remove_window());
		return None;
	}
	Some(child)
}

/// Opening a popup dismisses the previous popup for the same parent.
pub(super) fn claim(
	parent: AnyWindowHandle,
	popup: AnyWindowHandle,
	dismiss: Rc<dyn Fn(&mut App)>,
	cx: &mut App,
) {
	initialize(cx);
	if cx
		.global::<Hosts>()
		.active
		.get(&parent.window_id())
		.is_some_and(|active| active.id == popup.window_id())
	{
		return;
	}
	if let Some(active) = cx.global_mut::<Hosts>().active.remove(&parent.window_id()) {
		(active.dismiss)(cx);
	}
	cx.global_mut::<Hosts>()
		.active
		.insert(parent.window_id(), ActivePopup { id: popup.window_id(), dismiss });
}

pub(super) fn dismiss_host(parent: AnyWindowHandle, cx: &mut App) {
	if !cx.has_global::<Hosts>() {
		return;
	}
	if let Some(child) = cx.global::<Hosts>().children.get(&parent.window_id()).copied() {
		let _ = child.update(cx, |panel, _, cx| panel.close(cx));
	}
}

pub(super) fn dismiss(parent: AnyWindowHandle, id: ElementId, cx: &mut App) {
	if !cx.has_global::<Hosts>() {
		return;
	}
	if let Some(child) = cx.global::<Hosts>().children.get(&parent.window_id()).copied() {
		let _ = child.update(cx, |panel, _, cx| {
			if panel.content.as_ref().is_some_and(|content| content.id == id) {
				panel.close(cx);
			}
		});
	}
}
