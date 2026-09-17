//! Discrete reasoning slider. Values come from the selected model's capabilities.
use super::*;
use gpui::{MouseButton, canvas, relative};

fn index_at(position: f32, count: usize) -> usize {
	(position.clamp(0.0, 1.0) * count.saturating_sub(1) as f32).round() as usize
}

impl ChiefSurface {
	pub(crate) fn set_effort_position(&mut self, position: f32, cx: &mut Context<Self>) {
		let levels = self.model_efforts(cx);
		if let Some(level) = levels.get(index_at(position, levels.len())) {
			self.effort = *level;
			cx.notify();
		}
	}

	pub(crate) fn effort_scale(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let levels = self.model_efforts(cx);
		let count = levels.len();
		let index = levels.iter().position(|v| *v == self.effort).unwrap_or(0);
		let fraction = index as f32 / count.saturating_sub(1).max(1) as f32;
		let measured = cx.entity().downgrade();
		let events = measured.clone();
		div()
			.px(px(14.))
			.py(px(6.))
			.flex()
			.flex_col()
			.gap(px(12.))
			.child(
				div()
					.w_full()
					.text_center()
					.text_size(px(14.))
					.text_color(rgb(ui_theme::TEXT))
					.child(level_label(self.effort.as_str())),
			)
			.child(
				div()
					.id("reasoning-slider")
					.role(Role::Slider)
					.track_focus(&self.effort_focus)
					.tab_index(0)
					.aria_label(format!(
						"Reasoning: {}. Use Left and Right to adjust.",
						level_label(self.effort.as_str())
					))
					.w_full()
					.h(px(30.))
					.relative()
					.cursor_pointer()
					.on_mouse_down(
						MouseButton::Left,
						cx.listener(move |s, e: &gpui::MouseDownEvent, window, cx| {
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
					.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
						let levels = s.model_efforts(cx);
						let current = levels.iter().position(|v| *v == s.effort).unwrap_or(0);
						let next = match e.keystroke.key.as_str() {
							"left" | "down" => current.saturating_sub(1),
							"right" | "up" => (current + 1).min(levels.len().saturating_sub(1)),
							"home" => 0,
							"end" => levels.len().saturating_sub(1),
							_ => return,
						};
						if let Some(level) = levels.get(next) {
							s.effort = *level;
						}
						cx.stop_propagation();
						cx.notify();
					}))
					.child(
						canvas(
							move |bounds, _, cx| {
								let _ = measured
									.update(cx, |s, _| s.effort_track_bounds = Some(bounds));
							},
							move |_, _, window, _| {
								let movement = events.clone();
								window.on_mouse_event(
									move |e: &gpui::MouseMoveEvent, phase, _, cx| {
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
												}
											}
										});
									},
								);
								let release = events.clone();
								window.on_mouse_event(move |_: &gpui::MouseUpEvent, _, _, cx| {
									let _ = release.update(cx, |s, _| s.effort_drag = None);
								});
							},
						)
						.absolute()
						.inset_0(),
					)
					.child(SliderTrack { fraction, dragging: self.effort_drag.is_some() }),
			)
			.into_any_element()
	}
}

#[derive(gpui::IntoElement)]
struct SliderTrack {
	fraction: f32,
	dragging: bool,
}
impl gpui::RenderOnce for SliderTrack {
	fn render(self, window: &mut gpui::Window, cx: &mut gpui::App) -> impl IntoElement {
		let fraction = if self.dragging {
			self.fraction
		} else {
			crate::ui_motion::value("reasoning-thumb", self.fraction, window, cx)
		};
		div()
			.absolute()
			.inset_0()
			.child(
				div()
					.absolute()
					.left_0()
					.right_0()
					.top(px(3.))
					.h(px(24.))
					.rounded_full()
					.bg(rgba(0xffffff18)),
			)
			.child(
				div()
					.absolute()
					.left_0()
					.top(px(3.))
					.w(relative(fraction))
					.h(px(24.))
					.rounded_full()
					.bg(rgb(if self.fraction > 0.9 { 0xf06a72 } else { 0x8f9fe8 })),
			)
			.child(
				div()
					.absolute()
					.left(relative(fraction))
					.top(px(1.))
					.ml(px(-14.))
					.size(px(28.))
					.rounded_full()
					.bg(rgb(0xf4f4f8)),
			)
	}
}
#[cfg(test)]
mod tests {
	use super::{index_at, *};
	#[gpui::test]
	fn real_slider_drag_and_outside_dismiss(cx: &mut gpui::TestAppContext) {
		use crate::shell::chief_surface::ConversationReasoningEffort as Effort;
		use decodex_protocol::{ChiefCapabilitiesResult, ChiefModelDto};
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.simulate_resize(gpui::size(px(1400.), px(900.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.capabilities = Some(ChiefCapabilitiesResult::Available {
				models: vec![ChiefModelDto {
					model: decodex_protocol::ConversationModel::new(s.model.read(cx).content())
						.unwrap(),
					name: "Test model".into(),
					efforts: vec![Effort::Low, Effort::High, Effort::Ultra],
					default_effort: Some(Effort::High),
					supports_fast: true,
					supports_images: true,
				}],
				memory_enabled: None,
			});
			s.composer_menu = Some("effort");
			s.composer_menu_content = Some("effort");
		});
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});
		let bounds = surface.update(visual, |s, _| s.effort_track_bounds.unwrap());
		let start = bounds.center();
		visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
		surface.update(visual, |s, _| assert!(s.effort_drag.is_some()));
		visual.simulate_mouse_move(
			gpui::point(bounds.right() + px(30.), start.y),
			MouseButton::Left,
			Default::default(),
		);
		surface.update(visual, |s, _| assert_eq!(s.effort, Effort::Ultra));
		visual.simulate_mouse_move(
			gpui::point(bounds.left() - px(30.), start.y),
			MouseButton::Left,
			Default::default(),
		);
		surface.update(visual, |s, _| assert_eq!(s.effort, Effort::Low));
		visual.simulate_mouse_up(start, MouseButton::Left, Default::default());
		surface.update(visual, |s, _| assert!(s.effort_drag.is_none()));
		visual.simulate_keystrokes("right");
		surface.update(visual, |s, _| assert_eq!(s.effort, Effort::High));
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});
		let model = surface.update(visual, |s, _| s.menu_trigger_bounds["model"].center());
		visual.simulate_mouse_down(model, MouseButton::Left, Default::default());
		visual.simulate_mouse_up(model, MouseButton::Left, Default::default());
		surface.update(visual, |s, _| assert_eq!(s.composer_menu, Some("model")));
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		visual.simulate_mouse_down(
			gpui::point(px(400.), px(200.)),
			MouseButton::Left,
			Default::default(),
		);
		surface.update(visual, |s, _| assert!(s.composer_menu.is_none()));
	}

	#[test]
	fn slider_snaps_and_clamps_to_supported_stops() {
		assert_eq!(index_at(-1., 5), 0);
		assert_eq!(index_at(0.37, 5), 1);
		assert_eq!(index_at(0.38, 5), 2);
		assert_eq!(index_at(1.5, 5), 4);
		assert_eq!(index_at(0.9, 1), 0);
		assert_eq!(index_at(0.9, 0), 0);
	}
}
