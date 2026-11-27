//! One transient material host per parent window. Content and actions retain their owners.
use std::{collections::HashMap, rc::Rc};

use gpui::{
	AnyElement, AnyWindowHandle, App, AppContext as _, Bounds, Context, ElementId, FocusHandle,
	Global, IntoElement, MouseDownEvent, Pixels, Point, Render, Size, Window,
	WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowId, WindowOptions,
	prelude::{FluentBuilder as _, InteractiveElement as _, ParentElement as _, Styled as _},
};

use super::{MaterialGeometry, MaterialSurface, Popover, PopoverContent, PopoverPlacement, Tween};
use crate::{
	ui_frame,
	ui_theme::{
		self,
		material::Surface,
		native_material_panel::{self, MaterialPanel},
	},
};

#[derive(Default)]
struct Hosts {
	children: HashMap<WindowId, WindowHandle<Panel>>,
	modals: HashMap<WindowId, ElementId>,
	hints: HashMap<WindowId, WindowHandle<Panel>>,
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
	escape: Rc<dyn Fn(&mut App)>,
	trigger: Option<Bounds<Pixels>>,
	origin: Point<Pixels>,
	viewport: Size<Pixels>,
	hover_viewport: Bounds<Pixels>,
	width: Pixels,
	viewport_clip: Option<Bounds<Pixels>>,
	placement: PopoverPlacement,
	preview: bool,
	attached: bool,
	passive: bool,
	geometry: MaterialGeometry,
	focus: Option<FocusHandle>,
}
impl Content {
	fn bounds(&self, height: Pixels) -> Bounds<Pixels> {
		let size = gpui::size(self.width, height);
		let origin = if self.preview && !self.attached {
			self.trigger.map_or(self.origin, |trigger| {
				super::hover_origin(trigger, size, self.hover_viewport)
			})
		} else {
			super::surface_origin(self.origin, size, self.viewport, self.attached)
		};
		Bounds::new(origin, size)
	}

	fn is_modal(&self) -> bool {
		self.attached && self.focus.is_some()
	}
}

struct Panel {
	parent: AnyWindowHandle,
	surface: Option<MaterialPanel>,
	content: Option<Content>,
	focus: FocusHandle,
	focus_pending: bool,
	height: Pixels,
	resize: Option<Tween>,
}

impl Panel {
	fn check_hover(&mut self, position: Point<Pixels>, cx: &mut App) {
		if let Some(content) = &self.content
			&& matches!(content.placement, PopoverPlacement::Hover { dismiss_on_exit: true, .. })
			&& let Some(trigger) = content.trigger
			&& !super::inside_hover_preview(trigger, content.bounds(self.height), position)
		{
			self.close(cx);
		}
	}

	fn close(&mut self, cx: &mut App) {
		self.resize = None;
		if let Some(content) = self.content.take() {
			(content.dismiss)(cx);
		}
		if let Some(surface) = &mut self.surface {
			surface.set_open(false);
		}
	}

	fn place(&mut self, window: &mut Window, cx: &mut App) {
		let Some(content) = &self.content else { return };
		let bounds = content.bounds(self.height);
		if let Some(surface) = &mut self.surface {
			surface.set_radius(content.geometry.radius);
			if surface.place(bounds) {
				window.bounds_changed(cx);
			}
			if let Some(clip) = content.geometry.clip {
				surface.clip_content(clip);
			}
			if content.attached {
				let contour = content.geometry.clip.unwrap_or_else(|| {
					Bounds::new(gpui::point(gpui::px(0.), gpui::px(0.)), bounds.size)
				});
				surface.occlude_attached_content(contour, content.viewport_clip);
			}
		}
	}
}

impl Render for Panel {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		if !ui_frame::application_active()
			&& self.content.as_ref().is_some_and(|c| !c.attached || c.passive)
		{
			self.close(cx);
		}
		let Some(content) = &self.content else { return gpui::div().into_any_element() };
		let width = content.width;
		let opacity = content.geometry.opacity;
		let focus = content.focus.clone().unwrap_or_else(|| self.focus.clone());
		let foreground = (content.render)(window, cx);
		let track_exit =
			matches!(content.placement, PopoverPlacement::Hover { dismiss_on_exit: true, .. });
		let owner = cx.entity().downgrade();
		let measured = cx.entity().downgrade();
		let root = gpui::div()
			.relative()
			.w_full()
			.when(content.is_modal(), |root| {
				root.h(self.height).bg(super::modal_backdrop(
					ui_theme::ColorScheme::current(),
					Some(Surface::Popover.material()),
				))
			})
			.when(!content.attached, ui_theme::menu_outline)
			.font_family(ui_theme::FONT_FAMILY)
			.text_size(gpui::px(ui_theme::COMPACT_BODY_SIZE))
			.line_height(gpui::px(ui_theme::COMPACT_BODY_LINE_HEIGHT))
			.text_color(ui_theme::rgb(ui_theme::TEXT))
			.track_focus(&self.focus)
			.on_key_down(cx.listener(|panel, event: &gpui::KeyDownEvent, window, cx| {
				let modal = panel.content.as_ref().is_some_and(Content::is_modal);
				match event.keystroke.key.as_str() {
					"escape" => {
						if let Some(content) = panel.content.as_ref().filter(|c| c.attached) {
							(content.escape)(cx);
						} else {
							panel.close(cx);
						}
						if !modal {
							let parent = panel.parent;
							cx.defer(move |cx| {
								let _ = parent.update(cx, |_, window, _| window.activate_window());
							});
						}
					},
					"down" if !modal => window.focus_next(cx),
					"up" if !modal => window.focus_prev(cx),
					_ => return,
				}
				cx.stop_propagation();
			}))
			.child(foreground)
			.child(
				gpui::canvas(
					|_, _, _| (),
					move |_, _, window, _| {
						super::observe_pointer_departure(window);
						if track_exit {
							window.on_mouse_event(
								move |event: &gpui::MouseExitEvent, phase, _, cx| {
									if phase.capture() {
										let _ = owner.update(cx, |panel, cx| {
											if let Some(content) = &panel.content {
												let position = content.bounds(panel.height).origin
													+ event.position;
												panel.check_hover(position, cx);
											}
										});
									}
								},
							);
						}
					},
				)
				.absolute()
				.inset_0(),
			);
		crate::ui_layout::intrinsic(width, root, move |size, window, cx| {
			let _ = measured.update(cx, |panel, cx| {
				if let Some(content) = panel.content.as_ref().filter(|content| !content.attached) {
					let height = size.height.max(gpui::px(1.));
					panel.height = if matches!(content.placement, PopoverPlacement::Hover { .. }) {
						super::popover_height(&mut panel.resize, height, window, cx)
					} else {
						height
					};
					panel.place(window, cx);
				}
				let modal = panel.content.as_ref().is_some_and(Content::is_modal);
				if let Some(surface) = &mut panel.surface {
					surface.refresh_material_with_menu_tint(!modal);
				}
				let focus = focus.clone();
				cx.defer_in(window, move |panel, window, cx| {
					if panel.content.is_none() {
						return;
					}
					if let Some(surface) = &mut panel.surface {
						surface.set_open_at_opacity(true, opacity);
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
	surface_available(Surface::Popover)
}

pub(super) fn surface_available(surface: Surface) -> bool {
	native_material_panel::available(surface)
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
	let hover_viewport = super::hover_viewport(window);
	let placement = popover.placement;
	let content = popover.content.take().expect("native popover content");
	gpui::canvas(
		move |bounds, _, cx| {
			let inset = if popover.preview { 0. } else { ui_theme::POPOVER_PADDING };
			let request = Content {
				id: popover.id,
				render: content,
				escape: popover.dismiss.clone(),
				dismiss: popover.dismiss,
				trigger: popover.trigger.get(),
				origin: bounds.origin
					+ gpui::point(
						gpui::px(inset),
						gpui::px(if matches!(placement, PopoverPlacement::Below) {
							ui_theme::POPOVER_GAP
						} else {
							0.
						}),
					),
				viewport,
				hover_viewport,
				width: match placement {
					PopoverPlacement::Hover { width, .. } => width,
					_ => bounds.size.width - gpui::px(inset * 2.),
				},
				viewport_clip: None,
				placement,
				preview: popover.preview,
				attached: false,
				passive: false,
				geometry: MaterialGeometry {
					radius: f64::from(ui_theme::POPOVER_RADIUS),
					clip: None,
					opacity: 1.,
				},
				focus: popover.focus,
			};
			cx.defer(move |cx| present(parent, request, cx));
		},
		move |_, _, window, _| {
			super::observe_pointer_departure(window);
			if matches!(placement, PopoverPlacement::Hover { dismiss_on_exit: true, .. }) {
				super::observe_preview_pointer(window, move |position, cx| {
					if let Some(child) =
						cx.global::<Hosts>().children.get(&parent.window_id()).copied()
					{
						let _ = child.update(cx, |panel, _, cx| panel.check_hover(position, cx));
					}
				});
			}
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

/// A native dialog owns its parent's overlay plane until dismissal. Its own child
/// menus and hints have a different parent, so they keep their usual behavior.
fn enter_modal(parent: WindowId, id: &ElementId, cx: &mut App) {
	let hosts = cx.global_mut::<Hosts>();
	if hosts.modals.get(&parent) == Some(id) {
		return;
	}
	hosts.modals.insert(parent, id.clone());
	let popup = hosts.active.remove(&parent);
	let siblings: Vec<_> = hosts
		.accessories
		.iter()
		.filter(|((owner, key), _)| *owner == parent && key != id)
		.map(|(_, child)| *child)
		.chain(hosts.hints.get(&parent).copied())
		.collect();
	if let Some(popup) = popup {
		(popup.dismiss)(cx);
	}
	for child in siblings {
		let _ = child.update(cx, |panel, _, cx| panel.close(cx));
	}
}

fn present(parent: AnyWindowHandle, request: Content, cx: &mut App) {
	let owner = parent.window_id();
	if !request.is_modal() && cx.global::<Hosts>().modals.contains_key(&owner) {
		return;
	}
	let key = (owner, request.id.clone());
	let existing = if request.passive {
		cx.global::<Hosts>().hints.get(&parent.window_id()).copied()
	} else if request.attached {
		cx.global::<Hosts>().accessories.get(&key).copied()
	} else {
		cx.global::<Hosts>().children.get(&parent.window_id()).copied()
	};
	let child = existing
		.or_else(|| parent.update(cx, |_, window, cx| create(parent, window, cx)).ok().flatten());
	let Some(child) = child else { return };
	if request.is_modal() {
		enter_modal(owner, &request.id, cx);
	}
	if request.passive {
		cx.global_mut::<Hosts>().hints.insert(parent.window_id(), child);
	} else if request.attached {
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
			panel.focus_pending =
				request.is_modal() || (!request.preview && !request.attached && !request.passive);
		}
		if request.attached
			&& let Some(bounds) = request.trigger
		{
			// Attached surfaces use their parent's complete layout. Avoid resizing
			// Metal with the new width and the previous frame's height first.
			panel.height = bounds.size.height.max(gpui::px(1.));
		}
		panel.content = Some(request);
		// Transient content is measured before placement in Panel::render. Reusing
		// the previous height here makes a short/long preview resize twice.
		if panel.content.as_ref().is_some_and(|content| content.attached) {
			panel.place(window, cx);
		}
		if let Some(surface) = &mut panel.surface {
			surface.set_passive(panel.content.as_ref().is_some_and(|content| content.passive));
			surface.prepare_open();
		}
		cx.notify();
	});
}

pub(super) fn render_surface(
	surface: MaterialSurface,
	window: &mut Window,
	cx: &mut App,
) -> AnyElement {
	initialize(cx);
	let parent = window.window_handle();
	let viewport = window.viewport_size();
	let hover_viewport = super::hover_viewport(window);
	let outside_id = surface.id.clone();
	gpui::canvas(
		move |bounds, window, cx| {
			// Attached native windows bypass GPUI's paint mask. Carry the
			// current viewport into their material and hit region as well.
			let visible = bounds.intersect(&window.content_mask().bounds);
			let viewport_clip = (visible != bounds)
				.then(|| Bounds::new(visible.origin - bounds.origin, visible.size));
			let request = Content {
				id: surface.id,
				render: surface.content,
				dismiss: surface.dismiss,
				escape: surface.escape,
				trigger: Some(bounds),
				origin: bounds.origin,
				viewport,
				hover_viewport,
				width: bounds.size.width,
				viewport_clip,
				placement: PopoverPlacement::Below,
				preview: true,
				attached: true,
				passive: false,
				geometry: MaterialGeometry {
					radius: surface.radius,
					clip: surface.clip,
					opacity: surface.opacity,
				},
				focus: surface.modal_focus,
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
	let key = (parent.window_id(), id);
	if !cx.has_global::<Hosts>() {
		return;
	}
	if let Some(child) = cx.global::<Hosts>().accessories.get(&key).copied() {
		let modal = child.update(cx, |panel, window, cx| {
			let modal = panel.content.as_ref().is_some_and(Content::is_modal);
			if modal && let Some(surface) = &panel.surface {
				surface.restore_modal_parent_focus();
			}
			panel.close(cx);
			if modal {
				// Closing the modal host also releases any nested menus and accessories.
				window.remove_window();
			}
			modal
		});
		if modal.unwrap_or(false) {
			let hosts = cx.global_mut::<Hosts>();
			hosts.accessories.remove(&key);
			if hosts.modals.get(&key.0) == Some(&key.1) {
				hosts.modals.remove(&key.0);
			}
		}
	}
}

/// Measure in GPUI's tooltip slot, then compose the foreground once in its material host.
pub(super) fn render_hint(
	id: ElementId,
	content: Rc<PopoverContent>,
	window: &mut Window,
	cx: &mut App,
) -> AnyElement {
	initialize(cx);
	HintAnchor { id, measure: content(window, cx), content }.into_any_element()
}

struct HintAnchor {
	id: ElementId,
	measure: AnyElement,
	content: Rc<PopoverContent>,
}
impl IntoElement for HintAnchor {
	type Element = Self;

	fn into_element(self) -> Self {
		self
	}
}
impl gpui::Element for HintAnchor {
	type PrepaintState = ();
	type RequestLayoutState = ();

	fn id(&self) -> Option<ElementId> {
		None
	}

	fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
		None
	}

	fn request_layout(
		&mut self,
		_: Option<&gpui::GlobalElementId>,
		_: Option<&gpui::InspectorElementId>,
		window: &mut Window,
		cx: &mut App,
	) -> (gpui::LayoutId, ()) {
		(self.measure.request_layout(window, cx), ())
	}

	fn prepaint(
		&mut self,
		_: Option<&gpui::GlobalElementId>,
		_: Option<&gpui::InspectorElementId>,
		bounds: Bounds<Pixels>,
		_: &mut (),
		window: &mut Window,
		cx: &mut App,
	) {
		let parent = window.window_handle();
		let request = Content {
			id: self.id.clone(),
			render: self.content.clone(),
			dismiss: Rc::new(|_| {}),
			escape: Rc::new(|_| {}),
			trigger: Some(bounds),
			origin: bounds.origin,
			viewport: window.viewport_size(),
			hover_viewport: super::hover_viewport(window),
			width: bounds.size.width,
			viewport_clip: None,
			placement: PopoverPlacement::Below,
			preview: true,
			attached: true,
			passive: true,
			geometry: MaterialGeometry {
				radius: f64::from(ui_theme::POPOVER_RADIUS),
				clip: None,
				opacity: 1.,
			},
			focus: None,
		};
		cx.defer(move |cx| present(parent, request, cx));
	}

	fn paint(
		&mut self,
		_: Option<&gpui::GlobalElementId>,
		_: Option<&gpui::InspectorElementId>,
		_: Bounds<Pixels>,
		_: &mut (),
		_: &mut (),
		window: &mut Window,
		_: &mut App,
	) {
		super::observe_pointer_departure(window);
	}
}

pub(super) fn hide_hint(parent: AnyWindowHandle, id: ElementId, cx: &mut App) {
	if cx.has_global::<Hosts>()
		&& let Some(child) = cx.global::<Hosts>().hints.get(&parent.window_id()).copied()
	{
		let _ = child.update(cx, |panel, _, cx| {
			if panel.content.as_ref().is_some_and(|content| content.id == id) {
				panel.close(cx);
			}
		});
	}
}

fn initialize(cx: &mut App) {
	if cx.has_global::<Hosts>() {
		return;
	}
	cx.set_global(Hosts::default());
	cx.on_window_closed(|cx, id| {
		cx.global_mut::<Hosts>().active.remove(&id);
		cx.global_mut::<Hosts>().modals.remove(&id);
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
		if let Some(child) = cx.global_mut::<Hosts>().hints.remove(&id) {
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
					cx.observe_window_activation(window, |panel: &mut Panel, window, cx| {
						if !window.is_window_active()
							&& !ui_frame::application_active()
							&& panel
								.content
								.as_ref()
								.is_some_and(|content| !content.attached || content.passive)
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
						resize: None,
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
	if let Some(hint) = cx.global::<Hosts>().hints.get(&parent.window_id()).copied() {
		let _ = hint.update(cx, |panel, _, cx| panel.close(cx));
	}
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
