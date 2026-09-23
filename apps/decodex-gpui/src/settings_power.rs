//! Host-owned macOS sleep policy. Never stores a competing application preference.
use super::*;

#[derive(Default)]
pub(super) struct PowerSettings {
	pub enabled: Option<bool>,
	pub pending: bool,
	pub error: Option<String>,
	pub checked: Option<std::time::Instant>,
	pub task: Option<gpui::Task<()>>,
}

fn parse_sleep_disabled(text: &str) -> Result<bool, String> {
	text.lines()
		.find_map(|line| {
			let mut words = line.split_whitespace();
			(words.next() == Some("SleepDisabled")).then(|| match words.next() {
				Some("0") => Ok(false),
				Some("1") => Ok(true),
				_ => Err("macOS returned an unknown sleep setting.".into()),
			})
		})
		.unwrap_or_else(|| Err("The macOS sleep setting could not be read.".into()))
}

#[cfg(target_os = "macos")]
fn read() -> Result<bool, String> {
	let result = std::process::Command::new("/usr/bin/pmset")
		.arg("-g")
		.output()
		.map_err(|_| "The macOS power service could not be read.".to_string())?;
	if !result.status.success() {
		return Err("The macOS power service could not be read.".into());
	}
	parse_sleep_disabled(&String::from_utf8_lossy(&result.stdout))
}
#[cfg(not(target_os = "macos"))]
fn read() -> Result<bool, String> {
	Err("Sleep control is available on macOS.".into())
}

#[cfg(target_os = "macos")]
fn set(enabled: bool) -> Result<bool, String> {
	let value = if enabled { "1" } else { "0" };
	// Use an existing authorization when available; never request a password in our UI.
	let direct = std::process::Command::new("/usr/bin/sudo")
		.args(["-n", "/usr/bin/pmset", "-a", "disablesleep", value])
		.output();
	if !direct.as_ref().is_ok_and(|result| result.status.success()) {
		let script = format!(
			"do shell script \"/usr/bin/pmset -a disablesleep {value}\" with administrator privileges"
		);
		let result = std::process::Command::new("/usr/bin/osascript")
			.args(["-e", &script])
			.output()
			.map_err(|_| "Could not request administrator authorization.".to_string())?;
		if !result.status.success() {
			if String::from_utf8_lossy(&result.stderr).contains("(-128)") {
				return read();
			}
			return Err(
				"macOS did not apply the sleep setting. Administrator authorization is required."
					.into(),
			);
		}
	}
	let actual = read()?;
	if actual != enabled {
		return Err("macOS did not confirm the sleep setting.".into());
	}
	Ok(actual)
}
#[cfg(not(target_os = "macos"))]
fn set(_: bool) -> Result<bool, String> {
	read()
}

impl SettingsSurface {
	pub(super) fn refresh_power(&mut self, cx: &mut Context<Self>) {
		if !cfg!(target_os = "macos")
			|| self.power.pending
			|| self.power.checked.is_some_and(|at| at.elapsed().as_secs() < 5)
		{
			return;
		}
		self.power_request(None, cx);
	}

	fn power_request(&mut self, desired: Option<bool>, cx: &mut Context<Self>) {
		if self.power.pending {
			return;
		}
		self.power.pending = true;
		let request = cx.background_executor().spawn(async move {
			match desired {
				Some(value) => set(value),
				None => read(),
			}
		});
		self.power.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				s.power.pending = false;
				s.power.checked = Some(std::time::Instant::now());
				match result {
					Ok(enabled) => {
						s.power.enabled = Some(enabled);
						s.power.error = None;
					},
					Err(error) => {
						s.power.enabled = None;
						s.power.error = Some(error);
					},
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn power_control(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		if !cfg!(target_os = "macos") {
			return div().into_any_element();
		}
		let enabled = self.power.enabled.unwrap_or(false);
		let interactive = self.power.enabled.is_some() && !self.power.pending;
		ui_theme::settings_row()
			.px_0()
			.child(
				div().flex_1().flex().flex_col().gap(px(3.)).child("Prevent system sleep").child(
					div().text_size(px(11.)).text_color(rgb(TEXT_MUTED)).child(
						"Applies to this Mac on power and battery, even after quitting Decodex.",
					),
				),
			)
			.child(
				div()
					.id("prevent-system-sleep")
					.debug_selector(|| "prevent-system-sleep".into())
					.role(Role::Switch)
					.aria_label("Prevent system sleep")
					.aria_toggled(if enabled { Toggled::True } else { Toggled::False })
					.tab_index(if interactive { 0 } else { -1 })
					.w(px(36.))
					.h(px(20.))
					.p(px(2.))
					.flex()
					.items_center()
					.rounded_full()
					.border_1()
					.border_color(rgb(if enabled { BLUE } else { LINE }))
					.bg(if enabled { rgba(0x8baaf730) } else { rgba(0xffffff0c) })
					.opacity(if interactive { 1. } else { 0.58 })
					.when(interactive, |toggle| {
						toggle
							.cursor_pointer()
							.hover(|d| d.border_color(rgb(TEXT_MUTED)))
							.on_click(
								cx.listener(move |s, _, _, cx| s.power_request(Some(!enabled), cx)),
							)
							.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
								if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
									s.power_request(Some(!enabled), cx);
									cx.stop_propagation();
								}
							}))
					})
					.child(switch_knob(
						"power-sleep-knob",
						enabled,
						div().size(px(14.)).rounded_full().bg(rgb(if enabled {
							BLUE
						} else {
							TEXT_MUTED
						})),
					)),
			)
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn reads_only_the_global_sleep_policy() {
		assert_eq!(
			parse_sleep_disabled("System-wide power settings:\n SleepDisabled\t1\n sleep 0"),
			Ok(true)
		);
		assert_eq!(parse_sleep_disabled("SleepDisabled 0\n sleep 1"), Ok(false));
		assert!(parse_sleep_disabled("sleep 0 (prevented by ChatGPT)").is_err());
		assert!(parse_sleep_disabled("SleepDisabled unknown").is_err());
	}
}
