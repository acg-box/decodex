//! Qualify permission-preserving Agent hydration against the installed native server.
use super::{native_goal_fixture::serve, *};
use futures_util::FutureExt as _;
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

#[tokio::test]
#[ignore = "requires DECODEX_NATIVE_BINARY; isolated native permission restoration"]
async fn native_agent_resume_preserves_selected_profile_policy_and_cwd() {
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

	let (mut agent, _, store_home) = fixture().await;

	agent.config =
		AgentConfig::new("gpt-5.6-sol".into(), "medium".into(), home.display().to_string());
	agent.config.sandbox = "danger-full-access".into();
	agent.config.approval_policy = json!("never");

	for cold in [false, true] {
		if cold {
			let root = decodex_core::DecodexRoot::new(
				store_home.path().canonicalize().unwrap().join("root"),
			)
			.unwrap();

			agent.store = SqliteStore::open(&root.paths()).unwrap();
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

		agent = AgentCoordinator::new(agent.store.clone(), client, agent.config.clone()).unwrap();

		let result = std::panic::AssertUnwindSafe(tokio::time::timeout(
			std::time::Duration::from_secs(40),
			async {
				agent.initialize().await.unwrap();

				if !cold {
					select_native_permissions(&mut agent, &mut events, &workspace).await;

					agent.loaded_threads.clear();
				}

				agent.continue_worker("agent", "Reply Done again.").await.unwrap();

				finish(&mut agent, &mut events).await;

				let item = agent.store.get_agent_work_item("agent".into()).await.unwrap();

				assert_eq!(
					agent
						.store
						.agent_permission_receipt(
							"agent".into(),
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
					agent
						.client
						.observed_task_permissions(item.codex_thread_id.as_deref().unwrap())
						.is_some(),
					"idle task must expose permissions before an extra resume; cold={cold}"
				);

				let actual = agent
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

				let (wire_permissions, wire_guard) = agent
					.client
					.observed_task_permissions(item.codex_thread_id.as_deref().unwrap())
					.expect("native resume must supply current permission facts");

				assert!(wire_guard.is_live(), "cold={cold}");
				assert_eq!(wire_permissions.profile_id.as_deref(), Some("scoped"), "cold={cold}");
				assert_eq!(wire_permissions.approvals_reviewer, "auto_review", "cold={cold}");

				let persisted = agent
					.store
					.agent_task_permissions(
						"agent".into(),
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
	agent: &mut AgentCoordinator,
	events: &mut tokio::sync::mpsc::Receiver<ServerEvent>,
	workspace: &std::path::Path,
) {
	agent.start_agent("agent", "Reply Done.").await.unwrap();

	finish(agent, events).await;

	let item = agent.store.get_agent_work_item("agent".into()).await.unwrap();

	agent
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

	let profiles = agent.client.permission_profiles(workspace.to_str().unwrap()).await.unwrap();

	assert!(profiles.iter().any(|profile| profile.id == "scoped" && profile.allowed));

	let selection = decodex_codex::app_server_client::ThreadPermissionSelection::new(
		item.codex_thread_id.as_deref().unwrap(),
		"scoped",
	)
	.unwrap();
	let observed = agent
		.store
		.agent_task_permissions("agent".into(), item.codex_thread_id.clone().unwrap(), None)
		.await
		.unwrap()
		.unwrap();
	let attempt = decodex_database::AgentPermissionAttempt {
		work: "agent".into(),
		thread: item.codex_thread_id.clone().unwrap(),
		generation: None,
		settings_event: observed.id,
		profile: "scoped".into(),
		review_token: "a".repeat(64),
		attempt_id: "native-profile-selection".into(),
	};
	let reservation =
		agent.store.reserve_agent_permission_selection(attempt.clone()).await.unwrap().unwrap();
	let guard = agent.client.history_guard(agent.client.history_revision()).unwrap();

	agent.client.queue_thread_permission_selection(&selection, guard).await.unwrap();

	assert!(
		agent
			.store
			.finish_agent_permission_selection(reservation, attempt, "queued".into())
			.await
			.unwrap()
	);

	while agent
		.store
		.agent_permission_receipt("agent".into(), item.codex_thread_id.clone().unwrap())
		.await
		.unwrap()
		.unwrap()
		.state
		!= "target_observed"
	{
		agent
			.handle_event(events.recv().await.expect("native permission publication"))
			.await
			.unwrap();
	}
}

async fn finish(
	agent: &mut AgentCoordinator,
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

		agent.handle_event(event).await.unwrap();

		if done {
			break;
		}
	}
}
