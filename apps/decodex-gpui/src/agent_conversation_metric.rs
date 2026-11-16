//! Shared compact metric symbols and values for chat history and navigation previews.
use crate::ui_motion::HoverHint as _;
use gpui::{
	Div, IntoElement, RenderOnce, Role, SharedString, Stateful,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

use crate::{shell::agent_surface, ui_theme, ui_time};

pub(super) use crate::ui_time::duration as duration_label;

const ICON_SIZE: f32 = 11.;
const ICON_VALUE_GAP: f32 = 3.;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum StepCount {
	Known(usize),
	#[default]
	Unknown,
	InProgress,
}

pub(super) enum Metric {
	Age(Option<i64>),
	Duration(Option<u64>),
	Steps(StepCount),
	Input(Option<u64>),
	Output(Option<u64>),
	Spend(Option<decodex_protocol::AgentSpendDto>),
	Status(String),
}

impl Metric {
	pub(super) fn element(self) -> Stateful<Div> {
		let (title, glyph) = match &self {
			Self::Age(_) => ("Age", Glyph(br#"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="11"><g fill="none" stroke="white" stroke-width="1" stroke-linecap="round" stroke-linejoin="round"><path d="M1 4a4.25 4.25 0 1 1 .2 3M1 1.5V4h2.5M5.5 3v2.5L7 6.5"/></g></svg>"#)),
			Self::Duration(_) => ("Duration", Glyph(br#"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="11"><g fill="none" stroke="white" stroke-width="1"><circle cx="5.5" cy="5.5" r="4.25"/><path d="M5.5 2.5V5.5L7.5 6.5"/></g></svg>"#)),
			Self::Steps(_) => ("Steps", Glyph(br#"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="11" viewBox="0 0 24 24"><g fill="none" stroke="white" stroke-width="2.18" stroke-linecap="round" stroke-linejoin="round"><path d="M4 16v-2.38C4 11.5 2.97 10.5 3 8c.03-2.72 1.49-6 4.5-6C9.37 2 10 3.8 10 5.5c0 3.11-2 5.66-2 8.68V16a2 2 0 1 1-4 0ZM20 20v-2.38c0-2.12 1.03-3.12 1-5.62-.03-2.72-1.49-6-4.5-6C14.63 6 14 7.8 14 9.5c0 3.11 2 5.66 2 8.68V20a2 2 0 1 0 4 0ZM16 17h4M4 13h4"/></g></svg>"#)),
			Self::Input(_) => ("Input tokens", Glyph(br#"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="11"><path fill="none" stroke="white" stroke-width="1" stroke-linecap="round" stroke-linejoin="round" d="M5.5 9V2M2.5 5l3-3 3 3"/></svg>"#)),
			Self::Output(_) => ("Output tokens", Glyph(br#"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="11"><path fill="none" stroke="white" stroke-width="1" stroke-linecap="round" stroke-linejoin="round" d="M5.5 2v7M2.5 6l3 3 3-3"/></svg>"#)),
			Self::Spend(_) => ("Spend", Glyph(br#"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="11"><g fill="none" stroke="white" stroke-width="1"><ellipse cx="5.5" cy="3" rx="3.5" ry="1.5"/><path d="M2 3v4.5a3.5 1.5 0 0 0 7 0V3M2 5.25a3.5 1.5 0 0 0 7 0"/></g></svg>"#)),
			Self::Status(_) => ("Status", Glyph(br#"<svg xmlns="http://www.w3.org/2000/svg" width="11" height="11"><circle fill="none" stroke="white" stroke-width="1" cx="5.5" cy="5.5" r="3.5"/></svg>"#)),
		};
		let value = self.value();
		ui_theme::metric_text()
			.id(SharedString::from(format!("conversation-metric-{title}")))
			.debug_selector(move || format!("conversation-metric-{title}"))
			.role(Role::Group)
			.aria_label(format!("{title}: {value}"))
			.flex_none()
			.flex()
			.items_center()
			.gap(gpui::px(ICON_VALUE_GAP))
			.whitespace_nowrap()
			.child(glyph)
			.child(value)
	}

	fn value(&self) -> String {
		match self {
			Self::Age(micros) => micros
				.map(|micros| ui_time::relative_time(micros, ui_time::now()))
				.unwrap_or_else(|| "—".into()),
			Self::Duration(ms) => ui_time::optional_duration(*ms),
			Self::Steps(StepCount::Known(count)) => count.to_string(),
			Self::Steps(StepCount::Unknown) => "—".into(),
			Self::Steps(StepCount::InProgress) => "…".into(),
			Self::Input(count) | Self::Output(count) =>
				count.map(agent_surface::compact_tokens).unwrap_or_else(|| "—".into()),
			Self::Status(value) => value.clone(),
			Self::Spend(value) => value.as_ref().map(spend_label).unwrap_or_else(|| "—".into()),
		}
	}
}

pub(super) fn spend_label(spend: &decodex_protocol::AgentSpendDto) -> String {
	format!(
		"{}{}{}",
		if spend.estimated { "≈ " } else { "" },
		super::usage_estimates::currency(spend.usd_micros),
		if spend.partial { "+" } else { "" }
	)
}

/// Compact step value and explanation; the footer owns its column geometry.
#[derive(IntoElement)]
pub(super) struct StepMetric {
	pub count: StepCount,
	pub status: Option<&'static str>,
}

impl RenderOnce for StepMetric {
	fn render(self, _: &mut gpui::Window, _: &mut gpui::App) -> impl IntoElement {
		let metric = Metric::Steps(self.count);
		let explanation = match self.count {
			StepCount::Known(count) => format!("{} recorded.", steps_label(count)),
			StepCount::Unknown => "The step count is unavailable for this record.".into(),
			StepCount::InProgress => "The process is still running.".into(),
		};
		let terminal = match self.status {
			Some("Failed") => " The turn failed.",
			Some("Interrupted") => " The turn was interrupted.",
			_ => "",
		};
		let explanation = format!("{explanation}{terminal}");
		let show_tip = !matches!(self.count, StepCount::Known(_)) || !terminal.is_empty();
		metric
			.element()
			.aria_label(explanation.clone())
			.when(show_tip, |metric| metric.hover_hint(explanation.clone()))
	}
}

#[derive(IntoElement)]
struct Glyph(&'static [u8]);
impl RenderOnce for Glyph {
	fn render(self, window: &mut gpui::Window, _: &mut gpui::App) -> impl IntoElement {
		let ink = window.text_style().color;
		gpui::svg().data(self.0).text_color(ink).size(gpui::px(ICON_SIZE)).flex_none()
	}
}

pub(super) fn steps_label(count: usize) -> String {
	format!("{count} {}", if count == 1 { "step" } else { "steps" })
}

#[cfg(test)]
mod tests {
	use super::{Metric, StepCount};
	#[test]
	fn recorded_zero_counts_remain_distinct_from_missing_metrics() {
		assert_eq!(Metric::Input(None).value(), "—");
		assert_eq!(Metric::Input(Some(0)).value(), "0");
		assert_eq!(Metric::Output(None).value(), "—");
		assert_eq!(Metric::Output(Some(0)).value(), "0");
		assert_eq!(Metric::Duration(None).value(), "—");
		assert_eq!(Metric::Duration(Some(0)).value(), "0s");
		assert_eq!(Metric::Age(None).value(), "—");
		assert_eq!(Metric::Steps(StepCount::Known(0)).value(), "0");
		assert_eq!(Metric::Steps(StepCount::Unknown).value(), "—");
		assert_eq!(Metric::Steps(StepCount::InProgress).value(), "…");
	}

	#[test]
	fn spend_preserves_unknown_zero_estimated_and_partial_amounts() {
		let mut spend = decodex_protocol::AgentSpendDto {
			usd_micros: 0,
			estimated: false,
			partial: false,
			explanation: String::new(),
		};
		assert_eq!(Metric::Spend(None).value(), "—");
		assert_eq!(Metric::Spend(Some(spend.clone())).value(), "$0.00");
		spend.usd_micros = 7_500;
		assert_eq!(Metric::Spend(Some(spend.clone())).value(), "< $0.01");
		spend.usd_micros = 307_838;
		spend.estimated = true;
		assert_eq!(Metric::Spend(Some(spend.clone())).value(), "≈ $0.31");
		spend.partial = true;
		assert_eq!(Metric::Spend(Some(spend)).value(), "≈ $0.31+");
	}
}
