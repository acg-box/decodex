//! A thin allocation line uses recorded per-agent counters, never per-turn estimates.
use std::{cell::Cell, mem, rc::Rc, time::Instant};

use gpui::{
	AnyElement, Bounds, FontWeight, KeyDownEvent, MouseDownEvent, Pixels, Role, SharedString,
	prelude::{
		FluentBuilder, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
		Styled,
	},
};

use crate::{
	shell::{
		agent_surface::{
			work_board::relations::{
				AgentSurface, Context, Row,
				allocation::{Mark, Strip},
				card::SECONDARY_TEXT,
			},
			workspace,
		},
		workspace_symbols::{self, Symbol},
	},
	ui_motion::{self, SmoothControl},
	ui_theme::{
		self, CAPTION_LINE_HEIGHT, CAPTION_SIZE, TEXT,
		foreground::{METRIC_SIZE, PRIMARY},
	},
	ui_work_metrics::{self, Sample},
};

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) enum Metric {
	#[default]
	Tokens,
	Time,
}
impl Metric {
	fn label(self) -> &'static str {
		match self {
			Self::Tokens => "Tokens",
			Self::Time => "Time",
		}
	}

	fn value(self, sample: Sample) -> Option<u64> {
		match self {
			Self::Tokens => sample.tokens,
			Self::Time => sample.time(),
		}
	}

	fn value_label(self, value: Option<u64>) -> String {
		match (self, value) {
			(_, None) => "Unavailable".into(),
			(Self::Tokens, Some(tokens)) =>
				format!("{tokens} recorded tokens, including cached input"),
			(Self::Time, Some(ms)) =>
				format!("{} agent time", ui_work_metrics::time_label(Some(ms))),
		}
	}
}

struct Entry {
	row: Row,
	slot: Option<u32>,
	value: Option<u64>,
}

impl AgentSurface {
	fn relation_usage_entries(&self) -> Vec<Entry> {
		let mut entries: Vec<_> = self
			.factory_rows()
			.into_iter()
			.map(|row| {
				let sample = self
					.work_board
					.usage
					.as_ref()
					.and_then(|usage| usage.get(&row.key))
					.filter(|(thread, _)| *thread == row.thread)
					.map(|(_, sample)| *sample)
					.unwrap_or_default();
				let slot =
					self.snapshot.as_ref().and_then(|s| s.agent_slots.get(&row.key)).copied();

				Entry { slot, value: self.work_board.view.usage_metric.value(sample), row }
			})
			.collect();

		entries.sort_by(|a, b| {
			b.value.cmp(&a.value).then_with(|| {
				(a.slot.unwrap_or(u32::MAX), &a.row.key)
					.cmp(&(b.slot.unwrap_or(u32::MAX), &b.row.key))
			})
		});

		entries
	}

	pub(super) fn relation_usage_options(&self, cx: &mut Context<Self>) -> AnyElement {
		let open = self.work_board.view.usage_menu;
		let trigger_bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
		let measured = trigger_bounds.clone();
		let mut anchor = gpui::div()
			.relative()
			.child(
				gpui::canvas(move |bounds, _, _| measured.set(Some(bounds)), |_, _, _, _| {})
					.absolute()
					.inset_0(),
			)
			.child(self.graph_control(
				"usage",
				workspace_symbols::icon_sized(Symbol::AllWork, 14.),
				"Usage distribution",
				|s, cx| {
					s.work_board.view.usage_menu = !s.work_board.view.usage_menu;

					cx.notify();
				},
				cx,
			));

		let owner = cx.entity().downgrade();
		let dismiss = owner.clone();
		let outside = trigger_bounds.clone();
		anchor = anchor.child(
			gpui::deferred(
				gpui::div()
					.absolute()
					.top(gpui::px(26.))
					.right(gpui::px(-8.))
					.w(gpui::px(184.))
					.child(
						ui_motion::popover_lazy("graph-usage-menu-motion", open, move |_, cx| {
							owner
								.update(cx, |s, cx| s.relation_usage_menu(outside.clone(), cx))
								.unwrap_or_else(|_| gpui::div().into_any_element())
						})
						.trigger(trigger_bounds)
						.on_dismiss(move |cx| {
							let _ = dismiss.update(cx, |s, cx| {
								s.work_board.view.usage_menu = false;
								cx.notify();
							});
						}),
					),
			)
			.priority(3),
		);

		anchor.into_any_element()
	}

	fn relation_usage_menu(
		&self,
		trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
		cx: &mut Context<Self>,
	) -> AnyElement {
		gpui::div()
			.id("graph-usage-menu")
			.debug_selector(|| "graph-usage-menu".into())
			.role(Role::Group)
			.aria_label("Usage distribution options")
			.occlude()
			.p(gpui::px(ui_theme::POPOVER_CONTENT_PADDING))
			.flex()
			.flex_col()
			.on_mouse_down_out(cx.listener(move |s, event: &MouseDownEvent, window, cx| {
				if !trigger_bounds.get().is_some_and(|bounds| bounds.contains(&event.position)) {
					s.work_board.view.usage_menu = false;

					workspace::restore_shell_focus(window, cx);

					cx.notify();
				}
			}))
			.child(self.relation_usage_control(Metric::Tokens, cx))
			.child(self.relation_usage_control(Metric::Time, cx))
			.child(ui_theme::menu_separator())
			.child(
				ui_theme::menu_row("graph-usage-visibility", true)
					.debug_selector(|| "graph-usage-visibility".into())
					.flex()
					.items_center()
					.child(if self.work_board.view.usage_hidden {
						"Show distribution"
					} else {
						"Hide distribution"
					})
					.on_click(cx.listener(|s, _, _, cx| s.toggle_relation_usage_visibility(cx)))
					.on_key_down(cx.listener(|s, event: &KeyDownEvent, _, cx| {
						if !event.is_held
							&& ["enter", "space"].contains(&event.keystroke.key.as_str())
						{
							s.toggle_relation_usage_visibility(cx);
							cx.stop_propagation();
						}
					}))
					.smooth(),
			)
			.into_any_element()
	}

	fn toggle_relation_usage_visibility(&mut self, cx: &mut Context<Self>) {
		self.work_board.view.usage_hidden = !self.work_board.view.usage_hidden;
		self.work_board.view.usage_menu = false;
		cx.notify();
	}

	pub(in super::super::super) fn dismiss_relation_usage_options(
		&mut self,
		cx: &mut Context<Self>,
	) -> bool {
		if !mem::take(&mut self.work_board.view.usage_menu) {
			return false;
		}

		cx.notify();

		true
	}

	fn relation_usage_control(&self, metric: Metric, cx: &mut Context<Self>) -> AnyElement {
		let active = metric == self.work_board.view.usage_metric;
		let name = match metric {
			Metric::Tokens => "graph-usage-tokens",
			Metric::Time => "graph-usage-time",
		};

		ui_theme::menu_row(name, true)
			.debug_selector(move || name.into())
			.role(Role::Button)
			.occlude()
			.aria_selected(active)
			.aria_label(format!("Compare agent {}", metric.label().to_lowercase()))
			.flex()
			.items_center()
			.justify_between()
			.gap(gpui::px(8.))
			.on_click(cx.listener(move |s, _, window, cx| {
				s.set_relation_usage_metric(metric, cx);

				workspace::restore_shell_focus(window, cx);
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, window, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.set_relation_usage_metric(metric, cx);

					workspace::restore_shell_focus(window, cx);

					cx.stop_propagation();
				}
			}))
			.child(if metric == Metric::Time { "Agent time" } else { metric.label() })
			.child(workspace_symbols::menu_check(active))
			.smooth()
			.into_any_element()
	}

	fn set_relation_usage_metric(&mut self, metric: Metric, cx: &mut Context<Self>) {
		self.work_board.view.usage_metric = metric;
		self.work_board.view.usage_hidden = false;
		self.work_board.view.usage_menu = false;

		cx.stop_propagation();
		cx.notify();
	}

	pub(super) fn relation_usage(&self, cx: &mut Context<Self>) -> AnyElement {
		let entries = self.relation_usage_entries();

		if entries.is_empty() {
			return gpui::div().into_any_element();
		}

		let metric = self.work_board.view.usage_metric;
		let total = entries.iter().filter_map(|entry| entry.value).map(u128::from).sum::<u128>();
		let known = entries.iter().any(|entry| entry.value.is_some());
		let entries: Vec<_> =
			entries.into_iter().filter(|entry| entry.value.is_some_and(|v| v > 0)).collect();
		let width = (self.work_board.view.viewport.0 - 24.).max(0.);
		let gap = (width / entries.len().max(1) as f32 * 0.15).min(4.);
		let available = (width - entries.len().saturating_sub(1) as f32 * gap).max(0.);
		let segments = entries
			.into_iter()
			.map(|entry| {
				let share = entry.value.unwrap_or(0) as f64 / total as f64;
				let width = available * share as f32;

				self.relation_usage_segment(entry, metric, share, width, cx)
			})
			.collect();
		let allocation = Strip { segments, width, gap };

		gpui::div()
			.id("graph-usage-footer")
			.debug_selector(|| "graph-usage-footer".into())
			.font_weight(FontWeight::NORMAL)
			.role(Role::Group)
			.aria_label(format!("Agent {} allocation", metric.label().to_lowercase()))
			.w_full()
			.h(gpui::px(32.))
			.flex_none()
			.px_3()
			.pt(gpui::px(3.))
			.pb(gpui::px(6.))
			.flex()
			.items_end()
			.when(total == 0, |footer| {
				footer.child(
					gpui::div()
						.text_size(gpui::px(CAPTION_SIZE))
						.text_color(ui_theme::rgb(SECONDARY_TEXT))
						.child(if known { "No recorded usage" } else { "Usage unavailable" }),
				)
			})
			.when(total > 0, |footer| footer.child(allocation))
			.into_any_element()
	}

	fn relation_usage_segment(
		&self,
		entry: Entry,
		metric: Metric,
		share: f64,
		width: f32,
		cx: &mut Context<Self>,
	) -> super::allocation::Segment {
		let selected = self.work_board.view.picked_agent.as_ref() == Some(&entry.row.key);
		let hovered =
			self.work_board.view.hover.current(Instant::now()) == Some(entry.row.key.as_str());
		let number = self.relation_agent_number(&entry.row);
		let percent = if share < 0.01 { "<1%".into() } else { format!("{:.0}%", share * 100.) };
		let description = format!(
			"{number} · {}\n{} · {percent} of recorded {}",
			self.relation_agent_name(&entry.row),
			metric.value_label(entry.value),
			if metric == Metric::Time { "agent time" } else { "tokens" }
		);
		let key = entry.row.key;
		let motion_key = key.clone();
		let keyboard_key = key.clone();
		let hover_key = key.clone();
		let name = format!("graph-usage-segment-{key}");
		let child = gpui::div()
			.id(SharedString::from(name.clone()))
			.debug_selector(move || name.clone())
			.role(Role::Button)
			.occlude()
			.aria_selected(selected)
			.aria_label(description)
			.tab_index(0)
			.min_w_0()
			.h_full()
			.flex()
			.flex_col()
			.justify_end()
			.gap(gpui::px(4.))
			.cursor_pointer()
			.on_hover(cx.listener(move |s, inside: &bool, _, cx| {
				if s.work_board.view.hover.allocation(&hover_key, *inside, Instant::now()) {
					cx.notify();
				}
			}))
			.on_click(cx.listener(move |s, _, _, cx| s.select_workbench_agent(&key, cx)))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.select_workbench_agent(&keyboard_key, cx);
					cx.stop_propagation();
				}
			}))
			.child(
				gpui::div()
					.w_full()
					.h(gpui::px(CAPTION_LINE_HEIGHT))
					.min_w_0()
					.flex()
					.items_baseline()
					.gap(gpui::px(6.))
					.pr(gpui::px(8.))
					.overflow_hidden()
					.whitespace_nowrap()
					.text_size(gpui::px(METRIC_SIZE))
					.line_height(gpui::px(CAPTION_LINE_HEIGHT))
					.font_weight(FontWeight::MEDIUM)
					.text_color(ui_theme::rgb(if selected || hovered {
						TEXT
					} else {
						SECONDARY_TEXT
					}))
					.when(width >= 22., |label| label.child(number))
					.when(width >= 48., |label| {
						label.child(gpui::div().text_color(ui_theme::rgb(PRIMARY)).child(percent))
					}),
			)
			.child(Mark { key: motion_key, selected, hovered });

		super::allocation::Segment { share: share as f32, child }
	}
}

#[cfg(test)]
#[path = "agent_relation_usage_tests.rs"]
mod tests;
