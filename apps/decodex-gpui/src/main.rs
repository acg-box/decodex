//! Production Decodex GPUI macOS composition root.

#[cfg(target_os = "macos")] use objc2_foundation as _;

mod account_login;
mod account_profile;
mod accounts;
mod bundled_daemon;
#[cfg_attr(
	not(test),
	allow(
		dead_code,
		reason = "XY-1333 cache inspection/disposal constructors are exercised only by colocated tests"
	)
)]
mod client_cache;
mod client_lifecycle;
mod composer_input;
mod conversations;
mod desktop_settings;
mod health_query;
#[cfg_attr(
	not(test),
	allow(
		dead_code,
		reason = "XY-1429 pager controls are composed by the later Conversation destination"
	)
)]
mod history_pager;
mod native_menu_bar;
mod settings_surface;
mod shell;
mod ui_motion;
mod ui_theme;

#[cfg(target_os = "macos")] use objc2 as _;
use std::{cell::RefCell, rc::Rc, sync::Arc};

use gpui::{
	App, AppContext as _, Bounds, WindowBackgroundAppearance, WindowBounds, WindowHandle,
	WindowOptions, point, px, size,
};
use gpui_platform::application;

use decodex_protocol::{ClientFailure, ClientProfile};

use crate::{
	account_login::AccountLoginController,
	client_lifecycle::{
		ClientLifecycle, CompatibilityReason, ConnectionView, QuarantineReason, QuarantineRecovery,
	},
	shell::Shell,
};

fn main() {
	let application = application();
	let main_window: Rc<RefCell<Option<WindowHandle<Shell>>>> = Rc::new(RefCell::new(None));
	application.on_reopen({
		let main_window = Rc::clone(&main_window);
		move |cx| {
			if let Some(window) = main_window.borrow().as_ref() {
				activate_main_window(window, cx);
			}
		}
	});
	application.run(move |cx: &mut App| {
		shell::bind_keys(cx);
		install_application_menu(cx);
		let profile = ClientProfile::load_default(None);
		let chief_profile = profile.as_ref().ok().cloned();
		let bundled_daemon = profile.as_ref().ok().and_then(|profile| {
			bundled_daemon::BundledDaemonSupervisor::launch_for_profile(profile).ok().flatten()
		});
		if let Some(supervisor) = bundled_daemon.as_ref() {
			bundled_daemon::retain(Arc::clone(supervisor), cx);
		}
		let (initial_connection, lifecycle, account_login) =
			compose_lifecycle(profile, bundled_daemon);
		let bounds = Bounds::centered(None, size(px(1248.0), px(840.0)), cx);
		let window = cx
			.open_window(
				WindowOptions {
					titlebar: Some(gpui::TitlebarOptions {
						title: Some("Decodex".into()),
						appears_transparent: true,
						traffic_light_position: Some(point(px(14.0), px(14.0))),
					}),
					window_background: WindowBackgroundAppearance::Blurred,
					app_owns_titlebar_drag: true,
					window_bounds: Some(WindowBounds::Windowed(bounds)),
					window_min_size: Some(size(px(1180.0), px(720.0))),
					focus: false,
					show: false,
					..Default::default()
				},
				move |window, cx| {
					let account_login = account_login.clone();
					cx.new(|cx| {
						Shell::new(window, cx, initial_connection)
							.with_account_login(account_login)
							.with_chief_profile(chief_profile, cx)
					})
				},
			)
			.expect("open the Decodex production window");

		#[cfg(target_os = "macos")]
		window
			.update(cx, |_, window, cx| {
				configure_window_material(window);
				schedule_window_control_alignment(window);
				cx.observe_window_activation(window, |_, window, _| {
					schedule_window_control_alignment(window);
				})
				.detach();
				cx.observe_window_bounds(window, |_, window, _| {
					schedule_window_control_alignment(window);
				})
				.detach();
				cx.observe_window_appearance(window, |_, window, _| {
					schedule_window_control_alignment(window);
				})
				.detach();
			})
			.expect("configure native window material");

		if let Some(lifecycle) = lifecycle {
			shell::retain_lifecycle(window, lifecycle, cx);
		}
		window
			.update(cx, |_, window, cx| {
				window.on_window_should_close(cx, |_, cx| {
					hide_main_window(cx);
					false
				});
			})
			.expect("install the Decodex close-to-background behavior");
		let launched_as_login_item =
			window.entity(cx).is_ok_and(|shell| shell.read(cx).was_launched_as_login_item(cx));
		main_window.borrow_mut().replace(window);
		if launched_as_login_item {
			#[cfg(target_os = "macos")]
			hide_main_window(cx);
			#[cfg(not(target_os = "macos"))]
			cx.hide();
		} else {
			activate_main_window(&window, cx);
		}
	});
}

fn activate_main_window(window: &WindowHandle<Shell>, cx: &mut App) {
	window
		.update(cx, |_, window, _| window.activate_window())
		.expect("activate the retained Decodex window");
	#[cfg(target_os = "macos")]
	activate_native_application();
	#[cfg(not(target_os = "macos"))]
	cx.activate(true);
}

#[cfg(target_os = "macos")]
fn activate_native_application() {
	use objc2::{ClassType, MainThreadMarker};
	use objc2_app_kit::{NSApplication, NSPanel};
	use objc2_foundation::NSObjectProtocol;

	let main_thread =
		MainThreadMarker::new().expect("GPUI application callback runs on main thread");
	let application = NSApplication::sharedApplication(main_thread);
	application.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Regular);
	for window in application.windows().iter() {
		if !window.isKindOfClass(NSPanel::class()) && window.title().to_string() == "Decodex" {
			if window.isMiniaturized() {
				window.deminiaturize(None);
			}
			window.makeKeyAndOrderFront(None);
		}
	}
	application.activate();
}

#[cfg(target_os = "macos")]
fn configure_window_material(_gpui_window: &gpui::Window) {
	use objc2::MainThreadMarker;
	use objc2_app_kit::{
		NSApplication, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
		NSVisualEffectView,
	};
	let main_thread =
		MainThreadMarker::new().expect("window material is configured on the main thread");
	for window in NSApplication::sharedApplication(main_thread).windows().iter() {
		if window.title().to_string() != "Decodex" {
			continue;
		}
		if let Some(content) = window.contentView() {
			for view in content.subviews().iter() {
				if let Some(effect) = view.downcast_ref::<NSVisualEffectView>() {
					effect.setMaterial(NSVisualEffectMaterial::Sidebar);
					effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
					effect.setState(NSVisualEffectState::Active);
				}
			}
		}
	}
}

#[cfg(target_os = "macos")]
fn schedule_window_control_alignment(window: &gpui::Window) {
	// AppKit can replace or resize standard buttons during activation. Measure
	// after that layout pass, not once while the initial window is still hidden.
	window.on_next_frame(|window, _| {
		use objc2::MainThreadMarker;
		use objc2_app_kit::{NSApplication, NSWindowButton};
		let main_thread = MainThreadMarker::new().expect("window layout runs on the main thread");
		for native in NSApplication::sharedApplication(main_thread).windows().iter() {
			if native.title().to_string() != "Decodex" {
				continue;
			}
			if let Some(button) = native.standardWindowButton(NSWindowButton::CloseButton) {
				let center_y = ui_theme::CONTROL_MARGIN + ui_theme::CONTROL_GROUP_HEIGHT / 2.0;
				window.set_traffic_light_position(point(
					px(16.0),
					px(center_y - button.frame().size.height as f32 / 2.0),
				));
			}
		}
	});
}

#[cfg(target_os = "macos")]
fn order_out_native_windows() {
	use objc2::{ClassType, MainThreadMarker};
	use objc2_app_kit::{NSApplication, NSPanel};
	use objc2_foundation::NSObjectProtocol;

	let main_thread =
		MainThreadMarker::new().expect("GPUI application callback runs on main thread");
	let application = NSApplication::sharedApplication(main_thread);
	for window in application.windows().iter() {
		if !window.isKindOfClass(NSPanel::class()) && window.title().to_string() == "Decodex" {
			window.orderOut(None);
		}
	}
	application.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Accessory);
}

fn compose_lifecycle(
	profile: Result<ClientProfile, ClientFailure>,
	bundled_daemon: Option<Arc<bundled_daemon::BundledDaemonSupervisor>>,
) -> (ConnectionView, Option<ClientLifecycle>, Option<Arc<AccountLoginController>>) {
	let profile = match profile {
		Ok(profile) => profile,
		Err(failure) => {
			return (
				ConnectionView::Incompatible(CompatibilityReason::Startup(failure)),
				None,
				None,
			);
		},
	};
	let account_login = Arc::new(AccountLoginController::new(profile.clone()));
	let config = match profile.retained_session_config() {
		Ok(config) => config,
		Err(_) => {
			return (
				ConnectionView::Incompatible(CompatibilityReason::InvalidEndpoint),
				None,
				None,
			);
		},
	};
	match ClientLifecycle::production(config) {
		Ok(mut lifecycle) => {
			if let Some(supervisor) = bundled_daemon {
				lifecycle.supervise_app_owned_daemon(supervisor);
			}
			(lifecycle.view(), Some(lifecycle), Some(account_login))
		},
		Err(_) => (
			ConnectionView::Quarantined {
				reason: QuarantineReason::CacheRootUnsafe,
				recovery: QuarantineRecovery::OperatorRequired,
			},
			None,
			None,
		),
	}
}

gpui::actions!(
	decodex_application,
	[
		/// Quit the application and release its owned services.
		Quit,
		/// Close the main window while keeping the menu bar available.
		CloseWindow,
		/// Minimize the active main window.
		Minimize,
		/// Hide the application windows.
		Hide,
		/// Hide other applications.
		HideOthers,
	]
);

fn hide_main_window(cx: &mut App) {
	#[cfg(target_os = "macos")]
	{
		let _ = cx;
		order_out_native_windows();
	}
	#[cfg(not(target_os = "macos"))]
	cx.hide();
}

fn install_application_menu(cx: &mut App) {
	use gpui::{KeyBinding, Menu, MenuItem, SystemMenuType};
	cx.bind_keys([
		KeyBinding::new("cmd-q", Quit, None),
		KeyBinding::new("cmd-w", CloseWindow, None),
		KeyBinding::new("cmd-m", Minimize, None),
		KeyBinding::new("cmd-h", Hide, None),
		KeyBinding::new("alt-cmd-h", HideOthers, None),
	]);
	cx.on_action(|_: &Quit, cx| cx.quit());
	cx.on_action(|_: &CloseWindow, cx| hide_main_window(cx));
	cx.on_action(|_: &Hide, cx| cx.hide());
	cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
	cx.on_action(|_: &Minimize, cx| {
		if let Some(window) = cx.active_window() {
			let _ = window.update(cx, |_, window, _| window.minimize_window());
		}
	});
	cx.set_menus([
		Menu::new("Decodex").items([
			MenuItem::os_submenu("Services", SystemMenuType::Services),
			MenuItem::separator(),
			MenuItem::action("Hide Decodex", Hide),
			MenuItem::action("Hide Others", HideOthers),
			MenuItem::separator(),
			MenuItem::action("Quit Decodex", Quit),
		]),
		Menu::new("File").items([MenuItem::action("Close Window", CloseWindow)]),
		Menu::new("Edit").items([
			MenuItem::action("Cut", composer_input::Cut),
			MenuItem::action("Copy", composer_input::Copy),
			MenuItem::action("Paste", composer_input::Paste),
			MenuItem::action("Select All", composer_input::SelectAll),
		]),
		Menu::new("Window").items([MenuItem::action("Minimize", Minimize)]),
	]);
}
