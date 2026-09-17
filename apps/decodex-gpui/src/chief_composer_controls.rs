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

#[path = "chief_effort_slider.rs"] mod effort_slider;

pub(super) fn live_mark() -> gpui::AnyElement {
	div()
		.size(px(16.))
		.flex()
		.items_center()
		.justify_center()
		.gap(px(1.5))
		.children(
			[5., 10., 15., 10., 5.]
				.map(|height| div().w(px(2.)).h(px(height)).rounded_full().bg(rgb(0xf4f2f7))),
		)
		.into_any_element()
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
