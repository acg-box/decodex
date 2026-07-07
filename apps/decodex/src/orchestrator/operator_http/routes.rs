use crate::orchestrator::operator_http::{
	self, OPERATOR_ACCOUNTS_ENDPOINT_PATH, OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH,
	OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH, OPERATOR_DASHBOARD_ENDPOINT_PATH,
	OPERATOR_DASHBOARD_WS_ENDPOINT_PATH, OPERATOR_LANE_INSPECT_ENDPOINT_PATH,
	OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH, OPERATOR_LANE_STEER_ALIAS_ENDPOINT_PATH,
	OPERATOR_LANE_STEER_ENDPOINT_PATH, OPERATOR_LINEAR_SCAN_ENDPOINT_PATH,
	OPERATOR_LIVE_ENDPOINT_PATH, OperatorRequestRoute,
	assets::{
		OPERATOR_DASHBOARD_HTML, OPERATOR_DASHBOARD_ICON_PNG, OPERATOR_DASHBOARD_LOGO_ICO,
		OPERATOR_DASHBOARD_LOGO_TOUCH_PNG,
	},
};

pub(super) fn build_operator_state_http_response_for_route(route: OperatorRequestRoute) -> Vec<u8> {
	match route {
		OperatorRequestRoute::Dashboard => operator_http::http_response_bytes(
			"200 OK",
			"text/html; charset=utf-8",
			OPERATOR_DASHBOARD_HTML.as_bytes(),
		),
		OperatorRequestRoute::DashboardIconPng => {
			operator_http::http_response_bytes("200 OK", "image/png", OPERATOR_DASHBOARD_ICON_PNG)
		},
		OperatorRequestRoute::DashboardLogoIco => operator_http::http_response_bytes(
			"200 OK",
			"image/x-icon",
			OPERATOR_DASHBOARD_LOGO_ICO,
		),
		OperatorRequestRoute::DashboardLogoTouchPng => operator_http::http_response_bytes(
			"200 OK",
			"image/png",
			OPERATOR_DASHBOARD_LOGO_TOUCH_PNG,
		),
		OperatorRequestRoute::DashboardWs => operator_http::websocket_upgrade_required_response(),
		OperatorRequestRoute::AppSnapshot => {
			operator_http::http_response_bytes("200 OK", "application/json", b"{}")
		},
		OperatorRequestRoute::LinearScan => operator_http::http_response_bytes(
			"405 Method Not Allowed",
			"text/plain; charset=utf-8",
			b"method not allowed",
		),
		OperatorRequestRoute::LaneInspect
		| OperatorRequestRoute::LaneInterrupt
		| OperatorRequestRoute::LaneSteer => operator_http::http_response_bytes(
			"405 Method Not Allowed",
			"text/plain; charset=utf-8",
			b"method not allowed",
		),
		OperatorRequestRoute::Live => {
			operator_http::http_response_bytes("200 OK", "text/plain; charset=utf-8", b"ok")
		},
		OperatorRequestRoute::AccountList { .. }
		| OperatorRequestRoute::AccountSelect
		| OperatorRequestRoute::AccountClear
		| OperatorRequestRoute::AccountLogout
		| OperatorRequestRoute::AccountImport
		| OperatorRequestRoute::AccountUse
		| OperatorRequestRoute::AccountRerollName => operator_http::http_response_bytes(
			"405 Method Not Allowed",
			"text/plain; charset=utf-8",
			b"method not allowed",
		),
	}
}

pub(super) fn parse_operator_state_request_route(
	request: &[u8],
) -> std::result::Result<OperatorRequestRoute, Vec<u8>> {
	let request = String::from_utf8_lossy(request);
	let mut request_line = request.lines();
	let Some(request_line) = request_line.next() else {
		return Err(operator_http::http_response_bytes(
			"400 Bad Request",
			"text/plain; charset=utf-8",
			b"missing request line",
		));
	};
	let mut parts = request_line.split_whitespace();
	let Some(method) = parts.next() else {
		return Err(operator_http::http_response_bytes(
			"400 Bad Request",
			"text/plain; charset=utf-8",
			b"missing method",
		));
	};
	let Some(path) = parts.next() else {
		return Err(operator_http::http_response_bytes(
			"400 Bad Request",
			"text/plain; charset=utf-8",
			b"missing path",
		));
	};
	let path_without_query =
		path.split_once('?').map_or(path, |(path_without_query, _)| path_without_query);
	let query = path.split_once('?').map(|(_, query)| query).unwrap_or_default();
	let normalized_path = path_without_query
		.split_once('#')
		.map_or(path_without_query, |(path_without_fragment, _)| path_without_fragment);

	match (method, normalized_path) {
		("GET", OPERATOR_DASHBOARD_ENDPOINT_PATH | OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH) => {
			Ok(OperatorRequestRoute::Dashboard)
		},
		("GET", "/assets/icon.png") => Ok(OperatorRequestRoute::DashboardIconPng),
		("GET", "/assets/logo.ico") => Ok(OperatorRequestRoute::DashboardLogoIco),
		("GET", "/assets/logo-touch.png") => Ok(OperatorRequestRoute::DashboardLogoTouchPng),
		("GET", OPERATOR_DASHBOARD_WS_ENDPOINT_PATH) => Ok(OperatorRequestRoute::DashboardWs),
		("GET", OPERATOR_LIVE_ENDPOINT_PATH) => Ok(OperatorRequestRoute::Live),
		("GET", OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH) => Ok(OperatorRequestRoute::AppSnapshot),
		("GET", OPERATOR_ACCOUNTS_ENDPOINT_PATH) => Ok(OperatorRequestRoute::AccountList {
			force_refresh: operator_query_has_flag(query, "refresh"),
		}),
		("POST", OPERATOR_LINEAR_SCAN_ENDPOINT_PATH) => Ok(OperatorRequestRoute::LinearScan),
		("GET", OPERATOR_LANE_INSPECT_ENDPOINT_PATH) => Ok(OperatorRequestRoute::LaneInspect),
		("POST", OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH) => Ok(OperatorRequestRoute::LaneInterrupt),
		("POST", OPERATOR_LANE_STEER_ENDPOINT_PATH | OPERATOR_LANE_STEER_ALIAS_ENDPOINT_PATH) => {
			Ok(OperatorRequestRoute::LaneSteer)
		},
		("POST", "/api/accounts/select") => Ok(OperatorRequestRoute::AccountSelect),
		("POST", "/api/accounts/clear") => Ok(OperatorRequestRoute::AccountClear),
		("POST", "/api/accounts/logout") => Ok(OperatorRequestRoute::AccountLogout),
		("POST", "/api/accounts/import") => Ok(OperatorRequestRoute::AccountImport),
		("POST", "/api/accounts/use") => Ok(OperatorRequestRoute::AccountUse),
		("POST", "/api/accounts/reroll-name") => Ok(OperatorRequestRoute::AccountRerollName),
		(
			_,
			OPERATOR_DASHBOARD_ENDPOINT_PATH
			| OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH
			| OPERATOR_DASHBOARD_WS_ENDPOINT_PATH
			| OPERATOR_LIVE_ENDPOINT_PATH
			| OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH
			| OPERATOR_LINEAR_SCAN_ENDPOINT_PATH
			| OPERATOR_LANE_INSPECT_ENDPOINT_PATH
			| OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH
			| OPERATOR_LANE_STEER_ENDPOINT_PATH
			| OPERATOR_LANE_STEER_ALIAS_ENDPOINT_PATH
			| OPERATOR_ACCOUNTS_ENDPOINT_PATH
			| "/api/accounts/select"
			| "/api/accounts/clear"
			| "/api/accounts/logout"
			| "/api/accounts/import"
			| "/api/accounts/use"
			| "/api/accounts/reroll-name",
		) => Err(operator_http::http_response_bytes(
			"405 Method Not Allowed",
			"text/plain; charset=utf-8",
			b"method not allowed",
		)),
		_ => Err(operator_http::http_response_bytes(
			"404 Not Found",
			"text/plain; charset=utf-8",
			b"not found",
		)),
	}
}

pub(super) fn operator_query_has_flag(query: &str, name: &str) -> bool {
	query.split('&').any(|part| {
		let key = part.split_once('=').map_or(part, |(key, _)| key);

		key == name
	})
}
