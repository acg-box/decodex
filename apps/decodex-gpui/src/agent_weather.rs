//! Compact in-message weather presentation.
use gpui::{
	self, AnyElement, App, BoxShadow, FontWeight, Role, SharedString, Window,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
		StatefulInteractiveElement as _, Styled as _,
	},
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

		gpui::div()
			.id(SharedString::from(format!("weather-card-{key}")))
			.debug_selector(move || selector.clone())
			.mt(gpui::px(14.))
			.w(gpui::px(280.))
			.max_w_full()
			.flex_none()
			.rounded(gpui::px(14.))
			.bg(gpui::rgba(0xffffff08))
			.border_1()
			.border_color(gpui::rgba(0xffffff10))
			.shadow(vec![BoxShadow {
				inset: false,
				color: gpui::rgba(0x0000000d).into(),
				offset: gpui::point(gpui::px(0.), gpui::px(3.)),
				blur_radius: gpui::px(10.),
				spread_radius: gpui::px(-3.),
			}])
			.px(gpui::px(12.))
			.py(gpui::px(9.))
			.flex()
			.flex_col()
			.gap(gpui::px(5.))
			.child(weather_header(weather))
			.child(
				gpui::div().relative().w_full().h(gpui::px(56.)).overflow_hidden().child(
					gpui::div()
						.absolute()
						.top_0()
						.left(gpui::relative(-position))
						.w(gpui::relative(pages as f32))
						.h_full()
						.flex()
						.children((0..pages).map(|index| {
							gpui::div()
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
				card.child(gpui::div().flex().justify_center().children((0..pages).map(|index| {
					let state = state.clone();
					let selector = format!("weather-page-{key}-{index}");

					gpui::div()
						.id(SharedString::from(selector.clone()))
						.debug_selector(move || selector.clone())
						.role(Role::Button)
						.aria_label(format!("Forecast page {} of {}", index + 1, pages))
						.w(gpui::px(16.))
						.h(gpui::px(16.))
						.flex()
						.items_center()
						.justify_center()
						.cursor_pointer()
						.rounded(gpui::px(8.))
						.hover(|s| s.bg(gpui::rgba(0xffffff0a)))
						.on_click(move |_, window, cx| {
							state.update(cx, |s, cx| {
								s.select(index);
								cx.notify();
							});
							window.refresh();
						})
						.child(
							gpui::div().size(gpui::px(5.)).rounded_full().bg(gpui::rgba(
								if page == index { 0xffffffb0 } else { 0xffffff30 },
							)),
						)
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
	gpui::div()
		.id(SharedString::from(format!("weather-hour-{key}-{i}")))
		.debug_selector(move || format!("weather-hour-{i}"))
		.w(gpui::relative(1. / 6.))
		.flex_none()
		.min_w_0()
		.flex()
		.flex_col()
		.items_center()
		.justify_center()
		.gap(gpui::px(3.))
		.child(
			gpui::div()
				.text_size(gpui::px(9.))
				.text_color(gpui::rgb(0xaaa4af))
				.child(hour_label(hour)),
		)
		.child(
			gpui::div()
				.text_size(gpui::px(13.))
				.text_color(gpui::rgb(0xd5e3f1))
				.child(symbol(condition)),
		)
		.child(gpui::div().text_size(gpui::px(11.)).child(format!("{t}°")))
		.into_any_element()
}

fn weather_header(weather: &WeatherForecast) -> AnyElement {
	let location = weather.location.split(", ").next().unwrap_or(&weather.location);

	gpui::div()
		.flex()
		.items_center()
		.justify_between()
		.child(
			gpui::div()
				.flex()
				.flex_col()
				.gap(gpui::px(2.))
				.line_height(gpui::px(14.))
				.child(
					gpui::div()
						.text_size(gpui::px(12.))
						.font_weight(FontWeight::SEMIBOLD)
						.child(location.to_owned()),
				)
				.child(
					gpui::div()
						.text_size(gpui::px(10.))
						.text_color(gpui::rgb(0xaaa4af))
						.child(weather.condition.clone()),
				),
		)
		.child(
			gpui::div()
				.flex()
				.items_center()
				.gap_2()
				.child(
					gpui::div()
						.text_size(gpui::px(16.))
						.text_color(gpui::rgb(0xd5e3f1))
						.child(symbol(&weather.condition)),
				)
				.child(
					gpui::div()
						.text_size(gpui::px(23.))
						.line_height(gpui::px(26.))
						.child(format!("{}°", weather.celsius)),
				),
		)
		.into_any_element()
}

#[cfg(test)]
mod tests {
	use core::prelude::v1::test;
	use std::thread;

	use gpui::{
		self, Context, IntoElement, Modifiers, Render, ScrollDelta, ScrollWheelEvent, Window,
		prelude::{InteractiveElement as _, ParentElement as _, Styled as _},
	};

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

			gpui::div()
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
			assert_eq!(super::hour_label(source), expected, "{source}");
		}
	}

	#[gpui::test]
	fn weather_pages_change_only_on_dot_click_and_allow_parent_scroll(
		cx: &mut gpui::TestAppContext,
	) {
		let bubbled = std::rc::Rc::new(std::cell::Cell::new(0));
		let (_, visual) = cx.add_window_view(|_, _| Parent { bubbled: bubbled.clone() });

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(600.), gpui::px(400.)));
			window.draw(cx).clear();
		});

		let bounds = visual.debug_bounds("weather-card-test").unwrap();
		let start = visual.debug_bounds("weather-hour-0").unwrap().origin.x;

		visual.simulate_event(ScrollWheelEvent {
			position: bounds.center(),
			delta: ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-60.))),
			..Default::default()
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert_eq!(visual.debug_bounds("weather-hour-0").unwrap().origin.x, start);

		for delta in [
			gpui::point(gpui::px(0.), gpui::px(-60.)),
			gpui::point(gpui::px(-1_000.), gpui::px(-30.)),
			gpui::point(gpui::px(-1_000.), gpui::px(-30.)),
			gpui::point(gpui::px(1_000.), gpui::px(0.)),
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

		assert!(
			(visual.debug_bounds("weather-hour-0").unwrap().origin.x - start).abs() < gpui::px(1.)
		);
	}
}
