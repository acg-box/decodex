//! One source for graph counters and the global activity island.

use std::time::Instant;

use decodex_protocol::{
	AgentThreadMetrics, AgentThreadTokenBreakdown, AgentTimelineContent, AgentTimelineEntry,
	AgentTimelinePage,
};

#[derive(Clone, Copy, Default)]
pub(crate) struct Sample {
	pub tokens: Option<u64>,
	pub breakdown: Option<AgentThreadTokenBreakdown>,
	pub stats: Option<AgentThreadMetrics>,
	pub read_at: Option<Instant>,
}
impl Sample {
	pub(crate) fn from_page(page: &AgentTimelinePage) -> Self {
		let mut sample = Self::from_entries(&page.entries);

		sample.tokens = page.thread_token_total.or(sample.tokens);
		sample.breakdown = page.thread_token_breakdown;
		sample.stats = page.thread_metrics.as_deref().copied();
		sample.read_at = Some(Instant::now());

		sample
	}

	pub(crate) fn from_entries(entries: &[AgentTimelineEntry]) -> Self {
		let tokens = entries.iter().rev().find_map(|entry| match &entry.content {
			AgentTimelineContent::TurnBoundary { usage: Some(usage), .. } =>
				usage.details.as_ref().and_then(|details| details.thread_total),
			_ => None,
		});

		Self { tokens, ..Default::default() }
	}

	fn since_read(self) -> u64 {
		self.read_at.map_or(0, |read| read.elapsed().as_millis().min(u128::from(u64::MAX)) as u64)
	}

	pub(crate) fn time(self) -> Option<u64> {
		let stats = self.stats?;

		Some(stats.completed_ms?.saturating_add(
			stats.active_ms.map_or(0, |active| active.saturating_add(self.since_read())),
		))
	}

	pub(crate) fn active_time(self) -> Option<u64> {
		self.stats?.active_ms.map(|ms| ms.saturating_add(self.since_read()))
	}

	pub(crate) fn rate(self) -> Option<(f64, bool)> {
		let rate = self.stats?.rate?;
		let ms = rate.duration_ms.saturating_add(if !rate.estimated && rate.active {
			self.since_read()
		} else {
			0
		});

		(ms > 0).then(|| (rate.output_tokens as f64 * 1_000. / ms as f64, rate.active))
	}

	pub(crate) fn rate_estimated(self) -> bool {
		self.stats.and_then(|stats| stats.rate).is_some_and(|rate| rate.estimated)
	}

	pub(crate) fn rate_text(self) -> String {
		rate_label(self.rate().map(|(value, _)| value))
	}

	pub(crate) fn rate_caption(self) -> &'static str {
		if self.rate_estimated() { "tok/s" } else { "avg tok/s" }
	}

	pub(crate) fn label(self) -> String {
		format!(
			"Recorded total: {} tokens. Agent time: {}. Throughput: {} {}. {}",
			self.tokens.map_or_else(|| "—".into(), |value| value.to_string()),
			time_label(self.time()),
			self.rate_text(),
			self.rate_caption(),
			if self.rate_estimated() {
				"Output tokens per generation second; excludes tool and user waits. Reasoning counts only when its start is observed."
			} else {
				"Turn average; includes reasoning and tool waits."
			}
		)
	}
}

impl PartialEq for Sample {
	fn eq(&self, other: &Self) -> bool {
		// A new receipt clock alone does not change idle counters or invalidate the island.
		self.tokens == other.tokens
			&& self.breakdown == other.breakdown
			&& self.stats == other.stats
	}
}

#[derive(Clone, PartialEq)]
pub(crate) struct Live {
	pub key: String,
	pub agent: String,
	pub action: String,
	pub sample: Sample,
}

#[derive(Clone, Default, PartialEq)]
pub(crate) struct Summary {
	pub samples: Vec<Sample>,
	pub live: Vec<Live>,
	pub working: usize,
}
impl Summary {
	pub(crate) fn total_tokens(&self) -> Option<u64> {
		if self.samples.is_empty() {
			return None;
		}

		self.samples.iter().try_fold(0_u64, |total, sample| total.checked_add(sample.tokens?))
	}

	/// Sum only complete native observations for the visible agents.
	pub(crate) fn token_breakdown(&self) -> Option<AgentThreadTokenBreakdown> {
		if self.samples.is_empty() {
			return None;
		}

		self.samples.iter().try_fold(AgentThreadTokenBreakdown::default(), |mut total, sample| {
			let value = sample.breakdown?;

			total.input_tokens = total.input_tokens.checked_add(value.input_tokens)?;
			total.cached_input_tokens =
				total.cached_input_tokens.checked_add(value.cached_input_tokens)?;
			total.output_tokens = total.output_tokens.checked_add(value.output_tokens)?;
			total.reasoning_output_tokens =
				total.reasoning_output_tokens.checked_add(value.reasoning_output_tokens)?;

			Some(total)
		})
	}

	/// Combine the latest overlapping work period; never add unrelated historical speeds.
	pub(crate) fn rate(&self) -> Option<(f64, bool)> {
		let estimated = self.rate_estimated();
		let intervals: Vec<_> = self
			.samples
			.iter()
			.filter_map(|s| {
				let rate = s.stats?.rate?;

				if rate.estimated != estimated {
					return None;
				}

				let (value, live) = s.rate()?;
				let end = rate.completed_at_ms.unwrap_or_else(|| {
					rate.started_at_ms
						.saturating_add(rate.duration_ms)
						.saturating_add(s.since_read())
				});

				Some((rate.started_at_ms, end, value, live))
			})
			.collect();
		let latest = intervals.iter().max_by_key(|(_, end, _, _)| *end)?;
		let mut start = latest.0;

		loop {
			let next = intervals
				.iter()
				.filter(|(begin, end, _, _)| *begin <= latest.1 && *end >= start)
				.map(|(begin, ..)| *begin)
				.min()
				.unwrap_or(start);

			if next == start {
				break;
			}

			start = next;
		}

		let group =
			intervals.iter().filter(|(begin, end, _, _)| *begin <= latest.1 && *end >= start);

		Some(
			group.fold((0., false), |(sum, active), (_, _, value, live)| {
				(sum + value, active || *live)
			}),
		)
	}

	pub(crate) fn rate_estimated(&self) -> bool {
		self.samples.iter().any(|sample| sample.rate_estimated())
	}

	pub(crate) fn rate_text(&self) -> String {
		rate_label(self.rate().map(|(value, _)| value))
	}

	pub(crate) fn display_rate_text(&self) -> String {
		if !self.generating() {
			self.rate_text()
		} else {
			// Native output counts arrive after each response. While generating, estimate
			// from each active agent's latest measured generation interval, including a
			// preceding turn. Never divide its output by the current tool/wait clock.
			let rate = self
				.samples
				.iter()
				.filter(|s| s.stats.is_some_and(|v| v.generating))
				.try_fold(0., |sum, sample| {
					let (value, _) = sample.rate()?;

					sample.rate_estimated().then_some(sum + value)
				});

			rate_label(rate)
		}
	}

	pub(crate) fn generating(&self) -> bool {
		self.samples.iter().any(|s| s.stats.is_some_and(|stats| stats.generating))
	}
}

pub(crate) fn time_label(ms: Option<u64>) -> String {
	match ms {
		Some(ms) if ms >= 3_600_000 => format!("{}h {}m", ms / 3_600_000, ms / 60_000 % 60),
		Some(ms) if ms >= 60_000 => format!("{}m {}s", ms / 60_000, ms / 1_000 % 60),
		Some(ms) => format!("{}s", ms / 1_000),
		None => "—".into(),
	}
}

pub(crate) fn rate_label(value: Option<f64>) -> String {
	value.map_or_else(
		|| "—".into(),
		|value| {
			if value >= 1_000. { format!("{:.1}K", value / 1_000.) } else { format!("{value:.1}") }
		},
	)
}

#[cfg(test)]
mod tests {
	use crate::ui_work_metrics::{AgentThreadMetrics, Instant, Sample, Summary};
	use decodex_protocol::{AgentOutputRate, AgentThreadTokenBreakdown, AgentTimelinePage};
	#[test]
	fn recorded_total_preserves_missing_zero_and_cumulative_values() {
		let mut summary = Summary::default();

		assert_eq!(summary.total_tokens(), None);

		summary.samples = vec![Sample { tokens: Some(0), ..Default::default() }];

		assert_eq!(summary.total_tokens(), Some(0));

		summary.samples.push(Sample { tokens: Some(1_250), ..Default::default() });

		assert_eq!(summary.total_tokens(), Some(1_250));

		summary.samples.push(Sample::default());

		assert_eq!(summary.total_tokens(), None);
	}

	#[test]
	fn breakdown_sums_complete_observations_without_inventing_missing_counts() {
		let first = AgentThreadTokenBreakdown {
			input_tokens: 100,
			cached_input_tokens: 60,
			output_tokens: 30,
			reasoning_output_tokens: 10,
		};
		let second = AgentThreadTokenBreakdown {
			input_tokens: 200,
			cached_input_tokens: 40,
			output_tokens: 50,
			reasoning_output_tokens: 20,
		};
		let mut summary = Summary::default();
		let page = AgentTimelinePage {
			thread_id: "thread-1".into(),
			thread_token_total: Some(130),
			thread_token_breakdown: Some(first),
			thread_metrics: None,
			safety_buffering_turn_id: None,
			entries: vec![],
			next_cursor: None,
			weather: Default::default(),
			active_realtime_session_at_page_start: None,
		};

		assert_eq!(summary.token_breakdown(), None);
		summary.samples = vec![
			Sample::from_page(&page),
			Sample { breakdown: Some(second), ..Default::default() },
		];
		assert_eq!(
			summary.token_breakdown(),
			Some(AgentThreadTokenBreakdown {
				input_tokens: 300,
				cached_input_tokens: 100,
				output_tokens: 80,
				reasoning_output_tokens: 30,
			})
		);

		summary.samples.push(Sample::default());
		assert_eq!(summary.token_breakdown(), None);
		summary.samples.pop();
		summary.samples[0].breakdown.as_mut().unwrap().input_tokens = u64::MAX;
		assert_eq!(summary.token_breakdown(), None);
	}

	#[test]
	fn measured_rate_stays_fixed_during_tool_wait_and_global_does_not_mix_turn_averages() {
		let sample = Sample {
			stats: Some(AgentThreadMetrics {
				completed_ms: Some(1_000),
				active_ms: Some(60_000),
				generating: true,
				rate: Some(AgentOutputRate {
					estimated: true,
					active: true,
					output_tokens: 100,
					duration_ms: 1_000,
					started_at_ms: 2_000,
					completed_at_ms: Some(3_000),
				}),
			}),
			read_at: Instant::now().checked_sub(std::time::Duration::from_secs(60)),
			..Default::default()
		};

		assert_eq!(sample.rate(), Some((100., true)));
		assert_eq!(sample.rate_text(), "100.0");

		let mut old = sample;

		old.stats.as_mut().unwrap().generating = false;

		let rate = old.stats.as_mut().unwrap().rate.as_mut().unwrap();

		rate.estimated = false;
		rate.active = false;
		rate.started_at_ms = 1_000;
		rate.completed_at_ms = Some(9_000);

		let summary = Summary { samples: vec![old, sample], working: 1, live: vec![] };

		assert_eq!(summary.rate(), Some((100., true)));
		assert!(summary.rate_estimated());
		assert_eq!(summary.display_rate_text(), "100.0");

		let mut next_turn = summary.clone();

		next_turn.samples[1].stats.as_mut().unwrap().rate.as_mut().unwrap().active = false;

		assert_eq!(
			next_turn.display_rate_text(),
			"100.0",
			"a new stream uses its last measured interval until the next native counter arrives"
		);

		let mut parallel = summary.clone();
		let stats = parallel.samples[0].stats.as_mut().unwrap();

		stats.generating = true;

		let rate = stats.rate.as_mut().unwrap();

		rate.active = true;
		rate.estimated = true;

		assert_eq!(
			parallel.display_rate_text(),
			"200.0",
			"current streams sum across separate prior response intervals"
		);

		parallel.samples[0].stats.as_mut().unwrap().rate = None;

		assert_eq!(
			parallel.display_rate_text(),
			"—",
			"missing live measurements must not look like a complete total"
		);

		let mut idle = summary;

		for sample in &mut idle.samples {
			let stats = sample.stats.as_mut().unwrap();

			stats.generating = false;
			stats.rate.as_mut().unwrap().active = false;
		}

		assert_eq!(idle.display_rate_text(), "100.0");
		assert_eq!(idle.rate_text(), "100.0", "idle retains the most recent measured throughput");
	}
	#[test]
	fn global_last_rate_uses_the_latest_overlapping_batch_and_scales_past_ninety_agents() {
		let sample = |start, end, output| Sample {
			stats: Some(AgentThreadMetrics {
				completed_ms: Some(end - start),
				active_ms: None,
				generating: false,
				rate: Some(AgentOutputRate {
					estimated: true,
					active: false,
					output_tokens: output,
					duration_ms: end - start,
					started_at_ms: start,
					completed_at_ms: Some(end),
				}),
			}),
			..Default::default()
		};
		let mut summary = Summary {
			samples: vec![
				sample(0, 1_000, 9_999),
				sample(2_000, 5_000, 30),
				sample(3_000, 4_000, 20),
			],
			working: 0,
			live: vec![],
		};

		assert_eq!(summary.rate(), Some((30., false)));

		summary.samples.extend((0..100).map(|_| sample(10_000, 11_000, 10)));

		assert_eq!(summary.rate(), Some((1_000., false)));
	}
}
