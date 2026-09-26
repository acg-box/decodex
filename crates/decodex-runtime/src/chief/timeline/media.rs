//! Resolve exact native media and keep each byte chunk bound to the same source and content.
use crate::chief_usage_estimate::{Source, SourceKey};
use base64::{Engine, engine::general_purpose::STANDARD};
use decodex_protocol::{
	CHIEF_MEDIA_CHUNK_BYTES, ChiefMediaRequest, ChiefMediaResult as Result, EntityId,
	MAX_CHIEF_MEDIA_BYTES,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(crate) async fn read<F, Fut>(
	source: F,
	directory: impl Fn(&SourceKey) -> Option<String>,
	request: &ChiefMediaRequest,
) -> Result
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
{
	if request.offset as usize >= MAX_CHIEF_MEDIA_BYTES
		|| (request.offset > 0 && request.fingerprint.is_none())
	{
		return Result::Unavailable;
	}
	let Some(before) = source().await else { return Result::Unavailable };
	if before.key.work != request.work_id.as_str()
		|| before.key.thread != request.thread_id.as_str()
	{
		return Result::Unavailable;
	}
	let response = tokio::time::timeout(std::time::Duration::from_secs(25), async {
		let history = before
			.client
			.thread_read_turn(request.thread_id.as_str(), request.turn_id.as_str())
			.await
			.map_err(client_error)?;
		let item = super::promotions::exact_item(
			&history,
			request.thread_id.as_str(),
			request.turn_id.as_str(),
			request.item_id.as_str(),
		)
		.ok_or(Result::Unavailable)?;
		let media = locate(item, request.index as usize)?;
		resolve(media, directory(&before.key).as_deref()).await
	})
	.await;
	let Some(after) = source().await else { return Result::Unavailable };
	if before.key != after.key {
		return Result::Unavailable;
	}
	match response {
		Ok(Ok((mime, bytes))) => chunk(&before.key, request, mime, bytes),
		Ok(Err(result)) => result,
		Err(_) => Result::Unavailable,
	}
}

enum Media<'a> {
	Uri(&'a str),
	Encoded(&'a str, &'a str),
	Local(&'a str),
}

fn locate(item: &Value, index: usize) -> std::result::Result<Media<'_>, Result> {
	let string = |value: &Value| value.as_str().filter(|text| !text.is_empty()).is_some();
	match item["type"].as_str() {
		Some("userMessage") => {
			let part = item["content"]
				.as_array()
				.and_then(|parts| parts.get(index))
				.ok_or(Result::Unavailable)?;
			match part["type"].as_str() {
				Some("image" | "audio") if string(&part["url"]) =>
					Ok(Media::Uri(part["url"].as_str().expect("checked"))),
				Some("localImage" | "localAudio") if string(&part["path"]) =>
					Ok(Media::Local(part["path"].as_str().expect("checked"))),
				_ => Err(Result::Unsupported),
			}
		},
		Some("dynamicToolCall") => {
			let part = item["contentItems"]
				.as_array()
				.and_then(|parts| parts.get(index))
				.ok_or(Result::Unavailable)?;
			let field = match part["type"].as_str() {
				Some("inputImage") => "imageUrl",
				Some("inputAudio") => "audioUrl",
				_ => return Err(Result::Unsupported),
			};
			Ok(Media::Uri(part[field].as_str().ok_or(Result::Unavailable)?))
		},
		Some("functionCallOutput") => {
			let part = item["output"]
				.as_array()
				.and_then(|parts| parts.get(index))
				.ok_or(Result::Unavailable)?;
			let field = match part["type"].as_str() {
				Some("input_image") => "image_url",
				Some("input_audio") => "audio_url",
				_ => return Err(Result::Unsupported),
			};
			Ok(Media::Uri(part[field].as_str().ok_or(Result::Unavailable)?))
		},
		Some("mcpToolCall") => {
			let part = item["result"]["content"]
				.as_array()
				.and_then(|parts| parts.get(index))
				.ok_or(Result::Unavailable)?;
			if !matches!(part["type"].as_str(), Some("image" | "audio")) {
				return Err(Result::Unsupported);
			}
			Ok(Media::Encoded(
				part["mimeType"].as_str().ok_or(Result::Unavailable)?,
				part["data"].as_str().ok_or(Result::Unavailable)?,
			))
		},
		// The public imageView path omits executor identity. A local app-server can
		// read remote environments; never substitute a same-named service-host file.
		Some("imageView") => Err(Result::Unsupported),
		Some("imageGeneration") if index == 0 => {
			// savedPath can belong to a remote executor. The native result bytes
			// retain exact item ownership without interpreting that path locally.
			let data = item["result"]
				.as_str()
				.filter(|data| !data.is_empty())
				.ok_or(Result::Unsupported)?;
			Ok(Media::Encoded("image/png", data))
		},
		_ => Err(Result::Unsupported),
	}
}

async fn resolve(
	media: Media<'_>,
	directory: Option<&str>,
) -> std::result::Result<(String, Vec<u8>), Result> {
	match media {
		Media::Encoded(mime, data) => decode(mime, data),
		Media::Uri(uri) => {
			let data = uri.strip_prefix("data:").ok_or(Result::Unsupported)?;
			let (header, encoded) = data.split_once(',').ok_or(Result::Unavailable)?;
			let mime = header.strip_suffix(";base64").ok_or(Result::Unsupported)?;
			decode(mime, encoded)
		},
		Media::Local(path) => {
			let original = std::path::Path::new(path);
			if original.is_absolute() {
				return local_media(path).await;
			}
			// Relative user input is interpreted by the admitted native process,
			// not by this service or a child thread's configured directory.
			let base = directory
				.map(std::path::Path::new)
				.filter(|base| base.is_absolute())
				.ok_or(Result::Unavailable)?;
			let resolved = base.join(original);
			local_media(resolved.to_str().ok_or(Result::Unavailable)?).await
		},
	}
}

async fn local_media(path: &str) -> std::result::Result<(String, Vec<u8>), Result> {
	let path = path.to_owned();
	tokio::task::spawn_blocking(move || local_media_sync(&path))
		.await
		.map_err(|_| Result::Unavailable)?
}

fn local_media_sync(path: &str) -> std::result::Result<(String, Vec<u8>), Result> {
	use std::io::Read;
	#[cfg(unix)] use std::os::unix::fs::OpenOptionsExt;
	// The admitted Codex child runs on this service host (account_launch/chief_process).
	// Native fs/readFile returns an unbounded base64 frame; fs/getMetadata has no size.
	// Read only the path recovered from the exact native item, never a UI-supplied path.
	if path.len() > 4096 || path.contains('\0') || !std::path::Path::new(path).is_absolute() {
		return Err(Result::Unavailable);
	}
	let mut options = std::fs::OpenOptions::new();
	options.read(true);
	#[cfg(unix)]
	options.custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
	let file = options.open(path).map_err(|_| Result::Unavailable)?;
	let metadata = file.metadata().map_err(|_| Result::Unavailable)?;
	if !metadata.is_file() {
		return Err(Result::Unsupported);
	}
	if metadata.len() > MAX_CHIEF_MEDIA_BYTES as u64 {
		return Err(Result::CapacityExceeded);
	}
	let mut bytes = Vec::new();
	file.take((MAX_CHIEF_MEDIA_BYTES + 1) as u64)
		.read_to_end(&mut bytes)
		.map_err(|_| Result::Unavailable)?;
	if bytes.len() > MAX_CHIEF_MEDIA_BYTES {
		return Err(Result::CapacityExceeded);
	}
	let mime = sniff(&bytes).ok_or(Result::Unsupported)?;
	Ok((mime.into(), bytes))
}

fn client_error(error: decodex_codex::app_server_client::ClientError) -> Result {
	use decodex_codex::app_server_client::ClientError;
	match error {
		ClientError::CapacityExceeded
		| ClientError::FrameTooLarge
		| ClientError::RequestTooLarge
		| ClientError::RequestQueueFull => Result::CapacityExceeded,
		ClientError::Remote(error) if error.code == -32601 => Result::Unsupported,
		_ => Result::Unavailable,
	}
}

fn decode(mime: &str, data: &str) -> std::result::Result<(String, Vec<u8>), Result> {
	if !matches!(
		mime,
		"image/png"
			| "image/jpeg"
			| "image/webp"
			| "image/gif"
			| "audio/wav"
			| "audio/mpeg"
			| "audio/ogg"
			| "audio/mp4"
			| "audio/webm"
	) {
		return Err(Result::Unsupported);
	}
	Ok((mime.into(), decode_bytes(data)?))
}

fn decode_bytes(data: &str) -> std::result::Result<Vec<u8>, Result> {
	if data.len() > MAX_CHIEF_MEDIA_BYTES.div_ceil(3) * 4 {
		return Err(Result::CapacityExceeded);
	}
	let bytes = STANDARD.decode(data).map_err(|_| Result::Unavailable)?;
	if bytes.len() > MAX_CHIEF_MEDIA_BYTES {
		return Err(Result::CapacityExceeded);
	}
	if bytes.is_empty() {
		return Err(Result::Unavailable);
	}
	Ok(bytes)
}

fn sniff(bytes: &[u8]) -> Option<&'static str> {
	if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
		Some("image/png")
	} else if bytes.starts_with(b"\xff\xd8\xff") {
		Some("image/jpeg")
	} else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
		Some("image/gif")
	} else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
		Some("image/webp")
	} else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
		Some("audio/wav")
	} else if bytes.starts_with(b"OggS") {
		Some("audio/ogg")
	} else if bytes.starts_with(b"ID3") {
		Some("audio/mpeg")
	} else {
		None
	}
}

fn chunk(key: &SourceKey, request: &ChiefMediaRequest, mime: String, bytes: Vec<u8>) -> Result {
	let mut hash = Sha256::new();
	// Structured fields bind every source transition and the exact native attachment.
	hash.update(
		serde_json::to_vec(&json!([
			key.generation.as_str(),
			key.account.as_str(),
			key.revision,
			key.history_revision,
			key.work,
			key.thread,
			request.turn_id,
			request.item_id,
			request.index,
			mime
		]))
		.expect("JSON values serialize"),
	);
	hash.update(&bytes);
	let digest: String = hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
	if request.fingerprint.as_ref().is_some_and(|expected| expected.as_str() != digest) {
		return Result::Unavailable;
	}
	let start = request.offset as usize;
	if start >= bytes.len() {
		return Result::Unavailable;
	}
	let end = (start + CHIEF_MEDIA_CHUNK_BYTES).min(bytes.len());
	let (Ok(account_id), Ok(fingerprint)) =
		(EntityId::new(key.account.as_str()), EntityId::new(digest))
	else {
		return Result::Unavailable;
	};
	Result::Available {
		request: Box::new(request.clone()),
		account_id,
		fingerprint,
		mime_type: mime,
		total_bytes: bytes.len() as u32,
		bytes: bytes[start..end].to_vec(),
	}
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod tests;
