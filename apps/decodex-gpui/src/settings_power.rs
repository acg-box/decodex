//! Host-owned macOS sleep policy. Never stores a competing application preference.
use gpui::{AnyElement, Task};

use crate::settings_surface::{Context, IntoElement, ParentElement, SettingsSurface, ui_theme};

#[derive(Default)]
pub(super) struct PowerSettings {
	pub enabled: Option<bool>,
	pub pending: bool,
	pub error: Option<String>,
	pub checked: Option<std::time::Instant>,
	pub task: Option<Task<()>>,
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

	pub(super) fn power_control(&self, cx: &mut Context<Self>) -> AnyElement {
		if !cfg!(target_os = "macos") {
			return gpui::div().into_any_element();
		}

		let enabled = self.power.enabled.unwrap_or(false);
		let interactive = self.power.enabled.is_some() && !self.power.pending;

		ui_theme::settings_row()
			.child(ui_theme::settings_label(
				"Prevent system sleep",
				"Applies to this Mac on power and battery, even after quitting Decodex.",
			))
			.child(
				super::switch::settings_switch(
					"prevent-system-sleep",
					"power-sleep-knob",
					"Prevent system sleep",
					enabled,
					interactive,
				)
				.aria_description(if self.power.pending {
					"Checking the system setting"
				} else if self.power.enabled.is_none() {
					"The system setting is unavailable"
				} else {
					"Changes the macOS sleep setting"
				})
				.on_change(
					cx.listener(move |s, desired: &bool, _, cx| {
						s.power_request(Some(*desired), cx)
					}),
				),
			)
			.into_any_element()
	}
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

#[cfg(test)]
mod tests {
	use crate::settings_surface::power;

	#[test]
	fn reads_only_the_global_sleep_policy() {
		assert_eq!(
			power::parse_sleep_disabled("System-wide power settings:\n SleepDisabled\t1\n sleep 0"),
			Ok(true)
		);
		assert_eq!(power::parse_sleep_disabled("SleepDisabled 0\n sleep 1"), Ok(false));
		assert!(power::parse_sleep_disabled("sleep 0 (prevented by ChatGPT)").is_err());
		assert!(power::parse_sleep_disabled("SleepDisabled unknown").is_err());
	}
}
