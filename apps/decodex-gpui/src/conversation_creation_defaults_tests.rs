//! Initial discovery changes selection, never sends or replays a message.
use super::*;
use decodex_protocol::{InitialExecutionDefaults, InitialModelCatalogResult, InitialModelDefaults};

pub(super) fn install_defaults(
	conversations: &Conversations,
	server: &ServerId,
	defaults: InitialModelDefaults,
) {
	assert!(conversations.refresh_catalog());
	reply_defaults(conversations, server, defaults);
}

pub(crate) fn reply_defaults(
	conversations: &Conversations,
	server: &ServerId,
	defaults: InitialModelDefaults,
) {
	let query = conversations.try_take_dispatch(1, server).unwrap().query().unwrap().clone();
	let result = response(server, query.query_id, Some(defaults));
	assert_eq!(
		conversations.route_query_result(1, server, &result),
		ConversationRouteOutcome::Fresh
	);
}

fn response(
	server: &ServerId,
	query_id: QueryId,
	defaults: Option<InitialModelDefaults>,
) -> QueryResultEnvelope {
	QueryResultEnvelope {
		version: CURRENT_VERSION,
		server_id: server.clone(),
		query_id,
		payload: QueryResultPayload::InitialModelCatalog(InitialModelCatalogResult::Available {
			account_id: EntityId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			account_revision: 1,
			working_directory: ConversationWorkingDirectory::new("/tmp").unwrap(),
			models: vec![],
			defaults: defaults.map(Box::new),
		}),
	}
}

fn defaults() -> InitialModelDefaults {
	InitialModelDefaults {
		configured: InitialExecutionDefaults {
			model: Some(ConversationModel::new("configured-model").unwrap()),
			reasoning_effort: None,
			service_tier: Some(decodex_protocol::ServiceTier::new("flex").unwrap()),
		},
		managed: InitialExecutionDefaults {
			model: Some(ConversationModel::new("managed-model").unwrap()),
			reasoning_effort: Some(ConversationReasoningEffort::Low),
			service_tier: None,
		},
		catalog_model: None,
	}
}

#[test]
fn automatic_discovery_is_once_per_context_and_missing_defaults_cannot_send() {
	let (conversations, server, _) = tests::catalog_conversations();
	conversations.begin_new();
	assert_eq!(conversations.create("Keep original"), Err(ConversationInputError::NotReady));
	conversations.ensure_initial_catalog();
	let query = conversations.try_take_dispatch(1, &server).unwrap().query().unwrap().clone();
	conversations.ensure_initial_catalog();
	assert!(conversations.try_take_dispatch(1, &server).is_none());
	conversations.route_query_result(1, &server, &response(&server, query.query_id, None));
	conversations.ensure_initial_catalog();
	assert!(conversations.try_take_dispatch(1, &server).is_none());
	assert!(!conversations.snapshot().can_submit);
	install_defaults(&conversations, &server, defaults());
	assert!(conversations.snapshot().can_submit);
	assert!(conversations.lock().pending_command.is_none());
	assert_eq!(conversations.snapshot().execution.model.as_str(), "managed-model");
	conversations.create("Original input").unwrap();
	let command = tests::dispatched_command(&conversations, &server);
	let CommandPayload::CreateConversation { execution, message, initial_model_source, .. } =
		command.payload
	else {
		panic!("create")
	};
	assert_eq!(
		initial_model_source,
		Some(Box::new(decodex_protocol::InitialModelSource {
			account_id: EntityId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			account_revision: 1
		}))
	);
	assert_eq!(message.as_str(), "Original input");
	assert_eq!(execution.model.as_str(), "managed-model");
	assert_eq!(execution.reasoning_effort, Some(ConversationReasoningEffort::Low));
	assert_eq!(execution.effective_service_tier().as_str(), "flex");
}

#[test]
fn explicit_reasoning_opts_out_of_managed_model_and_survives_refresh() {
	let (conversations, server, _) = tests::catalog_conversations();
	conversations.begin_new();
	install_defaults(&conversations, &server, defaults());
	conversations.cycle_reasoning_effort();
	let chosen = conversations.snapshot().execution.reasoning_effort;
	assert_eq!(conversations.snapshot().execution.model.as_str(), "configured-model");
	conversations.select_service_tier(decodex_protocol::ServiceTier::standard());
	install_defaults(&conversations, &server, defaults());
	let selection = conversations.snapshot().execution;
	assert_eq!(selection.reasoning_effort, chosen);
	assert_eq!(selection.model.as_str(), "configured-model");
	assert_eq!(selection.effective_service_tier().as_str(), "default");
}

#[test]
fn account_event_invalidates_default_source_and_stale_reply_cannot_restore_it() {
	let (conversations, server, _) = tests::catalog_conversations();
	conversations.begin_new();
	install_defaults(&conversations, &server, defaults());
	assert!(conversations.refresh_catalog());
	let query = conversations.try_take_dispatch(1, &server).unwrap().query().unwrap().clone();
	conversations.apply_event(&EventEnvelope {
		version: CURRENT_VERSION,
		server_id: server.clone(),
		cursor: decodex_protocol::Cursor(1),
		channel: decodex_protocol::Channel::AccountsHealth,
		entity_id: EntityId::new("10000000-0000-4000-8000-000000000001").unwrap(),
		entity_revision: EntityRevision(2),
		correlation_id: CorrelationId::new("change").unwrap(),
		causation_id: None,
		payload: EventPayload::AccountLoggedOut {
			account_id: EntityId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			tombstone_revision: EntityRevision(2),
		},
	});
	conversations.route_query_result(
		1,
		&server,
		&response(&server, query.query_id, Some(defaults())),
	);
	assert!(!conversations.snapshot().can_submit);
	assert!(conversations.snapshot().catalog.is_none());
	assert_eq!(conversations.create("Do not send"), Err(ConversationInputError::NotReady));
	conversations.ensure_initial_catalog();
	assert!(conversations.try_take_dispatch(1, &server).unwrap().query().is_some());
}

#[test]
fn fully_explicit_creation_does_not_require_native_defaults() {
	let (conversations, server, _) = tests::catalog_conversations();
	conversations.begin_new();
	conversations.cycle_model();
	conversations.cycle_reasoning_effort();
	assert!(!conversations.snapshot().can_submit);
	assert!(conversations.select_service_tier(decodex_protocol::ServiceTier::standard()));
	assert!(conversations.snapshot().can_submit);
	let chosen = conversations.snapshot().execution;
	conversations.create("Use my explicit choices").unwrap();
	let command = tests::dispatched_command(&conversations, &server);
	let CommandPayload::CreateConversation { execution, .. } = command.payload else {
		panic!("create")
	};
	assert_eq!(execution, chosen);
}
