//! Product-level settings for optional Decodex desktop surfaces.
//!
//! The menu bar remains a credential-negative protocol client. The main GPUI app
//! controls only whether the signed embedded companion is running; it does not
//! share credentials, database access, or mutation authority with that process.

use std::path::{Path, PathBuf};

use gpui::{
	Context, Render, Role, SharedString, Window, accesskit::Toggled, div, prelude::*, px, rgb, rgba,
};

use crate::ui_theme;

#[cfg(not(test))]
const MENUBAR_BUNDLE_ID: &str = "box.acg.decodex.menubar";
#[cfg(not(test))]
const MENUBAR_PREFERENCE_KEY: &str = "decodex.operator.menubar-enabled";
const MENUBAR_RELATIVE_BUNDLE: &str = "Library/LoginItems/DecodexMenuBar.app";

const LINE: u32 = ui_theme::LINE_STRONG;
const TEXT: u32 = ui_theme::TEXT;
const TEXT_MUTED: u32 = ui_theme::TEXT_MUTED;
const BLUE: u32 = ui_theme::BLUE;
const GREEN: u32 = ui_theme::GREEN;
const AMBER: u32 = ui_theme::AMBER;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(test, allow(dead_code))]
enum MenuBarRuntimeState {
	Running,
	Stopped,
	Starting,
	Stopping,
	Unavailable,
}

impl MenuBarRuntimeState {
	const fn label(self) -> &'static str {
		match self {
			Self::Running => "RUNNING",
			Self::Stopped => "OFF",
			Self::Starting => "STARTING",
			Self::Stopping => "STOPPING",
			Self::Unavailable => "NOT PACKAGED",
		}
	}

	const fn color(self) -> u32 {
		match self {
			Self::Running => GREEN,
			Self::Stopped => TEXT_MUTED,
			Self::Starting | Self::Stopping => AMBER,
			Self::Unavailable => 0xb56a6a,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(test, allow(dead_code))]
enum MenuBarControlFailure {
	BundlePathUnavailable,
	BundleNotInstalled,
	TerminationRefused,
	#[cfg(not(target_os = "macos"))]
	UnsupportedPlatform,
}

impl MenuBarControlFailure {
	const fn detail(self) -> &'static str {
		match self {
			Self::BundlePathUnavailable => {
				"The embedded menu bar bundle path is not representable on this host."
			},
			Self::BundleNotInstalled => {
				"The menu bar surface is included only in a staged Decodex application bundle."
			},
			Self::TerminationRefused => {
				"macOS did not accept the companion's normal termination request."
			},
			#[cfg(not(target_os = "macos"))]
			Self::UnsupportedPlatform => "The menu bar surface is available only on macOS.",
		}
	}
}

enum MenuBarController {
	#[cfg(not(test))]
	System { helper_bundle: PathBuf },
	#[cfg(test)]
	Simulated,
}

impl MenuBarController {
	#[cfg(not(test))]
	fn production() -> Self {
		Self::System { helper_bundle: embedded_menubar_bundle() }
	}

	#[cfg(test)]
	fn simulated() -> Self {
		Self::Simulated
	}

	fn current_state(&self) -> MenuBarRuntimeState {
		match self {
			#[cfg(not(test))]
			Self::System { helper_bundle } => system_current_state(helper_bundle),
			#[cfg(test)]
			Self::Simulated => MenuBarRuntimeState::Stopped,
		}
	}

	fn apply(&self, enabled: bool) -> Result<MenuBarRuntimeState, MenuBarControlFailure> {
		match self {
			#[cfg(not(test))]
			Self::System { helper_bundle } => system_apply(helper_bundle, enabled),
			#[cfg(test)]
			Self::Simulated => Ok(if enabled {
				MenuBarRuntimeState::Running
			} else {
				MenuBarRuntimeState::Stopped
			}),
		}
	}
}

#[derive(Clone, Copy)]
enum PreferenceBackend {
	#[cfg(not(test))]
	System,
	#[cfg(test)]
	Memory,
}

impl PreferenceBackend {
	fn load(self) -> bool {
		match self {
			#[cfg(not(test))]
			Self::System => system_load_preference(),
			#[cfg(test)]
			Self::Memory => true,
		}
	}

	fn store(self, enabled: bool) {
		match self {
			#[cfg(not(test))]
			Self::System => system_store_preference(enabled),
			#[cfg(test)]
			Self::Memory => {
				let _ = enabled;
			},
		}
	}
}

pub(crate) struct SettingsSurface {
	menubar_enabled: bool,
	runtime: MenuBarRuntimeState,
	detail: SharedString,
	controller: MenuBarController,
	preferences: PreferenceBackend,
}

impl SettingsSurface {
	pub(crate) fn new(_: &mut Context<Self>) -> Self {
		#[cfg(not(test))]
		let (controller, preferences) = (MenuBarController::production(), PreferenceBackend::System);
		#[cfg(test)]
		let (controller, preferences) = (MenuBarController::simulated(), PreferenceBackend::Memory);

		let menubar_enabled = preferences.load();
		let (runtime, detail) = match controller.apply(menubar_enabled) {
			Ok(runtime) => (runtime, runtime_detail(runtime).into()),
			Err(failure) => (MenuBarRuntimeState::Unavailable, failure.detail().into()),
		};

		Self { menubar_enabled, runtime, detail, controller, preferences }
	}

	pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
		self.runtime = self.controller.current_state();
		self.detail = runtime_detail(self.runtime).into();
		cx.notify();
	}

	fn toggle_menubar(&mut self, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
		self.set_menubar_enabled(!self.menubar_enabled, cx);
	}

	fn set_menubar_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
		self.menubar_enabled = enabled;
		self.preferences.store(self.menubar_enabled);
		match self.controller.apply(self.menubar_enabled) {
			Ok(runtime) => {
				self.runtime = runtime;
				self.detail = runtime_detail(runtime).into();
			},
			Err(failure) => {
				self.runtime = MenuBarRuntimeState::Unavailable;
				self.detail = failure.detail().into();
			},
		}
		cx.notify();
	}

	fn toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let enabled = self.menubar_enabled;
		div()
			.id("menubar-surface-toggle")
			.role(Role::Switch)
			.aria_label("Show the Decodex menu bar surface")
			.aria_toggled(if enabled { Toggled::True } else { Toggled::False })
			.w(px(52.0))
			.h(px(28.0))
			.p(px(3.0))
			.flex()
			.items_center()
			.rounded_full()
			.border_1()
			.border_color(rgb(if enabled { BLUE } else { LINE }))
			.bg(rgb(if enabled { 0x17314c } else { 0x151b20 }))
			.cursor_pointer()
			.hover(|element| element.border_color(rgb(TEXT_MUTED)))
			.active(|element| element.opacity(0.78))
			.focus_visible(|element| element.border_color(rgb(BLUE)))
			.on_click(cx.listener(Self::toggle_menubar))
			.child(
				div()
					.size(px(20.0))
					.rounded_full()
					.bg(rgb(if enabled { BLUE } else { TEXT_MUTED }))
					.when(enabled, |knob| knob.ml_auto()),
			)
	}
}

impl Render for SettingsSurface {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let runtime_color = self.runtime.color();
		div()
			.id("settings-surface")
			.role(Role::Main)
			.aria_label("Decodex settings")
			.size_full()
			.min_w_0()
			.min_h_0()
			.flex()
			.flex_col()
			.bg(rgba(0x00000000))
			.text_color(rgb(TEXT))
			.child(
				div()
					.id("settings-scroll-viewport")
					.flex_1()
					.min_h_0()
					.overflow_y_scroll()
					.px_7()
					.py_6()
					.flex()
					.justify_center()
					.child(
						div()
							.w_full()
							.max_w(px(940.0))
							.flex()
							.flex_col()
							.gap_6()
							.child(
								div()
									.flex()
									.flex_col()
									.gap_2()
									.child(
										div()
											.font_family("SF Mono")
											.text_size(px(8.0))
											.text_color(rgb(BLUE))
											.child("SETTINGS / DESKTOP SURFACES"),
									)
									.child(
										div()
											.text_size(px(22.0))
											.child("One product, one desktop system."),
									)
									.child(
										div()
											.max_w(px(700.0))
											.text_size(px(11.5))
											.line_height(px(17.0))
											.text_color(rgb(TEXT_MUTED))
											.child(
												"The main window owns conversations and factory operations. The optional menu bar keeps multi-account capacity, routing, and recovery close at hand.",
											),
									),
							)
							.child(
								div()
									.px_5()
									.py_5()
									.flex()
									.items_center()
									.gap_6()
									.border_1()
									.border_color(rgba(0xffffff12))
									.rounded(px(14.0))
									.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
									.child(
										div()
											.flex_1()
											.min_w_0()
											.flex()
											.flex_col()
											.gap_3()
											.child(
												div()
													.flex()
													.items_center()
													.gap_3()
													.child(div().text_size(px(15.0)).child("Menu bar"))
													.child(
														div()
																.px_2()
																.py_1()
																.border_1()
																.border_color(rgb(runtime_color))
																.rounded_full()
																.font_family("SF Mono")
																.text_size(px(8.0))
															.text_color(rgb(runtime_color))
															.child(self.runtime.label()),
													),
												)
											.child(
												div()
														.text_size(px(11.0))
														.line_height(px(16.0))
													.text_color(rgb(TEXT_MUTED))
													.child(
														"Show multi-account quota, routing, reauthentication and Reset Cards in the macOS menu bar.",
													),
											)
											.child(
												div()
													.id("menubar-runtime-status")
													.role(Role::Status)
													.aria_label(self.detail.clone())
														.font_family("SF Mono")
														.text_size(px(8.5))
													.text_color(rgb(runtime_color))
													.child(self.detail.clone()),
											),
									)
									.child(self.toggle(cx)),
							)
							.child(
								div()
									.p_6()
									.flex()
									.flex_col()
									.gap_5()
									.border_1()
									.border_color(rgba(0xffffff0e))
									.rounded(px(14.0))
									.bg(rgba(ui_theme::SURFACE_MATERIAL))
									.child(
										div()
											.font_family("SF Mono")
											.text_size(px(10.0))
											.text_color(rgb(TEXT_MUTED))
											.child("PROCESS AND AUTHORITY BOUNDARY"),
									)
									.child(
										div()
											.flex()
											.items_center()
											.justify_between()
											.text_size(px(11.5))
											.child(boundary_node("GPUI MAIN", "Factory · Settings", BLUE))
											.child(boundary_edge("typed protocol"))
											.child(boundary_node("DECODEXD", "state · effects", GREEN))
											.child(boundary_edge("typed protocol"))
											.child(boundary_node("MENU BAR", "accounts · quota", BLUE)),
									),
								),
						),
			)
	}
}

fn boundary_node(title: &'static str, detail: &'static str, color: u32) -> impl IntoElement {
	div()
		.w(px(190.0))
		.px_4()
		.py_3()
		.flex()
		.flex_col()
		.gap_1()
		.border_1()
		.border_color(rgba((color << 8) | 0x66))
		.rounded(px(9.0))
		.bg(rgba(0xffffff05))
		.child(div().font_family("SF Mono").text_size(px(9.0)).text_color(rgb(color)).child(title))
		.child(div().text_size(px(9.5)).text_color(rgb(TEXT_MUTED)).child(detail))
}

fn boundary_edge(label: &'static str) -> impl IntoElement {
	div()
		.flex_1()
		.min_w(px(80.0))
		.flex()
		.flex_col()
		.items_center()
		.gap_2()
		.text_size(px(9.0))
		.text_color(rgb(TEXT_MUTED))
		.child(label)
		.child(div().w_full().h(px(1.0)).bg(rgb(LINE)))
}

fn runtime_detail(state: MenuBarRuntimeState) -> &'static str {
	match state {
		MenuBarRuntimeState::Running => "The embedded menu bar companion is running.",
		MenuBarRuntimeState::Stopped => "The menu bar surface is disabled.",
		MenuBarRuntimeState::Starting => "macOS accepted the companion launch request.",
		MenuBarRuntimeState::Stopping => "macOS accepted the normal termination request.",
		MenuBarRuntimeState::Unavailable => "The embedded companion is unavailable.",
	}
}

fn embedded_menubar_bundle() -> PathBuf {
	std::env::current_exe()
		.ok()
		.and_then(|executable| executable.parent().map(Path::to_path_buf))
		.and_then(|macos| macos.parent().map(Path::to_path_buf))
		.map_or_else(
			|| PathBuf::from(MENUBAR_RELATIVE_BUNDLE),
			|contents| contents.join(MENUBAR_RELATIVE_BUNDLE),
		)
}

#[cfg(all(not(test), target_os = "macos"))]
fn system_current_state(_: &Path) -> MenuBarRuntimeState {
	use objc2_app_kit::NSRunningApplication;
	use objc2_foundation::NSString;

	let bundle_id = NSString::from_str(MENUBAR_BUNDLE_ID);
	let applications = NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id);
	if applications.is_empty() {
		MenuBarRuntimeState::Stopped
	} else {
		MenuBarRuntimeState::Running
	}
}

#[cfg(all(not(test), not(target_os = "macos")))]
fn system_current_state(_: &Path) -> MenuBarRuntimeState {
	MenuBarRuntimeState::Unavailable
}

#[cfg(all(not(test), target_os = "macos"))]
fn system_apply(
	helper_bundle: &Path,
	enabled: bool,
) -> Result<MenuBarRuntimeState, MenuBarControlFailure> {
	use objc2_app_kit::{NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
	use objc2_foundation::{NSString, NSURL};

	let bundle_id = NSString::from_str(MENUBAR_BUNDLE_ID);
	let applications = NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id);
	if enabled {
		if !applications.is_empty() {
			return Ok(MenuBarRuntimeState::Running);
		}
		if !helper_bundle.is_dir() {
			return Err(MenuBarControlFailure::BundleNotInstalled);
		}
		let helper_path =
			helper_bundle.to_str().ok_or(MenuBarControlFailure::BundlePathUnavailable)?;
		let helper_path = NSString::from_str(helper_path);
		let helper_url = NSURL::fileURLWithPath_isDirectory(&helper_path, true);
		let configuration = NSWorkspaceOpenConfiguration::configuration();
		configuration.setActivates(false);
		configuration.setAddsToRecentItems(false);
		NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
			&helper_url,
			&configuration,
			None,
		);
		Ok(MenuBarRuntimeState::Starting)
	} else {
		if applications.is_empty() {
			return Ok(MenuBarRuntimeState::Stopped);
		}
		if applications.iter().all(|application| application.terminate()) {
			Ok(MenuBarRuntimeState::Stopping)
		} else {
			Err(MenuBarControlFailure::TerminationRefused)
		}
	}
}

#[cfg(all(not(test), not(target_os = "macos")))]
fn system_apply(_: &Path, _: bool) -> Result<MenuBarRuntimeState, MenuBarControlFailure> {
	Err(MenuBarControlFailure::UnsupportedPlatform)
}

#[cfg(all(not(test), target_os = "macos"))]
fn system_load_preference() -> bool {
	use objc2_foundation::{NSString, NSUserDefaults};

	let defaults = NSUserDefaults::standardUserDefaults();
	let key = NSString::from_str(MENUBAR_PREFERENCE_KEY);
	defaults.objectForKey(&key).is_none_or(|_| defaults.boolForKey(&key))
}

#[cfg(all(not(test), not(target_os = "macos")))]
fn system_load_preference() -> bool {
	false
}

#[cfg(all(not(test), target_os = "macos"))]
fn system_store_preference(enabled: bool) {
	use objc2_foundation::{NSString, NSUserDefaults};

	let defaults = NSUserDefaults::standardUserDefaults();
	let key = NSString::from_str(MENUBAR_PREFERENCE_KEY);
	defaults.setBool_forKey(enabled, &key);
}

#[cfg(all(not(test), not(target_os = "macos")))]
fn system_store_preference(_: bool) {}

#[cfg(test)]
mod tests {
	use gpui::{TestAppContext, VisualTestContext, size};

	use super::*;

	fn open_settings(
		cx: &mut TestAppContext,
	) -> (gpui::Entity<SettingsSurface>, &mut VisualTestContext) {
		cx.add_window_view(|_, cx| SettingsSurface::new(cx))
	}

	#[test]
	fn embedded_bundle_is_below_the_outer_app_contents_directory() {
		let bundle = embedded_menubar_bundle();
		assert!(bundle.ends_with(MENUBAR_RELATIVE_BUNDLE));
	}

	#[gpui::test]
	fn toggle_persists_the_requested_surface_state_in_the_model(cx: &mut TestAppContext) {
		let (settings, visual) = open_settings(cx);
		assert!(settings.read_with(visual, |settings, _| settings.menubar_enabled));
		settings.update(visual, |settings, cx| {
			settings.set_menubar_enabled(false, cx);
		});
		let state =
			settings.read_with(visual, |settings, _| (settings.menubar_enabled, settings.runtime));
		assert_eq!(state, (false, MenuBarRuntimeState::Stopped));
	}

	#[gpui::test]
	fn settings_draw_at_the_selected_desktop_size(cx: &mut TestAppContext) {
		let (_settings, visual) = open_settings(cx);
		visual.update(|window, cx| {
			window.resize(size(px(1_490.0), px(1_055.0)));
			window.draw(cx).clear();
		});
	}
}
