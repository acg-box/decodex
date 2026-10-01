//! Compact in-message weather presentation.
use gpui::{
	AnyElement, App, BoxShadow, FontWeight, Role, SharedString, Window, div, point, prelude::*, px,
	rgb, rgba,
};

use decodex_protocol::WeatherForecast;

#[derive(IntoElement)]
struct WeatherCard {
	weather: WeatherForecast,
	key: String,
}
impl RenderOnce for WeatherCard {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let weather = &self.weather;
		let key = &self.key;
		let pages = weather.hours.len().div_ceil(6).max(1);
		let state = window.use_keyed_state(
			SharedString::from(format!("weather-page-{key}")),
			cx,
			|_, _| PageMotion::default(),
		);
		let page = state.read(cx).target.min(pages - 1);
		let position = state.read(cx).position().clamp(0., (pages - 1) as f32);

		if (position - page as f32).abs() > 0.001 {
			let state = state.clone();

			window.on_next_frame(move |window, cx| {
				state.update(cx, |_, cx| cx.notify());
				window.refresh();
			});
			window.request_animation_frame();
		}

		let selector = format!("weather-card-{key}");

		div()
			.id(SharedString::from(format!("weather-card-{key}")))
			.debug_selector(move || selector.clone())
			.mt(px(14.))
			.w(px(280.))
			.max_w_full()
			.flex_none()
			.rounded(px(14.))
			.bg(rgba(0xffffff08))
			.border_1()
			.border_color(rgba(0xffffff10))
			.shadow(vec![BoxShadow {
				inset: false,
				color: rgba(0x0000000d).into(),
				offset: point(px(0.), px(3.)),
				blur_radius: px(10.),
				spread_radius: px(-3.),
			}])
			.px(px(12.))
			.py(px(9.))
			.flex()
			.flex_col()
			.gap(px(5.))
			.child(weather_header(weather))
			.child(
				div().relative().w_full().h(px(56.)).overflow_hidden().child(
					div()
						.absolute()
						.top_0()
						.left(gpui::relative(-position))
						.w(gpui::relative(pages as f32))
						.h_full()
						.flex()
						.children((0..pages).map(|index| {
							div()
								.w(gpui::relative(1. / pages as f32))
								.flex_none()
								.h_full()
								.flex()
								.children(
									weather.hours.iter().enumerate().skip(index * 6).take(6).map(
										|(i, (hour, condition, t))| {
											weather_hour(key, i, hour, condition, *t)
										},
									),
								)
						})),
				),
			)
			.when(pages > 1, |card| {
				card.child(div().flex().justify_center().children((0..pages).map(|index| {
					let state = state.clone();
					let selector = format!("weather-page-{key}-{index}");

					div()
						.id(SharedString::from(selector.clone()))
						.debug_selector(move || selector.clone())
						.role(Role::Button)
						.aria_label(format!("Forecast page {} of {}", index + 1, pages))
						.w(px(16.))
						.h(px(16.))
						.flex()
						.items_center()
						.justify_center()
						.cursor_pointer()
						.rounded(px(8.))
						.hover(|s| s.bg(rgba(0xffffff0a)))
						.on_click(move |_, window, cx| {
							state.update(cx, |s, cx| {
								s.select(index);
								cx.notify();
							});
							window.refresh();
						})
						.child(div().size(px(5.)).rounded_full().bg(rgba(if page == index {
							0xffffffb0
						} else {
							0xffffff30
						})))
				})))
			})
			.into_any_element()
	}
}

#[derive(Default)]
struct PageMotion {
	target: usize,
	from: f32,
	started: Option<std::time::Instant>,
}
impl PageMotion {
	fn position(&self) -> f32 {
		let t = self.started.map(|at| (at.elapsed().as_secs_f32() / 0.24).min(1.)).unwrap_or(1.);

		self.from + (self.target as f32 - self.from) * (1. - (1. - t).powi(3))
	}

	fn select(&mut self, target: usize) {
		if self.target != target {
			self.from = self.position();
			self.target = target;
			self.started = Some(std::time::Instant::now());
		}
	}
}

pub(super) fn render(weather: &WeatherForecast, key: &str) -> AnyElement {
	WeatherCard { weather: weather.clone(), key: key.into() }.into_any_element()
}

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

fn hour_label(hour: &str) -> String {
	let short = hour.trim_start_matches('0');
	let short = if short.starts_with(':') { format!("0{short}") } else { short.to_owned() };

	short.replace(":00", "")
}

fn weather_hour(key: &str, i: usize, hour: &str, condition: &str, t: i32) -> AnyElement {
	div()
		.id(SharedString::from(format!("weather-hour-{key}-{i}")))
		.debug_selector(move || format!("weather-hour-{i}"))
		.w(gpui::relative(1. / 6.))
		.flex_none()
		.min_w_0()
		.flex()
		.flex_col()
		.items_center()
		.justify_center()
		.gap(px(3.))
		.child(div().text_size(px(9.)).text_color(rgb(0xaaa4af)).child(hour_label(hour)))
		.child(div().text_size(px(13.)).text_color(rgb(0xd5e3f1)).child(symbol(condition)))
		.child(div().text_size(px(11.)).child(format!("{t}°")))
		.into_any_element()
}

fn weather_header(weather: &WeatherForecast) -> AnyElement {
	let location = weather.location.split(", ").next().unwrap_or(&weather.location);

	div()
		.flex()
		.items_center()
		.justify_between()
		.child(
			div()
				.flex()
				.flex_col()
				.gap(px(2.))
				.line_height(px(14.))
				.child(
					div()
						.text_size(px(12.))
						.font_weight(FontWeight::SEMIBOLD)
						.child(location.to_owned()),
				)
				.child(
					div()
						.text_size(px(10.))
						.text_color(rgb(0xaaa4af))
						.child(weather.condition.clone()),
				),
		)
		.child(
			div()
				.flex()
				.items_center()
				.gap_2()
				.child(
					div()
						.text_size(px(16.))
						.text_color(rgb(0xd5e3f1))
						.child(symbol(&weather.condition)),
				)
				.child(
					div()
						.text_size(px(23.))
						.line_height(px(26.))
						.child(format!("{}°", weather.celsius)),
				),
		)
		.into_any_element()
}

#[cfg(test)]
mod tests {
	use super::*;

	use core::prelude::v1::test;

	use gpui::{self, Context, Modifiers, Render, ScrollDelta, ScrollWheelEvent};

	use std::thread;

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
				.child(super::render(&forecast, "test"))
		}
	}

	#[test]
	fn compact_hours_keep_midnight_and_nonzero_minutes() {
		for (source, expected) in [
			("00:00", "0"),
			("0:00", "0"),
			("00:30", "0:30"),
			("02:00 AM", "2 AM"),
			("12:00 PM", "12 PM"),
			("23:45", "23:45"),
		] {
			assert_eq!(hour_label(source), expected, "{source}");
		}
	}

	#[gpui::test]
	fn weather_pages_change_only_on_dot_click_and_allow_parent_scroll(
		cx: &mut gpui::TestAppContext,
	) {
		let bubbled = std::rc::Rc::new(std::cell::Cell::new(0));
		let (_, visual) = cx.add_window_view(|_, _| Parent { bubbled: bubbled.clone() });

		visual.update(|window, cx| {
			window.resize(gpui::size(px(600.), px(400.)));
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

		assert_eq!(visual.debug_bounds("weather-hour-0").unwrap().origin.x, start);

		for delta in [
			point(px(0.), px(-60.)),
			point(px(-1_000.), px(-30.)),
			point(px(-1_000.), px(-30.)),
			point(px(1_000.), px(0.)),
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

		assert_eq!(bubbled.get(), 5);

		let dot = visual.debug_bounds("weather-page-test-1").unwrap();

		visual.simulate_click(dot.center(), Modifiers::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let first_frame = visual.debug_bounds("weather-hour-0").unwrap().origin.x;

		thread::sleep(std::time::Duration::from_millis(100));

		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		let middle = visual.debug_bounds("weather-hour-0").unwrap().origin.x;

		assert!(middle < first_frame);

		thread::sleep(std::time::Duration::from_millis(180));

		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		assert!(visual.debug_bounds("weather-hour-6").unwrap().left() < bounds.right());
		assert!(visual.debug_bounds("weather-hour-6").is_some());
		assert!(visual.debug_bounds("weather-hour-11").is_some());

		let dot = visual.debug_bounds("weather-page-test-0").unwrap();

		visual.simulate_click(dot.center(), Modifiers::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("weather-hour-0").is_some());

		thread::sleep(std::time::Duration::from_millis(280));

		visual.update(|w, cx| {
			w.draw(cx).clear();
		});

		assert!((visual.debug_bounds("weather-hour-0").unwrap().origin.x - start).abs() < px(1.));
	}
}
