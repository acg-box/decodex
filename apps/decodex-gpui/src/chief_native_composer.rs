//! Native composition boundary for the existing Chief composer.
use super::ChiefSurface;
use crate::ui_theme::{
	self,
	native_glass_panel::{self, GlassPanel},
};
use gpui::{
	AnyElement, AnyWindowHandle, Bounds, Context, Entity, Focusable, Pixels, Render, Subscription,
	Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div,
	point, prelude::*, px, size,
};

pub(super) struct NativeComposer {
	pub(super) enabled: bool,
	child: Option<WindowHandle<ComposerPanel>>,
	bounds: Option<Bounds<Pixels>>,

	height: f32,
	creating: bool,
	failed: bool,
	resume_after: Option<std::time::Instant>,
}
impl Default for NativeComposer {
	fn default() -> Self {
		Self {
			enabled: false,
			child: None,
			bounds: None,
			height: 42.,
			creating: false,
			failed: false,
			resume_after: None,
		}
	}
}
struct ComposerPanel {
	owner: Entity<ChiefSurface>,
	parent: AnyWindowHandle,
	glass: Option<GlassPanel>,
	_observation: Subscription,
}

impl ChiefSurface {
	/// Called by the main shell; settings and other windows must not create composers.
	pub(crate) fn prepare_native_composer(
		&mut self,
		allowed: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let requested = allowed
			&& self.selected_is_manager()
			&& native_glass_panel::available()
			&& self.resources.is_none()
			&& self.integrations.is_none()
			&& self.activity_detail.is_none()
			&& self.usage_estimate.is_none()
			&& !self.graph_expanded;
		let now = std::time::Instant::now();
		if !requested {
			self.native_composer.resume_after = Some(now + std::time::Duration::from_millis(200));
		}
		let settling = self.native_composer.resume_after.is_some_and(|until| until > now);
		if requested && settling {
			crate::ui_motion::request_frame(window, cx);
		}
		let enabled = requested && !settling && !self.native_composer.failed;
		if enabled
			&& !self.native_composer.enabled
			&& self.composer.focus_handle(cx).is_focused(window)
			&& let Some(child) = self.native_composer.child
		{
			cx.defer(move |cx| {
				let _ = child.update(cx, |panel, window, cx| {
					let focus = panel.owner.read(cx).composer.focus_handle(cx);
					window.focus(&focus, cx);
					if let Some(glass) = &panel.glass {
						glass.focus_text();
					}
					window.activate_window();
				});
			});
		}
		if self.native_composer.enabled != enabled {
			self.native_composer.enabled = enabled;
			// Shell owns the decision, but Chief owns the cached composer layout.
			cx.notify();
		}
		self.sync_native_composer(cx);
		if !enabled {
			return;
		}

		if self.native_composer.child.is_some() || self.native_composer.creating {
			return;
		}
		self.native_composer.creating = true;
		let owner = cx.entity();
		let parent = window.window_handle();
		// Opening draws the new root immediately. Do this after releasing Chief's borrow.
		cx.defer(move |cx| {
			let result = parent
				.update(cx, |_, parent_window, cx| create_panel(owner.clone(), parent_window, cx));
			owner.update(cx, |s, cx| {
				s.native_composer.creating = false;
				s.native_composer.child = result.ok().flatten();
				s.native_composer.enabled = s.native_composer.child.is_some();
				s.native_composer.failed = s.native_composer.child.is_none();
				cx.notify();
			});
		});
	}

	/// Reconcile from the latest state after layout. Cached Chief views may skip
	/// prepaint when only the shell (for example notifications) changes.
	fn sync_native_composer(&self, cx: &mut Context<Self>) {
		let owner = cx.entity().downgrade();
		cx.defer(move |cx| {
			let Ok((child, bounds, enabled)) = owner.read_with(cx, |s, _| {
				(s.native_composer.child, s.native_composer.bounds, s.native_composer.enabled)
			}) else {
				return;
			};
			if let Some(child) = child {
				let _ = child.update(cx, |panel, window, cx| {
					if let Some(glass) = &mut panel.glass {
						if enabled && let Some(bounds) = bounds {
							glass.set_style(
								ui_theme::window_material::GlassStyle::configured()
									== ui_theme::window_material::GlassStyle::Clear,
							);
							if glass.place(bounds) {
								window.bounds_changed(cx);
							}
						}
						glass.set_visible(enabled && bounds.is_some());
					}
				});
			}
		});
	}

	pub(super) fn render_native_composer_anchor(&self, cx: &mut Context<Self>) -> AnyElement {
		let owner = cx.entity().downgrade();
		let anchor = gpui::canvas(
			move |bounds, _, cx| {
				let _ = owner.update(cx, |s, cx| {
					s.native_composer.bounds = Some(bounds);
					s.sync_native_composer(cx);
				});
			},
			|_, _, _, _| {},
		);
		div()
			.w_full()
			.px_4()
			.pt(px(12.))
			.pb(px(20.))
			.flex()
			.justify_center()
			.child(
				div()
					.relative()
					.w_full()
					.max_w(px(820.))
					.h(px(self.native_composer.height))
					.child(anchor.absolute().size_full())
					.child(self.render_composer_popover(cx)),
			)
			.into_any_element()
	}
}

impl Render for ComposerPanel {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let owner = self.owner.downgrade();
		let capsule = self.owner.update(cx, |s, cx| s.render_composer_capsule(true, window, cx));
		let parent = self.parent;
		div()
			.w_full()
			.font_family(ui_theme::FONT_FAMILY)
			.on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
				forward(parent, &super::super::DismissStatus, cx);
			})
			.text_size(px(ui_theme::BODY_SIZE))
			.text_color(gpui::rgb(ui_theme::TEXT))
			.on_children_prepainted(move |bounds, _, cx| {
				if let Some(bounds) = bounds.first() {
					let height = f32::from(bounds.size.height).max(42.);
					let _ = owner.update(cx, |s, cx| {
						if (s.native_composer.height - height).abs() > 0.5 {
							s.native_composer.height = height;
							cx.notify();
						}
					});
				}
			})
			.on_action(move |action: &super::super::ActivateChief, _, cx| {
				forward(parent, action, cx)
			})
			.on_action(move |action: &super::super::ActivateHealth, _, cx| {
				forward(parent, action, cx)
			})
			.on_action(move |action: &super::super::ActivateSettings, _, cx| {
				forward(parent, action, cx)
			})
			.on_action(move |action: &super::super::ToggleSidebar, _, cx| {
				forward(parent, action, cx)
			})
			.on_action(move |action: &super::super::ToggleInspector, _, cx| {
				forward(parent, action, cx)
			})
			.on_action(move |action: &super::super::ToggleGraph, _, cx| forward(parent, action, cx))
			.on_action(move |action: &super::super::InterruptReply, _, cx| {
				forward(parent, action, cx)
			})
			.on_action(move |action: &super::super::NavigateBack, _, cx| {
				forward(parent, action, cx)
			})
			.on_action(move |action: &super::super::NavigateForward, _, cx| {
				forward(parent, action, cx)
			})
			.child(capsule)
	}
}
fn forward(parent: AnyWindowHandle, action: &dyn gpui::Action, cx: &mut gpui::App) {
	let action = action.boxed_clone();
	cx.defer(move |cx| {
		let _ = parent.update(cx, |_, window, cx| window.dispatch_action(action, cx));
	});
	cx.stop_propagation();
}

fn create_panel(
	owner: Entity<ChiefSurface>,
	parent_window: &mut Window,
	cx: &mut gpui::App,
) -> Option<WindowHandle<ComposerPanel>> {
	let parent = parent_window.window_handle();
	let child = cx
		.open_window(
			WindowOptions {
				kind: WindowKind::Normal,
				titlebar: None,
				window_bounds: Some(WindowBounds::Windowed(Bounds::new(
					point(px(0.), px(0.)),
					size(px(600.), px(42.)),
				))),
				window_background: WindowBackgroundAppearance::Transparent,
				show: false,
				focus: false,
				..Default::default()
			},
			move |_, cx| {
				cx.new(|cx| {
					let observation = cx.observe(&owner, |_, _, cx| cx.notify());
					ComposerPanel { owner, parent, glass: None, _observation: observation }
				})
			},
		)
		.ok()?;
	let installed = child
		.update(cx, |s, window, _| {
			s.glass = GlassPanel::install(parent_window, window, 24.);
			s.glass.is_some()
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
