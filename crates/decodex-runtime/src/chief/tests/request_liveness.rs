use super::*;

#[tokio::test]
async fn pending_replies_follow_transport_liveness_not_actor_queue_or_current_turn() {
	for method in [
		"item/commandExecution/requestApproval",
		"item/tool/requestUserInput",
		"mcpServer/elicitation/request",
	] {
		for resolved in [false, true] {
			let (mut chief, _sent, _directory) = fixture().await;
			chief.start_chief("chief", "Coordinate").await.unwrap();
			let (incoming, frames) = tokio::sync::mpsc::channel(8);
			let (outgoing, mut writes) = tokio::sync::mpsc::channel(8);
			let (client, mut events) = AppServerClient::from_framed(1, frames, outgoing).unwrap();
			chief.client = client;
			let id = RequestId::String("late-request".into());
			let params = json!({"threadId":"opaque thread/1","turnId":"completed-origin",
				"serverName":"fixture","mode":"form","requestedSchema":{"type":"object","properties":{}},"questions":[]});
			incoming.send(Ok(json!({"id":id,"method":method,"params":params}))).await.unwrap();
			chief.handle_event(events.recv().await.unwrap()).await.unwrap();
			let event = chief.pending_requests[&id];
			let response = match method {
				"mcpServer/elicitation/request" => json!({"action":"accept","content":{}}),
				"item/tool/requestUserInput" => json!({"answers":{}}),
				_ => json!({"decision":"decline"}),
			};
			if resolved {
				incoming.send(Ok(json!({"method":"serverRequest/resolved","params":{"threadId":"opaque thread/1","requestId":id}}))).await.unwrap();
				assert!(chief.respond_pending_event(event, response).await.is_err());
				assert!(writes.try_recv().is_err(), "queued resolution must prevent the reply");
				assert_eq!(chief.pending_requests.get(&id), Some(&event));
				chief.handle_event(events.recv().await.unwrap()).await.unwrap();
			} else {
				chief.respond_pending_event(event, response.clone()).await.unwrap();
				assert_eq!(writes.recv().await.unwrap(), json!({"id":id,"result":response}));
			}
			assert!(!chief.pending_requests.contains_key(&id));
			assert!(chief.store.get_chief_inbox_event(event).await.unwrap().disposition.is_some());
			assert!(chief.respond_pending_event(event, json!({})).await.is_err());
			assert!(writes.try_recv().is_err());
		}
	}
}
