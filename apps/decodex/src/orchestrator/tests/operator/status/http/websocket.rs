mod connection;
mod controls;
mod run_activity;

use std::io::{Read, Write};

use crate::orchestrator::tests::operator::status::http::{
	Instant, OPERATOR_DASHBOARD_TEST_TIMEOUT, SocketAddr, TcpStream, Value, orchestrator,
};

pub(super) fn websocket_text_payload(frame: &[u8]) -> Option<(&[u8], usize)> {
	if frame.len() < 2 || frame[0] != 0x81 {
		return None;
	}

	let payload_length_marker = frame[1] & 0x7f;
	let (payload_offset, payload_length): (usize, usize) = match payload_length_marker {
		length @ 0..=125 => (2_usize, usize::from(length)),
		126 => {
			if frame.len() < 4 {
				return None;
			}

			(4_usize, usize::from(u16::from_be_bytes([frame[2], frame[3]])))
		},
		127 => {
			if frame.len() < 10 {
				return None;
			}

			let length = u64::from_be_bytes([
				frame[2], frame[3], frame[4], frame[5], frame[6], frame[7], frame[8], frame[9],
			]);
			let Ok(length) = usize::try_from(length) else {
				return None;
			};

			(10_usize, length)
		},
		_ => return None,
	};
	let payload_end = payload_offset.checked_add(payload_length)?;

	(frame.len() >= payload_end).then(|| (&frame[payload_offset..payload_end], payload_end))
}

pub(super) fn open_dashboard_websocket_client(address: SocketAddr) -> (TcpStream, String, Vec<u8>) {
	let mut client = TcpStream::connect(address).expect("client should connect");
	let mut bytes = Vec::new();
	let mut buffer = [0_u8; 2_048];

	client
		.set_read_timeout(Some(OPERATOR_DASHBOARD_TEST_TIMEOUT))
		.expect("client timeout should configure");
	client
		.write_all(
			format!(
				"GET {} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
				orchestrator::OPERATOR_DASHBOARD_WS_ENDPOINT_PATH
			)
			.as_bytes(),
		)
		.expect("client should write request");

	let header_end = loop {
		let header_bytes = client.read(&mut buffer).expect("client should read stream headers");

		bytes.extend_from_slice(&buffer[..header_bytes]);

		if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
			break index + 4;
		}
	};
	let response =
		String::from_utf8(bytes[..header_end].to_vec()).expect("headers should be utf-8");
	let frame = bytes[header_end..].to_vec();

	(client, response, frame)
}

pub(super) fn read_websocket_json_until(
	client: &mut TcpStream,
	frame: &mut Vec<u8>,
	matches: impl Fn(&Value) -> bool,
) -> Value {
	let deadline = Instant::now() + OPERATOR_DASHBOARD_TEST_TIMEOUT;
	let mut buffer = [0_u8; 2_048];

	loop {
		assert!(Instant::now() < deadline, "websocket should send expected event");

		if frame.is_empty() {
			let event_bytes = client.read(&mut buffer).expect("client should read websocket event");

			frame.extend_from_slice(&buffer[..event_bytes]);
		}

		if let Some((payload, consumed)) = websocket_text_payload(frame) {
			let payload: Value =
				serde_json::from_slice(payload).expect("event payload should be json");

			frame.drain(..consumed);

			if matches(&payload) {
				return payload;
			}
		} else {
			let event_bytes =
				client.read(&mut buffer).expect("client should continue websocket event");

			frame.extend_from_slice(&buffer[..event_bytes]);
		}
	}
}

pub(super) fn websocket_client_text_frame(payload: &str) -> Vec<u8> {
	let payload = payload.as_bytes();
	let mask = [0x11_u8, 0x22, 0x33, 0x44];
	let mut frame = Vec::new();

	frame.push(0x81);

	match payload.len() {
		length @ 0..=125 => frame.push(0x80 | length as u8),
		length @ 126..=65_535 => {
			frame.push(0x80 | 126);
			frame.extend_from_slice(&(length as u16).to_be_bytes());
		},
		length => {
			frame.push(0x80 | 127);
			frame.extend_from_slice(&(length as u64).to_be_bytes());
		},
	}

	frame.extend_from_slice(&mask);
	frame.extend(payload.iter().enumerate().map(|(index, byte)| byte ^ mask[index % mask.len()]));

	frame
}
