//! Cross-process archive observations through the retained bridge and a local Responses fixture.
//! Native reference: openai/codex 33d306d457a4632b22bebf214e0c25d91f90f6e8,
//! thread/list useStateDbOnly and thread/archive, thread/unarchive and thread/delete.
use std::{
	collections::HashSet,
	env, fs,
	path::Path,
	sync::{Arc, atomic::AtomicUsize},
	time::{Duration, Instant},
};

use rusqlite::Connection;
use tokio::{net::TcpListener, sync::Mutex, time};

use crate::{
	account_launch::agent_process::native_tests::{self, NativeSession, ServerEvent},
	agent_visibility::{self, ArchiveVisibility},
};
use decodex_protocol::{
	AgentDispatchStateDto, AgentSnapshotDto, AgentWorkItemDto, AgentWorkKindDto, AgentWorkStatusDto,
};

fn seed_unrelated_archive(home: &Path, source: &str) {
	let mut db =
		Connection::open(home.join("state_5.sqlite")).expect("open isolated native database");

	db.busy_timeout(Duration::from_secs(5)).expect("set isolated database timeout");

	let columns: Vec<String> = db
		.prepare("PRAGMA table_info(threads)")
		.expect("prepare native column inspection")
		.query_map([], |row| row.get(1))
		.expect("read native columns")
		.collect::<Result<_, _>>()
		.expect("collect native columns");
	let values = columns
		.iter()
		.map(|column| match column.as_str() {
			"id" => "?1".to_owned(),
			"archived" => "1".to_owned(),
			"archived_at" => "1".to_owned(),
			_ => format!("\"{column}\""),
		})
		.collect::<Vec<_>>()
		.join(",");
	let sql = format!("INSERT INTO threads SELECT {values} FROM threads WHERE id=?2");
	let transaction = db.transaction().expect("start isolated archive seed");

	for index in 0..10_300 {
		let id = format!("10000000-0000-4000-8000-{index:012}");

		assert_eq!(
			transaction
				.execute(&sql, rusqlite::params![id, source])
				.expect("insert unrelated fixture archive"),
			1
		);
	}

	transaction.commit().expect("commit isolated archive seed");
}

fn snapshot(ids: &[String]) -> AgentSnapshotDto {
	let work_items = ["main", "archivable", "child"]
		.into_iter()
		.enumerate()
		.map(|(index, id)| AgentWorkItemDto {
			id: id.into(),
			parent_goal_id: match index {
				0 => None,
				1 => Some("main".into()),
				_ => Some("archivable".into()),
			},
			kind: AgentWorkKindDto::Goal,
			title: id.into(),
			codex_thread_id: Some(ids[index].clone()),
			active_turn_id: None,
			dispatch_state: AgentDispatchStateDto::Idle,
			status: AgentWorkStatusDto::Open,
			next_check_at_micros: None,
			created_at_micros: 1,
			updated_at_micros: 1,
		})
		.collect();

	AgentSnapshotDto {
		agents: Default::default(),
		guardian: Default::default(),
		portfolio: Default::default(),
		archived_work_ids: Default::default(),
		agent_slots: Default::default(),
		context_references: vec![],
		connection_initializing: false,
		runtime_source: None,
		projects: vec![],
		work_items,
		dependencies: vec![],
		pending_events: vec![],
	}
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated cross-process archive qualification"]
async fn installed_native_archive_roundtrip_refreshes_without_restarting_either_process() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(native_tests::serve(listener, requests.clone()));

	fs::write(home.path().join("config.toml"), format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[model_providers.fixture]\nname=\"Archive fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).unwrap();

	let mut actor = NativeSession::start(&binary, home.path());
	let mut ids = Vec::new();

	for _ in 0..3 {
		let response = actor
			.client
			.thread_start(serde_json::json!({
				"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"
			}))
			.await
			.unwrap();
		let id = response["thread"]["id"].as_str().unwrap().to_owned();

		actor
			.client
			.turn_start(
				serde_json::json!({"threadId":id,"input":[{"type":"text","text":"Archive fixture"}]}),
			)
			.await
			.unwrap();

		wait_for_completion(&mut actor, &id).await;

		ids.push(id);
	}

	seed_unrelated_archive(home.path(), &ids[0]);

	let observer = NativeSession::start(&binary, home.path());
	let process_ids = (observer.child.0.id(), actor.child.0.id());
	let threads: HashSet<_> = ids.iter().cloned().collect();
	let initial = observer.client.observe_thread_archives(&threads).await;

	assert_eq!(initial.len(), 3);
	assert!(
		initial
			.values()
			.all(|state| *state == decodex_codex::app_server_client::ThreadArchiveState::Active)
	);

	let original = snapshot(&ids);
	let cache = Arc::new(Mutex::new(ArchiveVisibility::default()));

	for archived in [true, false, true, false] {
		actor
			.client
			.request(
				if archived { "thread/archive" } else { "thread/unarchive" },
				serde_json::json!({"threadId":ids[1]}),
			)
			.await
			.unwrap();

		let start = Instant::now();

		time::timeout(Duration::from_secs(12), async {
			loop {
				ArchiveVisibility::refresh(
					&cache,
					"same-process",
					observer.client.clone(),
					threads.clone(),
				)
				.await;

				if cache.lock().await.archived.contains(&ids[1]) == archived {
					break;
				}

				time::sleep(Duration::from_millis(20)).await;
			}
		})
		.await
		.expect("archive projection must converge without restarting");

		let mut visible = original.clone();

		agent_visibility::retain_active(&mut visible, &cache.lock().await.archived);

		assert!(visible.is_valid());

		if archived {
			assert_eq!(visible.work_items.len(), 2);
			assert_eq!(visible.work_items[1].parent_goal_id.as_deref(), Some("main"));
		} else {
			assert_eq!(visible, original, "restore original identities and hierarchy");
		}

		assert_eq!(process_ids, (observer.child.0.id(), actor.child.0.id()));
		eprintln!("native archive={archived}: projection updated in {:?}", start.elapsed());
	}

	assert_eq!(
		requests.load(std::sync::atomic::Ordering::Acquire),
		3,
		"archive observations must not start additional turns"
	);

	backend.abort();
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native permanent deletion"]
async fn installed_native_delete_removes_only_the_selected_archive_and_survives_reopen() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(native_tests::serve(listener, requests.clone()));
	fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[model_providers.fixture]\nname=\"Delete fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).unwrap();
	let mut actor = NativeSession::start(&binary, home.path());
	let mut ids = Vec::new();
	for _ in 0..2 {
		let response=actor.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.unwrap();
		let id = response["thread"]["id"].as_str().unwrap().to_owned();
		actor
			.client
			.turn_start(
				serde_json::json!({"threadId":id,"input":[{"type":"text","text":"Isolated deletion fixture"}]}),
			)
			.await
			.unwrap();
		wait_for_completion(&mut actor, &id).await;
		ids.push(id);
	}
	actor.client.request("thread/archive", serde_json::json!({"threadId":ids[0]})).await.unwrap();
	if let Err(error) = actor.client.thread_delete(&ids[0]).await {
		match error {
			decodex_codex::app_server_client::ClientError::Remote(error) =>
				panic!("native deletion {}: {}", error.code, error.message),
			other => panic!("native deletion: {other:?}"),
		}
	}
	drop(actor);
	let observer = NativeSession::start(&binary, home.path());
	assert!(observer.client.archived_thread_absent(&ids[0]).await.unwrap());
	assert_eq!(
		observer.client.thread_archive_state(&ids[1]).await.unwrap(),
		decodex_codex::app_server_client::ThreadArchiveState::Active
	);
	let kept = observer
		.client
		.thread_read(serde_json::json!({"threadId":ids[1],"includeTurns":true}))
		.await
		.unwrap();
	assert!(!kept["thread"]["turns"].as_array().unwrap().is_empty());
	assert_eq!(
		requests.load(std::sync::atomic::Ordering::Acquire),
		2,
		"archive and deletion must not start another turn"
	);
	backend.abort();
}

async fn wait_for_completion(session: &mut NativeSession, id: &str) {
	time::timeout(Duration::from_secs(20), async {
		loop {
			if let ServerEvent::Notification { method, params } =
				session.events.recv().await.expect("native fixture event stream")
				&& method == "turn/completed"
				&& params["threadId"] == id
			{
				assert_eq!(params["turn"]["status"], "completed");

				break;
			}
		}
	})
	.await
	.expect("native fixture turn must complete");
}
