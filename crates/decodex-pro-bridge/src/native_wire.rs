//! Chrome Native Messaging framing. Large CDP packets use bounded JSON fragments.

use std::io;

use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

const FRAME_LIMIT: usize = 1024 * 1024;
const FRAGMENT_SIZE: usize = 96 * 1024;
const PACKET_LIMIT: usize = 32 * 1024 * 1024;

pub(crate) async fn read_packet(read: &mut (impl AsyncRead + Unpin)) -> io::Result<Value> {
	let mut assembled = String::new();
	let mut expected = 0;
	loop {
		let size = read.read_u32_le().await? as usize;
		if size == 0 || size > FRAME_LIMIT {
			return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid native frame size"));
		}
		let mut bytes = vec![0; size];
		read.read_exact(&mut bytes).await?;
		let value: Value = serde_json::from_slice(&bytes)?;
		if value["kind"] != "fragment" {
			if expected != 0 {
				return Err(io::Error::new(io::ErrorKind::InvalidData, "incomplete native packet"));
			}
			return Ok(value);
		}
		let text = value["text"].as_str().ok_or_else(|| {
			io::Error::new(io::ErrorKind::InvalidData, "missing native fragment text")
		})?;
		if value["index"].as_u64() != Some(expected) || assembled.len() + text.len() > PACKET_LIMIT
		{
			return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid native fragment"));
		}
		assembled.push_str(text);
		expected += 1;
		if value["last"] == true {
			return Ok(serde_json::from_str(&assembled)?);
		}
	}
}

pub(crate) async fn write_packet(
	write: &mut (impl AsyncWrite + Unpin),
	value: &Value,
) -> io::Result<()> {
	let packet = serde_json::to_string(value)?;
	if packet.len() > PACKET_LIMIT {
		return Err(io::Error::new(io::ErrorKind::InvalidInput, "native packet exceeds 32 MiB"));
	}
	if packet.len() <= FRAGMENT_SIZE {
		write_frame(write, packet.as_bytes()).await?;
	} else {
		let mut start = 0;
		let mut index = 0;
		while start < packet.len() {
			let mut end = (start + FRAGMENT_SIZE).min(packet.len());
			while !packet.is_char_boundary(end) {
				end -= 1;
			}
			let frame = serde_json::to_vec(&json!({
				"kind":"fragment","index":index,"last":end == packet.len(),
				"text":&packet[start..end]
			}))?;
			write_frame(write, &frame).await?;
			start = end;
			index += 1;
		}
	}
	write.flush().await
}

async fn write_frame(write: &mut (impl AsyncWrite + Unpin), bytes: &[u8]) -> io::Result<()> {
	write.write_all(&(bytes.len() as u32).to_le_bytes()).await?;
	write.write_all(bytes).await
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn large_unicode_context_round_trips_below_chrome_frame_limit() {
		let value = json!({"id":1,"params":{"text":"研究🦀\n\"".repeat(400_000)}});
		let mut encoded = Vec::new();
		write_packet(&mut encoded, &value).await.unwrap();
		let mut frames = encoded.as_slice();
		let mut count = 0;
		while !frames.is_empty() {
			let length = u32::from_le_bytes(frames[..4].try_into().unwrap()) as usize;
			assert!(length < FRAME_LIMIT);
			frames = &frames[4 + length..];
			count += 1;
		}
		assert!(count > 10);
		assert_eq!(read_packet(&mut encoded.as_slice()).await.unwrap(), value);
	}

	#[tokio::test]
	async fn rejects_out_of_order_fragments_and_oversized_frames() {
		let mut bad = Vec::new();
		write_packet(&mut bad, &json!({"kind":"fragment","index":1,"text":"{}","last":true}))
			.await
			.unwrap();
		assert_eq!(
			read_packet(&mut bad.as_slice()).await.unwrap_err().kind(),
			io::ErrorKind::InvalidData
		);
		let size = ((FRAME_LIMIT + 1) as u32).to_le_bytes();
		assert_eq!(
			read_packet(&mut size.as_slice()).await.unwrap_err().kind(),
			io::ErrorKind::InvalidData
		);
	}
}
