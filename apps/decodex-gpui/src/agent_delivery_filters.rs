//! Compact agent selection and explicit local-time bounds for the task timeline.
use super::{Panel, ui_theme};
use crate::{
	shell::workspace_symbols::{self, Symbol},
	ui_motion::{self, SmoothControl},
};
use gpui::{
	Context, IntoElement, KeyDownEvent, ScrollHandle,
	prelude::{
		FluentBuilder, InteractiveElement, ParentElement, StatefulInteractiveElement, Styled,
	},
};
use std::{cell::Cell, rc::Rc};

#[path = "agent_delivery_slider.rs"] mod slider;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AgentChoice {
	pub id: String,
	pub label: String,
	pub slot: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct TimeRange {
	pub after: Option<i64>,
	pub before: Option<i64>,
}

#[derive(Default)]
pub(super) struct State {
	pub agents: Vec<AgentChoice>,
	pub agent: Option<String>,
	pub range: TimeRange,
	pub menu: bool,
	pub time_menu: bool,
	pub extent: Option<decodex_protocol::AgentDeliveryExtent>,
	period: slider::Period,
	slider: Option<slider::Slider>,
	time_focus: Option<[gpui::FocusHandle; 2]>,
	agent_scroll: ScrollHandle,
	controls: Rc<Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
}

impl State {
	pub(super) fn receive_extent(
		&mut self,
		extent: Option<decodex_protocol::AgentDeliveryExtent>,
	) -> bool {
		let changed = self.extent != extent;
		if self.slider.as_ref().is_none_or(|s| !s.dragging()) && (changed || self.slider.is_none())
		{
			self.slider = Some(slider::Slider::new(extent, self.range, self.period));
		}
		self.extent = extent;
		changed
	}
}

impl Panel {
	pub(super) fn toggle_filter(&mut self, cx: &mut Context<Self>) {
		self.filter.menu = !self.filter.menu;
		self.filter.time_menu = false;
		cx.notify();
	}

	fn toggle_time_filter(&mut self, cx: &mut Context<Self>) {
		self.filter.time_menu = !self.filter.time_menu;
		self.filter.menu = false;
		cx.notify();
	}

	pub(super) fn header(&mut self, cx: &mut Context<Self>) -> gpui::Div {
		let controls = self.filter.controls.clone();
		self.filter.time_focus.get_or_insert_with(|| [cx.focus_handle(), cx.focus_handle()]);
		let agent = self
			.filter
			.agent
			.as_ref()
			.and_then(|id| self.filter.agents.iter().find(|a| &a.id == id));
		gpui::div()
			.flex_none()
			.px(gpui::px(12.))
			.pt(gpui::px(6.))
			.pb(gpui::px(8.))
			.flex()
			.flex_col()
			.on_children_prepainted(move |bounds, _, _| controls.set(bounds.first().copied()))
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap(gpui::px(6.))
					.child(self.filter_button(
						agent.map_or_else(
							|| {
								if self.filter.agent.is_some() {
									"Selected agent"
								} else {
									if self.width < 220. { "Agents" } else { "All agents" }
								}
								.into()
							},
							|a| {
								format!(
									"{} A{}",
									a.label
										.chars()
										.find(|c| c.is_alphanumeric())
										.unwrap_or('A')
										.to_uppercase(),
									a.slot
								)
							},
						),
						self.filter.agent.is_some(),
						cx,
					))
					.child(self.time_filter_summary(cx)),
			)
			.child(self.time_slider(cx))
	}

	fn time_filter_summary(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let active = self.filter.range != TimeRange::default();
		let label = self.filter.slider.as_ref().map_or_else(
			|| self.filter.period.label().to_owned(),
			|s| {
				if s.covers_domain() { self.filter.period.label().into() } else { s.clock_label() }
			},
		);
		gpui::div()
			.flex_none()
			.flex()
			.items_center()
			.child(
				gpui::div()
					.id("delivery-time-filter")
					.debug_selector(|| "delivery-time-filter".into())
					.role(gpui::Role::Button)
					.tab_index(0)
					.aria_label("Choose time range")
					.aria_expanded(self.filter.time_menu)
					.h(gpui::px(26.))
					.px(gpui::px(6.))
					.flex()
					.items_center()
					.gap(gpui::px(5.))
					.rounded(gpui::px(6.))
					.cursor_pointer()
					.text_size(gpui::px(11.5))
					.text_color(crate::ui_theme::rgb(if active {
						ui_theme::BLUE
					} else {
						ui_theme::TEXT_MUTED
					}))
					.child(label)
					.child(workspace_symbols::icon_sized(Symbol::ChevronDown, 9.))
					.on_click(cx.listener(|s, _, _, cx| s.toggle_time_filter(cx)))
					.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.toggle_time_filter(cx);
							cx.stop_propagation();
						}
					}))
					.smooth(),
			)
			.when(active, |d| {
				d.child(
					gpui::div()
						.id("delivery-time-reset")
						.debug_selector(|| "delivery-time-reset".into())
						.role(gpui::Role::Button)
						.tab_index(0)
						.aria_label("Reset time filter")
						.size(gpui::px(20.))
						.flex()
						.items_center()
						.justify_center()
						.cursor_pointer()
						.text_color(crate::ui_theme::rgb(ui_theme::TEXT_MUTED))
						.child(workspace_symbols::icon_sized(Symbol::Restore, 12.))
						.on_click(cx.listener(|s, _, _, cx| s.reset_time(cx)))
						.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
							if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str())
							{
								s.reset_time(cx);
								cx.stop_propagation();
							}
						}))
						.smooth(),
				)
			})
			.into_any_element()
	}

	fn choose_period(&mut self, period: slider::Period, cx: &mut Context<Self>) {
		let slider = slider::Slider::new(self.filter.extent, TimeRange::default(), period);
		self.filter.period = period;
		self.filter.range = slider.range();
		self.filter.slider = Some(slider);
		self.reload(cx);
	}

	fn reset_time(&mut self, cx: &mut Context<Self>) {
		if self.filter.range == TimeRange::default() {
			return;
		}
		self.filter.range = TimeRange::default();
		self.filter.period = slider::Period::All;
		self.filter.slider = None;
		self.filter.receive_extent(self.filter.extent);
		self.reload(cx);
	}

	fn filter_button(
		&self,
		label: String,
		active: bool,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let (id, aria) = ("delivery-agent-filter", "Filter deliveries by agent");
		gpui::div()
			.id(id)
			.debug_selector(move || id.into())
			.role(gpui::Role::Button)
			.aria_label(aria)
			.aria_expanded(self.filter.menu)
			.tab_index(0)
			.flex_1()
			.min_w_0()
			.h(gpui::px(26.))
			.px(gpui::px(6.))
			.flex()
			.items_center()
			.gap(gpui::px(5.))
			.rounded(gpui::px(6.))
			.bg(crate::ui_theme::rgba(if active { 0x8aaaff20 } else { 0x00000000 }))
			.text_size(gpui::px(11.5))
			.overflow_hidden()
			.cursor_pointer()
			.when(self.width >= 220., |d| {
				d.child(workspace_symbols::icon_sized(Symbol::Agents, 13.))
			})
			.child(gpui::div().flex_1().min_w_0().text_ellipsis().child(label))
			.child(workspace_symbols::icon_sized(Symbol::ChevronDown, 9.))
			.on_click(cx.listener(move |s, _, _, cx| s.toggle_filter(cx)))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					s.toggle_filter(cx);
					cx.stop_propagation();
				}
			}))
			.smooth()
			.into_any_element()
	}

	pub(super) fn filter_menu(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let content = if self.filter.menu {
			self.agent_menu(cx)
		} else if self.filter.time_menu {
			self.time_menu(cx)
		} else {
			gpui::div().into_any_element()
		};
		gpui::div()
			.absolute()
			.top(gpui::px(31.))
			.left(gpui::px(4.))
			.right(gpui::px(4.))
			.child(ui_motion::popover(
				"delivery-filter-motion",
				self.filter.menu || self.filter.time_menu,
				gpui::div()
					.id("delivery-filter-menu")
					.occlude()
					.p(gpui::px(10.))
					.on_key_down(cx.listener(|s, event: &KeyDownEvent, _, cx| {
						if event.keystroke.key == "escape" {
							s.filter.menu = false;
							s.filter.time_menu = false;
							cx.stop_propagation();
							cx.notify();
						}
					}))
					.on_mouse_down_out(cx.listener(|s, event: &gpui::MouseDownEvent, _, cx| {
						if !s
							.filter
							.controls
							.get()
							.is_some_and(|bounds| bounds.contains(&event.position))
						{
							s.filter.menu = false;
							s.filter.time_menu = false;
							cx.notify();
						}
					}))
					.child(content),
			))
			.into_any_element()
	}

	fn time_menu(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let mut list = gpui::div().flex().flex_col().gap(gpui::px(2.));
		for period in [
			slider::Period::Today,
			slider::Period::Week,
			slider::Period::Fortnight,
			slider::Period::All,
		] {
			let label = period.label();
			list = list.child(
				gpui::div()
					.id(gpui::SharedString::from(format!("delivery-period-{label}")))
					.debug_selector(move || format!("delivery-period-{label}"))
					.role(gpui::Role::Button)
					.aria_label(format!("Time range: {label}"))
					.tab_index(0)
					.h(gpui::px(28.))
					.px(gpui::px(6.))
					.flex()
					.items_center()
					.justify_between()
					.rounded(gpui::px(6.))
					.cursor_pointer()
					.text_size(gpui::px(11.5))
					.bg(crate::ui_theme::rgba(if self.filter.period == period {
						0x8aaaff1c
					} else {
						0x00000000
					}))
					.child(label)
					.when(self.filter.period == period, |d| {
						d.child(workspace_symbols::icon_sized(Symbol::Confirm, 10.))
					})
					.on_click(cx.listener(move |s, _, _, cx| s.choose_period(period, cx)))
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.choose_period(period, cx);
							cx.stop_propagation();
						}
					}))
					.smooth(),
			);
		}
		list.into_any_element()
	}

	fn agent_menu(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let choices = std::iter::once((None, "All agents".to_owned(), None)).chain(
			self.filter.agents.iter().map(|a| (Some(a.id.clone()), a.label.clone(), Some(a.slot))),
		);
		let mut list = gpui::div()
			.id("delivery-agent-choices")
			.max_h(gpui::px(300.))
			.track_scroll(&self.filter.agent_scroll)
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap(gpui::px(2.));
		for (index, (id, label, slot)) in choices.enumerate() {
			let selected = id == self.filter.agent;
			let choice = id.clone();
			let key_choice = id.clone();
			list = list.child(
				gpui::div()
					.id(("delivery-agent-option", index))
					.debug_selector(move || format!("delivery-agent-option-{index}"))
					.role(gpui::Role::Button)
					.aria_label(format!("Filter agent: {label}"))
					.tab_index(0)
					.min_h(gpui::px(34.))
					.py(gpui::px(5.))
					.px(gpui::px(6.))
					.flex()
					.items_center()
					.gap(gpui::px(7.))
					.rounded(gpui::px(6.))
					.cursor_pointer()
					.bg(crate::ui_theme::rgba(if selected { 0x8aaaff1c } else { 0x00000000 }))
					.hover(|s| s.bg(crate::ui_theme::rgba(0xffffff0a)))
					.child(
						gpui::div()
							.w(gpui::px(18.))
							.flex_none()
							.text_size(gpui::px(12.5))
							.font_weight(gpui::FontWeight::SEMIBOLD)
							.text_color(crate::ui_theme::rgb(ui_theme::BLUE))
							.child(slot.map_or_else(
								|| "·".into(),
								|_| {
									label
										.chars()
										.find(|c| c.is_alphanumeric())
										.unwrap_or('A')
										.to_uppercase()
										.to_string()
								},
							)),
					)
					.child(
						gpui::div()
							.flex_1()
							.min_w_0()
							.whitespace_normal()
							.line_clamp(2)
							.text_ellipsis()
							.text_size(gpui::px(11.5))
							.line_height(gpui::px(15.))
							.child(label),
					)
					.when_some(slot, |d, n| {
						d.child(
							gpui::div()
								.flex_none()
								.text_size(gpui::px(10.))
								.text_color(crate::ui_theme::rgb(ui_theme::TEXT_MUTED))
								.child(format!("A{n}")),
						)
					})
					.when(selected, |d| {
						d.child(workspace_symbols::icon_sized(Symbol::Confirm, 10.))
					})
					.on_click(cx.listener(move |s, _, _, cx| {
						s.filter.agent = choice.clone();
						s.filter.extent = None;
						s.filter.slider = None;
						s.reload(cx);
					}))
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.filter.agent = key_choice.clone();
							s.filter.extent = None;
							s.filter.slider = None;
							s.reload(cx);
							cx.stop_propagation();
						}
					}))
					.smooth(),
			);
		}
		gpui::div()
			.relative()
			.child(list)
			.child(crate::ui_scroll::Scrollbar::new(
				"delivery-agent-scrollbar",
				"Scroll agents",
				self.filter.agent_scroll.clone(),
			))
			.into_any_element()
	}
}
