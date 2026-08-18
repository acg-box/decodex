//! Production GPUI window, navigation, focus, and lifecycle rendering boundary.

use std::{future::Future, pin::Pin, sync::mpsc::Receiver, time::Duration};

use gpui::{
	Animation, AnimationExt, AnyElement, App, BoxShadow, Context, Entity, FocusHandle, Focusable,
	FontWeight, Global, Hsla, KeyBinding, MouseButton, Render, Role, SharedString, Subscription,
	Task, WeakEntity, Window, WindowControlArea, WindowHandle, WindowId, actions, div, ease_in_out,
	img, prelude::*, px, rgb, rgba,
};

use decodex_protocol::{
	AccountDto, AccountLifecycleReadinessDto, AccountObservedStateDto, AccountQuotaStateDto,
	AccountQuotaWindowDto, AccountSelectionModeDto, AppServerCapability, ClientFailure,
	DoctorComponent, DoctorIssue, DoctorStatus, EntityId, HistoryItemDto, HistoryItemKindDto,
	HistoryItemStatusDto, HistoryPayloadDto, HistoryTurnRole, QuickTaskRecoveryAction,
	QuickTaskState, QuickTaskSummary, WorkItemBoardCard, WorkItemState,
};

use crate::{
	accounts::{
		AccountCommandState, AccountInputError, AccountsController, AccountsLoadState,
		AccountsSnapshot,
	},
	client_lifecycle::{
		ClientLifecycle, CompatibilityReason, ConnectionView, LifecycleCancellation,
		QuarantineReason, QuarantineRecovery,
	},
	composer_input::{self, ComposerEvent, ComposerInput, MAX_COMPOSER_BYTES, SubmitComposer},
	factory_surface::{FactoryEvent, FactoryRoute, FactorySurface, app_icon_path},
	health_query::{HealthLoadState, HealthQuery, HealthSnapshot},
	history_pager::{HistoryLoadState, HistoryPageSource, HistoryPager, HistorySnapshot},
	programs::{Programs, ProgramsSnapshot},
	quick_tasks::{
		QueuedQuickTaskSubmission, QuickTaskCommandState, QuickTaskInputError,
		QuickTaskRefreshState, QuickTasks, QuickTasksLoadState, QuickTasksSnapshot,
	},
	settings_surface::SettingsSurface,
	ui_theme,
	work_items::{WorkItems, WorkItemsSnapshot},
};

const WORKBENCH_TOPBAR_HEIGHT: f32 = 42.0;
const WORKBENCH_SESSION_SIDEBAR_WIDTH: f32 = 248.0;
const WORKBENCH_INSPECTOR_WIDTH: f32 = 344.0;
const LIFECYCLE_POLL: Duration = Duration::from_millis(40);

const WB_CANVAS: u32 = ui_theme::CANVAS;
const WB_TEXT: u32 = ui_theme::TEXT;
const WB_TEXT_MUTED: u32 = ui_theme::TEXT_MUTED;
const WB_TEXT_FAINT: u32 = ui_theme::TEXT_FAINT;
const WB_ACCENT: u32 = ui_theme::ACCENT;
const WB_BLUE: u32 = ui_theme::BLUE;
const WB_GREEN: u32 = ui_theme::GREEN;
const WB_AMBER: u32 = ui_theme::AMBER;

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingComposerSubmission {
	content: String,
	result_generation: u64,
	conversation_id: EntityId,
	turn_id: Option<EntityId>,
	accepted: bool,
}

fn pending_submission_clear_decision(
	pending: &PendingComposerSubmission,
	result_generation: u64,
	accepted: bool,
	current_content: &str,
) -> Option<bool> {
	(result_generation > pending.result_generation)
		.then_some(accepted && current_content == pending.content)
}

fn pending_submission_is_persisted(
	pending: &PendingComposerSubmission,
	history: Option<&HistorySnapshot>,
) -> bool {
	let Some(history) = history
		.filter(|history| history.conversation_id.as_ref() == Some(&pending.conversation_id))
	else {
		return false;
	};
	let Some(page) = history.visible.as_ref() else {
		return false;
	};

	page.items.iter().any(|item| {
		item.turn_role == HistoryTurnRole::User
			&& item.kind == HistoryItemKindDto::Message
			&& pending.turn_id.as_ref().map_or_else(
				|| item.payload.inline_text().is_some_and(|text| text.as_str() == pending.content),
				|turn_id| &item.turn_id == turn_id,
			)
	})
}

fn deferred_provider_refresh_ready(
	pending_conversation_id: Option<&EntityId>,
	selected_conversation_id: Option<&EntityId>,
	history: Option<&HistorySnapshot>,
) -> bool {
	let (Some(pending), Some(selected), Some(history)) =
		(pending_conversation_id, selected_conversation_id, history)
	else {
		return false;
	};

	pending == selected
		&& history.conversation_id.as_ref() == Some(selected)
		&& history.visible.is_some()
		&& history.visible_source == Some(HistoryPageSource::FreshServer)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TranscriptRow {
	Prompt {
		turn_id: Option<EntityId>,
		text: String,
		pending: bool,
	},
	Response {
		turn_id: EntityId,
		text: String,
		live: bool,
	},
	Activity {
		history_item_id: EntityId,
		kind: HistoryItemKindDto,
		status: HistoryItemStatusDto,
		text: String,
	},
}

fn history_item_text(item: &HistoryItemDto) -> String {
	match &item.payload {
		HistoryPayloadDto::Inline { text } => text.as_str().to_owned(),
		HistoryPayloadDto::Blob(reference) => format!(
			"Stored content: {} bytes; SHA-256 {}...",
			reference.byte_length.get(),
			&reference.sha256.as_str()[..12],
		),
	}
}

fn append_response_row(rows: &mut Vec<TranscriptRow>, turn_id: &EntityId, text: &str, live: bool) {
	if let Some(TranscriptRow::Response {
		turn_id: existing_turn,
		text: existing,
		live: existing_live,
	}) = rows.last_mut()
		&& existing_turn == turn_id
	{
		existing.push_str(text);
		*existing_live |= live;
		return;
	}

	rows.push(TranscriptRow::Response { turn_id: turn_id.clone(), text: text.to_owned(), live });
}

fn quick_task_transcript_rows(
	snapshot: &QuickTasksSnapshot,
	history: Option<&HistorySnapshot>,
	pending: Option<&PendingComposerSubmission>,
) -> Vec<TranscriptRow> {
	let selected = snapshot.selected.as_ref();
	let visible_history = history.filter(|history| history.conversation_id.as_ref() == selected);
	let persisted_inline_ids = visible_history
		.and_then(|history| history.visible.as_ref())
		.into_iter()
		.flat_map(|page| page.items.iter())
		.filter(|item| item.payload.inline_text().is_some())
		.map(|item| item.history_item_id.clone())
		.collect::<Vec<_>>();
	let mut rows = Vec::new();

	for item in visible_history
		.and_then(|history| history.visible.as_ref())
		.into_iter()
		.flat_map(|page| page.items.iter())
	{
		let text = history_item_text(item);
		match (item.turn_role, item.kind) {
			(HistoryTurnRole::User, HistoryItemKindDto::Message) => {
				rows.push(TranscriptRow::Prompt {
					turn_id: Some(item.turn_id.clone()),
					text,
					pending: false,
				});
			},
			(HistoryTurnRole::Assistant, HistoryItemKindDto::Message) => {
				append_response_row(&mut rows, &item.turn_id, &text, false);
			},
			_ => rows.push(TranscriptRow::Activity {
				history_item_id: item.history_item_id.clone(),
				kind: item.kind,
				status: item.status,
				text,
			}),
		}
	}

	let active_conversation = selected.or_else(|| pending.map(|pending| &pending.conversation_id));
	if let Some(pending) = pending
		&& active_conversation == Some(&pending.conversation_id)
		&& !pending_submission_is_persisted(pending, visible_history)
	{
		rows.push(TranscriptRow::Prompt {
			turn_id: pending.turn_id.clone(),
			text: pending.content.clone(),
			pending: true,
		});
	}

	for delta in snapshot.live_deltas.iter().filter(|delta| {
		active_conversation == Some(&delta.conversation_id)
			&& !persisted_inline_ids.iter().any(|persisted| persisted == &delta.history_item_id)
	}) {
		append_response_row(&mut rows, &delta.turn_id, delta.text.as_str(), true);
	}

	rows
}

fn quick_task_recovery_presentation(task: Option<&QuickTaskSummary>) -> (bool, &'static str) {
	let recovery_action = task.and_then(|task| task.recovery_action);
	let outcome_unknown = task.is_some_and(|task| task.state == QuickTaskState::OutcomeUnknown);
	let executable = outcome_unknown
		|| recovery_action.is_some_and(|action| {
			matches!(
				action,
				QuickTaskRecoveryAction::ResumeRouting
					| QuickTaskRecoveryAction::CreateRoutingSuccessor
					| QuickTaskRecoveryAction::ResumeEstablishment
					| QuickTaskRecoveryAction::StartNewConversation
			)
		});
	let label = if outcome_unknown {
		"Retry sync"
	} else if recovery_action == Some(QuickTaskRecoveryAction::StartNewConversation) {
		"Start new"
	} else {
		"Recover"
	};
	(executable, label)
}

const HEALTH_CORE_COMPONENTS: [DoctorComponent; 8] = [
	DoctorComponent::Configuration,
	DoctorComponent::ProductStore,
	DoctorComponent::QuickTask,
	DoctorComponent::Protocol,
	DoctorComponent::ProtocolVersion,
	DoctorComponent::ServerIdentity,
	DoctorComponent::SharedCodexHome,
	DoctorComponent::CredentialVault,
];
const HEALTH_APP_SERVER_COMPONENTS: [DoctorComponent; 8] = [
	DoctorComponent::AppServerCapability(AppServerCapability::Initialize),
	DoctorComponent::AppServerCapability(AppServerCapability::AccountRead),
	DoctorComponent::AppServerCapability(AppServerCapability::ThreadList),
	DoctorComponent::AppServerCapability(AppServerCapability::ThreadRead),
	DoctorComponent::AppServerCapability(AppServerCapability::ThreadArchive),
	DoctorComponent::AppServerCapability(AppServerCapability::PaginatedHistory),
	DoctorComponent::AppServerCapability(AppServerCapability::NativeCollaboration),
	DoctorComponent::AppServerCapability(AppServerCapability::ThreadSearch),
];
const HEALTH_OPTIONAL_COMPONENTS: [DoctorComponent; 3] = [
	DoctorComponent::ManagedRepository,
	DoctorComponent::BlobIntegrity,
	DoctorComponent::PluginReadiness,
];

actions!(
	decodex_shell,
	[
		FocusNext,
		FocusPrevious,
		ActivateDestination,
		ActivateFactory,
		ActivateQuickTasks,
		ActivateHealth,
		RefreshHealth,
		ToggleSidebar,
		ToggleInspector,
	]
);

/// Stable shell destinations. Each live destination remains issue-owned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Destination {
	Factory,
	Advisor,
	Projects,
	QuickTasks,
	Runs,
	Automations,
	Accounts,
	Health,
	Settings,
}

impl Destination {
	pub(crate) const ALL: [Self; 9] = [
		Self::Factory,
		Self::Advisor,
		Self::Projects,
		Self::QuickTasks,
		Self::Runs,
		Self::Automations,
		Self::Accounts,
		Self::Health,
		Self::Settings,
	];
	const CHROME: [Self; 4] = [Self::QuickTasks, Self::Factory, Self::Accounts, Self::Health];

	pub(crate) const fn label(self) -> &'static str {
		match self {
			Self::Factory => "Factory",
			Self::Advisor => "Advisor",
			Self::Projects => "Projects",
			Self::QuickTasks => "Quick Tasks",
			Self::Runs => "Runs",
			Self::Automations => "Automations",
			Self::Accounts => "Accounts",
			Self::Health => "Health",
			Self::Settings => "Settings",
		}
	}

	const fn description(self) -> &'static str {
		match self {
			Self::Factory => "Move managed work through Codex.",
			Self::Advisor => "Review guidance and bounded decisions.",
			Self::Projects => "Own repositories and product context.",
			Self::QuickTasks => "Converse with Codex and inspect execution.",
			Self::Runs => "Inspect managed run activity and evidence.",
			Self::Automations => "Operate scheduled and event-driven work.",
			Self::Accounts => "Open multi-account routing and recovery.",
			Self::Health => "Inspect daemon and app-server readiness.",
			Self::Settings => "Configure the window and menu bar companion.",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InspectorTab {
	WorkItem,
	Activity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConnectionPresentation {
	label: &'static str,
	detail: SharedString,
	color: u32,
}

fn connection_presentation(view: ConnectionView) -> ConnectionPresentation {
	match view {
		ConnectionView::Connecting { attempt } => ConnectionPresentation {
			label: "Connecting",
			detail: format!("Establishing verified session · attempt {attempt}").into(),
			color: 0xf59e0b,
		},
		ConnectionView::Online { generation, applied } => ConnectionPresentation {
			label: "Online",
			detail: match applied {
				Some(cursor) => format!("Authority generation {generation} · applied {}", cursor.0),
				None => format!("Authority generation {generation} · awaiting snapshot"),
			}
			.into(),
			color: 0x22c55e,
		},
		ConnectionView::OfflineRetrying { next_attempt, delay } => ConnectionPresentation {
			label: "Offline · retrying",
			detail: format!(
				"Connection unavailable · attempt {next_attempt} in {} ms",
				delay.as_millis()
			)
			.into(),
			color: 0xf97316,
		},
		ConnectionView::Incompatible(reason) => ConnectionPresentation {
			label: "Incompatible",
			detail: match reason {
				CompatibilityReason::Startup(failure) => startup_failure(failure),
				CompatibilityReason::InvalidEndpoint => "Selected endpoint is not supported",
				CompatibilityReason::ProtocolMajor => "Protocol generation does not match",
				CompatibilityReason::ProtocolMinor => "Protocol revision is not supported",
				CompatibilityReason::PublicationIdentityUnavailable =>
					"Publication identity is unavailable",
			}
			.into(),
			color: 0xef4444,
		},
		ConnectionView::Quarantined { reason, recovery } => ConnectionPresentation {
			label: quarantine_label(reason),
			detail: format!("{} · {}", quarantine_reason(reason), recovery_label(recovery)).into(),
			color: 0xdc2626,
		},
		ConnectionView::ShuttingDown => ConnectionPresentation {
			label: "Shutting down",
			detail: "Closing the retained session cooperatively".into(),
			color: 0x94a3b8,
		},
		ConnectionView::Stopped => ConnectionPresentation {
			label: "Stopped",
			detail: "No connection or retry work remains".into(),
			color: 0x64748b,
		},
	}
}

const fn startup_failure(failure: ClientFailure) -> &'static str {
	match failure {
		ClientFailure::ConfigurationMissing => "Client configuration is missing",
		ClientFailure::ConfigurationMalformed => "Client configuration is malformed",
		ClientFailure::ConfigurationVersion => "Client configuration version is unsupported",
		ClientFailure::ProfileMissing => "Selected server profile is missing",
		ClientFailure::UnsafeHostPath => "Client configuration path is unsafe",
		ClientFailure::ServerIdentityUnavailable => "Stable server identity is unavailable",
		ClientFailure::RemoteMutationUnsupported =>
			"Reset-card operations require a local pinned profile",
		ClientFailure::LocalTransportDisabled => "Local daemon transport is disabled",
		ClientFailure::RemoteTransportDisabled => "Remote daemon transport is disabled",
		ClientFailure::LocalTransportUnsupported => "Local daemon transport is unsupported",
		ClientFailure::UnsafeLocalEndpoint => "Local daemon endpoint is unsafe",
		ClientFailure::LocalPeerIdentityUnavailable => "Local daemon identity is unavailable",
		ClientFailure::LocalPeerUidMismatch => "Local daemon peer UID does not match",
		ClientFailure::ProtocolDisconnected => "Daemon protocol is disconnected",
		ClientFailure::ProtocolTimeout => "Daemon protocol timed out",
		ClientFailure::ProtocolMajorMismatch => "Protocol generation does not match",
		ClientFailure::ProtocolMinorMismatch => "Protocol revision is not supported",
		ClientFailure::ArtifactCohortMismatch => "Installed Decodex artifacts do not match",
		ClientFailure::ServerIdentityMismatch => "Stable server identity does not match",
		ClientFailure::ProtocolMalformed => "Daemon response is malformed",
		ClientFailure::ProtocolViolation => "Daemon protocol ordering was refused",
		ClientFailure::ProtocolBackpressure => "Daemon message allowance was exhausted",
		ClientFailure::ApplicationAcceptanceUnknown => "Application command acceptance is unknown",
	}
}

const fn quarantine_label(reason: QuarantineReason) -> &'static str {
	match reason {
		QuarantineReason::StableServerIdentity
		| QuarantineReason::AuthorityChanged
		| QuarantineReason::PublicationInstanceChanged
		| QuarantineReason::CheckpointMismatch => "Quarantined · identity mismatch",
		QuarantineReason::CacheCorrupt
		| QuarantineReason::CacheRootUnsafe
		| QuarantineReason::ContentAttestation => "Quarantined · cache integrity",
		QuarantineReason::ApplicationOrder => "Quarantined · application order",
		QuarantineReason::ApplicationConfirmation => "Quarantined · confirmation failure",
		QuarantineReason::StaleConnectionGeneration => "Quarantined · generation fence",
	}
}

const fn quarantine_reason(reason: QuarantineReason) -> &'static str {
	match reason {
		QuarantineReason::StableServerIdentity => "Stable server identity changed",
		QuarantineReason::AuthorityChanged => "Selected authority changed",
		QuarantineReason::PublicationInstanceChanged => "Publication instance changed",
		QuarantineReason::CheckpointMismatch => "Checkpoint identity did not match",
		QuarantineReason::CacheCorrupt => "Disposable cache could not be attested",
		QuarantineReason::CacheRootUnsafe => "Disposable cache root is unsafe",
		QuarantineReason::ContentAttestation => "Published content failed attestation",
		QuarantineReason::ApplicationOrder => "Application order was invalid",
		QuarantineReason::ApplicationConfirmation => "Application confirmation failed",
		QuarantineReason::StaleConnectionGeneration => "Connection generation became stale",
	}
}

const fn recovery_label(recovery: QuarantineRecovery) -> &'static str {
	match recovery {
		QuarantineRecovery::VerifiedSnapshotReplacement => "waiting for verified replacement",
		QuarantineRecovery::DisposedBeforeRebuild => "disposed before bounded rebuild",
		QuarantineRecovery::OperatorRequired => "operator action required",
	}
}

/// Bind the shell's complete keyboard path once at application startup.
pub(crate) fn bind_keys(cx: &mut App) {
	composer_input::bind_keys(cx);
	crate::factory_surface::bind_keys(cx);
	cx.bind_keys([
		KeyBinding::new("tab", FocusNext, None),
		KeyBinding::new("shift-tab", FocusPrevious, None),
		KeyBinding::new("cmd-1", ActivateFactory, None),
		KeyBinding::new("cmd-2", ActivateQuickTasks, None),
		KeyBinding::new("cmd-3", ActivateHealth, None),
		KeyBinding::new("cmd-b", ToggleSidebar, None),
		KeyBinding::new("cmd-shift-b", ToggleInspector, None),
		KeyBinding::new("enter", ActivateDestination, Some("Destination")),
		KeyBinding::new("space", ActivateDestination, Some("Destination")),
		KeyBinding::new("enter", RefreshHealth, Some("HealthRefresh")),
		KeyBinding::new("space", RefreshHealth, Some("HealthRefresh")),
	]);
}

/// One window-owned production shell. Connection ownership lives at application scope.
pub(crate) struct Shell {
	selected: Destination,
	inspector_tab: InspectorTab,
	left_sidebar_visible: bool,
	left_sidebar_mounted: bool,
	left_sidebar_motion_generation: u64,
	inspector_visible: bool,
	inspector_mounted: bool,
	inspector_motion_generation: u64,
	connection: ConnectionView,
	root_focus: FocusHandle,
	destination_focus: Vec<FocusHandle>,
	refresh_focus: FocusHandle,
	composer: Entity<ComposerInput>,
	factory: Entity<FactorySurface>,
	settings: Entity<SettingsSurface>,
	accounts_controller: AccountsController,
	accounts: AccountsSnapshot,
	account_status: Option<SharedString>,
	health_query: HealthQuery,
	health: HealthSnapshot,
	quick_tasks: QuickTasks,
	quick: QuickTasksSnapshot,
	programs: Programs,
	program: ProgramsSnapshot,
	work_items: WorkItems,
	work: WorkItemsSnapshot,
	history_pager: Option<HistoryPager>,
	history: Option<HistorySnapshot>,
	opened_history: Option<EntityId>,
	deferred_provider_refresh: Option<EntityId>,
	creating_new: bool,
	pending_submission: Option<PendingComposerSubmission>,
	input_status: Option<SharedString>,
	titlebar_drag_pending: bool,
}

impl Shell {
	pub(crate) fn new(
		window: &mut Window,
		cx: &mut Context<Self>,
		connection: ConnectionView,
	) -> Self {
		let destination_focus = Destination::ALL
			.iter()
			.enumerate()
			.map(|(index, _)| cx.focus_handle().tab_index(index as isize).tab_stop(true))
			.collect::<Vec<_>>();
		let refresh_focus =
			cx.focus_handle().tab_index(Destination::ALL.len() as isize).tab_stop(true);
		let root_focus = cx.focus_handle();
		let composer = cx.new(|cx| ComposerInput::new(Destination::ALL.len() as isize + 1, cx));
		cx.subscribe(&composer, |shell, _, _: &ComposerEvent, cx| {
			shell.input_status = None;
			cx.notify();
		})
		.detach();
		let factory = cx.new(FactorySurface::new);
		cx.subscribe(&factory, |shell, _, event: &FactoryEvent, cx| {
			shell.handle_factory_event(event, cx);
		})
		.detach();
		let settings = cx.new(SettingsSurface::new);
		let accounts_controller = AccountsController::production();
		let accounts = accounts_controller.snapshot();
		let health_query = HealthQuery::production();
		let health = health_query.snapshot();
		let quick_tasks = QuickTasks::production();
		quick_tasks.activate();
		let quick = quick_tasks.snapshot();
		let programs = Programs::production();
		let program = programs.snapshot();
		let work_items = WorkItems::production();
		let work = work_items.snapshot();
		factory.update(cx, |factory, cx| factory.bind_work_items(work_items.clone(), cx));
		factory.update(cx, |factory, cx| factory.bind_programs(programs.clone(), cx));
		window.focus(&root_focus, cx);

		Self {
			selected: Destination::QuickTasks,
			inspector_tab: InspectorTab::WorkItem,
			left_sidebar_visible: true,
			left_sidebar_mounted: true,
			left_sidebar_motion_generation: 0,
			inspector_visible: true,
			inspector_mounted: true,
			inspector_motion_generation: 0,
			connection,
			root_focus,
			destination_focus,
			refresh_focus,
			composer,
			factory,
			settings,
			accounts_controller,
			accounts,
			account_status: None,
			health_query,
			health,
			quick_tasks,
			quick,
			programs,
			program,
			work_items,
			work,
			history_pager: None,
			history: None,
			opened_history: None,
			deferred_provider_refresh: None,
			creating_new: true,
			pending_submission: None,
			input_status: None,
			titlebar_drag_pending: false,
		}
	}

	#[cfg(feature = "visual-capture")]
	#[allow(dead_code)]
	pub(crate) fn visual_workbench(window: &mut Window, cx: &mut Context<Self>) -> Self {
		use decodex_protocol::{
			AccountRoutingControlDto, ConversationHistoryPage, EntityRevision, ProjectSummary,
			QuickTaskSummary, WireText, WorkItemBoardLeadId, WorkItemBoardProjectId,
			WorkItemBoardTitle, WorkItemBoardWorkItemId, WorkItemPriority,
		};

		use crate::{
			history_pager::{HistoryCursorObservation, HistoryPageSource},
			work_items::{WorkItemCommandState, WorkItemsLoadState},
		};

		let mut shell = Self::new(
			window,
			cx,
			ConnectionView::Online { generation: 7, applied: Some(decodex_protocol::Cursor(42)) },
		);
		let conversation_id = EntityId::new("10000000-0000-4000-8000-000000000001")
			.expect("visual conversation identity is bounded");
		let second_conversation_id = EntityId::new("10000000-0000-4000-8000-000000000002")
			.expect("visual conversation identity is bounded");
		let third_conversation_id = EntityId::new("10000000-0000-4000-8000-000000000003")
			.expect("visual conversation identity is bounded");
		let runtime_session_id = EntityId::new("20000000-0000-4000-8000-000000000001")
			.expect("visual runtime identity is bounded");
		let active_turn_id = EntityId::new("30000000-0000-4000-8000-000000000001")
			.expect("visual turn identity is bounded");
		let task = |conversation_id: EntityId,
		            runtime_id: &str,
		            state: QuickTaskState,
		            active_turn_id: Option<EntityId>,
		            revision: u64| {
			QuickTaskSummary::new(
				conversation_id,
				EntityRevision(revision),
				1_786_000_000_000_000 + i64::try_from(revision).unwrap_or_default(),
				Some(EntityId::new(runtime_id).expect("visual runtime identity is bounded")),
				Some(EntityRevision(revision)),
				state,
				active_turn_id,
				None,
			)
			.expect("visual Quick Task projection is valid")
		};
		shell.quick = QuickTasksSnapshot {
			load: QuickTasksLoadState::Ready,
			command: QuickTaskCommandState::Idle,
			submission_result_generation: 0,
			last_submission_accepted: false,
			refresh: QuickTaskRefreshState::Idle,
			tasks: vec![
				task(
					conversation_id.clone(),
					runtime_session_id.as_str(),
					QuickTaskState::Running,
					Some(active_turn_id),
					14,
				),
				task(
					second_conversation_id.clone(),
					"20000000-0000-4000-8000-000000000002",
					QuickTaskState::Ready,
					None,
					8,
				),
				task(
					third_conversation_id.clone(),
					"20000000-0000-4000-8000-000000000003",
					QuickTaskState::Ready,
					None,
					5,
				),
			],
			selected: Some(conversation_id.clone()),
			live_deltas: Vec::new(),
			can_submit: true,
			execution: decodex_protocol::QuickTaskExecutionSettings::new(
				decodex_protocol::QuickTaskModel::new("gpt-5.6-sol")
					.expect("visual model identifier is valid"),
				decodex_protocol::QuickTaskReasoningEffort::High,
				false,
			),
		};

		let project_id = WorkItemBoardProjectId::new("40000000-0000-4000-8000-000000000001")
			.expect("visual project identity is valid");
		let lead_id = WorkItemBoardLeadId::new("50000000-0000-4000-8000-000000000001")
			.expect("visual lead identity is valid");
		let project = ProjectSummary::new(
			project_id.clone(),
			lead_id.clone(),
			WireText::new("acg-box/decodex").expect("visual repository identity is bounded"),
		)
		.expect("visual Project projection is valid");
		let card = |id: &str,
		            title: &str,
		            description: &str,
		            state: WorkItemState,
		            revision: u64,
		            conversation: EntityId| {
			WorkItemBoardCard::new(
				WorkItemBoardWorkItemId::new(id).expect("visual WorkItem identity is valid"),
				project_id.clone(),
				lead_id.clone(),
				None,
				Vec::new(),
				Vec::new(),
				Vec::new(),
				WorkItemBoardTitle::new(title).expect("visual title is valid"),
				WireText::new(description).expect("visual description is bounded"),
				WorkItemPriority::High,
				state,
				EntityRevision(revision),
				None,
				Some(conversation),
			)
			.expect("visual WorkItem card is valid")
		};
		shell.work = WorkItemsSnapshot {
			load: WorkItemsLoadState::Ready,
			command: WorkItemCommandState::Idle,
			projects: vec![project],
			selected_project: Some(project_id.clone()),
			cards: vec![
				card(
					"60000000-0000-4000-8000-000000000001",
					"Redesign the Codex Workbench",
					"Make conversation the primary operating surface. Keep Work Item context visible without turning the product into another issue tracker.",
					WorkItemState::Running,
					14,
					conversation_id.clone(),
				),
				card(
					"60000000-0000-4000-8000-000000000002",
					"Harden managed run recovery",
					"Preserve durable readback across app-server reconnects.",
					WorkItemState::Ready,
					8,
					second_conversation_id,
				),
				card(
					"60000000-0000-4000-8000-000000000003",
					"Review account routing",
					"Verify multi-account routing evidence before acceptance.",
					WorkItemState::Review,
					5,
					third_conversation_id,
				),
			],
			can_mutate: true,
		};
		let visual_account =
			|id: &str, alias: &str, used_five_hour: u8, used_seven_day: u8, revision: u64| {
				AccountDto {
					account_id: EntityId::new(id).expect("visual account identity is canonical"),
					alias: WireText::new(alias).expect("visual account alias is bounded"),
					enabled: true,
					account_revision: EntityRevision(revision),
					observed_state: AccountObservedStateDto::Available,
					lifecycle_readiness: AccountLifecycleReadinessDto::Ready,
					credential_binding: None,
					unsettled_operation: None,
					five_hour_quota: AccountQuotaWindowDto {
						duration_minutes: 300,
						observed_at_unix_micros: Some(1_786_000_000_000_000),
						result: AccountQuotaStateDto::Current {
							used_percent: used_five_hour,
							resets_at_unix_micros: 1_786_018_000_000_000,
						},
					},
					seven_day_quota: AccountQuotaWindowDto {
						duration_minutes: 10_080,
						observed_at_unix_micros: Some(1_786_000_000_000_000),
						result: AccountQuotaStateDto::Current {
							used_percent: used_seven_day,
							resets_at_unix_micros: 1_786_604_800_000_000,
						},
					},
				}
			};
		let primary = visual_account("70000000-0000-4000-8000-000000000001", "Primary", 64, 28, 12);
		let reserve =
			visual_account("70000000-0000-4000-8000-000000000002", "Build reserve", 18, 9, 7);
		let research =
			visual_account("70000000-0000-4000-8000-000000000003", "Research reserve", 91, 55, 4);
		shell.accounts = AccountsSnapshot {
			load: AccountsLoadState::Ready,
			command: AccountCommandState::Idle,
			accounts: vec![primary.clone(), reserve.clone(), research.clone()],
			routing: Some(AccountRoutingControlDto {
				revision: EntityRevision(6),
				mode: AccountSelectionModeDto::Fixed(primary.account_id.clone()),
				order: vec![primary.account_id, reserve.account_id, research.account_id],
			}),
			can_manage: true,
		};
		let health_checks = DoctorComponent::ALL
			.into_iter()
			.map(|component| {
				let status = match component {
					DoctorComponent::AppServerCapability(_) | DoctorComponent::BlobIntegrity =>
						DoctorStatus::Unknown(DoctorIssue::NotProbed),
					DoctorComponent::ManagedRepository =>
						DoctorStatus::Unavailable(DoctorIssue::Disabled),
					DoctorComponent::PluginReadiness => DoctorStatus::Unknown(DoctorIssue::Plugin),
					_ => DoctorStatus::Ready,
				};
				decodex_protocol::DoctorCheck::new(component, status)
			})
			.collect();
		shell.health = HealthSnapshot {
			load: HealthLoadState::Ready,
			report: Some(
				decodex_protocol::DoctorReport::new(
					decodex_protocol::ServerId::new("visual-health")
						.expect("visual health server identity is bounded"),
					decodex_protocol::CURRENT_VERSION,
					health_checks,
				)
				.expect("visual health report is complete"),
			),
			can_refresh: true,
		};

		let item = |history_item_id: &str,
		            turn_id: &str,
		            role: &str,
		            kind: &str,
		            text: &str,
		            revision: u64| {
			serde_json::from_value(serde_json::json!({
				"history_item_id": history_item_id,
				"turn_id": turn_id,
				"runtime_session_id": runtime_session_id.as_str(),
				"turn_role": role,
				"possible_side_effects": "none",
				"kind": kind,
				"status": "completed",
				"payload": {"kind": "inline", "data": {"text": text}},
				"media_type": "text/plain",
				"metadata": {},
				"revision": revision
			}))
			.expect("visual history item is valid")
		};
		let page = ConversationHistoryPage {
			items: vec![
				item(
					"history-01",
					"turn-01",
					"user",
					"message",
					"The current interface still feels like a demo. Redesign it around the actual Codex workflow and keep the ontology context useful, not decorative.",
					1,
				),
				item(
					"history-02",
					"turn-01",
					"assistant",
					"message",
					"I’ll make the conversation the primary surface, move Factory to a secondary view, and bind the inspector to the current Work Item. The UI will not invent diff data that the app-server does not provide.",
					2,
				),
				item(
					"history-03",
					"turn-01",
					"tool",
					"tool_call",
					"Inspected Shell, QuickTasks, WorkItems, and HistoryPager ownership boundaries",
					3,
				),
				item(
					"history-04",
					"turn-01",
					"assistant",
					"message",
					"The first pass is now a compact Workbench: integrated title bar, horizontal sessions, dense transcript, floating composer, and a real Work Item inspector. Factory remains available for graph-level planning and managed execution.",
					4,
				),
				item(
					"history-05",
					"turn-02",
					"user",
					"message",
					"Keep the visual language quiet and professional. The information architecture should carry the factory feeling.",
					5,
				),
				item(
					"history-06",
					"turn-02",
					"tool",
					"tool_result",
					"GPUI check passed for the redesigned shell",
					6,
				),
				item(
					"history-07",
					"turn-02",
					"assistant",
					"message",
					"I’m tightening spacing and hierarchy against the reference now, then I’ll capture the same state for a direct visual comparison.",
					7,
				),
			],
			next_cursor: None,
		};
		shell.history = Some(HistorySnapshot {
			conversation_id: Some(conversation_id.clone()),
			view_generation: 1,
			load: HistoryLoadState::Visible,
			visible: Some(page),
			visible_source: Some(HistoryPageSource::FreshServer),
			next_cursor: None,
			cursor: HistoryCursorObservation::NoContinuationObserved,
			cache_diagnostic: None,
			retained_pages: 1,
			retained_items: 7,
			retained_bytes: 1_836,
			can_show_previous: false,
			can_show_next: false,
			can_retry: false,
			last_stale_cancellation: None,
		});
		let programs = Programs::visual_closed_cycle();
		shell.program = programs.snapshot();
		shell.programs = programs.clone();
		shell.factory.update(cx, |factory, cx| factory.bind_programs(programs, cx));
		shell.opened_history = Some(conversation_id);
		shell.creating_new = false;
		shell
	}

	#[cfg(feature = "visual-capture")]
	#[allow(
		dead_code,
		reason = "the production binary shares this feature with the dedicated visual-capture binary"
	)]
	pub(crate) fn visual_destination(
		destination: Destination,
		left_sidebar_visible: bool,
		inspector_visible: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let mut shell = Self::visual_workbench(window, cx);
		shell.selected = destination;
		shell.left_sidebar_visible = left_sidebar_visible;
		shell.left_sidebar_mounted = left_sidebar_visible;
		shell.inspector_visible = inspector_visible;
		shell.inspector_mounted = inspector_visible;
		shell
	}

	fn handle_factory_event(&mut self, event: &FactoryEvent, cx: &mut Context<Self>) {
		match event {
			FactoryEvent::OpenRoute(route) => {
				let destination = match route {
					FactoryRoute::QuickTasks => Destination::QuickTasks,
					FactoryRoute::Accounts => Destination::Accounts,
					FactoryRoute::Health => Destination::Health,
					FactoryRoute::Settings => Destination::Settings,
				};
				self.select_destination(destination, cx);
			},
			FactoryEvent::StartCodexConversation { context, message } => {
				self.quick_tasks.begin_new();
				self.pending_submission = None;
				self.deferred_provider_refresh = None;
				self.creating_new = true;
				self.opened_history = None;
				let prompt = format!("Decodex factory context: {context}\n\n{message}");
				let result_generation = self.quick_tasks.snapshot().submission_result_generation;
				match self.quick_tasks.create(&prompt) {
					Ok(submission) => {
						self.pending_submission = Some(PendingComposerSubmission {
							content: prompt,
							result_generation,
							conversation_id: submission.conversation_id,
							turn_id: submission.turn_id,
							accepted: false,
						});
						self.input_status = None;
					},
					Err(error) => self.input_status = Some(input_error_label(error).into()),
				}
				self.synchronize_quick_tasks();
				self.select_destination(Destination::QuickTasks, cx);
			},
			FactoryEvent::StartProgramWorkItem { work_item_id, message, working_directory } => {
				self.quick_tasks.begin_new();
				self.pending_submission = None;
				self.deferred_provider_refresh = None;
				self.creating_new = true;
				self.opened_history = None;
				let prompt =
					format!("Decodex Program WorkItem {}\n\n{}", work_item_id.as_str(), message);
				let result_generation = self.quick_tasks.snapshot().submission_result_generation;
				match self.quick_tasks.create_for_program_work_item(
					&prompt,
					work_item_id.clone(),
					working_directory.clone(),
				) {
					Ok(submission) => {
						self.programs.expect_execution(submission.conversation_id.clone());
						self.pending_submission = Some(PendingComposerSubmission {
							content: prompt,
							result_generation,
							conversation_id: submission.conversation_id,
							turn_id: submission.turn_id,
							accepted: false,
						});
						self.input_status = None;
					},
					Err(error) => self.input_status = Some(input_error_label(error).into()),
				}
				self.synchronize_quick_tasks();
				self.select_destination(Destination::QuickTasks, cx);
			},
			FactoryEvent::OpenWorkItemConversation { conversation_id } => {
				self.quick_tasks.select_when_available(conversation_id.clone());
				self.deferred_provider_refresh = Some(conversation_id.clone());
				self.creating_new = false;
				self.opened_history = None;
				self.select_destination(Destination::QuickTasks, cx);
				self.synchronize_quick_tasks();
			},
		}
	}

	fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
		window.focus_next(cx);
		cx.stop_propagation();
	}

	fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
		window.focus_prev(cx);
		cx.stop_propagation();
	}

	fn activate_destination(
		&mut self,
		_: &ActivateDestination,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if let Some(index) =
			self.destination_focus.iter().position(|handle| handle.is_focused(window))
		{
			self.select_destination(Destination::ALL[index], cx);
		}
	}

	fn select_destination(&mut self, destination: Destination, cx: &mut Context<Self>) {
		if self.selected == destination {
			return;
		}
		if self.selected == Destination::Health {
			self.health_query.deactivate();
		}
		if self.selected == Destination::QuickTasks {
			self.quick_tasks.deactivate();
		}
		if self.selected == Destination::Factory {
			self.programs.deactivate();
		}
		if self.selected == Destination::Accounts {
			self.accounts_controller.deactivate();
		}

		self.selected = destination;
		if destination == Destination::Health {
			self.health_query.activate();
		}
		if destination == Destination::QuickTasks {
			self.quick_tasks.activate();
		}
		if destination == Destination::Factory {
			self.programs.activate();
			self.synchronize_programs(cx);
		}
		if destination == Destination::Accounts {
			self.accounts_controller.activate();
			self.synchronize_accounts();
		}
		if destination == Destination::Settings {
			self.settings.update(cx, SettingsSurface::refresh);
		}
		self.health = self.health_query.snapshot();
		cx.notify();
	}

	fn activate_factory(&mut self, _: &ActivateFactory, _: &mut Window, cx: &mut Context<Self>) {
		self.select_destination(Destination::Factory, cx);
		cx.stop_propagation();
	}

	fn activate_quick_tasks(
		&mut self,
		_: &ActivateQuickTasks,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.select_destination(Destination::QuickTasks, cx);
		cx.stop_propagation();
	}

	fn activate_health(&mut self, _: &ActivateHealth, _: &mut Window, cx: &mut Context<Self>) {
		self.select_destination(Destination::Health, cx);
		cx.stop_propagation();
	}

	fn set_left_sidebar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
		if self.left_sidebar_visible == visible {
			return;
		}
		self.left_sidebar_visible = visible;
		self.left_sidebar_mounted = true;
		self.left_sidebar_motion_generation = self.left_sidebar_motion_generation.wrapping_add(1);
		let generation = self.left_sidebar_motion_generation;
		if !visible {
			cx.spawn(async move |shell, cx| {
				cx.background_executor()
					.timer(ui_theme::MOTION_PANEL + Duration::from_millis(24))
					.await;
				let _ = shell.update(cx, |shell, cx| {
					if !shell.left_sidebar_visible
						&& shell.left_sidebar_motion_generation == generation
					{
						shell.left_sidebar_mounted = false;
						cx.notify();
					}
				});
			})
			.detach();
		}
		cx.notify();
	}

	fn set_inspector_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
		if self.inspector_visible == visible {
			return;
		}
		self.inspector_visible = visible;
		self.inspector_mounted = true;
		self.inspector_motion_generation = self.inspector_motion_generation.wrapping_add(1);
		let generation = self.inspector_motion_generation;
		if !visible {
			cx.spawn(async move |shell, cx| {
				cx.background_executor()
					.timer(ui_theme::MOTION_PANEL + Duration::from_millis(24))
					.await;
				let _ = shell.update(cx, |shell, cx| {
					if !shell.inspector_visible && shell.inspector_motion_generation == generation {
						shell.inspector_mounted = false;
						cx.notify();
					}
				});
			})
			.detach();
		}
		cx.notify();
	}

	fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
		if self.selected == Destination::QuickTasks {
			self.set_left_sidebar_visible(!self.left_sidebar_visible, cx);
		}
		cx.stop_propagation();
	}

	fn toggle_inspector(&mut self, _: &ToggleInspector, _: &mut Window, cx: &mut Context<Self>) {
		if self.selected == Destination::QuickTasks {
			self.set_inspector_visible(!self.inspector_visible, cx);
		}
		cx.stop_propagation();
	}

	fn refresh_health(&mut self, _: &RefreshHealth, _: &mut Window, cx: &mut Context<Self>) {
		self.request_health_refresh(cx);
	}

	fn request_health_refresh(&mut self, cx: &mut Context<Self>) {
		if self.health_query.refresh() {
			self.health = self.health_query.snapshot();
			cx.notify();
		}
	}

	fn bind_health_query(&mut self, health_query: HealthQuery, cx: &mut Context<Self>) {
		self.health_query = health_query;
		if self.selected == Destination::Health {
			self.health_query.activate();
		}
		self.health = self.health_query.snapshot();
		cx.notify();
	}

	fn bind_quick_tasks(
		&mut self,
		quick_tasks: QuickTasks,
		history_pager: HistoryPager,
		cx: &mut Context<Self>,
	) {
		self.quick_tasks.deactivate();
		self.quick_tasks = quick_tasks;
		self.history_pager = Some(history_pager);
		if self.selected == Destination::QuickTasks {
			self.quick_tasks.activate();
		}
		self.synchronize_quick_tasks();
		self.reconcile_pending_submission(cx);
		cx.notify();
	}

	fn bind_work_items(&mut self, work_items: WorkItems, cx: &mut Context<Self>) {
		self.work_items.deactivate();
		self.work_items = work_items;
		self.factory.update(cx, |factory, cx| factory.bind_work_items(self.work_items.clone(), cx));
		self.synchronize_work_items(cx);
	}

	fn bind_programs(&mut self, programs: Programs, cx: &mut Context<Self>) {
		self.programs.deactivate();
		self.programs = programs;
		if self.selected == Destination::Factory {
			self.programs.activate();
		}
		self.factory.update(cx, |factory, cx| factory.bind_programs(self.programs.clone(), cx));
		self.synchronize_programs(cx);
	}

	fn bind_accounts(&mut self, accounts: AccountsController, cx: &mut Context<Self>) {
		self.accounts_controller.deactivate();
		self.accounts_controller = accounts;
		if self.selected == Destination::Accounts {
			self.accounts_controller.activate();
		}
		self.synchronize_accounts();
		cx.notify();
	}

	fn synchronize_accounts(&mut self) {
		self.accounts = self.accounts_controller.snapshot();
	}

	fn refresh_accounts(&mut self, cx: &mut Context<Self>) {
		if self.accounts_controller.refresh() {
			self.account_status = None;
			self.synchronize_accounts();
			cx.notify();
		}
	}

	fn set_account_enabled(
		&mut self,
		account_id: &EntityId,
		enabled: bool,
		cx: &mut Context<Self>,
	) {
		self.account_status = self
			.accounts_controller
			.set_enabled(account_id, enabled)
			.err()
			.map(account_input_error_label)
			.map(Into::into);
		self.synchronize_accounts();
		cx.notify();
	}

	fn select_fixed_account(&mut self, account_id: &EntityId, cx: &mut Context<Self>) {
		self.account_status = self
			.accounts_controller
			.select_fixed(account_id)
			.err()
			.map(account_input_error_label)
			.map(Into::into);
		self.synchronize_accounts();
		cx.notify();
	}

	fn select_balanced_accounts(&mut self, cx: &mut Context<Self>) {
		self.account_status = self
			.accounts_controller
			.select_balanced()
			.err()
			.map(account_input_error_label)
			.map(Into::into);
		self.synchronize_accounts();
		cx.notify();
	}

	fn synchronize_work_items(&mut self, cx: &mut Context<Self>) {
		self.work = self.work_items.snapshot();
		self.factory.update(cx, FactorySurface::synchronize_work_items);
		if let Some(conversation) = self.work_items.take_started_conversation() {
			self.quick_tasks.adopt_and_select(conversation);
			self.creating_new = false;
			self.opened_history = None;
			self.select_destination(Destination::QuickTasks, cx);
			self.synchronize_quick_tasks();
		}
	}

	fn synchronize_programs(&mut self, cx: &mut Context<Self>) {
		self.program = self.programs.snapshot();
		self.factory.update(cx, FactorySurface::synchronize_programs);
		cx.notify();
	}

	fn synchronize_quick_tasks(&mut self) {
		let snapshot = self.quick_tasks.snapshot();
		let selected = snapshot.selected.clone();
		if selected.is_none()
			&& self.opened_history.is_some()
			&& let Some(pager) = self.history_pager.as_ref()
		{
			pager.cancel();
			self.opened_history = None;
		}
		let should_open = selected
			.as_ref()
			.is_some_and(|conversation_id| self.opened_history.as_ref() != Some(conversation_id));
		if should_open
			&& let (Some(pager), Some(conversation_id)) =
				(self.history_pager.as_ref(), selected.clone())
			&& pager.open(conversation_id.clone()).is_ok()
		{
			self.opened_history = Some(conversation_id);
		}
		if self.creating_new && snapshot.selected.is_some() {
			self.creating_new = false;
		}
		self.quick = snapshot;
		self.history = self.history_pager.as_ref().map(HistoryPager::snapshot);
		if deferred_provider_refresh_ready(
			self.deferred_provider_refresh.as_ref(),
			self.quick.selected.as_ref(),
			self.history.as_ref(),
		) {
			self.deferred_provider_refresh = None;
			let _ = self.quick_tasks.refresh_selected();
			self.quick = self.quick_tasks.snapshot();
		}
	}

	fn reconcile_pending_submission(&mut self, cx: &mut Context<Self>) {
		let Some(pending) = self.pending_submission.as_ref() else {
			return;
		};
		if pending.accepted && pending_submission_is_persisted(pending, self.history.as_ref()) {
			self.pending_submission = None;
			return;
		}
		let Some(clear) = pending_submission_clear_decision(
			pending,
			self.quick.submission_result_generation,
			self.quick.last_submission_accepted,
			self.composer.read(cx).content(),
		) else {
			return;
		};
		if !self.quick.last_submission_accepted {
			self.pending_submission = None;
			return;
		}
		if let Some(pending) = self.pending_submission.as_mut() {
			pending.accepted = true;
		}
		if clear {
			self.composer.update(cx, |composer, cx| composer.clear(cx));
		}
		if self
			.pending_submission
			.as_ref()
			.is_some_and(|pending| pending_submission_is_persisted(pending, self.history.as_ref()))
		{
			self.pending_submission = None;
		}
	}

	fn start_new_quick_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.quick_tasks.begin_new();
		self.deferred_provider_refresh = None;
		if let Some(pager) = self.history_pager.as_ref() {
			pager.cancel();
		}
		self.opened_history = None;
		self.creating_new = true;
		self.input_status = None;
		self.synchronize_quick_tasks();
		window.focus(&self.composer.focus_handle(cx), cx);
		cx.notify();
	}

	fn choose_quick_task(
		&mut self,
		conversation_id: EntityId,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if self.quick_tasks.select(conversation_id.clone()) {
			// The same retained connection serializes provider commands before later queries.
			// Show daemon-owned SQLite history first, then reconcile the provider in background.
			self.deferred_provider_refresh = Some(conversation_id);
			self.creating_new = false;
			self.opened_history = None;
			self.synchronize_quick_tasks();
			window.focus(&self.composer.focus_handle(cx), cx);
			cx.notify();
		}
	}

	fn submit_composer(&mut self, _: &SubmitComposer, window: &mut Window, cx: &mut Context<Self>) {
		self.submit_quick_task(window, cx);
		cx.stop_propagation();
	}

	fn submit_quick_task(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		let creating = self.creating_new || self.quick.selected.is_none();
		let message = self.composer.read(cx).content().to_owned();
		// Read the controller's current terminal-result fence before queueing. The rendered
		// Shell snapshot can be one publication behind a just-settled prior submission.
		let result_generation = self.quick_tasks.snapshot().submission_result_generation;
		let result = if creating {
			self.quick_tasks.create(&message)
		} else {
			self.quick_tasks.submit(&message)
		};
		match result {
			Ok(QueuedQuickTaskSubmission { conversation_id, turn_id }) => {
				self.pending_submission = Some(PendingComposerSubmission {
					content: message,
					result_generation,
					conversation_id,
					turn_id,
					accepted: false,
				});
				self.creating_new = creating;
				self.input_status = None;
			},
			Err(error) => self.input_status = Some(input_error_label(error).into()),
		}
		self.synchronize_quick_tasks();
		self.reconcile_pending_submission(cx);
		cx.notify();
	}

	fn recover_quick_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let state = self.quick.selected_task().map(|task| task.state);
		let action = self.quick.selected_task().and_then(|task| task.recovery_action);
		if state == Some(QuickTaskState::OutcomeUnknown) {
			self.input_status =
				self.quick_tasks.refresh_selected().err().map(input_error_label).map(Into::into);
			self.synchronize_quick_tasks();
			cx.notify();
			return;
		}
		if action == Some(QuickTaskRecoveryAction::StartNewConversation) {
			self.start_new_quick_task(window, cx);
			return;
		}
		self.input_status =
			self.quick_tasks.recover_selected().err().map(input_error_label).map(Into::into);
		self.synchronize_quick_tasks();
		cx.notify();
	}

	fn interrupt_quick_task(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		if let Err(error) = self.quick_tasks.interrupt() {
			self.input_status = Some(input_error_label(error).into());
		}
		self.synchronize_quick_tasks();
		cx.notify();
	}

	fn refresh_quick_task(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		self.input_status =
			self.quick_tasks.refresh_all().err().map(input_error_label).map(Into::into);
		self.synchronize_quick_tasks();
		cx.notify();
	}

	fn archive_quick_task(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		self.input_status =
			self.quick_tasks.archive_selected().err().map(input_error_label).map(Into::into);
		self.synchronize_quick_tasks();
		cx.notify();
	}

	fn cycle_quick_task_model(&mut self, cx: &mut Context<Self>) {
		self.quick_tasks.cycle_model();
		self.synchronize_quick_tasks();
		cx.notify();
	}

	fn cycle_quick_task_effort(&mut self, cx: &mut Context<Self>) {
		self.quick_tasks.cycle_reasoning_effort();
		self.synchronize_quick_tasks();
		cx.notify();
	}

	fn toggle_quick_task_fast(&mut self, cx: &mut Context<Self>) {
		self.quick_tasks.toggle_fast();
		self.synchronize_quick_tasks();
		cx.notify();
	}

	fn show_previous_history(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		if let Some(pager) = self.history_pager.as_ref() {
			let _ = pager.show_previous();
		}
		self.synchronize_quick_tasks();
		cx.notify();
	}

	fn show_next_history(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		if let Some(pager) = self.history_pager.as_ref() {
			let _ = pager.show_next();
		}
		self.synchronize_quick_tasks();
		cx.notify();
	}

	fn retry_history(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		if let Some(pager) = self.history_pager.as_ref() {
			let _ = pager.retry();
		}
		self.synchronize_quick_tasks();
		cx.notify();
	}
}

const fn input_error_label(error: QuickTaskInputError) -> &'static str {
	match error {
		QuickTaskInputError::Offline => "Quick Tasks are offline.",
		QuickTaskInputError::Busy => "Wait for the current command result.",
		QuickTaskInputError::InvalidMessage => "Enter a message within the supported limit.",
		QuickTaskInputError::NoSelection => "Select a Quick Task first.",
		QuickTaskInputError::NotReady => "The selected Quick Task is not ready for this command.",
		QuickTaskInputError::NotInterruptible => "The selected turn is not running.",
		QuickTaskInputError::IdentityUnavailable => "A command identity could not be created.",
		QuickTaskInputError::WorkingDirectoryUnavailable =>
			"The local Quick Task working directory is unavailable.",
	}
}

struct LifecycleOwnerGlobal {
	_owner: Entity<LifecycleOwner>,
}

impl Global for LifecycleOwnerGlobal {}

pub(crate) struct LifecycleOwner {
	cancellation: LifecycleCancellation,
	task: Option<Task<()>>,
	last_view: ConnectionView,
	running: bool,
	#[cfg(test)]
	observed_views: Vec<ConnectionView>,
	_subscriptions: Vec<Subscription>,
}

impl LifecycleOwner {
	fn new<R: 'static>(
		cancellation: LifecycleCancellation,
		views: Receiver<ConnectionView>,
		background: Task<R>,
		shell: WeakEntity<Shell>,
		initial_view: ConnectionView,
		cx: &mut Context<Self>,
	) -> Self {
		let task = cx.spawn(async move |owner, cx| {
			let background = background;
			loop {
				publish_views(&owner, &shell, &views, cx);
				if background.is_ready() {
					let _ = background.await;
					publish_views(&owner, &shell, &views, cx);
					let _ = owner.update(cx, |owner, _| owner.running = false);

					return;
				}
				cx.background_executor().timer(LIFECYCLE_POLL).await;
			}
		});

		Self {
			cancellation,
			task: Some(task),
			last_view: initial_view,
			running: true,
			#[cfg(test)]
			observed_views: vec![initial_view],
			_subscriptions: vec![cx.on_app_quit(|owner, _| owner.shutdown())],
		}
	}

	fn shutdown(&mut self) -> Pin<Box<dyn Future<Output = ()> + 'static>> {
		self.cancellation.cancel();
		let task = self.task.take();

		Box::pin(async move {
			if let Some(task) = task {
				task.await;
			}
		})
	}

	#[cfg(test)]
	pub(crate) fn last_view(&self) -> ConnectionView {
		self.last_view
	}

	#[cfg(test)]
	pub(crate) fn is_running(&self) -> bool {
		self.running
	}

	#[cfg(test)]
	pub(crate) fn observed_views(&self) -> &[ConnectionView] {
		&self.observed_views
	}
}

pub(crate) fn retain_lifecycle(
	window: WindowHandle<Shell>,
	mut lifecycle: ClientLifecycle,
	cx: &mut App,
) {
	let cancellation = lifecycle.cancellation();
	let views = lifecycle.observe_views();
	let initial_view = lifecycle.view();
	let shell = window.entity(cx).expect("the production shell window remains open");
	let accounts = lifecycle.accounts();
	let health_query = lifecycle.health_query();
	let quick_tasks = lifecycle.quick_tasks();
	let programs = lifecycle.programs();
	let work_items = lifecycle.work_items();
	let history_pager = lifecycle.history_pager();
	shell.update(cx, |shell, cx| {
		shell.bind_accounts(accounts, cx);
		shell.bind_health_query(health_query, cx);
		shell.bind_quick_tasks(quick_tasks, history_pager, cx);
		shell.bind_programs(programs, cx);
		shell.bind_work_items(work_items, cx);
	});
	let shell = shell.downgrade();
	let background = cx.background_executor().spawn(async move {
		let runtime = tokio::runtime::Builder::new_current_thread()
			.enable_all()
			.build()
			.expect("build the bounded client runtime");

		runtime.block_on(lifecycle.run())
	});
	retain_lifecycle_task(
		window.window_id(),
		shell,
		cancellation,
		views,
		initial_view,
		background,
		cx,
	);
}

pub(crate) fn retain_lifecycle_task<R: 'static>(
	window_id: WindowId,
	shell: WeakEntity<Shell>,
	cancellation: LifecycleCancellation,
	views: Receiver<ConnectionView>,
	initial_view: ConnectionView,
	background: Task<R>,
	cx: &mut App,
) -> Entity<LifecycleOwner> {
	debug_assert!(
		!cx.has_global::<LifecycleOwnerGlobal>(),
		"the application retains exactly one lifecycle owner"
	);
	let owner =
		cx.new(|cx| LifecycleOwner::new(cancellation, views, background, shell, initial_view, cx));
	let weak_owner = owner.downgrade();
	let close_subscription = cx.on_window_closed(move |cx, closed_id| {
		if closed_id == window_id {
			let _ = weak_owner.update(cx, |owner, _| owner.cancellation.cancel());
		}
	});
	owner.update(cx, |owner, _| owner._subscriptions.push(close_subscription));
	cx.set_global(LifecycleOwnerGlobal { _owner: owner.clone() });

	owner
}

fn publish_views(
	owner: &WeakEntity<LifecycleOwner>,
	shell: &WeakEntity<Shell>,
	views: &Receiver<ConnectionView>,
	cx: &mut gpui::AsyncApp,
) {
	while let Ok(view) = views.try_recv() {
		let _ = owner.update(cx, |owner, _| {
			owner.last_view = view;
			#[cfg(test)]
			owner.observed_views.push(view);
		});
		let _ = shell.update(cx, |shell, cx| {
			shell.connection = view;
			cx.notify();
		});
	}
	let _ = shell.update(cx, |shell, cx| {
		let accounts = shell.accounts_controller.snapshot();
		let health = shell.health_query.snapshot();
		let quick = shell.quick_tasks.snapshot();
		let program = shell.programs.snapshot();
		let work = shell.work_items.snapshot();
		let history = shell.history_pager.as_ref().map(HistoryPager::snapshot);

		if accounts != shell.accounts {
			shell.accounts = accounts;
			cx.notify();
		}
		if health != shell.health {
			shell.health = health;
			cx.notify();
		}
		if quick != shell.quick || history != shell.history {
			shell.synchronize_quick_tasks();
			shell.reconcile_pending_submission(cx);
			cx.notify();
		}
		if program != shell.program {
			shell.synchronize_programs(cx);
		}
		if work != shell.work {
			shell.synchronize_work_items(cx);
			cx.notify();
		}
	});
}

fn topbar_destination_tab(
	index: usize,
	destination: Destination,
	is_selected: bool,
	focus: FocusHandle,
	window: &Window,
	cx: &Context<Shell>,
) -> AnyElement {
	let display_label =
		if destination == Destination::QuickTasks { "Workbench" } else { destination.label() };
	div()
		.id(("destination", index))
		.role(Role::Tab)
		.aria_label(format!("{display_label}: {}", destination.description()))
		.aria_selected(is_selected)
		.key_context("Destination")
		.track_focus(&focus)
		.on_action(cx.listener(Shell::focus_next))
		.on_action(cx.listener(Shell::focus_previous))
		.on_action(cx.listener(Shell::activate_destination))
		.h(px(27.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(7.0))
		.text_size(px(9.0))
		.font_weight(if is_selected { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
		.text_color(if is_selected { rgb(WB_TEXT) } else { rgb(WB_TEXT_MUTED) })
		.bg(if is_selected { rgba(0xffffff0f) } else { rgba(0x00000000) })
		.border_1()
		.border_color(if focus.is_focused(window) {
			rgb(WB_BLUE)
		} else if is_selected {
			rgba(0xffffff18)
		} else {
			rgba(0x00000000)
		})
		.on_click(cx.listener(move |shell, _, _, cx| {
			shell.select_destination(destination, cx);
		}))
		.occlude()
		.cursor_pointer()
		.on_mouse_down(MouseButton::Left, |_, window, cx| {
			window.prevent_default();
			cx.stop_propagation();
		})
		.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
		.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
		.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
		.child(display_label)
		.into_any_element()
}

fn work_item_state_color(state: WorkItemState) -> u32 {
	match state {
		WorkItemState::Ready => WB_BLUE,
		WorkItemState::Running => WB_GREEN,
		WorkItemState::Review => WB_ACCENT,
		WorkItemState::Blocked | WorkItemState::Canceled => WB_AMBER,
		WorkItemState::Done => WB_GREEN,
		WorkItemState::Inbox | WorkItemState::Planned => WB_TEXT_FAINT,
	}
}

fn compact_identity(value: &str) -> String {
	let prefix = value.chars().take(8).collect::<String>();
	if value.chars().count() > 8 { format!("{prefix}…") } else { prefix }
}

fn workbench_topbar(
	shell: &Shell,
	presentation: &ConnectionPresentation,
	window: &Window,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let selected_card = shell
		.quick
		.selected
		.as_ref()
		.and_then(|conversation_id| bound_work_item(shell, conversation_id));
	let title = if shell.selected == Destination::QuickTasks {
		selected_card
			.map(|card| card.title().as_str().to_owned())
			.unwrap_or_else(|| "Codex Workbench".to_owned())
	} else {
		shell.selected.label().to_owned()
	};
	let workspace = selected_card
		.and_then(|card| {
			shell.work.projects.iter().find(|project| project.project_id() == card.project_id())
		})
		.map(|project| project.repository_identity().as_str().to_owned())
		.unwrap_or_else(|| "local workspace".to_owned());
	let connection_color = presentation.color;
	let connection_label = presentation.label;
	let left_sidebar_visible = shell.left_sidebar_visible;
	let inspector_visible = shell.inspector_visible;
	let page_tabs = Destination::CHROME.into_iter().map(|destination| {
		let index = Destination::ALL
			.iter()
			.position(|candidate| *candidate == destination)
			.expect("topbar destination is part of the complete destination set");
		topbar_destination_tab(
			index,
			destination,
			shell.selected == destination,
			shell.destination_focus[index].clone(),
			window,
			cx,
		)
	});
	let settings_index = Destination::ALL
		.iter()
		.position(|destination| *destination == Destination::Settings)
		.expect("Settings is part of the complete destination set");

	div()
		.id("workbench-topbar")
		.role(Role::Navigation)
		.aria_label("Workbench navigation")
		.h(px(WORKBENCH_TOPBAR_HEIGHT))
		.min_h(px(WORKBENCH_TOPBAR_HEIGHT))
		.w_full()
		.pl(px(78.0))
		.pr_3()
		.flex()
		.items_center()
		.border_b_1()
		.border_color(rgba(0xffffff0d))
		.bg(rgba(ui_theme::TOPBAR_MATERIAL))
		.window_control_area(WindowControlArea::Drag)
		.on_mouse_down(
			MouseButton::Left,
			cx.listener(|shell, _, _, _| shell.titlebar_drag_pending = true),
		)
		.on_mouse_up(
			MouseButton::Left,
			cx.listener(|shell, _, _, _| shell.titlebar_drag_pending = false),
		)
		.on_mouse_move(cx.listener(|shell, _, window, _| {
			if shell.titlebar_drag_pending {
				shell.titlebar_drag_pending = false;
				window.start_window_move();
			}
		}))
		.on_click(|event, window, _| {
			if event.click_count() == 2 {
				window.titlebar_double_click();
			}
		})
		.child(
			div()
				.h_full()
				.w(px(350.0))
				.min_w(px(250.0))
				.flex()
				.items_center()
				.gap_2()
				.child(img(app_icon_path()).size(px(20.0)).rounded(px(5.0)))
				.child(
					div()
						.min_w_0()
						.flex()
						.items_center()
						.gap_2()
						.text_size(px(10.5))
						.child(
							div()
								.font_weight(FontWeight::SEMIBOLD)
								.text_color(rgb(WB_TEXT))
								.child("Decodex"),
						)
						.child(div().text_color(rgb(WB_TEXT_FAINT)).child("/"))
						.child(
							div()
								.min_w_0()
								.overflow_hidden()
								.whitespace_nowrap()
								.text_ellipsis()
								.text_color(rgb(WB_TEXT_MUTED))
								.child(title),
						),
				)
				.child(
					div()
						.h(px(18.0))
						.max_w(px(138.0))
						.px_2()
						.flex()
						.items_center()
						.rounded(px(5.0))
						.bg(rgba(0xffffff08))
						.font_family("SF Mono")
						.text_size(px(8.0))
						.text_color(rgb(WB_TEXT_FAINT))
						.overflow_hidden()
						.whitespace_nowrap()
						.text_ellipsis()
						.child(workspace),
				),
		)
		.child(
			div()
				.id("product-destinations")
				.role(Role::TabList)
				.aria_label("Product destinations")
				.flex_1()
				.min_w_0()
				.h_full()
				.flex()
				.items_center()
				.justify_center()
				.gap_1()
				.children(page_tabs),
		)
		.child(
			div()
				.w(px(400.0))
				.min_w(px(370.0))
				.h_full()
				.flex()
				.items_center()
				.justify_end()
				.gap_2()
				.text_size(px(9.0))
				.when(shell.selected == Destination::QuickTasks, |controls| {
					controls.child(
						div()
							.id("toggle-left-sidebar")
							.role(Role::Button)
							.aria_label("Toggle conversation sidebar")
							.aria_expanded(left_sidebar_visible)
							.tooltip(|_, cx| {
								cx.new(|_| ControlTooltip("Toggle sessions · Command-B")).into()
							})
							.h(px(27.0))
							.px_3()
							.flex()
							.items_center()
							.rounded(px(7.0))
							.border_1()
							.border_color(if left_sidebar_visible {
								rgba(0xffffff20)
							} else {
								rgba(0xffffff10)
							})
							.bg(if left_sidebar_visible {
								rgba(0xffffff10)
							} else {
								rgba(0x00000000)
							})
							.text_color(if left_sidebar_visible {
								rgb(WB_TEXT)
							} else {
								rgb(WB_TEXT_MUTED)
							})
							.cursor_pointer()
							.occlude()
							.on_mouse_down(MouseButton::Left, |_, window, cx| {
								window.prevent_default();
								cx.stop_propagation();
							})
							.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(WB_TEXT)))
							.active(|element| element.bg(rgba(0xffffff1c)).opacity(0.82))
							.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
							.on_click(cx.listener(|shell, _, _, cx| {
								shell.set_left_sidebar_visible(!shell.left_sidebar_visible, cx);
							}))
							.child("Sessions"),
					)
				})
				.when(shell.selected == Destination::QuickTasks, |controls| {
					controls.child(
						div()
							.id("toggle-inspector")
							.role(Role::Button)
							.aria_label("Toggle conversation context")
							.aria_expanded(inspector_visible)
							.tooltip(|_, cx| {
								cx.new(|_| ControlTooltip("Toggle context · Command-Shift-B"))
									.into()
							})
							.h(px(27.0))
							.px_3()
							.flex()
							.items_center()
							.rounded(px(7.0))
							.border_1()
							.border_color(if inspector_visible {
								rgba(0xffffff20)
							} else {
								rgba(0xffffff10)
							})
							.bg(if inspector_visible { rgba(0xffffff10) } else { rgba(0x00000000) })
							.text_color(if inspector_visible {
								rgb(WB_TEXT)
							} else {
								rgb(WB_TEXT_MUTED)
							})
							.cursor_pointer()
							.occlude()
							.on_mouse_down(MouseButton::Left, |_, window, cx| {
								window.prevent_default();
								cx.stop_propagation();
							})
							.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(WB_TEXT)))
							.active(|element| element.bg(rgba(0xffffff1c)).opacity(0.82))
							.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
							.on_click(cx.listener(|shell, _, _, cx| {
								shell.set_inspector_visible(!shell.inspector_visible, cx);
							}))
							.child("Context"),
					)
				})
				.child(
					div()
						.id("workbench-connection-status")
						.role(Role::Status)
						.aria_label(format!("Connection: {connection_label}"))
						.flex()
						.items_center()
						.gap_2()
						.text_color(rgb(WB_TEXT_MUTED))
						.child(div().size(px(5.0)).rounded_full().bg(rgb(connection_color)))
						.child(connection_label),
				)
				.child(
					div()
						.id("open-settings")
						.role(Role::Button)
						.aria_label("Open settings")
						.key_context("Destination")
						.track_focus(&shell.destination_focus[settings_index])
						.on_action(cx.listener(Shell::focus_next))
						.on_action(cx.listener(Shell::focus_previous))
						.on_action(cx.listener(Shell::activate_destination))
						.h(px(26.0))
						.px_2()
						.flex()
						.items_center()
						.rounded(px(7.0))
						.border_1()
						.border_color(if shell.selected == Destination::Settings {
							rgba(0xffffff20)
						} else {
							rgba(0x00000000)
						})
						.bg(if shell.selected == Destination::Settings {
							rgba(0xffffff10)
						} else {
							rgba(0x00000000)
						})
						.text_color(if shell.selected == Destination::Settings {
							rgb(WB_TEXT)
						} else {
							rgb(WB_TEXT_MUTED)
						})
						.occlude()
						.cursor_pointer()
						.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
						.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
						.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
						.on_click(cx.listener(|shell, _, _, cx| {
							shell.select_destination(Destination::Settings, cx);
						}))
						.child("Settings"),
				),
		)
		.into_any_element()
}

struct RefreshTooltip;

impl Render for RefreshTooltip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
			.px_2()
			.py_1()
			.rounded(px(6.0))
			.border_1()
			.border_color(rgba(0xffffff14))
			.bg(rgba(ui_theme::SURFACE_OVERLAY_MATERIAL))
			.text_size(px(9.0))
			.text_color(rgb(WB_TEXT))
			.child("Refresh health")
	}
}

struct ControlTooltip(&'static str);

impl Render for ControlTooltip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
			.px_2()
			.py_1()
			.rounded(px(6.0))
			.border_1()
			.border_color(rgba(0xffffff14))
			.bg(rgba(ui_theme::SURFACE_OVERLAY_MATERIAL))
			.text_size(px(9.0))
			.text_color(rgb(WB_TEXT))
			.child(self.0)
	}
}

#[derive(Clone, Copy)]
struct HealthPresentation {
	label: &'static str,
	detail: &'static str,
	color: u32,
}

fn health_presentation(snapshot: &HealthSnapshot) -> HealthPresentation {
	match snapshot.load {
		HealthLoadState::NeverRequested => HealthPresentation {
			label: "Not requested",
			detail: "No health report is available.",
			color: 0x64748b,
		},
		HealthLoadState::Loading => HealthPresentation {
			label: "Loading",
			detail: if snapshot.report.is_some() {
				"Refreshing the retained report."
			} else {
				"Requesting the current report."
			},
			color: 0x60a5fa,
		},
		HealthLoadState::Ready => {
			let core_statuses = HEALTH_CORE_COMPONENTS.map(|component| {
				snapshot
					.report
					.as_ref()
					.and_then(|report| report.check(component))
					.map(|check| check.status)
			});
			if core_statuses.iter().all(|status| *status == Some(DoctorStatus::Ready)) {
				HealthPresentation {
					label: "Core ready",
					detail: "All required Decodex services are ready.",
					color: 0x22c55e,
				}
			} else if core_statuses
				.iter()
				.any(|status| matches!(status, Some(DoctorStatus::Unavailable(_))))
			{
				HealthPresentation {
					label: "Core unavailable",
					detail: "At least one required Decodex service is unavailable.",
					color: 0xef4444,
				}
			} else {
				HealthPresentation {
					label: "Core not verified",
					detail: "At least one required Decodex service has not been verified.",
					color: 0xf59e0b,
				}
			}
		},
		HealthLoadState::Offline => HealthPresentation {
			label: "Offline",
			detail: "Health is unavailable while the daemon is offline.",
			color: 0xf97316,
		},
		HealthLoadState::Stale => HealthPresentation {
			label: "Stale",
			detail: "The retained report belongs to an earlier connection.",
			color: 0xf59e0b,
		},
		HealthLoadState::Refused => HealthPresentation {
			label: "Response refused",
			detail: "The retained report was not replaced.",
			color: 0xef4444,
		},
	}
}

fn component_label(component: DoctorComponent) -> &'static str {
	match component {
		DoctorComponent::Configuration => "Configuration",
		DoctorComponent::ProductStore => "Product store",
		DoctorComponent::QuickTask => "Quick Task",
		DoctorComponent::Protocol => "Protocol",
		DoctorComponent::ProtocolVersion => "Protocol version",
		DoctorComponent::ServerIdentity => "Server identity",
		DoctorComponent::SharedCodexHome => "Shared Codex home",
		DoctorComponent::AppServerCapability(capability) => match capability {
			AppServerCapability::Initialize => "App server: initialize",
			AppServerCapability::AccountRead => "App server: account read",
			AppServerCapability::ThreadList => "App server: thread list",
			AppServerCapability::ThreadRead => "App server: thread read",
			AppServerCapability::ThreadArchive => "App server: thread archive",
			AppServerCapability::PaginatedHistory => "App server: paginated history",
			AppServerCapability::NativeCollaboration => "App server: native collaboration",
			AppServerCapability::ThreadSearch => "App server: thread search",
		},
		DoctorComponent::ManagedRepository => "Managed repository",
		DoctorComponent::BlobIntegrity => "Blob integrity",
		DoctorComponent::CredentialVault => "Credential vault",
		DoctorComponent::PluginReadiness => "Plugin readiness",
	}
}

fn component_presentation(status: Option<DoctorStatus>) -> HealthPresentation {
	match status {
		Some(DoctorStatus::Ready) =>
			HealthPresentation { label: "Ready", detail: "", color: 0x22c55e },
		Some(DoctorStatus::Unavailable(DoctorIssue::Disabled))
		| Some(DoctorStatus::Unknown(DoctorIssue::Disabled)) => HealthPresentation {
			label: "Disabled",
			detail: "This optional capability is intentionally disabled.",
			color: 0x64748b,
		},
		Some(DoctorStatus::Unavailable(DoctorIssue::NotProbed))
		| Some(DoctorStatus::Unknown(DoctorIssue::NotProbed)) => HealthPresentation {
			label: "Not checked",
			detail: "The owning boundary did not run an active probe.",
			color: 0x64748b,
		},
		Some(DoctorStatus::Unknown(DoctorIssue::Plugin)) => HealthPresentation {
			label: "Not configured",
			detail: "No required plugin inventory is configured.",
			color: 0x64748b,
		},
		Some(DoctorStatus::Unavailable(issue)) => HealthPresentation {
			label: "Unavailable",
			detail: doctor_issue_detail(issue),
			color: 0xef4444,
		},
		Some(DoctorStatus::Unknown(issue)) => HealthPresentation {
			label: "Not verified",
			detail: doctor_issue_detail(issue),
			color: 0xf59e0b,
		},
		None => HealthPresentation { label: "No report", detail: "", color: 0x64748b },
	}
}

fn doctor_issue_detail(issue: DoctorIssue) -> &'static str {
	match issue {
		DoctorIssue::Authentication => "Authentication was not established.",
		DoctorIssue::Plugin => "Required plugin readiness was not established.",
		DoctorIssue::ConfigurationMissing => "The operator configuration is missing.",
		DoctorIssue::ConfigurationMalformed => "The operator configuration is malformed.",
		DoctorIssue::ConfigurationVersion => "The configuration version is unsupported.",
		DoctorIssue::DatabaseNotConfigured => "The local database is not configured.",
		DoctorIssue::DatabaseMalformedConfig => "The database configuration is malformed.",
		DoctorIssue::DatabaseUnreachable => "The local database cannot be opened.",
		DoctorIssue::DatabaseIncompatible => "The local database state is incompatible.",
		DoctorIssue::UnsafeDatabaseAuthority => "The database retains unsafe authority.",
		DoctorIssue::ProtocolDisconnected => "The daemon protocol is disconnected.",
		DoctorIssue::ProtocolVersionMismatch => "The protocol versions are incompatible.",
		DoctorIssue::ServerIdentityMismatch => "The connected server identity does not match.",
		DoctorIssue::ServerIdentityUnavailable => "A stable server identity is unavailable.",
		DoctorIssue::UnsafeHostPath => "A required host path failed its safety contract.",
		DoctorIssue::Integrity => "Storage integrity was not established.",
		DoctorIssue::NotProbed => "The owning boundary did not run an active probe.",
		DoctorIssue::Disabled => "This optional capability is intentionally disabled.",
	}
}

fn health_component_row(
	index: usize,
	component: DoctorComponent,
	status: Option<DoctorStatus>,
) -> AnyElement {
	let label = component_label(component);
	let presentation = component_presentation(status);

	div()
		.id(("health-component", index))
		.role(Role::ListItem)
		.aria_label(format!("{label}: {}", presentation.label))
		.min_h(px(52.0))
		.py_2()
		.flex()
		.items_center()
		.justify_between()
		.gap_4()
		.border_b_1()
		.border_color(rgba(0xffffff0a))
		.text_size(px(10.5))
		.text_color(rgb(WB_TEXT_MUTED))
		.child(div().min_w_0().flex().flex_col().gap_1().child(label).when(
			!presentation.detail.is_empty(),
			|element| {
				element.child(
					div()
						.text_size(px(8.0))
						.text_color(rgb(WB_TEXT_FAINT))
						.child(presentation.detail),
				)
			},
		))
		.child(
			div()
				.w(px(128.0))
				.min_w(px(128.0))
				.flex()
				.items_center()
				.gap_2()
				.child(
					div().size(px(9.0)).min_w(px(9.0)).rounded_full().bg(rgb(presentation.color)),
				)
				.child(presentation.label),
		)
		.into_any_element()
}

fn health_component_section(
	id: &'static str,
	title: &'static str,
	detail: &'static str,
	index_offset: usize,
	components: &[DoctorComponent],
	snapshot: &HealthSnapshot,
) -> AnyElement {
	let rows = components.iter().copied().enumerate().map(|(index, component)| {
		let status = snapshot
			.report
			.as_ref()
			.and_then(|report| report.check(component))
			.map(|check| check.status);
		health_component_row(index_offset + index, component, status)
	});

	div()
		.id(id)
		.flex()
		.flex_col()
		.child(
			div()
				.px_1()
				.pb_2()
				.flex()
				.items_end()
				.justify_between()
				.gap_4()
				.child(
					div()
						.text_size(px(10.0))
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(rgb(WB_TEXT))
						.child(title),
				)
				.child(div().text_size(px(8.0)).text_color(rgb(WB_TEXT_FAINT)).child(detail)),
		)
		.child(
			div()
				.id(("health-section-list", index_offset))
				.role(Role::List)
				.aria_label(title)
				.px_4()
				.border_1()
				.border_color(rgba(0xffffff10))
				.rounded(px(12.0))
				.bg(rgba(0x00000024))
				.children(rows),
		)
		.into_any_element()
}

fn refresh_control(
	focus: FocusHandle,
	can_refresh: bool,
	window: &Window,
	cx: &mut Context<Shell>,
) -> AnyElement {
	div()
		.id("health-refresh")
		.role(Role::Button)
		.aria_label("Refresh health")
		.tooltip(|_, cx| cx.new(|_| RefreshTooltip).into())
		.h(px(30.0))
		.min_w(px(74.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(8.0))
		.border_1()
		.border_color(if can_refresh && focus.is_focused(window) {
			rgb(WB_BLUE)
		} else {
			rgba(0xffffff16)
		})
		.bg(if can_refresh { rgba(0xffffff08) } else { rgba(0xffffff03) })
		.text_color(if can_refresh { rgb(WB_TEXT_MUTED) } else { rgb(WB_TEXT_FAINT) })
		.when(can_refresh, |element| {
			element
				.key_context("HealthRefresh")
				.track_focus(&focus)
				.on_action(cx.listener(Shell::focus_next))
				.on_action(cx.listener(Shell::focus_previous))
				.on_action(cx.listener(Shell::refresh_health))
				.on_click(cx.listener(|shell, _, _, cx| shell.request_health_refresh(cx)))
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0f)).text_color(rgb(WB_TEXT)))
				.active(|element| element.bg(rgba(0xffffff1c)).opacity(0.82))
				.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
		})
		.text_size(px(9.5))
		.child("Refresh")
		.into_any_element()
}

fn destination_header(
	selected: Destination,
	health: &HealthSnapshot,
	refresh_focus: FocusHandle,
	window: &Window,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let title = div()
		.id("destination-heading")
		.role(Role::Heading)
		.aria_level(1)
		.aria_label(format!("{} workspace", selected.label()))
		.flex()
		.flex_col()
		.gap_1()
		.child(
			div()
				.font_family("SF Mono")
				.text_size(px(7.5))
				.text_color(rgb(WB_TEXT_FAINT))
				.child("OPERATIONAL SURFACE"),
		)
		.child(
			div()
				.text_size(px(17.0))
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(rgb(WB_TEXT))
				.child(selected.label()),
		);
	let header = div()
		.h(px(72.0))
		.min_h(px(72.0))
		.px_6()
		.flex()
		.items_center()
		.justify_between()
		.border_b_1()
		.border_color(rgba(0xffffff0d))
		.bg(rgba(0x00000014))
		.child(title);

	if selected == Destination::Health {
		header
			.child(refresh_control(refresh_focus, health.can_refresh, window, cx))
			.into_any_element()
	} else {
		header.into_any_element()
	}
}

fn placeholder_content(selected: Destination) -> AnyElement {
	div()
		.flex_1()
		.min_h_0()
		.p_7()
		.flex()
		.items_start()
		.justify_center()
		.child(
			div()
				.w_full()
				.max_w(px(760.0))
				.p_6()
				.flex()
				.flex_col()
				.gap_3()
				.rounded(px(14.0))
				.border_1()
				.border_color(rgba(0xffffff10))
				.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(8.0))
						.text_color(rgb(WB_ACCENT))
						.child("PLANNED SURFACE"),
				)
				.child(
					div()
						.text_size(px(18.0))
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(rgb(WB_TEXT))
						.child(selected.label()),
				)
				.child(
					div()
						.text_size(px(11.0))
						.line_height(px(17.0))
						.text_color(rgb(WB_TEXT_MUTED))
						.child(selected.description()),
				)
				.child(
					div()
						.pt_3()
						.border_t_1()
						.border_color(rgba(0xffffff0d))
						.font_family("SF Mono")
						.text_size(px(8.5))
						.text_color(rgb(WB_TEXT_FAINT))
						.child(
							"No speculative controls are exposed before this projection has an authority owner.",
						),
				),
		)
		.into_any_element()
}

fn accounts_content(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let snapshot = &shell.accounts;
	let fixed = snapshot.routing.as_ref().and_then(|routing| match &routing.mode {
		AccountSelectionModeDto::Fixed(account_id) => Some(account_id),
		AccountSelectionModeDto::Balanced => None,
	});
	let balanced = snapshot
		.routing
		.as_ref()
		.is_some_and(|routing| routing.mode == AccountSelectionModeDto::Balanced);
	let can_manage = snapshot.can_manage;
	let rows = snapshot
		.accounts
		.iter()
		.enumerate()
		.map(|(index, account)| {
			account_pool_row(account, index, fixed == Some(&account.account_id), can_manage, cx)
		})
		.collect::<Vec<_>>();
	let status = shell
		.account_status
		.as_ref()
		.map(SharedString::to_string)
		.or_else(|| account_command_label(snapshot.command).map(str::to_owned))
		.unwrap_or_else(|| accounts_load_label(snapshot.load).to_owned());
	let count = snapshot.accounts.len();
	let available = snapshot
		.accounts
		.iter()
		.filter(|account| {
			account.enabled
				&& account.observed_state == AccountObservedStateDto::Available
				&& account.lifecycle_readiness == AccountLifecycleReadinessDto::Ready
		})
		.count();

	div()
		.flex_1()
		.min_h_0()
		.px_6()
		.py_5()
		.flex()
		.justify_center()
		.child(
			div()
				.w_full()
				.max_w(px(900.0))
				.min_h_0()
				.flex()
				.flex_col()
				.gap_3()
				.child(
					div()
						.p_5()
						.flex()
						.items_center()
						.justify_between()
						.gap_6()
						.rounded(px(13.0))
						.border_1()
						.border_color(rgba(0xffffff12))
						.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
						.child(
							div()
								.min_w_0()
								.flex()
								.flex_col()
								.gap_1()
								.child(
									div()
										.flex()
										.items_center()
										.gap_2()
										.child(div().size(px(6.0)).rounded_full().bg(rgb(WB_GREEN)))
										.child(
											div()
												.text_size(px(14.0))
												.font_weight(FontWeight::SEMIBOLD)
												.child("Account pool"),
										)
										.child(
											div()
										.font_family("SF Mono")
										.text_size(px(8.0))
										.text_color(rgb(WB_TEXT_FAINT))
										.child(format!("{count} ACCOUNTS · {available} AVAILABLE")),
										),
								)
								.child(
									div()
										.max_w(px(570.0))
										.text_size(px(10.5))
										.line_height(px(16.0))
										.text_color(rgb(WB_TEXT_MUTED))
										.child("Routing is chosen only for a new Codex conversation. Existing threads keep their bound account and cache affinity."),
								),
						)
						.child(
							div()
								.flex()
								.items_center()
								.gap_2()
								.child(account_mode_button("Balanced", balanced, can_manage, cx))
								.child(
									div()
										.id("accounts-refresh")
										.role(Role::Button)
										.aria_label("Refresh account pool")
										.h(px(28.0))
										.px_3()
										.flex()
										.items_center()
										.rounded(px(7.0))
										.border_1()
										.border_color(rgba(0xffffff14))
										.text_size(px(9.0))
										.text_color(rgb(WB_TEXT_MUTED))
										.cursor_pointer()
										.hover(|element| {
											element.bg(rgba(0xffffff0d)).text_color(rgb(WB_TEXT))
										})
										.active(|element| element.bg(rgba(0xffffff1b)).opacity(0.84))
										.on_click(cx.listener(|shell, _, _, cx| {
											shell.refresh_accounts(cx);
										}))
										.child("Refresh"),
								),
						),
				)
				.child(
					div()
						.id("account-list")
						.flex_1()
						.min_h_0()
						.overflow_y_scroll()
						.flex()
						.flex_col()
						.gap_2()
						.when(count == 0, |list| {
							list.child(
								div()
									.h(px(150.0))
									.flex()
									.flex_col()
									.items_center()
									.justify_center()
									.gap_2()
									.rounded(px(12.0))
									.border_1()
									.border_color(rgba(0xffffff0d))
									.bg(rgba(0x0000001f))
									.text_size(px(10.0))
									.text_color(rgb(WB_TEXT_MUTED))
									.child("No account projection is available yet.")
									.child(
										div()
											.font_family("SF Mono")
											.text_size(px(8.0))
											.text_color(rgb(WB_TEXT_FAINT))
											.child("Enroll or recover credentials from the menu bar surface."),
									),
							)
						})
						.children(rows),
				)
				.child(
					div()
						.h(px(28.0))
						.px_3()
						.flex()
						.items_center()
						.justify_between()
						.rounded(px(8.0))
						.border_1()
						.border_color(rgba(0xffffff0d))
						.bg(rgba(0x0000001f))
						.font_family("SF Mono")
						.text_size(px(8.0))
						.text_color(rgb(WB_TEXT_FAINT))
						.child(status)
						.child("CREDENTIAL-NEGATIVE · DAEMON AUTHORITY"),
				),
		)
		.into_any_element()
}

fn account_mode_button(
	label: &'static str,
	selected: bool,
	can_manage: bool,
	cx: &mut Context<Shell>,
) -> AnyElement {
	div()
		.id("accounts-balanced")
		.role(Role::Button)
		.aria_label("Use balanced routing for new conversations")
		.h(px(28.0))
		.px_3()
		.flex()
		.items_center()
		.rounded(px(7.0))
		.border_1()
		.border_color(if selected { rgba(0x60a5fa55) } else { rgba(0xffffff14) })
		.bg(if selected { rgba(0x60a5fa16) } else { rgba(0x00000000) })
		.text_size(px(9.0))
		.text_color(if selected { rgb(WB_TEXT) } else { rgb(WB_TEXT_MUTED) })
		.when(can_manage && !selected, |button| {
			button
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(WB_TEXT)))
				.active(|element| element.bg(rgba(0xffffff1b)).opacity(0.84))
				.on_click(cx.listener(|shell, _, _, cx| shell.select_balanced_accounts(cx)))
		})
		.child(label)
		.into_any_element()
}

fn account_pool_row(
	account: &AccountDto,
	index: usize,
	fixed: bool,
	can_manage: bool,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let account_id = account.account_id.clone();
	let toggle_account_id = account.account_id.clone();
	let enabled = account.enabled;
	let pin_enabled = can_manage && enabled && !fixed;
	let toggle_enabled = can_manage;
	let state_color = account_state_color(account);
	let short_id = account.account_id.as_str().get(..8).unwrap_or(account.account_id.as_str());

	div()
		.id(("account-row", index))
		.p_4()
		.flex()
		.items_center()
		.gap_4()
		.rounded(px(11.0))
		.border_1()
		.border_color(if fixed { rgba(0x60a5fa42) } else { rgba(0xffffff0f) })
		.bg(if fixed { rgba(0x60a5fa0d) } else { rgba(ui_theme::SURFACE_MATERIAL) })
		.child(
			div()
				.w(px(222.0))
				.min_w(px(190.0))
				.flex()
				.items_center()
				.gap_3()
				.child(div().size(px(7.0)).rounded_full().bg(rgb(state_color)))
				.child(
					div()
						.min_w_0()
						.flex()
						.flex_col()
						.gap_1()
						.child(
							div()
								.overflow_hidden()
								.whitespace_nowrap()
								.text_ellipsis()
								.text_size(px(11.0))
								.font_weight(FontWeight::SEMIBOLD)
								.text_color(if enabled { rgb(WB_TEXT) } else { rgb(WB_TEXT_FAINT) })
								.child(account.alias.as_str().to_owned()),
						)
						.child(
							div()
								.flex()
								.items_center()
								.gap_2()
								.font_family("SF Mono")
								.text_size(px(7.5))
								.text_color(rgb(WB_TEXT_FAINT))
								.child(account_readiness_label(account.lifecycle_readiness))
								.child(format!("· {short_id}")),
						),
				),
		)
		.child(
			div().flex_1().min_w_0().flex().items_center().gap_5().children(
				[
					account_quota("5 HOUR", account.five_hour_quota),
					account_quota("7 DAY", account.seven_day_quota),
				]
				.into_iter()
				.flatten(),
			),
		)
		.child(
			div()
				.flex()
				.items_center()
				.gap_2()
				.child(
					div()
						.id(("account-pin", index))
						.role(Role::Button)
						.aria_label(format!(
							"Route new conversations to {}",
							account.alias.as_str()
						))
						.h(px(27.0))
						.w(px(58.0))
						.flex()
						.items_center()
						.justify_center()
						.rounded(px(7.0))
						.border_1()
						.border_color(if fixed { rgba(0x60a5fa55) } else { rgba(0xffffff12) })
						.bg(if fixed { rgba(0x60a5fa18) } else { rgba(0x00000000) })
						.text_size(px(8.5))
						.text_color(if fixed { rgb(WB_BLUE) } else { rgb(WB_TEXT_MUTED) })
						.when(pin_enabled, |button| {
							button
								.cursor_pointer()
								.hover(|element| {
									element.bg(rgba(0xffffff0d)).text_color(rgb(WB_TEXT))
								})
								.active(|element| element.bg(rgba(0xffffff1b)).opacity(0.84))
								.on_click(cx.listener(move |shell, _, _, cx| {
									shell.select_fixed_account(&account_id, cx);
								}))
						})
						.child(if fixed { "Pinned" } else { "Route" }),
				)
				.child(
					div()
						.id(("account-enabled", index))
						.role(Role::Button)
						.aria_label(format!(
							"{} {}",
							if enabled { "Disable" } else { "Enable" },
							account.alias.as_str()
						))
						.h(px(27.0))
						.w(px(58.0))
						.flex()
						.items_center()
						.justify_center()
						.rounded(px(7.0))
						.border_1()
						.border_color(if enabled { rgba(0x22c55e45) } else { rgba(0xffffff12) })
						.bg(if enabled { rgba(0x22c55e12) } else { rgba(0x00000000) })
						.text_size(px(8.5))
						.text_color(if enabled { rgb(WB_GREEN) } else { rgb(WB_TEXT_FAINT) })
						.when(toggle_enabled, |button| {
							button
								.cursor_pointer()
								.hover(|element| {
									element.bg(rgba(0xffffff0d)).text_color(rgb(WB_TEXT))
								})
								.active(|element| element.bg(rgba(0xffffff1b)).opacity(0.84))
								.on_click(cx.listener(move |shell, _, _, cx| {
									shell.set_account_enabled(&toggle_account_id, !enabled, cx);
								}))
						})
						.child(if enabled { "Enabled" } else { "Disabled" }),
				),
		)
		.into_any_element()
}

fn account_quota(label: &'static str, quota: AccountQuotaWindowDto) -> Option<AnyElement> {
	let AccountQuotaStateDto::Current { used_percent, .. } = quota.result else {
		return None;
	};
	let detail = format!("{used_percent}% used");
	let used = f32::from(used_percent);
	let color = if used_percent >= 90 {
		0xef4444
	} else if used_percent >= 70 {
		WB_AMBER
	} else {
		WB_BLUE
	};
	Some(
		div()
			.w(px(122.0))
			.flex()
			.flex_col()
			.gap_1()
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.font_family("SF Mono")
					.text_size(px(7.5))
					.text_color(rgb(WB_TEXT_FAINT))
					.child(label)
					.child(detail),
			)
			.child(div().h(px(3.0)).w_full().rounded_full().bg(rgba(0xffffff0c)).child(
				div().h_full().w(px(used.clamp(0.0, 100.0) * 1.22)).rounded_full().bg(rgb(color)),
			))
			.into_any_element(),
	)
}

fn account_state_color(account: &AccountDto) -> u32 {
	if !account.enabled {
		return WB_TEXT_FAINT;
	}
	match account.observed_state {
		AccountObservedStateDto::Available => WB_GREEN,
		AccountObservedStateDto::Unknown | AccountObservedStateDto::PluginUnready => WB_AMBER,
		AccountObservedStateDto::Unavailable
		| AccountObservedStateDto::Depleted
		| AccountObservedStateDto::AuthFailed => 0xef4444,
	}
}

fn account_readiness_label(readiness: AccountLifecycleReadinessDto) -> &'static str {
	match readiness {
		AccountLifecycleReadinessDto::Ready => "READY",
		AccountLifecycleReadinessDto::CredentialAbsent => "NO CREDENTIAL",
		AccountLifecycleReadinessDto::StoreUnavailable => "STORE UNAVAILABLE",
		AccountLifecycleReadinessDto::StoreMismatch => "STORE MISMATCH",
		AccountLifecycleReadinessDto::ProviderMismatch => "PROVIDER MISMATCH",
		AccountLifecycleReadinessDto::OperationUnsettled => "OPERATION PENDING",
		AccountLifecycleReadinessDto::CallbackCapabilityUnready => "CALLBACK UNREADY",
		AccountLifecycleReadinessDto::Tombstoned => "LOGGED OUT",
	}
}

fn accounts_load_label(load: AccountsLoadState) -> &'static str {
	match load {
		AccountsLoadState::NeverRequested => "Open Accounts to load the daemon-owned pool.",
		AccountsLoadState::Loading => "Loading account pool…",
		AccountsLoadState::Ready => "Account pool is synchronized.",
		AccountsLoadState::Offline => "Account authority is offline.",
		AccountsLoadState::Stale => "Showing retained account state; refresh after reconnect.",
		AccountsLoadState::Unavailable => "The daemon could not return a safe account snapshot.",
		AccountsLoadState::Refused => "The account response did not match this request.",
	}
}

fn account_command_label(command: AccountCommandState) -> Option<&'static str> {
	match command {
		AccountCommandState::Idle => None,
		AccountCommandState::Sending => Some("Sending account command…"),
		AccountCommandState::AwaitingResult => Some("Waiting for durable account result…"),
		AccountCommandState::Accepted => Some("Account pool updated."),
		AccountCommandState::OutcomeUnknown =>
			Some("Outcome is unknown. Refresh readback before another account change."),
		AccountCommandState::Refused => Some("The account change was refused. Refresh and retry."),
	}
}

fn account_input_error_label(error: AccountInputError) -> &'static str {
	match error {
		AccountInputError::Offline => "Account authority is offline.",
		AccountInputError::Busy => "Wait for the current account change to finish.",
		AccountInputError::AccountMissing => "That account is no longer in the current pool.",
		AccountInputError::RoutingUnavailable => "Routing controls are unavailable. Refresh first.",
		AccountInputError::IdentityUnavailable => "A command identity could not be created.",
	}
}

fn quick_task_state_label(state: QuickTaskState) -> &'static str {
	match state {
		QuickTaskState::RoutingPending => "Routing pending",
		QuickTaskState::EstablishmentPending => "Establishment pending",
		QuickTaskState::QuotaExhausted => "Quota exhausted",
		QuickTaskState::NoRoute => "No route",
		QuickTaskState::Establishing => "Establishing",
		QuickTaskState::Ready => "Ready",
		QuickTaskState::Running => "Running",
		QuickTaskState::ManualRecovery => "Action required",
		QuickTaskState::OutcomeUnknown => "Recovering",
	}
}

fn quick_task_state_color(state: QuickTaskState) -> u32 {
	match state {
		QuickTaskState::Ready => 0x22c55e,
		QuickTaskState::Running | QuickTaskState::Establishing => 0x60a5fa,
		QuickTaskState::RoutingPending
		| QuickTaskState::EstablishmentPending
		| QuickTaskState::QuotaExhausted
		| QuickTaskState::NoRoute => 0xf59e0b,
		QuickTaskState::ManualRecovery => 0xef4444,
		QuickTaskState::OutcomeUnknown => 0xf59e0b,
	}
}

fn command_status(command: QuickTaskCommandState) -> Option<&'static str> {
	match command {
		QuickTaskCommandState::Idle => None,
		QuickTaskCommandState::Sending => Some("Sending command"),
		QuickTaskCommandState::AwaitingResult => Some("Waiting for durable result"),
		QuickTaskCommandState::Accepted => Some("Command accepted"),
		QuickTaskCommandState::ManualRecovery(action) => Some(recovery_action_label(action)),
		QuickTaskCommandState::OutcomeUnknown =>
			Some("Connection interrupted. Decodex will check durable state before continuing."),
		QuickTaskCommandState::Refused => Some("The command was refused."),
	}
}

fn recovery_action_label(action: QuickTaskRecoveryAction) -> &'static str {
	match action {
		QuickTaskRecoveryAction::ResumeRouting => "Resume the pending account route.",
		QuickTaskRecoveryAction::CreateRoutingSuccessor =>
			"Create a new conversation and route it explicitly.",
		QuickTaskRecoveryAction::ResumeEstablishment =>
			"Resume the selected account session establishment.",
		QuickTaskRecoveryAction::ConfigureAccount => "Configure an account before continuing.",
		QuickTaskRecoveryAction::EnableAccount => "Enable the selected account before continuing.",
		QuickTaskRecoveryAction::EnrollCredentials =>
			"Enroll account credentials before continuing.",
		QuickTaskRecoveryAction::ResolveAccountOperation =>
			"Resolve the unsettled account operation before continuing.",
		QuickTaskRecoveryAction::RepairCredentialStore =>
			"Repair the protected credential store before continuing.",
		QuickTaskRecoveryAction::RestoreProviderAgreement =>
			"Restore provider account agreement before continuing.",
		QuickTaskRecoveryAction::RefreshQuota => "Refresh account quota before continuing.",
		QuickTaskRecoveryAction::UpgradeCodex =>
			"Use a Codex build with the required app-server methods.",
		QuickTaskRecoveryAction::SelectWorkingDirectory =>
			"Select an owned local working directory before continuing.",
		QuickTaskRecoveryAction::StartNewConversation =>
			"This thread cannot resume. Start a new conversation.",
		QuickTaskRecoveryAction::ResolvePriorActiveTurn =>
			"Resolve the prior active turn before continuing.",
		QuickTaskRecoveryAction::ResolvePriorAttempt =>
			"Resolve the prior provider attempt before continuing.",
		QuickTaskRecoveryAction::RestoreProcessReadiness =>
			"Restore process readiness before continuing.",
		QuickTaskRecoveryAction::WaitForCurrentCommand =>
			"Wait for the current command or turn to settle.",
		QuickTaskRecoveryAction::RefreshConversation =>
			"Refresh this conversation before continuing.",
	}
}

fn quick_task_load_status(load: QuickTasksLoadState) -> &'static str {
	match load {
		QuickTasksLoadState::NeverRequested => "Quick Tasks have not loaded.",
		QuickTasksLoadState::Loading => "Loading Quick Tasks",
		QuickTasksLoadState::Ready =>
			"Local task list loaded. Open or refresh a task for latest Codex state.",
		QuickTasksLoadState::Offline => "Offline. Retained conversation state remains visible.",
		QuickTasksLoadState::Unavailable => "Quick Task state is temporarily unavailable.",
		QuickTasksLoadState::Refused => "Quick Task readback was refused.",
	}
}

fn quick_task_refresh_status(refresh: QuickTaskRefreshState) -> Option<String> {
	match refresh {
		QuickTaskRefreshState::Idle => None,
		QuickTaskRefreshState::Refreshing { completed, total, archived, failed } =>
			Some(format!("Syncing {completed}/{total} · {archived} archived · {failed} skipped")),
		QuickTaskRefreshState::Complete { checked, archived, failed } =>
			Some(format!("{checked} checked · {archived} archived · {failed} skipped")),
		QuickTaskRefreshState::Stopped { checked, total, archived, failed } =>
			Some(format!("Stopped {checked}/{total} · {archived} archived · {failed} skipped")),
	}
}

fn bound_work_item<'a>(
	shell: &'a Shell,
	conversation_id: &EntityId,
) -> Option<&'a WorkItemBoardCard> {
	shell.work.cards.iter().find(|card| card.conversation_id() == Some(conversation_id))
}

fn quick_task_session_sidebar(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let selected = shell.quick.selected.clone();
	let can_control = shell.quick.can_submit
		&& shell.quick.selected_task().is_some_and(|task| task.state == QuickTaskState::Ready);
	let can_refresh_all = shell.quick.can_submit && shell.quick.load == QuickTasksLoadState::Ready;
	let refresh_status = quick_task_refresh_status(shell.quick.refresh);
	let refresh_label = match shell.quick.refresh {
		QuickTaskRefreshState::Refreshing { completed, total, .. } => {
			format!("{completed}/{total}")
		},
		_ => "↻".to_owned(),
	};
	let refresh_text_size =
		if matches!(shell.quick.refresh, QuickTaskRefreshState::Refreshing { .. }) {
			8.0
		} else {
			12.0
		};
	let rows = shell.quick.tasks.iter().enumerate().map(|(index, task)| {
		let conversation_id = task.conversation_id.clone();
		let short_id = task.conversation_id.as_str().chars().take(8).collect::<String>();
		let is_selected = selected.as_ref() == Some(&task.conversation_id);
		let state = task.state;
		let label = bound_work_item(shell, &task.conversation_id)
			.map(|card| card.title().as_str().to_owned())
			.unwrap_or_else(|| format!("Conversation {short_id}"));
		div()
			.id(("quick-task-row", index))
			.role(Role::Tab)
			.aria_label(format!("Conversation {short_id}, {}", quick_task_state_label(state)))
			.aria_selected(is_selected)
			.w_full()
			.min_h(px(52.0))
			.px_3()
			.py_2()
			.flex()
			.flex_col()
			.justify_center()
			.gap_1()
			.rounded(px(9.0))
			.border_1()
			.border_color(if is_selected { rgba(0xffffff18) } else { rgba(0x00000000) })
			.bg(if is_selected { rgba(0xffffff0f) } else { rgba(0x00000000) })
			.text_size(px(9.5))
			.text_color(if is_selected { rgb(WB_TEXT) } else { rgb(WB_TEXT_MUTED) })
			.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
			.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
			.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
			.cursor_pointer()
			.on_click(cx.listener(move |shell, _, window, cx| {
				shell.choose_quick_task(conversation_id.clone(), window, cx);
			}))
			.child(
				div()
					.w_full()
					.min_w_0()
					.flex()
					.items_center()
					.gap_2()
					.child(
						div()
							.size(px(5.0))
							.min_w(px(5.0))
							.rounded_full()
							.bg(rgb(quick_task_state_color(state))),
					)
					.child(
						div()
							.flex_1()
							.min_w_0()
							.overflow_hidden()
							.whitespace_nowrap()
							.text_ellipsis()
							.child(label),
					),
			)
			.child(
				div()
					.pl(px(13.0))
					.font_family("SF Mono")
					.text_size(px(7.5))
					.text_color(rgb(WB_TEXT_FAINT))
					.child(format!("{} · {short_id}", quick_task_state_label(state))),
			)
	});

	div()
		.id("quick-task-session-sidebar")
		.role(Role::TabList)
		.aria_label("Quick Task conversations")
		.w(px(WORKBENCH_SESSION_SIDEBAR_WIDTH))
		.min_w(px(WORKBENCH_SESSION_SIDEBAR_WIDTH))
		.h_full()
		.flex()
		.flex_col()
		.border_r_1()
		.border_color(rgba(0xffffff0d))
		.bg(rgba(ui_theme::SIDEBAR_MATERIAL))
		.child(
			div()
				.h(px(48.0))
				.min_h(px(48.0))
				.px_3()
				.flex()
				.items_center()
				.justify_between()
				.border_b_1()
				.border_color(rgba(0xffffff0d))
				.child(
					div()
						.min_w_0()
						.flex()
						.flex_col()
						.gap_1()
						.font_weight(FontWeight::SEMIBOLD)
						.text_size(px(10.0))
						.text_color(rgb(WB_TEXT))
						.child("Sessions")
						.when_some(refresh_status, |element, status| {
							element.child(
								div()
									.max_w(px(156.0))
									.overflow_hidden()
									.whitespace_nowrap()
									.text_ellipsis()
									.font_weight(FontWeight::NORMAL)
									.text_size(px(7.0))
									.text_color(rgb(WB_TEXT_FAINT))
									.child(status),
							)
						}),
				)
				.child(
					div()
						.flex()
						.items_center()
						.gap_1()
						.child(
							div()
								.id("refresh-quick-task")
								.role(Role::Button)
								.aria_label("Sync Codex-backed conversations")
								.tooltip(|_, cx| {
									cx.new(|_| ControlTooltip("Sync Codex-backed conversations"))
										.into()
								})
								.h(px(27.0))
								.min_w(px(27.0))
								.px_2()
								.flex()
								.items_center()
								.justify_center()
								.rounded(px(7.0))
								.text_size(px(refresh_text_size))
								.text_color(if can_refresh_all {
									rgb(WB_TEXT_MUTED)
								} else {
									rgb(WB_TEXT_FAINT)
								})
								.when(can_refresh_all, |element| {
									element
										.cursor_pointer()
										.hover(|element| {
											element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT))
										})
										.active(|element| {
											element.bg(rgba(0xffffff18)).opacity(0.82)
										})
										.on_click(cx.listener(|shell, _, window, cx| {
											shell.refresh_quick_task(window, cx);
										}))
								})
								.child(refresh_label),
						)
						.child(
							div()
								.id("archive-quick-task")
								.role(Role::Button)
								.aria_label("Archive selected Codex conversation")
								.tooltip(|_, cx| {
									cx.new(|_| ControlTooltip("Archive selected thread")).into()
								})
								.h(px(27.0))
								.px_2()
								.flex()
								.items_center()
								.rounded(px(7.0))
								.text_size(px(8.0))
								.text_color(if can_control {
									rgb(WB_TEXT_MUTED)
								} else {
									rgb(WB_TEXT_FAINT)
								})
								.when(can_control, |element| {
									element
										.cursor_pointer()
										.hover(|element| {
											element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT))
										})
										.active(|element| {
											element.bg(rgba(0xffffff18)).opacity(0.82)
										})
										.on_click(cx.listener(|shell, _, window, cx| {
											shell.archive_quick_task(window, cx);
										}))
								})
								.child("Archive"),
						)
						.child(
							div()
								.id("new-quick-task")
								.role(Role::Button)
								.aria_label("New conversation")
								.h(px(27.0))
								.px_2()
								.flex()
								.items_center()
								.rounded(px(7.0))
								.border_1()
								.border_color(rgba(0xffffff14))
								.text_size(px(8.5))
								.text_color(rgb(WB_TEXT_MUTED))
								.hover(|element| {
									element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT))
								})
								.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
								.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
								.cursor_pointer()
								.on_click(cx.listener(|shell, _, window, cx| {
									shell.start_new_quick_task(window, cx);
								}))
								.child("+ New"),
						),
				),
		)
		.child(
			div()
				.id("quick-task-list")
				.flex_1()
				.min_h_0()
				.p_2()
				.flex()
				.flex_col()
				.gap_2()
				.overflow_y_scroll()
				.children(rows),
		)
		.into_any_element()
}

fn animated_horizontal_panel_slot(
	id: &'static str,
	visible: bool,
	generation: u64,
	full_width: f32,
	panel: AnyElement,
) -> AnyElement {
	let animation_id = format!("{id}-{generation}-{}", if visible { "open" } else { "close" });
	div()
		.h_full()
		.flex_none()
		.overflow_hidden()
		.child(panel)
		.with_animation(
			animation_id,
			Animation::new(ui_theme::MOTION_PANEL).with_easing(ease_in_out),
			move |slot, delta| {
				let progress = if visible { delta } else { 1.0 - delta };
				let width = px(full_width * progress);
				slot.w(width).min_w(width).max_w(width).opacity(0.3 + progress * 0.7)
			},
		)
		.into_any_element()
}

fn history_role_label(role: HistoryTurnRole) -> &'static str {
	match role {
		HistoryTurnRole::User => "You",
		HistoryTurnRole::Assistant => "Codex",
		HistoryTurnRole::System => "System",
		HistoryTurnRole::Tool => "Tool",
	}
}

fn history_kind_label(kind: HistoryItemKindDto, status: HistoryItemStatusDto) -> &'static str {
	match (kind, status) {
		(HistoryItemKindDto::ToolCall, HistoryItemStatusDto::Completed) => "Ran command",
		(HistoryItemKindDto::ToolCall, HistoryItemStatusDto::Streaming) => "Running command",
		(HistoryItemKindDto::ToolCall, HistoryItemStatusDto::Failed) => "Command failed",
		(HistoryItemKindDto::ToolResult, HistoryItemStatusDto::Completed) => "Command result",
		(HistoryItemKindDto::ToolResult, HistoryItemStatusDto::Streaming) => "Receiving result",
		(HistoryItemKindDto::ToolResult, HistoryItemStatusDto::Failed) => "Result failed",
		(HistoryItemKindDto::Reasoning, _) => "Reasoning",
		(HistoryItemKindDto::Artifact, _) => "Artifact",
		(HistoryItemKindDto::Status, _) => "Activity",
		(HistoryItemKindDto::Message, _) => "Message",
	}
}

fn inspector_tab(
	id: &'static str,
	label: &'static str,
	tab: InspectorTab,
	selected: InspectorTab,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let is_selected = tab == selected;
	div()
		.id(id)
		.role(Role::Tab)
		.aria_label(label)
		.aria_selected(is_selected)
		.h(px(27.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(6.0))
		.bg(if is_selected { rgba(0xffffff0e) } else { rgba(0x00000000) })
		.text_size(px(9.5))
		.font_weight(if is_selected { FontWeight::MEDIUM } else { FontWeight::NORMAL })
		.text_color(if is_selected { rgb(WB_TEXT) } else { rgb(WB_TEXT_FAINT) })
		.cursor_pointer()
		.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
		.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
		.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
		.on_click(cx.listener(move |shell, _, _, cx| {
			shell.inspector_tab = tab;
			cx.notify();
		}))
		.child(label)
		.into_any_element()
}

fn execution_lineage_node(
	index: usize,
	label: &'static str,
	value: String,
	color: u32,
	is_last: bool,
) -> AnyElement {
	div()
		.id(("execution-lineage", index))
		.w_full()
		.min_h(px(39.0))
		.flex()
		.gap_3()
		.child(
			div()
				.w(px(9.0))
				.min_w(px(9.0))
				.flex()
				.flex_col()
				.items_center()
				.child(
					div()
						.mt(px(4.0))
						.size(px(6.0))
						.rounded_full()
						.border_1()
						.border_color(rgb(color))
						.bg(rgba(0x0b0a0fff)),
				)
				.when(!is_last, |element| {
					element.child(div().w(px(1.0)).flex_1().bg(rgba(0xffffff14)))
				}),
		)
		.child(
			div()
				.min_w_0()
				.flex_1()
				.flex()
				.flex_col()
				.gap_1()
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(7.5))
						.text_color(rgb(WB_TEXT_FAINT))
						.child(label),
				)
				.child(
					div()
						.min_w_0()
						.overflow_hidden()
						.whitespace_nowrap()
						.text_ellipsis()
						.text_size(px(9.5))
						.text_color(rgb(WB_TEXT_MUTED))
						.child(value),
				),
		)
		.into_any_element()
}

fn inspector_metadata_row(label: &'static str, value: String) -> AnyElement {
	div()
		.w_full()
		.min_h(px(26.0))
		.flex()
		.items_start()
		.justify_between()
		.gap_3()
		.text_size(px(9.0))
		.child(div().w(px(78.0)).min_w(px(78.0)).text_color(rgb(WB_TEXT_FAINT)).child(label))
		.child(
			div()
				.min_w_0()
				.flex_1()
				.font_family("SF Mono")
				.text_color(rgb(WB_TEXT_MUTED))
				.text_right()
				.overflow_hidden()
				.whitespace_nowrap()
				.text_ellipsis()
				.child(value),
		)
		.into_any_element()
}

fn work_item_inspector_content(shell: &Shell) -> AnyElement {
	let selected_id = shell.quick.selected.as_ref();
	let card = selected_id.and_then(|conversation_id| bound_work_item(shell, conversation_id));
	let content = if let Some(card) = card {
		let project = shell
			.work
			.projects
			.iter()
			.find(|project| project.project_id() == card.project_id())
			.map(|project| project.repository_identity().as_str().to_owned())
			.unwrap_or_else(|| card.project_id().as_str().to_owned());
		let relation_count = card.depends_on_ids().len() + card.blocked_by_ids().len();
		let state = card.state();
		let conversation = card
			.conversation_id()
			.map(|identity| compact_identity(identity.as_str()))
			.unwrap_or_else(|| "not bound".to_owned());
		let runtime = shell
			.quick
			.selected_task()
			.and_then(|task| task.runtime_session_id.as_ref())
			.map(|identity| compact_identity(identity.as_str()))
			.unwrap_or_else(|| "not established".to_owned());
		div()
			.flex()
			.flex_col()
			.gap_4()
			.child(
				div()
					.flex()
					.flex_col()
					.gap_2()
					.child(
						div()
							.flex()
							.items_center()
							.gap_2()
							.child(
								div()
									.size(px(6.0))
									.rounded_full()
									.bg(rgb(work_item_state_color(state))),
							)
							.child(
								div()
									.font_family("SF Mono")
									.text_size(px(8.0))
									.text_color(rgb(work_item_state_color(state)))
									.child(state.as_str().to_uppercase()),
							),
					)
					.child(
						div()
							.id("inspector-work-item-heading")
							.role(Role::Heading)
							.aria_level(2)
							.text_size(px(14.0))
							.font_weight(FontWeight::SEMIBOLD)
							.text_color(rgb(WB_TEXT))
							.child(card.title().as_str().to_owned()),
					)
					.child(
						div()
							.text_size(px(10.0))
							.line_height(px(15.0))
							.text_color(rgb(WB_TEXT_MUTED))
							.whitespace_normal()
							.child(card.description().as_str().to_owned()),
					),
			)
			.child(
				div()
					.w_full()
					.pt_3()
					.flex()
					.flex_col()
					.border_t_1()
					.border_color(rgba(0xffffff0c))
					.child(
						div()
							.mb_3()
							.font_family("SF Mono")
							.text_size(px(7.5))
							.text_color(rgb(WB_TEXT_FAINT))
							.child("EXECUTION LINEAGE"),
					)
					.child(execution_lineage_node(
						0,
						"PROJECT",
						project.clone(),
						WB_TEXT_FAINT,
						false,
					))
					.child(execution_lineage_node(
						1,
						"WORK ITEM",
						compact_identity(card.work_item_id().as_str()),
						work_item_state_color(state),
						false,
					))
					.child(execution_lineage_node(2, "CONVERSATION", conversation, WB_BLUE, false))
					.child(execution_lineage_node(3, "RUNTIME SESSION", runtime, WB_GREEN, true)),
			)
			.child(
				div()
					.w_full()
					.pt_3()
					.flex()
					.flex_col()
					.border_t_1()
					.border_color(rgba(0xffffff0c))
					.child(inspector_metadata_row("Project", project))
					.child(inspector_metadata_row("Priority", card.priority().as_str().to_owned()))
					.child(inspector_metadata_row("Revision", format!("r{}", card.revision().0)))
					.child(inspector_metadata_row("Relations", relation_count.to_string()))
					.child(inspector_metadata_row(
						"Work item",
						compact_identity(card.work_item_id().as_str()),
					))
					.when_some(card.conversation_id(), |element, conversation_id| {
						element.child(inspector_metadata_row(
							"Conversation",
							compact_identity(conversation_id.as_str()),
						))
					}),
			)
			.into_any_element()
	} else {
		div()
			.py_8()
			.flex()
			.flex_col()
			.items_center()
			.gap_3()
			.text_center()
			.child(
				div()
					.size(px(32.0))
					.flex()
					.items_center()
					.justify_center()
					.rounded(px(9.0))
					.border_1()
					.border_color(rgba(0xffffff12))
					.font_family("SF Mono")
					.text_size(px(10.0))
					.text_color(rgb(WB_TEXT_FAINT))
					.child("WI"),
			)
			.child(
				div()
					.text_size(px(11.0))
					.font_weight(FontWeight::MEDIUM)
					.text_color(rgb(WB_TEXT))
					.child("No Work Item bound"),
			)
			.child(
				div()
					.max_w(px(240.0))
					.text_size(px(9.5))
					.line_height(px(14.0))
					.text_color(rgb(WB_TEXT_FAINT))
					.child("This is an ordinary Codex conversation. Start managed work from Factory to bind product context."),
			)
			.into_any_element()
	};

	div().w_full().child(content).into_any_element()
}

fn activity_inspector_content(shell: &Shell) -> AnyElement {
	let task = shell.quick.selected_task();
	let task_state =
		task.map_or("No active conversation", |task| quick_task_state_label(task.state));
	let task_color = task.map_or(WB_TEXT_FAINT, |task| quick_task_state_color(task.state));
	let mut items = shell
		.history
		.as_ref()
		.and_then(|history| history.visible.as_ref())
		.into_iter()
		.flat_map(|page| page.items.iter())
		.rev()
		.take(8)
		.map(|item| {
			let summary = match &item.payload {
				HistoryPayloadDto::Inline { text } => text.as_str().to_owned(),
				HistoryPayloadDto::Blob(reference) => {
					format!("Stored content · {} bytes", reference.byte_length.get())
				},
			};
			(
				history_kind_label(item.kind, item.status).to_owned(),
				history_role_label(item.turn_role).to_owned(),
				summary,
				item.status,
			)
		})
		.collect::<Vec<_>>();
	items.reverse();
	let rows = items.into_iter().enumerate().map(|(index, (kind, role, summary, status))| {
		let color = match status {
			HistoryItemStatusDto::Streaming => WB_BLUE,
			HistoryItemStatusDto::Completed => WB_TEXT_FAINT,
			HistoryItemStatusDto::Failed => WB_AMBER,
		};
		div()
			.id(("inspector-activity", index))
			.w_full()
			.min_h(px(54.0))
			.flex()
			.gap_3()
			.child(
				div()
					.w(px(9.0))
					.min_w(px(9.0))
					.flex()
					.flex_col()
					.items_center()
					.child(div().mt(px(5.0)).size(px(5.0)).rounded_full().bg(rgb(color)))
					.when(index + 1 < 8, |element| {
						element.child(div().mt_1().w(px(1.0)).flex_1().bg(rgba(0xffffff0c)))
					}),
			)
			.child(
				div()
					.min_w_0()
					.flex_1()
					.pb_3()
					.flex()
					.flex_col()
					.gap_1()
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.gap_2()
							.text_size(px(8.5))
							.child(div().text_color(rgb(WB_TEXT_MUTED)).child(kind))
							.child(div().text_color(rgb(WB_TEXT_FAINT)).child(role)),
					)
					.child(
						div()
							.max_h(px(30.0))
							.overflow_hidden()
							.text_size(px(9.5))
							.line_height(px(14.0))
							.text_color(rgb(WB_TEXT_FAINT))
							.whitespace_normal()
							.child(summary),
					),
			)
	});

	div()
		.flex()
		.flex_col()
		.gap_4()
		.child(
			div()
				.h(px(34.0))
				.px_3()
				.flex()
				.items_center()
				.gap_2()
				.rounded(px(8.0))
				.bg(rgba(0xffffff07))
				.text_size(px(9.0))
				.text_color(rgb(WB_TEXT_MUTED))
				.child(div().size(px(5.0)).rounded_full().bg(rgb(task_color)))
				.child(task_state),
		)
		.when(
			shell.history.as_ref().and_then(|history| history.visible.as_ref()).is_none(),
			|element| {
				element.child(
					div()
						.py_6()
						.text_center()
						.text_size(px(9.5))
						.text_color(rgb(WB_TEXT_FAINT))
						.child("Activity appears after verified history readback."),
				)
			},
		)
		.children(rows)
		.into_any_element()
}

fn workbench_inspector(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let content = match shell.inspector_tab {
		InspectorTab::WorkItem => work_item_inspector_content(shell),
		InspectorTab::Activity => activity_inspector_content(shell),
	};

	div()
		.id("workbench-inspector")
		.role(Role::Complementary)
		.aria_label("Conversation context")
		.w(px(WORKBENCH_INSPECTOR_WIDTH))
		.min_w(px(WORKBENCH_INSPECTOR_WIDTH))
		.h_full()
		.flex()
		.flex_col()
		.border_l_1()
		.border_color(rgba(0xffffff0f))
		.bg(rgba(ui_theme::SIDEBAR_MATERIAL))
		.child(
			div()
				.h(px(44.0))
				.min_h(px(44.0))
				.px_3()
				.flex()
				.items_center()
				.justify_between()
				.border_b_1()
				.border_color(rgba(0xffffff0d))
				.child(
					div()
						.id("inspector-tabs")
						.role(Role::TabList)
						.aria_label("Inspector views")
						.p_1()
						.flex()
						.gap_1()
						.rounded(px(8.0))
						.bg(rgba(0x00000024))
						.child(inspector_tab(
							"inspector-work-item",
							"Work Item",
							InspectorTab::WorkItem,
							shell.inspector_tab,
							cx,
						))
						.child(inspector_tab(
							"inspector-activity-tab",
							"Activity",
							InspectorTab::Activity,
							shell.inspector_tab,
							cx,
						)),
				)
				.child(
					div()
						.id("open-factory")
						.role(Role::Button)
						.aria_label("Open Work Item in Factory")
						.h(px(26.0))
						.px_2()
						.flex()
						.items_center()
						.rounded(px(6.0))
						.border_1()
						.border_color(rgba(0xffffff10))
						.text_size(px(8.5))
						.text_color(rgb(WB_TEXT_MUTED))
						.cursor_pointer()
						.hover(|element| element.bg(rgba(0xffffff09)).text_color(rgb(WB_TEXT)))
						.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
						.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
						.on_click(cx.listener(|shell, _, _, cx| {
							shell.select_destination(Destination::Factory, cx);
						}))
						.child("Open Factory"),
				),
		)
		.child(
			div()
				.id("workbench-inspector-scroll")
				.flex_1()
				.min_h_0()
				.overflow_y_scroll()
				.p_4()
				.child(content),
		)
		.into_any_element()
}

fn quick_task_transcript(
	snapshot: &QuickTasksSnapshot,
	history: Option<&HistorySnapshot>,
	pending: Option<&PendingComposerSubmission>,
) -> AnyElement {
	let rows = quick_task_transcript_rows(snapshot, history, pending);
	let has_rows = !rows.is_empty();
	let rendered_rows = rows.into_iter().map(|row| {
		let content = match row {
			TranscriptRow::Prompt { text, pending, .. } => div()
				.w_full()
				.flex()
				.justify_end()
				.child(
					div()
						.max_w(px(620.0))
						.px_3()
						.py_2()
						.rounded(px(8.0))
						.bg(rgba(0xffffff0a))
						.border_1()
						.border_color(rgba(if pending { 0x7aa2ff32 } else { 0xffffff0b }))
						.text_size(px(11.0))
						.text_color(rgb(WB_TEXT))
						.whitespace_normal()
						.child(text),
				)
				.into_any_element(),
			TranscriptRow::Response { text, live, .. } => div()
				.w_full()
				.flex()
				.gap_3()
				.when(live, |element| {
					element.child(div().mt(px(5.0)).size(px(5.0)).rounded_full().bg(rgb(WB_ACCENT)))
				})
				.child(
					div()
						.flex_1()
						.min_w_0()
						.text_size(px(11.0))
						.text_color(rgb(WB_TEXT))
						.whitespace_normal()
						.child(text),
				)
				.into_any_element(),
			TranscriptRow::Activity { kind, status, text, .. } => div()
				.w_full()
				.h(px(26.0))
				.px_2()
				.flex()
				.items_center()
				.gap_2()
				.rounded(px(6.0))
				.text_size(px(9.0))
				.text_color(rgb(WB_TEXT_FAINT))
				.child(
					div()
						.font_family("SF Mono")
						.text_size(px(8.5))
						.text_color(rgb(if status == HistoryItemStatusDto::Failed {
							WB_AMBER
						} else {
							WB_TEXT_FAINT
						}))
						.child(history_kind_label(kind, status)),
				)
				.child(
					div()
						.min_w_0()
						.overflow_hidden()
						.whitespace_nowrap()
						.text_ellipsis()
						.child(text),
				)
				.into_any_element(),
		};

		div()
			.w_full()
			.py_2()
			.flex()
			.justify_center()
			.child(div().w_full().max_w(px(760.0)).child(content))
	});
	let history_status = history.map_or_else(
		|| (!has_rows).then_some("Conversation history is not connected."),
		|history| match history.load {
			HistoryLoadState::Inactive =>
				(!has_rows).then_some("Select a conversation or start a new conversation."),
			HistoryLoadState::InitialLoading | HistoryLoadState::RefreshingVisible =>
				Some(if has_rows {
					"Syncing earlier context"
				} else {
					"Loading conversation history"
				}),
			HistoryLoadState::PrefetchingAdjacent | HistoryLoadState::Visible => None,
			HistoryLoadState::RetryableUnavailable(_) =>
				Some("History is temporarily unavailable. Reconnect or retry."),
			HistoryLoadState::ClosedUnavailable(_) => Some("History readback was refused."),
		},
	);

	div()
		.id("quick-task-transcript")
		.role(Role::Log)
		.aria_label("Quick Task conversation")
		.flex_1()
		.min_h_0()
		.overflow_y_scroll()
		.px_5()
		.py_5()
		.when_some(history_status, |element, history_status| {
			element.child(
				div().w_full().flex().justify_center().child(
					div()
						.w_full()
						.max_w(px(760.0))
						.py_3()
						.text_size(px(10.0))
						.text_color(rgb(WB_TEXT_FAINT))
						.child(history_status),
				),
			)
		})
		.children(rendered_rows)
		.into_any_element()
}

fn history_page_controls(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let can_previous = shell.history.as_ref().is_some_and(|history| history.can_show_previous);
	let can_next = shell.history.as_ref().is_some_and(|history| history.can_show_next);
	let can_retry = shell.history.as_ref().is_some_and(|history| history.can_retry);
	if !can_previous && !can_next && !can_retry {
		return div().w(px(0.0)).into_any_element();
	}
	let previous = div()
		.id("quick-task-history-previous")
		.role(Role::Button)
		.aria_label("Show less conversation history")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Show less history")).into())
		.h(px(24.0))
		.px_2()
		.flex()
		.items_center()
		.justify_center()
		.rounded_sm()
		.text_size(px(8.5))
		.text_color(if can_previous { rgb(WB_TEXT_MUTED) } else { rgb(WB_TEXT_FAINT) })
		.when(can_previous, |element| {
			element.cursor_pointer().hover(|element| element.bg(rgb(0x25324a))).on_click(
				cx.listener(|shell, _, window, cx| {
					shell.show_previous_history(window, cx);
				}),
			)
		})
		.child("Earlier");
	let retry = div()
		.id("quick-task-history-retry")
		.role(Role::Button)
		.aria_label("Retry conversation history")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Retry conversation history")).into())
		.h(px(24.0))
		.px_2()
		.flex()
		.items_center()
		.justify_center()
		.rounded_sm()
		.text_size(px(8.5))
		.text_color(if can_retry { rgb(WB_TEXT_MUTED) } else { rgb(WB_TEXT_FAINT) })
		.when(can_retry, |element| {
			element.cursor_pointer().hover(|element| element.bg(rgb(0x25324a))).on_click(
				cx.listener(|shell, _, window, cx| {
					shell.retry_history(window, cx);
				}),
			)
		})
		.child("Retry");
	let next = div()
		.id("quick-task-history-next")
		.role(Role::Button)
		.aria_label("Load more conversation history")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Load more history")).into())
		.h(px(24.0))
		.px_2()
		.flex()
		.items_center()
		.justify_center()
		.rounded_sm()
		.text_size(px(8.5))
		.text_color(if can_next { rgb(WB_TEXT_MUTED) } else { rgb(WB_TEXT_FAINT) })
		.when(can_next, |element| {
			element.cursor_pointer().hover(|element| element.bg(rgb(0x25324a))).on_click(
				cx.listener(|shell, _, window, cx| {
					shell.show_next_history(window, cx);
				}),
			)
		})
		.child("Later");

	div()
		.min_w(px(126.0))
		.flex()
		.items_center()
		.justify_between()
		.child(previous)
		.child(retry)
		.child(next)
		.into_any_element()
}

fn quick_task_composer(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let task = shell.quick.selected_task();
	let (has_executable_recovery, recovery_label) = quick_task_recovery_presentation(task);
	let can_continue = shell.creating_new
		|| task.is_none()
		|| task.is_some_and(|task| task.state == QuickTaskState::Ready);
	let composer = shell.composer.read(cx);
	let composer_len = composer.len();
	let has_message = !composer.content().trim().is_empty();
	let can_send = shell.quick.can_submit && can_continue && has_message;
	let can_recover = shell.quick.can_submit && has_executable_recovery;
	let can_interrupt =
		shell.quick.can_submit && task.is_some_and(|task| task.state == QuickTaskState::Running);
	let model_label = shell.quick.execution.model.as_str().to_owned();
	let effort_label = shell.quick.execution.reasoning_effort.as_str().to_uppercase();
	let fast_enabled = shell.quick.execution.fast;

	let send = div()
		.id("quick-task-send")
		.role(Role::Button)
		.aria_label("Send message")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Send message")).into())
		.h(px(23.0))
		.min_h(px(23.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(7.0))
		.bg(if can_send { rgb(WB_TEXT) } else { rgba(0xffffff08) })
		.text_size(px(9.5))
		.font_weight(FontWeight::SEMIBOLD)
		.text_color(if can_send { rgb(WB_CANVAS) } else { rgb(WB_TEXT_FAINT) })
		.when(can_send, |element| {
			element
				.cursor_pointer()
				.hover(|element| element.opacity(0.9))
				.active(|element| element.opacity(0.72))
				.focus_visible(|element| element.border_1().border_color(rgb(WB_BLUE)))
				.on_click(cx.listener(|shell, _, window, cx| {
					shell.submit_quick_task(window, cx);
				}))
		})
		.child("Send");
	let interrupt = div()
		.id("quick-task-interrupt")
		.role(Role::Button)
		.aria_label("Interrupt active turn")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Interrupt active turn")).into())
		.h(px(23.0))
		.min_h(px(23.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(7.0))
		.border_1()
		.border_color(rgba(0xffffff12))
		.text_size(px(9.5))
		.text_color(if can_interrupt { rgb(WB_TEXT_MUTED) } else { rgb(WB_TEXT_FAINT) })
		.when(can_interrupt, |element| {
			element
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0a)))
				.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
				.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
				.on_click(cx.listener(|shell, _, window, cx| {
					shell.interrupt_quick_task(window, cx);
				}))
		})
		.child("Stop");
	let recover = div()
		.id("quick-task-recover")
		.role(Role::Button)
		.aria_label(recovery_label)
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Run the explicit recovery action")).into())
		.h(px(23.0))
		.min_h(px(23.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(7.0))
		.border_1()
		.border_color(if can_recover { rgba(0xf59e0b55) } else { rgba(0xffffff10) })
		.text_size(px(9.5))
		.text_color(if can_recover { rgb(WB_AMBER) } else { rgb(WB_TEXT_FAINT) })
		.when(can_recover, |element| {
			element
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xf59e0b12)))
				.active(|element| element.opacity(0.72))
				.on_click(cx.listener(|shell, _, window, cx| {
					shell.recover_quick_task(window, cx);
				}))
		})
		.child(recovery_label);
	let model_control = div()
		.id("quick-task-model")
		.role(Role::Button)
		.aria_label(format!("Model {model_label}; select next model"))
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Model · click to cycle")).into())
		.h(px(23.0))
		.px_2()
		.flex()
		.items_center()
		.rounded(px(6.0))
		.border_1()
		.border_color(rgba(0xffffff10))
		.bg(rgba(0x00000018))
		.font_family("SF Mono")
		.text_size(px(8.0))
		.text_color(rgb(WB_TEXT_MUTED))
		.cursor_pointer()
		.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
		.active(|element| element.opacity(0.72))
		.on_click(cx.listener(|shell, _, _, cx| shell.cycle_quick_task_model(cx)))
		.child(model_label);
	let fast_control = div()
		.id("quick-task-fast")
		.role(Role::Button)
		.aria_label(if fast_enabled { "Fast mode on" } else { "Fast mode off" })
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Fast · priority service tier")).into())
		.h(px(23.0))
		.px_2()
		.flex()
		.items_center()
		.gap_1()
		.rounded(px(6.0))
		.border_1()
		.border_color(if fast_enabled { rgba(0xffa45d40) } else { rgba(0xffffff10) })
		.bg(if fast_enabled { rgba(0xff8a3d16) } else { rgba(0x00000018) })
		.font_family("SF Mono")
		.text_size(px(8.0))
		.text_color(if fast_enabled { rgb(WB_AMBER) } else { rgb(WB_TEXT_MUTED) })
		.cursor_pointer()
		.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
		.active(|element| element.opacity(0.72))
		.on_click(cx.listener(|shell, _, _, cx| shell.toggle_quick_task_fast(cx)))
		.child(div().size(px(4.0)).rounded_full().bg(if fast_enabled {
			rgb(WB_AMBER)
		} else {
			rgb(WB_TEXT_FAINT)
		}))
		.child("Fast");
	let effort_control = div()
		.id("quick-task-effort")
		.role(Role::Button)
		.aria_label(format!("Reasoning effort {effort_label}; select next effort"))
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Reasoning effort · click to cycle")).into())
		.h(px(23.0))
		.px_2()
		.flex()
		.items_center()
		.rounded(px(6.0))
		.border_1()
		.border_color(rgba(0xffffff10))
		.bg(rgba(0x00000018))
		.font_family("SF Mono")
		.text_size(px(8.0))
		.text_color(rgb(WB_TEXT_MUTED))
		.cursor_pointer()
		.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
		.active(|element| element.opacity(0.72))
		.on_click(cx.listener(|shell, _, _, cx| shell.cycle_quick_task_effort(cx)))
		.child(effort_label);
	div()
		.min_h(px(88.0))
		.px_5()
		.pt_1()
		.pb_3()
		.flex()
		.justify_center()
		.child(
			div()
				.w_full()
				.max_w(px(780.0))
				.p_1()
				.flex()
				.flex_col()
				.rounded(px(11.0))
				.border_1()
				.border_color(rgba(0xffffff16))
				.bg(rgba(ui_theme::COMPOSER_MATERIAL))
				.shadow(vec![
					BoxShadow::new(px(0.0), px(10.0), Hsla { h: 0.0, s: 0.0, l: 0.0, a: 0.22 })
						.blur_radius(px(28.0))
						.spread_radius(px(-10.0)),
				])
				.child(div().h(px(35.0)).min_h(px(35.0)).child(shell.composer.clone()))
				.child(
					div()
						.h(px(27.0))
						.px_1()
						.flex()
						.items_center()
						.justify_between()
						.child(
							div()
								.min_w_0()
								.flex()
								.items_center()
								.gap_1()
								.child(model_control)
								.child(fast_control)
								.child(effort_control),
						)
						.child(
							div()
								.flex()
								.items_center()
								.gap_2()
								.when(composer_len > 0, |element| {
									element.child(
										div()
											.font_family("SF Mono")
											.text_size(px(7.5))
											.text_color(rgb(WB_TEXT_FAINT))
											.child(format!("{composer_len}/{MAX_COMPOSER_BYTES}")),
									)
								})
								.child(interrupt)
								.when(has_executable_recovery, |element| element.child(recover))
								.child(send),
						),
				),
		)
		.into_any_element()
}

fn quick_tasks_content(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let selected_task = shell.quick.selected_task();
	let state_label = if shell.creating_new {
		"New conversation"
	} else {
		selected_task.map_or("No conversation selected", |task| quick_task_state_label(task.state))
	};
	let state_color = selected_task.map_or(WB_BLUE, |task| quick_task_state_color(task.state));
	let detail = shell
		.input_status
		.as_ref()
		.map(SharedString::to_string)
		.or_else(|| command_status(shell.quick.command).map(str::to_owned))
		.or_else(|| {
			selected_task
				.is_some_and(|task| task.state == QuickTaskState::OutcomeUnknown)
				.then(|| "Checking the interrupted turn before continuing.".to_owned())
		})
		.or_else(|| {
			selected_task
				.and_then(|task| task.recovery_action)
				.map(recovery_action_label)
				.map(str::to_owned)
		})
		.unwrap_or_else(|| quick_task_load_status(shell.quick.load).to_owned());

	div()
		.flex_1()
		.min_w_0()
		.min_h_0()
		.flex()
		.when(shell.left_sidebar_mounted, |content| {
			content.child(animated_horizontal_panel_slot(
				"quick-task-sidebar-motion",
				shell.left_sidebar_visible,
				shell.left_sidebar_motion_generation,
				WORKBENCH_SESSION_SIDEBAR_WIDTH,
				quick_task_session_sidebar(shell, cx),
			))
		})
		.child(
			div()
				.flex_1()
				.min_w_0()
				.min_h_0()
				.flex()
				.child(
					div()
						.flex_1()
						.min_w_0()
						.min_h_0()
						.flex()
						.flex_col()
						.bg(rgba(ui_theme::CONTENT_MATERIAL))
						.child(
							div()
								.h(px(44.0))
								.min_h(px(44.0))
								.px_5()
								.flex()
								.items_center()
								.gap_3()
								.border_b_1()
								.border_color(rgba(0xffffff0d))
								.child(div().size(px(6.0)).rounded_full().bg(rgb(state_color)))
								.child(
									div()
										.text_size(px(10.0))
										.font_weight(FontWeight::MEDIUM)
										.text_color(rgb(WB_TEXT))
										.child(state_label),
								)
								.child(
									div()
										.flex_1()
										.min_w_0()
										.overflow_hidden()
										.whitespace_nowrap()
										.text_ellipsis()
										.text_size(px(9.0))
										.text_color(rgb(WB_TEXT_FAINT))
										.child(detail),
								)
								.child(history_page_controls(shell, cx)),
						)
						.child(quick_task_transcript(
							&shell.quick,
							shell.history.as_ref(),
							shell.pending_submission.as_ref(),
						))
						.child(quick_task_composer(shell, cx)),
				)
				.when(shell.inspector_mounted, |content| {
					content.child(animated_horizontal_panel_slot(
						"workbench-inspector-motion",
						shell.inspector_visible,
						shell.inspector_motion_generation,
						WORKBENCH_INSPECTOR_WIDTH,
						workbench_inspector(shell, cx),
					))
				}),
		)
		.into_any_element()
}

fn health_content(snapshot: &HealthSnapshot) -> AnyElement {
	let presentation = health_presentation(snapshot);

	div()
		.id("health-scroll-viewport")
		.flex_1()
		.min_h_0()
		.overflow_y_scroll()
		.px_7()
		.py_5()
		.child(
			div()
				.id("health-query-status")
				.role(Role::Status)
				.aria_label(format!("Health report: {}", presentation.label))
				.h(px(56.0))
				.min_h(px(56.0))
				.px_4()
				.flex()
				.items_center()
				.gap_3()
				.rounded(px(12.0))
				.border_1()
				.border_color(rgba(0xffffff10))
				.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
				.child(
					div().size(px(10.0)).min_w(px(10.0)).rounded_full().bg(rgb(presentation.color)),
				)
				.child(
					div()
						.w(px(144.0))
						.min_w(px(144.0))
						.text_size(px(11.0))
						.font_weight(FontWeight::SEMIBOLD)
						.child(presentation.label),
				)
				.child(
					div()
						.min_w_0()
						.text_size(px(10.0))
						.text_color(rgb(WB_TEXT_MUTED))
						.child(presentation.detail),
				),
		)
		.child(
			div()
				.id("health-components")
				.pt_5()
				.flex()
				.flex_col()
				.gap_5()
				.child(health_component_section(
					"health-core-components",
					"Core services",
					"Required for normal operation",
					0,
					&HEALTH_CORE_COMPONENTS,
					snapshot,
				))
				.child(health_component_section(
					"health-app-server-components",
					"Codex app-server",
					"Capabilities are reported only after an active probe",
					HEALTH_CORE_COMPONENTS.len(),
					&HEALTH_APP_SERVER_COMPONENTS,
					snapshot,
				))
				.child(health_component_section(
					"health-optional-components",
					"Optional capabilities",
					"Disabled or unconfigured entries do not block Decodex",
					HEALTH_CORE_COMPONENTS.len() + HEALTH_APP_SERVER_COMPONENTS.len(),
					&HEALTH_OPTIONAL_COMPONENTS,
					snapshot,
				)),
		)
		.into_any_element()
}

fn connection_status(presentation: ConnectionPresentation) -> AnyElement {
	div()
		.id("connection-status")
		.role(Role::Status)
		.aria_label(format!("Connection: {}", presentation.label))
		.h(px(42.0))
		.min_h(px(42.0))
		.px_6()
		.flex()
		.items_center()
		.gap_3()
		.border_t_1()
		.border_color(rgba(0xffffff0d))
		.bg(rgba(0x00000016))
		.font_family("SF Mono")
		.text_size(px(8.0))
		.text_color(rgb(WB_TEXT_FAINT))
		.child(div().size(px(6.0)).rounded_full().bg(rgb(presentation.color)))
		.child(div().w(px(110.0)).min_w(px(110.0)).child(presentation.label))
		.child(
			div()
				.min_w_0()
				.overflow_hidden()
				.whitespace_nowrap()
				.text_ellipsis()
				.child(presentation.detail),
		)
		.into_any_element()
}

fn destination_content(
	shell: &Shell,
	presentation: ConnectionPresentation,
	refresh_focus: FocusHandle,
	window: &Window,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let selected = shell.selected;
	match selected {
		Destination::QuickTasks => {
			return div()
				.id("destination-content")
				.role(Role::Main)
				.aria_label("Codex Workbench")
				.flex_1()
				.min_w_0()
				.min_h_0()
				.flex()
				.flex_col()
				.child(quick_tasks_content(shell, cx))
				.into_any_element();
		},
		Destination::Factory => {
			return div()
				.id("destination-content")
				.role(Role::Main)
				.aria_label("Codex Factory")
				.flex_1()
				.min_w_0()
				.min_h_0()
				.flex()
				.bg(rgba(ui_theme::CONTENT_MATERIAL))
				.child(shell.factory.clone())
				.into_any_element();
		},
		Destination::Settings => {
			return div()
				.id("destination-content")
				.role(Role::Main)
				.aria_label("Decodex settings")
				.flex_1()
				.min_w_0()
				.min_h_0()
				.flex()
				.bg(rgba(ui_theme::CONTENT_MATERIAL))
				.child(shell.settings.clone())
				.into_any_element();
		},
		_ => {},
	}

	let content = if selected == Destination::Health {
		health_content(&shell.health)
	} else if selected == Destination::Accounts {
		accounts_content(shell, cx)
	} else {
		placeholder_content(selected)
	};
	div()
		.id("destination-content")
		.role(Role::Main)
		.aria_label(format!("{} destination", selected.label()))
		.flex_1()
		.min_w_0()
		.h_full()
		.flex()
		.flex_col()
		.bg(rgba(ui_theme::CONTENT_MATERIAL))
		.child(destination_header(selected, &shell.health, refresh_focus, window, cx))
		.child(content)
		.child(connection_status(presentation))
		.into_any_element()
}

impl Render for Shell {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let presentation = connection_presentation(self.connection);
		let root = div()
			.id("decodex-shell")
			.role(Role::Application)
			.aria_label("Decodex operational shell")
			.track_focus(&self.root_focus)
			.on_action(cx.listener(Self::focus_next))
			.on_action(cx.listener(Self::focus_previous))
			.on_action(cx.listener(Self::activate_factory))
			.on_action(cx.listener(Self::activate_quick_tasks))
			.on_action(cx.listener(Self::activate_health))
			.on_action(cx.listener(Self::toggle_sidebar))
			.on_action(cx.listener(Self::toggle_inspector))
			.on_action(cx.listener(Self::submit_composer))
			.size_full()
			.min_w(px(1180.0))
			.min_h(px(720.0))
			.flex()
			.flex_col()
			.bg(rgba(ui_theme::SHELL_MATERIAL))
			.text_color(rgb(WB_TEXT));

		root.child(workbench_topbar(self, &presentation, window, cx)).child(destination_content(
			self,
			presentation,
			self.refresh_focus.clone(),
			window,
			cx,
		))
	}
}

#[cfg(test)]
mod tests {
	use gpui::{TestAppContext, VisualTestContext, size};

	use super::*;
	use crate::{
		client_lifecycle::{CompatibilityReason, QuarantineReason, QuarantineRecovery},
		work_items::WorkItemsLoadState,
	};

	fn open_shell(cx: &mut TestAppContext) -> (gpui::Entity<Shell>, &mut VisualTestContext) {
		cx.update(bind_keys);
		cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped))
	}

	#[test]
	fn destinations_have_exact_labels_and_live_classification() {
		assert_eq!(
			Destination::ALL.map(|destination| (
				destination.label(),
				matches!(
					destination,
					Destination::Factory
						| Destination::QuickTasks
						| Destination::Accounts
						| Destination::Health
						| Destination::Settings
				),
			)),
			[
				("Factory", true),
				("Advisor", false),
				("Projects", false),
				("Quick Tasks", true),
				("Runs", false),
				("Automations", false),
				("Accounts", true),
				("Health", true),
				("Settings", true),
			]
		);
	}

	#[test]
	fn composer_clears_only_after_exact_submission_acceptance() {
		let conversation_id = EntityId::new("10000000-0000-4000-8000-000000000080")
			.expect("test conversation identity is canonical");
		let pending = PendingComposerSubmission {
			content: "Keep this draft until accepted.".to_owned(),
			result_generation: 7,
			conversation_id,
			turn_id: Some(
				EntityId::new("20000000-0000-4000-8000-000000000080")
					.expect("test turn identity is canonical"),
			),
			accepted: false,
		};

		assert_eq!(
			pending_submission_clear_decision(&pending, 7, false, &pending.content),
			None,
			"queueing or waiting for a result must retain the composer"
		);
		assert_eq!(
			pending_submission_clear_decision(&pending, 8, false, &pending.content),
			Some(false),
			"an archived-thread rejection must retain the composer"
		);
		assert_eq!(
			pending_submission_clear_decision(&pending, 8, true, "A newer user edit"),
			Some(false),
			"a later accepted result must not erase newer typing"
		);
		assert_eq!(
			pending_submission_clear_decision(&pending, 8, true, &pending.content),
			Some(true),
			"only exact accepted content is safe to clear"
		);
	}

	fn transcript_snapshot(
		conversation_id: &EntityId,
		live_deltas: Vec<crate::quick_tasks::QuickTaskLiveDelta>,
	) -> QuickTasksSnapshot {
		QuickTasksSnapshot {
			load: QuickTasksLoadState::Ready,
			command: QuickTaskCommandState::AwaitingResult,
			submission_result_generation: 0,
			last_submission_accepted: false,
			refresh: QuickTaskRefreshState::Idle,
			tasks: Vec::new(),
			selected: Some(conversation_id.clone()),
			live_deltas,
			can_submit: false,
			execution: decodex_protocol::QuickTaskExecutionSettings::new(
				decodex_protocol::QuickTaskModel::new("gpt-5.6-sol").expect("test model is valid"),
				decodex_protocol::QuickTaskReasoningEffort::High,
				false,
			),
		}
	}

	fn history_item(
		history_item_id: &str,
		turn_id: &str,
		role: &str,
		text: &str,
	) -> HistoryItemDto {
		serde_json::from_value(serde_json::json!({
			"history_item_id": history_item_id,
			"turn_id": turn_id,
			"runtime_session_id": "30000000-0000-4000-8000-000000000080",
			"turn_role": role,
			"possible_side_effects": "none",
			"kind": "message",
			"status": "completed",
			"payload": {"kind": "inline", "data": {"text": text}},
			"media_type": "text/markdown",
			"metadata": {},
			"revision": 1
		}))
		.expect("test history item is valid")
	}

	fn history_snapshot(
		conversation_id: &EntityId,
		items: Vec<HistoryItemDto>,
		load: HistoryLoadState,
		source: Option<HistoryPageSource>,
	) -> HistorySnapshot {
		HistorySnapshot {
			conversation_id: Some(conversation_id.clone()),
			view_generation: 1,
			load,
			visible: source
				.map(|_| decodex_protocol::ConversationHistoryPage { items, next_cursor: None }),
			visible_source: source,
			next_cursor: None,
			cursor: crate::history_pager::HistoryCursorObservation::NoContinuationObserved,
			cache_diagnostic: None,
			retained_pages: 1,
			retained_items: 0,
			retained_bytes: 0,
			can_show_previous: false,
			can_show_next: false,
			can_retry: false,
			last_stale_cancellation: None,
		}
	}

	#[test]
	fn pending_prompt_is_visible_before_the_daemon_command_finishes() {
		let conversation_id = EntityId::new("10000000-0000-4000-8000-000000000081")
			.expect("test conversation identity is canonical");
		let turn_id = EntityId::new("20000000-0000-4000-8000-000000000081")
			.expect("test turn identity is canonical");
		let snapshot = transcript_snapshot(&conversation_id, Vec::new());
		let pending = PendingComposerSubmission {
			content: "Show this immediately.".to_owned(),
			result_generation: 0,
			conversation_id,
			turn_id: Some(turn_id.clone()),
			accepted: false,
		};

		assert_eq!(
			quick_task_transcript_rows(&snapshot, None, Some(&pending)),
			vec![TranscriptRow::Prompt {
				turn_id: Some(turn_id),
				text: "Show this immediately.".to_owned(),
				pending: true,
			}]
		);
	}

	#[test]
	fn assistant_chunks_coalesce_into_one_response_per_turn() {
		let conversation_id = EntityId::new("10000000-0000-4000-8000-000000000082")
			.expect("test conversation identity is canonical");
		let live_turn = EntityId::new("20000000-0000-4000-8000-000000000083")
			.expect("test live turn identity is canonical");
		let live = |item: &str, text: &str| crate::quick_tasks::QuickTaskLiveDelta {
			history_item_id: EntityId::new(item).expect("test history identity is canonical"),
			conversation_id: conversation_id.clone(),
			turn_id: live_turn.clone(),
			text: decodex_protocol::HistoryText::new(text).expect("test live delta is bounded"),
		};
		let snapshot = transcript_snapshot(
			&conversation_id,
			vec![
				live("40000000-0000-4000-8000-000000000083", "Streaming "),
				live("40000000-0000-4000-8000-000000000084", "response."),
			],
		);
		let history = history_snapshot(
			&conversation_id,
			vec![
				history_item(
					"40000000-0000-4000-8000-000000000081",
					"20000000-0000-4000-8000-000000000082",
					"assistant",
					"Durable ",
				),
				history_item(
					"40000000-0000-4000-8000-000000000082",
					"20000000-0000-4000-8000-000000000082",
					"assistant",
					"response.",
				),
			],
			HistoryLoadState::Visible,
			Some(HistoryPageSource::FreshServer),
		);

		assert_eq!(
			quick_task_transcript_rows(&snapshot, Some(&history), None),
			vec![
				TranscriptRow::Response {
					turn_id: EntityId::new("20000000-0000-4000-8000-000000000082")
						.expect("test turn identity is canonical"),
					text: "Durable response.".to_owned(),
					live: false,
				},
				TranscriptRow::Response {
					turn_id: live_turn,
					text: "Streaming response.".to_owned(),
					live: true,
				},
			]
		);
	}

	#[test]
	fn provider_refresh_waits_until_fresh_local_history_is_visible() {
		let conversation_id = EntityId::new("10000000-0000-4000-8000-000000000084")
			.expect("test conversation identity is canonical");
		let loading =
			history_snapshot(&conversation_id, Vec::new(), HistoryLoadState::InitialLoading, None);
		let cached = history_snapshot(
			&conversation_id,
			Vec::new(),
			HistoryLoadState::RefreshingVisible,
			Some(HistoryPageSource::CachedUnverified),
		);
		let fresh = history_snapshot(
			&conversation_id,
			Vec::new(),
			HistoryLoadState::Visible,
			Some(HistoryPageSource::FreshServer),
		);

		assert!(!deferred_provider_refresh_ready(
			Some(&conversation_id),
			Some(&conversation_id),
			Some(&loading)
		));
		assert!(!deferred_provider_refresh_ready(
			Some(&conversation_id),
			Some(&conversation_id),
			Some(&cached)
		));
		assert!(deferred_provider_refresh_ready(
			Some(&conversation_id),
			Some(&conversation_id),
			Some(&fresh)
		));
	}

	#[test]
	fn outcome_unknown_offers_safe_readback_instead_of_discarding_the_conversation() {
		let task = QuickTaskSummary::new(
			EntityId::new("10000000-0000-4000-8000-000000000091")
				.expect("test conversation identity is canonical"),
			decodex_protocol::EntityRevision(3),
			1_786_000_000_000_000,
			Some(
				EntityId::new("20000000-0000-4000-8000-000000000091")
					.expect("test runtime identity is canonical"),
			),
			Some(decodex_protocol::EntityRevision(4)),
			QuickTaskState::OutcomeUnknown,
			None,
			None,
		)
		.expect("outcome-unknown projection is valid");

		assert_eq!(quick_task_recovery_presentation(Some(&task)), (true, "Retry sync"));
		assert_eq!(quick_task_recovery_presentation(None), (false, "Recover"));
	}

	#[test]
	fn every_connection_state_has_a_bounded_deterministic_presentation() {
		let states = [
			ConnectionView::Connecting { attempt: 2 },
			ConnectionView::Online { generation: 4, applied: Some(decodex_protocol::Cursor(9)) },
			ConnectionView::OfflineRetrying { next_attempt: 3, delay: Duration::from_millis(250) },
			ConnectionView::Incompatible(CompatibilityReason::ProtocolMajor),
			ConnectionView::Quarantined {
				reason: QuarantineReason::StableServerIdentity,
				recovery: QuarantineRecovery::OperatorRequired,
			},
			ConnectionView::ShuttingDown,
			ConnectionView::Stopped,
		];
		let labels = states.map(|state| connection_presentation(state).label);

		assert_eq!(
			labels,
			[
				"Connecting",
				"Online",
				"Offline · retrying",
				"Incompatible",
				"Quarantined · identity mismatch",
				"Shutting down",
				"Stopped",
			]
		);
	}

	#[test]
	fn startup_failures_preserve_their_typed_presentation() {
		let cases = [
			(ClientFailure::ConfigurationMissing, "Client configuration is missing"),
			(ClientFailure::ConfigurationMalformed, "Client configuration is malformed"),
			(ClientFailure::UnsafeHostPath, "Client configuration path is unsafe"),
			(ClientFailure::ProfileMissing, "Selected server profile is missing"),
			(ClientFailure::ServerIdentityUnavailable, "Stable server identity is unavailable"),
		];

		for (failure, detail) in cases {
			let view = ConnectionView::Incompatible(CompatibilityReason::Startup(failure));
			assert_eq!(connection_presentation(view).detail, detail);
		}
	}

	#[test]
	fn account_quota_renders_only_a_current_provider_window() {
		let quota = |result| AccountQuotaWindowDto {
			duration_minutes: 300,
			observed_at_unix_micros: None,
			result,
		};

		assert!(account_quota("5 HOUR", quota(AccountQuotaStateDto::Unknown)).is_none());
		assert!(
			account_quota(
				"5 HOUR",
				AccountQuotaWindowDto {
					observed_at_unix_micros: Some(1),
					result: AccountQuotaStateDto::Error {
						error: decodex_protocol::AccountQuotaErrorDto::UnsupportedWindow,
					},
					..quota(AccountQuotaStateDto::Unknown)
				},
			)
			.is_none()
		);
		assert!(
			account_quota(
				"5 HOUR",
				AccountQuotaWindowDto {
					observed_at_unix_micros: Some(1),
					result: AccountQuotaStateDto::Current {
						used_percent: 42,
						resets_at_unix_micros: 2,
					},
					..quota(AccountQuotaStateDto::Unknown)
				},
			)
			.is_some()
		);
	}

	#[test]
	fn quarantine_labels_describe_the_actual_typed_reason() {
		let cases = [
			(QuarantineReason::StableServerIdentity, "Quarantined · identity mismatch"),
			(QuarantineReason::CacheCorrupt, "Quarantined · cache integrity"),
			(QuarantineReason::ApplicationOrder, "Quarantined · application order"),
			(QuarantineReason::ApplicationConfirmation, "Quarantined · confirmation failure"),
			(QuarantineReason::StaleConnectionGeneration, "Quarantined · generation fence"),
		];

		for (reason, label) in cases {
			let view = ConnectionView::Quarantined {
				reason,
				recovery: QuarantineRecovery::OperatorRequired,
			};
			assert_eq!(connection_presentation(view).label, label);
		}
	}

	#[test]
	fn health_distinguishes_core_readiness_from_deferred_capabilities() {
		let checks = DoctorComponent::ALL
			.into_iter()
			.map(|component| {
				let status = match component {
					DoctorComponent::AppServerCapability(_) | DoctorComponent::BlobIntegrity =>
						DoctorStatus::Unknown(DoctorIssue::NotProbed),
					DoctorComponent::ManagedRepository =>
						DoctorStatus::Unavailable(DoctorIssue::Disabled),
					DoctorComponent::PluginReadiness => DoctorStatus::Unknown(DoctorIssue::Plugin),
					_ => DoctorStatus::Ready,
				};
				decodex_protocol::DoctorCheck::new(component, status)
			})
			.collect();
		let snapshot = HealthSnapshot {
			load: HealthLoadState::Ready,
			report: Some(
				decodex_protocol::DoctorReport::new(
					decodex_protocol::ServerId::new("health-ui-test")
						.expect("test server identity is valid"),
					decodex_protocol::CURRENT_VERSION,
					checks,
				)
				.expect("complete health report is valid"),
			),
			can_refresh: true,
		};

		assert_eq!(health_presentation(&snapshot).label, "Core ready");
		assert_eq!(
			component_presentation(Some(DoctorStatus::Unknown(DoctorIssue::NotProbed))).label,
			"Not checked"
		);
		assert_eq!(
			component_presentation(Some(DoctorStatus::Unavailable(DoctorIssue::Disabled))).label,
			"Disabled"
		);
		assert_eq!(
			component_presentation(Some(DoctorStatus::Unknown(DoctorIssue::Plugin))).label,
			"Not configured"
		);
	}

	#[gpui::test]
	fn keyboard_focus_and_activation_cover_rendered_workbench_destinations(
		cx: &mut TestAppContext,
	) {
		let (shell, visual) = open_shell(cx);
		for expected in Destination::CHROME {
			let focused = shell.read_with(visual, |shell, _| {
				let index = Destination::ALL
					.iter()
					.position(|value| *value == expected)
					.expect("test operation must succeed");
				shell.destination_focus[index].clone()
			});
			shell.update(visual, |shell, cx| shell.select_destination(Destination::Advisor, cx));
			visual.update(|window, cx| window.focus(&focused, cx));
			assert!(visual.update(|window, _| focused.is_focused(window)));
			visual.simulate_keystrokes("enter");
			assert_eq!(shell.read_with(visual, |shell, _| shell.selected), expected);
		}
	}

	#[gpui::test]
	fn global_workspace_shortcuts_keep_factory_quick_tasks_and_health_reachable(
		cx: &mut TestAppContext,
	) {
		let (shell, visual) = open_shell(cx);
		for (keys, expected) in [
			("cmd-2", Destination::QuickTasks),
			("cmd-3", Destination::Health),
			("cmd-1", Destination::Factory),
		] {
			visual.simulate_keystrokes(keys);
			assert_eq!(shell.read_with(visual, |shell, _| shell.selected), expected);
		}
	}

	#[gpui::test]
	fn workbench_does_not_activate_the_deferred_factory_protocol(cx: &mut TestAppContext) {
		let (shell, _visual) = open_shell(cx);

		assert_eq!(
			shell.read_with(_visual, |shell, _| shell.work.load),
			WorkItemsLoadState::NeverRequested,
			"the default conversation surface must not send Factory-only queries"
		);
	}

	#[gpui::test]
	fn panel_shortcuts_toggle_both_workbench_sidebars(cx: &mut TestAppContext) {
		let (shell, visual) = open_shell(cx);
		assert!(shell.read_with(visual, |shell, _| shell.left_sidebar_visible));
		assert!(shell.read_with(visual, |shell, _| shell.inspector_visible));

		visual.simulate_keystrokes("cmd-b");
		visual.simulate_keystrokes("cmd-shift-b");

		assert!(!shell.read_with(visual, |shell, _| shell.left_sidebar_visible));
		assert!(!shell.read_with(visual, |shell, _| shell.inspector_visible));
		assert!(shell.read_with(visual, |shell, _| shell.left_sidebar_mounted));
		assert!(shell.read_with(visual, |shell, _| shell.inspector_mounted));

		visual.executor().advance_clock(ui_theme::MOTION_PANEL + Duration::from_millis(24));
		visual.run_until_parked();
		assert!(!shell.read_with(visual, |shell, _| shell.left_sidebar_mounted));
		assert!(!shell.read_with(visual, |shell, _| shell.inspector_mounted));

		visual.simulate_keystrokes("cmd-b");
		visual.simulate_keystrokes("cmd-shift-b");
		assert!(shell.read_with(visual, |shell, _| shell.left_sidebar_visible));
		assert!(shell.read_with(visual, |shell, _| shell.left_sidebar_mounted));
		assert!(shell.read_with(visual, |shell, _| shell.inspector_visible));
		assert!(shell.read_with(visual, |shell, _| shell.inspector_mounted));
	}

	#[gpui::test]
	fn supported_sizes_preserve_fixed_shell_dimensions(cx: &mut TestAppContext) {
		let (_shell, visual) = open_shell(cx);
		for (width, height) in [(1180.0, 720.0), (1440.0, 900.0)] {
			visual.update(|window, cx| {
				window.resize(size(px(width), px(height)));
				window.draw(cx).clear();
				assert_eq!(WORKBENCH_TOPBAR_HEIGHT, 42.0);
				assert_eq!(WORKBENCH_SESSION_SIDEBAR_WIDTH, 248.0);
				assert_eq!(WORKBENCH_INSPECTOR_WIDTH, 344.0);
			});
		}
	}
}
