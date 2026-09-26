//! Exercise the production ordinary coordinator against an isolated native backend.
use crate::{
	application::{Application, ProductStore, ServiceApplication},
	conversation::{
		ConversationCapability, ConversationExecutionSettings, ConversationOutcome,
		ConversationRuntime, CreateConversation,
	},
};
use decodex_core::{ConversationId, ServiceTier};
use decodex_database::SqliteStore;
use decodex_protocol::{
	CURRENT_VERSION, ClientCommandId, CommandEnvelope, CommandPayload, ConversationModel,
	ConversationReasoningEffort, ConversationWorkingDirectory, CorrelationId, DoctorCheck,
	DoctorComponent, DoctorIssue, DoctorReport, DoctorStatus, EntityId, EntityRevision,
	HistoryText, IdempotencyKey, ServerId,
};

pub(super) async fn qualify(
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	home: &std::path::Path,
) {
	let conversation_id =
		ConversationId::new("61000000-0000-4000-8000-000000000001").expect("conversation");
	let instructions = home.join(".codex/AGENTS.md");
	std::fs::write(&instructions, "Keep the fixture local.").expect("native warning fixture");
	let created = runtime
		.create(CreateConversation {
			operation_key: "native-runtime-create".into(),
			correlation_id: "native-runtime-create".into(),
			causation_id: None,
			conversation_id: conversation_id.clone(),
			message: "Create runtime fixture".into(),
			working_directory: home.to_str().expect("home").into(),
			execution: ConversationExecutionSettings {
				model: "cold-native-model".into(),
				reasoning_effort: Some("provider-effort".into()),
				fast: false,
				service_tier: ServiceTier::standard(),
			},
		})
		.await;
	assert!(
		matches!(created, ConversationOutcome::Started { .. }),
		"initial production submission: {created:?}"
	);
	complete(runtime, store, home).await;
	std::fs::remove_file(&instructions).expect("native warning fixture");
	std::os::unix::fs::symlink("AGENTS.md", &instructions).expect("native warning fixture");
	let notices = submit_inherited(
		runtime,
		store,
		home,
		"native-runtime-inherit",
		"62000000-0000-4000-8000-000000000001",
	)
	.await;
	assert!(notices > 0, "warning must publish a history refresh");
	assert_warning_history(runtime, store, home).await;
	std::fs::remove_file(&instructions).expect("native warning fixture");
	std::fs::write(&instructions, "Keep the fixture local.").expect("native warning fixture");
}

pub(super) async fn submit_inherited(
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	home: &std::path::Path,
	key: &str,
	turn: &str,
) -> usize {
	let id = ConversationId::new("61000000-0000-4000-8000-000000000001").expect("conversation");
	let source = store
		.read_ordinary_runtime_session_for_resume(&id)
		.await
		.expect("source")
		.expect("saved session");
	let app = application(runtime, store, home);
	let command = CommandEnvelope {
		version: CURRENT_VERSION,
		client_command_id: ClientCommandId::new(key).expect("command"),
		idempotency_key: IdempotencyKey::new(key).expect("key"),
		correlation_id: CorrelationId::new(key).expect("correlation"),
		causation_id: None,
		expected_revision: Some(EntityRevision(
			source.conversation_revision.try_into().expect("revision"),
		)),
		payload: CommandPayload::SubmitConversationTurn {
			conversation_id: EntityId::new(id.as_str()).expect("id"),
			turn_id: EntityId::new(turn).expect("turn"),
			message: HistoryText::new("Keep native settings").expect("message"),
			working_directory: ConversationWorkingDirectory::new(home.to_str().expect("home"))
				.expect("directory"),
			execution: decodex_protocol::ConversationExecutionSettings {
				model: ConversationModel::new("deliberately-stale-model").expect("model"),
				reasoning_effort: Some(
					ConversationReasoningEffort::new("deliberately-stale-effort").expect("effort"),
				),
				fast: true,
				service_tier: Some(ServiceTier::from_fast(true)),
			},
			overrides: Some(Default::default()),
		},
	};
	let restored: CommandEnvelope =
		serde_json::from_slice(&serde_json::to_vec(&command).expect("saved command"))
			.expect("restored command");
	assert_eq!(restored, command);
	app.execute(&restored).await.expect("public service inherited submission");
	complete(runtime, store, home).await
}

async fn complete(
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	home: &std::path::Path,
) -> usize {
	let app = application(runtime, store, home);
	let mut notices = 0;
	loop {
		match app.next_publication().await.expect("public runtime output").event {
			decodex_protocol::EventPayload::ConversationTurnFinished { outcome, .. } => {
				assert_eq!(outcome, decodex_protocol::ConversationTurnOutcome::Succeeded);
				return notices;
			},
			decodex_protocol::EventPayload::ConversationHistoryChanged { conversation_id } => {
				assert_eq!(conversation_id.as_str(), "61000000-0000-4000-8000-000000000001");
				notices += 1;
			},
			decodex_protocol::EventPayload::ConversationMessageDelta { delta, .. } => {
				assert!(!delta.as_str().contains("Codex warning:"));
			},
			other => panic!("unexpected public runtime event: {other:?}"),
		}
	}
}

pub(super) async fn assert_warning_history(
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	home: &std::path::Path,
) {
	let query = decodex_protocol::QueryEnvelope {
		version: CURRENT_VERSION,
		query_id: decodex_protocol::QueryId::new("native-warning-history")
			.expect("native warning fixture"),
		payload: decodex_protocol::QueryPayload::GetConversationHistory {
			conversation_id: EntityId::new("61000000-0000-4000-8000-000000000001")
				.expect("native warning fixture"),
			after: None,
			page_size: decodex_protocol::MAX_HISTORY_PAGE_SIZE,
		},
	};
	let result = application(runtime, store, home).query(&query).await;
	let decodex_protocol::QueryResultPayload::ConversationHistory(
		decodex_protocol::ConversationHistoryResult::Page(page),
	) = result
	else {
		panic!("history unavailable: {result:?}");
	};
	let notices: Vec<_> = page
		.items
		.iter()
		.filter(|item| {
			item.payload.inline_text().is_some_and(|text| {
				text.as_str().contains("AGENTS.md") && text.as_str().starts_with("Codex warning:")
			})
		})
		.collect();
	assert_eq!(notices.len(), 1, "native warning persists once");
	assert_eq!(notices[0].kind, decodex_protocol::HistoryItemKindDto::Status);
	assert_eq!(notices[0].status, decodex_protocol::HistoryItemStatusDto::Completed);
}

pub(super) fn application(
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	home: &std::path::Path,
) -> ServiceApplication {
	let doctor = DoctorReport::new(
		ServerId::new("native-fixture").expect("server"),
		CURRENT_VERSION,
		DoctorComponent::ALL
			.into_iter()
			.map(|component| {
				DoctorCheck::new(component, DoctorStatus::Unavailable(DoctorIssue::NotProbed))
			})
			.collect(),
	)
	.expect("doctor");
	let root = decodex_core::DecodexRoot::new(home.join("product")).expect("fixture root");
	ServiceApplication::new(
		ProductStore::Available(store.clone()),
		None,
		None,
		decodex_codex::CodexAdapter::unavailable(),
		Some(decodex_core::BlobStore::open(root.paths()).expect("fixture blobs")),
		ConversationCapability::Ready(runtime.clone()),
		doctor,
	)
}

pub(super) async fn archive(
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	home: &std::path::Path,
) {
	let id = ConversationId::new("61000000-0000-4000-8000-000000000001").expect("conversation");
	let source =
		store.read_ordinary_runtime_session_for_resume(&id).await.expect("read").expect("session");
	let app = application(runtime, store, home);
	let command = CommandEnvelope {
		version: CURRENT_VERSION,
		client_command_id: ClientCommandId::new("archive-native").expect("command"),
		idempotency_key: IdempotencyKey::new("archive-native").expect("key"),
		correlation_id: CorrelationId::new("archive-native").expect("correlation"),
		causation_id: None,
		expected_revision: Some(EntityRevision(
			source.conversation_revision.try_into().expect("revision"),
		)),
		payload: CommandPayload::ArchiveConversation {
			conversation_id: EntityId::new(id.as_str()).expect("id"),
		},
	};
	app.execute(&command).await.expect("native archive and local projection");
	let query = decodex_protocol::QueryEnvelope {
		version: CURRENT_VERSION,
		query_id: decodex_protocol::QueryId::new("archived-state").expect("query"),
		payload: decodex_protocol::QueryPayload::GetConversation {
			conversation_id: EntityId::new(id.as_str()).expect("id"),
		},
	};
	assert_eq!(
		app.query(&query).await,
		decodex_protocol::QueryResultPayload::Conversation(
			decodex_protocol::ConversationResult::Archived {
				conversation_id: EntityId::new(id.as_str()).expect("id"),
				conversation_revision: EntityRevision(
					(source.conversation_revision + 1).try_into().expect("revision")
				),
			}
		)
	);
	let missing = decodex_protocol::QueryEnvelope {
		payload: decodex_protocol::QueryPayload::GetConversation {
			conversation_id: EntityId::new("61000000-0000-4000-8000-000000000099")
				.expect("missing"),
		},
		..query
	};
	assert_eq!(
		app.query(&missing).await,
		decodex_protocol::QueryResultPayload::Conversation(
			decodex_protocol::ConversationResult::NotFound
		)
	);
}
