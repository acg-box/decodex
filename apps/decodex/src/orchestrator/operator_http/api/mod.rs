mod account;
mod lane;
mod linear_scan;

#[cfg(test)]
use super::{
	OperatorControlRequests, OperatorRequestRoute, Result,
	build_operator_state_http_response_for_route, parse_operator_state_request_route,
};

pub(crate) use self::lane::{
	build_operator_lane_inspect_http_response, build_operator_lane_interrupt_http_response,
	build_operator_lane_steer_http_response,
};
pub(super) use self::{
	account::{build_operator_account_http_response, operator_request_route_is_account_api},
	linear_scan::build_operator_linear_scan_http_response,
};

#[cfg(test)]
pub(crate) fn build_operator_state_http_response(request: &[u8]) -> Result<Vec<u8>> {
	let control_requests = OperatorControlRequests::default();

	build_operator_state_http_response_with_control_requests(request, &control_requests)
}

#[cfg(test)]
pub(crate) fn build_operator_state_http_response_with_control_requests(
	request: &[u8],
	control_requests: &OperatorControlRequests,
) -> Result<Vec<u8>> {
	let route = match parse_operator_state_request_route(request) {
		Ok(route) => route,
		Err(response) => return Ok(response),
	};

	if operator_request_route_is_account_api(&route) {
		return Ok(build_operator_account_http_response(route, request));
	}
	if route == OperatorRequestRoute::LinearScan {
		return Ok(build_operator_linear_scan_http_response(control_requests, request));
	}

	Ok(build_operator_state_http_response_for_route(route))
}
