use super::*;

#[tokio::test]
async fn summary_history_rejects_crossed_work_and_thread() {
	for mode in ["valid", "work", "thread"] {
		let (temp, authority) = local_transport();
		let mut listener = authority.bind().await.expect("listener");
		let profile = ClientProfile::fixture(authority, ServerId::new(SERVER_ID).expect("server"));
		let server = tokio::spawn(async move {
			let _temp = temp;
			let stream = listener.accept().await.expect("accept");
			let mut socket = tokio_tungstenite::accept_async(stream).await.expect("socket");
			let _ = socket.next().await;
			for response in initial(SERVER_ID) {
				socket.send(response).await.expect("initial");
			}
			let Message::Text(request) = socket.next().await.expect("request").expect("frame")
			else {
				panic!("query frame")
			};
			let ClientMessage::Query(query) = serde_json::from_str(&request).expect("query") else {
				panic!("read-only query")
			};
			assert!(
				matches!(&query.payload, crate::QueryPayload::GetChiefTimeline {work_id, thread_id, cursor}
                if work_id.as_str() == "root" && thread_id.as_str() == "native" && cursor.is_none())
			);
			let result = crate::ChiefTimelineResult::Summary {
				work_id: EntityId::new(if mode == "work" { "other" } else { "root" })
					.expect("work"),
				account_id: EntityId::new("account").expect("account"),
				thread_id: if mode == "thread" { "other" } else { "native" }.into(),
				items: vec![],
			};
			socket
				.send(typed(ServerMessage::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: ServerId::new(SERVER_ID).expect("server"),
					query_id: query.query_id,
					payload: QueryResultPayload::ChiefTimeline(result),
				})))
				.await
				.expect("reply");
			drop(socket);
			listener.cleanup().expect("cleanup");
		});
		let result = crate::ChiefClient::new(profile)
			.timeline(
				EntityId::new("root").expect("work"),
				EntityId::new("native").expect("thread"),
				None,
			)
			.await;
		server.await.expect("server task");
		if mode == "valid" {
			assert!(matches!(result, Ok(crate::ChiefTimelineResult::Summary { .. })));
		} else {
			assert!(matches!(result, Err(ClientFailure::ProtocolMalformed)));
		}
	}
}
