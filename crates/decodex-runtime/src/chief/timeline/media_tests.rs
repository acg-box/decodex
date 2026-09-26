use super::*;
use decodex_codex::app_server_client::AppServerClient;
use decodex_core::{AccountId, ProcessGenerationId};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn key() -> SourceKey {
	SourceKey {
		history_revision: 0,
		generation: ProcessGenerationId::new("10000000-0000-4000-8000-000000000001").unwrap(),
		account: AccountId::new("30000000-0000-4000-8000-000000000003").unwrap(),
		revision: 1,
		thread: "thread".into(),
		work: "work".into(),
	}
}
fn request() -> ChiefMediaRequest {
	ChiefMediaRequest {
		work_id: EntityId::new("work").unwrap(),
		thread_id: EntityId::new("thread").unwrap(),
		turn_id: EntityId::new("turn").unwrap(),
		item_id: EntityId::new("item").unwrap(),
		index: 1,
		offset: 0,
		fingerprint: None,
	}
}

#[test]
fn continuation_rejects_changed_bytes_binding_and_out_of_range_offsets() {
	let bytes = vec![255; CHIEF_MEDIA_CHUNK_BYTES + 17];
	let mut request = request();
	let first = chunk(&key(), &request, "image/png".into(), bytes.clone());
	assert!(serde_json::to_vec(&first).unwrap().len() < 256 * 1024);
	let Result::Available { fingerprint, bytes: first_bytes, total_bytes, .. } = first else {
		panic!("available")
	};
	assert_eq!(first_bytes.len(), CHIEF_MEDIA_CHUNK_BYTES);
	assert_eq!(total_bytes as usize, bytes.len());
	request.offset = CHIEF_MEDIA_CHUNK_BYTES as u32;
	request.fingerprint = Some(fingerprint);
	assert!(
		matches!(chunk(&key(), &request, "image/png".into(), bytes.clone()), Result::Available { bytes, .. } if bytes == vec![255;17])
	);
	assert_eq!(chunk(&key(), &request, "image/jpeg".into(), bytes.clone()), Result::Unavailable);
	let mut changed = bytes.clone();
	changed[0] = 0;
	assert_eq!(chunk(&key(), &request, "image/png".into(), changed), Result::Unavailable);
	let mut other = key();
	other.revision += 1;
	assert_eq!(chunk(&other, &request, "image/png".into(), bytes.clone()), Result::Unavailable);
	request.offset = total_bytes;
	assert_eq!(chunk(&key(), &request, "image/png".into(), bytes), Result::Unavailable);
}

#[test]
fn exact_item_indices_and_supported_payloads_are_required() {
	let history = json!({"thread":{"id":"thread","turns":[{"id":"turn","items":[{"id":"item","type":"userMessage","content":[{"type":"text","text":"literal"},{"type":"image","url":"data:image/png;base64,AQID"}]}]}]}});
	let mut req = request();
	let item = super::super::promotions::exact_item(
		&history,
		req.thread_id.as_str(),
		req.turn_id.as_str(),
		req.item_id.as_str(),
	)
	.unwrap();
	assert!(matches!(locate(item, 1), Ok(Media::Uri("data:image/png;base64,AQID"))));
	assert!(matches!(locate(item, 0), Err(Result::Unsupported)));
	assert!(matches!(locate(item, 2), Err(Result::Unavailable)));
	req.thread_id = EntityId::new("another").unwrap();
	assert!(
		super::super::promotions::exact_item(
			&history,
			req.thread_id.as_str(),
			req.turn_id.as_str(),
			req.item_id.as_str()
		)
		.is_none()
	);
	assert_eq!(decode("image/svg+xml", "AQID"), Err(Result::Unsupported));
	assert_eq!(decode("image/png", "INVALID"), Err(Result::Unavailable));
	assert_eq!(decode("image/png", ""), Err(Result::Unavailable));
	assert_eq!(
		decode("image/png", &"A".repeat(MAX_CHIEF_MEDIA_BYTES.div_ceil(3) * 4 + 4)),
		Err(Result::CapacityExceeded)
	);
	assert_eq!(sniff(b"<html>not an image</html>"), None);
	for (item, index) in [
		(
			json!({"type":"dynamicToolCall","contentItems":[{"type":"inputAudio","audioUrl":"data:audio/wav;base64,AQID"}]}),
			0,
		),
		(
			json!({"type":"mcpToolCall","result":{"content":[{"type":"image","mimeType":"image/png","data":"AQID"}]}}),
			0,
		),
		(json!({"type":"imageGeneration","savedPath":null,"result":"AQID"}), 0),
	] {
		assert!(locate(&item, index).is_ok());
	}
}

#[test]
fn standalone_tool_media_uses_native_indices_without_exposing_encrypted_parts() {
	let item = json!({"type":"functionCallOutput","output":[
		{"type":"input_text","text":"Description"},
		{"type":"input_image","image_url":"data:image/png;base64,AQID"},
		{"type":"input_audio","audio_url":"data:audio/wav;base64,AQID"},
		{"type":"encrypted_content","encrypted_content":"opaque"}
	]});
	assert!(matches!(locate(&item, 1), Ok(Media::Uri("data:image/png;base64,AQID"))));
	assert!(matches!(locate(&item, 2), Ok(Media::Uri("data:audio/wav;base64,AQID"))));
	for index in [0, 3] {
		assert!(matches!(locate(&item, index), Err(Result::Unsupported)));
	}
	assert!(matches!(locate(&item, 4), Err(Result::Unavailable)));
}

async fn server(remote: tokio::io::DuplexStream, path: Option<String>) {
	let (reader, mut writer) = tokio::io::split(remote);
	let mut lines = BufReader::new(reader).lines();
	let content = if let Some(path) = path {
		json!({"type":"localImage","path":path})
	} else {
		json!({"type":"image","url":"data:image/png;base64,AQID"})
	};
	let replies = vec![
		("thread/read", json!({"thread":{"id":"thread","historyMode":"paginated"}})),
		("thread/turns/list", json!({"data":[{"id":"turn"}],"nextCursor":null})),
		(
			"thread/items/list",
			json!({"data":[{"turnId":"turn","item":{"id":"item","type":"userMessage","content":[{"type":"text","text":"image"},content]}}],"nextCursor":null}),
		),
	];
	for (method, result) in replies {
		let request: Value =
			serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
		assert_eq!(request["method"], method);
		writer
			.write_all(format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes())
			.await
			.unwrap();
	}
}

#[tokio::test]
async fn native_reads_use_exact_item_and_discard_bytes_after_source_changes() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("photo.png");
	std::fs::write(&path, b"\x89PNG\r\n\x1a\nfixture").unwrap();
	for change in ["none", "revision", "history", "account", "process", "thread", "closed"] {
		for local in ["inline", "absolute", "relative"] {
			let (io, remote) = tokio::io::duplex(65536);
			let (reader, writer) = tokio::io::split(io);
			let (client, _events) = AppServerClient::from_io(reader, writer);
			let native_path = match local {
				"absolute" => Some(path.to_str().unwrap().to_owned()),
				"relative" => Some("photo.png".into()),
				_ => None,
			};
			let server = tokio::spawn(server(remote, native_path));
			let calls = AtomicUsize::new(0);
			let result = read(
				|| {
					let later = calls.fetch_add(1, Ordering::SeqCst) > 0;
					let client = client.clone();
					async move {
						let mut key = key();
						if later {
							match change {
								"revision" => key.revision += 1,
								"history" => key.history_revision += 1,
								"account" =>
									key.account =
										AccountId::new("40000000-0000-4000-8000-000000000004")
											.unwrap(),
								"process" =>
									key.generation = ProcessGenerationId::new(
										"20000000-0000-4000-8000-000000000002",
									)
									.unwrap(),
								"thread" => key.thread = "other".into(),
								"closed" => return None,
								_ => {},
							}
						}
						Some(Source { key, client })
					}
				},
				|source| {
					assert_eq!(source, &key());
					Some(directory.path().to_str().unwrap().into())
				},
				&request(),
			)
			.await;
			server.await.unwrap();
			if change == "none" {
				assert!(
					matches!(result,Result::Available { mime_type, bytes, .. } if mime_type == "image/png" && !bytes.is_empty())
				);
			} else {
				assert_eq!(result, Result::Unavailable, "{change}");
			}
		}
	}
}

#[tokio::test]
async fn oversized_local_attachment_does_not_send_file_bytes_through_native_transport() {
	let file = tempfile::NamedTempFile::new().unwrap();
	file.as_file().set_len((MAX_CHIEF_MEDIA_BYTES + 1) as u64).unwrap();
	let (io, remote) = tokio::io::duplex(65536);
	let (reader, writer) = tokio::io::split(io);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let path = file.path().to_str().unwrap().to_owned();
	// Keep the same native connection after the three history reads.
	let server = tokio::spawn(async move {
		let (reader, mut writer) = tokio::io::split(remote);
		let mut lines = BufReader::new(reader).lines();
		for (method, result) in [
			("thread/read", json!({"thread":{"id":"thread","historyMode":"paginated"}})),
			("thread/turns/list", json!({"data":[{"id":"turn"}],"nextCursor":null})),
			(
				"thread/items/list",
				json!({"data":[{"turnId":"turn","item":{"id":"item","type":"userMessage","content":[{"type":"text","text":"image"},{"type":"localImage","path":path}]}}],"nextCursor":null}),
			),
			("thread/read", json!({"thread":{"id":"peer","historyMode":"paginated"}})),
		] {
			let request: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
			assert_eq!(request["method"], method);
			writer
				.write_all(format!("{}\n", json!({"id":request["id"],"result":result})).as_bytes())
				.await
				.unwrap();
		}
	});
	let result = read(
		|| std::future::ready(Some(Source { key: key(), client: client.clone() })),
		|_| None,
		&request(),
	)
	.await;
	assert_eq!(result, Result::CapacityExceeded);
	let peer = client.thread_read(json!({"threadId":"peer"})).await.unwrap();
	assert_eq!(peer["thread"]["id"], "peer");
	server.await.unwrap();
}

#[tokio::test]
async fn local_reads_reject_relative_paths_and_non_files() {
	assert_eq!(local_media("relative.png").await, Err(Result::Unavailable));
	let directory = tempfile::tempdir().unwrap();
	assert_eq!(local_media(directory.path().to_str().unwrap()).await, Err(Result::Unsupported));
	#[cfg(unix)]
	{
		use std::ffi::CString;
		let path = directory.path().join("pipe");
		let native = CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
		// Fixture-only FIFO demonstrates that opening an attachment cannot block waiting for a
		// writer.
		assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
		assert_eq!(
			tokio::time::timeout(
				std::time::Duration::from_secs(1),
				local_media(path.to_str().unwrap())
			)
			.await
			.unwrap(),
			Err(Result::Unsupported)
		);
	}
}

#[test]
fn executor_image_path_cannot_read_a_same_named_host_file() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("remote-image.png");
	std::fs::write(&path, b"\x89PNG\r\n\x1a\nlocal-private-content").unwrap();
	let item = json!({"id":"image","type":"imageView","path":path});
	assert!(matches!(locate(&item, 0), Err(Result::Unsupported)));
	let (descriptors, _) = super::super::attachments::project(&item);
	assert_eq!(descriptors[0].source, decodex_protocol::ChiefTimelineAttachmentSource::Unknown);
	assert!(!serde_json::to_string(&descriptors).unwrap().contains(path.to_str().unwrap()));
}

#[tokio::test]
async fn generated_image_uses_native_bytes_even_when_saved_path_exists() {
	let directory = tempfile::tempdir().unwrap();
	let path = directory.path().join("generated.png");
	std::fs::write(&path, b"\x89PNG\r\n\x1a\nwrong-host-image").unwrap();
	let expected = b"\x89PNG\r\n\x1a\nnative-image";
	let mut item =
		json!({"type":"imageGeneration","savedPath":path,"result":STANDARD.encode(expected)});
	assert_eq!(
		resolve(locate(&item, 0).unwrap(), None).await,
		Ok(("image/png".into(), expected.to_vec()))
	);
	std::fs::remove_file(&path).unwrap();
	assert_eq!(
		resolve(locate(&item, 0).unwrap(), None).await,
		Ok(("image/png".into(), expected.to_vec()))
	);
	item["result"] = json!("");
	assert!(matches!(locate(&item, 0), Err(Result::Unsupported)));
}

#[tokio::test]
async fn relative_media_requires_an_absolute_admitted_process_directory() {
	let directory = tempfile::tempdir().unwrap();
	let expected = b"\x89PNG\r\n\x1a\nrelative-native-image";
	std::fs::write(directory.path().join("photo.png"), expected).unwrap();
	for base in [None, Some("relative-base")] {
		assert_eq!(resolve(Media::Local("photo.png"), base).await, Err(Result::Unavailable));
	}
	assert_eq!(
		resolve(Media::Local("photo.png"), directory.path().to_str()).await,
		Ok(("image/png".into(), expected.to_vec()))
	);
}
