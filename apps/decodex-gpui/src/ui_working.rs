//! A quiet, stationary work signal. Visibility never changes the transcript's height.
use std::{
	f32::consts::TAU,
	sync::{Arc, LazyLock},
	time::Instant,
};

use gpui::{
	App, Image, ImageFormat, IntoElement, RenderOnce, Role, SharedString, Window, prelude::*,
};

use crate::{ui_motion, ui_theme::TEXT_MUTED};

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
		if moving || (active && !reduced) {
			ui_motion::request_frame(window, cx);
		}

		// Reserve tail space instead of shrinking a row on completion. Keep 12
		// points below the signal, in addition to the composer's own top gap.
		gpui::div()
			.id("work-status-slot")
			.debug_selector(|| "work-status-slot".into())
			.h(gpui::px(40.))
			.flex_none()
			.pt(gpui::px(4.))
			.pb(gpui::px(12.))
			.when(opacity > 0., |slot| {
				slot.child(
					gpui::div()
						.id("pixel-work-status")
						.role(Role::Status)
						.aria_label("Working")
						.flex()
						.items_center()
						.gap(gpui::px(8.))
						.h(gpui::px(24.))
						.opacity(opacity)
						.text_size(gpui::px(12.))
						.line_height(gpui::px(18.))
						.text_color(gpui::rgb(TEXT_MUTED))
						.child(gpui::img(CLOUD.clone()).size(gpui::px(24.)).opacity(if reduced {
							0.9
						} else {
							0.84 + 0.10 * (elapsed * TAU / 2.6).cos()
						}))
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

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

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

	struct Signal(bool);
	impl gpui::Render for Signal {
		fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
			Working { key: "test-work".into(), turn: self.0.then(|| "turn".into()) }
		}
	}

	#[gpui::test]
	fn status_visibility_never_resizes_its_slot(cx: &mut gpui::TestAppContext) {
		let (signal, visual) = cx.add_window_view(|_, _| Signal(false));
		for active in [false, true, false] {
			signal.update(visual, |s, cx| {
				s.0 = active;
				cx.notify();
			});
			visual.update(|window, cx| window.draw(cx).clear(cx));
			assert_eq!(visual.debug_bounds("work-status-slot").unwrap().size.height, gpui::px(40.));
		}
	}
}
