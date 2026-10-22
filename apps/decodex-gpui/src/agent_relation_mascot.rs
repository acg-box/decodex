//! Native Cloudlets geometry. The approved catalog is the only artwork source.
use super::card;
use crate::ui_motion;
use gpui::{App, IntoElement, Styled as _, Window};
use serde::Deserialize;
use std::{
	collections::BTreeMap,
	sync::{Arc, LazyLock},
	time::Instant,
};

#[path = "agent_relation_mascot_motion.rs"] mod motion;
use motion::State;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
	bodies: BTreeMap<String, Body>,
	palettes: BTreeMap<String, BTreeMap<String, String>>,
	expressions: BTreeMap<String, Part>,
	accessories: BTreeMap<String, Part>,
	props: BTreeMap<String, Part>,
	hand_styles: BTreeMap<String, Part>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Body {
	first_row: usize,
	rows: Vec<[f32; 2]>,
	shade: Vec<[f32; 4]>,
	highlight: Vec<[f32; 4]>,
	particles: Vec<[f32; 4]>,
	anchors: BTreeMap<String, [f32; 2]>,
}
#[derive(Deserialize)]
struct Part {
	rects: Vec<(f32, f32, f32, f32, String)>,
	anchor: Option<String>,
}
static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
	serde_json::from_str(include_str!(concat!(
		env!("CARGO_MANIFEST_DIR"),
		"/../../assets/mascots/cloudlets/catalog.json"
	)))
	.expect("checked-in Cloudlets catalog")
});

#[derive(Debug, PartialEq, Eq)]
struct Traits {
	palette: String,
	expression: String,
	accessory: String,
	prop: String,
	held_right: bool,
}
fn hash(value: &str) -> u32 {
	value.bytes().fold(0x811c_9dc5, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193))
}
fn pick<T>(seed: &str, category: &str, choices: &BTreeMap<String, T>) -> String {
	choices
		.keys()
		.max_by_key(|id| hash(&format!("{seed}:{category}:{id}")))
		.expect("catalog choices")
		.clone()
}
impl Traits {
	fn for_seed(seed: &str) -> Self {
		Self {
			palette: pick(seed, "palette", &CATALOG.palettes),
			expression: pick(seed, "expression", &CATALOG.expressions),
			accessory: pick(seed, "accessory", &CATALOG.accessories),
			prop: pick(seed, "prop", &CATALOG.props),
			held_right: hash(&format!("{seed}:held-side")) % 2 == 1,
		}
	}
}

struct Pixel {
	rect: [f32; 4],
	color: u32,
}
struct Drawing {
	cloud: Vec<Pixel>,
	left: Vec<Pixel>,
	right: Vec<Pixel>,
	particles: Vec<Pixel>,
	held_right: bool,
}
impl Drawing {
	fn new(seed: &str) -> Self {
		let traits = Traits::for_seed(seed);
		let body = &CATALOG.bodies["original"];
		let palette = &CATALOG.palettes[&traits.palette];
		let color = |fill: &str| {
			let hex = palette.get(fill).map_or(fill, String::as_str);
			u32::from_str_radix(hex.trim_start_matches('#'), 16).expect("catalog color")
		};
		let pixels = |part: &Part, origin: [f32; 2]| -> Vec<Pixel> {
			part.rects
				.iter()
				.map(|(x, y, w, h, fill)| Pixel {
					rect: [origin[0] + x, origin[1] + y, *w, *h],
					color: color(fill),
				})
				.collect()
		};
		let mut cloud: Vec<_> = body
			.rows
			.iter()
			.enumerate()
			.map(|(row, [x, width])| Pixel {
				rect: [*x, (body.first_row + row) as f32, *width, 1.],
				color: color("base"),
			})
			.collect();
		// Clip the shade and highlight to the same silhouette as the source SVG.
		for (rects, fill) in [(&body.shade, "shade"), (&body.highlight, "light")] {
			for [x, y, width, height] in rects {
				for (row, [body_x, body_width]) in body.rows.iter().enumerate() {
					let row_y = (body.first_row + row) as f32;
					let left = x.max(*body_x);
					let top = y.max(row_y);
					let right = (x + width).min(body_x + body_width);
					let bottom = (y + height).min(row_y + 1.);
					if right > left && bottom > top {
						cloud.push(Pixel {
							rect: [left, top, right - left, bottom - top],
							color: color(fill),
						});
					}
				}
			}
		}
		cloud.extend(pixels(&CATALOG.expressions[&traits.expression], body.anchors["face"]));
		let accessory = &CATALOG.accessories[&traits.accessory];
		cloud.extend(pixels(
			accessory,
			body.anchors[accessory.anchor.as_deref().unwrap_or("accessory")],
		));
		let hand = |side: &str, held: bool| {
			let origin = body.anchors[side];
			let mut result = pixels(&CATALOG.hand_styles["floating"], origin);
			if held {
				result.extend(pixels(&CATALOG.props[&traits.prop], [origin[0], origin[1] - 1.]));
			}
			result
		};
		Self {
			cloud,
			left: hand("leftHand", !traits.held_right),
			right: hand("rightHand", traits.held_right),
			particles: body
				.particles
				.iter()
				.map(|rect| Pixel { rect: *rect, color: color("light") })
				.collect(),
			held_right: traits.held_right,
		}
	}
}

/// One actor per stable agent identity; repeated reads do not restart its motion.
pub(super) struct Actor {
	drawing: Arc<Drawing>,
	state: State,
	changed: Instant,
	arrival: bool,
}
impl Actor {
	pub(super) fn new(seed: &str, status: &str, now: Instant) -> Self {
		Self {
			drawing: Arc::new(Drawing::new(seed)),
			state: State::for_status(status),
			changed: now,
			arrival: false,
		}
	}

	pub(super) fn frame(&mut self, status: &str, now: Instant) -> Avatar {
		let state = State::for_status(status);
		if self.state == State::Done && state == State::Idle {
			if now.duration_since(self.changed).as_secs_f32() >= 2.2 {
				self.state = State::Idle;
			}
		} else if state != self.state {
			self.arrival = state == State::Working;
			let completed =
				self.state == State::Working && matches!(status, "Idle" | "Marked complete");
			self.state = if completed { State::Done } else { state };
			self.changed = now;
		}
		let seconds = now.saturating_duration_since(self.changed).as_secs_f32();
		let animation = if self.arrival && self.state == State::Working && seconds < 0.9 {
			State::Hopping
		} else {
			self.state
		};
		Avatar { drawing: self.drawing.clone(), state: animation, seconds }
	}
}

pub(super) struct Avatar {
	drawing: Arc<Drawing>,
	state: State,
	seconds: f32,
}
impl Avatar {
	pub(super) fn element(self, layout: card::Layout) -> impl IntoElement {
		let state = if ui_motion::reduced() { State::Idle } else { self.state };
		let pose = state.pose(self.seconds, self.drawing.held_right);
		gpui::canvas(
			|_, _, _| (),
			move |bounds, _, window: &mut Window, cx: &mut App| {
				if bounds.intersect(&window.content_mask().bounds).is_empty() {
					return;
				}
				if state != State::Idle {
					ui_motion::request_frame(window, cx);
				}
				let scale = f32::from(bounds.size.width) / 40.;
				let density = window.scale_factor();
				let snap = |value: f32| (value * density).round() / density;
				let mut paint = |pixel: &Pixel, offset: [f32; 2], size: f32, opacity: f32| {
					let [x, y, w, h] = pixel.rect;
					let left = snap(
						f32::from(bounds.left()) + (x + offset[0] + w * (1. - size) / 2.) * scale,
					);
					let top = snap(
						f32::from(bounds.top()) + (y + offset[1] + h * (1. - size) / 2.) * scale,
					);
					let right = snap(
						f32::from(bounds.left()) + (x + offset[0] + w * (1. + size) / 2.) * scale,
					);
					let bottom = snap(
						f32::from(bounds.top()) + (y + offset[1] + h * (1. + size) / 2.) * scale,
					);
					window.paint_quad(gpui::fill(
						gpui::Bounds::new(
							gpui::point(gpui::px(left), gpui::px(top)),
							gpui::size(gpui::px(right - left), gpui::px(bottom - top)),
						),
						gpui::rgb(pixel.color).opacity(opacity),
					));
				};
				for (index, pixel) in self.drawing.particles.iter().enumerate() {
					let particle = pose.particle(state, self.seconds, index);
					paint(pixel, particle.offset, particle.size, particle.opacity);
				}
				for (pixels, offset) in [
					(&self.drawing.cloud, pose.cloud),
					(&self.drawing.left, pose.left),
					(&self.drawing.right, pose.right),
				] {
					for pixel in pixels {
						paint(pixel, offset, 1., 1.);
					}
				}
			},
		)
		.w(gpui::px(layout.spacing(card::PORTRAIT_WIDTH)))
		.h(gpui::px(layout.spacing(card::PORTRAIT_HEIGHT)))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn artwork_is_finite_and_identity_is_stable() {
		for seed in ["main", "release", "native:thread-1", "专项负责人", "A1234"] {
			assert_eq!(Traits::for_seed(seed), Traits::for_seed(seed));
			let drawing = Drawing::new(seed);
			for pixel in drawing
				.cloud
				.iter()
				.chain(&drawing.left)
				.chain(&drawing.right)
				.chain(&drawing.particles)
			{
				assert!(pixel.rect.iter().all(|value| value.is_finite()));
				assert!(pixel.rect[2] > 0. && pixel.rect[3] > 0.);
			}
		}
		assert_ne!(Traits::for_seed("main"), Traits::for_seed("release"));
	}
	#[test]
	fn source_seed_mapping_matches_the_asset_generator() {
		assert_eq!(hash("hello"), 0x4f9f2cab);
		assert_eq!(
			Traits::for_seed("main"),
			Traits {
				palette: "butter".into(),
				expression: "wink".into(),
				accessory: "leaf-pin".into(),
				prop: "paintbrush".into(),
				held_right: true
			}
		);
		assert_eq!(
			Traits::for_seed("专项负责人"),
			Traits {
				palette: "lilac".into(),
				expression: "dot".into(),
				accessory: "headset".into(),
				prop: "camera".into(),
				held_right: false
			}
		);
	}
	#[test]
	fn completion_plays_once_and_repeated_reads_do_not_restart_it() {
		use std::time::Duration;
		let now = Instant::now();
		let mut actor = Actor::new("main", "Running", now);
		let later = |ms| now + Duration::from_millis(ms);
		assert_eq!(actor.frame("Running", later(200)).state, State::Working);
		assert_eq!(actor.frame("Idle", later(300)).state, State::Done);
		assert_eq!(actor.frame("Idle", later(900)).state, State::Done);
		assert_eq!(actor.frame("Idle", later(2600)).state, State::Idle);
		assert_eq!(actor.frame("Idle", later(2800)).state, State::Idle);
		assert_eq!(actor.frame("Running", later(3000)).state, State::Hopping);
		assert_eq!(actor.frame("Running", later(4000)).state, State::Working);
	}

	#[test]
	fn lost_execution_status_does_not_celebrate_a_completion() {
		let now = Instant::now();
		for status in ["Not running", "Status unavailable", "Unknown", "Archived", "Error"] {
			let mut actor = Actor::new("main", "Running", now);
			assert_ne!(actor.frame(status, now).state, State::Done);
		}
	}
}
