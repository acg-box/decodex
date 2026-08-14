//! Public-API proof of one exact Quick Task continuation after reopening SQLite.

use getrandom as _;
use rusqlite as _;
use serde as _;
use serde_json as _;
use sha2 as _;

use decodex_core::{
	AccountId, AccountLifecycleReadiness, AccountOperationId, AccountProvider, AccountQuotaWindow,
	AccountQuotaWindowObservation, AccountRecord, AccountRoutingControl, AccountSelectionMode,
	AccountState, BlobStore, ContextPackInput, ContextPackPolicy, ContinuationCommandOutcome,
	ContinuationPlanKind, ConversationId, CredentialBinding, CredentialFingerprint,
	CredentialStoreSchemaVersion, CredentialVersion, DecodexRoot, HistoryItemId, HistoryItemKind,
	HistoryMediaType, HistoryMetadata, ItemStatus, PinnedContextSource, PossibleSideEffects,
	ProcessBootIdentity, ProcessControlKind, ProcessDeathEvidence, ProcessDeathEvidenceId,
	ProcessDeathEvidenceKind, ProcessExecutionAuthorization, ProcessExecutionEpochId,
	ProcessGenerationAccountBinding, ProcessGenerationId, ProcessGenerationIntent,
	ProcessGenerationState, ProcessIdentity, ProcessIsolationKind, ProcessRunnerIdentity,
	ProcessStartIdentity, ProviderAttemptConsumer, ProviderAttemptId, ProviderAttemptPreparation,
	ProviderAttemptState, ProviderDuplicateRisk, ProviderEvidenceId, ProviderEvidenceSource,
	ProviderIdentity, ProviderPositiveEvidence, ProviderRequestId, ProviderRequestKey,
	ProviderRequestKeys, ProviderTerminalOutcome, RoutingCommandOutcome, RuntimeSessionState,
	SameThreadContinuationEvidence, TurnId, TurnRole, compile_context_pack,
};
use decodex_database::{
	AdmitInitialQuickTaskTurn, AuthorizeProviderDispatchOutcome, BindQuickTaskContinuation,
	BindRuntimeSessionThreadOutcome, CodexAccountCapabilityAttestation, CommandIdentity,
	CreateQuickTaskConversation, CredentialKey, CredentialRecord, FenceRuntimeSessionThreadStart,
	FenceRuntimeSessionThreadStartOutcome, InitialQuickTaskTurnAdmissionOutcome,
	LocalAccountTransfer, LocalAccountTransferBatch, LocalAccountTransferOutcome, PlanContinuation,
	PlanInitialThreadContinuation, PrepareProcessGenerationOutcome, PrepareProviderAttemptOutcome,
	PrepareQuickTaskProcessGeneration, PrepareQuickTaskProcessGenerationOutcome,
	ProcessGenerationMutationOutcome, ProviderAttemptMutationOutcome, QuickTaskInitialRouteOutcome,
	QuickTaskPreEffectEvidenceKind, QuickTaskTerminalizationOutcome,
	QuickTaskThreadEstablishmentReadback, ReconcileQuickTaskThreadEstablishment, RecordHistoryItem,
	RouteQuickTaskInitial, RoutingControlOutcome, RuntimeSessionBindingReceipt, SqliteStore,
	SuccessfulRuntimeSessionThreadStart, TerminalizeQuickTaskTurn, TurnReservationOutcome,
};
use tempfile::tempdir;
use zeroize::Zeroizing;

const ACCOUNT_ID: &str = "10000000-0000-4000-8000-000000000001";
const ALTERNATE_ACCOUNT_ID: &str = "10000000-0000-4000-8000-000000000002";
const OPERATION_ID: &str = "20000000-0000-4000-8000-000000000001";
const ALTERNATE_OPERATION_ID: &str = "20000000-0000-4000-8000-000000000002";
const CONVERSATION_ID: &str = "30000000-0000-4000-8000-000000000001";
const ALTERNATE_CONVERSATION_ID: &str = "30000000-0000-4000-8000-000000000002";
const GENERATION_ID: &str = "40000000-0000-4000-8000-000000000001";
const EXECUTION_EPOCH_ID: &str = "50000000-0000-4000-8000-000000000001";
const ATTEMPT_ID: &str = "60000000-0000-4000-8000-000000000001";
const REQUEST_ID: &str = "70000000-0000-4000-8000-000000000001";
const EVIDENCE_ID: &str = "80000000-0000-4000-8000-000000000001";
const INITIAL_PLAN_ID: &str = "90000000-0000-4000-8000-000000000001";
const CONTINUATION_PLAN_ID: &str = "a0000000-0000-4000-8000-000000000001";
const INITIAL_HISTORY_ID: &str = "b0000000-0000-4000-8000-000000000001";
const ASSISTANT_TURN_ID: &str = "c0000000-0000-4000-8000-000000000001";
const ASSISTANT_HISTORY_ID: &str = "d0000000-0000-4000-8000-000000000001";
const LATER_TURN_ID: &str = "e0000000-0000-4000-8000-000000000001";
const LATER_HISTORY_ID: &str = "f0000000-0000-4000-8000-000000000001";
const DEATH_EVIDENCE_ID: &str = "11000000-0000-4000-8000-000000000001";
const REHYDRATED_GENERATION_ID: &str = "12000000-0000-4000-8000-000000000001";
const PROVIDER_ACCOUNT_ID: &str = "fixture-provider-account";
const ALTERNATE_PROVIDER_ACCOUNT_ID: &str = "fixture-provider-account-2";
const CODEX_THREAD_ID: &str = "fixture-codex-thread";
const PROVIDER_TURN_ID: &str = "fixture-provider-turn-1";
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DIGEST_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

#[tokio::test]
#[allow(clippy::too_many_lines)] // Keep one complete persisted execution and restart proof together.
async fn quick_task_continues_on_the_same_thread_after_sqlite_reopen_without_duplicate_dispatch() {
	let temporary = tempdir().expect("temporary Decodex root");
	let canonical_root = temporary.path().canonicalize().expect("canonical temporary root");
	let root = DecodexRoot::new(canonical_root).expect("typed Decodex root");
	let paths = root.paths();
	let blob_store = BlobStore::open(paths.clone()).expect("open blob store");
	let store = SqliteStore::open(&paths).expect("open SQLite product store");
	let (account_id, credential, alternate_account_id) = import_ready_accounts(&store).await;

	let conversation_id = ConversationId::new(CONVERSATION_ID).expect("conversation identity");
	let conversation_command =
		CommandIdentity::new("quick-task-conversation", b"create restart conversation")
			.expect("conversation command");
	let conversation = store
		.create_quick_task_conversation(
			&conversation_command,
			&CreateQuickTaskConversation {
				conversation_id: conversation_id.clone(),
				title: "SQLite restart proof".to_owned(),
				message: "Start the persisted task.".to_owned(),
				working_directory: temporary.path().display().to_string(),
			},
		)
		.await
		.expect("create Quick Task conversation");
	assert_eq!(conversation.revision, 1);

	let route = match store
		.route_quick_task_initial(
			"quick-task-route",
			&RouteQuickTaskInitial {
				conversation_id: conversation_id.clone(),
				expected_conversation_revision: 1,
			},
		)
		.await
		.expect("route initial Quick Task")
	{
		QuickTaskInitialRouteOutcome::Fresh(route) => route,
		other => panic!("initial route was not fresh: {other:?}"),
	};
	assert_eq!(route.decision.selected_account_id.as_ref(), Some(&account_id));

	let initial_plan = match store
		.plan_initial_thread_continuation(
			"quick-task-initial-plan",
			&PlanInitialThreadContinuation {
				operation_id: "91000000-0000-4000-8000-000000000001".to_owned(),
				routing_decision_id: route.decision_id.clone(),
				expected_conversation_revision: 1,
				plan_id: INITIAL_PLAN_ID.to_owned(),
			},
		)
		.await
		.expect("plan initial thread")
	{
		ContinuationCommandOutcome::Success(plan) => plan,
		ContinuationCommandOutcome::Rejected(rejection) => {
			panic!("initial plan was rejected: {rejection:?}")
		},
	};
	let initial_session =
		initial_plan.runtime_session.clone().expect("initial plan owns a RuntimeSession");
	assert_eq!(initial_plan.plan.kind, ContinuationPlanKind::InitialThread);
	assert_eq!(initial_session.state, RuntimeSessionState::Starting);
	assert_eq!(initial_session.account_snapshot.source_account_id, account_id);

	let initial_admission = store
		.admit_initial_quick_task_turn(
			&blob_store,
			"quick-task-initial-admission",
			&AdmitInitialQuickTaskTurn {
				expected_conversation_revision: 1,
				expected_runtime_session_revision: 1,
				continuation_plan_id: INITIAL_PLAN_ID.to_owned(),
				message: history_item(
					&conversation_id,
					&initial_session.runtime_session_id,
					&route.turn_id,
					1,
					TurnRole::User,
					INITIAL_HISTORY_ID,
					"Start the persisted task.",
				),
			},
		)
		.await
		.expect("admit initial user Turn");
	assert!(matches!(
		initial_admission,
		InitialQuickTaskTurnAdmissionOutcome::Fresh(ref admission)
			if admission.turn.turn_id == route.turn_id && admission.turn.revision == 1
	));

	let generation_id = ProcessGenerationId::new(GENERATION_ID).expect("generation identity");
	let process_admission = match store
		.prepare_quick_task_process_generation(
			"quick-task-process-admission",
			&PrepareQuickTaskProcessGeneration {
				conversation_id: conversation_id.clone(),
				expected_conversation_revision: 1,
				runtime_session_id: initial_session.runtime_session_id.clone(),
				expected_runtime_session_revision: 1,
				turn_id: route.turn_id.clone(),
				expected_turn_revision: 1,
				continuation_plan_id: INITIAL_PLAN_ID.to_owned(),
				routing_decision_id: route.decision_id.clone(),
				selected_account_id: account_id.clone(),
				process_generation_id: generation_id.clone(),
			},
		)
		.await
		.expect("admit process generation")
	{
		PrepareQuickTaskProcessGenerationOutcome::Fresh(admission) => admission,
		other => panic!("process admission was not fresh: {other:?}"),
	};
	let pre_spawn_readback = store
		.reconcile_quick_task_thread_establishment(&ReconcileQuickTaskThreadEstablishment {
			conversation_id: conversation_id.clone(),
			expected_conversation_revision: 1,
			runtime_session_id: initial_session.runtime_session_id.clone(),
			expected_runtime_session_revision: 1,
			turn_id: route.turn_id.clone(),
			expected_turn_revision: 1,
			continuation_plan_id: INITIAL_PLAN_ID.to_owned(),
			routing_decision_id: route.decision_id.clone(),
			selected_account_id: account_id.clone(),
			process_generation_id: generation_id.clone(),
		})
		.await
		.expect("reconcile admitted generation before spawn");
	assert!(matches!(
		pre_spawn_readback,
		QuickTaskThreadEstablishmentReadback::DefinitelyNotStarted(ref evidence)
			if evidence.process_generation_revision.is_none()
				&& evidence.kind == QuickTaskPreEffectEvidenceKind::AdmissionRejected
				&& evidence.evidence_id == "quick-task-process-admission"
	));
	let execution_epoch_id =
		ProcessExecutionEpochId::new(EXECUTION_EPOCH_ID).expect("execution epoch identity");
	let boot_id = ProcessBootIdentity::new("fixture-boot").expect("boot identity");
	let intent = ProcessGenerationIntent {
		generation_id: generation_id.clone(),
		account_id: account_id.clone(),
		runner_identity: ProcessRunnerIdentity::new(format!("sha256:{DIGEST_A}"))
			.expect("runner identity"),
		intended_boot_id: boot_id.clone(),
		control_kind: ProcessControlKind::StdioOnlyBestEffortEof,
		isolation_kind: ProcessIsolationKind::Session,
		execution_authorization: ProcessExecutionAuthorization::new(
			execution_epoch_id.clone(),
			DIGEST_B,
		)
		.expect("execution authorization"),
	};
	let process_binding = ProcessGenerationAccountBinding::new(1, credential.clone(), DIGEST_C)
		.expect("process account binding");
	let process_fence = match store
		.prepare_quick_task_bound_process_generation(&intent, &process_binding, process_admission)
		.await
		.expect("fence process generation")
	{
		PrepareProcessGenerationOutcome::Fresh(fence) => fence,
		other => panic!("process generation was not fresh: {other:?}"),
	};
	assert_eq!(process_fence.revision(), 1);
	let process_identity = ProcessIdentity::new(
		boot_id,
		41_001,
		ProcessStartIdentity::new("fixture-process-start").expect("process start identity"),
		41_001,
		41_001,
	)
	.expect("process identity");
	assert!(matches!(
		store
			.bind_process_generation_identity(&generation_id, 1, &process_identity)
			.await
			.expect("bind process identity"),
		ProcessGenerationMutationOutcome::Applied(ref mutation)
			if mutation.revision == 2 && mutation.state == ProcessGenerationState::Starting
	));
	assert!(matches!(
		store
			.mark_process_generation_ready(&generation_id, 2)
			.await
			.expect("mark process ready"),
		ProcessGenerationMutationOutcome::Applied(ref mutation)
			if mutation.revision == 3 && mutation.state == ProcessGenerationState::Ready
	));

	let thread_authority = match store
		.fence_runtime_session_thread_start(
			"quick-task-thread-fence",
			&FenceRuntimeSessionThreadStart {
				conversation_id: conversation_id.clone(),
				expected_conversation_revision: 1,
				runtime_session_id: initial_session.runtime_session_id.clone(),
				expected_revision: 1,
				turn_id: route.turn_id.clone(),
				expected_turn_revision: 1,
				continuation_plan_id: INITIAL_PLAN_ID.to_owned(),
				process_generation_id: generation_id.clone(),
				process_generation_revision: 3,
				process_execution_epoch_id: execution_epoch_id.clone(),
				thread_start_request_id: 1,
				thread_start_request_sha256: DIGEST_A.to_owned(),
			},
		)
		.await
		.expect("fence Codex thread start")
	{
		FenceRuntimeSessionThreadStartOutcome::Fresh(authority) => authority,
		other => panic!("thread-start fence was not fresh: {other:?}"),
	};
	let thread_binding = thread_authority.into_binding(SuccessfulRuntimeSessionThreadStart {
		response_id: 1,
		response_sha256: DIGEST_B.to_owned(),
		codex_thread_id: CODEX_THREAD_ID.to_owned(),
	});
	let bound_session = match store
		.bind_runtime_session_thread("quick-task-thread-binding", &thread_binding)
		.await
		.expect("bind Codex thread")
	{
		BindRuntimeSessionThreadOutcome::Applied(binding) => binding,
		other => panic!("thread binding was not applied: {other:?}"),
	};
	assert_eq!(bound_session.revision, 3);
	assert_eq!(bound_session.codex_thread_id, CODEX_THREAD_ID);

	let attempt_id = ProviderAttemptId::new(ATTEMPT_ID).expect("attempt identity");
	let request_id = ProviderRequestId::new(REQUEST_ID).expect("provider request identity");
	let provider_key =
		ProviderRequestKey::new("app-server:fixture:1").expect("provider correlation key");
	let preparation = ProviderAttemptPreparation::new(
		attempt_id.clone(),
		ProviderAttemptConsumer::ConversationTurn {
			conversation_id: conversation_id.clone(),
			turn_id: route.turn_id.clone(),
		},
		INITIAL_PLAN_ID,
		request_id.clone(),
		DIGEST_C,
		ProviderRequestKeys::new(None, Some(provider_key.clone())).expect("provider keys"),
		ProviderDuplicateRisk::OriginalIntent,
	)
	.expect("provider-attempt preparation");
	let prepared = match store
		.prepare_provider_attempt(
			&preparation,
			&generation_id,
			3,
			&execution_epoch_id,
			Some(&RuntimeSessionBindingReceipt::from_binding(&bound_session)),
			(Some(1), Some(1)),
		)
		.await
		.expect("prepare provider attempt")
	{
		PrepareProviderAttemptOutcome::Fresh(prepared) => prepared,
		other => panic!("provider attempt was not fresh: {other:?}"),
	};
	let authorized = match store
		.authorize_provider_attempt_dispatch(prepared, &generation_id, 3)
		.await
		.expect("authorize provider dispatch")
	{
		AuthorizeProviderDispatchOutcome::Fresh(fence) => fence,
		other => panic!("provider dispatch was not freshly authorized: {other:?}"),
	};
	assert_eq!(authorized.attempt_revision(), 2);

	let assistant_turn_id = TurnId::new(ASSISTANT_TURN_ID).expect("assistant Turn identity");
	store
		.record_history_item(
			&blob_store,
			&CommandIdentity::new("quick-task-assistant-history", b"assistant reply")
				.expect("assistant history command"),
			&history_item(
				&conversation_id,
				&initial_session.runtime_session_id,
				&assistant_turn_id,
				2,
				TurnRole::Assistant,
				ASSISTANT_HISTORY_ID,
				"The first persisted response.",
			),
		)
		.await
		.expect("record assistant response");
	let evidence_id = ProviderEvidenceId::new(EVIDENCE_ID).expect("provider evidence identity");
	let evidence = ProviderPositiveEvidence::new(
		evidence_id.clone(),
		attempt_id.clone(),
		request_id,
		ProviderEvidenceSource::ProviderReceipt,
		ProviderTerminalOutcome::Succeeded,
		provider_key,
		Some("fixture-provider-receipt".to_owned()),
		Some(CODEX_THREAD_ID.to_owned()),
		Some(PROVIDER_TURN_ID.to_owned()),
		DIGEST_D,
	)
	.expect("positive provider evidence");
	assert!(matches!(
		store
			.record_provider_attempt_positive_evidence(2, &evidence)
			.await
			.expect("record positive provider evidence"),
		ProviderAttemptMutationOutcome::Applied(ref mutation)
			if mutation.revision == 3 && mutation.state == ProviderAttemptState::Succeeded
	));
	let terminalized = match store
		.terminalize_quick_task_turn(
			"quick-task-terminalization",
			&TerminalizeQuickTaskTurn {
				conversation_id: conversation_id.clone(),
				expected_conversation_revision: 1,
				runtime_session_id: initial_session.runtime_session_id.clone(),
				expected_runtime_session_revision: 3,
				user_turn_id: route.turn_id.clone(),
				expected_user_turn_revision: 1,
				assistant_turn: Some((assistant_turn_id, 1)),
				provider_attempt_id: attempt_id.clone(),
				expected_provider_attempt_revision: 3,
				provider_evidence_id: evidence_id,
				provider_outcome: ProviderTerminalOutcome::Succeeded,
				provider_thread_id: CODEX_THREAD_ID.to_owned(),
				provider_turn_id: PROVIDER_TURN_ID.to_owned(),
			},
		)
		.await
		.expect("terminalize Quick Task Turn")
	{
		QuickTaskTerminalizationOutcome::Applied(readback) => readback,
		other => panic!("Quick Task terminalization was not applied: {other:?}"),
	};
	assert_eq!(terminalized.runtime_session_revision, 4);
	assert_eq!(
		store
			.conversation_history(&blob_store, &conversation_id, None, 10)
			.await
			.expect("read initial history")
			.entries
			.len(),
		2
	);
	let death = ProcessDeathEvidence::new(
		ProcessDeathEvidenceId::new(DEATH_EVIDENCE_ID).expect("death evidence identity"),
		generation_id.clone(),
		ProcessDeathEvidenceKind::ExactTerminationExit,
		process_identity.boot_id.clone(),
		Some(process_identity.clone()),
		DIGEST_A,
	)
	.expect("positive process death evidence");
	assert!(matches!(
		store
			.record_process_generation_death(3, &death)
			.await
			.expect("record process death before restart"),
		ProcessGenerationMutationOutcome::Applied(ref mutation)
			if mutation.revision == 4 && mutation.state == ProcessGenerationState::Dead
	));

	drop(store);
	let reopened = SqliteStore::open(&paths).expect("reopen SQLite after daemon restart");
	reopened.revalidate().await.expect("revalidate reopened SQLite");
	let routing = reopened
		.read_account_routing_control()
		.await
		.expect("read account routing before changing the default");
	let changed_routing = match reopened
		.set_fixed_account_selection(routing.revision, &alternate_account_id, 1)
		.await
		.expect("change the default account for new conversations")
	{
		RoutingControlOutcome::Updated { routing } => routing,
		other => panic!("account routing did not update: {other:?}"),
	};
	assert_eq!(
		changed_routing.mode,
		AccountSelectionMode::Fixed(alternate_account_id.clone())
	);

	let alternate_conversation_id =
		ConversationId::new(ALTERNATE_CONVERSATION_ID).expect("alternate conversation identity");
	reopened
		.create_quick_task_conversation(
			&CommandIdentity::new("alternate-quick-task-conversation", b"create alternate conversation")
				.expect("alternate conversation command"),
			&CreateQuickTaskConversation {
				conversation_id: alternate_conversation_id.clone(),
				title: "Independent account affinity proof".to_owned(),
				message: "Start an independent task.".to_owned(),
				working_directory: temporary.path().display().to_string(),
			},
		)
		.await
		.expect("create alternate Quick Task conversation");
	let alternate_route = match reopened
		.route_quick_task_initial(
			"alternate-quick-task-route",
			&RouteQuickTaskInitial {
				conversation_id: alternate_conversation_id,
				expected_conversation_revision: 1,
			},
		)
		.await
		.expect("route alternate Quick Task")
	{
		QuickTaskInitialRouteOutcome::Fresh(route) => route,
		other => panic!("alternate initial route was not fresh: {other:?}"),
	};
	assert_eq!(
		alternate_route.decision.selected_account_id.as_ref(),
		Some(&alternate_account_id),
		"a new conversation may use the newly selected account",
	);

	let resume = reopened
		.read_ordinary_runtime_session_for_resume(&conversation_id)
		.await
		.expect("read restart projection")
		.expect("active RuntimeSession survives restart");
	assert_eq!(resume.runtime_session_id, initial_session.runtime_session_id);
	assert_eq!(resume.runtime_session_revision, 4);
	assert_eq!(resume.codex_thread_id, CODEX_THREAD_ID);
	assert_eq!(resume.source_account_id, account_id);
	assert_eq!(resume.next_turn_sequence, 3);
	assert!(resume.has_acknowledged_turn);
	assert!(!resume.has_active_turn);
	assert!(!resume.has_unresolved_provider_attempt);

	let later_turn_id = TurnId::new(LATER_TURN_ID).expect("later Turn identity");
	let later_history = history_item(
		&conversation_id,
		&resume.runtime_session_id,
		&later_turn_id,
		resume.next_turn_sequence,
		TurnRole::User,
		LATER_HISTORY_ID,
		"Continue after restart.",
	);
	assert!(matches!(
		reopened
			.reserve_user_turn_with_history_item(
				&blob_store,
				&CommandIdentity::new("quick-task-later-turn", b"continue after restart")
					.expect("later Turn command"),
				&later_history,
			)
			.await
			.expect("reserve later user Turn"),
		TurnReservationOutcome::Fresh(ref reservation)
			if reservation.turn_id == later_turn_id && reservation.sequence == 3
	));
	let continuation_binding = match reopened
		.bind_quick_task_continuation(
			"quick-task-continuation-route",
			&BindQuickTaskContinuation {
				operation_id: "a1000000-0000-4000-8000-000000000001".to_owned(),
				conversation_id: conversation_id.clone(),
				expected_conversation_revision: 1,
				source_runtime_session_id: resume.runtime_session_id.clone(),
				expected_source_runtime_session_revision: resume.runtime_session_revision,
				turn_id: later_turn_id.clone(),
			},
		)
		.await
		.expect("bind restart continuation")
	{
		RoutingCommandOutcome::Success(binding) => binding,
		RoutingCommandOutcome::Rejected(rejection) => {
			panic!("restart continuation binding was rejected: {rejection:?}")
		},
	};
	let fallback_pack = compile_context_pack(ContextPackInput {
		conversation_id: conversation_id.clone(),
		possible_side_effects: PossibleSideEffects::Unknown,
		policy: ContextPackPolicy::new(4_096, 4).expect("Context Pack policy"),
		pinned: PinnedContextSource::new(
			"restart-proof",
			1,
			"This fallback is inert because same-thread evidence exists.",
		)
		.expect("pinned Context Pack source"),
		optional_sources: vec![],
	})
	.expect("compile fallback Context Pack");
	let continuation_request = PlanContinuation {
		operation_id: "a2000000-0000-4000-8000-000000000001".to_owned(),
		routing_decision_id: continuation_binding.decision_id.clone(),
		expected_consumer_revision: 1,
		plan_id: CONTINUATION_PLAN_ID.to_owned(),
		fallback_runtime_session_id: "a3000000-0000-4000-8000-000000000001".to_owned(),
		fallback_account_snapshot_id: continuation_binding.account_snapshot_id,
		fallback_context_pack_id: "a4000000-0000-4000-8000-000000000001".to_owned(),
	};
	let continuation = match reopened
		.plan_continuation(
			&blob_store,
			"quick-task-continuation-plan",
			&continuation_request,
			&fallback_pack,
		)
		.await
		.expect("plan restart continuation")
	{
		ContinuationCommandOutcome::Success(plan) => plan,
		ContinuationCommandOutcome::Rejected(rejection) => {
			panic!("restart continuation plan was rejected: {rejection:?}")
		},
	};
	assert_eq!(continuation.plan.kind, ContinuationPlanKind::SameThread);
	assert_eq!(continuation.plan.codex_thread_id.as_deref(), Some(CODEX_THREAD_ID));
	assert_eq!(continuation.plan.source_runtime_session_id, resume.runtime_session_id);
	assert_eq!(continuation.plan.source_runtime_session_revision, 4);
	assert_eq!(continuation.plan.selected_account_id, account_id);
	assert_ne!(continuation.plan.selected_account_id, alternate_account_id);
	assert!(matches!(
		continuation.plan.same_thread_evidence,
		Some(SameThreadContinuationEvidence::ProviderAttempt {
			ref attempt_id,
			attempt_revision: 3,
			..
		}) if attempt_id.as_str() == ATTEMPT_ID
	));
	let rehydrated_generation_id =
		ProcessGenerationId::new(REHYDRATED_GENERATION_ID).expect("rehydrated generation identity");
	assert!(matches!(
		reopened
			.prepare_quick_task_process_generation(
				"quick-task-rehydrated-process-admission",
				&PrepareQuickTaskProcessGeneration {
					conversation_id: conversation_id.clone(),
					expected_conversation_revision: 1,
					runtime_session_id: resume.runtime_session_id.clone(),
					expected_runtime_session_revision: resume.runtime_session_revision,
					turn_id: later_turn_id.clone(),
					expected_turn_revision: 1,
					continuation_plan_id: CONTINUATION_PLAN_ID.to_owned(),
					routing_decision_id: continuation_binding.decision_id.clone(),
					selected_account_id: account_id.clone(),
					process_generation_id: rehydrated_generation_id,
				},
			)
			.await
			.expect("admit rehydrated process generation"),
		PrepareQuickTaskProcessGenerationOutcome::Fresh(_)
	));

	assert!(matches!(
		reopened
			.prepare_provider_attempt(
				&preparation,
				&generation_id,
				3,
				&execution_epoch_id,
				Some(&RuntimeSessionBindingReceipt::from_binding(&bound_session)),
				(Some(1), Some(1)),
			)
			.await
			.expect("replay original provider attempt"),
		PrepareProviderAttemptOutcome::Replayed(ref mutation)
			if mutation.revision == 3 && mutation.state == ProviderAttemptState::Succeeded
	));
	let attempts = reopened
		.read_provider_attempt_page(None, None, None, 16)
		.await
		.expect("read provider attempts");
	assert_eq!(attempts.len(), 1, "restart must not create a duplicate dispatch intent");
	assert_eq!(attempts[0].attempt_id, attempt_id);
	assert_eq!(
		reopened
			.conversation_history(&blob_store, &conversation_id, None, 10)
			.await
			.expect("read history after restart")
			.entries
			.len(),
		3
	);
}

async fn import_ready_accounts(
	store: &SqliteStore,
) -> (AccountId, CredentialBinding, AccountId) {
	let (account_id, credential, primary) = fixture_account_transfer(
		ACCOUNT_ID,
		OPERATION_ID,
		PROVIDER_ACCOUNT_ID,
		DIGEST_A,
		"Primary fixture account",
		b"opaque-primary-fixture-credential",
	);
	let (alternate_account_id, _, alternate) = fixture_account_transfer(
		ALTERNATE_ACCOUNT_ID,
		ALTERNATE_OPERATION_ID,
		ALTERNATE_PROVIDER_ACCOUNT_ID,
		DIGEST_B,
		"Alternate fixture account",
		b"opaque-alternate-fixture-credential",
	);
	assert_eq!(
		store
			.import_local_accounts(LocalAccountTransferBatch {
				source_sha256: DIGEST_D.to_owned(),
				accounts: vec![primary, alternate],
				routing: AccountRoutingControl {
					revision: 1,
					mode: AccountSelectionMode::Fixed(account_id.clone()),
					order: vec![account_id.clone(), alternate_account_id.clone()],
				},
			})
			.expect("import fixture accounts"),
		LocalAccountTransferOutcome::Imported { account_count: 2 }
	);
	assert!(
		store
			.attest_codex_account_capability(&CodexAccountCapabilityAttestation {
				build_identity: "fixture-codex-build".to_owned(),
				executable_sha256: DIGEST_A.to_owned(),
				schema_sha256: DIGEST_B.to_owned(),
				callback_profile_sha256: DIGEST_C.to_owned(),
				login_chatgpt_auth_tokens: true,
				refresh_callback: true,
			})
			.await
			.expect("attest Codex account capability")
	);
	(account_id, credential, alternate_account_id)
}

fn fixture_account_transfer(
	account_id: &str,
	operation_id: &str,
	provider_account_id: &str,
	fingerprint: &str,
	label: &str,
	payload: &[u8],
) -> (AccountId, CredentialBinding, LocalAccountTransfer) {
	let typed_account_id = AccountId::new(account_id).expect("account identity");
	let typed_operation_id =
		AccountOperationId::new(operation_id).expect("operation identity");
	let provider = ProviderIdentity::new(AccountProvider::Chatgpt, provider_account_id)
		.expect("provider identity");
	let credential = CredentialBinding {
		schema_version: CredentialStoreSchemaVersion::V1,
		version: CredentialVersion::new(1).expect("credential version"),
		fingerprint: CredentialFingerprint::new(fingerprint).expect("credential fingerprint"),
		provider,
		writer_operation_id: typed_operation_id,
	};
	let account = AccountRecord {
		account_id: typed_account_id.clone(),
		label: label.to_owned(),
		enabled: true,
		revision: 1,
		observed_state: AccountState::Available,
		lifecycle_readiness: AccountLifecycleReadiness::Ready,
		credential: Some(credential.clone()),
		unsettled_operation: None,
		five_hour_quota: AccountQuotaWindowObservation::unknown(
			AccountQuotaWindow::FIVE_HOURS_MINUTES,
		)
		.expect("unknown five-hour window"),
		seven_day_quota: AccountQuotaWindowObservation::unknown(
			AccountQuotaWindow::SEVEN_DAYS_MINUTES,
		)
		.expect("unknown seven-day window"),
		tombstoned: false,
	};
	let transferred = LocalAccountTransfer {
		account,
		credential: CredentialRecord {
			key: CredentialKey {
				account_id: account_id.to_owned(),
				schema_version: 1,
				credential_version: 1,
				fingerprint: fingerprint.to_owned(),
				writer_operation_id: operation_id.to_owned(),
				provider: "chatgpt".to_owned(),
				provider_account_id: provider_account_id.to_owned(),
			},
			payload: Zeroizing::new(payload.to_vec()),
		},
	};
	(typed_account_id, credential, transferred)
}

fn history_item(
	conversation_id: &ConversationId,
	runtime_session_id: &decodex_core::RuntimeSessionId,
	turn_id: &TurnId,
	turn_sequence: i64,
	turn_role: TurnRole,
	history_item_id: &str,
	text: &str,
) -> RecordHistoryItem {
	RecordHistoryItem {
		conversation_id: conversation_id.clone(),
		runtime_session_id: runtime_session_id.clone(),
		turn_id: turn_id.clone(),
		turn_sequence,
		turn_role,
		possible_side_effects: PossibleSideEffects::Unknown,
		history_item_id: HistoryItemId::new(history_item_id).expect("history item identity"),
		ordinal: 0,
		kind: HistoryItemKind::Message,
		status: ItemStatus::Completed,
		text: text.to_owned(),
		media_type: HistoryMediaType::new("text/markdown").expect("history media type"),
		metadata: HistoryMetadata::empty(),
		expected_revision: None,
		artifact: None,
	}
}
