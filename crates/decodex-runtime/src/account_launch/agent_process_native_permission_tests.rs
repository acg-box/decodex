//! Native permission catalog, observations and restart with one materialization turn.
use super::*;
use decodex_codex::app_server_client::{NativeTaskPermissions, ThreadPermissionSelection};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated permission selection qualification"]
async fn installed_permission_selection_publishes_and_survives_native_restart() {
	tokio::time::timeout(Duration::from_secs(45), qualify(false))
		.await
		.expect("bounded permissions fixture");
}
#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated active permission qualification"]
async fn installed_named_permission_selection_during_active_turn_survives_restart() {
	tokio::time::timeout(Duration::from_secs(45), qualify(true))
		.await
		.expect("bounded active permissions fixture");
}
async fn qualify(running: bool) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit binary");
	assert!(std::path::Path::new(&binary).is_absolute());
	let home = tempfile::tempdir().expect("fixture home");
	let root = home.path().canonicalize().expect("canonical home");
	let workspace = root.join("workspace");
	std::fs::create_dir_all(workspace.join("writable/private")).expect("fixture workspace");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
	let address = listener.local_addr().expect("address");
	let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let mut listener = Some(listener);
	let mut backend = None;
	if !running {
		backend = Some(tokio::spawn(serve(listener.take().expect("listener"), calls.clone())));
	}
	std::fs::write(root.join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\ncli_auth_credentials_store=\"file\"\napprovals_reviewer=\"user\"\n[model_providers.fixture]\nname=\"Isolated permission fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n[permissions.scoped.filesystem]\n\":root\"=\"read\"\n{}=\"write\"\n{}=\"deny\"\n",json!(workspace.join("writable")),json!(workspace.join("writable/private")))).expect("fixture config");
	let mut session = NativeSession::start(&binary, &root);
	let profiles = session
		.client
		.permission_profiles(workspace.to_str().expect("cwd"))
		.await
		.expect("native profiles");
	assert!(profiles.iter().any(|p| p.id == "scoped" && p.allowed));
	assert!(profiles.iter().any(|p| p.id == ":read-only" && p.allowed));
	let started=session.client.thread_start(json!({"cwd":workspace,"permissions":":read-only","approvalPolicy":"on-request","approvalsReviewer":"user"})).await.expect("native thread");
	let thread = started["thread"]["id"].as_str().expect("thread").to_owned();
	let initial =
		NativeTaskPermissions::from_thread_response(&started).expect("initial permission facts");
	assert_eq!(initial.profile_id.as_deref(), Some(":read-only"));
	assert_eq!(
		session.client.observed_task_permissions(&thread).expect("start hydration").0,
		initial
	);
	start_turn(&mut session, &thread).await;
	if !running {
		wait_turn(&mut session, &thread, "turn/completed").await;
		assert_eq!(
			session
				.client
				.observed_task_permissions(&thread)
				.expect("idle after exact completion")
				.0,
			initial
		);
		assert_eq!(calls.load(Ordering::Acquire), 1);
	} else {
		wait_turn(&mut session, &thread, "turn/started").await;
		assert!(session.client.observed_task_permissions(&thread).is_none());
		assert_eq!(
			session.client.configured_task_permissions(&thread).expect("running facts").0,
			initial
		);
	}
	let guard = session.client.thread_settings_guard(&thread).expect("settings guard");
	if running {
		super::reviewer::select_permission(&session.client, &root, &thread).await;
	} else {
		session
			.client
			.queue_thread_permission_selection(
				&ThreadPermissionSelection::new(&thread, "scoped").expect("selection"),
				guard.clone(),
			)
			.await
			.expect("queue selection");
	}
	let observed = loop {
		let event = session.events.recv().await.expect("native notification");
		if let ServerEvent::Notification { method, params } = event
			&& method == "thread/settings/updated"
			&& params["threadId"] == thread
		{
			let facts = NativeTaskPermissions::from_notification(&params["threadSettings"])
				.expect("native permission publication");
			if facts.profile_id.as_deref() == Some("scoped") {
				break facts;
			}
		}
	};
	assert_eq!(
		session.client.configured_task_permissions(&thread).expect("wire observation").0,
		observed
	);
	assert!(!guard.is_live(), "wire publication invalidates the previous settings guard");
	assert_eq!(observed.cwd, workspace.to_str().expect("cwd"));
	assert_eq!(observed.approval_policy, initial.approval_policy);
	assert_eq!(observed.approvals_reviewer, initial.approvals_reviewer);
	assert_ne!(observed.sandbox_policy, initial.sandbox_policy);
	assert!(matches!(
		session
			.client
			.queue_thread_permission_selection(
				&ThreadPermissionSelection::new(&thread, ":read-only").expect("selection"),
				guard
			)
			.await,
		Err(ClientError::StaleHistory)
	));
	if running {
		assert!(session.client.observed_task_permissions(&thread).is_none());
		backend = Some(tokio::spawn(serve(listener.take().expect("held listener"), calls.clone())));
		wait_turn(&mut session, &thread, "turn/completed").await;
		assert_eq!(
			session.client.observed_task_permissions(&thread).expect("idle configured facts").0,
			observed
		);
	}
	drop(session);
	verify_restart(&binary, &root, &thread, observed).await;
	assert_eq!(calls.load(Ordering::Acquire), 1, "settings must not start extra inference");
	backend.expect("started backend").abort();
}

async fn start_turn(session: &mut NativeSession, thread: &str) {
	session
		.client
		.turn_start(
			json!({"threadId":thread,"input":[{"type":"text","text":"Return a fixture answer"}]}),
		)
		.await
		.expect("initial fixture turn");
}

async fn wait_turn(session: &mut NativeSession, thread: &str, expected: &str) {
	loop {
		if let ServerEvent::Notification { method, params } =
			session.events.recv().await.expect("fixture event")
			&& method == expected
			&& params["threadId"] == thread
		{
			if expected == "turn/completed" {
				assert_eq!(params["turn"]["status"], "completed");
			}
			break;
		}
	}
}

async fn verify_restart(
	binary: &std::ffi::OsStr,
	root: &std::path::Path,
	thread: &str,
	observed: NativeTaskPermissions,
) {
	let reopened = NativeSession::start(binary, root);
	let resumed = reopened
		.client
		.thread_resume(json!({"threadId":thread,"excludeTurns":true}))
		.await
		.expect("resume exact thread without defaults");
	assert_eq!(NativeTaskPermissions::from_thread_response(&resumed), Some(observed.clone()));
	assert_eq!(
		reopened.client.observed_task_permissions(thread).expect("resume hydration").0,
		observed
	);
	drop(reopened);
}
