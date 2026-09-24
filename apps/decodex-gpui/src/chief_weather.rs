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
		.w(px(360.))
		.max_w_full()
		.flex_none()
		.rounded(px(18.))
		.bg(rgba(0xffffff0b))
		.shadow(vec![BoxShadow {
			inset: false,
			color: rgba(0x00000022).into(),
			offset: point(px(0.), px(5.)),
			blur_radius: px(18.),
			spread_radius: px(-5.),
		}])
		.px(px(14.))
		.py(px(12.))
		.flex()
		.flex_col()
		.gap(px(8.))
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
								.text_size(px(13.))
								.font_weight(FontWeight::SEMIBOLD)
								.child(location.to_owned()),
						)
						.child(div().text_size(px(11.)).text_color(rgb(0xaab9c9)).child(format!(
							"{} · {}",
							weather.condition,
							date.split(" · ").next().unwrap_or(date)
						))),
				)
				.child(
					div()
						.flex()
						.items_center()
						.gap_2()
						.child(
							div()
								.text_size(px(20.))
								.text_color(rgb(0xd5e3f1))
								.child(symbol(&weather.condition)),
						)
						.child(
							div()
								.text_size(px(28.))
								.line_height(px(32.))
								.child(format!("{}°", weather.celsius)),
						),
				),
		)
		.child(
			div()
				.id(SharedString::from(format!("weather-hours-{key}")))
				.flex()
				.overflow_x_scroll()
				.children(weather.hours.iter().enumerate().map(|(i, (hour, condition, t))| {
					div()
						.id(SharedString::from(format!("weather-hour-{key}-{i}")))
						.flex_none()
						.w(px(55.))
						.py(px(4.))
						.rounded(px(10.))
						.flex()
						.flex_col()
						.items_center()
						.gap_1()
						.child(
							div()
								.text_size(px(10.))
								.text_color(rgb(0xaab9c9))
								.child(hour.trim_start_matches('0').replace(":00", "")),
						)
						.child(
							div()
								.text_size(px(15.))
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
