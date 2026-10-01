//! Composer-specific visual controls. Exact values remain visible and keyboard accessible.
#[path = "agent_effort_slider.rs"] mod effort_slider;

use crate::shell::agent_surface::composer::{
	AgentSurface, SmoothControl,
	ui_theme::{self, CANVAS, HOVER_FILL, SELECTED_HOVER_FILL, TEXT, TEXT_MUTED},
};

use gpui::{
	Context, Role, SharedString, div,
	prelude::{InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled},
	px, rgb, rgba,
};

use crate::{shell::agent_surface::composer::model_settings, ui_loading, ui_motion};

use std::cmp::Reverse;

use gpui::{AnyElement, App, KeyDownEvent, PathBuilder, RenderOnce, Window};

use decodex_protocol::AgentCapabilitiesResult;

const LEVELS: [(&str, &str); 9] = [
	("none", "None"),
	("minimal", "Minimal"),
	("low", "Low"),
	("medium", "Medium"),
	("high", "High"),
	("xhigh", "XHigh"),
	("max", "Max"),
	("ultra", "Ultra"),
	("persistent", "Persistent"),
];

impl AgentSurface {
	pub(super) fn model_palette(&self, cx: &mut Context<Self>) -> AnyElement {
		self.catalog_model_palette(cx)
	}

	fn catalog_model_palette(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut palette = div()
			.id("native-model-palette")
			.max_h(px(240.))
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap(px(2.));
		let models = match self.current_model_catalog(cx) {
			Some(AgentCapabilitiesResult::Available { models, .. }) => models,
			_ if self.capability_task.is_some() =>
				return palette
					.min_h(px(64.))
					.child(ui_loading::loading("Loading models"))
					.into_any_element(),
			_ =>
				return palette
					.child(
						div()
							.text_size(px(11.))
							.text_color(rgb(TEXT_MUTED))
							.child("Model options are unavailable. Reopen to retry."),
					)
					.into_any_element(),
		};
		let mut models: Vec<_> = models.iter().collect();

		models.sort_by_key(|entry| Reverse(model_version(entry.model.as_str())));

		for pair in models.chunks(1) {
			let mut row = div().flex().gap(px(2.));

			for entry in pair {
				let model = entry.model.as_str().to_owned();
				let click_model = model.clone();
				let selected = self.composer_model_value(cx).as_deref() == Some(model.as_str());
				let full = model_settings::model_choice_label(entry);

				row = row.child(
					div()
						.id(SharedString::from(format!("model-{model}")))
						.role(Role::Button)
						.tab_index(0)
						.aria_label(format!("Select {full}"))
						.flex_1()
						.min_w_0()
						.h(px(32.))
						.px(px(7.))
						.rounded(px(6.))
						.bg(if selected { rgba(0xffffff0c) } else { rgba(0x00000000) })
						.flex()
						.items_center()
						.justify_between()
						.cursor_pointer()
						.hover(move |d| {
							d.bg(rgba(if selected { SELECTED_HOVER_FILL } else { HOVER_FILL }))
								.text_color(rgb(TEXT))
						})
						.on_click(cx.listener(move |s, _, _, cx| {
							s.select_composer_option("model", &click_model, cx)
						}))
						.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
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
						.child(
							div()
								.w(px(16.))
								.text_size(px(13.))
								.text_color(rgb(TEXT_MUTED))
								.child(if selected { "✓" } else { "" }),
						)
						.smooth(),
				);
			}

			palette = palette.child(row);
		}

		palette.into_any_element()
	}
}

pub(super) fn effort_indicator(level: &str) -> AnyElement {
	div()
		.flex()
		.items_center()
		.gap(px(7.))
		.child(div().text_size(px(11.)).child(level_label(level)))
		.into_any_element()
}

pub(super) fn live_mark() -> AnyElement {
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
			let mut path = PathBuilder::stroke(px(1.5));
			let origin = bounds.origin;

			path.move_to(origin + gpui::point(px(8.), px(13.)));
			path.line_to(origin + gpui::point(px(8.), px(3.)));
			path.move_to(origin + gpui::point(px(3.), px(8.)));
			path.line_to(origin + gpui::point(px(8.), px(3.)));
			path.line_to(origin + gpui::point(px(13.), px(8.)));

			if let Ok(path) = path.build() {
				window.paint_path(path, rgb(TEXT));
			}
		},
	)
	.size(px(16.))
}

fn level_label(level: &str) -> String {
	LEVELS.iter().find(|(value, _)| *value == level).map_or(level, |(_, label)| *label).to_owned()
}

/// Group GPT releases newest first while retaining catalog order within a release.
fn model_version(model: &str) -> Vec<u32> {
	model
		.strip_prefix("gpt-")
		.unwrap_or("")
		.split('-')
		.next()
		.unwrap_or("")
		.split('.')
		.map_while(|part| part.parse().ok())
		.collect()
}

#[cfg(test)]
mod ordering_tests {
	#[test]
	fn versions_descend_without_reordering_same_release_variants() {
		let mut models =
			["gpt-5.6-sol", "gpt-5.6-terra", "gpt-6-astra", "gpt-5.5", "gpt-5.10", "custom"];

		models.sort_by_key(|model| std::cmp::Reverse(super::model_version(model)));

		assert_eq!(
			models,
			["gpt-6-astra", "gpt-5.10", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.5", "custom"]
		);
	}
}

/// A stable stop glyph with a soft confirmation halo; its footprint never changes.
#[derive(gpui::IntoElement)]
pub(super) struct StopMark {
	pub armed: bool,
	pub pending: bool,
}
impl RenderOnce for StopMark {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let strength =
			ui_motion::value("stop-confirmation", if self.armed { 1. } else { 0. }, window, cx);
		let pending =
			ui_motion::value("stop-pending", if self.pending { 1. } else { 0. }, window, cx);

		div()
			.size(px(16.))
			.relative()
			.flex()
			.items_center()
			.justify_center()
			.child(
				div()
					.absolute()
					.size(px(24.))
					.rounded_full()
					.bg(rgba(0xf5c98500 | (strength * 48.) as u32)),
			)
			.child(
				div()
					.size(px(9.))
					.rounded(px(2.))
					.bg(rgb(CANVAS))
					.opacity(0.9 - pending * 0.35)
					.child(
						div()
							.size_full()
							.rounded(px(2.))
							.bg(rgba(0xf5c98500 | (strength * 255.) as u32)),
					),
			)
	}
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum PrimaryMode {
	Live,
	Stop,
	Send,
	Done,
}

/// Keep all primary glyphs on one footprint so phase changes crossfade in place.
#[derive(gpui::IntoElement)]
pub(super) struct PrimaryMark {
	pub mode: PrimaryMode,
	pub armed: bool,
	pub pending: bool,
}
impl RenderOnce for PrimaryMark {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let mut row = div().size(px(16.)).relative();

		for (id, mode, glyph) in [
			("primary-live", PrimaryMode::Live, live_mark()),
			(
				"primary-stop",
				PrimaryMode::Stop,
				StopMark { armed: self.armed, pending: self.pending }.into_any_element(),
			),
			("primary-send", PrimaryMode::Send, launch_mark().into_any_element()),
			("primary-done", PrimaryMode::Done, div().child("✓").into_any_element()),
		] {
			let opacity = ui_motion::value(id, if self.mode == mode { 1. } else { 0. }, window, cx);

			row = row.child(
				div()
					.absolute()
					.inset_0()
					.flex()
					.items_center()
					.justify_center()
					.opacity(opacity)
					.child(glyph),
			);
		}

		row
	}
}
