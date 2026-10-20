//! Cross-process archive observations through the retained bridge and a local Responses fixture.
//! Native reference: openai/codex 12cca49c81c8aa6a37a5cc4e39964ee3b70f9458,
//! thread/list useStateDbOnly and thread/archive + thread/unarchive.
use std::{collections::HashSet, env, fs, sync::Arc, time::Duration};

use tokio::sync::Mutex;

use super::{NativeSession, ServerEvent, serve};
use crate::agent_visibility::{ArchiveVisibility, retain_active};
use decodex_protocol::{
	AgentDispatchStateDto, AgentSnapshotDto, AgentWorkItemDto, AgentWorkKindDto, AgentWorkStatusDto,
};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated cross-process archive qualification"]
async fn installed_native_archive_roundtrip_refreshes_without_restarting_either_process() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let backend = tokio::spawn(serve(listener, requests.clone()));
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
		let start = std::time::Instant::now();
		tokio::time::timeout(Duration::from_secs(12), async {
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
				tokio::time::sleep(Duration::from_millis(20)).await;
			}
		})
		.await
		.expect("archive projection must converge without restarting");
		let mut visible = original.clone();
		retain_active(&mut visible, &cache.lock().await.archived);
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

async fn wait_for_completion(session: &mut NativeSession, id: &str) {
	tokio::time::timeout(Duration::from_secs(20), async {
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

fn seed_unrelated_archive(home: &std::path::Path, source: &str) {
	let mut db = rusqlite::Connection::open(home.join("state_5.sqlite"))
		.expect("open isolated native database");
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
		archived_work_ids: Default::default(),
		agent_slots: Default::default(),
		context_references: vec![],
		connection_initializing: false,
		runtime_source: None,
		workspaces: vec![],
		work_items,
		dependencies: vec![],
		pending_events: vec![],
	}
}
