use std::{
	io::{ErrorKind, Read as _, Write as _},
	net::TcpStream,
};

use serde_json::Value;

use crate::orchestrator::operator_http::{
	self, DASHBOARD_WS_MESSAGE_MAX_BYTES, DashboardClientFrame, Result, eyre,
};

pub(crate) fn write_dashboard_websocket_event(
	stream: &mut TcpStream,
	event_type: &'static str,
	payload: &Value,
) -> Result<()> {
	stream.write_all(&dashboard_websocket_message(event_type, payload)?)?;

	Ok(())
}

pub(crate) fn websocket_frame(opcode: u8, payload: &[u8]) -> Result<Vec<u8>> {
	let mut frame = Vec::with_capacity(payload.len().saturating_add(10));

	frame.push(0x80 | opcode);

	match payload.len() {
		length @ 0..=125 => frame.push(length as u8),
		length @ 126..=65_535 => {
			frame.push(126);
			frame.extend_from_slice(&(length as u16).to_be_bytes());
		},
		length => {
			frame.push(127);

			let length = u64::try_from(length)
				.map_err(|error| eyre::eyre!("WebSocket frame payload length overflow: {error}"))?;

			frame.extend_from_slice(&length.to_be_bytes());
		},
	}

	frame.extend_from_slice(payload);

	Ok(frame)
}

pub(crate) fn websocket_ping_frame() -> Vec<u8> {
	vec![0x89, 0]
}

pub(crate) fn read_dashboard_websocket_client_frames(
	stream: &mut TcpStream,
	buffer: &mut Vec<u8>,
) -> Result<Vec<DashboardClientFrame>> {
	let mut frames = Vec::new();
	let mut chunk = [0_u8; 2_048];

	loop {
		match stream.read(&mut chunk) {
			Ok(0) => {
				frames.push(DashboardClientFrame::Close);

				break;
			},
			Ok(bytes_read) => {
				buffer.extend_from_slice(&chunk[..bytes_read]);

				while let Some(frame) = parse_dashboard_websocket_client_frame(buffer)? {
					frames.push(frame);
				}
			},
			Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
				break;
			},
			Err(error) if error.kind() == ErrorKind::Interrupted => continue,
			Err(error) => return Err(error.into()),
		}
	}

	Ok(frames)
}

pub(crate) fn parse_dashboard_websocket_client_frame(
	buffer: &mut Vec<u8>,
) -> Result<Option<DashboardClientFrame>> {
	if buffer.len() < 2 {
		return Ok(None);
	}

	let fin = buffer[0] & 0x80 != 0;
	let opcode = buffer[0] & 0x0f;
	let masked = buffer[1] & 0x80 != 0;
	let payload_length_marker = buffer[1] & 0x7f;
	let mut offset = 2_usize;
	let payload_length = match payload_length_marker {
		length @ 0..=125 => usize::from(length),
		126 => {
			if buffer.len() < offset + 2 {
				return Ok(None);
			}

			let length = usize::from(u16::from_be_bytes([buffer[offset], buffer[offset + 1]]));

			offset += 2;

			length
		},
		127 => {
			if buffer.len() < offset + 8 {
				return Ok(None);
			}

			let length = u64::from_be_bytes([
				buffer[offset],
				buffer[offset + 1],
				buffer[offset + 2],
				buffer[offset + 3],
				buffer[offset + 4],
				buffer[offset + 5],
				buffer[offset + 6],
				buffer[offset + 7],
			]);

			offset += 8;

			usize::try_from(length)
				.map_err(|error| eyre::eyre!("WebSocket client frame length overflow: {error}"))?
		},
		_ => unreachable!("websocket payload length marker is masked to seven bits"),
	};

	if payload_length > DASHBOARD_WS_MESSAGE_MAX_BYTES {
		eyre::bail!("WebSocket client frame exceeded the operator message limit.");
	}
	if !fin {
		eyre::bail!("Fragmented operator WebSocket messages are not supported.");
	}
	if !masked {
		eyre::bail!("Dashboard WebSocket client frame was not masked.");
	}
	if buffer.len() < offset + 4 {
		return Ok(None);
	}

	let mask = [buffer[offset], buffer[offset + 1], buffer[offset + 2], buffer[offset + 3]];

	offset += 4;

	let frame_end = offset
		.checked_add(payload_length)
		.ok_or_else(|| eyre::eyre!("WebSocket client frame length overflow."))?;

	if buffer.len() < frame_end {
		return Ok(None);
	}

	let mut payload = buffer[offset..frame_end].to_vec();

	for (index, byte) in payload.iter_mut().enumerate() {
		*byte ^= mask[index % mask.len()];
	}

	buffer.drain(..frame_end);

	let frame = match opcode {
		0x1 => DashboardClientFrame::Text(payload),
		0x8 => DashboardClientFrame::Close,
		0x9 => DashboardClientFrame::Ping(payload),
		0xA => DashboardClientFrame::Pong,
		_ => return Ok(None),
	};

	Ok(Some(frame))
}

pub(crate) fn dashboard_websocket_message(event_type: &str, payload: &Value) -> Result<Vec<u8>> {
	let message = serde_json::to_vec(&operator_http::json!({
		"type": event_type,
		"payload": payload,
	}))?;

	websocket_frame(0x1, &message)
}
