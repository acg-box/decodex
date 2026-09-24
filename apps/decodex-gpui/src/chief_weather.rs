//! Compact in-message weather presentation.
use gpui::{prelude::*, *};

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
pub(super) fn render(
	weather: &decodex_protocol::WeatherForecast,
	date: &str,
	key: &str,
) -> AnyElement {
	let selector = format!("weather-card-{key}");
	let location = weather.location.split(", ").next().unwrap_or(&weather.location);

	div()
		.id(SharedString::from(format!("weather-card-{key}")))
		.debug_selector(move || selector.clone())
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
								.child(date.to_owned()),
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
		.child(div().text_size(px(12.)).text_color(rgb(0xc5d1df)).child(weather.condition.clone()))
		.child(
			div()
				.id(SharedString::from(format!("weather-hours-{key}")))
				.flex()
				.overflow_x_scroll()
				.children(weather.hours.iter().enumerate().map(|(i, (hour, condition, t))| {
					div()
						.id(SharedString::from(format!("weather-hour-{key}-{i}")))
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
				})),
		)
		.into_any_element()
}
