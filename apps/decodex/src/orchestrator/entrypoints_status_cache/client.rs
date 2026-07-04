use std::{
	io::{Read as _, Write as _},
	net::{SocketAddr, TcpStream},
	str,
};

use crate::orchestrator::{
	DEFAULT_OPERATOR_LISTEN_ADDRESS, OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH,
	OPERATOR_STATE_HEADER_TERMINATOR, STATUS_OPERATOR_SNAPSHOT_CONNECT_TIMEOUT,
	STATUS_OPERATOR_SNAPSHOT_IO_TIMEOUT,
};

pub(crate) struct StatusSnapshotHttpResponse {
	pub(crate) body: Vec<u8>,
	pub(crate) published_at_unix_epoch: Option<i64>,
}

pub(crate) fn fetch_local_operator_snapshot_response()
-> std::result::Result<StatusSnapshotHttpResponse, String> {
	let address = DEFAULT_OPERATOR_LISTEN_ADDRESS
		.parse::<SocketAddr>()
		.map_err(|error| format!("default operator listener address is invalid: {error}"))?;
	let mut stream = TcpStream::connect_timeout(&address, STATUS_OPERATOR_SNAPSHOT_CONNECT_TIMEOUT)
		.map_err(|error| format!("local operator listener is unavailable at {address}: {error}"))?;

	stream
		.set_read_timeout(Some(STATUS_OPERATOR_SNAPSHOT_IO_TIMEOUT))
		.map_err(|error| format!("failed to set operator snapshot read timeout: {error}"))?;
	stream
		.set_write_timeout(Some(STATUS_OPERATOR_SNAPSHOT_IO_TIMEOUT))
		.map_err(|error| format!("failed to set operator snapshot write timeout: {error}"))?;
	stream
		.write_all(
			format!(
				"GET {OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH} HTTP/1.1\r\nHost: {DEFAULT_OPERATOR_LISTEN_ADDRESS}\r\nConnection: close\r\n\r\n"
			)
			.as_bytes(),
		)
		.map_err(|error| format!("failed to request local operator snapshot: {error}"))?;

	let mut response = Vec::new();

	stream
		.read_to_end(&mut response)
		.map_err(|error| format!("failed to read local operator snapshot: {error}"))?;

	parse_operator_snapshot_http_response(&response)
}

fn parse_operator_snapshot_http_response(
	response: &[u8],
) -> std::result::Result<StatusSnapshotHttpResponse, String> {
	let header_end = response
		.windows(OPERATOR_STATE_HEADER_TERMINATOR.len())
		.position(|window| window == OPERATOR_STATE_HEADER_TERMINATOR)
		.ok_or_else(|| String::from("local operator snapshot response omitted HTTP headers"))?;
	let headers = str::from_utf8(&response[..header_end])
		.map_err(|error| format!("local operator snapshot headers were not UTF-8: {error}"))?;
	let Some(status_line) = headers.lines().next() else {
		return Err(String::from("local operator snapshot response omitted HTTP status"));
	};

	if !status_line.contains(" 200 ") {
		return Err(format!("local operator snapshot request returned `{status_line}`"));
	}

	let published_at_unix_epoch = headers.lines().find_map(|line| {
		line.strip_prefix("X-Decodex-Snapshot-Unix-Epoch: ")
			.and_then(|value| value.trim().parse::<i64>().ok())
	});
	let body = response[header_end + OPERATOR_STATE_HEADER_TERMINATOR.len()..].to_vec();

	if body.is_empty() || body.as_slice() == b"{}" {
		return Err(String::from(
			"local operator listener has not published a status snapshot yet",
		));
	}

	Ok(StatusSnapshotHttpResponse { body, published_at_unix_epoch })
}
