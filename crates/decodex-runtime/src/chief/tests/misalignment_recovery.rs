use super::*;

#[tokio::test]
async fn complete_later_native_history_reconciles_only_the_exact_old_precaution() {
	for case in ["latest", "later", "voice", "missing-original", "missing-item"] {
		let error = json!({"codexErrorInfo":"misalignmentPolicyViolation"});
		let mut turns =
			vec![json!({"id":"opaque turn/1","status":"failed","error":error,"items":[]})];
		if case == "missing-original" {
			turns.clear();
		}
		if case != "latest" {
			turns.push(json!({"id":"later","status":"completed","items":[]}));
		}
		let (mut chief, mut sent, _directory) = fixture_with_history(json!({
			"opaque thread/1":{"thread":{"id":"opaque thread/1","turns":turns}}
		}))
		.await;
		chief.start_chief("chief", "Coordinate").await.unwrap();
		chief.enqueue_user_message("chief", "retired", "Never replay this input").await.unwrap();
		chief.observe_misalignment("opaque thread/1", "opaque turn/1", &error).await.unwrap();
		chief.store.complete_chief_turn("chief".into(), "opaque turn/1".into()).await.unwrap();
		if case == "voice" {
			chief.store.retire_chief_misalignment_voice("opaque thread/1".into()).await.unwrap();
		}
		if case == "missing-item" {
			chief
				.store
				.request_chief_async_recovery("opaque thread/1".into(), "not-in-history".into())
				.await
				.unwrap();
		} else {
			chief.store.refresh_chief_async_projection("opaque thread/1".into()).await.unwrap();
		}
		while sent.try_recv().is_ok() {}
		chief.recover_async_questions().await.unwrap();
		assert_eq!(
			chief.store.chief_misalignment("chief".into()).await.unwrap().is_none(),
			case == "later",
			"{case}"
		);
		assert!(chief.store.list_undelivered_chief_events(100).await.unwrap().is_empty());
		while let Ok(request) = sent.try_recv() {
			assert!(matches!(
				request["method"].as_str(),
				Some("thread/read" | "thread/turns/list" | "thread/turns/items/list")
			));
		}
	}
}
