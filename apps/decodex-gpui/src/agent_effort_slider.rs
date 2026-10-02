//! Discrete reasoning slider. Values come from the selected model's capabilities.
use gpui::{
	AnyElement, App, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
	RenderOnce, Window,
};

use crate::{
	shell::agent_surface::composer::{
		controls,
		controls::{
			AgentSurface, Context, InteractiveElement, IntoElement, ParentElement, Role,
			StatefulInteractiveElement, Styled, ui_theme::TEXT,
		},
	},
	ui_motion,
};

impl AgentSurface {
	pub(crate) fn set_effort_position(&mut self, position: f32, cx: &mut Context<Self>) {
		if self.effort_drag.is_some() {
			self.effort_pointer = Some(position.clamp(0., 1.));
		}

		let levels = self.model_efforts(cx);

		if let Some(level) = levels.get(index_at(position, levels.len())) {
			self.effort = level.clone();

			self.mark_effort_intent(cx);
			self.save_draft_document(cx);
			cx.notify();
		}
	}

	fn configured_effort_label(&self) -> AnyElement {
		gpui::div()
			.id("reasoning-configured")
			.debug_selector(|| "reasoning-configured".into())
			.text_size(gpui::px(12.))
			.text_color(gpui::rgb(TEXT))
			.child(format!("{} · configured", controls::level_label(&self.composer_effort_value())))
			.into_any_element()
	}

	pub(crate) fn effort_scale(&self, cx: &mut Context<Self>) -> AnyElement {
		if self.root_id().is_none() && self.creation_inherit_effort {
			return gpui::div().child("Inherited from native configuration").into_any_element();
		}

		self.explicit_effort_scale(cx)
	}

	fn handle_effort_key(&mut self, e: &KeyDownEvent, cx: &mut Context<Self>) {
		let levels = self.model_efforts(cx);
		let current = levels.iter().position(|v| *v == self.effort).unwrap_or(0);
		let next = match e.keystroke.key.as_str() {
			"left" | "down" => current.saturating_sub(1),
			"right" | "up" => (current + 1).min(levels.len().saturating_sub(1)),
			"home" => 0,
			"end" => levels.len().saturating_sub(1),
			_ => return,
		};

		if let Some(level) = levels.get(next) {
			self.effort = level.clone();

			self.mark_effort_intent(cx);
			self.save_draft_document(cx);
		}

		cx.stop_propagation();
		cx.notify();
	}

	fn explicit_effort_scale(&self, cx: &mut Context<Self>) -> AnyElement {
		let levels = self.model_efforts(cx);
		let count = levels.len();

		if count == 0 {
			return self.configured_effort_label();
		}

		let index = levels.iter().position(|v| *v == self.effort).unwrap_or(0);
		let fraction =
			self.effort_pointer.unwrap_or(index as f32 / count.saturating_sub(1).max(1) as f32);
		let measured = cx.entity().downgrade();
		let events = measured.clone();

		gpui::div()
			.px(gpui::px(7.))
			.py(gpui::px(3.))
			.flex()
			.items_center()
			.gap(gpui::px(8.))
			.child(
				gpui::div()
					.min_w(gpui::px(54.))
					.flex_none()
					.text_size(gpui::px(12.))
					.whitespace_nowrap()
					.text_color(gpui::rgb(TEXT))
					.child(controls::level_label(&self.composer_effort_value())),
			)
			.child(
				gpui::div()
					.id("reasoning-slider")
					.debug_selector(|| "reasoning-slider".into())
					.role(Role::Slider)
					.track_focus(&self.effort_focus)
					.tab_index(0)
					.aria_label(format!(
						"Reasoning: {}. Use Left and Right to adjust.",
						controls::level_label(&self.composer_effort_value())
					))
					.flex_1()
					.h(gpui::px(24.))
					.relative()
					.cursor_pointer()
					.on_mouse_down(
						MouseButton::Left,
						cx.listener(move |s, e: &MouseDownEvent, window, cx| {
							window.focus(&s.effort_focus, cx);

							let Some(bounds) = s.effort_track_bounds else {
								return;
							};
							let (left, width) =
								(f32::from(bounds.origin.x), f32::from(bounds.size.width).max(1.));

							s.effort_drag = Some((left, width));

							s.set_effort_position((f32::from(e.position.x) - left) / width, cx);
							cx.stop_propagation();
						}),
					)
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						s.handle_effort_key(e, cx);
					}))
					.child(
						gpui::canvas(
							move |bounds, _, cx| {
								let _ = measured
									.update(cx, |s, _| s.effort_track_bounds = Some(bounds));
							},
							move |_, _, window, _| {
								let movement = events.clone();

								window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
									if !phase.bubble() {
										return;
									}

									let _ = movement.update(cx, |s, cx| {
										if let Some((left, width)) = s.effort_drag {
											if e.pressed_button == Some(MouseButton::Left) {
												s.set_effort_position(
													(f32::from(e.position.x) - left) / width,
													cx,
												);
											} else {
												s.effort_drag = None;
												s.effort_pointer = None;

												cx.notify();
											}
										}
									});
								});

								let release = events.clone();

								window.on_mouse_event(move |_: &MouseUpEvent, _, _, cx| {
									let _ = release.update(cx, |s, cx| {
										if s.effort_drag.take().is_some() {
											s.effort_pointer = None;

											cx.notify();
										}
									});
								});
							},
						)
						.absolute()
						.inset_0(),
					)
					.child(SliderTrack { fraction, dragging: self.effort_drag.is_some(), count }),
			)
			.into_any_element()
	}
}

#[derive(gpui::IntoElement)]
struct SliderTrack {
	fraction: f32,
	dragging: bool,
	count: usize,
}
impl RenderOnce for SliderTrack {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let fraction =
			ui_motion::direct_value("reasoning-thumb", self.fraction, self.dragging, window, cx);
		let hover = window.use_keyed_state("reasoning-hover-state", cx, |_, _| false);
		let active = *hover.read(cx) || self.dragging;
		let feedback =
			ui_motion::value("reasoning-hover-motion", if active { 1. } else { 0. }, window, cx);
		let thumb = 12. + 2. * feedback;

		gpui::div()
			.id("reasoning-feedback")
			.absolute()
			.inset_0()
			.on_hover(move |over, _, cx| {
				hover.update(cx, |state, cx| {
					*state = *over;

					cx.notify();
				})
			})
			.child(
				gpui::div()
					.absolute()
					.left_0()
					.right_0()
					.top(gpui::px(10. - feedback * 0.5))
					.h(gpui::px(4. + feedback))
					.rounded_full()
					.bg(gpui::rgba(0xffffff18)),
			)
			.child(
				gpui::div()
					.absolute()
					.left_0()
					.top(gpui::px(10. - feedback * 0.5))
					.w(gpui::relative(fraction))
					.h(gpui::px(4. + feedback))
					.rounded_full()
					.bg(gpui::rgb(if active { 0xe7e7ea } else { 0xc0c0c5 })),
			)
			.children((0..self.count).map(|i| {
				gpui::div()
					.absolute()
					.left(gpui::relative(i as f32 / self.count.saturating_sub(1).max(1) as f32))
					.top(gpui::px(20.))
					.ml(gpui::px(-1.))
					.w(gpui::px(2.))
					.h(gpui::px(3.))
					.rounded_full()
					.bg(gpui::rgba(0xd5d5da85))
			}))
			.child(
				gpui::div()
					.absolute()
					.left(gpui::relative(fraction))
					.top(gpui::px(12. - thumb / 2.))
					.ml(gpui::px(-thumb / 2.))
					.w(gpui::px(thumb))
					.h(gpui::px(thumb))
					.rounded_full()
					.bg(gpui::rgb(0xe7e7ea)),
			)
	}
}

fn index_at(position: f32, count: usize) -> usize {
	(position.clamp(0.0, 1.0) * count.saturating_sub(1) as f32).round() as usize
}
#[cfg(test)]
mod tests {
	use std::thread;

	use gpui::AppContext;

	use crate::shell::agent_surface::composer::{
		controls,
		controls::effort_slider::{self, AgentSurface, MouseButton},
	};
	use decodex_protocol::{AgentCapabilitiesResult, AgentModelDto};
	#[cfg(test)] use decodex_protocol::{ConversationModel, ConversationReasoningEffort};

	#[gpui::test]
	fn real_slider_drag_and_outside_dismiss(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.mark_model_intent(cx);

			s.capabilities = Some(AgentCapabilitiesResult::Available {
				models: vec![AgentModelDto {
					model: decodex_protocol::ConversationModel::new(s.model.read(cx).content())
						.unwrap(),
					name: "Test model".into(),
					efforts: vec![
						crate::shell::agent_surface::ConversationReasoningEffort::Low,
						crate::shell::agent_surface::ConversationReasoningEffort::High,
						crate::shell::agent_surface::ConversationReasoningEffort::Ultra,
						crate::shell::agent_surface::ConversationReasoningEffort::Persistent,
					],
					default_effort: Some(
						crate::shell::agent_surface::ConversationReasoningEffort::High,
					),
					supports_fast: true,
					service_tiers: vec![],
					default_service_tier: None,
					available_cyber_programs: None,
					specialty: None,
					supports_images: true,
					availability: None,
					upgrade: None,
				}],
				memory_enabled: None,
			});
			s.composer_menu = Some("model");
			s.composer_menu_content = Some("model");
		});

		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		for _ in 0..2 {
			thread::sleep(std::time::Duration::from_millis(200));

			visual.update(|w, cx| {
				w.draw(cx).clear();
			});
		}

		let bounds = surface.update(visual, |s, _| s.effort_track_bounds.unwrap());
		let start = bounds.center();

		visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
		surface.update(visual, |s, _| assert!(s.effort_drag.is_some()));
		visual.simulate_mouse_move(
			gpui::point(bounds.left() + bounds.size.width * 0.4, start.y),
			MouseButton::Left,
			Default::default(),
		);
		surface.update(visual, |s, _| {
			assert!((s.effort_pointer.unwrap() - 0.4).abs() < 0.01);
			assert_eq!(s.effort, crate::shell::agent_surface::ConversationReasoningEffort::High);
		});
		visual.simulate_mouse_move(
			gpui::point(bounds.right() + gpui::px(30.), start.y),
			MouseButton::Left,
			Default::default(),
		);
		surface.update(visual, |s, _| {
			assert_eq!(
				s.effort,
				crate::shell::agent_surface::ConversationReasoningEffort::Persistent
			);
			assert_eq!(controls::level_label(s.effort.as_str()), "Persistent");
		});
		visual.simulate_mouse_move(
			gpui::point(bounds.left() - gpui::px(30.), start.y),
			MouseButton::Left,
			Default::default(),
		);
		surface.update(visual, |s, _| {
			assert_eq!(s.effort, crate::shell::agent_surface::ConversationReasoningEffort::Low)
		});
		visual.simulate_mouse_up(start, MouseButton::Left, Default::default());
		surface.update(visual, |s, _| {
			assert!(s.effort_drag.is_none());
			assert!(s.effort_pointer.is_none());
		});
		visual.simulate_keystrokes("right");
		surface.update(visual, |s, _| {
			assert_eq!(s.effort, crate::shell::agent_surface::ConversationReasoningEffort::High)
		});
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		let model = surface.update(visual, |s, _| s.menu_trigger_bounds["model"].center());

		visual.simulate_mouse_down(model, MouseButton::Left, Default::default());
		visual.simulate_mouse_up(model, MouseButton::Left, Default::default());
		surface.update(visual, |s, _| assert!(s.composer_menu.is_none()));
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});
		visual.simulate_mouse_down(model, MouseButton::Left, Default::default());
		visual.simulate_mouse_up(model, MouseButton::Left, Default::default());
		surface.update(visual, |s, _| assert_eq!(s.composer_menu, Some("model")));
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});
		visual.simulate_mouse_down(
			gpui::point(gpui::px(400.), gpui::px(200.)),
			MouseButton::Left,
			Default::default(),
		);
		surface.update(visual, |s, _| assert!(s.composer_menu.is_none()));
	}

	#[gpui::test]
	fn slider_keeps_custom_catalog_effort_and_its_label(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.mark_model_intent(cx);

			let custom = ConversationReasoningEffort::new("provider-defined-effort").unwrap();

			s.capabilities = Some(AgentCapabilitiesResult::Available {
				memory_enabled: None,
				models: vec![AgentModelDto {
					model: ConversationModel::new(s.model.read(cx).content()).unwrap(),
					name: "Fixture".into(),
					efforts: vec![ConversationReasoningEffort::High, custom.clone()],
					default_effort: Some(custom.clone()),
					supports_fast: false,
					available_cyber_programs: None,
					specialty: None,
					supports_images: true,
					availability: None,
					upgrade: None,
					service_tiers: vec![],
					default_service_tier: None,
				}],
			});

			s.set_effort_position(1.0, cx);

			assert_eq!(s.effort, custom);
			assert_eq!(controls::level_label(s.effort.as_str()), "provider-defined-effort");
			assert_eq!(s.draft_profiles.execution.choice("agent").reasoning_effort, Some(custom));
			assert!(s.composer_capability_error(cx).is_none());
		});
	}

	#[test]
	fn slider_snaps_and_clamps_to_supported_stops() {
		assert_eq!(effort_slider::index_at(-1., 5), 0);
		assert_eq!(effort_slider::index_at(0.37, 5), 1);
		assert_eq!(effort_slider::index_at(0.38, 5), 2);
		assert_eq!(effort_slider::index_at(1.5, 5), 4);
		assert_eq!(effort_slider::index_at(0.9, 1), 0);
		assert_eq!(effort_slider::index_at(0.9, 0), 0);
	}
}
