//! Native component preview using an actual saved tool response, not live weather.
#[path = "../src/agent_weather.rs"] mod weather_card;

use futures_util as _;
use gpui::{
	AppContext as _, Bounds, ClipboardItem, Context, Render, TitlebarOptions, Window,
	WindowBackgroundAppearance, WindowBounds, WindowOptions,
	prelude::{InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled},
};
use libc as _;
use objc2 as _;
use objc2_app_kit as _;
use objc2_foundation as _;
use pulldown_cmark as _;
use raw_window_handle as _;
use reqwest as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use tempfile as _;
use time as _;
use tokio as _;
use tokio_tungstenite as _;
use unicode_segmentation as _;
use unicode_width as _;
#[cfg(target_os = "macos")]
use {
	block2 as _, libwebrtc as _, objc2_audio_toolbox as _, objc2_avf_audio as _,
	objc2_core_audio_types as _, rtrb as _,
};

use decodex_protocol::WeatherForecast;

struct Preview {
	forecast: WeatherForecast,
	copied: bool,
}
impl Render for Preview {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.size_full()
			.bg(gpui::rgba(0x17191ed0))
			.text_color(gpui::rgb(0xe8eaf0))
			.font_family(".AppleSystemUIFont")
			.p_6()
			.flex()
			.flex_col()
			.gap_4()
			.child(
				gpui::div()
					.max_w(gpui::px(460.))
					.text_size(gpui::px(14.))
					.line_height(gpui::px(22.))
					.child("Mostly cloudy in Singapore, with showers possible overnight."),
			)
			.child(weather_card::render(&self.forecast, "preview"))
			.child(
				gpui::div()
					.id("copy-weather")
					.w(gpui::px(26.))
					.h(gpui::px(26.))
					.text_color(gpui::rgb(0x9ca7b5))
					.cursor_pointer()
					.on_click(cx.listener(|s, _, _, cx| {
						cx.write_to_clipboard(ClipboardItem::new_string(s.forecast.markdown()));

						s.copied = true;

						cx.notify();
					}))
					.child(if self.copied { "✓" } else { "⧉" }),
			)
	}
}

fn main() {
	gpui_platform::application().run(|cx| {
		cx.open_window(
			WindowOptions {
				window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
					None,
					gpui::size(gpui::px(560.), gpui::px(360.)),
					cx,
				))),
				window_background: WindowBackgroundAppearance::Blurred,
				titlebar: Some(TitlebarOptions {
					title: Some("Decodex · Weather preview".into()),
					..Default::default()
				}),
				..Default::default()
			},
			|_, cx| {
				cx.new(|_| Preview {
					forecast: WeatherForecast::parse(include_str!(
						"fixtures/singapore-weather.txt"
					))
					.expect("captured weather format"),
					copied: false,
				})
			},
		)
		.expect("open weather preview window");
		cx.activate(true);
	});
}
#[cfg(test)]
mod tests {
	use crate::*;

	#[::core::prelude::v1::test]
	fn saved_weather_is_parsed_and_copied_without_control_markers() {
		let forecast =
			WeatherForecast::parse(include_str!("fixtures/singapore-weather.txt")).unwrap();

		assert_eq!(forecast.reference, "turn0forecast0");
		assert_eq!(forecast.celsius, 32);
		assert_eq!(forecast.hours.len(), 12);
		assert_eq!(forecast.hours.last().unwrap(), &("02:00 AM".into(), "Showers".into(), 28));
		assert!(!forecast.markdown().contains('\u{e200}'));
		assert!(forecast.markdown().contains("| 02:00 AM | Showers | 28 |"));
		assert!(WeatherForecast::parse("Weather unavailable").is_none());
	}
}
