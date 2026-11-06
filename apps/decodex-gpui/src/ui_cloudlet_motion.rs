//! Pixel-step poses ported from the approved Cloudlets motion.css.
use std::f32::consts::{PI, TAU};

type Steps = &'static [(f32, f32)];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum State {
	Idle,
	Hopping,
	Working,
	Waiting,
	Done,
	Attention,
	Opening,
	Thinking,
	Reading,
	Writing,
	Inspecting,
	Handoff,
	Still,
}
impl State {
	pub(crate) fn label(self) -> &'static str {
		match self {
			Self::Idle => "Ready",
			Self::Opening | Self::Hopping => "Loading",
			Self::Working => "Working",
			Self::Thinking => "Thinking",
			Self::Reading => "Reading",
			Self::Writing => "Writing",
			Self::Inspecting => "Checking",
			Self::Waiting => "Waiting on work",
			Self::Attention => "Needs you",
			Self::Handoff => "Handing off",
			Self::Done => "Done",
			Self::Still => "Stopped",
		}
	}

	pub(super) fn for_status(status: &str) -> Self {
		match status {
			"Running" => Self::Working,
			"Waiting" | "Waiting on work" | "Queued" | "Starting" | "Busy" | "Paused" =>
				Self::Waiting,
			"Needs you" | "Approval" | "Input needed" => Self::Attention,
			"Error" | "Failed" | "Stopped" | "Cancelled" => Self::Still,
			_ => Self::Idle,
		}
	}

	pub(super) fn pose(self, seconds: f32, held_right: bool) -> Pose {
		let blink = seconds.rem_euclid(5.8);
		let mut pose = Pose {
			blink: (2.05..2.20).contains(&blink) || (2.33..2.42).contains(&blink),
			..Default::default()
		};
		if self.action_pose(seconds, held_right, &mut pose) {
			return pose;
		}
		let Some((period, actor, hands)) = self.cycle(seconds, held_right, &mut pose) else {
			return pose;
		};
		let t = (seconds / period).fract();
		let y = step(t, actor);

		pose.cloud[1] = y;

		let hand_y = step(t, hands);
		let spread = match self {
			Self::Hopping => (0.38..0.64).contains(&t),
			Self::Done => (0.12..0.62).contains(&t),
			_ => false,
		};

		pose.left = [if spread { -1. } else { 0. }, y + hand_y];
		pose.right = [if spread { 1. } else { 0. }, y + hand_y];

		if self == Self::Attention {
			let hand = if held_right { &mut pose.left } else { &mut pose.right };

			hand[1] += step(
				t,
				&[
					(0., 0.),
					(0.12, -1.),
					(0.18, -2.),
					(0.24, -3.),
					(0.3, -2.),
					(0.36, -3.),
					(0.42, -2.),
					(0.48, -1.),
					(0.58, 0.),
				],
			);
			hand[0] = if (0.18..0.24).contains(&t)
				|| (0.3..0.36).contains(&t)
				|| (0.42..0.48).contains(&t)
			{
				if held_right { -1. } else { 1. }
			} else {
				0.
			};
		}

		pose
	}

	fn action_pose(self, seconds: f32, held_right: bool, pose: &mut Pose) -> bool {
		match self {
			Self::Opening => {
				let t = seconds * TAU / 3.8;
				pose.cloud[1] = -0.325 * (1. - t.cos());
				pose.left[1] = pose.cloud[1] - 0.275 * (1. - t.cos());
				pose.right[1] = pose.cloud[1] - 0.275 * (1. + t.cos());
			},
			Self::Thinking => {
				let t = ((seconds + 1.6) / 4.8).rem_euclid(1.);
				let lift =
					linear(t, &[(0., 0.), (0.12, 0.), (0.28, 1.), (0.72, 1.), (0.9, 0.), (1., 0.)]);
				pose.right = [-lift, -3. * lift];
				pose.eyes = [lift, -lift];
			},
			Self::Reading => {
				pose.left = [0.5, -2.];
				pose.cloud[1] = 0.275 * (1. - (seconds * TAU / 4.4).cos());
				pose.eyes = [-1., -0.5];
			},
			Self::Writing => {
				let t = (seconds / 0.9).rem_euclid(1.);
				pose.right = [
					linear(t, &[(0., 0.5), (0.25, 1.25), (0.5, 0.5), (0.75, 1.), (1., 0.5)]),
					linear(t, &[(0., 0.), (0.25, -0.5), (0.5, -0.75), (0.75, 0.25), (1., 0.)]),
				];
				pose.eyes = [1., 0.];
			},
			Self::Inspecting => {
				let t = ((seconds + 1.) / 3.2).rem_euclid(1.);
				let sweep = linear(t, &[(0., 0.), (0.3, 1.), (0.7, 1.), (1., 0.)]);
				pose.left = [sweep, -1. - sweep];
				pose.eyes = [-1., -0.5];
			},
			Self::Handoff => *pose = Self::Working.pose(seconds, held_right),
			Self::Still => pose.blink = false,
			_ => return false,
		}
		true
	}

	fn cycle(self, seconds: f32, held_right: bool, pose: &mut Pose) -> Option<(f32, Steps, Steps)> {
		Some(match self {
			Self::Hopping => (
				0.9,
				&[(0., 0.), (0.24, -1.), (0.36, -2.), (0.56, -1.), (0.7, 0.)],
				&[(0., 0.), (0.22, -1.), (0.38, -2.), (0.64, -1.), (0.78, 0.)],
			),
			Self::Waiting =>
				(3.6, &[(0., 0.), (0.4, 1.), (0.9, 0.)], &[(0., 0.), (0.48, 1.), (0.96, 0.)]),
			Self::Done => (
				2.2,
				&[
					(0., 0.),
					(0.08, -1.),
					(0.12, -2.),
					(0.16, -3.),
					(0.22, -4.),
					(0.29, -3.),
					(0.34, -2.),
					(0.39, -1.),
					(0.44, 0.),
					(0.51, -1.),
					(0.58, 0.),
				],
				&[(0., 0.), (0.12, -1.), (0.2, -2.), (0.48, -1.), (0.7, 0.)],
			),
			Self::Attention => (2.4, &[(0., 0.), (0.14, -1.), (0.56, 0.)], &[]),
			Self::Working => {
				let cloud_t = ((seconds + if held_right { 0.35 } else { 0. }) / 1.4).fract();

				pose.cloud[1] =
					step(cloud_t, &[(0., 0.), (0.18, -1.), (0.32, 0.), (0.68, -1.), (0.82, 0.)]);

				for (right, hand) in [(false, &mut pose.left), (true, &mut pose.right)] {
					let t = ((seconds + if right { 0.35 } else { 0. }) / 0.7).fract();

					hand[0] =
						if (0.42..0.86).contains(&t) { if right { 1. } else { -1. } } else { 0. };
					hand[1] = step(
						t,
						&[
							(0., 0.),
							(0.24, -1.),
							(0.42, if right == held_right { -1. } else { -2. }),
							(0.64, -1.),
							(0.86, 0.),
						],
					);
				}

				return None;
			},
			Self::Idle => {
				let breath = (seconds * TAU / 3.8).sin();

				pose.cloud = [0.16 * (seconds * 0.8).sin(), -0.30 * breath];
				pose.left = [0., -0.40 * (seconds * 1.1).sin()];
				pose.right = [0., -0.40 * (seconds * 1.1 + 0.65).sin()];

				// One small stretch per long cycle; seeds offset each actor.
				let stretch = seconds.rem_euclid(9.4);

				if (5.5..6.2).contains(&stretch) {
					let amount = ((stretch - 5.5) / 0.7 * PI).sin();

					pose.cloud[1] -= 0.65 * amount;

					if held_right {
						pose.left[1] -= 1.2 * amount;
					} else {
						pose.right[1] -= 1.2 * amount;
					}
				}

				return None;
			},
			_ => return None,
		})
	}
}

#[derive(Default)]
pub(super) struct Pose {
	pub cloud: [f32; 2],
	pub eyes: [f32; 2],
	pub blink: bool,
	pub left: [f32; 2],
	pub right: [f32; 2],
}
impl Pose {
	pub(super) fn particle(&self, state: State, seconds: f32, index: usize) -> Particle {
		let moving =
			!matches!(state, State::Idle | State::Waiting | State::Attention | State::Still);

		if !moving {
			return Particle { offset: [0., 0.], size: 1., opacity: 0.65 };
		}

		let period = match state {
			State::Done => 2.2,
			State::Opening | State::Inspecting => 3.2,
			State::Reading => 3.4,
			State::Thinking => 4.2,
			_ => 2.8,
		};
		let t = (seconds / period + index as f32 / 4.).fract();
		let drift = if matches!(state, State::Working | State::Hopping | State::Done) {
			step(t, &[(0., 0.), (0.25, 1.), (0.4, 2.), (0.55, 3.), (0.7, 4.), (0.85, 5.)])
		} else {
			linear(t, &[(0., 0.), (0.1, 0.), (0.85, 5.), (1., 5.)])
		};

		Particle {
			offset: [
				if index == 3 { -6. } else { 0. } + drift,
				if index == 2 {
					-3.
				} else if index == 3 {
					1.
				} else {
					-2.
				} - drift,
			],
			size: if index == 3 || t >= 0.7 { 0.5 } else { 1. },
			opacity: linear(
				t,
				&[(0., 0.), (0.12, 1.), (0.55, 0.85), (0.8, 0.35), (0.94, 0.), (1., 0.)],
			),
		}
	}
}

pub(super) struct Particle {
	pub offset: [f32; 2],
	pub size: f32,
	pub opacity: f32,
}
fn step(t: f32, keys: &[(f32, f32)]) -> f32 {
	keys.iter().rev().find(|(at, _)| t >= *at).map_or(0., |(_, value)| *value)
}
fn linear(t: f32, keys: &[(f32, f32)]) -> f32 {
	let pair = keys
		.windows(2)
		.find(|pair| t <= pair[1].0)
		.expect("particle opacity keyframes cover the complete phase");

	pair[0].1 + (pair[1].1 - pair[0].1) * ((t - pair[0].0) / (pair[1].0 - pair[0].0))
}

#[cfg(test)]
mod tests {
	use super::State;

	#[test]
	fn attention_keeps_the_held_hand_with_the_body_and_waves_the_free_hand() {
		for held_right in [false, true] {
			let pose = State::Attention.pose(2.4 * 0.25, held_right);
			let (held, free) =
				if held_right { (pose.right, pose.left) } else { (pose.left, pose.right) };

			assert_eq!(held, pose.cloud);
			assert!(free[1] < held[1]);
		}
	}

	#[test]
	fn state_motion_stays_bounded_and_particles_fade() {
		for state in [
			State::Idle,
			State::Hopping,
			State::Working,
			State::Waiting,
			State::Done,
			State::Attention,
			State::Opening,
			State::Thinking,
			State::Reading,
			State::Writing,
			State::Inspecting,
			State::Handoff,
			State::Still,
		] {
			for frame in 0..280 {
				let seconds = frame as f32 / 50.;
				let pose = state.pose(seconds, true);

				assert!(pose.cloud[1].abs() <= 4.);

				for index in 0..4 {
					let pixel = pose.particle(state, seconds, index);

					assert!((0.0..=1.0).contains(&pixel.opacity));
				}
			}
		}
	}
}
