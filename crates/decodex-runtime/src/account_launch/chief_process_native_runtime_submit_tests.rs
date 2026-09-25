//! Exercise the production ordinary coordinator against an isolated native backend.
use crate::{
	application::{Application, ProductStore, ServiceApplication},
	conversation::{
		ConversationCapability, ConversationExecutionSettings, ConversationOutcome,
		ConversationRuntime, ConversationTerminalState, CreateConversation,
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
	complete(runtime).await;
	submit_inherited(
		runtime,
		store,
		home,
		"native-runtime-inherit",
		"62000000-0000-4000-8000-000000000001",
	)
	.await;
}

pub(super) async fn submit_inherited(
	runtime: &ConversationRuntime,
	store: &SqliteStore,
	home: &std::path::Path,
	key: &str,
	turn: &str,
) {
	let id = ConversationId::new("61000000-0000-4000-8000-000000000001").expect("conversation");
	let source = store
		.read_ordinary_runtime_session_for_resume(&id)
		.await
		.expect("source")
		.expect("saved session");
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
	let app = ServiceApplication::new(
		ProductStore::Available(store.clone()),
		None,
		None,
		decodex_codex::CodexAdapter::unavailable(),
		None,
		ConversationCapability::Ready(runtime.clone()),
		doctor,
	);
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
	complete(runtime).await;
}

async fn complete(runtime: &ConversationRuntime) {
	loop {
		match runtime.next_event().await.expect("runtime output") {
			ConversationOutcome::Terminal { state, .. } => {
				assert_eq!(state, ConversationTerminalState::Succeeded);
				return;
			},
			ConversationOutcome::Streaming { .. } | ConversationOutcome::Started { .. } => {},
			other => panic!("unexpected production runtime event: {other:?}"),
		}
	}
}
