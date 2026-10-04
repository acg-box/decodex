//! A small pixel cloud for live work. The clock follows one conversation's active state.
use std::{
	f32::consts::PI,
	sync::{Arc, LazyLock},
	time::Instant,
};

use gpui::{
	self, App, Background, Bounds, Image, ImageFormat, IntoElement, PathBuilder, Pixels,
	RenderOnce, Role, SharedString, Window,
	prelude::{
		InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{ui_motion, ui_theme::TEXT_MUTED};

// Use the approved logo silhouette, including its lightning and cursor cutouts.
static CLOUD: LazyLock<Arc<Image>> = LazyLock::new(|| {
	let svg = include_str!("../../../assets/app-icon/liquid-glass/01-mercury-cloud/AppIcon.icon/Assets/shape-0.svg")
        .replace("<g transform", r##"<defs><linearGradient id="glass" x1="0" y1="0" x2="0.7" y2="1"><stop stop-color="#edf4f6"/><stop offset=".42" stop-color="#c6dde6"/><stop offset=".72" stop-color="#91b8ca"/><stop offset="1" stop-color="#d8e9ed"/></linearGradient></defs><g transform"##)
        .replace(r#"fill="white""#, r##"fill="url(#glass)" stroke="#d2f2ff" stroke-opacity=".45" stroke-width="6""##);
	Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()))
});

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
		let reduced = ui_motion::reduced();
		let visible = elapsed.is_some() && closing.is_none_or(|t| t < 0.35 && !reduced);

		if !visible {
			return gpui::div().hidden().into_any_element();
		}
		if !reduced {
			ui_motion::request_frame(window, cx);
		}

		let time = elapsed.unwrap_or_default();
		let settle = closing.map_or(1., |t| (1. - t / 0.35).clamp(0., 1.));

		gpui::div()
			.id("pixel-work-status")
			.role(Role::Status)
			.aria_label("Working")
			.flex()
			.items_center()
			.gap(gpui::px(8.))
			.h(gpui::px(24. * settle))
			.text_size(gpui::px(12.))
			.line_height(gpui::px(18.))
			.text_color(gpui::rgb(TEXT_MUTED))
			.child(
				gpui::div()
					.relative()
					.w(gpui::px(32.))
					.h(gpui::px(24.))
					.flex_none()
					.child(
						gpui::img(CLOUD.clone())
							.absolute()
							.top_0()
							.left_0()
							.size(gpui::px(24.))
							.opacity(0.9 * settle),
					)
					.child(
						gpui::canvas(
							|_, _, _| (),
							move |bounds, _, window, _| {
								for index in 0..10 {
									let (x, y, edge, alpha) =
										particle(if reduced { 1.4 } else { time }, index);
									let alpha = alpha * settle;
									let b = Bounds::new(
										bounds.origin + gpui::point(gpui::px(x), gpui::px(y)),
										gpui::size(gpui::px(edge), gpui::px(edge)),
									);

									paint_particle(
										window,
										b,
										gpui::linear_gradient(
											155.,
											gpui::linear_color_stop(
												gpui::rgba(0xe8f3f8ff).opacity(alpha),
												0.,
											),
											gpui::linear_color_stop(
												gpui::rgba(0x9ec8d9ff).opacity(alpha * 0.7),
												1.,
											),
										),
									);

									let rim = Bounds::new(
										b.origin,
										gpui::size(b.size.width, gpui::px(0.35)),
									);

									paint_particle(
										window,
										rim,
										gpui::rgba(0xf1fcffff).opacity(alpha * 0.55).into(),
									);
								}
							},
						)
						.absolute()
						.size_full(),
					),
			)
			.child(gpui::div().opacity(settle).child("Working"))
			.into_any_element()
	}
}

#[derive(Default)]
struct Motion {
	turn: Option<String>,
	started: Option<Instant>,
	ended: Option<Instant>,
}

fn smooth(value: f32) -> f32 {
	let t = value.clamp(0., 1.);

	t * t * (3. - 2. * t)
}

// Staggered births hide recycling at zero alpha. Every particle only travels outwards.
fn particle(time: f32, index: usize) -> (f32, f32, f32, f32) {
	let p = (time / 2.8 + index as f32 / 10.).rem_euclid(1.);
	let lane = (index % 4) as f32;
	let seed = ((index * 7) % 11) as f32 / 10.;
	let x = 11.3 + lane * 1.77 + p * (7. + seed * 3.);
	let y = 6.2 + lane * 1.77 - p * (5. + seed * 3.) + (p * PI).sin() * 0.7 * (seed - 0.5);
	let alpha = smooth(p / 0.12) * (1. - smooth((p - 0.55) / 0.45)) * (0.6 + seed * 0.35);

	(x, y, 1.5, alpha)
}

// paint_quad snaps to device pixels. Paths retain subpixel travel and antialiased edges.
fn paint_particle(window: &mut Window, bounds: Bounds<Pixels>, color: Background) {
	let mut path = PathBuilder::fill();

	path.add_polygon(
		&[
			bounds.origin,
			bounds.origin + gpui::point(bounds.size.width, gpui::px(0.)),
			bounds.origin + gpui::point(bounds.size.width, bounds.size.height),
			bounds.origin + gpui::point(gpui::px(0.), bounds.size.height),
		],
		true,
	);

	if let Ok(path) = path.build() {
		window.paint_path(path, color);
	}
}
