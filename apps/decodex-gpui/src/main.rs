//! Production Decodex GPUI macOS composition root.

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
mod desktop_settings;
mod factory_surface;
mod health_query;
#[cfg_attr(
	not(test),
	allow(
		dead_code,
		reason = "XY-1429 pager controls are composed by the later Conversation destination"
	)
)]
mod history_pager;
mod programs;
mod native_menu_bar;
mod program_graph;
mod quick_tasks;
mod settings_surface;
mod shell;
mod ui_theme;
mod work_items;

#[cfg(target_os = "macos")]
use objc2 as _;
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
		let profile = ClientProfile::load_default(None);
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
						Shell::new(window, cx, initial_connection).with_account_login(account_login)
					})
				},
			)
			.expect("open the Decodex production window");

		if let Some(lifecycle) = lifecycle {
			shell::retain_lifecycle(window, lifecycle, cx);
		}
		window
			.update(cx, |_, window, cx| {
				window.on_window_should_close(cx, |_, cx| {
					cx.hide();
					false
				});
			})
			.expect("install the Decodex close-to-background behavior");
		let launched_as_login_item = window
			.entity(cx)
			.is_ok_and(|shell| shell.read(cx).was_launched_as_login_item(cx));
		main_window.borrow_mut().replace(window);
		if launched_as_login_item {
			#[cfg(target_os = "macos")]
			order_out_native_windows();
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
	use objc2::MainThreadMarker;
	use objc2_app_kit::NSApplication;

	let main_thread = MainThreadMarker::new().expect("GPUI application callback runs on main thread");
	NSApplication::sharedApplication(main_thread).activate();
}

#[cfg(target_os = "macos")]
fn order_out_native_windows() {
	use objc2::MainThreadMarker;
	use objc2_app_kit::NSApplication;

	let main_thread = MainThreadMarker::new().expect("GPUI application callback runs on main thread");
	let application = NSApplication::sharedApplication(main_thread);
	for window in application.windows().iter() {
		window.orderOut(None);
	}
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
