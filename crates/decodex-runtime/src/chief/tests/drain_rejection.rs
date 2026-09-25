use super::*;

#[tokio::test]
async fn drain_wire_refusal_distinguishes_direct_input_from_injected_updates() {
	let (mut chief, mut sent, _directory) =
		fixture_with_history(json!({"_turn_draining":true})).await;
	assert!(matches!(
		chief.start_chief("chief", "Keep the original instruction").await,
		Err(ChiefError::InputNotSent(decodex_database::ChiefDispatchRefusal::ServerDraining))
	));
	assert_eq!(
		chief.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Idle
	);
	let mut starts = 0;
	while let Ok(request) = sent.try_recv() {
		assert_ne!(request["method"], "thread/inject_items");
		starts += usize::from(request["method"] == "turn/start");
	}
	assert_eq!(starts, 1);
	chief.wake_pending().await.unwrap();
	assert!(sent.try_recv().is_err());

	let (mut chief, mut sent, _directory) =
		fixture_with_history(json!({"_turn_draining_after_injection":true})).await;
	chief.start_chief("chief", "Initial").await.unwrap();
	complete(&mut chief, "chief").await;
	while sent.try_recv().is_ok() {}
	let before = chief.store.get_chief_work_item("chief".into()).await.unwrap();
	let event = chief
		.store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: "external-update".into(),
			work_item_id: "chief".into(),
			event_kind: "automation_result".into(),
			payload: json!({"text":"External evidence"}).to_string(),
		})
		.await
		.unwrap();
	assert!(chief.dispatch_with_events(&before, "Process update", vec![event.id]).await.is_err());
	let mut methods = Vec::new();
	while let Ok(request) = sent.try_recv() {
		methods.push(request["method"].as_str().unwrap().to_owned());
	}
	assert!(
		methods.iter().position(|m| m == "thread/inject_items").unwrap()
			< methods.iter().position(|m| m == "turn/start").unwrap()
	);
	assert_eq!(
		chief.store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
		decodex_database::ChiefDispatchState::Unknown
	);
}

#[tokio::test]
async fn native_rejection_preserves_input_and_never_replays_ambiguous_effects() {
	for (managed, message) in [
		(false, "Server is draining; retry after reconnecting"),
		(
			true,
			"failed to load configuration: Your organization's required model provider settings changed. Restart Codex to apply them; this request was not sent",
		),
	] {
		for (prior_effects, lost_response, expected_rejection) in
			[(false, false, true), (true, false, false), (false, true, false)]
		{
			let (mut chief, mut sent, directory) = fixture().await;
			chief.start_chief("chief", "Initial").await.unwrap();
			complete(&mut chief, "chief").await;
			let before = chief.store.get_chief_work_item("chief".into()).await.unwrap();
			let event = chief
				.store
				.enqueue_chief_event(EnqueueChiefEvent {
					source_event_id: "drain-input".into(),
					work_item_id: "chief".into(),
					event_kind: "user_message".into(),
					payload: json!({"text":"Keep this input","source":"user"}).to_string(),
				})
				.await
				.unwrap();
			chief
				.store
				.begin_chief_dispatch_with_input("chief".into(), vec![event.id], None)
				.await
				.unwrap();
			let error = if lost_response {
				ClientError::Closed
			} else {
				ClientError::Remote(decodex_codex::app_server_client::RpcError {
					code: -32600,
					message: message.into(),
					data: None,
				})
			};
			let result = chief
				.finish_dispatch_attempt(
					&before,
					None,
					Err(error.into()),
					None,
					!prior_effects,
					None,
				)
				.await;
			assert_eq!(
				if managed {
					matches!(
						result,
						Err(ChiefError::InputNotSent(
							decodex_database::ChiefDispatchRefusal::ManagedProviderChanged
						))
					)
				} else {
					matches!(
						result,
						Err(ChiefError::InputNotSent(
							decodex_database::ChiefDispatchRefusal::ServerDraining
						))
					)
				},
				expected_rejection
			);
			while sent.try_recv().is_ok() {}
			chief.wake_pending().await.unwrap();
			assert!(sent.try_recv().is_err(), "Refused or uncertain input must not be replayed");
			drop(chief);
			let root = decodex_core::DecodexRoot::new(
				directory.path().canonicalize().unwrap().join("root"),
			)
			.unwrap();
			let store = SqliteStore::open(&root.paths()).unwrap();
			let work = store.get_chief_work_item("chief".into()).await.unwrap();
			let saved = store.get_chief_inbox_event(event.id).await.unwrap();
			assert!(saved.payload.contains("Keep this input"));
			if expected_rejection {
				assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Idle);
				assert_eq!(work.status, decodex_database::ChiefWorkStatus::UserDecision);
				assert_eq!(
					saved.disposition,
					Some(decodex_database::ChiefDisposition::UserDecision)
				);
				assert!(saved.delivered_turn_id.is_none());
				let note = saved.disposition_note.unwrap();
				assert!(note.contains("Not sent"));
				assert_eq!(note.contains("managed model provider"), managed);
			} else {
				assert_eq!(work.dispatch_state, decodex_database::ChiefDispatchState::Unknown);
				assert!(saved.disposition.is_none());
			}
		}
	}
}
