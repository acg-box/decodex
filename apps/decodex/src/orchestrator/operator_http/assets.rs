//! Static dashboard assets and HTTP constants.

mod html;

pub(super) use self::html::OPERATOR_DASHBOARD_HTML;

use std::time::Duration;

pub(crate) const DASHBOARD_MAX_WEBSOCKET_CLIENTS: usize = 64;

pub(in crate::orchestrator::operator_http) const OPERATOR_HTTP_READ_TIMEOUT: Duration =
	Duration::from_millis(250);
pub(in crate::orchestrator::operator_http) const DASHBOARD_RUN_ACTIVITY_FINGERPRINT_VOLATILE_FIELDS: &[&str] =
	&["idle_for_seconds", "protocol_idle_for_seconds", "current_elapsed_seconds", "wall_seconds"];
pub(in crate::orchestrator::operator_http) const OPERATOR_DASHBOARD_ICON_PNG: &[u8] =
	include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/orchestrator/assets/icon.png"));
pub(in crate::orchestrator::operator_http) const OPERATOR_DASHBOARD_LOGO_ICO: &[u8] =
	include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/orchestrator/assets/logo.ico"));
pub(in crate::orchestrator::operator_http) const OPERATOR_DASHBOARD_LOGO_TOUCH_PNG: &[u8] =
	include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/orchestrator/assets/logo-touch.png"));
