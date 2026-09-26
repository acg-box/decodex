use super::*;

#[tokio::test]
async fn prompt_edit_pages_preserve_canonical_content_and_reject_changed_evidence() {
	for mode in ["complete", "changed-token", "wrong-offset"] {
		let content = serde_json::json!([{"type":"text","text":"界".repeat(26000),"text_elements":[]},{"type":"image","fileId":"native-file","detail":"original"}]);
		let encoded = serde_json::to_string(&content).unwrap();
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.unwrap();
		let profile = ClientProfile::fixture(authority, ServerId::new(SERVER_ID).unwrap());
		let server = tokio::spawn(async move {
			let _temp = temp;
			let mut cursor = 0;
			for page in 0..2 {
				let stream = listener.accept().await.unwrap();
				let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
				let _ = socket.next().await;
				for response in initial(SERVER_ID) {
					socket.send(response).await.unwrap();
				}
				let Message::Text(request) = socket.next().await.unwrap().unwrap() else {
					panic!("query frame")
				};
				let ClientMessage::Query(query) = serde_json::from_str(&request).unwrap() else {
					panic!("read only query")
				};
				assert!(
					matches!(&query.payload,crate::QueryPayload::GetChiefPromptEdit {work_id,thread_id,review_token,offset} if work_id.as_str()=="root" && thread_id.as_str()=="native" && *offset==cursor as u64 && review_token.as_ref().map(|v|v.as_str())==if page==0 {None} else {Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")})
				);
				let mut end = (cursor + 64 * 1024).min(encoded.len());
				while !encoded.is_char_boundary(end) {
					end -= 1;
				}
				let result = crate::PromptEditStatus {
					work_id: EntityId::new("root").unwrap(),
					thread_id: WireText::new("native").unwrap(),
					phase: crate::PromptEditPhase::Review,
					evidence: Some(crate::PromptEditEvidence {
						review_token: WireText::new(if page == 1 && mode == "changed-token" {
							"b".repeat(64)
						} else {
							"a".repeat(64)
						})
						.unwrap(),
						receipt_id: None,
						before_turn_id: WireText::new("turn").unwrap(),
						item_id: WireText::new("item").unwrap(),
						removed_turns: 2,
						content_bytes: encoded.len() as u64,
						offset: cursor as u64 + u64::from(page == 1 && mode == "wrong-offset"),
						fragment: encoded[cursor..end].into(),
					}),
				};
				cursor = end;
				socket
					.send(typed(ServerMessage::QueryResult(QueryResultEnvelope {
						version: CURRENT_VERSION,
						server_id: ServerId::new(SERVER_ID).unwrap(),
						query_id: query.query_id,
						payload: QueryResultPayload::ChiefPromptEdit(result),
					})))
					.await
					.unwrap();
				drop(socket);
			}
			listener.cleanup().unwrap();
		});
		let result = crate::ChiefClient::new(profile)
			.prompt_edit(EntityId::new("root").unwrap(), WireText::new("native").unwrap())
			.await;
		server.await.unwrap();
		if mode == "complete" {
			assert_eq!(serde_json::json!(result.unwrap().1.unwrap()), content);
		} else {
			assert!(matches!(result, Err(ClientFailure::ProtocolMalformed)));
		}
	}
}

#[tokio::test]
async fn prompt_upload_query_rejects_crossed_sources_and_impossible_progress() {
	for mode in ["valid", "crossed", "overflow"] {
		let upload = crate::PromptInputUpload {
			work_id: EntityId::new("root").unwrap(),
			thread_id: WireText::new("native").unwrap(),
			edit_receipt_id: 1,
			upload_id: crate::IdempotencyKey::new("upload").unwrap(),
			sha256: crate::Sha256Digest::new("a".repeat(64)).unwrap(),
			total_bytes: 128,
		};
		let expected = upload.clone();
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.unwrap();
		let profile = ClientProfile::fixture(authority, ServerId::new(SERVER_ID).unwrap());
		let server = tokio::spawn(async move {
			let _temp = temp;
			let stream = listener.accept().await.unwrap();
			let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
			let _ = socket.next().await;
			for response in initial(SERVER_ID) {
				socket.send(response).await.unwrap();
			}
			let Message::Text(request) = socket.next().await.unwrap().unwrap() else {
				panic!("query frame")
			};
			let ClientMessage::Query(query) = serde_json::from_str(&request).unwrap() else {
				panic!("read-only query")
			};
			assert!(
				matches!(&query.payload, crate::QueryPayload::GetChiefPromptInputUpload { upload } if upload == &expected)
			);
			let mut echoed = expected;
			if mode == "crossed" {
				echoed.edit_receipt_id += 1;
			}
			let status = crate::PromptInputUploadStatus::Receiving {
				upload: echoed,
				received_bytes: if mode == "overflow" { 129 } else { 64 },
			};
			socket
				.send(typed(ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).unwrap(),
					query_id: query.query_id,
					payload: QueryResultPayload::ChiefPromptInputUpload(status),
				})))
				.await
				.unwrap();
			drop(socket);
			listener.cleanup().unwrap();
		});
		let result = crate::ChiefClient::new(profile).prompt_input_upload_status(upload).await;
		server.await.unwrap();
		if mode == "valid" {
			assert!(result.is_ok());
		} else {
			assert!(matches!(result, Err(ClientFailure::ProtocolMalformed)));
		}
	}
}

#[tokio::test]
async fn prompt_upload_resumes_durable_bytes_after_lost_replies_without_submitting() {
	for mode in ["fresh", "resume", "no-progress"] {
		let input = crate::PromptDraft::new(vec![
			serde_json::json!({"type":"text","text":"界\\\"".repeat(18000),"text_elements":[]}),
		])
		.unwrap();
		let encoded = serde_json::to_string(&input).unwrap();
		let upload = crate::PromptInputUpload {
			work_id: EntityId::new("root").unwrap(),
			thread_id: WireText::new("native").unwrap(),
			edit_receipt_id: 1,
			upload_id: crate::IdempotencyKey::new("upload").unwrap(),
			sha256: input.fingerprint().unwrap(),
			total_bytes: encoded.len() as u64,
		};
		let expected = upload.clone();
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.unwrap();
		let profile = ClientProfile::fixture(authority, ServerId::new(SERVER_ID).unwrap());
		let server = tokio::spawn(async move {
			let _temp = temp;
			let mut saved = if mode == "resume" { encoded[..13].to_owned() } else { String::new() };
			let mut complete = false;
			let mut commands = 0;
			loop {
				let stream = listener.accept().await.unwrap();
				let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
				let _ = socket.next().await;
				for response in initial(SERVER_ID) {
					socket.send(response).await.unwrap();
				}
				let Message::Text(request) = socket.next().await.unwrap().unwrap() else {
					panic!("text frame")
				};
				assert!(request.len() < 256 * 1024);
				match serde_json::from_str::<ClientMessage>(&request).unwrap() {
					ClientMessage::Query(query) => {
						assert!(
							matches!(&query.payload, crate::QueryPayload::GetChiefPromptInputUpload { upload } if upload == &expected)
						);
						let status = if complete {
							crate::PromptInputUploadStatus::Ready {
								upload: expected.clone(),
								input_id: 7,
							}
						} else {
							crate::PromptInputUploadStatus::Receiving {
								upload: expected.clone(),
								received_bytes: saved.len() as u64,
							}
						};
						socket
							.send(typed(ServerMessage::QueryResult(QueryResultEnvelope {
								version: CURRENT_VERSION,
								server_id: ServerId::new(SERVER_ID).unwrap(),
								query_id: query.query_id,
								payload: QueryResultPayload::ChiefPromptInputUpload(status),
							})))
							.await
							.unwrap();
						if complete || (mode == "no-progress" && commands > 0) {
							break;
						}
					},
					ClientMessage::Command(command) => {
						let crate::CommandPayload::Chief { action } = command.payload else {
							panic!("only staging commands")
						};
						commands += 1;
						match *action {
							crate::ChiefActionDto::UploadPromptInput {
								upload,
								offset,
								fragment,
							} => {
								assert_eq!(upload, expected);
								assert_eq!(offset as usize, saved.len());
								assert!(fragment.len() <= 65536);
								if mode != "no-progress" {
									saved.push_str(&fragment);
								}
							},
							crate::ChiefActionDto::CompletePromptInputUpload { upload } => {
								assert_eq!(upload, expected);
								assert_eq!(saved, encoded);
								complete = true;
							},
							_ => panic!("staging must never submit input"),
						}
						// Simulate a lost command reply after durable storage.
					},
					_ => panic!("unexpected client message"),
				}
			}
			listener.cleanup().unwrap();
		});
		let result = crate::ChiefClient::new(profile).stage_prompt_input(upload, &input).await;
		server.await.unwrap();
		if mode == "no-progress" {
			assert_eq!(result, Err(ClientFailure::ApplicationAcceptanceUnknown));
		} else {
			assert_eq!(result, Ok(7));
		}
	}
}
