//! Native component preview using an actual saved tool response, not live weather.
use gpui::{prelude::*, *};

use decodex_protocol::WeatherForecast as Forecast;
#[path = "../src/chief_weather.rs"] mod weather_card;
struct Preview {
	forecast: Forecast,
	copied: bool,
}
impl Render for Preview {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		div()
			.size_full()
			.bg(rgba(0x17191ed0))
			.text_color(rgb(0xe8eaf0))
			.font_family(".AppleSystemUIFont")
			.p_6()
			.flex()
			.flex_col()
			.gap_4()
			.child(
				div()
					.max_w(px(460.))
					.text_size(px(14.))
					.line_height(px(22.))
					.child("Mostly cloudy in Singapore, with showers possible overnight."),
			)
			.child(weather_card::render(&self.forecast, "Sep 23", "preview"))
			.child(
				div()
					.id("copy-weather")
					.w(px(26.))
					.h(px(26.))
					.text_color(rgb(0x9ca7b5))
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
					size(px(560.), px(360.)),
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
					forecast: Forecast::parse(include_str!("fixtures/singapore-weather.txt"))
						.expect("captured weather format"),
					copied: false,
				})
			},
		)
		.unwrap();
		cx.activate(true);
	});
}
#[cfg(test)]
mod tests {
	use super::*;
	#[::core::prelude::v1::test]
	fn saved_weather_is_parsed_and_copied_without_control_markers() {
		let forecast = Forecast::parse(include_str!("fixtures/singapore-weather.txt")).unwrap();
		assert_eq!(forecast.reference, "turn0forecast0");
		assert_eq!(forecast.celsius, 32);
		assert_eq!(forecast.hours.len(), 12);
		assert_eq!(forecast.hours.last().unwrap(), &("02:00 AM".into(), "Showers".into(), 28));
		assert!(!forecast.markdown().contains('\u{e200}'));
		assert!(forecast.markdown().contains("| 02:00 AM | Showers | 28 |"));
		assert!(Forecast::parse("Weather unavailable").is_none());
	}
}
