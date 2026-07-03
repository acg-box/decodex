//! Static dashboard assets and HTTP constants.

mod branding;
mod constants;
mod html;

pub(super) use self::{
	branding::{
		OPERATOR_DASHBOARD_ICON_PNG, OPERATOR_DASHBOARD_LOGO_ICO, OPERATOR_DASHBOARD_LOGO_TOUCH_PNG,
	},
	constants::{DASHBOARD_RUN_ACTIVITY_FINGERPRINT_VOLATILE_FIELDS, OPERATOR_HTTP_READ_TIMEOUT},
	html::OPERATOR_DASHBOARD_HTML,
};

pub(crate) const DASHBOARD_MAX_WEBSOCKET_CLIENTS: usize = 64;
