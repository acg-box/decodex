//! A native call can mutate Work only during its matching Agent turn and process generation.
use super::{Call, NativeCall, ToolResponse};
use crate::{
	agent::{AgentConfig, AgentCoordinator, AppServerClient},
	agent_model_settings::tests::OwnedReviewer,
};
use decodex_core::{
	DecodexRoot, ProcessBootIdentity, ProcessDeathEvidence, ProcessDeathEvidenceId,
	ProcessDeathEvidenceKind, work::ThreadRegistration,
};
use decodex_database::ProcessGenerationMutationOutcome;
use rusqlite::Connection;
use serde_json::{Value, json};
use tokio::{io, sync::oneshot};

const THREAD: &str = "registered-thread";
const CURRENT_TURN: &str = "current-turn";

fn queued_call(
	agent: &mut AgentCoordinator,
	decision: i64,
	id: &str,
	observed_turn: &str,
	presented_thread: &str,
) -> (Call, oneshot::Receiver<ToolResponse>) {
	let arguments = json!({"commandId":id,"command":{"operation":"create","work":{
		"id":id,"owner_agent_id":THREAD,"title":id,"state":"open","user_hold":false,
		"requirements":{"id":format!("{id}-r1"),"goal":"Review the fixture","criteria":"Record one finding","source":format!("event:{decision}")}
	}}});
	agent.managed_tools.observe("item/started",&json!({"threadId":THREAD,"turnId":observed_turn,
        "item":{"id":id,"type":"mcpToolCall","server":"decodex","tool":"work_manage","arguments":arguments}}));
	let (admit, admitted) = oneshot::channel();
	(
		Call {
			key: (presented_thread.into(), id.into()),
			native: NativeCall {
				turn: observed_turn.into(),
				tool: "work_manage".into(),
				arguments,
			},
			admit,
		},
		admitted,
	)
}

async fn call(
	agent: &mut AgentCoordinator,
	decision: i64,
	id: &str,
	observed_turn: &str,
	presented_thread: &str,
) -> Result<Value, String> {
	let (call, admitted) = queued_call(agent, decision, id, observed_turn, presented_thread);
	agent.finish_managed_tool(call).await;
	admitted.await.unwrap().await.unwrap()
}

#[tokio::test]
async fn work_mutations_require_the_observed_thread_current_turn_and_ready_generation() {
	let home = tempfile::tempdir().unwrap();
	let (transport, _server) = io::duplex(1024);
	let (reader, writer) = io::split(transport);
	let (client, _events) = AppServerClient::from_io(reader, writer);
	let host = OwnedReviewer::new(home.path(), &client, "host-thread", "host-turn").await;
	let store = &host.store;
	let generation = &host.key.generation;
	store
		.register_native_thread(
			THREAD.into(),
			"Registered".into(),
			ThreadRegistration::Promote { expected_owner: None },
		)
		.await
		.unwrap();
	assert!(
		store
			.attach_independent_agent_process(
				THREAD.into(),
				THREAD.into(),
				generation.as_str().into()
			)
			.await
			.unwrap()
	);
	let config =
		AgentConfig::new("fixture".into(), "medium".into(), home.path().display().to_string());
	let mut agent = AgentCoordinator::new(store.clone(), client, config).unwrap();
	agent.bind_native_generation(generation.clone());
	store.begin_agent_dispatch(THREAD.into()).await.unwrap();
	store.acknowledge_agent_dispatch(THREAD.into(), "previous-turn".into()).await.unwrap();
	store.complete_agent_turn(THREAD.into(), "previous-turn".into()).await.unwrap();
	agent
		.enqueue_user_message(THREAD, "decision", "Create a Work to review the fixture")
		.await
		.unwrap();
	let decision = store
		.list_undelivered_agent_events(10)
		.await
		.unwrap()
		.into_iter()
		.find(|e| e.work_item_id == THREAD)
		.unwrap()
		.id;
	store.begin_agent_dispatch_with_events(THREAD.into(), vec![decision]).await.unwrap();
	store.acknowledge_agent_dispatch(THREAD.into(), CURRENT_TURN.into()).await.unwrap();
	let (cancelled, admission) =
		queued_call(&mut agent, decision, "cancelled", CURRENT_TURN, THREAD);
	drop(admission);
	agent.finish_managed_tool(cancelled).await;
	assert!(store.work_portfolio(None).await.unwrap().works.is_empty());
	let foreign =
		call(&mut agent, decision, "foreign", CURRENT_TURN, "host-thread").await.unwrap_err();
	assert!(foreign.contains("Native call has ended"), "{foreign}");
	let stale = call(&mut agent, decision, "stale", "previous-turn", THREAD).await.unwrap_err();
	assert!(stale.contains("current Agent turn"), "{stale}");
	assert!(store.work_portfolio(None).await.unwrap().works.is_empty());

	let accepted = call(&mut agent, decision, "authorized", CURRENT_TURN, THREAD).await.unwrap();
	assert_eq!(accepted["recorded"], true);
	let evidence = ProcessDeathEvidence::new(
		ProcessDeathEvidenceId::new("60000000-0000-4000-8000-000000000001").unwrap(),
		generation.clone(),
		ProcessDeathEvidenceKind::PriorBootEnded,
		ProcessBootIdentity::new("subsequent-boot").unwrap(),
		None,
		"a".repeat(64),
	)
	.unwrap();
	let retired = store.record_process_generation_death(3, &evidence).await.unwrap();
	assert!(matches!(retired, ProcessGenerationMutationOutcome::Applied(_)), "{retired:?}");
	let retired = call(&mut agent, decision, "retired", CURRENT_TURN, THREAD).await.unwrap_err();
	assert!(retired.contains("process has changed"), "{retired}");
	let works = store.work_portfolio(None).await.unwrap().works;
	assert_eq!(works.len(), 1);
	assert_eq!(works[0].id, "authorized");
	let root = DecodexRoot::new(home.path().canonicalize().unwrap().join("state")).unwrap();
	let db = Connection::open(root.paths().product_database_file()).unwrap();
	let receipts: (i64, String) = db
		.query_row("SELECT count(*), min(id) FROM work_commands", [], |row| {
			Ok((row.get(0)?, row.get(1)?))
		})
		.unwrap();
	assert_eq!(receipts, (1, "authorized".into()));
}
