//! Production Decodex GPUI macOS composition root.

mod accounts;
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
mod quick_tasks;
mod settings_surface;
mod shell;
mod ui_theme;
mod work_items;

use objc2_app_kit as _;
use objc2_foundation as _;

use gpui::{
	App, AppContext as _, Bounds, WindowBackgroundAppearance, WindowBounds, WindowOptions, point,
	px, size,
};
use gpui_platform::application;

use decodex_protocol::ClientProfile;

use crate::{
	client_lifecycle::{
		ClientLifecycle, CompatibilityReason, ConnectionView, QuarantineReason, QuarantineRecovery,
	},
	shell::Shell,
};

fn main() {
	application().run(|cx: &mut App| {
		shell::bind_keys(cx);
		let (initial_connection, lifecycle) = compose_lifecycle();
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
				|window, cx| cx.new(|cx| Shell::new(window, cx, initial_connection)),
			)
			.expect("open the Decodex production window");

		if let Some(lifecycle) = lifecycle {
			shell::retain_lifecycle(window, lifecycle, cx);
		}
		window
			.update(cx, |_, window, _| window.activate_window())
			.expect("activate after the accessibility adapter is installed");
		cx.activate(true);
	});
}

fn compose_lifecycle() -> (ConnectionView, Option<ClientLifecycle>) {
	let profile = match ClientProfile::load_default(None) {
		Ok(profile) => profile,
		Err(failure) => {
			return (ConnectionView::Incompatible(CompatibilityReason::Startup(failure)), None);
		},
	};
	let config = match profile.retained_session_config() {
		Ok(config) => config,
		Err(_) => {
			return (ConnectionView::Incompatible(CompatibilityReason::InvalidEndpoint), None);
		},
	};
	match ClientLifecycle::production(config) {
		Ok(lifecycle) => (lifecycle.view(), Some(lifecycle)),
		Err(_) => (
			ConnectionView::Quarantined {
				reason: QuarantineReason::CacheRootUnsafe,
				recovery: QuarantineRecovery::OperatorRequired,
			},
			None,
		),
	}
}
