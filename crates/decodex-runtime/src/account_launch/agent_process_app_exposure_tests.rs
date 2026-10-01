//! Host edits use an owned process and retain uncertain receipts across database reopen.
use super::*;
use crate::agent_app_exposure::{Change, read, write};
use decodex_protocol::{AgentAppExposureResult as State, AgentToolExposureSurface as Surface};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Default)]
struct Config {
	preference: Option<Vec<String>>,
	writes: usize,
	reject: bool,
	apply_then_reject: bool,
	missing: bool,
}
async fn server(remote: tokio::io::DuplexStream, state: Arc<std::sync::Mutex<Config>>) {
	let (reader, mut writer) = tokio::io::split(remote);
	let mut lines = BufReader::new(reader).lines();

	while let Some(line) = lines.next_line().await.expect("App exposure fixture") {
		let request: Value = serde_json::from_str(&line).expect("App exposure fixture");
		let result = {
			let mut config = state.lock().expect("App exposure fixture");

			match request["method"].as_str().expect("App exposure fixture") {
				"thread/read" => json!({"thread":{"id":"thread","cwd":"/fixture"}}),
				"app/installed" => {
					assert_eq!(
						request["params"],
						json!({"threadId":"thread","forceRefresh":false})
					);

					if config.missing {
						json!({"apps":[]})
					} else {
						json!({"apps":[{"id":"calendar","enabled":true,"callable":true}]})
					}
				},
				"config/read" => {
					let values = json!({"apps":{"calendar":{"omit_tools_from":config.preference}}});

					json!({"config":values,"layers":[{"name":{"type":"user","file":"/fixture/config.toml"},"version":format!("v{}",config.writes),"config":values}]})
				},
				"config/batchWrite" => {
					assert!(decodex_codex::app_server_client::is_app_tool_exposure_write(
						&request["params"]
					));

					config.writes += 1;

					if !config.reject || config.apply_then_reject {
						config.preference =
							serde_json::from_value(request["params"]["edits"][0]["value"].clone())
								.expect("App exposure fixture");
					}
					if config.reject {
						json!({"fixtureError":true})
					} else {
						json!({"status":"ok","filePath":"/fixture/config.toml","version":format!("v{}",config.writes)})
					}
				},
				method => panic!("unexpected native method {method}"),
			}
		};
		let response = if result["fixtureError"] == true {
			json!({"id":request["id"],"error":{"code":-32_603,"message":"failed to load configuration: /fixture/config.toml:1:24: unclosed array, expected `]`","data":{"private":"do-not-retain"}}})
		} else {
			json!({"id":request["id"],"result":result})
		};

		writer.write_all(format!("{response}\n").as_bytes()).await.expect("App exposure fixture");
	}
}

#[tokio::test]
async fn app_exposure_host_binds_inventory_source_and_durable_attempt() {
	let home = tempfile::tempdir().expect("App exposure fixture");
	let (local, remote) = tokio::io::duplex(65_536);
	let (reader, writer) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let config = Arc::new(std::sync::Mutex::new(Config::default()));
	let backend = tokio::spawn(server(remote, config.clone()));
	let owner = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;

	config.lock().expect("App exposure fixture").missing = true;

	assert_eq!(
		read(&owner.store, || async { Some(owner.source(&owner.key)) }, "calendar").await,
		State::Unavailable
	);

	config.lock().expect("App exposure fixture").missing = false;

	let State::Available { review_token, can_update: true, .. } =
		read(&owner.store, || async { Some(owner.source(&owner.key)) }, "calendar").await
	else {
		panic!("owned inventory")
	};
	let mut changed = owner.key.clone();

	changed.revision += 1;

	assert!(
		write(
			&owner.store,
			|| async { Some(owner.source(&changed)) },
			Change {
				connector: "calendar",
				review: review_token.as_str(),
				omit: Some(vec![Surface::Direct]),
				attempt: "stale"
			}
		)
		.await
		.is_err()
	);
	assert_eq!(config.lock().expect("App exposure fixture").writes, 0);

	write(
		&owner.store,
		|| async { Some(owner.source(&owner.key)) },
		Change {
			connector: "calendar",
			review: review_token.as_str(),
			omit: Some(vec![Surface::Direct]),
			attempt: "first",
		},
	)
	.await
	.expect("App exposure fixture");

	assert_eq!(config.lock().expect("App exposure fixture").writes, 1);
	assert!(
		write(
			&owner.store,
			|| async { Some(owner.source(&owner.key)) },
			Change {
				connector: "calendar",
				review: review_token.as_str(),
				omit: Some(vec![]),
				attempt: "replay"
			}
		)
		.await
		.is_err()
	);
	assert_eq!(config.lock().expect("App exposure fixture").writes, 1);

	let State::Available { review_token, preference, last_outcome, .. } =
		read(&owner.store, || async { Some(owner.source(&owner.key)) }, "calendar").await
	else {
		panic!("saved state")
	};

	assert_eq!(preference, Some(vec!["direct".into()]));
	assert_eq!(last_outcome.as_deref(), Some("saved"));

	config.lock().expect("App exposure fixture").reject = true;

	assert!(
		write(
			&owner.store,
			|| async { Some(owner.source(&owner.key)) },
			Change {
				connector: "calendar",
				review: review_token.as_str(),
				omit: None,
				attempt: "uncertain"
			}
		)
		.await
		.is_err()
	);

	let reopened = SqliteStore::open(&owner.root.paths()).expect("App exposure fixture");
	let receipt = reopened
		.agent_app_settings_receipt(crate::agent_config_settings::digest("/fixture/config.toml"))
		.await
		.expect("App exposure fixture")
		.expect("App exposure fixture");

	assert_eq!(receipt.state, "unknown");
	assert_eq!(receipt.attempt.attempt_id, "uncertain");
	// A persisted claim remains consumed even when a client invents a new command ID.
	let mut replay = receipt.attempt;

	replay.attempt_id = "new-command".into();

	assert!(
		reopened
			.reserve_agent_app_settings_attempt(replay)
			.await
			.expect("App exposure fixture")
			.is_none()
	);
	assert_eq!(config.lock().expect("App exposure fixture").writes, 2);

	let (events, _) = reopened
		.read_agent_transcript("root".into(), None, 100)
		.await
		.expect("diagnostics after reopen");

	assert!(events.iter().any(|e| e.payload.contains("/fixture/config.toml:1:24: unclosed array")));
	assert!(events.iter().all(|e| !e.payload.contains("do-not-retain")));
	assert!(
		reopened.list_agent_wake_events("root".into(), 100).await.expect("wake events").is_empty()
	);

	backend.abort();
}

#[tokio::test]
async fn legacy_app_exposure_outcomes_survive_reopen_without_becoming_native_preferences() {
	let home = tempfile::tempdir().expect("legacy exposure fixture");
	let (local, remote) = tokio::io::duplex(65_536);
	let (reader, writer) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let config = Arc::new(std::sync::Mutex::new(Config::default()));
	let backend = tokio::spawn(server(remote, config.clone()));
	let owner = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
	let mut previous_review: Option<String> = None;

	for state in ["reserved", "saved", "rejected", "unknown"] {
		let connection = rusqlite::Connection::open(owner.root.paths().product_database_file())
			.expect("fixture database");
		let source = format!("app-exposure:legacy-{state}");
		let payload = json!({"attempt":{"work":owner.key.work,"thread":"thread","generation":owner.key.generation.as_str(),"connector":"calendar","review_token":"a".repeat(64),"preference":["direct"],"attempt_id":"legacy"}}).to_string();

		connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'app_exposure_attempt',?3,1,'resolved','legacy reservation',1)",rusqlite::params![source,owner.key.work,payload]).expect("legacy attempt");

		if state != "reserved" {
			let result =
				json!({"reservation":connection.last_insert_rowid(),"state":state}).to_string();

			connection.execute("INSERT INTO agent_inbox_events(source_event_id,work_item_id,event_kind,payload,created_at_micros,disposition,disposition_note,disposed_at_micros) VALUES(?1,?2,'app_exposure_result',?3,2,'resolved','legacy result',2)",rusqlite::params![format!("{source}:result"),owner.key.work,result]).expect("legacy result");
		}

		drop(connection);

		let reopened = SqliteStore::open(&owner.root.paths()).expect("reopen legacy records");
		let State::Available { last_outcome, preference, review_token, .. } =
			read(&reopened, || async { Some(owner.source(&owner.key)) }, "calendar").await
		else {
			panic!("legacy state")
		};

		assert_eq!(last_outcome.as_deref(), Some(state));
		assert_eq!(preference, None, "history is not current configuration");

		for (work, thread, connector) in [
			("other-work", "thread", "calendar"),
			(owner.key.work.as_str(), "other-thread", "calendar"),
			(owner.key.work.as_str(), "thread", "other-connector"),
		] {
			assert!(
				reopened
					.legacy_agent_app_exposure_outcome(work.into(), thread.into(), connector.into())
					.await
					.expect("scoped legacy read")
					.is_none()
			);
		}

		assert!(
			reopened
				.agent_app_settings_receipt(crate::agent_config_settings::digest(
					"/fixture/config.toml"
				))
				.await
				.expect("shared journal")
				.is_none()
		);

		if let Some(review) = previous_review.as_ref() {
			assert!(
				write(
					&reopened,
					|| async { Some(owner.source(&owner.key)) },
					Change {
						connector: "calendar",
						review,
						omit: Some(vec![Surface::Direct]),
						attempt: "stale-legacy-review"
					}
				)
				.await
				.is_err()
			);
		}

		previous_review = Some(review_token.as_str().to_owned());

		assert_eq!(config.lock().expect("fixture config").writes, 0);
	}

	write(
		&owner.store,
		|| async { Some(owner.source(&owner.key)) },
		Change {
			connector: "calendar",
			review: previous_review.as_deref().expect("fresh review"),
			omit: Some(vec![Surface::Direct]),
			attempt: "explicit-current-edit",
		},
	)
	.await
	.expect("new reviewed edit");

	let State::Available { last_outcome, preference, .. } =
		read(&owner.store, || async { Some(owner.source(&owner.key)) }, "calendar").await
	else {
		panic!("current state")
	};

	assert_eq!(last_outcome.as_deref(), Some("saved"));
	assert_eq!(preference, Some(vec!["direct".into()]));
	assert_eq!(config.lock().expect("fixture config").writes, 1);

	backend.abort();
}

#[tokio::test]
async fn applied_app_exposure_error_recovers_after_reopen_without_replaying_write() {
	let home = tempfile::tempdir().expect("App exposure recovery fixture");
	let (local, remote) = tokio::io::duplex(65_536);
	let (reader, writer) = tokio::io::split(local);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let config = Arc::new(std::sync::Mutex::new(Config {
		reject: true,
		apply_then_reject: true,
		..Config::default()
	}));
	let backend = tokio::spawn(server(remote, config.clone()));
	let owner = OwnedReviewer::new(home.path(), &client, "thread", "turn").await;
	let State::Available { review_token, can_update: true, .. } =
		read(&owner.store, || async { Some(owner.source(&owner.key)) }, "calendar").await
	else {
		panic!("owned settings review")
	};

	assert!(
		write(
			&owner.store,
			|| async { Some(owner.source(&owner.key)) },
			Change {
				connector: "calendar",
				review: review_token.as_str(),
				omit: Some(vec![]),
				attempt: "applied-error",
			}
		)
		.await
		.is_err()
	);

	let reopened = SqliteStore::open(&owner.root.paths()).expect("App exposure recovery fixture");
	let scope = crate::agent_config_settings::digest("/fixture/config.toml");

	assert_eq!(
		reopened
			.agent_app_settings_receipt(scope.clone())
			.await
			.expect("App exposure recovery fixture")
			.expect("App exposure recovery fixture")
			.state,
		"unknown"
	);

	let State::Available { preference, last_outcome, can_update, .. } =
		read(&reopened, || async { Some(owner.source(&owner.key)) }, "calendar").await
	else {
		panic!("reconciled settings")
	};

	assert_eq!(preference, Some(vec![]));
	assert_eq!(last_outcome.as_deref(), Some("target_observed"));
	assert!(can_update);
	assert_eq!(
		reopened
			.agent_app_settings_receipt(scope)
			.await
			.expect("App exposure recovery fixture")
			.expect("App exposure recovery fixture")
			.state,
		"target_observed"
	);
	assert_eq!(
		config.lock().expect("App exposure recovery fixture").writes,
		1,
		"recovery must only read native state"
	);

	backend.abort();
}
