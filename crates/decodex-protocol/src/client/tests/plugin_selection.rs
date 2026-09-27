use super::*;

#[tokio::test]
async fn plugin_selection_waits_for_its_original_reply_without_retry() {
	let (temp, authority) = local_transport();
	let mut listener = authority.bind().await.unwrap();
	let profile = ClientProfile::fixture(authority, ServerId::new(SERVER_ID).unwrap());
	let server = tokio::spawn(async move {
		let _temp = temp;
		let mut socket =
			tokio_tungstenite::accept_async(listener.accept().await.unwrap()).await.unwrap();
		let _ = socket.next().await;
		for message in initial(SERVER_ID) {
			socket.send(message).await.unwrap();
		}
		let Message::Text(frame) = socket.next().await.unwrap().unwrap() else {
			panic!("command frame")
		};
		let ClientMessage::Command(command) = serde_json::from_str(&frame).unwrap() else {
			panic!("command")
		};
		assert!(
			matches!(&command.payload, crate::CommandPayload::Chief { action } if matches!(action.as_ref(), crate::ChiefActionDto::SetTaskPlugin { work_id, thread_id, plugin_id, enabled: false, .. } if work_id.as_str()=="work" && thread_id.as_str()=="thread" && plugin_id.as_str()=="plugin"))
		);
		socket
			.send(typed(ServerMessage::CommandReceipt(CommandReceipt {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER_ID).unwrap(),
				client_command_id: command.client_command_id.clone(),
				idempotency_key: command.idempotency_key.clone(),
				disposition: ReceiptDisposition::Executed,
				original_client_command_id: command.client_command_id.clone(),
			})))
			.await
			.unwrap();
		// Native catalog inspection can exceed the ordinary five-second transport budget.
		time::sleep(Duration::from_secs(6)).await;
		let _ = socket
			.send(typed(ServerMessage::CommandResult(CommandResultEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(SERVER_ID).unwrap(),
				client_command_id: command.client_command_id,
				idempotency_key: command.idempotency_key,
				outcome: CommandOutcome::Succeeded,
				entity_revision: Some(EntityRevision(0)),
				payload: Some(ResultPayload::ChiefAccepted {
					work_id: EntityId::new("work").unwrap(),
				}),
				error: None,
			})))
			.await;
		drop(socket);
		assert!(
			time::timeout(Duration::from_millis(30), listener.accept()).await.is_err(),
			"No reconnect or retry"
		);
		listener.cleanup().unwrap();
	});
	let response = crate::ChiefClient::new(profile)
		.execute(
			crate::ChiefActionDto::SetTaskPlugin {
				work_id: EntityId::new("work").unwrap(),
				thread_id: EntityId::new("thread").unwrap(),
				review_token: WireText::new("a".repeat(64)).unwrap(),
				plugin_id: WireText::new("plugin").unwrap(),
				enabled: false,
			},
			IdempotencyKey::new("one-plugin-selection").unwrap(),
		)
		.await
		.unwrap();
	server.await.unwrap();
	assert!(
		matches!(response, crate::ChiefCommandResponse::Accepted { work_id } if work_id.as_str()=="work"),
		"The original reply must retain its native settings budget"
	);
}
