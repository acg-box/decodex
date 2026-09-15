//! Disposable-profile fault qualification; never exposed through the product API.

use super::{SmokeResult, accept, history, idle, send, snapshot, wait_graph, wait_graph_for};
use decodex_core::DecodexRoot;
use decodex_protocol::{
	ChiefActionDto, ChiefClient, ChiefSnapshotDto, EntityId, HistoryText, WireText,
};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use std::{
	collections::HashSet,
	sync::{Mutex, OnceLock},
	time::Duration,
};

pub(super) fn record_threads(graph: &ChiefSnapshotDto) {
	static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
	let mut seen = SEEN.get_or_init(Mutex::default).lock().expect("qualification thread registry");
	for work in &graph.work_items {
		if let Some(thread) = &work.codex_thread_id
			&& seen.insert(thread.clone())
		{
			println!(
				"QUALIFICATION_EVIDENCE {}",
				json!({"kind":"created_thread","workId":work.id,"threadId":thread})
			);
		}
	}
}

fn database(root: &DecodexRoot) -> SmokeResult<Connection> {
	// This helper can inspect only this example's short, disposable profile layout.
	let path = root.as_path().canonicalize()?;
	let parent = path.parent().ok_or("missing disposable parent")?;
	if path.file_name().and_then(|name| name.to_str()) != Some("p")
		|| parent.parent() != Some(std::path::Path::new("/private/tmp"))
		|| !parent
			.file_name()
			.and_then(|name| name.to_str())
			.is_some_and(|name| name.starts_with("dc-"))
	{
		return Err("fault qualification requires the exact disposable profile layout".into());
	}
	Ok(Connection::open_with_flags(
		root.paths().product_database_file(),
		OpenFlags::SQLITE_OPEN_READ_ONLY,
	)?)
}

#[derive(Debug, PartialEq)]
struct Generation {
	id: String,
	account: String,
	pid: i32,
	start: String,
}

fn generation(root: &DecodexRoot) -> SmokeResult<Generation> {
	Ok(database(root)?.query_row(
		"SELECT g.generation_id,g.account_id,g.process_id,g.process_start_id FROM chief_process_bindings b JOIN process_generations g ON g.generation_id=b.generation_id WHERE b.root_id='chief-service-smoke' AND g.state='ready' ORDER BY b.created_at_micros DESC LIMIT 1",
		[], |row| Ok(Generation {id:row.get(0)?,account:row.get(1)?,pid:row.get(2)?,start:row.get(3)?})
	)?)
}

#[cfg(target_os = "macos")]
fn disconnect_owned_child(root: &DecodexRoot, expected: &Generation) -> SmokeResult<()> {
	if generation(root)? != *expected {
		return Err("generation changed before fault injection".into());
	}
	let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
	let size = i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>())?;
	// SAFETY: fixed output structure and flavor agree; no process state is changed.
	let read = unsafe {
		libc::proc_pidinfo(expected.pid, libc::PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast(), size)
	};
	if read != size {
		return Err("exact process metadata is unavailable".into());
	}
	// SAFETY: proc_pidinfo returned the entire fixed-size structure.
	let info = unsafe { info.assume_init() };
	let start = format!("macos-time:{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec);
	// SAFETY: geteuid and getsid perform identity lookups only.
	let (uid, session) = unsafe { (libc::geteuid(), libc::getsid(expected.pid)) };
	if info.pbi_ppid != std::process::id()
		|| info.pbi_uid != uid
		|| start != expected.start
		|| info.pbi_pgid != u32::try_from(expected.pid)?
		|| session != expected.pid
	{
		return Err("refusing signal: exact disposable direct-child identity was not proven".into());
	}
	println!(
		"QUALIFICATION_EVIDENCE {}",
		json!({"kind":"disconnect_owned_child","profile":root.as_path(),"generationId":expected.id,"pid":expected.pid,"parentPid":info.pbi_ppid,"startId":start})
	);
	// SAFETY: the exact persisted generation, start identity, user, direct parent,
	// process group and session were verified immediately above. Signal one PID only.
	if unsafe { libc::kill(expected.pid, libc::SIGTERM) } != 0 {
		return Err(std::io::Error::last_os_error().into());
	}
	Ok(())
}

#[cfg(not(target_os = "macos"))]
fn disconnect_owned_child(_: &DecodexRoot, _: &Generation) -> SmokeResult<()> {
	Err("live disconnect qualification requires macOS process identity checks".into())
}

pub(super) async fn timer_reconnect(client: &ChiefClient, root: &DecodexRoot) -> SmokeResult<()> {
	let before = snapshot(client).await?;
	if !idle(&before) {
		return Err("fault injection requires all work idle".into());
	}
	let original = generation(root)?;
	disconnect_owned_child(root, &original)?;
	let started = std::time::Instant::now();
	let mut diagnostics = [5, 15, 60].into_iter().peekable();
	let restored = tokio::time::timeout(Duration::from_secs(120), async {
		loop {
			if let Ok(next) = generation(root)
				&& next.id != original.id
			{
				break next;
			}
			if diagnostics.peek().is_some_and(|seconds| started.elapsed().as_secs() >= *seconds) {
				diagnostics.next();
				record_process_group(root, &original, started.elapsed().as_secs());
			}
			tokio::time::sleep(Duration::from_millis(500)).await;
		}
	})
	.await
	.map_err(|_| "timer-only recovery did not admit a new ready generation within 120 seconds")?;
	if restored.account != original.account {
		return Err("timer restore changed bound account".into());
	}
	let after = snapshot(client).await?;
	if before.work_items.iter().map(|work| (&work.id, &work.codex_thread_id)).collect::<Vec<_>>()
		!= after.work_items.iter().map(|work| (&work.id, &work.codex_thread_id)).collect::<Vec<_>>()
	{
		return Err("timer restore changed work or thread identities".into());
	}
	println!(
		"QUALIFICATION_EVIDENCE {}",
		json!({"kind":"timer_only_reconnect","profile":root.as_path(),"oldGenerationId":original.id,"newGenerationId":restored.id,"newPid":restored.pid,"sameAccount":true,"sendCommandsDuringRecovery":0})
	);
	Ok(())
}

fn record_process_group(root: &DecodexRoot, original: &Generation, elapsed: u64) {
	let state = database(root).ok().and_then(|db| {
		db.query_row(
			"SELECT state,authority_loss_reason FROM process_generations WHERE generation_id=?1",
			[&original.id],
			|row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
		)
		.ok()
	});
	// Inspect only numeric identities and executable names, never arguments or environment.
	let rows = std::process::Command::new("/bin/ps")
		.args(["-axo", "pid,ppid,pgid,comm"])
		.output()
		.ok()
		.filter(|output| output.status.success())
		.map(|output| String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| {
			let mut fields = line.split_whitespace();
			let pid = fields.next()?.parse::<i32>().ok()?;
			let parent = fields.next()?.parse::<i32>().ok()?;
			let group = fields.next()?.parse::<i32>().ok()?;
			(group == original.pid).then(|| json!({"pid":pid,"parentPid":parent,"processGroup":group,"executable":fields.collect::<Vec<_>>().join(" ")}))
		}).collect::<Vec<_>>());
	println!(
		"QUALIFICATION_EVIDENCE {}",
		json!({"kind":"recovery_process_group","elapsedSeconds":elapsed,"generationId":original.id,"persistedState":state,"processes":rows})
	);
}

fn carryover_event(root: &DecodexRoot) -> SmokeResult<(i64, Option<String>, Option<String>)> {
	Ok(database(root)?.query_row("SELECT id,delivered_turn_id,disposition FROM chief_inbox_events WHERE event_kind='automation_result' AND payload LIKE '%CARRYOVER_PROBE%' ORDER BY id DESC LIMIT 1",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)))?)
}

pub(super) async fn carryover(client: &ChiefClient, root: &DecodexRoot) -> SmokeResult<()> {
	send(client,"carryover-policy","Read-only recovery qualification: the next service-b automation event CARRYOVER_PROBE must deliberately remain undisposed for exactly its first Chief turn. Do not call chief_disposition on that event yet; reply CARRYOVER_HELD and end the turn. On the next explicit user message use chief_list_work to find and resolve that same saved event; do not create or dispatch workers.").await?;
	wait_graph(client, "carryover policy", |graph| {
		idle(graph) && !graph.pending_events.iter().any(|event| event.event_kind == "user_message")
	})
	.await?;
	accept(client,ChiefActionDto::AutomationResult {work_id:EntityId::new("service-b").expect("bounded fixture"),source_event_id:WireText::new("service-carryover-1").expect("bounded fixture"),payload:HistoryText::new(r#"{"observation":"CARRYOVER_PROBE: hold this exact result undisposed for this turn; next explicit user input will ask you to resolve it."}"#).expect("bounded fixture")},"carryover-result").await?;
	wait_graph(client, "carryover held", |graph| {
		idle(graph)
			&& graph.pending_events.iter().any(|event| event.event_kind == "automation_result")
			&& carryover_event(root).is_ok_and(|event| event.1.is_some() && event.2.is_none())
	})
	.await?;
	let held = carryover_event(root)?;
	if held.1.is_none() || held.2.is_some() {
		return Err("model did not leave a delivered result pending".into());
	}
	send(client,"carryover-resolve","Now call chief_list_work, find the earlier CARRYOVER_PROBE automation result in the inbox, and resolve that exact event with chief_disposition. Do not dispatch workers. Summarize CARRYOVER_RESOLVED.").await?;
	wait_graph(client, "carryover resolved", |graph| {
		idle(graph)
			&& !graph.pending_events.iter().any(|event| {
				event.event_kind == "automation_result" || event.event_kind == "user_message"
			})
	})
	.await?;
	let resolved = carryover_event(root)?;
	if held.0 != resolved.0 || held.1 == resolved.1 || resolved.2.is_none() {
		return Err("same held event was not delivered and disposed on a later turn".into());
	}
	println!(
		"QUALIFICATION_EVIDENCE {}",
		json!({"kind":"result_carryover","eventId":held.0,"firstTurn":held.1,"resolvedTurn":resolved.1,"disposition":resolved.2})
	);
	Ok(())
}

pub(super) async fn long_result(client: &ChiefClient, root: &DecodexRoot) -> SmokeResult<()> {
	send(client,"long-result","Read-only output retention qualification. Continue existing service-b exactly once: request a text-only final answer beginning LONG_OUTPUT_BEGIN, followed by roughly 60,000 lowercase a characters (line breaks optional), then LONG_OUTPUT_END. Approximate length is enough: do not spend time counting exactly; aim for 58–65KB total and stop before 70KB. No tools. Do not create work. On its completion, resolve the exact worker event and summarize LONG_OUTPUT_ACCEPTED. This tests bounded retention of a real response above48KB.").await?;
	wait_graph_for(client, "long result retained", Duration::from_secs(600), |graph| {
		idle(graph)
			&& !graph.pending_events.iter().any(|event| {
				event.event_kind == "worker_turn_completed" || event.event_kind == "user_message"
			})
	})
	.await?;
	let db = database(root)?;
	let (event_id,payload):(i64,String) = db.query_row("SELECT id,payload FROM chief_inbox_events WHERE work_item_id='service-b' AND event_kind='worker_turn_completed' ORDER BY id DESC LIMIT 1",[],|row|Ok((row.get(0)?,row.get(1)?)))?;
	let saved: serde_json::Value = serde_json::from_str(&payload)?;
	let readback = &saved["threadReadback"];
	let messages =
		readback["assistantMessages"].as_array().ok_or("saved messages are not structured JSON")?;
	if readback["truncated"] != true
		|| !messages.iter().any(|message| {
			message["text"].as_str().is_some_and(|text| text.contains("LONG_OUTPUT_BEGIN"))
		}) {
		return Err("real model output did not exercise the durable truncation boundary".into());
	}
	let public = history(client, "service-b").await?;
	if !public
		.iter()
		.any(|entry| entry.kind == "assistant" && entry.text.contains("LONG_OUTPUT_BEGIN"))
	{
		return Err("bounded long response is missing from public history".into());
	}
	println!(
		"QUALIFICATION_EVIDENCE {}",
		json!({"kind":"long_result_retained","eventId":event_id,"threadId":readback["threadId"],"turnId":readback["turnId"],"savedPayloadBytes":payload.len(),"structuredMessageCount":messages.len(),"truncated":true,"publicHistoryReadable":true})
	);
	Ok(())
}

pub(super) async fn no_stale_host_errors(client: &ChiefClient) -> SmokeResult<()> {
	let graph = snapshot(client).await?;
	if graph
		.pending_events
		.iter()
		.any(|event| event.source_event_id.starts_with("[\"chief_host\","))
	{
		return Err("reconnected Chief still has an undisposed generic host error".into());
	}
	println!(
		"QUALIFICATION_EVIDENCE {}",
		json!({"kind":"reconnect_attention","pendingHostErrors":0})
	);
	Ok(())
}
