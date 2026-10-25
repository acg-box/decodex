//! A stationary work signal whose space closes with its opacity after work ends.
use std::{
	f32::consts::PI,
	sync::{Arc, LazyLock},
	time::Instant,
};

use gpui::{
	App, Background, Bounds, Image, ImageFormat, IntoElement, PathBuilder, Pixels, RenderOnce,
	Role, SharedString, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{
	ui_motion,
	ui_theme::{self, TEXT_MUTED},
};

// Keep the approved logo silhouette, including its lightning and cursor cutouts.
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
		let now = Instant::now();
		let state =
			window.use_keyed_state(SharedString::from(self.key), cx, |_, _| Motion::new(now));
		let active = self.turn.is_some();
		let (opacity, moving, elapsed) = state.update(cx, |s, _| s.sample(active, now));
		let reduced = ui_motion::reduced();
		let time = elapsed;

		if moving || (active && !reduced) {
			ui_motion::request_frame(window, cx);
		}

		// Use one transition for both visibility and space. Keep the artwork at
		// its natural size instead of squeezing it as the slot closes.
		gpui::div()
			.id("work-status-slot")
			.debug_selector(|| "work-status-slot".into())
			.relative()
			.h(gpui::px(32. * opacity))
			.flex_none()
			.overflow_hidden()
			.when(opacity > 0., |slot| {
				slot.child(
					gpui::div()
						.id("pixel-work-status")
						.absolute()
						.top(gpui::px(4.))
						.left_0()
						.role(Role::Status)
						.aria_label("Working")
						.flex()
						.items_center()
						.gap(gpui::px(8.))
						.h(gpui::px(24.))
						.opacity(opacity)
						.text_size(gpui::px(12.))
						.line_height(gpui::px(18.))
						.text_color(ui_theme::rgb(TEXT_MUTED))
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
										.opacity(0.9),
								)
								.child(
									gpui::canvas(
										|_, _, _| (),
										move |bounds, _, window, _| {
											for index in 0..10 {
												let (x, y, edge, alpha) = particle(
													if reduced { 1.4 } else { time },
													index,
												);
												let b = Bounds::new(
													bounds.origin
														+ gpui::point(gpui::px(x), gpui::px(y)),
													gpui::size(gpui::px(edge), gpui::px(edge)),
												);

												paint_particle(
													window,
													b,
													gpui::linear_gradient(
														155.,
														gpui::linear_color_stop(
															ui_theme::rgba(0xe8f3f8ff)
																.opacity(alpha),
															0.,
														),
														gpui::linear_color_stop(
															ui_theme::rgba(0x9ec8d9ff)
																.opacity(alpha * 0.7),
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
													ui_theme::rgba(0xf1fcffff)
														.opacity(alpha * 0.55)
														.into(),
												);
											}
										},
									)
									.absolute()
									.size_full(),
								),
						)
						.child("Working"),
				)
			})
	}
}

struct Motion {
	active: bool,
	started: Instant,
	opacity: ui_motion::Tween,
}
impl Motion {
	fn new(now: Instant) -> Self {
		Self { active: false, started: now, opacity: ui_motion::Tween::new(0.) }
	}

	fn sample(&mut self, active: bool, now: Instant) -> (f32, bool, f32) {
		if active && !self.active {
			self.started = now;
		}

		self.active = active;

		self.opacity.target(if active { 1. } else { 0. }, now);

		(
			self.opacity.sample(now),
			self.opacity.moving(now),
			now.duration_since(self.started).as_secs_f32(),
		)
	}
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

#[cfg(test)]
mod tests {
	use std::{thread, time::Duration};

	use crate::ui_working::{Instant, IntoElement, Motion, Window, Working};

	struct Signal(bool);
	impl gpui::Render for Signal {
		fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
			Working { key: "test-work".into(), turn: self.0.then(|| "turn".into()) }
		}
	}

	#[test]
	fn appearance_and_disappearance_share_one_continuous_fade() {
		let now = Instant::now();
		let mut motion = Motion::new(now);

		assert_eq!(motion.sample(false, now).0, 0.);
		assert_eq!(motion.sample(true, now).0, 0.);

		let half = now + Duration::from_millis(100);
		let opacity = motion.sample(true, half).0;

		assert!(opacity > 0. && opacity < 1.);
		assert_eq!(motion.sample(false, half).0, opacity, "an interrupted entrance must not jump");
		assert_eq!(motion.sample(false, now + Duration::from_millis(300)).0, 0.);
	}

	#[gpui::test]
	fn finished_work_releases_its_slot(cx: &mut gpui::TestAppContext) {
		let (signal, visual) = cx.add_window_view(|_, _| Signal(false));

		for (active, height) in [(false, 0.), (true, 32.), (false, 0.)] {
			signal.update(visual, |s, cx| {
				s.0 = active;

				cx.notify();
			});

			visual.update(crate::ui_frame::draw_current_frame);

			thread::sleep(Duration::from_millis(220));

			visual.update(crate::ui_frame::draw_current_frame);

			assert_eq!(
				visual.debug_bounds("work-status-slot").unwrap().size.height,
				gpui::px(height)
			);
		}
	}
}
