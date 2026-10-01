use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{self, Value};
use tokio::{
	io::{self, AsyncBufReadExt as _, AsyncWriteExt as _, BufReader, DuplexStream},
	sync::oneshot,
	time,
};

use crate::{agent_native_goal::*, agent_usage_estimate::SourceKey};
use decodex_codex::app_server_client::AppServerClient;
use decodex_core::{AccountId, DecodexRoot, ProcessGenerationId};
use decodex_database::{AgentDispatchState, AgentWorkItem, AgentWorkKind, AgentWorkStatus};
use decodex_protocol::{AgentGoalBudgetEdit, AgentGoalEdit};

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
		"unicode",
	] {
		let home = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();

		root.paths().ensure_layout().unwrap();

		let store = SqliteStore::open(&root.paths()).unwrap();

		store
			.create_agent_work_item(AgentWorkItem {
				id: "work".into(),
				parent_goal_id: None,
				kind: AgentWorkKind::Goal,
				title: "Fixture".into(),
				instructions: "Fixture".into(),
				codex_thread_id: None,
				dispatch_state: AgentDispatchState::Idle,
				active_turn_id: None,
				status: AgentWorkStatus::Open,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			})
			.await
			.unwrap();
		store.bind_agent_thread("work".into(), "root".into()).await.unwrap();

		let (local, remote) = io::duplex(65_536);
		let (reader, writer) = io::split(local);
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

		check_observation(result, case, target);

		assert!(store.list_agent_wake_events("work".into(), 10).await.unwrap().is_empty());
	}
}

fn check_observation(result: AgentNativeGoalResult, case: &str, target: &str) {
	match case {
		"root" | "child" | "redacted" | "long" | "unicode" => {
			let AgentNativeGoalResult::Available { work_id, thread_id, goal: Some(goal), .. } =
				result
			else {
				panic!("{case}")
			};

			assert_eq!(work_id.as_str(), "work");
			assert_eq!(thread_id.as_str(), target);
			assert_eq!(goal.tokens_used, 12);
			assert_eq!(goal.time_used_seconds, 7);
			assert_eq!(goal.token_budget, Some(11));
			assert_eq!(goal.objective_truncated, matches!(case, "redacted" | "long"));
			assert!(goal.objective.len() <= 16_000);
			assert!(!goal.objective.contains("private-access"));
		},
		"missing" => assert!(matches!(result, AgentNativeGoalResult::Available { goal: None, .. })),
		"disabled" => assert_eq!(result, AgentNativeGoalResult::Disabled),
		"unsupported" => assert_eq!(result, AgentNativeGoalResult::Unsupported),
		_ => assert_eq!(result, AgentNativeGoalResult::Unavailable, "{case}"),
	}
}

async fn serve(remote: DuplexStream, target: &str, case: &str) {
	let (reader, mut writer) = io::split(remote);
	let mut lines = BufReader::new(reader).lines();

	if target == "child" {
		let request: Value = serde_json::from_str(
			&lines.next_line().await.expect("goal wire fixture").expect("goal wire fixture"),
		)
		.expect("goal wire fixture");

		assert_eq!(request["method"], "thread/read");
		assert_eq!(request["params"]["threadId"], "child");

		let thread = if case == "unowned" {
			serde_json::json!({"id":"child","parentThreadId":"root","source":"cli"})
		} else {
			serde_json::json!({"id":"child","parentThreadId":"root","source":{"subAgent":{"thread_spawn":{"parent_thread_id":"root"}}}})
		};

		writer
			.write_all(
				format!("{}\n", serde_json::json!({"id":request["id"],"result":{"thread":thread}}))
					.as_bytes(),
			)
			.await
			.expect("goal wire fixture");

		if case == "unowned" {
			assert!(
				time::timeout(std::time::Duration::from_millis(30), lines.next_line())
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
		"long" => "界".repeat(5_000),
		"unicode" => "界".repeat(4_000),
		_ => "Native objective".into(),
	};
	let response = match case {
		"disabled" =>
			serde_json::json!({"id":request["id"],"error":{"code":-32_600,"message":"goals feature is disabled"}}),
		"unsupported" =>
			serde_json::json!({"id":request["id"],"error":{"code":-32_601,"message":"unknown method"}}),
		"missing" => serde_json::json!({"id":request["id"],"result":{"goal":null}}),
		"malformed" => serde_json::json!({"id":request["id"],"result":{}}),
		_ =>
			serde_json::json!({"id":request["id"],"result":{"goal":{"threadId":target,"objective":objective,"status":"budgetLimited","tokenBudget":11,"tokensUsed":12,"timeUsedSeconds":7,"createdAt":1,"updatedAt":2}}}),
	};

	writer.write_all(format!("{response}\n").as_bytes()).await.expect("goal wire fixture");
}

#[tokio::test]
async fn native_goal_edits_reject_stale_reviews_and_changed_accounts_before_writing() {
	for case in ["accepted", "stale", "account"] {
		let home = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();

		root.paths().ensure_layout().unwrap();

		let store = SqliteStore::open(&root.paths()).unwrap();
		let (local, remote) = io::duplex(65_536);
		let (reader, writer) = io::split(local);
		let (client, _) = AppServerClient::from_io(reader, writer);
		let key = SourceKey {
			generation: ProcessGenerationId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			account: AccountId::new("30000000-0000-4000-8000-000000000003").unwrap(),
			revision: 1,
			history_revision: client.history_revision(),
			thread: "root".into(),
			work: "work".into(),
		};
		let raw = serde_json::json!({"threadId":"root","objective":"Original","status":"paused","tokenBudget":50,"tokensUsed":1,"timeUsedSeconds":1,"createdAt":1,"updatedAt":1});
		let goal = serde_json::from_value(raw.clone()).unwrap();
		let review =
			review_token(&Source { key: key.clone(), client: client.clone() }, "root", Some(&goal));
		let (release, done) = oneshot::channel();
		let server = tokio::spawn(async move {
			let (reader, mut writer) = io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			let mut current = raw;
			let read: Value =
				serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

			assert_eq!(read["method"], "thread/goal/get");

			if case == "stale" {
				current["objective"] = serde_json::json!("Other edit");
			}

			current["tokensUsed"] = serde_json::json!(2); // Usage updates do not invalidate semantic edits.

			writer
				.write_all(
					format!("{}\n", serde_json::json!({"id":read["id"],"result":{"goal":current}}))
						.as_bytes(),
				)
				.await
				.unwrap();

			if case == "accepted" {
				let update: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();

				assert_eq!(update["method"], "thread/goal/set");
				assert_eq!(
					update["params"],
					serde_json::json!({"threadId":"root","objective":"Updated"})
				);

				current["objective"] = serde_json::json!("Updated");

				writer
					.write_all(
						format!(
							"{}\n",
							serde_json::json!({"id":update["id"],"result":{"goal":current}})
						)
						.as_bytes(),
					)
					.await
					.unwrap();
			} else {
				tokio::select! {
					_ = done => return,
					_ = lines.next_line() => panic!("Rejected {case} edit sent another request"),
				}
			}

			let _ = done.await;
		});
		let calls = AtomicUsize::new(0);
		let result = write(
			&store,
			|| {
				let mut key = key.clone();
				let client = client.clone();

				if calls.fetch_add(1, Ordering::SeqCst) > 0 && case == "account" {
					key.account = AccountId::new("40000000-0000-4000-8000-000000000004").unwrap();
				}

				async move { Some(Source { key, client }) }
			},
			"root",
			&review,
			&AgentGoalEdit {
				objective: Some("Updated".into()),
				status: None,
				budget: AgentGoalBudgetEdit::Keep,
			},
		)
		.await;

		if case == "accepted" {
			result.unwrap();
		} else {
			assert!(
				matches!(result, Err(crate::agent_host::AgentHostError::Rejected(_))),
				"{case}: {result:?}"
			);
		}

		let _ = release.send(());

		server.await.unwrap();
	}
}
