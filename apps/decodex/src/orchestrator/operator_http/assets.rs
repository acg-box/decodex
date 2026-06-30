//! Static dashboard assets and HTTP constants.

use std::time::Duration;

pub(super) static OPERATOR_DASHBOARD_HTML: std::sync::LazyLock<String> =
	std::sync::LazyLock::new(|| {
		[
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/head.html"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/styles/foundation.css"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/styles/layout.css"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/styles/accounts.css"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/styles/activity.css"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/styles/details.css"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/styles/responsive.css"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/body.html"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/boot.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/formatting.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/preferences.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/render-primitives/labels.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/render-primitives/details.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/render-primitives/dom.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/render-primitives/history.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/render-primitives/lifecycle.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/accounts/selection.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/accounts/identity.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/accounts/usage.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/accounts/profile.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/accounts/pool.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/activity.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/overview.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/lanes.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/app/stream.js"
			)),
			include_str!(concat!(
				env!("CARGO_MANIFEST_DIR"),
				"/src/orchestrator/operator_dashboard/tail.html"
			)),
		]
		.concat()
	});
pub(super) const OPERATOR_DASHBOARD_ICON_PNG: &[u8] =
	include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/orchestrator/assets/icon.png"));
pub(super) const OPERATOR_DASHBOARD_LOGO_ICO: &[u8] =
	include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/orchestrator/assets/logo.ico"));
pub(super) const OPERATOR_DASHBOARD_LOGO_TOUCH_PNG: &[u8] =
	include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/orchestrator/assets/logo-touch.png"));
pub(super) const OPERATOR_HTTP_READ_TIMEOUT: Duration = Duration::from_millis(250);
pub(crate) const DASHBOARD_MAX_WEBSOCKET_CLIENTS: usize = 64;
pub(super) const DASHBOARD_RUN_ACTIVITY_FINGERPRINT_VOLATILE_FIELDS: &[&str] =
	&["idle_for_seconds", "protocol_idle_for_seconds", "current_elapsed_seconds", "wall_seconds"];
