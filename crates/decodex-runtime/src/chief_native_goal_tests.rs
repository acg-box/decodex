use super::*;
use crate::chief_usage_estimate::SourceKey;
use decodex_codex::app_server_client::AppServerClient;
use decodex_core::{AccountId, DecodexRoot, ProcessGenerationId};
use decodex_database::{ChiefDispatchState, ChiefWorkItem, ChiefWorkKind, ChiefWorkStatus};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn native_goal_observation_checks_source_ownership_and_absence() {
	for case in [
		"root",
		"child",
		"unowned",
		"missing",
		"disabled",
		"unsupported",
		"malformed",
		"lost",
		"account",
		"revision",
		"generation",
		"history",
		"thread",
		"work",
		"closed",
		"redacted",
		"long",
	] {
		let home = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();
		root.paths().ensure_layout().unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		store
			.create_chief_work_item(ChiefWorkItem {
				id: "work".into(),
				parent_goal_id: None,
				kind: ChiefWorkKind::Goal,
				title: "Fixture".into(),
				instructions: "Fixture".into(),
				codex_thread_id: None,
				dispatch_state: ChiefDispatchState::Idle,
				active_turn_id: None,
				status: ChiefWorkStatus::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			})
			.await
			.unwrap();
		store.bind_chief_thread("work".into(), "root".into()).await.unwrap();
		let (local, remote) = tokio::io::duplex(65536);
		let (reader, writer) = tokio::io::split(local);
		let (client, _events) = AppServerClient::from_io(reader, writer);
		let target = if matches!(case, "child" | "unowned") { "child" } else { "root" };
		let server = tokio::spawn(serve(remote, target, case));
		let calls = AtomicUsize::new(0);
		let result = read(
			&store,
			|| {
				let later = calls.fetch_add(1, Ordering::SeqCst) > 0;
				let client = client.clone();
				async move {
					if later && case == "closed" {
						return None;
					}
					Some(Source {
						client,
						key: SourceKey {
							generation: ProcessGenerationId::new(
								if later && case == "generation" {
									"20000000-0000-4000-8000-000000000002"
								} else {
									"10000000-0000-4000-8000-000000000001"
								},
							)
							.unwrap(),
							account: AccountId::new(if later && case == "account" {
								"40000000-0000-4000-8000-000000000004"
							} else {
								"30000000-0000-4000-8000-000000000003"
							})
							.unwrap(),
							revision: i64::from(later && case == "revision"),
							history_revision: u64::from(later && case == "history"),
							thread: if later && case == "thread" { "other" } else { "root" }.into(),
							work: if later && case == "work" { "other" } else { "work" }.into(),
						},
					})
				}
			},
			target,
		)
		.await;
		server.await.unwrap();
		match case {
			"root" | "child" | "redacted" | "long" => {
				let Result::Available { work_id, thread_id, goal: Some(goal), .. } = result else {
					panic!("{case}")
				};
				assert_eq!(work_id.as_str(), "work");
				assert_eq!(thread_id.as_str(), target);
				assert_eq!(goal.tokens_used, 12);
				assert_eq!(goal.time_used_seconds, 7);
				assert_eq!(goal.token_budget, Some(11));
				assert_eq!(goal.objective_truncated, matches!(case, "redacted" | "long"));
				assert!(goal.objective.len() <= 8192);
				assert!(!goal.objective.contains("private-access"));
			},
			"missing" => assert!(matches!(result, Result::Available { goal: None, .. })),
			"disabled" => assert_eq!(result, Result::Disabled),
			"unsupported" => assert_eq!(result, Result::Unsupported),
			_ => assert_eq!(result, Result::Unavailable, "{case}"),
		}
		assert!(store.list_chief_wake_events("work".into(), 10).await.unwrap().is_empty());
	}
}

async fn serve(remote: tokio::io::DuplexStream, target: &str, case: &str) {
	let (reader, mut writer) = tokio::io::split(remote);
	let mut lines = BufReader::new(reader).lines();
	if target == "child" {
		let request: Value = serde_json::from_str(
			&lines.next_line().await.expect("goal wire fixture").expect("goal wire fixture"),
		)
		.expect("goal wire fixture");
		assert_eq!(request["method"], "thread/read");
		assert_eq!(request["params"]["threadId"], "child");
		let thread = if case == "unowned" {
			json!({"id":"child","parentThreadId":"root","source":"cli"})
		} else {
			json!({"id":"child","parentThreadId":"root","source":{"subAgent":{"thread_spawn":{"parent_thread_id":"root"}}}})
		};
		writer
			.write_all(
				format!("{}\n", json!({"id":request["id"],"result":{"thread":thread}})).as_bytes(),
			)
			.await
			.expect("goal wire fixture");
		if case == "unowned" {
			assert!(
				tokio::time::timeout(std::time::Duration::from_millis(30), lines.next_line())
					.await
					.is_err()
			);
			return;
		}
	}
	let request: Value = serde_json::from_str(
		&lines.next_line().await.expect("goal wire fixture").expect("goal wire fixture"),
	)
	.expect("goal wire fixture");
	assert_eq!(request["method"], "thread/goal/get");
	assert_eq!(request["params"]["threadId"], target);
	if case == "lost" {
		return;
	}
	let objective = match case {
		"redacted" => "Bearer fixture-private-access-token-123456789".into(),
		"long" => "界".repeat(5000),
		_ => "Native objective".into(),
	};
	let response = match case {
		"disabled" =>
			json!({"id":request["id"],"error":{"code":-32600,"message":"goals feature is disabled"}}),
		"unsupported" =>
			json!({"id":request["id"],"error":{"code":-32601,"message":"unknown method"}}),
		"missing" => json!({"id":request["id"],"result":{"goal":null}}),
		"malformed" => json!({"id":request["id"],"result":{}}),
		_ =>
			json!({"id":request["id"],"result":{"goal":{"threadId":target,"objective":objective,"status":"budgetLimited","tokenBudget":11,"tokensUsed":12,"timeUsedSeconds":7,"createdAt":1,"updatedAt":2}}}),
	};
	writer.write_all(format!("{response}\n").as_bytes()).await.expect("goal wire fixture");
}
