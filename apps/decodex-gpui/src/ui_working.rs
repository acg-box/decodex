//! A small pixel cloud for live work. The clock belongs to the observed native turn.
use gpui::{prelude::*, *};
use std::{
	sync::{Arc, LazyLock},
	time::Instant,
};

// Use the approved logo silhouette, including its lightning and cursor cutouts.
static CLOUD: LazyLock<Arc<Image>> = LazyLock::new(|| {
	let svg = include_str!("../../../assets/app-icon/liquid-glass/01-mercury-cloud/AppIcon.icon/Assets/shape-0.svg")
        .replace("<g transform", r##"<defs><linearGradient id="glass" x1="0" y1="0" x2="0.7" y2="1"><stop stop-color="#eefbff"/><stop offset=".42" stop-color="#a2ddf4"/><stop offset=".72" stop-color="#64b8dc"/><stop offset="1" stop-color="#b9ecfc"/></linearGradient></defs><g transform"##)
        .replace(r#"fill="white""#, r##"fill="url(#glass)" stroke="#d2f2ff" stroke-opacity=".45" stroke-width="6""##);
	Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()))
});

fn smooth(value: f32) -> f32 {
	let t = value.clamp(0., 1.);
	t * t * (3. - 2. * t)
}

// Staggered births hide recycling at zero alpha. Every particle only travels outwards.
fn particle(time: f32, index: usize) -> (f32, f32, f32, f32) {
	let p = (time / 2.8 + index as f32 / 18.).rem_euclid(1.);
	let lane = (index % 4) as f32;
	let seed = ((index * 7) % 11) as f32 / 10.;
	let x = 11.3 + lane * 1.77 + p * (7. + seed * 3.);
	let y = 6.2 + lane * 1.77 - p * (5. + seed * 3.)
		+ (p * std::f32::consts::PI).sin() * 0.7 * (seed - 0.5);
	let alpha = smooth(p / 0.12) * (1. - smooth((p - 0.55) / 0.45)) * (0.6 + seed * 0.35);
	(x, y, 1.75 - p * 0.8, alpha)
}

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
				div()
					.relative()
					.w(px(32.))
					.h(px(24.))
					.flex_none()
					.child(
						img(CLOUD.clone())
							.absolute()
							.top_0()
							.left_0()
							.size(px(24.))
							.opacity(0.9 * settle),
					)
					.child(
						canvas(
							|_, _, _| (),
							move |bounds, _, window, _| {
								for index in 0..18 {
									let (x, y, edge, alpha) =
										particle(if reduced { 1.4 } else { time }, index);
									let alpha = alpha * settle;
									let b = Bounds::new(
										bounds.origin + point(px(x), px(y)),
										size(px(edge), px(edge)),
									);
									window.paint_quad(fill(
										b,
										linear_gradient(
											155.,
											linear_color_stop(rgba(0xdff7ffff).opacity(alpha), 0.),
											linear_color_stop(
												rgba(0x6fc5ecff).opacity(alpha * 0.7),
												1.,
											),
										),
									));
									let rim = Bounds::new(b.origin, size(b.size.width, px(0.35)));
									window.paint_quad(fill(
										rim,
										rgba(0xf1fcffff).opacity(alpha * 0.55),
									));
								}
							},
						)
						.absolute()
						.size_full(),
					),
			)
			.child(div().opacity(settle).child("Working"))
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::particle;
	#[test]
	fn particles_fade_before_recycling_and_move_outwards() {
		for i in 0..18 {
			let birth = (1. - i as f32 / 18.) * 2.8;
			let before = particle(birth - 0.0001, i);
			let after = particle(birth + 0.0001, i);
			assert!(before.3 < 0.001 && after.3 < 0.001);
			let first = particle(birth + 0.3, i);
			let later = particle(birth + 1.5, i);
			assert!(later.0 > first.0 && later.1 < first.1);
		}
	}
}
