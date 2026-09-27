//! Service shutdown with a witnessed active native request in an isolated fixture.
use super::*;

pub(super) async fn prepare(client: &ChiefClient, home: &std::path::Path, account: &AccountId) {
	accepted(
		client,
		Action::Start(ChiefStartDto {
			root_id: EntityId::new("recap-root").expect("fixture root"),
			prompt: HistoryText::new("Keep the isolated response pending for service shutdown.")
				.expect("fixture input"),
			model: ConversationModel::new("cold-native-model").expect("fixture model"),
			effort: Some(
				ConversationReasoningEffort::new("provider-effort").expect("fixture effort"),
			),
			cwd: ConversationWorkingDirectory::new(home.to_str().expect("fixture path"))
				.expect("fixture directory"),
			account_id: Some(EntityId::new(account.as_str()).expect("fixture account")),
			sandbox: ChiefSandboxDto::ReadOnly,
		}),
		"active-shutdown-start",
	)
	.await;
	tokio::time::timeout(Duration::from_secs(15), async {
		loop {
			if home.join("active-provider-started").exists()
				&& let ChiefSnapshotResult::Available(snapshot) =
					client.query().await.expect("public snapshot")
				&& let Some(work) = snapshot.work_items.iter().find(|work| work.id == "recap-root")
				&& work.dispatch_state == ChiefDispatchStateDto::Running
				&& work.active_turn_id.is_some()
			{
				std::fs::write(
					home.join("active-shutdown-before.json"),
					serde_json::to_vec_pretty(work).expect("work evidence"),
				)
				.expect("save active evidence");
				break;
			}
			tokio::time::sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("witness active model request before service shutdown");
}

pub(super) async fn verify(home: &std::path::Path, requests: &std::sync::atomic::AtomicUsize) {
	if std::env::var_os("DECODEX_TEST_ACTIVE_SERVICE_SHUTDOWN").is_none() {
		return;
	}
	tokio::time::timeout(Duration::from_secs(5), async {
		while !home.join("active-provider-closed").exists() {
			tokio::time::sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("native provider connection closed after service shutdown");
	assert_eq!(requests.load(Ordering::Acquire), 1, "shutdown cannot replay input");
	std::fs::write(
		home.join("active-shutdown-result.json"),
		b"{\"service_shutdown_success\":true,\"provider_closed\":true,\"model_requests\":1}\n",
	)
	.expect("save shutdown evidence");
}
