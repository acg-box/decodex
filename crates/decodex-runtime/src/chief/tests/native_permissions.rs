//! Qualify permission-preserving Chief hydration against the installed native server.
use super::*;
use super::native_goal_fixture::serve;
use futures_util::FutureExt as _;
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated native permission restoration"]
async fn native_chief_resume_preserves_selected_profile_policy_and_cwd() {
	let binary = std::env::var("DECODEX_NATIVE_BINARY").unwrap();
	let native_home = tempfile::tempdir().unwrap();
	let home = native_home.path().canonicalize().unwrap();
	let workspace = home.join("selected-workspace");
	std::fs::create_dir_all(workspace.join("writable/private")).unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, Arc::clone(&calls)));
	std::fs::write(home.join("config.toml"), format!(
		"model = \"gpt-5.6-sol\"\nmodel_provider = \"fixture\"\ncli_auth_credentials_store = \"file\"\napprovals_reviewer = \"user\"\n[model_providers.fixture]\nname = \"Isolated permissions fixture\"\nbase_url = \"http://{address}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\nsupports_websockets = false\n[permissions.scoped.filesystem]\n\":root\" = \"read\"\n{} = \"write\"\n{} = \"deny\"\n",
		json!(workspace.join("writable")), json!(workspace.join("writable/private"))
	)).unwrap();
	let (mut chief, _, store_home) = fixture().await;
	chief.config =
		ChiefConfig::new("gpt-5.6-sol".into(), "medium".into(), home.display().to_string());
	chief.config.sandbox = "danger-full-access".into();
	chief.config.approval_policy = json!("never");
	for cold in [false, true] {
		if cold {
			let root = decodex_core::DecodexRoot::new(
				store_home.path().canonicalize().unwrap().join("root"),
			)
			.unwrap();
			chief.store = SqliteStore::open(&root.paths()).unwrap();
		}
		let mut command = tokio::process::Command::new(&binary);
		command
			.arg("app-server")
			.current_dir(&home)
			.env_clear()
			.env("HOME", &home)
			.env("CODEX_HOME", &home)
			.env("PATH", "/usr/bin:/bin");
		let (client, mut events, mut process) = AppServerClient::spawn(&mut command).unwrap();
		chief = ChiefCoordinator::new(chief.store.clone(), client, chief.config.clone()).unwrap();
		let result = std::panic::AssertUnwindSafe(tokio::time::timeout(
			std::time::Duration::from_secs(40),
			async {
				chief.initialize().await.unwrap();
				if !cold {
					select_native_permissions(&mut chief, &mut events, &workspace).await;
					chief.loaded_threads.clear();
				}
				chief.continue_worker("chief", "Reply Done again.").await.unwrap();
				finish(&mut chief, &mut events).await;
				let item = chief.store.get_chief_work_item("chief".into()).await.unwrap();
				assert_eq!(
					chief
						.store
						.chief_permission_receipt(
							"chief".into(),
							item.codex_thread_id.clone().unwrap()
						)
						.await
						.unwrap()
						.unwrap()
						.state,
					"target_observed",
					"cold={cold}"
				);
				assert!(
					chief
						.client
						.observed_task_permissions(item.codex_thread_id.as_deref().unwrap())
						.is_some(),
					"idle task must expose permissions before an extra resume; cold={cold}"
				);
				let actual = chief
					.client
					.thread_resume(json!({
						"threadId":item.codex_thread_id, "excludeTurns":true
					}))
					.await
					.unwrap();
				assert_eq!(
					actual["activePermissionProfile"]["id"], "scoped",
					"cold={cold}: {actual}"
				);
				assert_eq!(actual["approvalPolicy"], "on-request", "cold={cold}");
				assert_eq!(actual["approvalsReviewer"], "auto_review", "cold={cold}");
				assert_eq!(actual["cwd"], json!(workspace), "cold={cold}");
				let (wire_permissions, wire_guard) = chief
					.client
					.observed_task_permissions(item.codex_thread_id.as_deref().unwrap())
					.expect("native resume must supply current permission facts");
				assert!(wire_guard.is_live(), "cold={cold}");
				assert_eq!(wire_permissions.profile_id.as_deref(), Some("scoped"), "cold={cold}");
				assert_eq!(wire_permissions.approvals_reviewer, "auto_review", "cold={cold}");
				let persisted = chief
					.store
					.chief_task_permissions(
						"chief".into(),
						item.codex_thread_id.clone().unwrap(),
						None,
					)
					.await
					.unwrap()
					.unwrap();
				let permissions: decodex_codex::app_server_client::NativeTaskPermissions =
					serde_json::from_str(persisted.settings_json.as_ref().unwrap()).unwrap();
				assert_eq!(permissions.profile_id.as_deref(), Some("scoped"), "cold={cold}");
				assert_eq!(permissions.approvals_reviewer, "auto_review", "cold={cold}");
				assert_eq!(permissions.cwd, workspace.to_str().unwrap(), "cold={cold}");
				assert_eq!(calls.load(Ordering::Acquire), if cold { 3 } else { 2 });
			},
		))
		.catch_unwind()
		.await;
		process.shutdown().await.unwrap();
		if result.as_ref().is_err() || result.as_ref().is_ok_and(|result| result.is_err()) {
			backend.abort();
		}
		result
			.expect("native permission qualification panicked")
			.expect("native permission qualification timed out");
	}
	backend.abort();
}

async fn select_native_permissions(
	chief: &mut ChiefCoordinator,
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
	workspace: &std::path::Path,
) {
	chief.start_chief("chief", "Reply Done.").await.unwrap();
	finish(chief, events).await;
	let item = chief.store.get_chief_work_item("chief".into()).await.unwrap();
	chief
		.client
		.request(
			"thread/settings/update",
			json!({
				"threadId": item.codex_thread_id,
				"approvalPolicy":"on-request", "approvalsReviewer":"auto_review", "cwd":workspace
			}),
		)
		.await
		.unwrap();
	let profiles = chief.client.permission_profiles(workspace.to_str().unwrap()).await.unwrap();
	assert!(profiles.iter().any(|profile| profile.id == "scoped" && profile.allowed));
	let selection = decodex_codex::app_server_client::ThreadPermissionSelection::new(
		item.codex_thread_id.as_deref().unwrap(),
		"scoped",
	)
	.unwrap();
	let observed = chief
		.store
		.chief_task_permissions("chief".into(), item.codex_thread_id.clone().unwrap(), None)
		.await
		.unwrap()
		.unwrap();
	let attempt = decodex_database::ChiefPermissionAttempt {
		work: "chief".into(),
		thread: item.codex_thread_id.clone().unwrap(),
		generation: None,
		settings_event: observed.id,
		profile: "scoped".into(),
		review_token: "a".repeat(64),
		attempt_id: "native-profile-selection".into(),
	};
	let reservation =
		chief.store.reserve_chief_permission_selection(attempt.clone()).await.unwrap().unwrap();
	let guard = chief.client.history_guard(chief.client.history_revision()).unwrap();
	chief.client.queue_thread_permission_selection(&selection, guard).await.unwrap();
	assert!(
		chief
			.store
			.finish_chief_permission_selection(reservation, attempt, "queued".into())
			.await
			.unwrap()
	);
	while chief
		.store
		.chief_permission_receipt("chief".into(), item.codex_thread_id.clone().unwrap())
		.await
		.unwrap()
		.unwrap()
		.state != "target_observed"
	{
		chief
			.handle_event(events.recv().await.expect("native permission publication"))
			.await
			.unwrap();
	}
}

async fn finish(
	chief: &mut ChiefCoordinator,
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
) {
	loop {
		let event = events.recv().await.expect("native events");
		let done = if let ServerEvent::Notification { method, params } = &event {
			if method == "turn/completed" {
				assert_eq!(params["turn"]["status"], "completed", "{params}");
				true
			} else {
				false
			}
		} else {
			false
		};
		chief.handle_event(event).await.unwrap();
		if done {
			break;
		}
	}
}
