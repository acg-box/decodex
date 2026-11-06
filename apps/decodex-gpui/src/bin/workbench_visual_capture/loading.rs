//! Native loading artwork at the sizes used by the app.
use std::{env, io::Error};

use gpui::{
	AnyWindowHandle, AppContext as _, Context, IntoElement, ParentElement as _, Render, Result,
	Styled as _, VisualTestAppContext, Window,
};

use crate::{
	ui_cloudlet::{Indicator, State},
	ui_loading, ui_theme,
};

pub(super) fn open(cx: &mut VisualTestAppContext) -> Result<AnyWindowHandle> {
	let elapsed = env::var("DECODEX_VISUAL_LOADING_TIME")
		.unwrap_or_else(|_| "0.75".into())
		.parse::<f32>()
		.map_err(|error| Error::other(format!("Invalid loading time: {error}")))?;
	Ok(cx
		.open_offscreen_window(gpui::size(gpui::px(640.), gpui::px(1_120.)), |_, cx| {
			cx.new(|_| Gallery { elapsed })
		})?
		.into())
}

struct Gallery {
	elapsed: f32,
}
impl Render for Gallery {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		let actions = [
			State::Working,
			State::Thinking,
			State::Reading,
			State::Writing,
			State::Inspecting,
			State::Waiting,
			State::Attention,
			State::Handoff,
		];
		gpui::div()
			.size_full()
			.bg(ui_theme::rgb(ui_theme::CANVAS))
			.text_color(ui_theme::rgb(ui_theme::TEXT))
			.font_family(ui_theme::FONT_FAMILY)
			.text_size(gpui::px(12.))
			.p(gpui::px(24.))
			.flex()
			.flex_col()
			.gap(gpui::px(12.))
			.child("Workspace")
			.child(
				gpui::div()
					.h(gpui::px(104.))
					.flex()
					.items_center()
					.justify_center()
					.child(ui_loading::workspace()),
			)
			.child("Conversation")
			.child(ui_loading::conversation("Loading conversation"))
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap(gpui::px(24.))
					.child(gpui::div().w(gpui::px(160.)).child("Agent activity"))
					.child(gpui::div().w(gpui::px(144.)).child("32 pt"))
					.child("24 pt"),
			)
			.children(actions.map(|action| {
				gpui::div()
					.h(gpui::px(44.))
					.flex()
					.items_center()
					.gap(gpui::px(24.))
					.child(gpui::div().w(gpui::px(160.)).child(action.label()))
					.child(gpui::div().w(gpui::px(144.)).child(Indicator {
						action,
						pose: action,
						elapsed: self.elapsed,
						edge: 32.,
						reduced: false,
					}))
					.child(Indicator {
						action,
						pose: action,
						elapsed: self.elapsed,
						edge: 24.,
						reduced: false,
					})
			}))
			.child("Compact loading (16 pt)")
			.child(ui_loading::loading("Loading earlier history"))
	}
}
