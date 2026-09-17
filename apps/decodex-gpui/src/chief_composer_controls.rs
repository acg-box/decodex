//! Composer-specific visual controls. Exact values remain visible and keyboard accessible.
use super::{ChiefSurface, SmoothControl, ui_theme};
use gpui::{
	Context, Role, SharedString, div,
	prelude::{InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled},
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
		.child(div().text_size(px(10.5)).child(level_label(level)))
		.into_any_element()
}

fn level_label(level: &str) -> &'static str {
	LEVELS.iter().find(|(value, _)| *value == level).map_or("High", |(_, label)| *label)
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
				div().flex().gap(px(2.)).p(px(3.)).rounded(px(6.)).bg(rgba(0xffffff06)).children(
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
								.h(px(24.))
								.rounded(px(7.))
								.bg(if active { rgba(0xffffff14) } else { rgba(0x00000000) })
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
								.child(label)
								.smooth()
						}),
				),
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
			path.move_to(origin + gpui::point(px(8.), px(13.)));
			path.line_to(origin + gpui::point(px(8.), px(3.)));
			path.move_to(origin + gpui::point(px(3.), px(8.)));
			path.line_to(origin + gpui::point(px(8.), px(3.)));
			path.line_to(origin + gpui::point(px(13.), px(8.)));
			if let Ok(path) = path.build() {
				window.paint_path(path, rgb(ui_theme::TEXT));
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
			.gap(px(2.));
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
		for pair in models.chunks(1) {
			let mut row = div().flex().gap(px(2.));
			for entry in pair {
				let model = entry.model.as_str().to_owned();
				let click_model = model.clone();
				let selected = self.model.read(cx).content() == model.as_str();
				let full = entry.name.clone();

				row = row.child(
					div()
						.id(SharedString::from(format!("model-{model}")))
						.role(Role::Button)
						.tab_index(0)
						.aria_label(format!("Select {full}"))
						.flex_1()
						.min_w_0()
						.h(px(26.))
						.px(px(7.))
						.rounded(px(6.))
						.bg(if selected { rgba(0xffffff0c) } else { rgba(0x00000000) })
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
							div().flex_1().min_w_0().flex().flex_col().gap(px(2.)).child(
								div()
									.text_size(px(12.))
									.whitespace_nowrap()
									.text_ellipsis()
									.child(full.clone()),
							),
						)
						.child(div().size(px(4.)).rounded_full().bg(if selected {
							rgb(0xc3b8ed)
						} else {
							rgba(0x00000000)
						}))
						.smooth(),
				);
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
