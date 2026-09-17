//! Composer-specific visual controls. Exact values remain visible and keyboard accessible.
use super::{ChiefSurface, SmoothControl, ui_theme};
use gpui::{
	Context, Role, SharedString, div,
	prelude::{
		FluentBuilder, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
		Styled,
	},
	px, rgb, rgba,
};

const LEVELS: [(&str, &str); 8] = [
	("none", "None"),
	("minimal", "Minimal"),
	("low", "Low"),
	("medium", "Medium"),
	("high", "High"),
	("xhigh", "XHigh"),
	("max", "Max"),
	("ultra", "Ultra"),
];

pub(super) fn effort_indicator(level: &str) -> gpui::AnyElement {
	div()
		.flex()
		.items_center()
		.gap(px(7.))
		.child(meter(level))
		.child(div().text_size(px(10.5)).child(level_label(level)))
		.into_any_element()
}

fn level_label(level: &str) -> &'static str {
	LEVELS.iter().find(|(value, _)| *value == level).map_or("High", |(_, label)| *label)
}

fn meter(level: &str) -> gpui::AnyElement {
	let selected = LEVELS.iter().position(|(value, _)| *value == level).unwrap_or(4);
	div()
		.h(px(14.))
		.flex()
		.items_end()
		.gap(px(2.))
		.children((0..6).map(|index| {
			div()
				.w(px(2.))
				.h(px(4. + index as f32 * 1.7))
				.rounded(px(1.))
				.bg(if index + 2 <= selected { rgb(0xc3b8ed) } else { rgba(0xffffff24) })
		}))
		.into_any_element()
}

impl ChiefSurface {
	pub(super) fn effort_scale(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let selected = self.effort.as_str();
		let supported = self.model_efforts(cx);
		div()
			.flex()
			.flex_col()
			.gap(px(10.))
			.child(
				div().flex().gap(px(3.)).children(
					LEVELS
						.into_iter()
						.filter(|(value, _)| {
							supported.iter().any(|effort| effort.as_str() == *value)
						})
						.map(|(value, label)| {
							let active = selected == value;
							div()
								.id(SharedString::from(format!("depth-{value}")))
								.role(Role::Button)
								.tab_index(0)
								.aria_label(format!("Reasoning depth: {label}"))
								.flex_1()
								.h(px(58.))
								.rounded(px(7.))
								.border_1()
								.border_color(if active {
									rgba(0xc3b8ed66)
								} else {
									rgba(0xffffff08)
								})
								.bg(if active { rgba(0xc3b8ed1a) } else { rgba(0xffffff03) })
								.flex()
								.flex_col()
								.items_center()
								.justify_center()
								.gap(px(7.))
								.text_size(px(10.))
								.text_color(rgb(if active {
									ui_theme::TEXT
								} else {
									ui_theme::TEXT_MUTED
								}))
								.cursor_pointer()
								.hover(|d| d.bg(rgba(0xc3b8ed22)))
								.on_click(cx.listener(move |s, _, _, cx| {
									s.select_composer_option("effort", value, cx)
								}))
								.on_key_down(cx.listener(
									move |s, event: &gpui::KeyDownEvent, _, cx| {
										if ["enter", "space"]
											.contains(&event.keystroke.key.as_str())
										{
											s.select_composer_option("effort", value, cx);
											cx.stop_propagation();
										}
									},
								))
								.child(meter(value))
								.child(label)
								.smooth()
						}),
				),
			)
			.child(
				div()
					.flex()
					.justify_between()
					.text_size(px(10.))
					.text_color(rgb(ui_theme::TEXT_MUTED))
					.child("Less reasoning")
					.child("More reasoning"),
			)
			.into_any_element()
	}
}

pub(super) fn launch_mark() -> impl IntoElement {
	gpui::canvas(
		|_, _, _| (),
		|bounds, _, window, _| {
			let mut path = gpui::PathBuilder::stroke(px(1.5));
			let origin = bounds.origin;
			path.move_to(origin + gpui::point(px(4.), px(12.)));
			path.line_to(origin + gpui::point(px(12.), px(4.)));
			path.move_to(origin + gpui::point(px(5.), px(4.)));
			path.line_to(origin + gpui::point(px(12.), px(4.)));
			path.line_to(origin + gpui::point(px(12.), px(11.)));
			if let Ok(path) = path.build() {
				window.paint_path(path, rgb(0x282331));
			}
		},
	)
	.size(px(16.))
}

impl ChiefSurface {
	pub(super) fn model_palette(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let mut palette = div()
			.id("native-model-palette")
			.max_h(px(240.))
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap(px(5.));
		let models = match &self.capabilities {
			Some(decodex_protocol::ChiefCapabilitiesResult::Available { models, .. }) => models,
			_ =>
				return palette
					.child(div().text_size(px(11.)).text_color(rgb(ui_theme::TEXT_MUTED)).child(
						if self.capability_task.is_some() {
							"Loading models…"
						} else {
							"Model catalog is available when Chief is connected."
						},
					))
					.into_any_element(),
		};
		for pair in models.chunks(2) {
			let mut row = div().flex().gap(px(5.));
			for entry in pair {
				let model = entry.model.as_str().to_owned();
				let click_model = model.clone();
				let selected = self.model.read(cx).content() == model.as_str();
				let full = entry.name.clone();
				let (version, name) = full.split_once(' ').unwrap_or((&full, ""));
				row = row.child(
					div()
						.id(SharedString::from(format!("model-{model}")))
						.role(Role::Button)
						.tab_index(0)
						.aria_label(format!("Select {full}"))
						.flex_1()
						.min_w_0()
						.h(px(50.))
						.px(px(10.))
						.rounded(px(8.))
						.border_1()
						.border_color(if selected { rgba(0xc3b8ed55) } else { rgba(0xffffff0c) })
						.bg(if selected { rgba(0xc3b8ed15) } else { rgba(0xffffff03) })
						.flex()
						.items_center()
						.justify_between()
						.cursor_pointer()
						.hover(|d| d.bg(rgba(0xc3b8ed20)))
						.on_click(cx.listener(move |s, _, _, cx| {
							s.select_composer_option("model", &click_model, cx)
						}))
						.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
							if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
								s.select_composer_option("model", &model, cx);
								cx.stop_propagation();
							}
						}))
						.child(
							div()
								.flex()
								.flex_col()
								.gap(px(2.))
								.child(
									div()
										.text_size(px(12.))
										.whitespace_nowrap()
										.text_ellipsis()
										.child(if name.is_empty() {
											version.to_owned()
										} else {
											name.to_owned()
										}),
								)
								.when(!name.is_empty(), |d| {
									d.child(
										div()
											.text_size(px(10.))
											.text_color(rgb(ui_theme::TEXT_MUTED))
											.child(version.to_owned()),
									)
								}),
						)
						.child(div().size(px(4.)).rounded_full().bg(if selected {
							rgb(0xc3b8ed)
						} else {
							rgba(0xffffff18)
						}))
						.smooth(),
				);
			}
			if pair.len() == 1 {
				row = row.child(div().flex_1());
			}
			palette = palette.child(row);
		}
		palette.into_any_element()
	}
}

pub(super) fn compact_model_label(model: &str) -> String {
	let full = super::model_label(model);
	full.split_once(' ').map_or_else(|| full.clone(), |(_, name)| name.to_owned())
}
