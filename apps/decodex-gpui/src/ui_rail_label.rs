//! A fixed first grapheme and a clipped title share one layout during sidebar motion.
use gpui::{
	App, FontWeight, IntoElement, RenderOnce, Rgba, Role, SharedString, TextRun, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};
use unicode_segmentation::UnicodeSegmentation as _;

use super::{HoverOverflowLabel, Viewport};
use crate::ui_theme;

#[derive(IntoElement)]
pub(crate) struct RailLabel {
	pub compact: bool,
	pub viewport: Option<Viewport>,
	pub scroll: bool,
	pub id: String,
	pub label: String,
	pub size: f32,
	pub initial_color: Rgba,
	pub initial_weight: FontWeight,
	pub badge: Option<Rgba>,
	pub status: Option<String>,
}

impl RenderOnce for RailLabel {
	fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
		let label = self.label.replace(['\n', '\r'], " ");
		let initial = label.graphemes(true).next().unwrap_or("?").to_owned();
		let rest = label.get(initial.len()..).unwrap_or("").to_owned();
		let size = gpui::px(self.size);
		let mut font = window.text_style().font();
		font.weight = self.initial_weight;
		let run = TextRun {
			len: initial.len(),
			font,
			color: self.initial_color.into(),
			background_color: None,
			underline: None,
			strikethrough: None,
		};
		let initial_width = f32::from(
			window.text_system().shape_line(initial.clone().into(), size, &[run], None).width,
		);
		let inset = if self.compact {
			0.
		} else {
			((ui_theme::CONVERSATION_TAB_SIZE - initial_width) * 0.5).max(0.)
		};
		let suffix = HoverOverflowLabel {
			id: SharedString::from(format!("left-title-{}", self.id)).into(),
			text: rest.into(),
			active: self.scroll,
		};
		let suffix = if let Some(viewport) = self.viewport {
			viewport.clip(ui_theme::SIDEBAR_INSET + inset, suffix).into_any_element()
		} else {
			suffix.into_any_element()
		};
		gpui::div()
			.debug_selector(|| "conversation-name".into())
			.min_w(gpui::px(if self.compact { 0. } else { ui_theme::CONVERSATION_TAB_SIZE }))
			.h_full()
			.flex_1()
			.relative()
			.flex()
			.items_center()
			.pl(gpui::px(inset))
			.pr(gpui::px(inset))
			.text_size(size)
			.when_some(self.badge, |row, color| {
				row.child(
					gpui::div()
						.absolute()
						.left(gpui::px(2.))
						.top(gpui::px(2.))
						.size(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
						.rounded(gpui::px(ui_theme::SMALL_CONTROL_RADIUS))
						.bg(color),
				)
			})
			.child(
				gpui::div()
					.id(SharedString::from(format!("conversation-mark-{}", self.id)))
					.debug_selector(move || format!("conversation-mark-{}", self.id))
					.when_some(self.status, |mark, status| {
						mark.role(Role::Status).aria_label(status)
					})
					.w(gpui::px(initial_width))
					.flex_none()
					.font_weight(self.initial_weight)
					.text_color(self.initial_color)
					.child(initial),
			)
			.child(gpui::div().flex_1().min_w_0().overflow_hidden().child(suffix))
	}
}
