//! Share one anchored native popover for model, permission and speed choices.
use composer::{MODEL_MENU_SHADOW, MODEL_MENU_WIDTH};
use gpui::{
	AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Render,
	Subscription, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowOptions,
	prelude::{InteractiveElement as _, IntoElement, ParentElement as _, Styled as _},
};

use crate::{
	shell::agent_surface::{AgentSurface, composer},
	ui_motion,
	ui_theme::{
		self, BODY_SIZE, FONT_FAMILY, TEXT, appearance, native_material_panel::MaterialPanel,
	},
};

#[derive(Default)]
pub(super) struct NativeModelMenu {
	child: Option<WindowHandle<ModelMenu>>,
	creating: bool,
	pub(super) failed: bool,
	height: f32,
}

struct ModelMenu {
	owner: Entity<AgentSurface>,
	surface: Option<MaterialPanel>,
	focus: FocusHandle,
	_observation: Subscription,
}
impl Render for ModelMenu {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let owner = self.owner.clone();
		let content = owner.update(cx, |s, cx| match s.composer_menu.or(s.composer_menu_content) {
			Some("permissions") => s.permission_options(cx),
			_ => s.model_options(cx),
		});

		gpui::div()
			.w_full()
			.p(gpui::px(MODEL_MENU_SHADOW))
			.font_family(FONT_FAMILY)
			.text_size(gpui::px(BODY_SIZE))
			.text_color(ui_theme::rgb(TEXT))
			.on_key_down({
				let owner = owner.clone();

				move |event, _, cx| {
					if event.keystroke.key == "escape" {
						owner.update(cx, |s, cx| {
							s.dismiss_composer_menu(cx);
							s.focus_native_message(cx);
						});
						cx.stop_propagation();
					}
				}
			})
			.on_children_prepainted(move |bounds, _, cx| {
				if let Some(bounds) = bounds.first() {
					let height = f32::from(bounds.size.height) + 2. * MODEL_MENU_SHADOW;

					owner.update(cx, |s, cx| {
						if (s.native_controls.model_menu.height - height).abs() > 0.5 {
							s.native_controls.model_menu.height = height;

							cx.notify();
						}
					});
				}
			})
			.id("native-model-menu")
			.track_focus(&self.focus)
			.child(content)
	}
}

impl AgentSurface {
	pub(crate) fn prepare_native_model_menu(
		&mut self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let anchor = self
			.native_controls
			.composer
			.bounds
			.zip(self.composer_menu.and_then(|menu| self.menu_trigger_bounds.get(menu)).copied())
			.map(|(composer, trigger)| Bounds::new(composer.origin + trigger.origin, trigger.size));
		let active = ui_motion::application_active();
		let available = self.composer_unavailable_reason().is_none();
		let open = active
			&& available
			&& self.native_controls.composer.enabled
			&& composer::selection_menu(self.composer_menu)
			&& anchor.is_some();
		let opacity = ui_motion::native_presence("model-menu-native-opacity", open, window, cx);
		let opacity = if active && available { opacity } else { 0. };

		if let Some(child) = self.native_controls.model_menu.child {
			let bounds = anchor.map(|anchor| {
				composer::model_menu_bounds(
					anchor,
					self.native_controls.model_menu.height.max(48.),
					window.viewport_size(),
				)
			});

			cx.defer(move |cx| {
				let _ = child.update(cx, |panel, window, cx| {
					if let Some(surface) = &mut panel.surface {
						if let Some(bounds) = bounds
							&& surface.place(bounds)
						{
							window.bounds_changed(cx);
						}

						surface.set_opacity(opacity);
						surface.set_visible(bounds.is_some() && opacity > 0.001);
					}
				});
			});

			return;
		}

		if !open
			|| self.native_controls.model_menu.creating
			|| self.native_controls.model_menu.failed
		{
			return;
		}

		self.native_controls.model_menu.creating = true;

		let owner = cx.entity();
		let parent = window.window_handle();

		cx.defer(move |cx| {
			let child = parent
				.update(cx, |_, window, cx| create(owner.clone(), parent, window, cx))
				.ok()
				.flatten();

			owner.update(cx, |s, cx| {
				s.native_controls.model_menu.child = child;
				s.native_controls.model_menu.creating = false;
				s.native_controls.model_menu.failed = child.is_none();

				cx.notify();
			});
		});
	}
}

fn create(
	owner: Entity<AgentSurface>,
	parent: AnyWindowHandle,
	window: &mut Window,
	cx: &mut App,
) -> Option<WindowHandle<ModelMenu>> {
	let child = cx
		.open_window(
			WindowOptions {
				titlebar: None,
				window_bounds: Some(WindowBounds::Windowed(Bounds::new(
					gpui::point(gpui::px(0.), gpui::px(0.)),
					gpui::size(gpui::px(MODEL_MENU_WIDTH + 2. * MODEL_MENU_SHADOW), gpui::px(240.)),
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

					cx.observe_window_activation(window, |panel: &mut ModelMenu, window, cx| {
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

					ModelMenu { owner, surface: None, focus, _observation: observation }
				})
			},
		)
		.ok()?;
	let installed = child
		.update(cx, |panel, child_window, _| {
			panel.surface = MaterialPanel::install_overlay(window, child_window);

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
