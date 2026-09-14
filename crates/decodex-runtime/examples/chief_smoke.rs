//! Opt-in live qualification on a disposable product database.

// These dependencies belong to the runtime library, not this auxiliary target.
use base64 as _;
#[cfg(target_os = "macos")] use core_foundation as _;
use decodex_account_login as _;
use decodex_protocol as _;
use futures_util as _;
use libc as _;
use reqwest as _;
use rusqlite as _;
#[cfg(target_os = "macos")] use security_framework as _;
use serde as _;
use sha2 as _;
use tokio_tungstenite as _;
use zeroize as _;
// Run with: cargo run -p decodex-runtime --example chief_smoke -- MODEL ABSOLUTE_CWD
// This submits small model turns. It does not open the installed product database.

use std::{error::Error, time::Duration};

use decodex_codex::app_server_client::AppServerClient;
use decodex_core::DecodexRoot;
use decodex_database::{ChiefDispatchState, ChiefDisposition, ChiefWorkStatus, SqliteStore};
use decodex_runtime::{ChiefConfig, ChiefCoordinator};
use serde_json::json;
use tokio::{process::Command, time::timeout};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	let mut arguments = std::env::args().skip(1);
	let model = arguments.next().ok_or("explicit MODEL required")?;
	let cwd = arguments.next().ok_or("explicit ABSOLUTE_CWD required")?;
	if !std::path::Path::new(&cwd).is_absolute() {
		return Err("working directory must be absolute".into());
	}
	let temporary = tempfile::tempdir()?;
	let root = DecodexRoot::new(temporary.path().canonicalize()?.join("product"))?;
	root.paths().ensure_layout()?;
	let store = SqliteStore::open(&root.paths())?;
	let mut command = Command::new("codex");
	command.arg("app-server").current_dir(&cwd);
	let (client, mut events, mut process) = AppServerClient::spawn(&mut command)?;
	let mut config = ChiefConfig::new(model.clone(), "medium".into(), cwd);
	config.sandbox = "read-only".into();
	config.approval_policy = json!("never");
	let result = timeout(Duration::from_secs(300), async {
		let mut chief = ChiefCoordinator::new(store.clone(), client.clone(), config)?;
		chief.initialize().await?;
		println!("App-server initialized.");
		let catalog = client.request("model/list", json!({})).await?;
		if !catalog["data"].as_array().is_some_and(|entries| {
			entries.iter().any(|entry| entry["model"] == model)
		}) {
			return Err::<(), Box<dyn Error>>("selected model is not available".into());
		}
		println!("Selected model is available; starting Chief thread.");
		chief.start_chief("smoke-chief", "This is a read-only coordination test. Reply READY and finish this initial turn. Do not create work or use tools now. Later worker results will arrive; inspect those results and record a resolved disposition for their exact inbox event IDs using your tools. Do not use shell, network, or file tools, and do not create additional workers.").await?;
		loop {
			chief.handle_event(events.recv().await.ok_or("event stream closed")?).await?;
			if store.get_chief_work_item("smoke-chief".into()).await?.dispatch_state == ChiefDispatchState::Idle {
				break;
			}
		}
		// The original Chief turn has ended; independent work now starts on the same connection.
		chief.create_worker("smoke-chief", "smoke-a", "Reply with exactly RESULT_A. Do not use any tools.").await?;
		chief.create_worker("smoke-chief", "smoke-b", "Reply with exactly RESULT_B. Do not use any tools.").await?;
		loop {
			chief.handle_event(events.recv().await.ok_or("event stream closed")?).await?;
			let work = store.list_chief_work_items().await?;
			let pending = store.list_pending_chief_events(100).await?;
			let workers_idle = work.iter().filter(|item| item.id != "smoke-chief")
				.all(|item| item.dispatch_state == ChiefDispatchState::Idle);
			let chief_idle = work.iter().any(|item| item.id == "smoke-chief" && item.dispatch_state == ChiefDispatchState::Idle);
			let worker_pending = pending.iter().any(|event| event.event_kind == "worker_turn_completed");
			if workers_idle && chief_idle && !worker_pending { break; }
		}
		let work = store.list_chief_work_items().await?;
		let ids: std::collections::HashSet<_> = work.iter().filter_map(|item| item.codex_thread_id.as_ref()).collect();
		if ids.len() != 3 { return Err("expected three independent Codex threads".into()); }
		println!("Initial evidence passed: three independent threads; both worker results disposed after the initial Chief ended.");
		let original_worker = store.get_chief_work_item("smoke-a".into()).await?;
		let first_results = store.read_chief_work_events("smoke-a".into(), 100).await?;
		let first_result = first_results.iter().find(|event| event.event_kind == "worker_turn_completed").ok_or("first worker result missing")?;
		chief.enqueue_user_message("smoke-chief", "repair-and-decision-policy", "Continue the EXISTING smoke-a worker exactly once using chief_continue_worker. Ask it to reply exactly REPAIRED_A without tools. Do not create a worker or goal. Record a resolved disposition for this user_message event and for the repaired worker completion when it arrives. For the later automation result whose source_event_id is smoke:automation:decision:1, record a user_decision disposition on its exact event ID and ask the user to select option A or B; do not choose for them. Do not use shell, network, or file tools.").await?;
		chief.wake_pending().await?;
		loop {
			chief.handle_event(events.recv().await.ok_or("event stream closed during repair")?).await?;
			let worker = store.get_chief_work_item("smoke-a".into()).await?;
			let root_work = store.get_chief_work_item("smoke-chief".into()).await?;
			let results = store.read_chief_work_events("smoke-a".into(), 100).await?;
			let completions: Vec<_> = results.iter().filter(|event| event.event_kind == "worker_turn_completed").collect();
			if completions.len() > 2 { return Err("repair dispatched more than once".into()); }
			if completions.len() == 2 && completions.iter().all(|event| event.disposition == Some(ChiefDisposition::Resolved))
				&& worker.dispatch_state == ChiefDispatchState::Idle && root_work.dispatch_state == ChiefDispatchState::Idle {
				if worker.codex_thread_id != original_worker.codex_thread_id { return Err("repair replaced the original worker thread".into()); }
				let repaired = completions.iter().find(|event| event.id != first_result.id).ok_or("repair completion missing")?;
				if !repaired.payload.contains("REPAIRED_A") { return Err("repair output marker missing".into()); }
				println!("Repair evidence passed: same smoke-a thread, distinct completion events {} and {}, repaired output observed.", first_result.id, repaired.id);
				break;
			}
		}
		let source = "smoke:automation:decision:1";
		let payload = json!({"observation":"Options A and B are available; user selection is required."});
		chief.ingest_automation_result(source, "smoke-a", payload.clone()).await?;
		chief.ingest_automation_result(source, "smoke-a", payload.clone()).await?;
		loop {
			chief.handle_event(events.recv().await.ok_or("event stream closed during automation")?).await?;
			let root_work = store.get_chief_work_item("smoke-chief".into()).await?;
			let records = store.read_chief_work_events("smoke-a".into(), 100).await?;
			let receipts: Vec<_> = records.iter().filter(|event| event.source_event_id == source).collect();
			if receipts.len() != 1 { return Err("duplicate automation intake created multiple receipts".into()); }
			if root_work.dispatch_state == ChiefDispatchState::Idle && receipts[0].disposition.is_some() {
				if receipts[0].disposition != Some(ChiefDisposition::UserDecision) { return Err("automation did not preserve the user decision".into()); }
				let delivered = receipts[0].delivered_turn_id.clone().ok_or("automation delivery was not acknowledged")?;
				chief.ingest_automation_result(source, "smoke-a", payload).await?;
				let after = store.get_chief_inbox_event(receipts[0].id).await?;
				if after != *receipts[0] || store.get_chief_work_item("smoke-chief".into()).await?.dispatch_state != ChiefDispatchState::Idle { return Err("disposed automation duplicate woke or changed the Chief".into()); }
				if store.get_chief_work_item("smoke-a".into()).await?.status != ChiefWorkStatus::UserDecision { return Err("user decision work state missing".into()); }
				println!("Automation evidence passed: one receipt {}, one delivery {}, duplicate before and after disposition unchanged; user decision persisted.", after.id, delivered);
				break;
			}
		}
		let work = store.list_chief_work_items().await?;
		if work.len() != 3 { return Err("repair or automation created extra work".into()); }
		let worker_events = store.read_chief_work_events("smoke-a".into(), 100).await?;
		let reopened = SqliteStore::open(&root.paths())?;
		if reopened.list_chief_work_items().await? != work { return Err("reopen changed work state".into()); }
		if reopened.read_chief_work_events("smoke-a".into(), 100).await? != worker_events { return Err("reopen changed repair or automation evidence".into()); }
		for item in &work {
			if let Some(thread_id) = &item.codex_thread_id {
				client.request("thread/archive", json!({"threadId":thread_id})).await?;
			}
		}
		println!("Chief qualification passed: late worker results, original-thread repair, stable-source automation deduplication, explicit user decision, durable work and event reopen.");
		Ok(())
	}).await;
	process.shutdown().await?;
	let result = result.map_err(|_| "Chief smoke timed out; no automatic dispatch retry")?;
	result?;
	Ok(())
}
