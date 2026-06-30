use super::{
	ErrorKind, OPERATOR_STATE_HEADER_TERMINATOR, OPERATOR_STATE_MAX_REQUEST_BYTES, Read, Result,
	TcpStream, eyre,
};

pub(super) fn operator_http_header_value<'a>(
	request: &'a str,
	header_name: &str,
) -> Option<&'a str> {
	request.lines().skip(1).take_while(|line| !line.trim().is_empty()).find_map(|line| {
		let (name, value) = line.split_once(':')?;

		name.trim().eq_ignore_ascii_case(header_name).then(|| value.trim())
	})
}

pub(super) fn operator_http_header_contains_token(value: &str, token: &str) -> bool {
	value.split(',').any(|candidate| candidate.trim().eq_ignore_ascii_case(token))
}

pub(super) fn read_operator_state_request_headers(stream: &mut TcpStream) -> Result<Vec<u8>> {
	let mut request = Vec::with_capacity(1_024);

	loop {
		if let Some(header_end) = request
			.windows(OPERATOR_STATE_HEADER_TERMINATOR.len())
			.position(|window| window == OPERATOR_STATE_HEADER_TERMINATOR)
		{
			let body_offset = header_end + OPERATOR_STATE_HEADER_TERMINATOR.len();
			let content_length = operator_http_content_length(&request[..body_offset])?;

			if request.len() >= body_offset + content_length {
				return Ok(request);
			}
		}

		if request.len() >= OPERATOR_STATE_MAX_REQUEST_BYTES {
			eyre::bail!("Operator state endpoint request headers exceeded the size limit.");
		}

		let mut buffer = [0_u8; 1_024];

		match stream.read(&mut buffer) {
			Ok(0) => return Ok(request),
			Ok(bytes_read) => request.extend_from_slice(&buffer[..bytes_read]),
			Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
				eyre::bail!("Timed out while reading operator state endpoint request headers.");
			},
			Err(error) => return Err(error.into()),
		}
	}
}

pub(super) fn operator_http_content_length(headers: &[u8]) -> Result<usize> {
	let headers = String::from_utf8_lossy(headers);

	for line in headers.lines().skip(1) {
		let Some((name, value)) = line.split_once(':') else {
			continue;
		};

		if name.trim().eq_ignore_ascii_case("Content-Length") {
			return value.trim().parse::<usize>().map_err(|error| {
				eyre::eyre!("Operator HTTP request Content-Length was invalid: {error}")
			});
		}
	}

	Ok(0)
}

pub(super) fn operator_http_request_body(request: &[u8]) -> Result<&[u8]> {
	let body_offset = request
		.windows(OPERATOR_STATE_HEADER_TERMINATOR.len())
		.position(|window| window == OPERATOR_STATE_HEADER_TERMINATOR)
		.map(|index| index + OPERATOR_STATE_HEADER_TERMINATOR.len())
		.ok_or_else(|| eyre::eyre!("Operator HTTP request omitted header terminator."))?;

	Ok(&request[body_offset..])
}

pub(super) fn operator_http_query_value(request: &[u8], key: &str) -> Result<Option<String>> {
	let request = String::from_utf8_lossy(request);
	let Some(request_line) = request.lines().next() else {
		return Ok(None);
	};
	let Some(path) = request_line.split_whitespace().nth(1) else {
		return Ok(None);
	};
	let Some(query) = path.split_once('?').map(|(_path, query)| query) else {
		return Ok(None);
	};

	for part in query.split('&') {
		let (name, value) = part.split_once('=').unwrap_or((part, ""));

		if name == key {
			return Ok(Some(percent_decode_operator_query_value(value)?));
		}
	}

	Ok(None)
}

pub(super) fn operator_http_query_value_alias(
	request: &[u8],
	primary: &str,
	secondary: &str,
) -> Result<Option<String>> {
	match operator_http_query_value(request, primary)? {
		Some(value) => Ok(Some(value)),
		None => operator_http_query_value(request, secondary),
	}
}

pub(super) fn percent_decode_operator_query_value(value: &str) -> Result<String> {
	let raw = value.as_bytes();
	let mut bytes = Vec::with_capacity(value.len());
	let mut index = 0;

	while index < raw.len() {
		match raw[index] {
			b'+' => {
				bytes.push(b' ');

				index += 1;
			},
			b'%' if index + 2 < raw.len() => {
				let hex = std::str::from_utf8(&raw[index + 1..index + 3])?;
				let byte = u8::from_str_radix(hex, 16)
					.map_err(|error| eyre::eyre!("Invalid percent-encoded query value: {error}"))?;

				bytes.push(byte);

				index += 3;
			},
			byte => {
				bytes.push(byte);

				index += 1;
			},
		}
	}

	String::from_utf8(bytes)
		.map_err(|error| eyre::eyre!("Query parameter was not valid UTF-8: {error}"))
}

pub(super) fn http_response_bytes(status_line: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
	http_response_bytes_with_headers(status_line, content_type, &[], body)
}

pub(super) fn http_response_bytes_with_headers(
	status_line: &str,
	content_type: &str,
	extra_headers: &[(&str, String)],
	body: &[u8],
) -> Vec<u8> {
	let mut response =
		format!("HTTP/1.1 {status_line}\r\nContent-Type: {content_type}\r\n").into_bytes();

	for (header, value) in extra_headers {
		response.extend_from_slice(format!("{header}: {value}\r\n").as_bytes());
	}

	response.extend_from_slice(
		format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes(),
	);
	response.extend_from_slice(body);

	response
}
