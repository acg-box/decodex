//! Native composition boundary for the existing Agent composer.
use std::time::Duration;

use gpui::{
	self, Action, AnyElement, AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity,
	Focusable, MouseButton, Pixels, Render, Subscription, Window, WindowBackgroundAppearance,
	WindowBounds, WindowHandle, WindowKind, WindowOptions,
	prelude::{InteractiveElement as _, IntoElement, ParentElement as _, Styled as _},
};

use crate::{
	shell::{
		ActivateAgent, ActivateHealth, ActivateSettings, DismissStatus, GrowPanel, GrowPanels,
		NavigateBack, NavigateForward, ResetPanel, ResetPanels, ShrinkPanel, ShrinkPanels,
		ToggleGraph, ToggleInspector, ToggleSidebar, agent_surface::AgentSurface,
	},
	ui_motion,
	ui_theme::{
		BODY_SIZE, FONT_FAMILY, TEXT,
		native_glass_panel::{self, GlassPanel},
		window_material::GlassStyle,
	},
};

pub(super) struct NativeComposer {
	pub(super) enabled: bool,
	child: Option<WindowHandle<ComposerPanel>>,
	pub(super) bounds: Option<Bounds<Pixels>>,

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
	owner: Entity<AgentSurface>,
	parent: AnyWindowHandle,
	glass: Option<GlassPanel>,
	_observation: Subscription,
}
impl Render for ComposerPanel {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let owner = self.owner.downgrade();
		let capsule = self.owner.update(cx, |s, cx| s.render_composer_capsule(true, window, cx));
		let parent = self.parent;
		let focus_owner = self.owner.clone();

		gpui::div()
			.w_full()
			.font_family(FONT_FAMILY)
			.on_mouse_down(MouseButton::Left, move |_, _, cx| {
				forward(parent, &DismissStatus, cx);
			})
			.text_size(gpui::px(BODY_SIZE))
			.text_color(gpui::rgb(TEXT))
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
			.id("native-composer-focus-panel")
			.capture_any_mouse_down(move |_, _, cx| {
				focus_owner.update(cx, |s, _| s.workspace.focused_panel = None);
			})
			.on_action(move |action: &ActivateAgent, _, cx| forward(parent, action, cx))
			.on_action(move |action: &ActivateHealth, _, cx| forward(parent, action, cx))
			.on_action(move |action: &ActivateSettings, _, cx| forward(parent, action, cx))
			.on_action(move |action: &ToggleSidebar, _, cx| forward(parent, action, cx))
			.on_action(move |action: &ToggleInspector, _, cx| forward(parent, action, cx))
			.on_action(move |action: &ShrinkPanel, _, cx| forward(parent, action, cx))
			.on_action(move |action: &GrowPanel, _, cx| forward(parent, action, cx))
			.on_action(move |action: &ResetPanel, _, cx| forward(parent, action, cx))
			.on_action(move |action: &ShrinkPanels, _, cx| forward(parent, action, cx))
			.on_action(move |action: &GrowPanels, _, cx| forward(parent, action, cx))
			.on_action(move |action: &ResetPanels, _, cx| forward(parent, action, cx))
			.on_action(move |action: &ToggleGraph, _, cx| forward(parent, action, cx))
			.on_action(move |action: &NavigateBack, _, cx| forward(parent, action, cx))
			.on_action(move |action: &NavigateForward, _, cx| forward(parent, action, cx))
			.child(capsule)
	}
}

impl AgentSurface {
	/// Called by the main shell; settings and other windows must not create composers.
	pub(crate) fn prepare_native_composer(
		&mut self,
		allowed: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		// Inline tool disclosures do not cover or replace the native composer.
		let requested = allowed
			&& !self.workspace.browsing
			&& self.snapshot.is_some()
			&& !self.selected_is_archived()
			&& (self.native_agents.selected.is_some() || self.selected_is_manager())
			&& native_glass_panel::available()
			&& self.resources.is_none()
			&& self.integrations.is_none()
			&& self.usage_estimate.is_none()
			&& !self.workspace.graph_expanded;
		let now = std::time::Instant::now();

		if !requested {
			self.native_composer.resume_after = Some(now + Duration::from_millis(200));
		}

		let settling = self.native_composer.resume_after.is_some_and(|until| until > now);

		if requested && settling {
			ui_motion::request_frame(window, cx);
		}

		let enabled = requested && !settling && !self.native_composer.failed;

		if enabled
			&& !self.native_composer.enabled
			&& self.conversation_composer().focus_handle(cx).is_focused(window)
			&& let Some(child) = self.native_composer.child
		{
			cx.defer(move |cx| {
				let _ = child.update(cx, |panel, window, cx| {
					let focus = panel.owner.read(cx).conversation_composer().focus_handle(cx);

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

			// Shell owns the decision, but Agent owns the cached composer layout.
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

		// Opening draws the new root immediately. Do this after releasing Agent's borrow.
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

	/// Reconcile from the latest state after layout. Cached Agent views may skip
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
							glass.set_style(GlassStyle::configured() == GlassStyle::Clear);

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

		gpui::div()
			.w_full()
			.px(gpui::px(crate::ui_theme::CONVERSATION_INSET))
			.pt(gpui::px(crate::ui_theme::COMPOSER_TOP_GAP))
			.pb(gpui::px(crate::ui_theme::COMPOSER_BOTTOM_GAP))
			.flex()
			.flex_col()
			.items_center()
			.child(
				gpui::div()
					.relative()
					.w_full()
					.max_w(gpui::px(crate::ui_theme::CONVERSATION_WIDTH))
					.h(gpui::px(self.native_composer.height))
					.child(anchor.absolute().size_full())
					.child(self.render_composer_popover(cx)),
			)
			.children(self.usage_line(cx))
			.into_any_element()
	}
}

fn forward(parent: AnyWindowHandle, action: &dyn Action, cx: &mut App) {
	let action = action.boxed_clone();

	cx.defer(move |cx| {
		let _ = parent.update(cx, |_, window, cx| window.dispatch_action(action, cx));
	});

	cx.stop_propagation();
}

fn create_panel(
	owner: Entity<AgentSurface>,
	parent_window: &mut Window,
	cx: &mut App,
) -> Option<WindowHandle<ComposerPanel>> {
	let parent = parent_window.window_handle();
	let child = cx
		.open_window(
			WindowOptions {
				kind: WindowKind::Normal,
				titlebar: None,
				window_bounds: Some(WindowBounds::Windowed(Bounds::new(
					gpui::point(gpui::px(0.), gpui::px(0.)),
					gpui::size(gpui::px(600.), gpui::px(42.)),
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
			s.glass = GlassPanel::install(
				parent_window,
				window,
				f64::from(crate::ui_theme::COMPOSER_RADIUS),
			);

			s.glass.is_some()
		})
		.unwrap_or(false);

	if !installed {
		let _ = child.update(cx, |_, window, _| window.remove_window());

		return None;
	}

	super::voice::Media::prepare_input(parent_window, "");

	cx.on_window_closed(move |cx, id| {
		if id == parent.window_id() {
			let _ = child.update(cx, |_, window, _| window.remove_window());
		}
	})
	.detach();

	Some(child)
}
