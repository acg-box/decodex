//! A small pixel cloud for live work. The clock belongs to the observed native turn.
use gpui::{prelude::*, *};
use std::{
	sync::{Arc, LazyLock},
	time::Instant,
};

// Use the approved logo silhouette, including its lightning and cursor cutouts.
static CLOUD: LazyLock<Arc<Image>> = LazyLock::new(|| {
	Arc::new(Image::from_bytes(
		ImageFormat::Svg,
		include_bytes!(
			"../../../assets/app-icon/liquid-glass/01-mercury-cloud/AppIcon.icon/Assets/shape-0.svg"
		)
		.to_vec(),
	))
});
// Same grid as scripts/assets/build_liquid_glass_icons.swift.
const CELLS: [(f32, f32, f32); 15] = [
	(0., 0., 0.18),
	(1., 0., 0.43),
	(3., 0., 0.82),
	(6., 0., 0.90),
	(0., 1., 0.10),
	(1., 1., 0.25),
	(2., 1., 0.55),
	(4., 1., 0.69),
	(1., 2., 0.12),
	(2., 2., 0.32),
	(3., 2., 0.62),
	(5., 2., 0.84),
	(2., 3., 0.14),
	(3., 3., 0.38),
	(4., 3., 0.57),
];

fn tile(time: f32, col: f32, row: f32, strength: f32) -> (f32, f32, f32) {
	let wave = (1. - (time * std::f32::consts::TAU / 3.2 - col * 0.32 - row * 0.2).cos()) * 0.5;
	let travel = wave * strength;
	let x = ((444. + col * 64.) * 1.18 - 114.58) * 24. / 1024.;
	let y = ((284. + row * 64.) * 1.18 - 68.56) * 24. / 1024.;
	(x + travel * (2. + col * 0.5), y - travel * (1.5 + (3. - row) * 0.6), travel)
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
					.w(px(28.))
					.h(px(24.))
					.flex_none()
					.child(
						img(CLOUD.clone())
							.absolute()
							.top_0()
							.left_0()
							.size(px(24.))
							.opacity(0.82 * settle),
					)
					.child(
						canvas(
							|_, _, _| (),
							move |bounds, _, window, _| {
								let clock = time - closing.unwrap_or_default();
								for (col, row, fade) in CELLS {
									let (x, y, travel) =
										tile(clock, col, row, if reduced { 0. } else { settle });
									let edge = 1.77 - travel * 0.3;
									let b = Bounds::new(
										bounds.origin + point(px(x), px(y)),
										size(px(edge), px(edge)),
									);
									window.paint_quad(fill(
										b,
										rgba(0xdedeeaff).opacity(
											(1. - fade * 0.7) * (1. - travel * 0.45) * settle,
										),
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
