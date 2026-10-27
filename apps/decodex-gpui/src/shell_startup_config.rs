//! On-demand native configuration checks in the existing diagnostic details.
use std::{env, path::PathBuf};

use gpui::{
	AnyElement, App, ClipboardItem, Context, InteractiveElement as _, IntoElement as _,
	ParentElement as _, Styled as _, Task,
};
use tokio::runtime::Builder;

use crate::{
	shell::diagnostics::{self, Shell},
	ui_feedback::{self, Feedback},
	ui_theme,
};
use decodex_protocol::{
	ClientProfile, ConversationWorkingDirectory, DoctorClient, NativeStartupConfiguration,
	ProfileKind,
};

#[derive(Default)]
pub(super) struct Panel {
	source: Option<Source>,
	result: Option<NativeStartupConfiguration>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: Feedback,
}
impl Panel {
	pub(super) fn reset(&mut self) {
		*self = Self { epoch: self.epoch.wrapping_add(1), ..Default::default() };
	}
}

#[derive(Clone, Eq, PartialEq)]
struct Source {
	directory: PathBuf,
	profile: ClientProfile,
}

pub(super) fn content(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let current = source(shell, cx);
	let panel = &shell.diagnostics.startup;
	let result = panel.result.as_ref().filter(|_| panel.source == current);
	let mut group = gpui::div()
		.id("native-startup-config")
		.debug_selector(|| "native-startup-config".into())
		.w_full()
		.min_w_0()
		.flex()
		.flex_col()
		.gap_2()
		.child(ui_theme::settings_section_title("Local Codex startup configuration"))
		.child(ui_theme::settings_description(
			"Checks shared Codex sign-in. Task account overrides are excluded.",
		));

	if let Some(current) = &current {
		group = group
			.child(ui_theme::settings_description(format!(
				"Folder: {}",
				current.directory.display()
			)))
			.child(gpui::div().flex().child(diagnostics::control(
				"native-startup-check",
				if panel.task.is_some() { "Checking…" } else { "Check Codex configuration" },
				cx,
				start,
			)));
	} else {
		group = group.children(ui_feedback::inline_feedback(
			"native-startup-unavailable",
			"Configuration check unavailable",
			&Feedback::info("Reconnect and select an available folder to check configuration."),
		));
	}

	if !panel.feedback.is_empty() {
		group = group.children(ui_feedback::inline_feedback(
			"native-startup-feedback",
			"Configuration check",
			&panel.feedback,
		));
	}

	if let Some(result) = result {
		let text = result.report_text();

		group = group
			.child(
				gpui::div()
					.id("native-startup-result")
					.debug_selector(|| "native-startup-result".into())
					.text_size(gpui::px(11.))
					.child(text.clone()),
			)
			.child(diagnostics::control(
				"native-startup-copy",
				"Copy configuration result",
				cx,
				move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone())),
			));
	}

	group.into_any_element()
}

fn source(shell: &Shell, cx: &App) -> Option<Source> {
	let profile = shell.reset_cards.profile.clone()?;

	if profile.kind() != ProfileKind::Local {
		return None;
	}

	let directory = shell
		.agent
		.read(cx)
		.native_configuration_directory()
		.or_else(|| env::var_os("HOME").map(PathBuf::from))?;
	let valid = ConversationWorkingDirectory::new(directory.to_str()?).ok()?;

	Some(Source { directory: PathBuf::from(valid.as_str()), profile })
}

fn start(shell: &mut Shell, cx: &mut Context<Shell>) {
	if shell.diagnostics.startup.task.is_some() {
		return;
	}

	let Some(owner) = source(shell, cx) else {
		return;
	};
	let panel = &mut shell.diagnostics.startup;

	panel.epoch = panel.epoch.wrapping_add(1);

	let epoch = panel.epoch;

	panel.source = Some(owner.clone());
	panel.result = None;
	panel.feedback = Feedback::info("Checking Codex configuration…");

	let directory =
		ConversationWorkingDirectory::new(owner.directory.to_string_lossy().into_owned())
			.expect("source directory was validated");
	let profile = owner.profile.clone();
	let check = cx.background_executor().spawn(async move {
		let runtime = Builder::new_current_thread().enable_all().build().ok()?;

		runtime.block_on(DoctorClient::new(profile).startup_configuration(directory)).ok()
	});

	panel.task = Some(cx.spawn(async move |view, cx| {
		let result = check.await;
		let _ = view.update(cx, |shell, cx| {
			if shell.diagnostics.startup.epoch != epoch {
				return;
			}

			let current = source(shell, cx) == Some(owner);
			let panel = &mut shell.diagnostics.startup;

			panel.task = None;

			if current {
				panel.feedback.clear();

				panel.result = result;

				if panel.result.is_none() {
					panel.feedback = Feedback::warning("Could not check. Reconnect and retry.");
				}
			} else {
				panel.result = None;
				panel.feedback = Feedback::warning("Folder or connection changed. Check again.");
			}

			cx.notify();
		});
	}));

	cx.notify();
}

#[cfg(test)]
#[path = "shell_startup_config_tests.rs"]
mod tests;
