use base64::Engine as _;

use crate::orchestrator::operator_http::{self, STANDARD, Sha1};

pub(crate) fn operator_dashboard_websocket_response_headers(
	request: &[u8],
) -> std::result::Result<Vec<u8>, Vec<u8>> {
	let request = String::from_utf8_lossy(request);
	let Some(upgrade) = operator_http::operator_http_header_value(&request, "Upgrade") else {
		return Err(websocket_upgrade_required_response());
	};
	let Some(connection) = operator_http::operator_http_header_value(&request, "Connection") else {
		return Err(websocket_upgrade_required_response());
	};
	let Some(version) =
		operator_http::operator_http_header_value(&request, "Sec-WebSocket-Version")
	else {
		return Err(websocket_upgrade_required_response());
	};
	let Some(key) = operator_http::operator_http_header_value(&request, "Sec-WebSocket-Key") else {
		return Err(websocket_upgrade_required_response());
	};

	if !upgrade.eq_ignore_ascii_case("websocket")
		|| !operator_http::operator_http_header_contains_token(connection, "upgrade")
		|| version != "13"
	{
		return Err(websocket_upgrade_required_response());
	}

	let accept_key = websocket_accept_key(key);
	let response = format!(
		"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept_key}\r\n\r\n"
	);

	Ok(response.into_bytes())
}

pub(crate) fn websocket_accept_key(key: &str) -> String {
	const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

	let mut hasher = <Sha1 as sha1::Digest>::new();

	sha1::Digest::update(&mut hasher, key.as_bytes());
	sha1::Digest::update(&mut hasher, WEBSOCKET_GUID.as_bytes());

	STANDARD.encode(sha1::Digest::finalize(hasher))
}

pub(crate) fn websocket_upgrade_required_response() -> Vec<u8> {
	operator_http::http_response_bytes_with_headers(
		"426 Upgrade Required",
		"text/plain; charset=utf-8",
		&[("Upgrade", String::from("websocket"))],
		b"websocket upgrade required",
	)
}
