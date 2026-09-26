//! Real same-UID transport proof for cold, read-only creation recovery.
use crate::{
	ProtocolServer, ServerConfig,
	application::{ProductStore, ServiceApplication},
	conversation::{ConversationCapability, CreateConversation},
};
use decodex_core::{ConversationId, DecodexRoot, LocalTrustPolicy};
use decodex_database::{CreateConversationRecord, SqliteStore};
use decodex_protocol::{
	CURRENT_VERSION, ClientCommandId, ClientDraftStore, CommandEnvelope, CommandPayload,
	ConversationCreationReceiptRequest, ConversationCreationReceiptResult as Receipt,
	ConversationExecutionSettings, ConversationModel, ConversationUnavailableReason,
	ConversationWorkingDirectory, CorrelationId, DesktopDraftDocument,
	DesktopOrdinaryComposerDraft, DesktopOrdinaryDraft, DoctorCheck, DoctorComponent, DoctorIssue,
	DoctorReport, DoctorStatus, EntityId, EntityRevision, HistoryText, IdempotencyKey,
	LocalTransportAuthority, QueryEnvelope, QueryId, QueryPayload, QueryResultPayload,
	RetainedSession, ServerId, SessionCancellation, SessionDelivery,
};

fn original() -> CommandEnvelope {
	CommandEnvelope {
		version: CURRENT_VERSION,
		client_command_id: ClientCommandId::new("original-attempt").unwrap(),
		idempotency_key: IdempotencyKey::new("original-creation").unwrap(),
		expected_revision: None,
		correlation_id: CorrelationId::new("original-correlation").unwrap(),
		causation_id: None,
		payload: CommandPayload::CreateConversation {
			initial_model_source: None,
			conversation_id: EntityId::new("30000000-0000-4000-8000-000000000001").unwrap(),
			message: HistoryText::new("Original input").unwrap(),
			working_directory: ConversationWorkingDirectory::new("/tmp").unwrap(),
			execution: ConversationExecutionSettings {
				model: ConversationModel::new("native-model").unwrap(),
				reasoning_effort: None,
				fast: false,
				service_tier: None,
			},
		},
	}
}

async fn persist_original(root: &DecodexRoot, original: &CommandEnvelope, scope: &str) {
	let request = ConversationCreationReceiptRequest::from_command(original).unwrap();
	let command = CreateConversation {
		initial_model_source: crate::application::runtime_initial_model_source(
			request.initial_model_source.as_deref(),
		)
		.unwrap(),
		operation_key: request.idempotency_key.as_str().into(),
		correlation_id: "original-correlation".into(),
		causation_id: None,
		conversation_id: ConversationId::new(request.conversation_id.as_str()).unwrap(),
		message: request.message.as_str().into(),
		working_directory: request.working_directory.as_str().into(),
		execution: super::runtime_execution_settings(&request.execution),
	};
	let store = SqliteStore::open(&root.paths()).unwrap();
	store
		.create_conversation(
			&command.creation_identity().unwrap(),
			&CreateConversationRecord {
				initial_model_source: None,
				conversation_id: command.conversation_id,
				title: "Original input".into(),
				message: command.message,
				working_directory: command.working_directory,
				model: command.execution.model,
				reasoning_effort: None,
				fast: false,
				service_tier: None,
			},
		)
		.await
		.unwrap();
	let mut document = DesktopDraftDocument::default();
	document.profiles.entry(scope.into()).or_default().ordinary.insert(
		"/tmp".into(),
		DesktopOrdinaryDraft {
			working_directory: request.working_directory,
			composer: DesktopOrdinaryComposerDraft {
				conversation_id: None,
				text: "Later unsent input".into(),
				execution: request.execution,
				creation_intent: Default::default(),
			},
			new_conversation: None,
			parked: Default::default(),
			unconfirmed: vec![original.clone()],
		},
	);
	ClientDraftStore::open_at(root.as_path())
		.unwrap()
		.save(0, &document.encode().unwrap())
		.unwrap();
}

async fn query(
	session: &mut RetainedSession,
	request: ConversationCreationReceiptRequest,
	sequence: u64,
) -> Receipt {
	let query_id = QueryId::new(format!("creation-read-{sequence}")).unwrap();
	session
		.send_query(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: query_id.clone(),
			payload: QueryPayload::GetConversationCreationReceipt { request },
		})
		.await
		.unwrap();
	let SessionDelivery::QueryResult(result) = session.next().await.unwrap() else {
		panic!("read-only query must not produce a command receipt or mutation event");
	};
	assert_eq!(result.query_id, query_id);
	let QueryResultPayload::ConversationCreationReceipt(receipt) = result.payload else {
		panic!("typed receipt")
	};
	receipt
}

#[tokio::test]
async fn creation_receipt_survives_store_and_service_restart_without_replay() {
	let temporary = tempfile::tempdir().unwrap();
	let root = DecodexRoot::new(temporary.path().canonicalize().unwrap()).unwrap();
	root.paths().ensure_layout().unwrap();
	let original = original();
	let server_id = ServerId::new("20000000-0000-4000-8000-000000000001").unwrap();
	// SAFETY: geteuid has no arguments or failure return.
	let uid = unsafe { libc::geteuid() };
	use std::os::unix::fs::PermissionsExt as _;
	let config = root.as_path().join("config.toml");
	std::fs::write(&config, format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"20000000-0000-4000-8000-000000000001\"\n")).unwrap();
	std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
	let profile = decodex_protocol::ClientProfile::load(root.as_path(), None).unwrap();
	let scope = profile.draft_scope_key();
	persist_original(&root, &original, &scope).await;
	for _ in 0..2 {
		let store = SqliteStore::open(&root.paths()).unwrap();
		let before = store.read_ordinary_task_conversations(None, None, 65).await.unwrap();
		let drafts = ClientDraftStore::open_at(root.as_path()).unwrap();
		let saved = drafts.load().unwrap();
		let restored = DesktopDraftDocument::decode(&saved.payload).unwrap();
		let profile = decodex_protocol::ClientProfile::load(root.as_path(), None).unwrap();
		let draft = &restored.profiles[&profile.draft_scope_key()].ordinary["/tmp"];
		assert_eq!(draft.composer.text, "Later unsent input");
		assert_eq!(draft.unconfirmed, vec![original.clone()]);
		let request =
			ConversationCreationReceiptRequest::from_command(&draft.unconfirmed[0]).unwrap();
		let doctor = DoctorReport::new(
			server_id.clone(),
			CURRENT_VERSION,
			DoctorComponent::ALL
				.into_iter()
				.map(|component| {
					DoctorCheck::new(component, DoctorStatus::Unavailable(DoctorIssue::NotProbed))
				})
				.collect(),
		)
		.unwrap();
		let app = ServiceApplication::new(
			ProductStore::Available(store.clone()),
			None,
			None,
			decodex_codex::CodexAdapter::unavailable(),
			None,
			ConversationCapability::Unavailable(ConversationUnavailableReason::AppServerProfile),
			doctor,
		);
		let authority =
			LocalTransportAuthority::new(root.paths(), LocalTrustPolicy::SameUid, Some(uid))
				.unwrap();
		let mut server = ProtocolServer::new(server_id.clone(), app, ServerConfig::default())
			.bind(authority.clone())
			.await
			.unwrap();
		let mut client = RetainedSession::connect(
			profile.retained_session_config().unwrap(),
			None,
			SessionCancellation::new(),
		)
		.await
		.unwrap();
		let SessionDelivery::Snapshot { confirmation, .. } = client.next().await.unwrap() else {
			panic!("initial snapshot")
		};
		client.confirm_applied(confirmation).unwrap();
		assert_eq!(
			query(&mut client, request.clone(), 1).await,
			Receipt::Recorded {
				conversation_id: request.conversation_id.clone(),
				creation_revision: EntityRevision(1)
			}
		);
		let mut missing = request.clone();
		missing.idempotency_key = IdempotencyKey::new("not-recorded").unwrap();
		assert_eq!(query(&mut client, missing, 2).await, Receipt::NotRecorded);
		let mut conflicting = request;
		conflicting.message = HistoryText::new("Later unsent input").unwrap();
		assert_eq!(query(&mut client, conflicting, 3).await, Receipt::Conflict);
		client.close().await.unwrap();
		server.shutdown().await.unwrap();
		assert_eq!(store.read_ordinary_task_conversations(None, None, 65).await.unwrap(), before);
		assert_eq!(drafts.load().unwrap().payload, saved.payload);
	}
}
