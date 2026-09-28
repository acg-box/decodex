//! A small pixel cloud for live work. The clock belongs to the observed native turn.
use gpui::{prelude::*, *};
use std::time::Instant;

#[derive(Default)]
struct Motion {
	turn: Option<String>,
	started: Option<Instant>,
	ended: Option<Instant>,
}

#[derive(IntoElement)]
pub(crate) struct Working {
	pub key: String,
	pub turn: Option<String>,
}

impl RenderOnce for Working {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state =
			window.use_keyed_state(SharedString::from(self.key), cx, |_, _| Motion::default());
		let now = Instant::now();
		let (elapsed, closing) = state.update(cx, |s, _| {
			if s.turn != self.turn {
				if self.turn.is_some() {
					s.started = Some(now);
					s.ended = None;
				} else if s.started.is_some() {
					s.ended = Some(now);
				}
				s.turn = self.turn.clone();
			}
			(
				s.started.map(|t| now.duration_since(t).as_secs_f32()),
				s.ended.map(|t| now.duration_since(t).as_secs_f32()),
			)
		});
		let reduced = crate::ui_motion::reduced();
		let visible = elapsed.is_some() && closing.is_none_or(|t| t < 0.35 && !reduced);
		if !visible {
			return div().into_any_element();
		}
		if !reduced {
			crate::ui_motion::request_frame(window, cx);
		}
		let time = elapsed.unwrap_or_default();
		let settle = closing.map_or(1., |t| (1. - t / 0.35).clamp(0., 1.));
		div()
			.id("pixel-work-status")
			.role(Role::Status)
			.aria_label("Working")
			.flex()
			.items_center()
			.gap(px(8.))
			.h(px(24. * settle))
			.text_size(px(12.))
			.line_height(px(18.))
			.text_color(rgb(crate::ui_theme::TEXT_MUTED))
			.child(
				canvas(
					|_, _, _| (),
					move |bounds, _, window, _| {
						// A stable cloud silhouette, with its right edge dissolving into square
						// cells.
						let cells = [
							(1., 4.),
							(1., 5.),
							(2., 3.),
							(2., 4.),
							(2., 5.),
							(3., 2.),
							(3., 3.),
							(3., 4.),
							(3., 5.),
							(4., 2.),
							(4., 3.),
							(4., 4.),
							(4., 5.),
							(5., 3.),
							(5., 4.),
							(5., 5.),
							(6., 4.),
							(6., 5.),
						];
						for (x, y) in cells {
							let b = Bounds::new(
								bounds.origin + point(px(x * 1.7), px(y * 1.7)),
								size(px(1.8), px(1.8)),
							);
							window.paint_quad(fill(b, rgba(0xd5d6dda0).opacity(settle)));
						}
						for i in 0..6 {
							let phase = time * 1.65 + i as f32 * 0.9;
							let drift =
								if reduced { 0.25 } else { (1. - phase.cos()) * 0.5 * settle };
							let x = 9. + (i % 3) as f32 * 2. + drift * 2.4;
							let y = 3. + (i / 3) as f32 * 3. - drift * 1.8;
							let b = Bounds::new(
								bounds.origin + point(px(x), px(y)),
								size(px(1.5), px(1.5)),
							);
							window.paint_quad(fill(
								b,
								rgba(0xe4e5ecff).opacity((0.8 - drift * 0.5) * settle),
							));
						}
					},
				)
				.size(px(18.))
				.flex_none(),
			)
			.child(div().opacity(settle).child("Working"))
			.into_any_element()
	}
}
