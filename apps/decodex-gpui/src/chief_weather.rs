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
	WeatherCard { weather: weather.clone(), date: date.into(), key: key.into() }.into_any_element()
}
#[derive(IntoElement)]
struct WeatherCard {
	weather: decodex_protocol::WeatherForecast,
	date: String,
	key: String,
}
impl RenderOnce for WeatherCard {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let weather = &self.weather;
		let date = &self.date;
		let key = &self.key;
		let state = window.use_keyed_state(
			SharedString::from(format!("weather-scroll-{key}")),
			cx,
			|_, _| ScrollHandle::new(),
		);
		let scroll = state.read(cx).clone();
		let max = f32::from(scroll.max_offset().x);
		let progress =
			if max > 0. { (-f32::from(scroll.offset().x) / max).clamp(0., 1.) } else { 0. };
		let wheel = scroll.clone();

		let selector = format!("weather-card-{key}");
		let location = weather.location.split(", ").next().unwrap_or(&weather.location);

		div()
			.id(SharedString::from(format!("weather-card-{key}")))
			.debug_selector(move || selector.clone())
			.on_scroll_wheel(move |event, window, cx| {
				let delta = event.delta.pixel_delta(px(20.));
				let amount = if delta.x.abs() >= delta.y.abs() { delta.x } else { delta.y };
				wheel.set_offset(point(
					(wheel.offset().x + amount).clamp(-wheel.max_offset().x, px(0.)),
					px(0.),
				));
				cx.stop_propagation();
				window.refresh();
			})
			.mt(px(14.))
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
							.child(div().text_size(px(11.)).text_color(rgb(0xaab9c9)).child(
								format!(
									"{} · {}",
									weather.condition,
									date.split(" · ").next().unwrap_or(date)
								),
							)),
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
					.track_scroll(&scroll)
					.children(weather.hours.iter().enumerate().map(|(i, (hour, condition, t))| {
						div()
							.id(SharedString::from(format!("weather-hour-{key}-{i}")))
							.debug_selector(move || format!("weather-hour-{i}"))
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
			.when(weather.hours.len() > 6, |card| {
				card.child(
					div().flex().justify_center().pt(px(2.)).child(
						div().w(px(48.)).h(px(2.)).rounded(px(1.)).bg(rgba(0xffffff12)).child(
							div()
								.ml(px(progress * 24.))
								.w(px(24.))
								.h(px(2.))
								.rounded(px(1.))
								.bg(rgba(0xffffff50)),
						),
					),
				)
			})
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use core::prelude::v1::test;
	struct Parent {
		bubbled: std::rc::Rc<std::cell::Cell<usize>>,
	}
	impl Render for Parent {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			let count = self.bubbled.clone();
			let forecast = decodex_protocol::WeatherForecast::parse(include_str!(
				"../examples/fixtures/singapore-weather.txt"
			))
			.unwrap();
			div()
				.id("parent")
				.size_full()
				.on_scroll_wheel(move |_, _, _| count.set(count.get() + 1))
				.child(super::render(&forecast, "Sep 23", "test"))
		}
	}
	#[gpui::test]
	fn weather_wheel_never_scrolls_parent_even_at_edges(cx: &mut gpui::TestAppContext) {
		let bubbled = std::rc::Rc::new(std::cell::Cell::new(0));
		let (_, visual) = cx.add_window_view(|_, _| Parent { bubbled: bubbled.clone() });
		visual.update(|window, cx| {
			window.resize(size(px(600.), px(400.)));
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("weather-card-test").unwrap();
		let start = visual.debug_bounds("weather-hour-0").unwrap().origin.x;
		visual.simulate_event(ScrollWheelEvent {
			position: bounds.center(),
			delta: ScrollDelta::Pixels(point(px(0.), px(-60.))),
			..Default::default()
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("weather-hour-0").unwrap().origin.x < start);

		for delta in [
			point(px(0.), px(-60.)),
			point(px(-1000.), px(-30.)),
			point(px(-1000.), px(-30.)),
			point(px(1000.), px(0.)),
		] {
			visual.simulate_event(ScrollWheelEvent {
				position: bounds.center(),
				delta: ScrollDelta::Pixels(delta),
				..Default::default()
			});
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
		}
		assert_eq!(bubbled.get(), 0);
	}
}
