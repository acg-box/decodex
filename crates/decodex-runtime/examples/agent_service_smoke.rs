//! Explicit live smoke through the production service and same-UID client.
//! Keep-alive imports retain dependencies owned by the runtime library.
//! Uses a disposable database and enrolls the current Codex login through AccountClient.
//! Does not route accounts or replace the installed Decodex service.

#[path = "agent_service_smoke/evidence.rs"] mod evidence;
#[path = "agent_service_smoke/reliability.rs"] mod reliability;

use std::{
	collections::HashSet,
	env::{self, VarError},
	error::Error,
	fs::OpenOptions,
	io::Write,
	os::unix::fs::OpenOptionsExt,
	path::Path,
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use base64 as _;
#[cfg(target_os = "macos")] use core_foundation as _;
use futures_util as _;
use reqwest as _;
use rusqlite as _;
#[cfg(target_os = "macos")] use security_framework as _;
use serde as _;
use sha2 as _;
use tempfile::{Builder, TempDir};
use time as _;
use tokio_tungstenite as _;
use zeroize as _;

use decodex_account_login as _;
use decodex_codex as _;
use decodex_core::{DecodexRoot, ProcessExecutionAuthorization, ServerIdentity};
use decodex_database as _;
use decodex_protocol::{
	AccountClient, AccountCommandResponse, AccountInspectResult, AccountProfileResult,
	AccountQuotaStateDto, AgentActionDto, AgentCapabilitiesResult, AgentClient,
	AgentCommandResponse, AgentDispatchStateDto, AgentHistoryEntryDto, AgentHistoryResult,
	AgentSandboxDto, AgentSnapshotDto, AgentSnapshotResult, AgentStartDto, AgentWorkStatusDto,
	ClientProfile, CommandPayload, ConversationModel, ConversationReasoningEffort,
	ConversationWorkingDirectory, EntityId, HistoryText, IdempotencyKey, ResetCardClient,
	ResetCardInventoryResult, WireText,
};
use decodex_runtime::{ServerConfig, ServiceComposition};

type SmokeResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, PartialEq)]
enum SmokeScope {
	Evidence,
	Full,
	Reconnect,
	LongOutput,
	Hierarchy,
}
impl SmokeScope {
	fn label(self) -> &'static str {
		match self {
			Self::Evidence => "EVIDENCE",
			Self::Full => "FULL",
			Self::Reconnect => "RECONNECT_ONLY",
			Self::LongOutput => "LONG_OUTPUT_ONLY",
			Self::Hierarchy => "HIERARCHY",
		}
	}

	fn selected() -> SmokeResult<Self> {
		match env::var("DECODEX_SMOKE_SCOPE").as_deref() {
			Err(VarError::NotPresent) | Ok("full") => Ok(Self::Full),
			Ok("reconnect") => Ok(Self::Reconnect),
			Ok("long-output") => Ok(Self::LongOutput),
			Ok("hierarchy") => Ok(Self::Hierarchy),
			Ok("evidence") => Ok(Self::Evidence),
			_ => Err(
				"DECODEX_SMOKE_SCOPE must be full, reconnect, long-output, hierarchy or evidence"
					.into(),
			),
		}
	}
}

fn id() -> EntityId {
	EntityId::new(ServerIdentity::generate().expect("valid bounded qualification fixture").as_str())
		.expect("valid bounded qualification fixture")
}

fn idle(graph: &AgentSnapshotDto) -> bool {
	graph.work_items.iter().all(|work| work.dispatch_state == AgentDispatchStateDto::Idle)
}

fn choice_receipts(entries: &[AgentHistoryEntryDto]) -> usize {
	entries
		.iter()
		.filter(|entry| {
			entry.kind == "automation"
				&& entry.text.contains("Choose summary format A concise or B detailed")
		})
		.count()
}

fn disposable_profile() -> SmokeResult<(TempDir, DecodexRoot)> {
	// Short canonical path is required for the platform Unix socket limit.
	let temporary = Builder::new().prefix("dc-").tempdir_in("/private/tmp")?;
	let root = DecodexRoot::new(temporary.path().join("p"))?;
	let paths = root.paths();

	paths.ensure_layout()?;

	let mut file =
		OpenOptions::new().write(true).create_new(true).mode(0o600).open(paths.config_file())?;

	// SAFETY: geteuid reads the current user's numeric identity.
	write!(
		file,
		"version = 1\nactive_profile = \"local\"\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {}\n[cache]\nmax_entries = 128\nmax_bytes = 1048576\nmax_entry_bytes = 65536\n",
		unsafe { libc::geteuid() }
	)?;

	file.sync_all()?;

	ProcessExecutionAuthorization::load_or_create(&paths)?;

	Ok((temporary, root))
}

fn hierarchy_prompt(directory: &Path) -> String {
	format!(
		"Read-only integration qualification. Use only Agent coordination tools; never shell, file, network or native subagent tools. The execution directory is {}. Create exactly one subordinate Agent with agent_create_manager: id project. Its prompt: use only Agent coordination tools, create exactly one subordinate Agent with id team; tell team to create exactly one worker id leaf whose sole task is to reply LEAF_RESULT with no tools. Each manager must assess its direct child's completion events, resolve each exact delivered event, and emit its own final result only after its child result is verified. Team final marker TEAM_RESULT, project final marker PROJECT_RESULT. Managers may finish initial turns while waiting, then act on later completion inbox events. You must consume project completion events; resolve initial waiting events without treating them as final success, and only emit HIERARCHY_DONE when PROJECT_RESULT arrives. Do not create other work. Finish your initial turn with SERVICE_READY.",
		directory.display()
	)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
	let model = env::args().nth(1).ok_or("explicit MODEL required")?;
	let scope = SmokeScope::selected()?;
	// Production process admission requires a selected directory beneath the
	// effective user's home. The disposable socket root is deliberately elsewhere.
	let working_directory = env::current_dir()?.canonicalize()?;
	let (_temporary, root) = disposable_profile()?;

	println!("Bootstrapping disposable service (120-second deadline).");

	let boot =
		tokio::time::timeout(Duration::from_secs(120), ServiceComposition::bootstrap(root.clone()))
			.await?;

	println!("Disposable service bootstrap completed.");

	let mut service =
		tokio::time::timeout(Duration::from_secs(10), boot.bind(ServerConfig::default())).await??;

	println!("Disposable service root: {}", root.as_path().display());

	let run = async {
		let (client, account) = enroll_service_account(&root).await?;
		let outcome=client.execute(AgentActionDto::Start(AgentStartDto {
			root_id:EntityId::new("agent-service-smoke").expect("valid bounded qualification fixture"),
			prompt:HistoryText::new(if scope == SmokeScope::Evidence { evidence::prompt() } else if scope == SmokeScope::Hierarchy { hierarchy_prompt(&working_directory) } else { "Read-only coordination qualification. Use only Agent coordination tools; never shell, file, network or native subagent tools. Create exactly two independent workers directly under this Agent: service-a asks its worker to reply exactly DRAFT_A without tools; service-b asks its worker to reply exactly RESULT_B without tools. Finish this initial turn with SERVICE_READY after creating both. When completion inbox events arrive later, assess each, record resolved disposition only if the requested output was delivered; otherwise record follow_up with the actual problem. Summarize. Do not create more work. Later explicit user inputs will request a repair and a decision.".into() }).expect("valid bounded qualification fixture"),
			model:ConversationModel::new(model).map_err(|_|"invalid model")?,effort:Some(ConversationReasoningEffort::Medium),
			cwd:ConversationWorkingDirectory::new(working_directory.to_string_lossy()).map_err(|_|"invalid smoke directory")?,account_id:Some(account),sandbox:AgentSandboxDto::ReadOnly,
		}),IdempotencyKey::new("smoke-start").expect("valid bounded qualification fixture")).await?;

		verify_initial_service(&client, outcome, &root, scope).await
	};
	let result = match tokio::time::timeout(Duration::from_secs(900), run).await {
		Ok(result) => result,
		Err(_) => Err("service closed-loop pre-restart deadline exceeded".into()),
	};
	// Capture created identities even when a later qualification assertion fails.
	let _ = snapshot(&AgentClient::new(ClientProfile::load(root.as_path(), None)?)).await;
	let original_thread =
		if result.is_ok() { agent_thread(&root).await.map(Some) } else { Ok(None) };
	let original_workers = if result.is_ok() {
		snapshot(&AgentClient::new(ClientProfile::load(root.as_path(), None)?)).await.map(Some)
	} else {
		Ok(None)
	};

	if result.is_ok() {
		capture_before_restart().await;
	}

	let stopped = Instant::now();

	tokio::time::timeout(Duration::from_secs(10), service.shutdown()).await??;

	println!("Production service shutdown completed in {:?}.", stopped.elapsed());

	result?;

	if scope != SmokeScope::Full {
		println!("{} passed through the production service.", scope.label());

		return Ok(());
	}

	let original_thread = original_thread?;
	let original_workers = original_workers?;

	println!("Rebootstrapping disposable service (120-second deadline).");

	let boot =
		tokio::time::timeout(Duration::from_secs(120), ServiceComposition::bootstrap(root.clone()))
			.await?;
	let (mut service, resumed) = (tokio::time::timeout(Duration::from_secs(10), boot.bind(ServerConfig::default())).await??,
async {
		if Some(agent_thread(&root).await?) != original_thread {return Err::<(),Box<dyn Error>>("Agent thread identity changed after restart".into());}

		let client=AgentClient::new(ClientProfile::load(root.as_path(),None)?);

		closed_loop_after_restart(&client,original_workers.as_ref().ok_or("missing original graph")?).await?;

		let outcome=client.execute(AgentActionDto::Send {
			root_id:EntityId::new("agent-service-smoke").expect("valid bounded qualification fixture"),
			text:HistoryText::new("Continue this same read-only qualification. Reply SERVICE_RESTORED. Do not use tools or create workers.").expect("valid bounded qualification fixture"),
		},IdempotencyKey::new("smoke-resume").expect("valid bounded qualification fixture")).await?;

		if !matches!(outcome,AgentCommandResponse::Accepted{..}) {return Err("resumed input was not accepted".into());}

		tokio::time::timeout(Duration::from_secs(90),async {
			loop {
				if let AgentHistoryResult::Available{entries,..}=client.history(EntityId::new("agent-service-smoke").expect("valid bounded qualification fixture")).await?
					&& entries.iter().any(|entry|entry.kind=="assistant"&&entry.text.contains("SERVICE_RESTORED")) {break;
				}

				tokio::time::sleep(Duration::from_millis(500)).await;
			}

			Ok::<(),Box<dyn Error>>(())
		}).await??;

		if Some(agent_thread(&root).await?) != original_thread {return Err("resumed response used a different thread".into());}

		println!("Production service restart preserved Agent thread and completed a later user turn.");

		reliability::long_result(&client, &root).await?;

		Ok(())
	}.await,);

	if resumed.is_ok() {
		capture_after_restart(&root).await;
	}

	tokio::time::timeout(Duration::from_secs(10), service.shutdown()).await??;

	resumed
}

async fn agent_thread(root: &DecodexRoot) -> Result<String, Box<dyn Error>> {
	let client = AgentClient::new(ClientProfile::load(root.as_path(), None)?);
	let AgentSnapshotResult::Available(snapshot) = client.query().await? else {
		return Err("snapshot unavailable".into());
	};

	snapshot
		.work_items
		.into_iter()
		.find(|work| work.id == "agent-service-smoke")
		.and_then(|work| work.codex_thread_id)
		.ok_or_else(|| "Agent thread is not bound".into())
}

async fn capture_before_restart() {
	if let Some(seconds) = env::var("DECODEX_SMOKE_CAPTURE_SECONDS")
		.ok()
		.and_then(|value| value.parse::<u64>().ok())
		.filter(|seconds| *seconds <= 60)
	{
		println!("Disposable service available for read-only capture for {seconds} seconds.");

		tokio::time::sleep(Duration::from_secs(seconds)).await;
	}
}

async fn snapshot(client: &AgentClient) -> SmokeResult<AgentSnapshotDto> {
	match client.query().await? {
		AgentSnapshotResult::Available(snapshot) => {
			reliability::record_threads(&snapshot);

			Ok(snapshot)
		},
		_ => Err("Agent snapshot unavailable".into()),
	}
}

async fn history(client: &AgentClient, work: &str) -> SmokeResult<Vec<AgentHistoryEntryDto>> {
	match client.history(EntityId::new(work).expect("valid bounded qualification fixture")).await? {
		AgentHistoryResult::Available { entries, .. } => Ok(entries),
		_ => Err("Agent history unavailable".into()),
	}
}

async fn send(client: &AgentClient, key: &str, text: &str) -> SmokeResult<()> {
	accept(
		client,
		AgentActionDto::Send {
			root_id: EntityId::new("agent-service-smoke")
				.expect("valid bounded qualification fixture"),
			text: HistoryText::new(text).expect("valid bounded qualification fixture"),
		},
		key,
	)
	.await
}

async fn accept(client: &AgentClient, action: AgentActionDto, key: &str) -> SmokeResult<()> {
	match client
		.execute(action, IdempotencyKey::new(key).expect("valid bounded qualification fixture"))
		.await?
	{
		AgentCommandResponse::Accepted { .. } => Ok(()),
		outcome => {
			eprintln!("Public command outcome: {outcome:?}");

			Err("command acceptance not confirmed; do not replay".into())
		},
	}
}

async fn wait_graph(
	client: &AgentClient,
	label: &str,
	predicate: impl Fn(&AgentSnapshotDto) -> bool,
) -> SmokeResult<AgentSnapshotDto> {
	wait_graph_for(client, label, Duration::from_secs(180), predicate).await
}

async fn wait_graph_for(
	client: &AgentClient,
	label: &str,
	deadline: Duration,
	predicate: impl Fn(&AgentSnapshotDto) -> bool,
) -> SmokeResult<AgentSnapshotDto> {
	let result = tokio::time::timeout(deadline, async {
		loop {
			let graph = snapshot(client).await?;

			if predicate(&graph) {
				return Ok::<_, Box<dyn Error>>(graph);
			}

			tokio::time::sleep(Duration::from_millis(500)).await;
		}
	})
	.await;

	match result {
		Ok(value) => value,
		Err(_) => {
			eprintln!("Timed out phase: {label}; public graph: {:?}", snapshot(client).await?);

			Err("closed-loop phase timed out; no automatic retry".into())
		},
	}
}

async fn closed_loop_before_restart(
	client: &AgentClient,
	root: &DecodexRoot,
	scope: SmokeScope,
) -> SmokeResult<()> {
	if scope == SmokeScope::Evidence {
		return evidence::qualify(client).await;
	}
	if scope == SmokeScope::Hierarchy {
		return qualify_hierarchy(client, root).await;
	}

	let initial = wait_graph(client, "two workers complete and wake Agent", |graph| {
		graph.work_items.len() == 3
			&& idle(graph)
			&& graph
				.work_items
				.iter()
				.filter(|work| work.id != "agent-service-smoke")
				.all(|work| work.status == AgentWorkStatusDto::Resolved)
			&& !graph.pending_events.iter().any(|event| event.event_kind == "worker_turn_completed")
	})
	.await?;
	let threads: HashSet<_> =
		initial.work_items.iter().filter_map(|work| work.codex_thread_id.as_ref()).collect();

	if threads.len() != 3 {
		return Err("workers did not use independent threads".into());
	}

	for (work, marker) in [("service-a", "DRAFT_A"), ("service-b", "RESULT_B")] {
		if !history(client, work)
			.await?
			.iter()
			.any(|entry| entry.kind == "assistant" && entry.text.contains(marker))
		{
			eprintln!("Missing marker {marker} in work {work}");

			return Err("worker result missing".into());
		}
	}

	println!(
		"Service graph: two independent workers completed; later Agent turns disposed both results."
	);

	if scope == SmokeScope::LongOutput {
		return reliability::long_result(client, root).await;
	}

	reliability::timer_reconnect(client, root).await?;

	if scope == SmokeScope::Reconnect {
		return reliability::no_stale_host_errors(client).await;
	}

	reliability::carryover(client, root).await?;

	send(client,"repair-original-worker","Use agent_continue_worker exactly once on existing service-a; request exactly REPAIRED_A without tools. Do not create work. Resolve the new worker completion event when it arrives and summarize REPAIR_ACCEPTED. Only Agent coordination tools.").await?;
	verify_worker_repair(client, &initial).await?;
	send(client,"decision-policy","The next automation result for service-a will present a choice of two summary formats: A concise or B detailed. Record user_decision on that exact automation event and ask the user to choose. Do not choose automatically. The next service-b automation result carries nextCheckAtMicros: record wait on that exact event and schedule that exact check; when followup_due arrives, resolve it and summarize FOLLOWUP_DONE. Never dispatch another worker for these events.").await?;
	wait_graph(client, "decision policy accepted", |graph| {
		idle(graph) && !graph.pending_events.iter().any(|event| event.event_kind == "user_message")
	})
	.await?;

	let decision=AgentActionDto::AutomationResult {work_id:EntityId::new("service-a").expect("valid bounded qualification fixture"),source_event_id:WireText::new("service-choice-1").expect("valid bounded qualification fixture"),payload:HistoryText::new(r#"{"observation":"Choose summary format A concise or B detailed; user selection is required."}"#).expect("valid bounded qualification fixture")};

	accept(client, decision.clone(), "automation-choice-first").await?;
	accept(client, decision.clone(), "automation-choice-duplicate").await?;
	wait_graph(client, "user decision surfaced", |graph| {
		idle(graph)
			&& graph.work_items.iter().any(|work| {
				work.id == "service-a" && work.status == AgentWorkStatusDto::UserDecision
			})
	})
	.await?;

	let events = history(client, "service-a").await?;

	if choice_receipts(&events) != 1 {
		return Err("automation source was not deduplicated".into());
	}

	accept(client, decision, "automation-choice-after-disposition").await?;

	if choice_receipts(&history(client, "service-a").await?) != 1 {
		return Err("disposed automation duplicate changed receipt count".into());
	}

	let due = SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros() + 150_000_000;

	accept(client,AgentActionDto::AutomationResult {work_id:EntityId::new("service-b").expect("valid bounded qualification fixture"),source_event_id:WireText::new("service-followup-1").expect("valid bounded qualification fixture"),payload:HistoryText::new(serde_json::json!({"observation":"Check readiness after restart without worker dispatch","nextCheckAtMicros":due}).to_string()).expect("valid bounded qualification fixture")},"automation-followup").await?;
	wait_graph(client, "durable pending followup", |graph| {
		idle(graph)
			&& graph
				.work_items
				.iter()
				.any(|work| work.id == "service-b" && work.next_check_at_micros.is_some())
	})
	.await?;

	println!(
		"Service automation: stable-source deduplication, user decision and scheduled obligation persisted before restart."
	);

	Ok(())
}

async fn closed_loop_after_restart(
	client: &AgentClient,
	original: &AgentSnapshotDto,
) -> SmokeResult<()> {
	send(client,"answer-decision","I choose option A: concise summary. Call agent_resolve_decision with id service-a, userEventId the exact currently delivered user_message event ID for this reply, and a summary of the user's chosen concise format. Do not dispatch workers. Report DECISION_RESOLVED. Continue honoring service-b scheduled check, resolving its due event without worker dispatch.").await?;

	let graph = wait_graph(client, "decision and due followup after restart", |graph| {
		idle(graph)
			&& graph.work_items.iter().filter(|work| work.id != "agent-service-smoke").all(|work| {
				work.status == AgentWorkStatusDto::Resolved && work.next_check_at_micros.is_none()
			})
	})
	.await?;

	if graph.work_items.len() != original.work_items.len() {
		return Err("restart created extra work".into());
	}

	for work in &original.work_items {
		if graph
			.work_items
			.iter()
			.find(|next| next.id == work.id)
			.and_then(|next| next.codex_thread_id.as_ref())
			!= work.codex_thread_id.as_ref()
		{
			return Err("restart changed thread identity".into());
		}
	}
	for (work, count) in [("service-a", 2), ("service-b", 1)] {
		if history(client, work).await?.iter().filter(|entry| entry.kind == "assistant").count()
			!= count
		{
			return Err("restart duplicated worker dispatch".into());
		}
	}

	if !history(client, "service-b").await?.iter().any(|entry| entry.text.contains("followup_due"))
	{
		return Err("due check evidence absent".into());
	}
	if !history(client, "service-a")
		.await?
		.iter()
		.any(|entry| entry.text.contains("user_decision_resolved"))
	{
		return Err("explicit decision-resolution evidence absent".into());
	}

	println!(
		"Service restart: pending obligation completed, explicit user choice resolved, all original threads preserved, no duplicate worker dispatch."
	);

	Ok(())
}

async fn enroll_service_account(root: &DecodexRoot) -> SmokeResult<(AgentClient, EntityId)> {
	let profile = ClientProfile::load(root.as_path(), None)?;
	let account = id();
	let enrolled = AccountClient::new(profile.clone())
		.execute(
			CommandPayload::EnrollAccountFromSharedCodex {
				operation_id: id(),
				account_id: account.clone(),
				enabled: true,
			},
			None,
			IdempotencyKey::new("smoke-enroll").expect("valid bounded qualification fixture"),
		)
		.await?;

	if !matches!(enrolled, AccountCommandResponse::Applied { .. }) {
		return Err("disposable account enrollment was not applied".into());
	}

	let accounts = AccountClient::new(profile.clone());
	let _ = accounts.request_observation_refresh(0).await?;
	let observed = tokio::time::timeout(Duration::from_secs(60), async {
		loop {
			if let AccountInspectResult::Available(row) = accounts.inspect(account.clone()).await?
				&& row.five_hour_quota.observed_at_unix_micros.is_some()
				&& row.seven_day_quota.observed_at_unix_micros.is_some()
				&& matches!(
					row.five_hour_quota.result,
					AccountQuotaStateDto::Current { .. }
						| decodex_protocol::AccountQuotaStateDto::NotApplicable
				)
				&& matches!(row.seven_day_quota.result, AccountQuotaStateDto::Current { .. })
			{
				break;
			}

			tokio::time::sleep(Duration::from_secs(1)).await;
		}

		Ok::<(), Box<dyn Error>>(())
	})
	.await;

	if let AccountInspectResult::Available(row) = accounts.inspect(account.clone()).await? {
		println!(
			"Account readiness: {:?}; quota windows: {:?}, {:?}",
			row.lifecycle_readiness, row.five_hour_quota, row.seven_day_quota
		);
	}

	if observed.is_err() {
		// These queries read daemon-owned cached observations. Print only closed
		// error classes and quota facts, never profile identity or provider bytes.
		match accounts.profile(account.clone(), false).await? {
			AccountProfileResult::Current(_) => {
				println!("Cached profile observation: current")
			},
			AccountProfileResult::Cached { refresh_error, .. } => {
				println!("Cached profile refresh error: {refresh_error:?}")
			},
			AccountProfileResult::Unavailable { error, .. } => {
				println!("Cached profile unavailable: {error:?}")
			},
		}
		match ResetCardClient::new(profile.clone()).list(account.clone()).await? {
			ResetCardInventoryResult::Available { five_hour_quota, seven_day_quota, .. } =>
				println!(
					"Cached quota observation available: {five_hour_quota:?}, {seven_day_quota:?}"
				),
			ResetCardInventoryResult::ObservationFailed {
				error,
				five_hour_quota,
				seven_day_quota,
				..
			} => println!(
				"Cached quota observation failed: {error:?}; {five_hour_quota:?}, {seven_day_quota:?}"
			),
			ResetCardInventoryResult::Unavailable { error } => {
				println!("Cached quota observation unavailable: {error:?}")
			},
		}
	}

	observed.map_err(|_| "account quota observations remain unavailable after 60 seconds")??;

	Ok((AgentClient::new(profile), account))
}

async fn capture_after_restart(root: &DecodexRoot) {
	if let Some(seconds) = env::var("DECODEX_SMOKE_FINAL_CAPTURE_SECONDS")
		.ok()
		.and_then(|value| value.parse::<u64>().ok())
		.filter(|seconds| *seconds <= 60)
	{
		println!(
			"ALL SCRIPTED ASSERTIONS PASSED. Final service root {} available for explicit UI qualification for {seconds} seconds.",
			root.as_path().display()
		);

		tokio::time::sleep(Duration::from_secs(seconds)).await;
	}
}

async fn qualify_hierarchy(client: &AgentClient, root: &DecodexRoot) -> SmokeResult<()> {
	let graph = wait_graph_for(
		client,
		"nested manager result propagation",
		Duration::from_secs(480),
		|graph| {
			graph.work_items.len() == 4
				&& idle(graph)
				&& graph
					.work_items
					.iter()
					.filter(|work| work.parent_goal_id.is_some())
					.all(|work| work.status == AgentWorkStatusDto::Resolved)
		},
	)
	.await?;

	for (id, parent) in [("project", "agent-service-smoke"), ("team", "project"), ("leaf", "team")]
	{
		if !graph
			.work_items
			.iter()
			.any(|work| work.id == id && work.parent_goal_id.as_deref() == Some(parent))
		{
			return Err("manager lineage differs".into());
		}
	}

	if graph.workspaces.len() != 1 || graph.workspaces[0].agent_id != "project" {
		return Err("workspace projection missing".into());
	}

	println!("Nested workspace -> manager -> worker results returned through their owners.");

	send(client,"stream-probe","Without tools, write 40 numbered lines. Each line must contain the sentence 'Live conversation output is visible while this reply is still being written.' End with STREAM_DONE.").await?;

	let mut saw_partial = false;

	tokio::time::timeout(Duration::from_secs(180), async {
		loop {
			if let AgentHistoryResult::Available { entries, live, .. } = client
				.history(EntityId::new("agent-service-smoke").expect("bounded fixture identity"))
				.await?
			{
				saw_partial |= live.iter().any(|message| !message.text.is_empty());

				if entries
					.iter()
					.any(|entry| entry.kind == "assistant" && entry.text.contains("STREAM_DONE"))
				{
					break;
				}
			}

			tokio::time::sleep(Duration::from_millis(100)).await;
		}

		Ok::<(), Box<dyn Error>>(())
	})
	.await??;

	if !saw_partial {
		return Err("no pre-completion streamed output observed".into());
	}

	println!("Partial assistant output observed before saved terminal reply.");

	wait_graph(client, "stream turn idle", idle).await?;

	reliability::timer_reconnect(client, root).await?;

	reliability::no_stale_host_errors(client).await
}

async fn verify_capabilities(client: &AgentClient) -> SmokeResult<()> {
	match client.capabilities().await? {
		AgentCapabilitiesResult::Available { models, memory_enabled } => {
			println!(
				"NATIVE_CAPABILITIES models={} memory_configured={memory_enabled:?}",
				models.len()
			);

			if models.is_empty() {
				return Err("native model catalog is empty".into());
			}
		},
		_ => return Err("native capabilities are unavailable".into()),
	}

	Ok(())
}

async fn verify_initial_service(
	client: &AgentClient,
	outcome: AgentCommandResponse,
	root: &DecodexRoot,
	scope: SmokeScope,
) -> SmokeResult<()> {
	if !matches!(outcome, AgentCommandResponse::Accepted { .. }) {
		eprintln!("Agent public start outcome: {outcome:?}");

		return Err::<(), Box<dyn Error>>("production Agent start was not accepted".into());
	}

	tokio::time::timeout(Duration::from_secs(90), async {
		loop {
			if let AgentHistoryResult::Available { entries, .. } = client
				.history(
					EntityId::new("agent-service-smoke")
						.expect("valid bounded qualification fixture"),
				)
				.await?
			{
				if entries
					.iter()
					.any(|entry| entry.kind == "assistant" && entry.text.contains("SERVICE_READY"))
				{
					break;
				}

				if let Some(entry) = entries.iter().find(|entry| {
					entry.kind == "system" && entry.text.contains("reconnection_needs_attention")
				}) {
					eprintln!("Agent public attention: {}", entry.text);

					return Err("production Agent connection needs attention".into());
				}
			}

			tokio::time::sleep(Duration::from_millis(500)).await;
		}

		Ok::<(), Box<dyn Error>>(())
	})
	.await??;

	if !matches!(client.query().await?, AgentSnapshotResult::Available(_)) {
		return Err("snapshot unavailable".into());
	}

	println!(
		"Production Agent start, model response, history and snapshot passed through same-UID protocol."
	);

	verify_capabilities(client).await?;
	closed_loop_before_restart(client, root, scope).await?;

	Ok(())
}

async fn verify_worker_repair(client: &AgentClient, initial: &AgentSnapshotDto) -> SmokeResult<()> {
	let repaired = wait_graph(client, "repair original worker", |graph| {
		idle(graph)
			&& graph
				.work_items
				.iter()
				.find(|work| work.id == "service-a")
				.is_some_and(|work| work.status == AgentWorkStatusDto::Resolved)
			&& !graph.pending_events.iter().any(|event| {
				event.event_kind == "user_message" || event.event_kind == "worker_turn_completed"
			})
	})
	.await?;

	for work in &initial.work_items {
		if repaired
			.work_items
			.iter()
			.find(|next| next.id == work.id)
			.and_then(|next| next.codex_thread_id.as_ref())
			!= work.codex_thread_id.as_ref()
		{
			return Err("repair changed a thread identity".into());
		}
	}

	let results = history(client, "service-a").await?;
	let assistants: Vec<_> = results.iter().filter(|entry| entry.kind == "assistant").collect();

	if assistants.len() != 2 || !assistants.iter().any(|entry| entry.text.contains("REPAIRED_A")) {
		return Err("repair did not complete exactly once".into());
	}

	println!(
		"Service repair: original service-a thread produced exactly two assistant completions."
	);

	Ok(())
}

#[cfg(test)]
mod tests {
	#[test]
	fn automation_receipt_uses_public_kind_and_source_payload() {
		let entry = |id, kind: &str, text: &str| decodex_protocol::AgentHistoryEntryDto {
			native_source: None,
			turn_id: None,
			weather: Vec::new(),
			receipt: None,
			activity: None,
			usage: None,
			duration_ms: None,
			id,
			kind: kind.into(),
			text: text.into(),
			created_at_micros: 1,
		};
		let entries = vec![
			entry(1, "assistant", "automation_result"),
			entry(
				2,
				"automation",
				r#"{"observation":"Choose summary format A concise or B detailed; user selection is required."}"#,
			),
			entry(3, "automation", "unrelated observation"),
		];

		assert_eq!(super::choice_receipts(&entries), 1);
	}
}
