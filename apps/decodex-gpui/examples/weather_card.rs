//! Native component preview using an actual saved tool response, not live weather.
use gpui::{prelude::*, *};

use decodex_protocol::WeatherForecast as Forecast;
fn symbol(condition: &str) -> &'static str {
	let value = condition.to_ascii_lowercase();
	if value.contains("shower") || value.contains("rain") {
		"☂"
	} else if value.contains("sun") {
		"☀"
	} else {
		"☁"
	}
}
struct Preview {
	forecast: Forecast,
	copied: bool,
}
impl Render for Preview {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let weather = &self.forecast;
		let location = weather.location.split(", ").next().unwrap_or(&weather.location);
		div()
			.size_full()
			.bg(rgba(0x17191ee6))
			.text_color(rgb(0xe8eaf0))
			.font_family(".AppleSystemUIFont")
			.p_6()
			.flex()
			.flex_col()
			.gap_4()
			.child(div().max_w(px(500.)).text_size(px(14.)).line_height(px(22.)).child(
				"Mostly cloudy in Singapore. A brighter afternoon, with showers possible overnight.",
			))
			.child(
				div()
					.w(px(420.))
					.max_w_full()
					.flex_none()
					.rounded(px(18.))
					.bg(rgba(0x536a8330))
					.p_4()
					.flex()
					.flex_col()
					.gap_3()
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(
								div()
									.flex()
									.flex_col()
									.gap_1()
									.child(
										div()
											.text_size(px(15.))
											.font_weight(FontWeight::SEMIBOLD)
											.child(location.to_owned()),
									)
									.child(
										div()
											.text_size(px(11.))
											.text_color(rgb(0xaab9c9))
											.child("Sep 23 · Saved forecast"),
									),
							)
							.child(
								div()
									.flex()
									.items_center()
									.gap_2()
									.child(
										div()
											.text_size(px(26.))
											.text_color(rgb(0xd5e3f1))
											.child(symbol(&weather.condition)),
									)
									.child(
										div()
											.text_size(px(34.))
											.line_height(px(38.))
											.child(format!("{}°", weather.celsius)),
									),
							),
					)
					.child(
						div()
							.text_size(px(12.))
							.text_color(rgb(0xc5d1df))
							.child(weather.condition.clone()),
					)
					.child(div().id("weather-hours").flex().overflow_x_scroll().children(
						weather.hours.iter().enumerate().map(|(i, (hour, condition, t))| {
							div()
								.id(("hour", i))
								.flex_none()
								.w(px(64.))
								.py_2()
								.rounded(px(10.))
								.flex()
								.flex_col()
								.items_center()
								.gap_1()
								.hover(|s| s.bg(rgba(0xffffff0a)))
								.child(
									div()
										.text_size(px(10.))
										.text_color(rgb(0xaab9c9))
										.child(hour.trim_start_matches('0').replace(":00", "")),
								)
								.child(
									div()
										.text_size(px(19.))
										.text_color(rgb(if condition.contains("sun") {
											0xefcc86
										} else {
											0xd5e3f1
										}))
										.child(symbol(condition)),
								)
								.child(div().text_size(px(13.)).child(format!("{t}°")))
						}),
					)),
			)
			.child(
				div()
					.id("copy-weather")
					.w(px(26.))
					.h(px(26.))
					.flex()
					.items_center()
					.justify_center()
					.rounded(px(6.))
					.cursor_pointer()
					.text_size(px(14.))
					.text_color(rgb(0x9ca7b5))
					.hover(|s| s.bg(rgba(0xffffff10)))
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
