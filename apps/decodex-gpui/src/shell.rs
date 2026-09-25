//! Production GPUI window, navigation, focus, and lifecycle rendering boundary.
#[path = "account_identity.rs"] mod account_identity;
#[cfg(all(target_os = "macos", not(test)))]
#[path = "shell_native_status.rs"]
mod native_status;
#[path = "quota_meter.rs"] mod quota_meter;
#[path = "shell_reset_cards.rs"] mod reset_cards;
#[path = "shell_status.rs"] mod status;
use crate::ui_motion::SmoothControl;
pub(crate) use status::{
	count_preference as notification_count_preference, question_notice_preference,
};

#[path = "chief_surface.rs"] pub(crate) mod chief_surface;
use chief_surface::ChiefSurface;
#[path = "shell_navigation.rs"] mod navigation;
#[path = "workspace_symbols.rs"] mod workspace_symbols;

use std::{
	future::Future,
	pin::Pin,
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
		mpsc::{self, Receiver},
	},
	time::Duration,
};

use gpui::{
	Animation, AnimationExt, AnyElement, App, BoxShadow, ClipboardItem, Context, Entity,
	FocusHandle, Focusable, FontWeight, Global, Hsla, KeyBinding, MouseButton, Render, Role,
	SharedString, Subscription, Task, WeakEntity, Window, WindowControlArea, WindowHandle, actions,
	div, ease_in_out, prelude::*, px, rgb, rgba,
};

use decodex_protocol::{
	AccountCommandRejectionDto, AccountDto, AccountLifecycleReadinessDto, AccountLoginInstallMode,
	AccountLoginMethod, AccountLoginStart, AccountLoginState, AccountLoginStatus,
	AccountObservedStateDto, AccountProfileResult, AccountQuotaStateDto, AccountQuotaWindowDto,
	AccountSelectionModeDto, AppServerCapability, ClientFailure, ConversationRecoveryAction,
	ConversationState, ConversationSummary, DoctorComponent, DoctorIssue, DoctorStatus, EntityId,
	HistoryItemDto, HistoryItemKindDto, HistoryItemStatusDto, HistoryPayloadDto, HistoryTurnRole,
	IdempotencyKey,
};

use crate::{
	account_login::AccountLoginController,
	account_profile::{AccountProfileController, AccountProfileLoadState, AccountProfileSnapshot},
	accounts::{
		AccountCommandState, AccountInputError, AccountsController, AccountsLoadState,
		AccountsSnapshot, canonical_uuid_v4,
	},
	client_lifecycle::{ClientLifecycle, ConnectionView, LifecycleCancellation},
	composer_input::{self, ComposerEvent, ComposerInput, MAX_COMPOSER_BYTES, SubmitComposer},
	conversations::{
		ConversationCommandState, ConversationInputError, ConversationRefreshState, Conversations,
		ConversationsLoadState, ConversationsSnapshot, QueuedConversationSubmission,
	},
	desktop_settings::{DesktopSettingsController, DesktopSettingsSnapshot},
	health_query::{HealthLoadState, HealthQuery, HealthSnapshot},
	history_pager::{HistoryLoadState, HistoryPageSource, HistoryPager, HistorySnapshot},
	settings_surface::SettingsSurface,
	ui_theme,
};

// Match the gap below floating controls to their inset from the window edge.
const WINDOW_CONTROLS_CLEARANCE: f32 =
	ui_theme::CONTROL_MARGIN * 2.0 + ui_theme::CONTROL_GROUP_HEIGHT;
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

fn conversation_transcript_rows(
	snapshot: &ConversationsSnapshot,
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

fn conversation_recovery_presentation(task: Option<&ConversationSummary>) -> (bool, &'static str) {
	let recovery_action = task.and_then(|task| task.recovery_action);
	let outcome_unknown = task.is_some_and(|task| task.state == ConversationState::OutcomeUnknown);
	let executable = outcome_unknown
		|| recovery_action.is_some_and(|action| {
			matches!(
				action,
				ConversationRecoveryAction::ResumeRouting
					| ConversationRecoveryAction::CreateRoutingSuccessor
					| ConversationRecoveryAction::ResumeEstablishment
					| ConversationRecoveryAction::StartNewConversation
			)
		});
	let label = if outcome_unknown {
		"Retry sync"
	} else if recovery_action == Some(ConversationRecoveryAction::StartNewConversation) {
		"Start new"
	} else {
		"Recover"
	};
	(executable, label)
}

const HEALTH_CORE_COMPONENTS: [DoctorComponent; 8] = [
	DoctorComponent::Configuration,
	DoctorComponent::ProductStore,
	DoctorComponent::Conversation,
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
const HEALTH_OPTIONAL_COMPONENTS: [DoctorComponent; 2] =
	[DoctorComponent::BlobIntegrity, DoctorComponent::PluginReadiness];

actions!(
	decodex_shell,
	[
		FocusNext,
		FocusPrevious,
		ActivateDestination,
		ActivateChief,
		ActivateConversations,
		ActivateHealth,
		ActivateSettings,
		CloseSettings,
		RefreshHealth,
		ToggleSidebar,
		ShrinkPanel,
		GrowPanel,
		ResetPanel,
		ShrinkPanels,
		GrowPanels,
		ResetPanels,
		ToggleInspector,
		ToggleGraph,
		DismissStatus,
		NavigateBack,
		NavigateForward,
		SelectPreviousConversation,
		SelectNextConversation,
		ActivateConversationRow,
	]
);

/// Stable shell destinations. Each live destination remains issue-owned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Destination {
	Chief,
	Advisor,
	Projects,
	Conversations,
	Runs,
	Automations,
	Accounts,
	Health,
	Settings,
}

impl Destination {
	pub(crate) const ALL: [Self; 9] = [
		Self::Chief,
		Self::Advisor,
		Self::Projects,
		Self::Conversations,
		Self::Runs,
		Self::Automations,
		Self::Accounts,
		Self::Health,
		Self::Settings,
	];

	pub(crate) const fn label(self) -> &'static str {
		match self {
			Self::Chief => "Main",
			Self::Advisor => "Advisor",
			Self::Projects => "Projects",
			Self::Conversations => "History",
			Self::Runs => "Runs",
			Self::Automations => "Automations",
			Self::Accounts => "Accounts",
			Self::Health => "Diagnostics",
			Self::Settings => "Settings",
		}
	}

	const fn description(self) -> &'static str {
		match self {
			Self::Chief => "Open Chief",
			Self::Advisor => "Review guidance and bounded decisions.",
			Self::Projects => "Own repositories and product context.",
			Self::Conversations =>
				"Chat directly with Codex: choose a conversation, write a message, and read its reply.",
			Self::Runs => "Inspect managed run activity and evidence.",
			Self::Automations => "Operate scheduled and event-driven work.",
			Self::Accounts =>
				"Accounts: manage sign-in, usage limits, and the account used for new work.",
			Self::Health => "Diagnostics: check connection health and find the cause of errors.",
			Self::Settings => "Configure the window and in-process menu bar.",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum InspectorTab {
	Context,
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
		ConnectionView::Connecting { .. } => ConnectionPresentation {
			label: "Connecting",
			detail: "Starting Decodex.".into(),
			color: 0xf59e0b,
		},
		ConnectionView::Online { .. } => ConnectionPresentation {
			label: "Online",
			detail: "Connected and ready.".into(),
			color: 0x22c55e,
		},
		ConnectionView::OfflineRetrying { .. } => ConnectionPresentation {
			label: "Reconnecting",
			detail: "Trying to restore Decodex.".into(),
			color: 0xf97316,
		},
		ConnectionView::Incompatible(_) => ConnectionPresentation {
			label: "Restart Decodex",
			detail: "Restart Decodex to restore the connection.".into(),
			color: 0xef4444,
		},
		ConnectionView::Quarantined { .. } => ConnectionPresentation {
			label: "Restart Decodex",
			detail: "Restart Decodex to restore the connection.".into(),
			color: 0xdc2626,
		},
		ConnectionView::ShuttingDown => ConnectionPresentation {
			label: "Shutting down",
			detail: "Closing the retained session cooperatively".into(),
			color: 0x94a3b8,
		},
		ConnectionView::Stopped => ConnectionPresentation {
			label: "Restart Decodex",
			detail: "Restart Decodex to restore the connection.".into(),
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
		ClientFailure::ProtocolDisconnected
		| ClientFailure::ProtocolTimeout
		| ClientFailure::ServiceVersionMismatch => "Restart Decodex.",
		ClientFailure::ServerIdentityMismatch => "Stable server identity does not match",
		ClientFailure::ProtocolMalformed => "Daemon response is malformed",
		ClientFailure::ProtocolViolation => "Daemon protocol ordering was refused",
		ClientFailure::ProtocolBackpressure => "Daemon message allowance was exhausted",
		ClientFailure::ApplicationAcceptanceUnknown => "Application command acceptance is unknown",
	}
}

/// Bind the shell's complete keyboard path once at application startup.
pub(crate) fn bind_keys(cx: &mut App) {
	composer_input::bind_keys(cx);
	cx.bind_keys([
		KeyBinding::new("cmd-w", CloseSettings, Some("SettingsWindow")),
		KeyBinding::new("escape", CloseSettings, Some("SettingsWindow")),
	]);
	cx.bind_keys([
		KeyBinding::new("tab", FocusNext, None),
		KeyBinding::new("shift-tab", FocusPrevious, None),
		KeyBinding::new("cmd-2", ActivateChief, None),
		KeyBinding::new("cmd-1", ActivateChief, None),
		KeyBinding::new("cmd-3", ActivateHealth, None),
		KeyBinding::new("cmd-,", ActivateSettings, None),
		KeyBinding::new("cmd-e", ToggleSidebar, None),
		KeyBinding::new("ctrl-alt--", ShrinkPanel, None),
		KeyBinding::new("ctrl-alt-=", GrowPanel, None),
		KeyBinding::new("ctrl-alt-0", ResetPanel, None),
		KeyBinding::new("ctrl-alt-shift--", ShrinkPanels, None),
		KeyBinding::new("ctrl-alt-shift-=", GrowPanels, None),
		KeyBinding::new("ctrl-alt-shift-0", ResetPanels, None),
		// macOS normalizes shifted punctuation and consumes the Shift modifier.
		KeyBinding::new("ctrl-alt-+", GrowPanels, None),
		KeyBinding::new("ctrl-alt-_", ShrinkPanels, None),
		KeyBinding::new("ctrl-alt-)", ResetPanels, None),
		KeyBinding::new("cmd-b", ToggleInspector, None),
		KeyBinding::new("cmd-j", ToggleGraph, None),
		KeyBinding::new("cmd-[", NavigateBack, None),
		KeyBinding::new("cmd-]", NavigateForward, None),
		KeyBinding::new("enter", ActivateDestination, Some("Destination")),
		KeyBinding::new("space", ActivateDestination, Some("Destination")),
		KeyBinding::new("enter", RefreshHealth, Some("HealthRefresh")),
		KeyBinding::new("space", RefreshHealth, Some("HealthRefresh")),
		KeyBinding::new("up", SelectPreviousConversation, Some("Conversations")),
		KeyBinding::new("down", SelectNextConversation, Some("Conversations")),
		KeyBinding::new("enter", ActivateConversationRow, Some("ConversationRow")),
		KeyBinding::new("space", ActivateConversationRow, Some("ConversationRow")),
	]);
}

/// One window-owned production shell. Connection ownership lives at application scope.
pub(crate) struct Shell {
	ordinary_last: Option<decodex_protocol::DesktopOrdinaryDraft>,
	ordinary_owner: Option<EntityId>,
	ordinary_syncing: bool,
	reset_cards: reset_cards::ResetCardsPanel,
	settings_window: Option<WindowHandle<SettingsWindow>>,
	settings_selected: Destination,
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
	chief: Entity<ChiefSurface>,
	settings: Entity<SettingsSurface>,
	account_login: Option<Arc<AccountLoginController>>,
	account_login_status: Option<AccountLoginStatus>,
	account_login_error: Option<SharedString>,
	account_login_updates: Option<Receiver<Result<AccountLoginStatus, ClientFailure>>>,
	account_login_task: Option<Task<()>>,
	account_login_cancellation: Option<Arc<AtomicBool>>,
	opened_account_login_url: Option<String>,
	pending_account_logout: Option<EntityId>,
	account_profile_controller: AccountProfileController,
	account_profile: AccountProfileSnapshot,
	account_emails: account_identity::Emails,
	desktop_settings: DesktopSettingsController,
	desktop_settings_snapshot: DesktopSettingsSnapshot,
	accounts_controller: AccountsController,
	accounts: AccountsSnapshot,
	account_status: Option<SharedString>,
	health_query: HealthQuery,
	health: HealthSnapshot,
	conversations: Conversations,
	quick: ConversationsSnapshot,
	history_pager: Option<HistoryPager>,
	history: Option<HistorySnapshot>,
	opened_history: Option<EntityId>,
	deferred_provider_refresh: Option<EntityId>,
	last_provider_sync: Option<std::time::Instant>,
	creating_new: bool,
	pending_submission: Option<PendingComposerSubmission>,
	input_status: Option<SharedString>,
	titlebar_drag_pending: bool,
	navigation: navigation::NavigationHistory,
	status_open: bool,
	dismissed_notifications: std::cell::RefCell<std::collections::HashSet<(String, String)>>,
	#[cfg(all(target_os = "macos", not(test)))]
	native_status: native_status::NativeStatus,
}

#[path = "ordinary_drafts.rs"] mod ordinary_drafts;

impl Shell {
	pub(crate) fn drafts_ready_for_quit(&mut self, cx: &mut Context<Self>) -> bool {
		self.sync_ordinary_drafts(cx);
		self.chief.update(cx, |surface, cx| surface.drafts_ready_for_quit(cx))
	}

	pub(crate) fn flush_drafts_for_quit(&mut self, cx: &mut Context<Self>) -> Task<bool> {
		self.sync_ordinary_drafts(cx);
		self.chief.update(cx, |surface, cx| surface.flush_drafts_for_quit(cx))
	}

	pub(crate) fn with_chief_profile(
		mut self,
		profile: Option<decodex_protocol::ClientProfile>,
		cx: &mut Context<Self>,
	) -> Self {
		self.account_emails = Default::default();
		self.reset_cards.profile = profile.clone();
		let cwd = self.conversations.working_directory();
		self.chief.update(cx, |surface, cx| {
			surface.bind_profile(profile, cx);
			surface.seed_context(cwd, vec![], cx);
			surface.refresh(cx);
		});
		self
	}

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
			shell.sync_ordinary_drafts(cx);
			cx.notify();
		})
		.detach();
		let chief = cx.new(ChiefSurface::new);
		cx.observe(&chief, |shell, _, cx| {
			shell.record_navigation(cx);
			shell.sync_ordinary_drafts(cx);
			cx.notify();
		})
		.detach();
		let desktop_settings = DesktopSettingsController::production();
		let desktop_settings_snapshot = desktop_settings.snapshot();
		let account_profile_controller = AccountProfileController::production();
		let account_profile = account_profile_controller.snapshot();
		let settings_controller = desktop_settings.clone();
		let settings = cx.new(|cx| SettingsSurface::new(settings_controller, cx));
		let accounts_controller = AccountsController::production();
		let accounts = accounts_controller.snapshot();
		let health_query = HealthQuery::production();
		let health = health_query.snapshot();
		let conversations = Conversations::production();
		conversations.activate();
		let quick = conversations.snapshot();
		window.focus(&root_focus, cx);

		Self {
			settings_window: None,
			settings_selected: Destination::Settings,
			selected: Destination::Chief,
			inspector_tab: InspectorTab::Context,
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
			chief,
			settings,
			account_login: None,
			account_login_status: None,
			account_login_error: None,
			account_login_updates: None,
			account_login_task: None,
			account_login_cancellation: None,
			opened_account_login_url: None,
			pending_account_logout: None,
			reset_cards: reset_cards::ResetCardsPanel::default(),
			account_profile_controller,
			account_profile,
			account_emails: Default::default(),
			desktop_settings,
			desktop_settings_snapshot,
			accounts_controller,
			accounts,
			account_status: None,
			health_query,
			health,
			conversations,
			quick,
			history_pager: None,
			history: None,
			opened_history: None,
			deferred_provider_refresh: None,
			last_provider_sync: None,
			creating_new: true,
			pending_submission: None,
			ordinary_last: None,
			ordinary_owner: None,
			ordinary_syncing: false,
			input_status: None,
			titlebar_drag_pending: false,
			navigation: navigation::NavigationHistory::new(),
			status_open: false,
			dismissed_notifications: Default::default(),
			#[cfg(all(target_os = "macos", not(test)))]
			native_status: Default::default(),
		}
	}

	pub(crate) fn with_account_login(
		mut self,
		account_login: Option<Arc<AccountLoginController>>,
	) -> Self {
		self.account_login = account_login;
		self
	}

	pub(crate) fn was_launched_as_login_item(&self, cx: &App) -> bool {
		self.settings.read(cx).was_launched_as_login_item()
	}

	#[cfg(feature = "visual-capture")]
	#[allow(dead_code)]
	pub(crate) fn visual_workbench(window: &mut Window, cx: &mut Context<Self>) -> Self {
		use decodex_protocol::{
			ConversationSummary, ConversationTitle, EntityRevision, ProviderThreadId,
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
		            title: &str,
		            runtime_id: &str,
		            state: ConversationState,
		            active_turn_id: Option<EntityId>,
		            revision: u64| {
			ConversationSummary::new(
				conversation_id,
				ConversationTitle::new(title).expect("visual title is valid"),
				Some(
					ProviderThreadId::new(format!("thread-{revision}"))
						.expect("visual thread is valid"),
				),
				None,
				EntityRevision(revision),
				1_786_000_000_000_000 + i64::try_from(revision).unwrap_or_default(),
				Some(EntityId::new(runtime_id).expect("visual runtime identity is bounded")),
				Some(EntityRevision(revision)),
				state,
				active_turn_id,
				None,
			)
			.expect("visual Conversation projection is valid")
		};
		shell.quick = ConversationsSnapshot {
			catalog: None,
			load: ConversationsLoadState::Ready,
			command: ConversationCommandState::Idle,
			command_conversation_id: None,
			submission_result_generation: 0,
			last_submission_accepted: false,
			refresh: ConversationRefreshState::Idle,
			tasks: vec![
				task(
					conversation_id.clone(),
					"Redesign the Codex Workbench",
					runtime_session_id.as_str(),
					ConversationState::Running,
					Some(active_turn_id),
					14,
				),
				task(
					second_conversation_id.clone(),
					"Harden conversation recovery",
					"20000000-0000-4000-8000-000000000002",
					ConversationState::Ready,
					None,
					8,
				),
				task(
					third_conversation_id.clone(),
					"Review account routing",
					"20000000-0000-4000-8000-000000000003",
					ConversationState::Ready,
					None,
					5,
				),
			],
			selected: Some(conversation_id.clone()),
			live_deltas: Vec::new(),
			can_submit: true,
			initial_defaults_ready: false,
			execution: decodex_protocol::ConversationExecutionSettings::new(
				decodex_protocol::ConversationModel::new("gpt-5.6-sol")
					.expect("visual model identifier is valid"),
				decodex_protocol::ConversationReasoningEffort::High,
				false,
			),
		};

		shell.visual_accounts_and_health();
		shell.visual_history(conversation_id, runtime_session_id);
		shell
	}

	#[cfg(feature = "visual-capture")]
	fn visual_accounts_and_health(&mut self) {
		use decodex_protocol::{AccountRoutingControlDto, EntityRevision, WireText};
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
		self.accounts = AccountsSnapshot {
			load: AccountsLoadState::Ready,
			command: AccountCommandState::Idle,
			accounts: vec![primary.clone(), reserve.clone(), research.clone()],
			routing: Some(AccountRoutingControlDto {
				revision: EntityRevision(6),
				mode: AccountSelectionModeDto::Fixed(primary.account_id.clone()),
				order: vec![primary.account_id, reserve.account_id, research.account_id],
			}),
			rejection: None,
			can_manage: true,
			can_route: true,
			route_reopen_notice: false,
		};
		let health_checks = DoctorComponent::ALL
			.into_iter()
			.map(|component| {
				let status = match component {
					DoctorComponent::AppServerCapability(_) | DoctorComponent::BlobIntegrity =>
						DoctorStatus::Unknown(DoctorIssue::NotProbed),
					DoctorComponent::PluginReadiness => DoctorStatus::Unknown(DoctorIssue::Plugin),
					_ => DoctorStatus::Ready,
				};
				decodex_protocol::DoctorCheck::new(component, status)
			})
			.collect();
		self.health = HealthSnapshot {
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
	}

	#[cfg(feature = "visual-capture")]
	fn visual_history(&mut self, conversation_id: EntityId, runtime_session_id: EntityId) {
		use crate::history_pager::{HistoryCursorObservation, HistoryPageSource};
		use decodex_protocol::ConversationHistoryPage;

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
					"I’ll make the conversation the primary surface, keep work details in a secondary view, and bind the inspector to the current Work Item. The UI will not invent diff data that the app-server does not provide.",
					2,
				),
				item(
					"history-03",
					"turn-01",
					"tool",
					"tool_call",
					"Inspected Shell, Conversations, Chief, and HistoryPager ownership boundaries",
					3,
				),
				item(
					"history-04",
					"turn-01",
					"assistant",
					"message",
					"The first pass is now a compact Workbench: integrated title bar, horizontal sessions, dense transcript, floating composer, and a real Work Item inspector. Chief coordinates work and its dependencies.",
					4,
				),
				item(
					"history-05",
					"turn-02",
					"user",
					"message",
					"Keep the visual language quiet and professional. The information architecture should make coordinated work clear.",
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
		self.history = Some(HistorySnapshot {
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
		self.opened_history = Some(conversation_id);
		self.creating_new = false;
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
		if destination == Destination::Chief {
			shell.connection = ConnectionView::Stopped;
			shell.chief.update(cx, |surface, cx| {
				surface.visual_workspace_fixture(cx);
				if let Ok(page) = std::env::var("DECODEX_VISUAL_WORKSPACE_PAGE") {
					surface
						.visual_workspace_page(if page == "status" { "empty" } else { &page }, cx);
					if page == "status" {
						surface.mark_stale(cx);
					}
				}
			});
		}
		shell.status_open = std::env::var("DECODEX_VISUAL_WORKSPACE_PAGE")
			.is_ok_and(|page| page == "status")
			|| std::env::var_os("DECODEX_VISUAL_STATUS").is_some();
		shell.left_sidebar_visible = left_sidebar_visible;
		shell.left_sidebar_mounted = left_sidebar_visible;
		shell.inspector_visible = inspector_visible;
		shell.inspector_mounted = inspector_visible;
		shell
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
			if Destination::ALL[index] == Destination::Settings {
				self.open_settings_window(Destination::Settings, cx);
			} else {
				self.select_destination(Destination::ALL[index], cx);
			}
		}
	}

	fn select_destination(&mut self, destination: Destination, cx: &mut Context<Self>) {
		if self.selected == destination {
			return;
		}
		if self.selected == Destination::Health {
			self.health_query.deactivate();
		}
		if self.selected == Destination::Conversations {
			self.conversations.deactivate();
		}
		if self.selected == Destination::Accounts {
			self.accounts_controller.deactivate();
		}

		if self.selected == Destination::Chief {
			self.chief.update(cx, ChiefSurface::stop_voice);
		}
		self.selected = destination;
		if destination == Destination::Chief {
			let cwd = self.conversations.working_directory();
			let accounts = self
				.accounts
				.accounts
				.iter()
				.map(|account| {
					(account.account_id.as_str().to_owned(), account.alias.as_str().to_owned())
				})
				.collect();
			self.chief.update(cx, |surface, cx| surface.seed_context(cwd, accounts, cx));
			self.chief.update(cx, ChiefSurface::refresh);
		}
		if destination == Destination::Health {
			self.health_query.activate();
		}
		if destination == Destination::Conversations {
			self.conversations.activate();
			self.last_provider_sync = None;
		}
		if destination == Destination::Accounts {
			self.accounts_controller.activate();
			self.synchronize_accounts();
		}
		if destination == Destination::Settings {
			self.settings.update(cx, SettingsSurface::refresh);
		}
		self.health = self.health_query.snapshot();
		self.record_navigation(cx);
		cx.notify();
	}

	fn activate_chief(&mut self, _: &ActivateChief, _: &mut Window, cx: &mut Context<Self>) {
		self.select_destination(Destination::Chief, cx);
	}

	fn activate_conversations(
		&mut self,
		_: &ActivateConversations,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.select_destination(Destination::Conversations, cx);
		cx.stop_propagation();
	}

	fn activate_health(&mut self, _: &ActivateHealth, _: &mut Window, cx: &mut Context<Self>) {
		self.open_settings_window(Destination::Health, cx);
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
		if self.selected == Destination::Chief {
			self.chief.update(cx, ChiefSurface::toggle_workspace_sidebar);
		}
		if self.selected != Destination::Chief {
			self.set_left_sidebar_visible(!self.left_sidebar_visible, cx);
		}
		cx.stop_propagation();
	}

	fn toggle_inspector(&mut self, _: &ToggleInspector, _: &mut Window, cx: &mut Context<Self>) {
		if self.selected == Destination::Chief {
			self.chief.update(cx, ChiefSurface::toggle_agent_tree);
		}
		if self.selected == Destination::Conversations {
			self.set_inspector_visible(!self.inspector_visible, cx);
		}
		cx.stop_propagation();
	}

	fn interrupt_reply(
		&mut self,
		event: &gpui::KeyDownEvent,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		if event.keystroke.key != "escape" || event.is_held {
			return;
		}
		if self.selected == Destination::Chief {
			if self.status_open {
				self.status_open = false;
				cx.notify();
				return;
			}
			self.chief.update(cx, ChiefSurface::escape_interrupt);
			cx.stop_propagation();
		}
	}

	fn toggle_graph(&mut self, _: &ToggleGraph, _: &mut Window, cx: &mut Context<Self>) {
		if self.selected == Destination::Chief {
			self.chief.update(cx, ChiefSurface::toggle_workspace_graph);
		}
		cx.stop_propagation();
	}

	fn activate_settings(&mut self, _: &ActivateSettings, _: &mut Window, cx: &mut Context<Self>) {
		self.open_settings_window(Destination::Settings, cx);
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
		if self.selected == Destination::Health
			|| (self.settings_window.is_some() && self.settings_selected == Destination::Health)
		{
			self.health_query.activate();
		}
		self.health = self.health_query.snapshot();
		cx.notify();
	}

	fn bind_desktop_settings(
		&mut self,
		desktop_settings: DesktopSettingsController,
		cx: &mut Context<Self>,
	) {
		self.desktop_settings = desktop_settings.clone();
		self.desktop_settings_snapshot = desktop_settings.snapshot();
		self.settings.update(cx, |settings, cx| {
			settings.bind_controller(desktop_settings, cx);
		});
	}

	fn bind_account_profile(
		&mut self,
		account_profile: AccountProfileController,
		cx: &mut Context<Self>,
	) {
		self.account_profile_controller = account_profile;
		self.account_profile = self.account_profile_controller.snapshot();
		cx.notify();
	}

	fn bind_conversations(
		&mut self,
		conversations: Conversations,
		history_pager: HistoryPager,
		cx: &mut Context<Self>,
	) {
		self.conversations.deactivate();
		self.conversations = conversations;
		self.reset_ordinary_draft_binding(cx);
		self.history_pager = Some(history_pager);
		if self.selected == Destination::Conversations {
			self.conversations.activate();
			self.last_provider_sync = None;
		}
		self.synchronize_conversations(cx);
		self.reconcile_pending_submission(cx);
		cx.notify();
	}

	fn bind_accounts(&mut self, accounts: AccountsController, cx: &mut Context<Self>) {
		self.accounts_controller.deactivate();
		self.accounts_controller = accounts;
		if self.selected == Destination::Accounts
			|| (self.settings_window.is_some() && self.settings_selected == Destination::Accounts)
		{
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

	fn logout_account(&mut self, account_id: &EntityId, cx: &mut Context<Self>) {
		if self.pending_account_logout.as_ref() != Some(account_id) {
			self.pending_account_logout = Some(account_id.clone());
			self.account_status =
				Some("Select Log out again to confirm credential deletion.".into());
			cx.notify();
			return;
		}
		self.pending_account_logout = None;
		self.account_status = self
			.accounts_controller
			.logout(account_id)
			.err()
			.map(account_input_error_label)
			.map(Into::into);
		self.synchronize_accounts();
		cx.notify();
	}

	fn move_account(&mut self, account_id: &EntityId, offset: isize, cx: &mut Context<Self>) {
		self.account_status = self
			.accounts_controller
			.move_account(account_id, offset)
			.err()
			.map(account_input_error_label)
			.map(Into::into);
		self.synchronize_accounts();
		cx.notify();
	}

	fn show_account_profile(&mut self, account_id: EntityId, cx: &mut Context<Self>) {
		self.account_profile_controller.select(account_id);
		self.account_profile = self.account_profile_controller.snapshot();
		cx.notify();
	}

	fn close_account_profile(&mut self, cx: &mut Context<Self>) {
		self.account_profile_controller.close();
		self.account_profile = self.account_profile_controller.snapshot();
		cx.notify();
	}

	fn refresh_account_profile(&mut self, cx: &mut Context<Self>) {
		let _ = self.account_profile_controller.refresh();
		self.account_profile = self.account_profile_controller.snapshot();
		cx.notify();
	}

	fn start_account_enrollment(&mut self, method: AccountLoginMethod, cx: &mut Context<Self>) {
		let start = account_login_start(method, None);
		self.start_account_login(start, cx);
	}

	fn start_account_reauthentication(
		&mut self,
		account_id: EntityId,
		expected_revision: decodex_protocol::EntityRevision,
		recovery_operation_id: Option<EntityId>,
		cx: &mut Context<Self>,
	) {
		let start = account_login_start(
			AccountLoginMethod::DeviceCode,
			Some((account_id, expected_revision, recovery_operation_id)),
		);
		self.start_account_login(start, cx);
	}

	fn start_account_login(
		&mut self,
		start: Result<AccountLoginStart, SharedString>,
		cx: &mut Context<Self>,
	) {
		if self.account_login_task.is_some() {
			self.account_login_error = Some("An account login is already active.".into());
			cx.notify();
			return;
		}
		let Some(controller) = self.account_login.clone() else {
			self.account_login_error =
				Some("The local account-login client is unavailable.".into());
			cx.notify();
			return;
		};
		let Ok(start) = start else {
			self.account_login_error = start.err();
			cx.notify();
			return;
		};

		let cancellation = Arc::new(AtomicBool::new(false));
		let task_cancellation = Arc::clone(&cancellation);
		let (updates, receiver) = mpsc::channel();
		self.account_login_status = None;
		self.account_login_error = None;
		self.opened_account_login_url = None;
		self.account_login_updates = Some(receiver);
		self.account_login_cancellation = Some(cancellation);
		self.account_login_task = Some(cx.background_executor().spawn(async move {
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.expect("build the bounded account-login runtime");
			runtime.block_on(async move {
				let mut status = match controller.start(start).await {
					Ok(status) => status,
					Err(failure) => {
						let _ = updates.send(Err(failure));
						return;
					},
				};
				loop {
					let terminal = matches!(
						status.state,
						AccountLoginState::Completed
							| AccountLoginState::Failed
							| AccountLoginState::Cancelled
					);
					let session_id = status.session_id.clone();
					if updates.send(Ok(status)).is_err() || terminal {
						return;
					}
					tokio::time::sleep(Duration::from_millis(350)).await;
					let next = if task_cancellation.load(Ordering::Acquire) {
						controller.cancel(session_id).await
					} else {
						controller.status(session_id).await
					};
					match next {
						Ok(next) => status = next,
						Err(failure) => {
							let _ = updates.send(Err(failure));
							return;
						},
					}
				}
			});
		}));
		cx.notify();
	}

	fn cancel_account_login(&mut self, cx: &mut Context<Self>) {
		if let Some(cancellation) = &self.account_login_cancellation {
			cancellation.store(true, Ordering::Release);
			self.account_login_error = Some("Cancelling account login…".into());
			cx.notify();
		}
	}

	fn poll_account_login(&mut self, cx: &mut Context<Self>) {
		let updates = self
			.account_login_updates
			.as_ref()
			.map(|receiver| receiver.try_iter().collect::<Vec<_>>())
			.unwrap_or_default();
		if updates.is_empty() {
			return;
		}
		for update in updates {
			match update {
				Ok(status) => {
					if let Some(url) = status.authorization_url.as_ref()
						&& self.opened_account_login_url.as_deref() != Some(url.as_str())
					{
						self.opened_account_login_url = Some(url.as_str().to_owned());
						cx.open_url(url.as_str());
					}
					let terminal = matches!(
						status.state,
						AccountLoginState::Completed
							| AccountLoginState::Failed
							| AccountLoginState::Cancelled
					);
					if status.state == AccountLoginState::Completed {
						let _ = self.accounts_controller.refresh();
					}
					self.account_login_error = None;
					self.account_login_status = Some(status);
					if terminal {
						self.account_login_task = None;
						self.account_login_cancellation = None;
						self.account_login_updates = None;
					}
				},
				Err(failure) => {
					self.account_login_error = Some(startup_failure(failure).into());
					self.account_login_task = None;
					self.account_login_cancellation = None;
					self.account_login_updates = None;
				},
			}
		}
		self.synchronize_accounts();
		cx.notify();
	}

	fn copy_account_login_code(&mut self, cx: &mut Context<Self>) {
		if let Some(code) = self
			.account_login_status
			.as_ref()
			.and_then(|status| status.prompt.as_ref())
			.map(|prompt| prompt.user_code.as_str().to_owned())
		{
			cx.write_to_clipboard(ClipboardItem::new_string(code));
			self.account_login_error = Some("Login code copied.".into());
			cx.notify();
		}
	}

	fn open_account_login_url(&mut self, cx: &mut Context<Self>) {
		let url =
			self.account_login_status.as_ref().and_then(|status| {
				status.authorization_url.as_ref().map(|url| url.as_str()).or_else(|| {
					status.prompt.as_ref().map(|prompt| prompt.verification_url.as_str())
				})
			});
		if let Some(url) = url {
			cx.open_url(url);
		}
	}

	fn synchronize_conversations(&mut self, cx: &mut Context<Self>) {
		self.sync_ordinary_drafts(cx);
		self.conversations.ensure_initial_catalog();
		let snapshot = self.conversations.snapshot();
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
			let _ = self.conversations.refresh_selected_silently();
			self.quick = self.conversations.snapshot();
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

	fn start_new_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.conversations.begin_new();
		self.deferred_provider_refresh = None;
		if let Some(pager) = self.history_pager.as_ref() {
			pager.cancel();
		}
		self.opened_history = None;
		self.creating_new = true;
		self.input_status = None;
		self.synchronize_conversations(cx);
		window.focus(&self.composer.focus_handle(cx), cx);
		cx.notify();
	}

	fn choose_conversation(
		&mut self,
		conversation_id: EntityId,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		if self.conversations.select(conversation_id.clone()) {
			// The same retained connection serializes provider commands before later queries.
			// Show daemon-owned SQLite history first, then reconcile the provider in background.
			self.deferred_provider_refresh = Some(conversation_id);
			self.creating_new = false;
			self.opened_history = None;
			self.input_status = None;
			self.synchronize_conversations(cx);
			window.focus(&self.composer.focus_handle(cx), cx);
			cx.notify();
		}
	}

	fn select_adjacent_conversation(&mut self, delta: isize, cx: &mut Context<Self>) {
		if self.selected != Destination::Conversations || self.quick.tasks.is_empty() {
			return;
		}
		let current = self.quick.selected.as_ref().and_then(|selected| {
			self.quick.tasks.iter().position(|task| &task.conversation_id == selected)
		});
		let Some(next) = adjacent_conversation_index(current, self.quick.tasks.len(), delta) else {
			return;
		};
		let conversation_id = self.quick.tasks[next].conversation_id.clone();
		if self.conversations.select(conversation_id.clone()) {
			self.deferred_provider_refresh = Some(conversation_id);
			self.creating_new = false;
			self.opened_history = None;
			self.input_status = None;
			self.synchronize_conversations(cx);
			cx.notify();
		}
	}

	fn select_previous_conversation(
		&mut self,
		_: &SelectPreviousConversation,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.select_adjacent_conversation(-1, cx);
		cx.stop_propagation();
	}

	fn select_next_conversation(
		&mut self,
		_: &SelectNextConversation,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.select_adjacent_conversation(1, cx);
		cx.stop_propagation();
	}

	fn submit_composer(&mut self, _: &SubmitComposer, window: &mut Window, cx: &mut Context<Self>) {
		self.submit_conversation(window, cx);
		cx.stop_propagation();
	}

	fn submit_conversation(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		let creating = self.creating_new || self.quick.selected.is_none();
		let message = self.composer.read(cx).content().to_owned();
		// Read the controller's current terminal-result fence before queueing. The rendered
		// Shell snapshot can be one publication behind a just-settled prior submission.
		let result_generation = self.conversations.snapshot().submission_result_generation;
		let result = if creating {
			self.conversations.create(&message)
		} else {
			self.conversations.submit(&message)
		};
		match result {
			Ok(QueuedConversationSubmission { conversation_id, turn_id }) => {
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
		self.synchronize_conversations(cx);
		self.reconcile_pending_submission(cx);
		cx.notify();
	}

	fn recover_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let state = self.quick.selected_task().map(|task| task.state);
		let action = self.quick.selected_task().and_then(|task| task.recovery_action);
		if state == Some(ConversationState::OutcomeUnknown) {
			self.input_status =
				self.conversations.refresh_selected().err().map(input_error_label).map(Into::into);
			self.synchronize_conversations(cx);
			cx.notify();
			return;
		}
		if action == Some(ConversationRecoveryAction::StartNewConversation) {
			self.start_new_conversation(window, cx);
			return;
		}
		self.input_status =
			self.conversations.recover_selected().err().map(input_error_label).map(Into::into);
		self.synchronize_conversations(cx);
		cx.notify();
	}

	fn interrupt_conversation(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		if let Err(error) = self.conversations.interrupt() {
			self.input_status = Some(input_error_label(error).into());
		}
		self.synchronize_conversations(cx);
		cx.notify();
	}

	fn refresh_conversation(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		self.input_status =
			self.conversations.refresh_all().err().map(input_error_label).map(Into::into);
		self.synchronize_conversations(cx);
		cx.notify();
	}

	fn archive_conversation(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		self.input_status =
			self.conversations.archive_selected().err().map(input_error_label).map(Into::into);
		self.synchronize_conversations(cx);
		cx.notify();
	}

	fn cycle_conversation_model(&mut self, cx: &mut Context<Self>) {
		self.conversations.cycle_model();
		self.synchronize_conversations(cx);
		cx.notify();
	}

	fn cycle_conversation_effort(&mut self, cx: &mut Context<Self>) {
		self.conversations.cycle_reasoning_effort();
		self.synchronize_conversations(cx);
		cx.notify();
	}

	fn toggle_conversation_fast(&mut self, cx: &mut Context<Self>) {
		self.conversations.toggle_fast();
		self.synchronize_conversations(cx);
		cx.notify();
	}

	fn show_previous_history(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		if let Some(pager) = self.history_pager.as_ref() {
			let _ = pager.show_previous();
		}
		self.synchronize_conversations(cx);
		cx.notify();
	}

	fn show_next_history(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		if let Some(pager) = self.history_pager.as_ref() {
			let _ = pager.show_next();
		}
		self.synchronize_conversations(cx);
		cx.notify();
	}

	fn retry_history(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		if let Some(pager) = self.history_pager.as_ref() {
			let _ = pager.retry();
		}
		self.synchronize_conversations(cx);
		cx.notify();
	}
}

impl Drop for Shell {
	fn drop(&mut self) {
		if let Some(cancellation) = &self.account_login_cancellation {
			cancellation.store(true, Ordering::Release);
		}
	}
}

const fn input_error_label(error: ConversationInputError) -> &'static str {
	match error {
		ConversationInputError::Offline => "Conversations are offline.",
		ConversationInputError::Busy => "Wait for the current command result.",
		ConversationInputError::InvalidMessage => "Enter a message within the supported limit.",
		ConversationInputError::NoSelection => "Select a Conversation first.",
		ConversationInputError::NotReady =>
			"The selected Conversation is not ready for this command.",
		ConversationInputError::NotInterruptible => "The selected turn is not running.",
		ConversationInputError::IdentityUnavailable => "A command identity could not be created.",
		ConversationInputError::WorkingDirectoryUnavailable =>
			"The local Conversation working directory is unavailable.",
	}
}

struct LifecycleOwnerGlobal {
	_owner: Entity<LifecycleOwner>,
}

impl Global for LifecycleOwnerGlobal {}

pub(crate) struct LifecycleOwner {
	cancellation: LifecycleCancellation,
	task: Option<Task<()>>,
	running: bool,
	_subscriptions: Vec<Subscription>,
}

impl LifecycleOwner {
	fn new<R: 'static>(
		cancellation: LifecycleCancellation,
		views: Receiver<ConnectionView>,
		background: Task<R>,
		shell: WeakEntity<Shell>,
		cx: &mut Context<Self>,
	) -> Self {
		let task = cx.spawn(async move |owner, cx| {
			let background = background;
			loop {
				publish_views(&shell, &views, cx);
				if background.is_ready() {
					let _ = background.await;
					publish_views(&shell, &views, cx);
					let _ = owner.update(cx, |owner, _| owner.running = false);

					return;
				}
				cx.background_executor().timer(LIFECYCLE_POLL).await;
			}
		});

		Self {
			cancellation,
			task: Some(task),
			running: true,
			_subscriptions: vec![cx.on_app_quit(|owner, _| owner.shutdown())],
		}
	}

	fn shutdown(&mut self) -> Pin<Box<dyn Future<Output = ()> + 'static>> {
		self.running = false;
		self.cancellation.cancel();
		let task = self.task.take();

		Box::pin(async move {
			if let Some(task) = task {
				task.await;
			}
		})
	}

	#[cfg(test)]
	pub(crate) fn is_running(&self) -> bool {
		self.running
	}
}

pub(crate) fn retain_lifecycle(
	window: WindowHandle<Shell>,
	mut lifecycle: ClientLifecycle,
	cx: &mut App,
) {
	let cancellation = lifecycle.cancellation();
	let views = lifecycle.observe_views();
	let shell = window.entity(cx).expect("the production shell window remains open");
	let accounts = lifecycle.accounts();
	let account_profile = lifecycle.account_profile();
	let desktop_settings = lifecycle.desktop_settings();
	let health_query = lifecycle.health_query();
	let conversations = lifecycle.conversations();
	let history_pager = lifecycle.history_pager();
	shell.update(cx, |shell, cx| {
		shell.bind_accounts(accounts, cx);
		shell.bind_account_profile(account_profile, cx);
		shell.bind_desktop_settings(desktop_settings, cx);
		shell.bind_health_query(health_query, cx);
		shell.bind_conversations(conversations, history_pager, cx);
	});
	let shell = shell.downgrade();
	let background = cx.background_executor().spawn(async move {
		let runtime = tokio::runtime::Builder::new_current_thread()
			.enable_all()
			.build()
			.expect("build the bounded client runtime");

		runtime.block_on(lifecycle.run())
	});
	retain_lifecycle_task(shell, cancellation, views, background, cx);
}

pub(crate) fn retain_lifecycle_task<R: 'static>(
	shell: WeakEntity<Shell>,
	cancellation: LifecycleCancellation,
	views: Receiver<ConnectionView>,
	background: Task<R>,
	cx: &mut App,
) -> Entity<LifecycleOwner> {
	debug_assert!(
		!cx.has_global::<LifecycleOwnerGlobal>(),
		"the application retains exactly one lifecycle owner"
	);
	let owner = cx.new(|cx| LifecycleOwner::new(cancellation, views, background, shell, cx));
	cx.set_global(LifecycleOwnerGlobal { _owner: owner.clone() });

	owner
}

fn connection_requires_recovery(previous: ConnectionView, next: ConnectionView) -> bool {
	match (previous, next) {
		(
			ConnectionView::Online { generation: before, .. },
			ConnectionView::Online { generation: after, .. },
		) => before != after,
		_ => previous != next,
	}
}

#[test]
fn online_cursor_progress_does_not_invalidate_the_conversation() {
	let online = |generation, cursor| ConnectionView::Online {
		generation,
		applied: Some(decodex_protocol::Cursor(cursor)),
	};
	assert!(!connection_requires_recovery(online(1, 10), online(1, 11)));
	assert!(connection_requires_recovery(online(1, 10), online(2, 11)));
	assert!(connection_requires_recovery(online(1, 10), ConnectionView::Stopped));
}

fn publish_views(
	shell: &WeakEntity<Shell>,
	views: &Receiver<ConnectionView>,
	cx: &mut gpui::AsyncApp,
) {
	while let Ok(view) = views.try_recv() {
		let _ = shell.update(cx, |shell, cx| {
			if connection_requires_recovery(shell.connection, view) {
				shell.chief.update(cx, ChiefSurface::mark_stale);
			} else if shell.connection != view && matches!(view, ConnectionView::Online { .. }) {
				shell.chief.update(cx, |s, cx| s.refresh(cx));
			}
			shell.connection = view;
			cx.notify();
		});
	}
	let _ = shell.update(cx, |shell, cx| {
		shell.poll_account_login(cx);
		shell.poll_reset_cards(cx);
		let accounts = shell.accounts_controller.snapshot();
		let account_profile = shell.account_profile_controller.snapshot();
		let desktop_settings = shell.desktop_settings.snapshot();
		let health = shell.health_query.snapshot();
		if shell.selected == Destination::Conversations
			&& shell.pending_submission.is_none()
			&& shell.last_provider_sync.is_none_or(|last| last.elapsed() >= Duration::from_secs(60))
			&& shell.conversations.refresh_all().is_ok()
		{
			shell.last_provider_sync = Some(std::time::Instant::now());
		}
		let quick = shell.conversations.snapshot();
		let history = shell.history_pager.as_ref().map(HistoryPager::snapshot);

		if accounts != shell.accounts {
			shell.accounts = accounts;
			cx.notify();
		}
		if account_profile != shell.account_profile {
			shell.account_profile = account_profile;
			cx.notify();
		}
		if desktop_settings != shell.desktop_settings_snapshot {
			shell.desktop_settings_snapshot = desktop_settings;
			shell.settings.update(cx, SettingsSurface::synchronize);
			cx.notify();
		}
		if health != shell.health {
			shell.health = health;
			cx.notify();
		}
		if quick != shell.quick || history != shell.history {
			shell.synchronize_conversations(cx);
			shell.reconcile_pending_submission(cx);
			cx.notify();
		}
	});
}

fn compact_identity(value: &str) -> String {
	let prefix = value.chars().take(8).collect::<String>();
	if value.chars().count() > 8 { format!("{prefix}…") } else { prefix }
}

fn adjacent_conversation_index(current: Option<usize>, len: usize, delta: isize) -> Option<usize> {
	let last = len.checked_sub(1)?;
	Some(current.unwrap_or(0).saturating_add_signed(delta).min(last))
}

fn floating_window_controls(
	shell: &Shell,
	presentation: &ConnectionPresentation,
	_window: &Window,
	cx: &mut Context<Shell>,
) -> AnyElement {
	div()
		.id("floating-window-controls")
		.role(Role::Navigation)
		.aria_label("Window controls")
		.absolute()
		.top(px(ui_theme::CONTROL_MARGIN))
		.left(px(ui_theme::CONTROL_MARGIN))
		.right(px(ui_theme::CONTROL_MARGIN))
		.h(px(ui_theme::CONTROL_GROUP_HEIGHT))
		.flex()
		.items_center()
		.justify_between()
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
			ui_theme::floating_group()
				.pl(px(74.0))
				.child(chief_panel_control(shell, 0, cx))
				.child(shell.navigation_control(false, cx))
				.child(shell.navigation_control(true, cx)),
		)
		.child(topbar_controls(shell, presentation, cx))
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
			.text_size(px(11.0))
			.text_color(rgb(WB_TEXT))
			.child("Refresh health")
	}
}

struct ControlTooltip<T>(T);

impl<T: Clone + Into<SharedString> + 'static> Render for ControlTooltip<T> {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
			.px_2()
			.py_1()
			.rounded(px(6.0))
			.border_1()
			.border_color(rgba(0xffffff14))
			.bg(rgba(ui_theme::SURFACE_OVERLAY_MATERIAL))
			.text_size(px(11.0))
			.text_color(rgb(WB_TEXT))
			.child(self.0.clone().into())
	}
}

#[derive(Clone, Copy)]
struct HealthPresentation {
	label: &'static str,
	detail: &'static str,
	color: u32,
}

fn topbar_controls(
	shell: &Shell,
	_presentation: &ConnectionPresentation,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let left_sidebar_visible = shell.left_sidebar_visible;
	let inspector_visible = shell.inspector_visible;
	let settings_index = Destination::ALL
		.iter()
		.position(|destination| *destination == Destination::Settings)
		.expect("Settings destination");
	ui_theme::floating_group()
		.text_size(px(11.0))
		.when(shell.selected == Destination::Chief, |controls| {
			controls
				.when(shell.chief.read(cx).workspace_panels()[2].1, |group| {
					group.child(chief_panel_control(shell, 2, cx))
				})
				.when(shell.chief.read(cx).workspace_panels()[1].1, |group| {
					group.child(chief_panel_control(shell, 1, cx))
				})
				.when(shell.chief.read(cx).workspace_panels()[3].1, |group| {
					group.child(chief_panel_control(shell, 3, cx))
				})
		})
		.when(shell.selected == Destination::Conversations, |controls| {
			controls.child(topbar_sessions_toggle(left_sidebar_visible, cx))
		})
		.when(shell.selected == Destination::Conversations, |controls| {
			controls.child(topbar_inspector_toggle(inspector_visible, cx))
		})
		.child(
			div()
				.id("open-settings")
				.role(Role::Button)
				.aria_label("Open settings")
				.tooltip(|_, cx| cx.new(|_| ControlTooltip("Settings · Command-,")).into())
				.key_context("Destination")
				.track_focus(&shell.destination_focus[settings_index])
				.on_action(cx.listener(Shell::focus_next))
				.on_action(cx.listener(Shell::focus_previous))
				.on_action(cx.listener(Shell::activate_destination))
				.size(px(ui_theme::CHROME_CONTROL_SIZE))
				.flex()
				.items_center()
				.justify_center()
				.rounded(px(6.0))
				.border_1()
				.border_color(rgba(0x00000000))
				.bg(
					if matches!(
						shell.selected,
						Destination::Settings | Destination::Accounts | Destination::Health
					) {
						rgba(0xffffff0a)
					} else {
						rgba(0x00000000)
					},
				)
				.text_color(
					if matches!(
						shell.selected,
						Destination::Settings | Destination::Accounts | Destination::Health
					) {
						rgb(WB_TEXT)
					} else {
						rgb(WB_TEXT_MUTED)
					},
				)
				.occlude()
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
				.active(|element| element.bg(rgba(0xffffff14)))
				.focus_visible(|element| element.border_color(rgba(0x8baaf780)))
				.on_mouse_down(MouseButton::Left, |_, window, cx| {
					window.prevent_default();
					cx.stop_propagation();
				})
				.on_click(cx.listener(|shell, _, _, cx| {
					shell.open_settings_window(Destination::Settings, cx);
				}))
				.child(workspace_symbols::icon(workspace_symbols::Symbol::Settings))
				.smooth(),
		)
		.into_any_element()
}

fn chief_panel_control(shell: &Shell, index: usize, cx: &Context<Shell>) -> AnyElement {
	let (active, enabled) = if index == 0 && shell.selected != Destination::Chief {
		(shell.left_sidebar_visible, true)
	} else {
		shell.chief.read(cx).workspace_panels()[index]
	};
	let label = match (index, enabled) {
		(0, _) => "Toggle sidebar · Command-E",
		(1, true) => "Toggle work graph · Command-J",
		(2, true) => "Toggle history rail",
		(3, _) => "Toggle agent structure · Command-B",
		(1, false) => "Work graph · no work yet",
		_ => "History · no messages yet",
	};
	div()
		.id(("chief-panel-control", index))
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label)
		.aria_expanded(active)
		.tooltip(move |_, cx| cx.new(|_| ControlTooltip(label)).into())
		.size(px(ui_theme::CHROME_CONTROL_SIZE))
		.rounded(px(5.0))
		.flex()
		.items_center()
		.justify_center()
		.when(active, |el| el.bg(rgba(0xffffff0c)))
		.when(!enabled, |el| el.opacity(0.35))
		.hover(|el| el.bg(rgba(0xffffff12)))
		.cursor_pointer()
		.occlude()
		.on_mouse_down(MouseButton::Left, |_, window, cx| {
			window.prevent_default();
			cx.stop_propagation();
		})
		.on_click(cx.listener(move |s, _, _, cx| {
			if enabled && index == 0 && s.selected != Destination::Chief {
				s.set_left_sidebar_visible(!s.left_sidebar_visible, cx);
			} else if enabled {
				s.chief.update(cx, |chief, cx| match index {
					0 => chief.toggle_workspace_sidebar(cx),
					1 => chief.toggle_workspace_graph(cx),
					2 => chief.toggle_workspace_timeline(cx),
					_ => chief.toggle_agent_tree(cx),
				});
			}
		}))
		.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
			if enabled && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
				if index == 0 && s.selected != Destination::Chief {
					s.set_left_sidebar_visible(!s.left_sidebar_visible, cx);
					cx.stop_propagation();
					return;
				}
				s.chief.update(cx, |chief, cx| match index {
					0 => chief.toggle_workspace_sidebar(cx),
					1 => chief.toggle_workspace_graph(cx),
					2 => chief.toggle_workspace_timeline(cx),
					_ => chief.toggle_agent_tree(cx),
				});
				cx.stop_propagation();
			}
		}))
		.child(ChiefSurface::panel_glyph(index))
		.smooth()
		.enabled(enabled)
		.into_any_element()
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
		DoctorComponent::Conversation => "Conversation",
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
		.w_full()
		.min_h(px(38.0))
		.py_2()
		.flex()
		.items_center()
		.justify_between()
		.gap_4()
		.border_b_1()
		.border_color(rgba(0xffffff0a))
		.text_size(px(11.0))
		.text_color(rgb(WB_TEXT_MUTED))
		.child(div().flex_1().min_w_0().flex().flex_col().gap_1().child(label).when(
			!presentation.detail.is_empty()
				&& !matches!(presentation.label, "Not checked" | "Disabled"),
			|element| {
				element.child(
					div()
						.text_size(px(11.0))
						.text_color(rgb(WB_TEXT_FAINT))
						.child(presentation.detail),
				)
			},
		))
		.child(
			div()
				.flex_none()
				.justify_end()
				.w(px(120.0))
				.min_w(px(120.0))
				.flex()
				.items_center()
				.gap_2()
				.child(
					div().size(px(5.0)).min_w(px(5.0)).rounded_full().bg(rgb(presentation.color)),
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
				.items_center()
				.justify_between()
				.gap_4()
				.child(
					div()
						.text_size(px(11.0))
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(rgb(WB_TEXT))
						.child(title),
				)
				.child(div().text_size(px(11.0)).text_color(rgb(WB_TEXT_FAINT)).child(detail)),
		)
		.child(
			div()
				.id(("health-section-list", index_offset))
				.role(Role::List)
				.aria_label(title)
				.px_4()
				.border_1()
				.border_color(rgba(0xffffff10))
				.rounded(px(10.0))
				.bg(rgba(0xffffff04))
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
		.text_size(px(11.0))
		.child("Refresh")
		.smooth()
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
				.text_size(px(ui_theme::HEADING_SIZE))
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(rgb(WB_TEXT))
				.child(selected.label()),
		);
	let header = div()
		.h(px(64.0))
		.min_h(px(64.0))
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
						.font_family(ui_theme::FONT_FAMILY)
						.text_size(px(11.0))
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
						.font_family(ui_theme::FONT_FAMILY)
						.text_size(px(11.0))
						.text_color(rgb(WB_TEXT_FAINT))
						.child(
							"No speculative controls are exposed before this projection has an authority owner.",
						),
				),
		)
		.into_any_element()
}

fn account_pool_rows(shell: &Shell, cx: &mut Context<Shell>) -> Vec<AnyElement> {
	let snapshot = &shell.accounts;
	let fixed = snapshot.routing.as_ref().and_then(|routing| match &routing.mode {
		AccountSelectionModeDto::Fixed(account_id) => Some(account_id),
		AccountSelectionModeDto::Balanced => None,
	});
	snapshot
		.accounts
		.iter()
		.enumerate()
		.map(|(index, account)| {
			let row = account_pool_row(
				account,
				AccountRowPresentation {
					reset_fill: shell.reset_fill_for(account),
					email: shell.account_emails.get(account),
					controls_busy: snapshot.controls_busy(),
					index,
					routing_revision: snapshot.routing.as_ref().map(|routing| routing.revision),
					fixed: fixed == Some(&account.account_id),
					can_manage: snapshot.can_manage,
					can_route: snapshot.can_route,
					login_available: shell.account_login.is_some()
						&& shell.account_login_task.is_none(),
					logout_pending: shell.pending_account_logout.as_ref()
						== Some(&account.account_id),
				},
				cx,
			);
			div()
				.w_full()
				.flex()
				.flex_col()
				.gap_1()
				.child(row)
				.when(shell.account_profile.selected.as_ref() == Some(&account.account_id), |row| {
					row.child(account_profile_panel(shell, cx))
				})
				.when(shell.reset_cards.is_selected(&account.account_id), |row| {
					row.children(reset_cards::panel(shell, cx))
				})
				.into_any_element()
		})
		.collect()
}

fn accounts_content(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let snapshot = &shell.accounts;
	let rows = account_pool_rows(shell, cx);
	let balanced = snapshot
		.routing
		.as_ref()
		.is_some_and(|routing| routing.mode == AccountSelectionModeDto::Balanced);
	let can_manage = snapshot.can_manage;
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
		.px(px(ui_theme::SETTINGS_INSET))
		.pt(px(ui_theme::SETTINGS_GROUP_GAP))
		.pb(px(ui_theme::SETTINGS_INSET))
		.flex()
		.justify_center()
		.child(
			div()
				.w_full()
				.max_w(px(ui_theme::SETTINGS_WIDTH))
				.min_h_0()
				.flex()
				.flex_col()
				.gap_3()
				.child(account_pool_header(
					count,
					available,
					balanced,
					can_manage,
					shell.account_emails.visible,
					cx,
				))
				.child(account_login_controls(shell, cx))
				.child(shell.settings.update(cx, |settings, cx| settings.quota_control(cx)))
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
									.h(px(100.0))
									.flex()
									.flex_col()
									.items_center()
									.justify_center()
									.gap_2()
									.rounded(px(12.0))
									.border_1()
									.border_color(rgba(0xffffff0d))
									.bg(rgba(0xffffff04))
									.text_size(px(11.0))
									.text_color(rgb(WB_TEXT_MUTED))
									.child("No accounts added")
									.child(
										div()
											.font_family(ui_theme::FONT_FAMILY)
											.text_size(px(11.0))
											.text_color(rgb(WB_TEXT_FAINT))
											.child("Sign in above to add your first account."),
									),
							)
						})
						.children(rows),
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
		.bg(if selected { rgba(0x60a5fa16) } else { rgba(0x00000000) })
		.text_size(px(11.0))
		.text_color(if selected { rgb(WB_TEXT) } else { rgb(WB_TEXT_MUTED) })
		.when(can_manage && !selected, |button| {
			button
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(WB_TEXT)))
				.active(|element| element.bg(rgba(0xffffff1b)).opacity(0.84))
				.on_click(cx.listener(|shell, _, _, cx| shell.select_balanced_accounts(cx)))
		})
		.child(label)
		.smooth()
		.into_any_element()
}

fn account_login_controls(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let busy = shell.account_login_task.is_some();
	let available = shell.account_login.is_some();
	let prompt = shell.account_login_status.as_ref().and_then(|status| status.prompt.as_ref()).map(
		|prompt| {
			(prompt.user_code.as_str().to_owned(), prompt.verification_url.as_str().to_owned())
		},
	);
	let status = shell
		.account_login_status
		.as_ref()
		.filter(|status| {
			!matches!(
				status.state,
				AccountLoginState::Completed
					| AccountLoginState::Failed
					| AccountLoginState::Cancelled
			)
		})
		.map(account_login_status_label);

	div()
		.id("account-login-controls")
		.px_3()
		.py(px(6.))
		.flex()
		.items_center()
		.justify_between()
		.gap_4()
		.rounded(px(10.0))
		.child(
			div()
				.flex_1()
				.min_w_0()
				.flex()
				.flex_col()
				.gap_1()
				.child(
					div()
						.font_family(ui_theme::FONT_FAMILY)
						.text_size(px(11.0))
						.text_color(rgb(WB_TEXT))
						.child("Add account"),
				)
				.when_some(status, |row, status| {
					row.child(
						div().text_size(px(10.5)).text_color(rgb(WB_TEXT_MUTED)).child(status),
					)
				})
				.when_some(prompt, |details, (code, url)| {
					details.child(account_login_prompt(code, url))
				}),
		)
		.child(
			div()
				.flex()
				.items_center()
				.gap_2()
				.child(
					account_login_button("account-login-browser", "Browser", available && !busy)
						.when(available && !busy, |button| {
							button.on_click(cx.listener(|shell, _, _, cx| {
								shell.start_account_enrollment(
									AccountLoginMethod::BrowserRedirect,
									cx,
								);
							}))
						}),
				)
				.child(
					account_login_button("account-login-device", "Device code", available && !busy)
						.when(available && !busy, |button| {
							button.on_click(cx.listener(|shell, _, _, cx| {
								shell.start_account_enrollment(AccountLoginMethod::DeviceCode, cx);
							}))
						}),
				)
				.when(
					shell.account_login_status.as_ref().is_some_and(|status| {
						status.prompt.is_some() || status.authorization_url.is_some()
					}),
					|actions| {
						actions
							.child(
								account_login_button("account-login-copy", "Copy code", true)
									.on_click(cx.listener(|shell, _, _, cx| {
										shell.copy_account_login_code(cx);
									})),
							)
							.child(
								account_login_button("account-login-open", "Open", true).on_click(
									cx.listener(|shell, _, _, cx| {
										shell.open_account_login_url(cx);
									}),
								),
							)
					},
				)
				.when(busy, |actions| {
					actions.child(
						account_login_button("account-login-cancel", "Cancel", true).on_click(
							cx.listener(|shell, _, _, cx| {
								shell.cancel_account_login(cx);
							}),
						),
					)
				}),
		)
		.into_any_element()
}

fn account_login_prompt(code: String, url: String) -> AnyElement {
	div()
		.pt_1()
		.flex()
		.items_center()
		.gap_2()
		.child(
			div().font_family("SF Mono").text_size(px(13.0)).text_color(rgb(WB_TEXT)).child(code),
		)
		.child(
			div()
				.max_w(px(360.0))
				.overflow_hidden()
				.whitespace_nowrap()
				.text_ellipsis()
				.font_family(ui_theme::FONT_FAMILY)
				.text_size(px(11.0))
				.text_color(rgb(WB_TEXT_FAINT))
				.child(url),
		)
		.into_any_element()
}

fn account_profile_panel(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let selected = shell
		.account_profile
		.selected
		.as_ref()
		.map(|account| account.as_str().to_owned())
		.unwrap_or_default();
	let (status, facts) = match shell.account_profile.result.as_ref() {
		Some(AccountProfileResult::Current(profile)) =>
			("Account profile".to_owned(), account_profile_facts(profile)),
		Some(AccountProfileResult::Cached { profile, .. }) =>
			("Cached profile".to_owned(), account_profile_facts(profile)),
		Some(AccountProfileResult::Unavailable { plan_type, .. }) => (
			"No current profile".to_owned(),
			plan_type
				.as_ref()
				.map(|plan| vec![format!("Plan · {}", account_plan_label(plan.as_str()))])
				.unwrap_or_default(),
		),
		None => (
			if shell.account_profile.load == AccountProfileLoadState::Loading {
				"Loading profile…"
			} else {
				"No current profile"
			}
			.to_owned(),
			Vec::new(),
		),
	};

	div()
		.id("account-profile-panel")
		.px_4()
		.py_3()
		.flex()
		.items_center()
		.justify_between()
		.gap_3()
		.rounded(px(10.0))
		.border_1()
		.border_color(rgba(0xffffff12))
		.bg(rgba(0xffffff04))
		.child(
			div()
				.flex_1()
				.min_w_0()
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
								.font_family(ui_theme::FONT_FAMILY)
								.text_size(px(11.0))
								.text_color(rgb(WB_BLUE))
								.child("Account details"),
						)
						.child(
							div()
								.font_family(ui_theme::FONT_FAMILY)
								.text_size(px(11.0))
								.text_color(rgb(WB_TEXT_FAINT))
								.child(selected),
						),
				)
				.child(div().text_size(px(11.0)).text_color(rgb(WB_TEXT_MUTED)).child(status))
				.child(div().flex().flex_wrap().gap_2().children(facts.into_iter().map(|fact| {
					div()
						.px_2()
						.py_1()
						.rounded(px(5.0))
						.bg(rgba(0xffffff08))
						.font_family(ui_theme::FONT_FAMILY)
						.text_size(px(11.0))
						.text_color(rgb(WB_TEXT_FAINT))
						.child(fact)
				}))),
		)
		.child(
			div()
				.flex()
				.items_center()
				.gap_2()
				.child(
					account_login_button(
						"account-profile-refresh",
						"Refresh",
						shell.account_profile.can_refresh,
					)
					.when(shell.account_profile.can_refresh, |button| {
						button.on_click(cx.listener(|shell, _, _, cx| {
							shell.refresh_account_profile(cx);
						}))
					}),
				)
				.child(
					account_login_button("account-profile-close", "Close", true)
						.on_click(cx.listener(|shell, _, _, cx| shell.close_account_profile(cx))),
				),
		)
		.into_any_element()
}

// Match native account labels; preserve unknown provider values and stored SKU identity.
fn account_plan_label(plan: &str) -> &str {
	match plan.to_ascii_lowercase().as_str() {
		"free" => "Free",
		"go" => "Go",
		"plus" => "Plus",
		"pro" => "Pro",
		"prolite" => "Pro Lite",
		"self_serve_business_prolite" => "Business Premium",
		"team" | "self_serve_business_usage_based" => "Business",
		"enterprise_cbp_automation" => "Enterprise (Automation)",
		"business" | "ent26" | "enterprise_cbp_usage_based" | "enterprise" | "hc" => "Enterprise",
		"edu" | "education" => "Edu",
		"edu_plus" => "Edu Plus",
		"edu_pro" => "Edu Pro",
		"unknown" => "Unknown",
		_ => plan,
	}
}

fn account_profile_facts(profile: &decodex_protocol::AccountProfileDto) -> Vec<String> {
	let mut facts = Vec::new();
	if let Some(plan) = &profile.plan_type {
		facts.push(format!("Plan · {}", account_plan_label(plan.as_str())));
	}
	if let Some(tokens) = profile.lifetime_tokens {
		facts.push(format!("Lifetime · {} tokens", chief_surface::compact_tokens(tokens)));
	}
	if let Some(tokens) = profile.peak_daily_tokens {
		facts.push(format!("Peak day · {} tokens", chief_surface::compact_tokens(tokens)));
	}
	if let Some(days) = profile.current_streak_days {
		facts.push(format!("Streak · {days} days"));
	}
	if let Some(seconds) = profile.longest_task_seconds {
		facts.push(format!("Longest task · {seconds}s"));
	}
	if !profile.daily_usage.is_empty() {
		facts.push(format!("{} days recorded", profile.daily_usage.len()));
	}
	facts
}

const fn account_profile_load_label(load: AccountProfileLoadState) -> &'static str {
	match load {
		AccountProfileLoadState::Closed => "Select an account profile.",
		AccountProfileLoadState::Loading => "Loading the daemon-owned profile…",
		AccountProfileLoadState::Ready => "Profile loaded.",
		AccountProfileLoadState::Offline => "Profile is offline.",
		AccountProfileLoadState::Refused => "The profile response was refused.",
	}
}

fn account_login_button(
	id: &'static str,
	label: &'static str,
	enabled: bool,
) -> gpui::Stateful<gpui::Div> {
	div()
		.id(id)
		.role(Role::Button)
		.aria_label(label)
		.h(px(27.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(7.0))
		.text_size(px(11.0))
		.text_color(rgb(if enabled { WB_TEXT_MUTED } else { WB_TEXT_FAINT }))
		.opacity(if enabled { 1.0 } else { 0.55 })
		.when(enabled, |button| {
			button
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(WB_TEXT)))
				.active(|element| element.bg(rgba(0xffffff1b)).opacity(0.84))
		})
		.child(label)
}

fn account_login_status_label(status: &AccountLoginStatus) -> String {
	match status.state {
		AccountLoginState::OpeningBrowser => "Preparing browser login…".into(),
		AccountLoginState::RequestingCode => "Requesting a device code…".into(),
		AccountLoginState::WaitingForBrowser =>
			"Complete sign-in in the browser, then return to Decodex.".into(),
		AccountLoginState::Installing =>
			"Installing the verified account through the Decodex service…".into(),
		AccountLoginState::Completed => status.resolved_account_id.as_ref().map_or_else(
			|| "Account login completed.".into(),
			|account_id| format!("Account {} is ready.", account_id.as_str()),
		),
		AccountLoginState::Failed => status.failure.map_or_else(
			|| "Account login failed.".into(),
			|failure| format!("Account login failed: {failure:?}."),
		),
		AccountLoginState::Cancelled => "Account login was cancelled.".into(),
	}
}

fn account_login_start(
	method: AccountLoginMethod,
	existing: Option<(EntityId, decodex_protocol::EntityRevision, Option<EntityId>)>,
) -> Result<AccountLoginStart, SharedString> {
	let next_entity = || {
		canonical_uuid_v4()
			.map_err(account_input_error_label)
			.and_then(|value| EntityId::new(value).map_err(|_| "Login identity is invalid."))
			.map_err(SharedString::from)
	};
	let session_id = next_entity()?;
	let operation_id = next_entity()?;
	let command_identity =
		canonical_uuid_v4().map_err(account_input_error_label).map_err(SharedString::from)?;
	let idempotency_key = IdempotencyKey::new(format!("account-login/{command_identity}"))
		.map_err(|_| SharedString::from("Login command identity is invalid."))?;
	let install_mode =
		if let Some((account_id, expected_revision, recovery_operation_id)) = existing {
			AccountLoginInstallMode::Reauthenticate {
				operation_id,
				account_id,
				expected_revision,
				recovery_operation_id,
				idempotency_key,
			}
		} else {
			AccountLoginInstallMode::Enroll {
				operation_id,
				account_id: next_entity()?,
				enabled: true,
				idempotency_key,
			}
		};
	let start = AccountLoginStart { session_id, method, install_mode };
	start.validate().map_err(|_| SharedString::from("Login request is invalid."))?;
	Ok(start)
}

#[derive(Clone)]
struct AccountDrag {
	id: EntityId,
	revision: Option<decodex_protocol::EntityRevision>,
	label: SharedString,
}
impl Render for AccountDrag {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
			.px_3()
			.py_2()
			.rounded(px(8.))
			.bg(rgb(0x29292d))
			.text_color(rgb(WB_TEXT))
			.text_size(px(12.))
			.child(self.label.clone())
	}
}
impl Shell {
	fn drop_account(&mut self, drag: &AccountDrag, target: &EntityId, cx: &mut Context<Self>) {
		let Some(routing) = &self.accounts.routing else { return };
		if !self.accounts.can_manage || Some(routing.revision) != drag.revision {
			return;
		}
		let Some(from) = routing.order.iter().position(|id| id == &drag.id) else { return };
		let Some(to) = routing.order.iter().position(|id| id == target) else { return };
		if from != to {
			self.move_account(&drag.id, to as isize - from as isize, cx);
		}
	}
}

#[derive(Clone)]
struct AccountRowPresentation {
	controls_busy: bool,
	email: Option<String>,
	reset_fill: Option<quota_meter::ResetFill>,
	index: usize,
	routing_revision: Option<decodex_protocol::EntityRevision>,
	fixed: bool,
	can_manage: bool,
	can_route: bool,
	login_available: bool,
	logout_pending: bool,
}

fn account_login_recovery_operation_id(account: &AccountDto) -> Option<EntityId> {
	account.unsettled_operation.as_ref().and_then(|operation| {
		(operation.kind == decodex_protocol::AccountOperationKindDto::Refresh
			&& operation.phase == decodex_protocol::AccountOperationPhaseDto::RecoveryRequired
			&& operation.recovery_code.as_ref().is_some_and(|code| {
				matches!(
					code.as_str(),
					"provider_refresh_rejected"
						| "provider_refresh_ambiguous"
						| "provider_refresh_outcome_unknown"
						| "provider_access_rejected_after_refresh"
				)
			}))
		.then(|| operation.operation_id.clone())
	})
}

fn account_needs_login(account: &AccountDto) -> bool {
	account.observed_state == AccountObservedStateDto::AuthFailed
		|| account.lifecycle_readiness == AccountLifecycleReadinessDto::CredentialAbsent
		|| account.lifecycle_readiness == AccountLifecycleReadinessDto::Tombstoned
		|| account_login_recovery_operation_id(account).is_some()
}

fn account_readiness_status(account: &AccountDto) -> &'static str {
	match account
		.unsettled_operation
		.as_ref()
		.and_then(|operation| operation.recovery_code.as_ref())
		.map(decodex_protocol::WireText::as_str)
	{
		Some("provider_refresh_rejected") => "Refresh rejected · re-login",
		Some("provider_refresh_ambiguous" | "provider_refresh_outcome_unknown") =>
			"Refresh uncertain · re-login",
		Some("provider_access_rejected_after_refresh") => "New login required · re-login",
		_ => account_readiness_label(account.lifecycle_readiness),
	}
}

fn account_pool_row(
	account: &AccountDto,
	presentation: AccountRowPresentation,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let AccountRowPresentation { index, fixed, can_manage, .. } = presentation;
	let summary = account_pool_summary(account, presentation.clone(), cx);
	div()
		.w_full()
		.rounded(px(8.))
		.bg(rgba(0xffffff04))
		.id(("account-row", index))
		.px(px(14.0))
		.py(px(6.0))
		.flex()
		.flex_col()
		.gap(px(6.0))
		.relative()
		.when(fixed, |d| d.bg(rgba(0xffffff0a)))
		.child(summary)
		.when(can_manage, |row| {
			let drag = AccountDrag {
				id: account.account_id.clone(),
				revision: presentation.routing_revision,
				label: account.alias.as_str().into(),
			};
			let target = account.account_id.clone();
			let keyboard = target.clone();
			row.on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
				.drag_over::<AccountDrag>(|style, _, _, _| style.bg(rgba(0x8baaf72a)))
				.on_drop(cx.listener(move |s, drag: &AccountDrag, _, cx| {
					s.drop_account(drag, &target, cx)
				}))
				.tab_index(0)
				.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
					if event.keystroke.modifiers.alt
						&& ["up", "down"].contains(&event.keystroke.key.as_str())
					{
						s.move_account(
							&keyboard,
							if event.keystroke.key == "up" { -1 } else { 1 },
							cx,
						);
						cx.stop_propagation();
					}
				}))
		})
		.into_any_element()
}

fn account_pool_summary(
	account: &AccountDto,
	presentation: AccountRowPresentation,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let AccountRowPresentation { index, fixed, can_route, .. } = presentation;
	let account_id = account.account_id.clone();
	let enabled = account.enabled;
	let pin_enabled = can_route && enabled && !fixed;

	let profile_id = account.account_id.clone();
	div()
		.id(("account-summary", index))
		.cursor_pointer()
		.hover(|style| style.bg(rgba(0xffffff05)))
		.on_click(cx.listener(move |shell, _, _, cx| {
			if shell.account_profile.selected.as_ref() == Some(&profile_id) {
				shell.close_account_profile(cx);
			} else {
				shell.show_account_profile(profile_id.clone(), cx);
			}
		}))
		.flex()
		.min_w_0()
		.items_center()
		.gap(px(8.0))
		.child(account_power_control(
			account,
			index,
			presentation.can_manage,
			presentation.controls_busy,
			cx,
		))
		.child(account_row_identity(account, presentation.email.as_deref()))
		.child(
			div().flex_1().min_w_0().flex().items_center().gap(px(8.0)).children(
				[
					quota_meter::meter(
						"5 hours",
						account.five_hour_quota,
						presentation.reset_fill.clone(),
					),
					quota_meter::meter(
						"7 days",
						account.seven_day_quota,
						presentation.reset_fill.clone(),
					),
				]
				.into_iter()
				.flatten(),
			),
		)
		.child(
			div().flex().items_center().gap_2().child(
				div()
					.id(("account-pin", index))
					.role(Role::Button)
					.aria_label(format!("Route new conversations to {}", account.alias.as_str()))
					.h(px(26.0))
					.w(px(26.0))
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.rounded(px(7.0))
					.bg(if fixed { rgba(0x8baaf738) } else { rgba(0x00000000) })
					.text_size(px(11.0))
					.text_color(if fixed { rgb(WB_BLUE) } else { rgb(WB_TEXT_MUTED) })
					.when(pin_enabled, |button| {
						button
							.cursor_pointer()
							.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(WB_TEXT)))
							.active(|element| element.bg(rgba(0xffffff1b)).opacity(0.84))
							.on_click(cx.listener(move |shell, _, _, cx| {
								cx.stop_propagation();
								shell.select_fixed_account(&account_id, cx);
							}))
					})
					.child(workspace_symbols::icon(if fixed {
						workspace_symbols::Symbol::AccountRouteActive
					} else {
						workspace_symbols::Symbol::AccountRoute
					}))
					.smooth(),
			),
		)
		.child(account_management_actions(account, &presentation, cx))
		.into_any_element()
}

fn account_power_control(
	account: &AccountDto,
	index: usize,
	interactive: bool,
	busy: bool,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let id = account.account_id.clone();
	let enabled = account.enabled;
	let key_id = id.clone();
	account_icon_action(
		"account-enabled",
		index,
		if enabled { "Disable account" } else { "Enable account" },
		if enabled {
			workspace_symbols::Symbol::PowerOn
		} else {
			workspace_symbols::Symbol::PowerOff
		},
		interactive,
	)
	.when(busy, |button| button.opacity(1.0))
	.role(Role::Switch)
	.aria_toggled(if enabled {
		gpui::accesskit::Toggled::True
	} else {
		gpui::accesskit::Toggled::False
	})
	.flex_none()
	.on_click(cx.listener(move |s, _, _, cx| {
		cx.stop_propagation();
		if interactive {
			s.set_account_enabled(&id, !enabled, cx);
		}
	}))
	.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
		if interactive && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
			cx.stop_propagation();
			s.set_account_enabled(&key_id, !enabled, cx);
		}
	}))
	.smooth()
	.into_any_element()
}

fn account_management_actions(
	account: &AccountDto,
	presentation: &AccountRowPresentation,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let index = presentation.index;
	let login_account_id = account.account_id.clone();
	let reset_account_id = account.account_id.clone();
	let reset_alias = account.alias.as_str().to_owned();
	let logout_account_id = account.account_id.clone();
	let login_account_revision = account.account_revision;
	let login_recovery_operation_id = account_login_recovery_operation_id(account);

	div()
		.flex()
		.justify_start()
		.items_center()
		.gap_1()
		.child(
			account_icon_action(
				"account-reset-cards",
				index,
				"Show Reset Cards",
				workspace_symbols::Symbol::AccountLogin,
				true,
			)
			.on_click(cx.listener(move |shell, _, _, cx| {
				cx.stop_propagation();
				shell.show_reset_cards(reset_account_id.clone(), reset_alias.clone(), cx)
			})),
		)
		.child(
			div()
				.flex()
				.items_center()
				.gap_1()
				.when(account_needs_login(account), |row| {
					row.child(
						account_icon_action(
							"account-login",
							index,
							"Sign in again",
							workspace_symbols::Symbol::AccountLogout,
							presentation.login_available,
						)
						.when(presentation.login_available, |button| {
							button.on_click(cx.listener(move |shell, _, _, cx| {
								cx.stop_propagation();
								shell.start_account_reauthentication(
									login_account_id.clone(),
									login_account_revision,
									login_recovery_operation_id.clone(),
									cx,
								);
							}))
						}),
					)
				})
				.child(
					account_icon_action(
						"account-logout",
						index,
						"Log out account",
						if presentation.logout_pending {
							workspace_symbols::Symbol::Confirm
						} else {
							workspace_symbols::Symbol::AccountLogout
						},
						presentation.can_manage,
					)
					.when(presentation.controls_busy, |button| button.opacity(1.0))
					.when(presentation.can_manage, |button| {
						button.on_click(cx.listener(move |shell, _, _, cx| {
							cx.stop_propagation();
							shell.logout_account(&logout_account_id, cx);
						}))
					}),
				),
		)
		.into_any_element()
}

fn account_icon_action(
	id: &'static str,
	index: usize,
	label: &'static str,
	symbol: workspace_symbols::Symbol,
	enabled: bool,
) -> gpui::Stateful<gpui::Div> {
	account_row_action(id, index, label, "", enabled)
		.w(px(26.))
		.h(px(26.))
		.px_0()
		.tab_index(0)
		.tooltip(move |_, cx| cx.new(|_| ControlTooltip(label)).into())
		.child(workspace_symbols::icon(symbol))
}

fn account_row_action(
	id: &'static str,
	index: usize,
	aria_label: &'static str,
	label: &'static str,
	enabled: bool,
) -> gpui::Stateful<gpui::Div> {
	div()
		.id((id, index))
		.role(Role::Button)
		.aria_label(aria_label)
		.h(px(23.0))
		.px_2()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(6.0))
		.font_family(ui_theme::FONT_FAMILY)
		.text_size(px(11.0))
		.text_color(rgb(if enabled { WB_TEXT_MUTED } else { WB_TEXT_FAINT }))
		.opacity(if enabled { 1.0 } else { 0.5 })
		.when(enabled, |button| {
			button
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(WB_TEXT)))
				.active(|element| element.opacity(0.82))
		})
		.child(label)
}

#[cfg(test)]
fn account_quota(label: &'static str, quota: AccountQuotaWindowDto) -> Option<AnyElement> {
	quota_meter::meter(label, quota, None)
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
		AccountCommandState::Sending | AccountCommandState::AwaitingResult => Some("Switching"),
		AccountCommandState::Accepted => Some("Ready"),
		AccountCommandState::OutcomeUnknown => Some("Restart Decodex."),
		AccountCommandState::Refused => Some("The account change was refused. Refresh and retry."),
	}
}

fn account_rejection_label(rejection: AccountCommandRejectionDto) -> &'static str {
	match rejection {
		AccountCommandRejectionDto::CodexIsRunning =>
			"Quit ChatGPT or Codex, then try switching again.",
		AccountCommandRejectionDto::AccountDisabled =>
			"Enable this account before switching to it.",
		AccountCommandRejectionDto::CredentialMissing =>
			"This account has no saved credential. Sign in again.",
		AccountCommandRejectionDto::CredentialNeedsLogin =>
			"This account needs you to sign in again.",
		AccountCommandRejectionDto::CredentialRefreshRejected =>
			"ChatGPT rejected the credential refresh. Sign in again.",
		AccountCommandRejectionDto::CredentialRefreshUnavailable =>
			"Credential refresh is unavailable. Try again later.",
		AccountCommandRejectionDto::AuthFileUnreadable =>
			"The Codex authentication file could not be read.",
		AccountCommandRejectionDto::AuthFileChanged =>
			"The Codex authentication file changed. Try switching again.",
		AccountCommandRejectionDto::AuthWriteFailed =>
			"The selected account could not be written to the Codex authentication file.",
		AccountCommandRejectionDto::AuthReadbackMismatch =>
			"The Codex authentication file did not match after switching.",
		_ => "The account change was refused. Refresh and retry.",
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

fn conversation_state_label(state: ConversationState) -> &'static str {
	match state {
		ConversationState::RoutingPending => "Routing pending",
		ConversationState::EstablishmentPending => "Establishment pending",
		ConversationState::QuotaExhausted => "Quota exhausted",
		ConversationState::NoRoute => "No route",
		ConversationState::Establishing => "Establishing",
		ConversationState::Ready => "Ready",
		ConversationState::Running => "Running",
		ConversationState::ManualRecovery => "Action required",
		ConversationState::OutcomeUnknown => "Recovering",
	}
}

fn conversation_state_color(state: ConversationState) -> u32 {
	match state {
		ConversationState::Ready => 0x22c55e,
		ConversationState::Running | ConversationState::Establishing => 0x60a5fa,
		ConversationState::RoutingPending
		| ConversationState::EstablishmentPending
		| ConversationState::QuotaExhausted
		| ConversationState::NoRoute => 0xf59e0b,
		ConversationState::ManualRecovery => 0xef4444,
		ConversationState::OutcomeUnknown => 0xf59e0b,
	}
}

fn command_status(command: ConversationCommandState) -> Option<&'static str> {
	match command {
		ConversationCommandState::Idle => None,
		ConversationCommandState::Sending => Some("Sending command"),
		ConversationCommandState::AwaitingResult => Some("Waiting for durable result"),
		ConversationCommandState::Accepted => Some("Command accepted"),
		ConversationCommandState::ManualRecovery(action) => Some(recovery_action_label(action)),
		ConversationCommandState::OutcomeUnknown =>
			Some("Submission could not be confirmed. Check its saved state before sending again."),
		ConversationCommandState::Refused => Some("The command was refused."),
	}
}

fn recovery_action_label(action: ConversationRecoveryAction) -> &'static str {
	match action {
		ConversationRecoveryAction::ResumeRouting => "Resume the pending account route.",
		ConversationRecoveryAction::CreateRoutingSuccessor =>
			"Create a new conversation and route it explicitly.",
		ConversationRecoveryAction::ResumeEstablishment =>
			"Resume the selected account session establishment.",
		ConversationRecoveryAction::ConfigureAccount => "Configure an account before continuing.",
		ConversationRecoveryAction::EnableAccount =>
			"Enable the selected account before continuing.",
		ConversationRecoveryAction::EnrollCredentials =>
			"Enroll account credentials before continuing.",
		ConversationRecoveryAction::ResolveAccountOperation =>
			"Resolve the unsettled account operation before continuing.",
		ConversationRecoveryAction::RepairCredentialStore =>
			"Repair the protected credential store before continuing.",
		ConversationRecoveryAction::RestoreProviderAgreement =>
			"Restore provider account agreement before continuing.",
		ConversationRecoveryAction::RefreshQuota => "Refresh account quota before continuing.",
		ConversationRecoveryAction::UpgradeCodex =>
			"Use a Codex build with the required app-server methods.",
		ConversationRecoveryAction::SelectWorkingDirectory =>
			"Select an owned local working directory before continuing.",
		ConversationRecoveryAction::StartNewConversation =>
			"This thread cannot resume. Start a new conversation.",
		ConversationRecoveryAction::ResolvePriorActiveTurn =>
			"Resolve the prior active turn before continuing.",
		ConversationRecoveryAction::ResolvePriorAttempt =>
			"Resolve the prior provider attempt before continuing.",
		ConversationRecoveryAction::RestoreArchivedThread =>
			"Unarchive the existing Codex thread, then refresh this conversation.",
		ConversationRecoveryAction::WaitForThreadClose =>
			"Codex is still closing this thread. Wait briefly, then refresh this conversation.",
		ConversationRecoveryAction::ReviewSandboxConfiguration =>
			"Check Codex sandbox permissions and writable roots, then refresh this conversation.",
		ConversationRecoveryAction::ReviewCodexConfiguration =>
			"Codex rejected resume. Check its model, provider and project configuration, then refresh.",
		ConversationRecoveryAction::RestoreProcessReadiness =>
			"Restore process readiness before continuing.",
		ConversationRecoveryAction::WaitForCurrentCommand =>
			"Wait for the current command or turn to settle.",
		ConversationRecoveryAction::RefreshConversation =>
			"Refresh this conversation before continuing.",
	}
}

fn conversation_load_status(load: ConversationsLoadState) -> &'static str {
	match load {
		ConversationsLoadState::NeverRequested => "Conversations have not loaded.",
		ConversationsLoadState::Loading => "Loading Conversations",
		ConversationsLoadState::Ready =>
			"Local task list loaded. Open or refresh a task for latest Codex state.",
		ConversationsLoadState::Offline => "Offline. Retained conversation state remains visible.",
		ConversationsLoadState::Unavailable => "Conversation state is temporarily unavailable.",
		ConversationsLoadState::Refused => "Conversation readback was refused.",
	}
}

fn conversation_refresh_status(refresh: ConversationRefreshState) -> Option<String> {
	match refresh {
		ConversationRefreshState::Idle => None,
		ConversationRefreshState::Refreshing { completed, total, archived, failed } =>
			Some(format!("Syncing {completed}/{total} · {archived} archived · {failed} skipped")),
		ConversationRefreshState::Complete { checked, archived, failed } =>
			Some(format!("{checked} checked · {archived} archived · {failed} skipped")),
		ConversationRefreshState::Stopped { checked, total, archived, failed } =>
			Some(format!("Stopped {checked}/{total} · {archived} archived · {failed} skipped")),
	}
}

fn conversation_session_sidebar(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let rows = conversation_session_rows(shell, cx);
	div()
		.id("conversation-session-sidebar")
		.role(Role::TabList)
		.aria_label("Conversation conversations")
		.w(px(WORKBENCH_SESSION_SIDEBAR_WIDTH))
		.min_w(px(WORKBENCH_SESSION_SIDEBAR_WIDTH))
		.h_full()
		.flex()
		.flex_col()
		.border_r_1()
		.border_color(rgba(0xffffff0d))
		.bg(rgba(ui_theme::SIDEBAR_MATERIAL))
		.child(conversation_sessions_header(shell, cx))
		.child(
			div()
				.id("conversation-list")
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
		.text_size(px(11.0))
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

fn inspector_metadata_row(label: &'static str, value: String) -> AnyElement {
	div()
		.w_full()
		.min_h(px(26.0))
		.flex()
		.items_start()
		.justify_between()
		.gap_3()
		.text_size(px(11.0))
		.child(div().w(px(78.0)).min_w(px(78.0)).text_color(rgb(WB_TEXT_FAINT)).child(label))
		.child(
			div()
				.min_w_0()
				.flex_1()
				.font_family(ui_theme::FONT_FAMILY)
				.text_color(rgb(WB_TEXT_MUTED))
				.text_right()
				.overflow_hidden()
				.whitespace_nowrap()
				.text_ellipsis()
				.child(value),
		)
		.into_any_element()
}

fn conversation_context_inspector(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let Some(task) = shell.quick.selected_task() else {
		return div()
			.py_8()
			.text_center()
			.text_size(px(11.0))
			.text_color(rgb(WB_TEXT_FAINT))
			.child("Select a Conversation to inspect its durable context.")
			.into_any_element();
	};
	let runtime = task
		.runtime_session_id
		.as_ref()
		.map(|identity| compact_identity(identity.as_str()))
		.unwrap_or_else(|| "not established".to_owned());
	let mut content = div()
		.flex()
		.flex_col()
		.gap_4()
		.child(
			div()
				.id("inspector-conversation-heading")
				.role(Role::Heading)
				.aria_level(2)
				.text_size(px(14.0))
				.font_weight(FontWeight::SEMIBOLD)
				.text_color(rgb(WB_TEXT))
				.child(task.title.as_str().to_owned()),
		)
		.child(inspector_metadata_row(
			"Conversation",
			compact_identity(task.conversation_id.as_str()),
		))
		.child(inspector_metadata_row("Runtime", runtime))
		.child(inspector_metadata_row("Revision", format!("r{}", task.conversation_revision.0)));
	if let Some(program) = task.program.as_ref() {
		content = content
			.child(inspector_metadata_row("Program", compact_identity(program.program_id.as_str())))
			.child(inspector_metadata_row(
				"Work item",
				compact_identity(program.work_item_id.as_str()),
			))
			.child(inspector_metadata_row("State", program.state.as_str().to_owned()))
			.child(
				div()
					.text_size(px(11.0))
					.line_height(px(14.0))
					.text_color(rgb(WB_TEXT_MUTED))
					.whitespace_normal()
					.child(program.instructions.as_str().to_owned()),
			);
	}
	if let Some(url) =
		task.codex_thread_id.as_ref().and_then(|thread_id| thread_id.codex_url().ok())
	{
		let url = url.to_string();
		content = content.child(
			div()
				.id("open-provider-thread")
				.role(Role::Button)
				.aria_label("Open exact Codex provider thread")
				.h(px(30.0))
				.px_3()
				.flex()
				.items_center()
				.justify_center()
				.rounded(px(7.0))
				.border_1()
				.border_color(rgba(0xffffff16))
				.text_size(px(11.0))
				.text_color(rgb(WB_BLUE))
				.cursor_pointer()
				.on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
				.child("OPEN IN CODEX")
				.smooth(),
		);
	} else {
		content = content.child(
			div()
				.text_size(px(11.0))
				.text_color(rgb(WB_TEXT_FAINT))
				.child("Codex link becomes available after exact provider-thread readback."),
		);
	}
	content.into_any_element()
}

fn activity_inspector_content(shell: &Shell) -> AnyElement {
	let task = shell.quick.selected_task();
	let task_state =
		task.map_or("No active conversation", |task| conversation_state_label(task.state));
	let task_color = task.map_or(WB_TEXT_FAINT, |task| conversation_state_color(task.state));
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
							.text_size(px(11.0))
							.child(div().text_color(rgb(WB_TEXT_MUTED)).child(kind))
							.child(div().text_color(rgb(WB_TEXT_FAINT)).child(role)),
					)
					.child(
						div()
							.max_h(px(30.0))
							.overflow_hidden()
							.text_size(px(11.0))
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
				.text_size(px(11.0))
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
						.text_size(px(11.0))
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
		InspectorTab::Context => conversation_context_inspector(shell, cx),
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
							"inspector-context",
							"Context",
							InspectorTab::Context,
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
						.id("open-chief")
						.role(Role::Button)
						.aria_label("Open Chief")
						.h(px(26.0))
						.px_2()
						.flex()
						.items_center()
						.rounded(px(6.0))
						.border_1()
						.border_color(rgba(0xffffff10))
						.text_size(px(11.0))
						.text_color(rgb(WB_TEXT_MUTED))
						.cursor_pointer()
						.hover(|element| element.bg(rgba(0xffffff09)).text_color(rgb(WB_TEXT)))
						.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
						.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
						.on_click(cx.listener(|shell, _, _, cx| {
							shell.select_destination(Destination::Chief, cx);
						}))
						.child("Open Chief")
						.smooth(),
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

fn conversation_transcript(
	snapshot: &ConversationsSnapshot,
	history: Option<&HistorySnapshot>,
	pending: Option<&PendingComposerSubmission>,
) -> AnyElement {
	let rows = conversation_transcript_rows(snapshot, history, pending);
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
				.text_size(px(11.0))
				.text_color(rgb(WB_TEXT_FAINT))
				.child(
					div()
						.font_family(ui_theme::FONT_FAMILY)
						.text_size(px(11.0))
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
	let history_status = transcript_history_status(history, has_rows);

	div()
		.id("conversation-transcript")
		.role(Role::Log)
		.aria_label("Conversation conversation")
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
						.text_size(px(11.0))
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
		.id("conversation-history-previous")
		.role(Role::Button)
		.aria_label("Show less conversation history")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Show less history")).into())
		.h(px(24.0))
		.px_2()
		.flex()
		.items_center()
		.justify_center()
		.rounded_sm()
		.text_size(px(11.0))
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
		.id("conversation-history-retry")
		.role(Role::Button)
		.aria_label("Retry conversation history")
		.h(px(24.0))
		.px_2()
		.flex()
		.items_center()
		.justify_center()
		.rounded_sm()
		.text_size(px(11.0))
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
		.id("conversation-history-next")
		.role(Role::Button)
		.aria_label("Load more conversation history")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Load more history")).into())
		.h(px(24.0))
		.px_2()
		.flex()
		.items_center()
		.justify_center()
		.rounded_sm()
		.text_size(px(11.0))
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

fn conversation_composer(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let task = shell.quick.selected_task();
	let (has_executable_recovery, recovery_label) = conversation_recovery_presentation(task);
	let can_continue = shell.creating_new
		|| task.is_none()
		|| task.is_some_and(|task| task.state == ConversationState::Ready);
	let composer = shell.composer.read(cx);
	let composer_len = composer.len();
	let has_message = !composer.content().trim().is_empty();
	let can_send = shell.quick.can_submit && can_continue && has_message;
	let can_recover = shell.quick.can_submit && has_executable_recovery;
	let can_interrupt =
		shell.quick.can_submit && task.is_some_and(|task| task.state == ConversationState::Running);
	let model_label = shell.quick.execution.model.as_str().to_owned();
	let effort_label = shell
		.quick
		.execution
		.reasoning_effort
		.as_ref()
		.map_or("Inherited", |effort| effort.as_str())
		.to_uppercase();
	let fast_enabled = shell.quick.execution.fast;

	let send = composer_send(can_send, cx);
	let interrupt = composer_interrupt(can_interrupt, cx);
	let recover = composer_recover(can_recover, recovery_label, cx);
	let model_control = composer_model_control(model_label, cx);
	let fast_control = composer_fast_control(fast_enabled, cx);
	let effort_control = composer_effort_control(effort_label, cx);
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
				.child(conversation_service_tiers(shell, cx))
				.child(ordinary_drafts::creation_receipt_controls(shell, cx))
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
								.when(shell.quick.catalog.is_none(), |row| row.child(fast_control))
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
											.font_family(ui_theme::FONT_FAMILY)
											.text_size(px(11.0))
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

fn conversation_service_tiers(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let mut row =
		div().id("conversation-service-tiers").flex().flex_wrap().gap_2().text_size(px(11.));
	if let Some(notice) = shell.chief.read(cx).ordinary_draft_notice() {
		row = row.child(div().id("ordinary-draft-storage-notice").child(notice.to_owned())).child(
			div()
				.id("ordinary-draft-recovery")
				.cursor_pointer()
				.child("Review saved drafts")
				.on_click(cx.listener(|shell, _, _, cx| {
					shell.chief.update(cx, |chief, cx| chief.show_ordinary_draft_recovery(cx));
					shell.selected = Destination::Chief;
					cx.notify();
				})),
		);
	}

	if shell.conversations.can_cancel_unsent_ordinary() {
		row = row.child(
			div()
				.id("ordinary-cancel-unsent")
				.debug_selector(|| "ordinary-cancel-unsent".into())
				.cursor_pointer()
				.child("Cancel pending send")
				.on_click(cx.listener(|shell, _, _, cx| {
					shell.conversations.cancel_unsent_ordinary();
					shell.synchronize_conversations(cx);
					cx.notify();
				})),
		);
	}

	if shell.quick.selected.is_none() && !shell.quick.initial_defaults_ready {
		row = row.child(
			div()
				.id("conversation-defaults-pending")
				.debug_selector(|| "conversation-defaults-pending".into())
				.child("Waiting for account model defaults. Refresh model options to retry."),
		);
	}
	if shell.quick.execution.effective_service_tier().as_str() == "flex" {
		row = row.child("Flex · configured");
	}
	row = row.child(
		div()
			.id("conversation-refresh-models")
			.cursor_pointer()
			.child("Refresh model options")
			.on_click(cx.listener(|shell, _, _, cx| {
				shell.conversations.refresh_catalog();
				shell.synchronize_conversations(cx);
				cx.notify();
			})),
	);
	if let Some(models) = &shell.quick.catalog {
		let mut choices = vec![decodex_protocol::ChiefServiceTierDto {
			id: decodex_protocol::ServiceTier::standard(),
			name: "Standard".into(),
			description: String::new(),
		}];
		if let Some(model) = models.iter().find(|model| model.model == shell.quick.execution.model)
		{
			choices.extend(
				model.service_tiers.iter().filter(|tier| tier.id.as_str() != "default").cloned(),
			);
		}
		let current = shell.quick.execution.effective_service_tier();
		for choice in choices {
			let id = choice.id.clone();
			row = row.child(
				div()
					.id(SharedString::from(format!("conversation-tier-{}", id.as_str())))
					.debug_selector({
						let label = format!("conversation-tier-{}", id.as_str());
						move || label.clone()
					})
					.cursor_pointer()
					.px_2()
					.py_1()
					.rounded_md()
					.text_color(if current == id { rgb(WB_AMBER) } else { rgb(WB_TEXT_MUTED) })
					.child(if choice.description.is_empty() {
						choice.name
					} else {
						format!("{} · {}", choice.name, choice.description)
					})
					.on_click(cx.listener(move |shell, _, _, cx| {
						shell.conversations.select_service_tier(id.clone());
						shell.synchronize_conversations(cx);
						cx.notify();
					})),
			);
		}
	}
	row.into_any_element()
}

fn conversations_content(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let selected_task = shell.quick.selected_task();
	let feedback_conversation = shell
		.quick
		.selected
		.as_ref()
		.or_else(|| shell.pending_submission.as_ref().map(|pending| &pending.conversation_id));
	let state_label = if shell.creating_new {
		"New conversation"
	} else {
		selected_task
			.map_or("No conversation selected", |task| conversation_state_label(task.state))
	};
	let state_color = selected_task.map_or(WB_BLUE, |task| conversation_state_color(task.state));
	let detail = shell
		.input_status
		.as_ref()
		.map(SharedString::to_string)
		.or_else(|| conversation_refresh_status(shell.quick.refresh))
		.or_else(|| {
			(shell.quick.command_conversation_id.as_ref() == feedback_conversation)
				.then(|| command_status(shell.quick.command))
				.flatten()
				.map(str::to_owned)
		})
		.or_else(|| {
			selected_task
				.is_some_and(|task| task.state == ConversationState::OutcomeUnknown)
				.then(|| "Checking the interrupted turn before continuing.".to_owned())
		})
		.or_else(|| {
			selected_task
				.and_then(|task| task.recovery_action)
				.map(recovery_action_label)
				.map(str::to_owned)
		})
		.unwrap_or_else(|| conversation_load_status(shell.quick.load).to_owned());

	div()
		.flex_1()
		.min_w_0()
		.min_h_0()
		.flex()
		.when(shell.left_sidebar_mounted, |content| {
			content.child(animated_horizontal_panel_slot(
				"conversation-sidebar-motion",
				shell.left_sidebar_visible,
				shell.left_sidebar_motion_generation,
				WORKBENCH_SESSION_SIDEBAR_WIDTH,
				conversation_session_sidebar(shell, cx),
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
										.text_size(px(11.0))
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
										.text_size(px(11.0))
										.text_color(rgb(WB_TEXT_FAINT))
										.child(detail),
								)
								.child(history_page_controls(shell, cx)),
						)
						.child(conversation_transcript(
							&shell.quick,
							shell.history.as_ref(),
							shell.pending_submission.as_ref(),
						))
						.child(conversation_composer(shell, cx)),
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

	let content = div()
		.w_full()
		.max_w(px(ui_theme::SETTINGS_WIDTH))
		.child(
			div()
				.id("health-query-status")
				.role(Role::Status)
				.aria_label(format!("Health report: {}", presentation.label))
				.h(px(44.0))
				.min_h(px(44.0))
				.px_4()
				.flex()
				.items_center()
				.gap_3()
				.rounded(px(10.0))
				.border_1()
				.border_color(rgba(0xffffff10))
				.bg(rgba(0xffffff04))
				.child(
					div().size(px(6.0)).min_w(px(6.0)).rounded_full().bg(rgb(presentation.color)),
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
						.text_size(px(11.0))
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
				.gap_4()
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
		);
	div()
		.id("health-scroll-viewport")
		.flex_1()
		.min_h_0()
		.overflow_y_scroll()
		.px(px(ui_theme::SETTINGS_INSET))
		.pt(px(ui_theme::SETTINGS_GROUP_GAP))
		.pb(px(ui_theme::SETTINGS_INSET))
		.flex()
		.justify_center()
		.child(content)
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
		.font_family(ui_theme::FONT_FAMILY)
		.text_size(px(11.0))
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
		Destination::Chief => {
			return div()
				.id("destination-content")
				.flex_1()
				.min_w_0()
				.min_h_0()
				.flex()
				.child(shell.chief.clone())
				.into_any_element();
		},
		Destination::Conversations => {
			return div()
				.id("destination-content")
				.role(Role::Main)
				.aria_label("Codex Workbench")
				.flex_1()
				.min_w_0()
				.min_h_0()
				.flex()
				.flex_col()
				.child(conversations_content(shell, cx))
				.into_any_element();
		},
		Destination::Settings | Destination::Accounts | Destination::Health => {
			return settings_workspace_content(shell, false, refresh_focus, window, cx);
		},
		_ => {},
	}

	let content = placeholder_content(selected);
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

fn settings_navigation(
	shell: &Shell,
	standalone: bool,
	selected: Destination,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let mut navigation = div()
		.w(px(192.0))
		.min_w(px(192.0))
		.h_full()
		.p_3()
		.pt(px(WINDOW_CONTROLS_CLEARANCE))
		.flex()
		.flex_col()
		.gap(px(3.0))
		.text_size(px(ui_theme::BODY_SIZE))
		.pr(px(16.))
		.bg(rgba(ui_theme::CHIEF_SIDEBAR_MATERIAL))
		.child(
			div()
				.px_2()
				.pt(px(ui_theme::SETTINGS_TOP))
				.pb(px(ui_theme::SETTINGS_GROUP_GAP))
				.text_size(px(13.0))
				.font_weight(FontWeight::SEMIBOLD)
				.child("Settings"),
		);
	use crate::settings_surface::SettingsCategory;
	for (destination, category, label) in [
		(Destination::Settings, Some(SettingsCategory::General), "General"),
		(Destination::Settings, Some(SettingsCategory::Appearance), "Appearance"),
		(Destination::Accounts, None, "Accounts"),
		(Destination::Health, None, "Diagnostics"),
	] {
		let index =
			Destination::ALL.iter().position(|d| *d == destination).expect("settings destination");
		let active = selected == destination
			&& category.is_none_or(|category| shell.settings.read(cx).category == category);
		navigation = navigation.child(
			div()
				.id(gpui::SharedString::from(format!("settings-section-{label}")))
				.role(Role::Tab)
				.aria_label(label)
				.aria_selected(active)
				.tab_index(0)
				.when(category.is_none() || category == Some(SettingsCategory::General), |row| {
					row.track_focus(&shell.destination_focus[index])
				})
				.key_context("Destination")
				.on_action(cx.listener(move |s, _: &ActivateDestination, _, cx| {
					s.select_settings_destination(destination, standalone, cx);
					if let Some(category) = category {
						s.settings.update(cx, |settings, cx| {
							settings.category = category;
							cx.notify();
						});
					}
				}))
				.on_action(cx.listener(Shell::focus_next))
				.on_action(cx.listener(Shell::focus_previous))
				.h(px(28.0))
				.px_2()
				.rounded(px(6.0))
				.flex()
				.items_center()
				.cursor_pointer()
				.when(active, |row| row.bg(rgba(0xffffff0c)).text_color(rgb(ui_theme::TEXT)))
				.hover(|row| row.bg(rgba(ui_theme::SURFACE_MATERIAL)))
				.on_click(cx.listener(move |s, _, _, cx| {
					s.select_settings_destination(destination, standalone, cx);
					if let Some(category) = category {
						s.settings.update(cx, |settings, cx| {
							settings.category = category;
							cx.notify();
						});
					}
				}))
				.child(label)
				.smooth(),
		);
	}
	navigation.into_any_element()
}

fn settings_workspace_content(
	shell: &Shell,
	standalone: bool,
	refresh_focus: FocusHandle,
	window: &Window,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let selected = if standalone { shell.settings_selected } else { shell.selected };
	let navigation = settings_navigation(shell, standalone, selected, cx);
	let panel = div()
		.flex_1()
		.min_w_0()
		.min_h_0()
		.h_full()
		.overflow_hidden()
		.bg(rgba(ui_theme::CHIEF_SIDEBAR_MATERIAL))
		.pt(px(WINDOW_CONTROLS_CLEARANCE))
		.flex()
		.flex_col();
	let content = if selected == Destination::Settings {
		panel
			.child(div().flex_1().min_h_0().overflow_hidden().child(shell.settings.clone()))
			.into_any_element()
	} else {
		panel
			.child(
				ui_theme::settings_header_inset().child(
					div()
						.w_full()
						.max_w(px(ui_theme::SETTINGS_WIDTH))
						.flex()
						.items_center()
						.justify_between()
						.child(ui_theme::settings_title(if selected == Destination::Accounts {
							"Accounts"
						} else {
							"Diagnostics"
						}))
						.when(selected == Destination::Health, |d| {
							d.child(refresh_control(
								refresh_focus,
								shell.health.can_refresh,
								window,
								cx,
							))
						}),
				),
			)
			.child(if selected == Destination::Accounts {
				accounts_content(shell, cx)
			} else {
				health_content(&shell.health)
			})
			.into_any_element()
	};
	div()
		.id("settings-workspace")
		.role(Role::Main)
		.aria_label("Settings")
		.flex_1()
		.min_w_0()
		.min_h_0()
		.flex()
		.when(standalone || shell.left_sidebar_visible, |layout| layout.child(navigation))
		.child(content)
		.into_any_element()
}

/// Settings render the existing controller-backed surfaces in their own window.
pub(crate) struct SettingsWindow {
	owner: Entity<Shell>,
	focus: FocusHandle,
	_observation: Subscription,
}
impl Render for SettingsWindow {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let content = self.owner.update(cx, |s, cx| {
			settings_workspace_content(s, true, s.refresh_focus.clone(), window, cx)
		});

		div()
			.id("settings-window")
			.size_full()
			.flex()
			.font_family(ui_theme::FONT_FAMILY)
			.text_color(rgb(WB_TEXT))
			.bg(rgba(ui_theme::SHELL_MATERIAL))
			.key_context("SettingsWindow")
			.track_focus(&self.focus)
			.on_action(cx.listener(|_, _: &CloseSettings, window, cx| {
				window.remove_window();
				cx.stop_propagation();
			}))
			.on_action(cx.listener(|_, _: &ActivateSettings, _, cx| cx.stop_propagation()))
			.on_action(cx.listener(|_, _: &FocusNext, window, cx| window.focus_next(cx)))
			.on_action(cx.listener(|_, _: &FocusPrevious, window, cx| window.focus_prev(cx)))
			.relative()
			.child(content)
	}
}
impl Shell {
	fn select_settings_destination(
		&mut self,
		destination: Destination,
		standalone: bool,
		cx: &mut Context<Self>,
	) {
		if standalone {
			self.select_settings_section(destination, cx);
		} else {
			self.select_destination(destination, cx);
		}
	}

	fn select_settings_section(&mut self, destination: Destination, cx: &mut Context<Self>) {
		self.settings_selected = destination;
		match destination {
			Destination::Accounts => {
				self.accounts_controller.activate();
				self.synchronize_accounts();
			},
			Destination::Health => self.health_query.activate(),
			_ => self.settings.update(cx, SettingsSurface::refresh),
		}
		cx.notify();
	}

	fn open_settings_window(&mut self, section: Destination, cx: &mut Context<Self>) {
		if section != Destination::Settings || self.settings_window.is_none() {
			self.select_settings_section(section, cx);
		}
		let owner = cx.entity();
		cx.defer(move |cx| open_settings_window(owner, cx));
	}
}

fn open_settings_window(owner: Entity<Shell>, cx: &mut App) {
	if let Some(handle) = owner.read(cx).settings_window
		&& handle.update(cx, |_, window, _| window.activate_window()).is_ok()
	{
		return;
	}
	let parent = cx
		.windows()
		.into_iter()
		.filter_map(|w| w.downcast::<Shell>())
		.find(|w| w.entity(cx).is_ok_and(|entity| entity == owner));
	let bounds = gpui::Bounds::centered(None, gpui::size(px(920.), px(620.)), cx);
	match cx.open_window(
		gpui::WindowOptions {
			titlebar: Some(gpui::TitlebarOptions {
				title: Some("Decodex Settings".into()),
				appears_transparent: true,
				..Default::default()
			}),
			window_background: gpui::WindowBackgroundAppearance::Blurred,
			window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
			window_min_size: Some(gpui::size(px(860.), px(480.))),
			..Default::default()
		},
		{
			let owner = owner.clone();
			move |window, cx| {
				cx.new(|cx| {
					{
						window.on_next_frame(|window, _| {
							ui_theme::window_material::configure(window)
						});
						cx.observe_window_appearance(window, |_, window, _| {
							ui_theme::window_material::configure(window)
						})
						.detach();
					}
					let closing_window = window.window_handle();
					cx.on_release(move |settings: &mut SettingsWindow, cx| {
						let owner = settings.owner.downgrade();
						// Run after AppKit has removed Settings so it cannot win key focus back.
						cx.defer(move |cx| {
							let Some(owner) = owner.upgrade() else {
								return;
							};
							owner.update(cx, |s, cx| {
								if s.settings_window
									.is_some_and(|w| w.window_id() == closing_window.window_id())
								{
									s.settings_window = None;
								}
								cx.notify();
							});
							if let Some(parent) = parent {
								let _ = parent.update(cx, |_, window, _| window.activate_window());
							}
						});
					})
					.detach();
					let focus = cx.focus_handle();
					window.focus(&focus, cx);
					let observation = cx.observe(&owner, |_, _, cx| cx.notify());
					SettingsWindow { owner, focus, _observation: observation }
				})
			}
		},
	) {
		Ok(handle) => owner.update(cx, |s, _| s.settings_window = Some(handle)),
		Err(error) => {
			owner.update(cx, |s, cx| {
				s.account_status = Some(format!("Could not open Settings: {error}").into());
				cx.notify();
			});
		},
	}
}

impl Render for Shell {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let presentation = connection_presentation(self.connection);
		let root = div()
			.id("decodex-shell")
			.role(Role::Application)
			.aria_label("Decodex operational shell")
			.font_family(ui_theme::FONT_FAMILY)
			.text_size(px(13.0))
			.key_context("Conversations")
			.track_focus(&self.root_focus)
			.on_action(cx.listener(Self::focus_next))
			.on_action(cx.listener(Self::focus_previous))
			.on_action(cx.listener(Self::activate_conversations))
			.on_action(cx.listener(Self::activate_chief))
			.on_action(cx.listener(Self::activate_health))
			.on_action(cx.listener(Self::activate_settings))
			.on_action(cx.listener(Self::toggle_sidebar))
			.on_action(cx.listener(|s, _: &ShrinkPanel, window, cx| {
				if s.selected == Destination::Chief {
					s.chief.update(cx, |a, cx| a.resize_panel(-24.0, false, false, window, cx));
					cx.stop_propagation();
				}
			}))
			.on_action(cx.listener(|s, _: &GrowPanel, window, cx| {
				if s.selected == Destination::Chief {
					s.chief.update(cx, |a, cx| a.resize_panel(24.0, false, false, window, cx));
					cx.stop_propagation();
				}
			}))
			.on_action(cx.listener(|s, _: &ResetPanel, window, cx| {
				if s.selected == Destination::Chief {
					s.chief.update(cx, |a, cx| a.resize_panel(0.0, true, false, window, cx));
					cx.stop_propagation();
				}
			}))
			.on_action(cx.listener(|s, _: &ShrinkPanels, window, cx| {
				if s.selected == Destination::Chief {
					s.chief.update(cx, |a, cx| a.resize_panel(-24.0, false, true, window, cx));
					cx.stop_propagation();
				}
			}))
			.on_action(cx.listener(|s, _: &GrowPanels, window, cx| {
				if s.selected == Destination::Chief {
					s.chief.update(cx, |a, cx| a.resize_panel(24.0, false, true, window, cx));
					cx.stop_propagation();
				}
			}))
			.on_action(cx.listener(|s, _: &ResetPanels, window, cx| {
				if s.selected == Destination::Chief {
					s.chief.update(cx, |a, cx| a.resize_panel(0.0, true, true, window, cx));
					cx.stop_propagation();
				}
			}))
			.on_action(cx.listener(Self::toggle_inspector))
			.on_action(cx.listener(Self::toggle_graph))
			.on_action(cx.listener(|s, _: &DismissStatus, _, cx| {
				if s.status_open {
					s.status_open = false;
					cx.notify();
				}
			}))
			.on_key_down(cx.listener(Self::interrupt_reply))
			.on_action(cx.listener(|s, _: &NavigateBack, _, cx| s.navigate_history(false, cx)))
			.on_action(cx.listener(|s, _: &NavigateForward, _, cx| s.navigate_history(true, cx)))
			.on_action(cx.listener(Self::select_previous_conversation))
			.on_action(cx.listener(Self::select_next_conversation))
			.on_action(cx.listener(Self::submit_composer))
			.size_full()
			.min_w(px(1180.0))
			.min_h(px(720.0))
			.flex()
			.flex_col()
			.bg(rgba(ui_theme::SHELL_MATERIAL))
			.text_color(rgb(WB_TEXT));

		#[cfg(all(target_os = "macos", not(test)))]
		self.chief.update(cx, |chief, cx| {
			chief.prepare_native_composer(self.selected == Destination::Chief, window, cx)
		});
		let controls = floating_window_controls(self, &presentation, window, cx);
		#[cfg(all(target_os = "macos", not(test)))]
		self.prepare_native_status(window, cx);
		let status = self.render_status_center(&presentation, cx);
		let route = format!("{:?}", self.selected);
		let content =
			destination_content(self, presentation, self.refresh_focus.clone(), window, cx);
		root.relative()
			.child(crate::ui_motion::arrival(route, content))
			.child(controls)
			.child(gpui::deferred(status).priority(3))
		// Keep global notifications above deferred composer menus throughout dismissal.
	}
}

fn composer_send(can_send: bool, cx: &mut Context<Shell>) -> AnyElement {
	div()
		.id("conversation-send")
		.debug_selector(|| "conversation-send".into())
		.role(Role::Button)
		.aria_label("Send message")
		.when(!can_send, |element| {
			element.aria_label("Send unavailable; check conversation status")
		})
		.h(px(23.0))
		.min_h(px(23.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(7.0))
		.bg(if can_send { rgb(WB_TEXT) } else { rgba(0xffffff08) })
		.text_size(px(11.0))
		.font_weight(FontWeight::SEMIBOLD)
		.text_color(if can_send { rgb(WB_CANVAS) } else { rgb(WB_TEXT_FAINT) })
		.when(can_send, |element| {
			element
				.cursor_pointer()
				.hover(|element| element.opacity(0.9))
				.active(|element| element.opacity(0.72))
				.focus_visible(|element| element.border_1().border_color(rgb(WB_BLUE)))
				.on_click(cx.listener(|shell, _, window, cx| {
					shell.submit_conversation(window, cx);
				}))
		})
		.child("Send")
		.into_any_element()
}

fn composer_interrupt(can_interrupt: bool, cx: &mut Context<Shell>) -> AnyElement {
	div()
		.id("conversation-interrupt")
		.role(Role::Button)
		.aria_label("Interrupt active turn")
		.h(px(23.0))
		.min_h(px(23.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(7.0))
		.border_1()
		.border_color(rgba(0xffffff12))
		.text_size(px(11.0))
		.text_color(if can_interrupt { rgb(WB_TEXT_MUTED) } else { rgb(WB_TEXT_FAINT) })
		.when(can_interrupt, |element| {
			element
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0a)))
				.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
				.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
				.on_click(cx.listener(|shell, _, window, cx| {
					shell.interrupt_conversation(window, cx);
				}))
		})
		.child("Stop")
		.into_any_element()
}

fn composer_recover(
	can_recover: bool,
	recovery_label: &'static str,
	cx: &mut Context<Shell>,
) -> AnyElement {
	div()
		.id("conversation-recover")
		.role(Role::Button)
		.aria_label(recovery_label)
		.h(px(23.0))
		.min_h(px(23.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(7.0))
		.border_1()
		.border_color(if can_recover { rgba(0xf59e0b55) } else { rgba(0xffffff10) })
		.text_size(px(11.0))
		.text_color(if can_recover { rgb(WB_AMBER) } else { rgb(WB_TEXT_FAINT) })
		.when(can_recover, |element| {
			element
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xf59e0b12)))
				.active(|element| element.opacity(0.72))
				.on_click(cx.listener(|shell, _, window, cx| {
					shell.recover_conversation(window, cx);
				}))
		})
		.child(recovery_label)
		.into_any_element()
}

fn composer_model_control(model_label: String, cx: &mut Context<Shell>) -> AnyElement {
	div()
		.id("conversation-model")
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
		.font_family(ui_theme::FONT_FAMILY)
		.text_size(px(11.0))
		.text_color(rgb(WB_TEXT_MUTED))
		.cursor_pointer()
		.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
		.active(|element| element.opacity(0.72))
		.on_click(cx.listener(|shell, _, _, cx| shell.cycle_conversation_model(cx)))
		.child(model_label)
		.into_any_element()
}

fn composer_fast_control(fast_enabled: bool, cx: &mut Context<Shell>) -> AnyElement {
	div()
		.id("conversation-fast")
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
		.font_family(ui_theme::FONT_FAMILY)
		.text_size(px(11.0))
		.text_color(if fast_enabled { rgb(WB_AMBER) } else { rgb(WB_TEXT_MUTED) })
		.cursor_pointer()
		.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
		.active(|element| element.opacity(0.72))
		.on_click(cx.listener(|shell, _, _, cx| shell.toggle_conversation_fast(cx)))
		.child(div().size(px(4.0)).rounded_full().bg(if fast_enabled {
			rgb(WB_AMBER)
		} else {
			rgb(WB_TEXT_FAINT)
		}))
		.child("Fast")
		.into_any_element()
}

fn composer_effort_control(effort_label: String, cx: &mut Context<Shell>) -> AnyElement {
	div()
		.id("conversation-effort")
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
		.font_family(ui_theme::FONT_FAMILY)
		.text_size(px(11.0))
		.text_color(rgb(WB_TEXT_MUTED))
		.cursor_pointer()
		.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
		.active(|element| element.opacity(0.72))
		.on_click(cx.listener(|shell, _, _, cx| shell.cycle_conversation_effort(cx)))
		.child(effort_label)
		.into_any_element()
}

fn topbar_sessions_toggle(left_sidebar_visible: bool, cx: &mut Context<Shell>) -> AnyElement {
	div()
		.id("toggle-left-sidebar")
		.role(Role::Button)
		.aria_label("Toggle conversation sidebar")
		.aria_expanded(left_sidebar_visible)
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Toggle sessions · Command-E")).into())
		.h(px(27.0))
		.px_3()
		.flex()
		.items_center()
		.rounded(px(7.0))
		.border_1()
		.border_color(if left_sidebar_visible { rgba(0xffffff20) } else { rgba(0xffffff10) })
		.bg(if left_sidebar_visible { rgba(0xffffff10) } else { rgba(0x00000000) })
		.text_color(if left_sidebar_visible { rgb(WB_TEXT) } else { rgb(WB_TEXT_MUTED) })
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
		.child("Sessions")
		.into_any_element()
}

fn topbar_inspector_toggle(inspector_visible: bool, cx: &mut Context<Shell>) -> AnyElement {
	div()
		.id("toggle-inspector")
		.role(Role::Button)
		.aria_label("Toggle conversation context")
		.aria_expanded(inspector_visible)
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Toggle context · Command-B")).into())
		.h(px(27.0))
		.px_3()
		.flex()
		.items_center()
		.rounded(px(7.0))
		.border_1()
		.border_color(if inspector_visible { rgba(0xffffff20) } else { rgba(0xffffff10) })
		.bg(if inspector_visible { rgba(0xffffff10) } else { rgba(0x00000000) })
		.text_color(if inspector_visible { rgb(WB_TEXT) } else { rgb(WB_TEXT_MUTED) })
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
		.child("Context")
		.into_any_element()
}

fn account_pool_header(
	count: usize,
	available: usize,
	balanced: bool,
	can_manage: bool,
	emails_visible: bool,
	cx: &mut Context<Shell>,
) -> AnyElement {
	div()
		.px(px(14.0))
		.py(px(6.0))
		.flex()
		.items_center()
		.justify_between()
		.gap_3()
		.rounded(px(10.0))
		.child(
			div().min_w_0().flex().flex_col().gap_1().child(
				div()
					.flex()
					.items_center()
					.gap_2()
					.child(div().size(px(6.0)).rounded_full().bg(rgb(WB_GREEN)))
					.child(
						div()
							.text_size(px(12.5))
							.font_weight(FontWeight::SEMIBOLD)
							.child("Routing"),
					)
					.child(
						div()
							.font_family(ui_theme::FONT_FAMILY)
							.text_size(px(11.0))
							.text_color(rgb(WB_TEXT_FAINT))
							.child(format!("{available} of {count} available")),
					),
			),
		)
		.child(
			div()
				.flex()
				.items_center()
				.gap_2()
				.child(
					account_icon_action(
						"account-email-visibility",
						0,
						if emails_visible {
							"Hide email addresses"
						} else {
							"Show email addresses"
						},
						if emails_visible {
							workspace_symbols::Symbol::Eye
						} else {
							workspace_symbols::Symbol::EyeSlash
						},
						true,
					)
					.on_click(cx.listener(|shell, _, _, cx| shell.toggle_account_emails(cx))),
				)
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
						.text_size(px(11.0))
						.text_color(rgb(WB_TEXT_MUTED))
						.cursor_pointer()
						.hover(|element| element.bg(rgba(0xffffff0d)).text_color(rgb(WB_TEXT)))
						.active(|element| element.bg(rgba(0xffffff1b)).opacity(0.84))
						.on_click(cx.listener(|shell, _, _, cx| {
							shell.refresh_accounts(cx);
						}))
						.child("Refresh")
						.smooth(),
				),
		)
		.into_any_element()
}

fn account_row_identity(account: &AccountDto, email: Option<&str>) -> AnyElement {
	let enabled = account.enabled;
	div()
		.id(SharedString::from(format!("account-identity-{}", account.account_id.as_str())))
		.when_some(email.map(str::to_owned), |row, email| {
			row.tooltip(move |_, cx| cx.new(|_| ControlTooltip(email.clone())).into())
		})
		.w(px(132.0))
		.min_w(px(108.0))
		.flex()
		.items_center()
		.gap_2()
		.child(
			div()
				.min_w_0()
				.flex()
				.flex_col()
				.gap(px(2.))
				.child(
					div()
						.overflow_hidden()
						.whitespace_nowrap()
						.text_ellipsis()
						.text_size(px(10.5))
						.font_weight(FontWeight::SEMIBOLD)
						.text_color(if enabled { rgb(WB_TEXT) } else { rgb(WB_TEXT_FAINT) })
						.child(email.unwrap_or(account.alias.as_str()).to_owned()),
				)
				.child(
					div()
						.flex()
						.items_center()
						.gap_2()
						.font_family(ui_theme::FONT_FAMILY)
						.text_size(px(10.5))
						.text_color(rgb(WB_TEXT_FAINT))
						.when(
							account.lifecycle_readiness != AccountLifecycleReadinessDto::Ready,
							|row| row.child(account_readiness_status(account)),
						),
				),
		)
		.into_any_element()
}

fn conversation_session_rows(shell: &Shell, cx: &mut Context<Shell>) -> Vec<AnyElement> {
	let selected = shell.quick.selected.clone();
	shell
		.quick
		.tasks
		.iter()
		.enumerate()
		.map(|(index, task)| {
			let conversation_id = task.conversation_id.clone();
			let short_id = task.conversation_id.as_str().chars().take(8).collect::<String>();
			let is_selected = selected.as_ref() == Some(&task.conversation_id);
			let state = task.state;
			let label = task.title.as_str().to_owned();
			div()
				.id(("conversation-row", index))
				.role(Role::Tab)
				.tab_index(index as isize)
				.key_context("ConversationRow")
				.aria_label(format!("{}, {}", label, conversation_state_label(state)))
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
				.text_size(px(11.0))
				.text_color(if is_selected { rgb(WB_TEXT) } else { rgb(WB_TEXT_MUTED) })
				.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
				.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
				.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
				.cursor_pointer()
				.on_click(cx.listener(move |shell, _, window, cx| {
					shell.choose_conversation(conversation_id.clone(), window, cx);
				}))
				.on_action(cx.listener({
					let conversation_id = task.conversation_id.clone();
					move |shell, _: &ActivateConversationRow, window, cx| {
						shell.choose_conversation(conversation_id.clone(), window, cx);
					}
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
								.bg(rgb(conversation_state_color(state))),
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
						.font_family(ui_theme::FONT_FAMILY)
						.text_size(px(11.0))
						.text_color(rgb(WB_TEXT_FAINT))
						.child(format!("{} · {short_id}", conversation_state_label(state))),
				)
		})
		.map(IntoElement::into_any_element)
		.collect()
}

fn conversation_sessions_header(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let can_control = shell.quick.can_submit
		&& shell.quick.selected_task().is_some_and(|task| task.state == ConversationState::Ready);
	let can_refresh_all =
		shell.quick.can_submit && shell.quick.load == ConversationsLoadState::Ready;
	let refresh_status = conversation_refresh_status(shell.quick.refresh);
	let refresh_label = match shell.quick.refresh {
		ConversationRefreshState::Refreshing { completed, total, .. } => {
			format!("{completed}/{total}")
		},
		_ => "↻".to_owned(),
	};
	let refresh_text_size =
		if matches!(shell.quick.refresh, ConversationRefreshState::Refreshing { .. }) {
			8.0
		} else {
			12.0
		};

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
				.text_size(px(11.0))
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
							.text_size(px(11.0))
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
				.child(conversation_refresh_button(
					can_refresh_all,
					refresh_text_size,
					refresh_label,
					cx,
				))
				.child(conversation_archive_button(can_control, cx))
				.child(
					div()
						.id("new-conversation")
						.role(Role::Button)
						.aria_label("New conversation")
						.h(px(27.0))
						.px_2()
						.flex()
						.items_center()
						.rounded(px(7.0))
						.border_1()
						.border_color(rgba(0xffffff14))
						.text_size(px(11.0))
						.text_color(rgb(WB_TEXT_MUTED))
						.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
						.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
						.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
						.cursor_pointer()
						.on_click(cx.listener(|shell, _, window, cx| {
							shell.start_new_conversation(window, cx);
						}))
						.child("+ New")
						.smooth(),
				),
		)
		.into_any_element()
}

fn conversation_refresh_button(
	can_refresh_all: bool,
	refresh_text_size: f32,
	refresh_label: String,
	cx: &mut Context<Shell>,
) -> AnyElement {
	div()
		.id("refresh-conversation")
		.role(Role::Button)
		.aria_label("Sync Codex-backed conversations")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Sync Codex-backed conversations")).into())
		.h(px(27.0))
		.min_w(px(27.0))
		.px_2()
		.flex()
		.items_center()
		.justify_center()
		.rounded(px(7.0))
		.text_size(px(refresh_text_size))
		.text_color(if can_refresh_all { rgb(WB_TEXT_MUTED) } else { rgb(WB_TEXT_FAINT) })
		.when(can_refresh_all, |element| {
			element
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
				.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
				.on_click(cx.listener(|shell, _, window, cx| {
					shell.refresh_conversation(window, cx);
				}))
		})
		.child(refresh_label)
		.into_any_element()
}

fn conversation_archive_button(can_control: bool, cx: &mut Context<Shell>) -> AnyElement {
	div()
		.id("archive-conversation")
		.role(Role::Button)
		.aria_label("Archive selected Codex conversation")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Archive selected thread")).into())
		.h(px(27.0))
		.px_2()
		.flex()
		.items_center()
		.rounded(px(7.0))
		.text_size(px(11.0))
		.text_color(if can_control { rgb(WB_TEXT_MUTED) } else { rgb(WB_TEXT_FAINT) })
		.when(can_control, |element| {
			element
				.cursor_pointer()
				.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
				.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
				.on_click(cx.listener(|shell, _, window, cx| {
					shell.archive_conversation(window, cx);
				}))
		})
		.child("Archive")
		.into_any_element()
}

fn transcript_history_status(
	history: Option<&HistorySnapshot>,
	has_rows: bool,
) -> Option<&'static str> {
	history.map_or_else(
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
	)
}

#[cfg(test)]
mod tests {
	use gpui::{TestAppContext, VisualTestContext, size};

	use super::*;
	use crate::client_lifecycle::{CompatibilityReason, QuarantineReason, QuarantineRecovery};

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
					Destination::Chief
						| Destination::Conversations
						| Destination::Accounts
						| Destination::Health
						| Destination::Settings
				),
			)),
			[
				("Main", true),
				("Advisor", false),
				("Projects", false),
				("History", true),
				("Runs", false),
				("Automations", false),
				("Accounts", true),
				("Diagnostics", true),
				("Settings", true),
			]
		);
	}

	#[test]
	fn conversation_arrow_selection_is_bounded_and_stable() {
		assert_eq!(adjacent_conversation_index(None, 0, 1), None);
		assert_eq!(adjacent_conversation_index(None, 3, 1), Some(1));
		assert_eq!(adjacent_conversation_index(Some(0), 3, -1), Some(0));
		assert_eq!(adjacent_conversation_index(Some(1), 3, 1), Some(2));
		assert_eq!(adjacent_conversation_index(Some(2), 3, 1), Some(2));
	}

	#[test]
	fn account_login_presentations_create_only_daemon_install_requests() {
		let enrollment = account_login_start(AccountLoginMethod::BrowserRedirect, None)
			.expect("browser enrollment request");
		assert!(enrollment.validate().is_ok());
		assert!(matches!(
			enrollment.install_mode,
			AccountLoginInstallMode::Enroll { enabled: true, .. }
		));

		let account_id =
			EntityId::new("10000000-0000-4000-8000-000000000001").expect("account identity");
		let recovery_operation_id =
			EntityId::new("10000000-0000-4000-8000-000000000002").expect("recovery identity");
		let reauthentication = account_login_start(
			AccountLoginMethod::DeviceCode,
			Some((
				account_id.clone(),
				decodex_protocol::EntityRevision(4),
				Some(recovery_operation_id.clone()),
			)),
		)
		.expect("device reauthentication request");
		assert!(matches!(
			reauthentication.install_mode,
			AccountLoginInstallMode::Reauthenticate {
				account_id: selected,
				expected_revision: decodex_protocol::EntityRevision(4),
				recovery_operation_id: Some(actual_recovery),
				..
			} if selected == account_id && actual_recovery == recovery_operation_id
		));
	}

	#[test]
	fn rejected_refresh_exposes_only_its_exact_relogin_takeover() {
		let quota = |duration_minutes| AccountQuotaWindowDto {
			duration_minutes,
			observed_at_unix_micros: None,
			result: AccountQuotaStateDto::Unknown,
		};
		let mut rejected = AccountDto {
			account_id: EntityId::new("10000000-0000-4000-8000-000000000001")
				.expect("account identity"),
			alias: decodex_protocol::WireText::new("Morgan").expect("account alias"),
			enabled: true,
			account_revision: decodex_protocol::EntityRevision(4),
			observed_state: AccountObservedStateDto::AuthFailed,
			lifecycle_readiness: AccountLifecycleReadinessDto::OperationUnsettled,
			credential_binding: None,
			unsettled_operation: None,
			five_hour_quota: quota(300),
			seven_day_quota: quota(10_080),
		};
		let recovery_operation_id =
			EntityId::new("10000000-0000-4000-8000-000000000002").expect("recovery identity");
		rejected.unsettled_operation = Some(decodex_protocol::AccountUnsettledOperationDto {
			operation_id: recovery_operation_id.clone(),
			kind: decodex_protocol::AccountOperationKindDto::Refresh,
			phase: decodex_protocol::AccountOperationPhaseDto::RecoveryRequired,
			recovery_code: Some(
				decodex_protocol::WireText::new("provider_refresh_rejected")
					.expect("recovery code"),
			),
		});

		assert_eq!(account_login_recovery_operation_id(&rejected), Some(recovery_operation_id));
		assert_eq!(account_readiness_status(&rejected), "Refresh rejected · re-login");
		rejected.unsettled_operation.as_mut().expect("recovery operation").recovery_code = Some(
			decodex_protocol::WireText::new("provider_access_rejected_after_refresh")
				.expect("access rejection code"),
		);
		assert!(account_login_recovery_operation_id(&rejected).is_some());
		assert_eq!(account_readiness_status(&rejected), "New login required · re-login");
		rejected.unsettled_operation.as_mut().expect("recovery operation").recovery_code = Some(
			decodex_protocol::WireText::new("credential_rotate_failed")
				.expect("other recovery code"),
		);
		assert_eq!(account_login_recovery_operation_id(&rejected), None);
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
		live_deltas: Vec<crate::conversations::ConversationLiveDelta>,
	) -> ConversationsSnapshot {
		ConversationsSnapshot {
			catalog: None,
			load: ConversationsLoadState::Ready,
			command: ConversationCommandState::AwaitingResult,
			command_conversation_id: Some(conversation_id.clone()),
			submission_result_generation: 0,
			last_submission_accepted: false,
			refresh: ConversationRefreshState::Idle,
			tasks: Vec::new(),
			selected: Some(conversation_id.clone()),
			live_deltas,
			can_submit: false,
			initial_defaults_ready: false,
			execution: decodex_protocol::ConversationExecutionSettings::new(
				decodex_protocol::ConversationModel::new("gpt-5.6-sol")
					.expect("test model is valid"),
				decodex_protocol::ConversationReasoningEffort::High,
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
			conversation_transcript_rows(&snapshot, None, Some(&pending)),
			vec![TranscriptRow::Prompt {
				turn_id: Some(turn_id),
				text: "Show this immediately.".to_owned(),
				pending: true,
			}]
		);
	}

	#[test]
	fn persisted_resume_failure_is_an_activity_not_a_user_prompt() {
		let conversation_id =
			EntityId::new("10000000-0000-4000-8000-000000000082").expect("conversation");
		let snapshot = transcript_snapshot(&conversation_id, Vec::new());
		let text = "Codex could not prepare its filesystem sandbox. This input was not sent.";
		let mut item = history_item(
			"40000000-0000-4000-8000-000000000082",
			"20000000-0000-4000-8000-000000000082",
			"user",
			text,
		);
		item.kind = HistoryItemKindDto::Status;
		item.status = HistoryItemStatusDto::Failed;
		let history = history_snapshot(
			&conversation_id,
			vec![item],
			HistoryLoadState::Visible,
			Some(HistoryPageSource::FreshServer),
		);
		assert!(matches!(conversation_transcript_rows(&snapshot, Some(&history), None).as_slice(),
			[TranscriptRow::Activity { text: actual, status: HistoryItemStatusDto::Failed, .. }] if actual == text));
	}

	#[test]
	fn assistant_chunks_coalesce_into_one_response_per_turn() {
		let conversation_id = EntityId::new("10000000-0000-4000-8000-000000000082")
			.expect("test conversation identity is canonical");
		let live_turn = EntityId::new("20000000-0000-4000-8000-000000000083")
			.expect("test live turn identity is canonical");
		let live = |item: &str, text: &str| crate::conversations::ConversationLiveDelta {
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
			conversation_transcript_rows(&snapshot, Some(&history), None),
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
		let task = ConversationSummary::new(
			EntityId::new("10000000-0000-4000-8000-000000000091")
				.expect("test conversation identity is canonical"),
			decodex_protocol::ConversationTitle::new("Outcome unknown")
				.expect("test title is valid"),
			None,
			None,
			decodex_protocol::EntityRevision(3),
			1_786_000_000_000_000,
			Some(
				EntityId::new("20000000-0000-4000-8000-000000000091")
					.expect("test runtime identity is canonical"),
			),
			Some(decodex_protocol::EntityRevision(4)),
			ConversationState::OutcomeUnknown,
			None,
			None,
		)
		.expect("outcome-unknown projection is valid");

		assert_eq!(conversation_recovery_presentation(Some(&task)), (true, "Retry sync"));
		assert_eq!(conversation_recovery_presentation(None), (false, "Recover"));
	}

	#[test]
	fn every_connection_state_has_a_bounded_deterministic_presentation() {
		let states = [
			ConnectionView::Connecting { attempt: 2 },
			ConnectionView::Online { generation: 4, applied: Some(decodex_protocol::Cursor(9)) },
			ConnectionView::OfflineRetrying { next_attempt: 3, delay: Duration::from_millis(250) },
			ConnectionView::Incompatible(CompatibilityReason::ProtocolMinor),
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
				"Reconnecting",
				"Restart Decodex",
				"Restart Decodex",
				"Shutting down",
				"Restart Decodex",
			]
		);
	}

	#[test]
	fn startup_failure_details_remain_available_outside_connection_recovery() {
		let cases = [
			(ClientFailure::ConfigurationMissing, "Client configuration is missing"),
			(ClientFailure::ConfigurationMalformed, "Client configuration is malformed"),
			(ClientFailure::UnsafeHostPath, "Client configuration path is unsafe"),
			(ClientFailure::ProfileMissing, "Selected server profile is missing"),
			(ClientFailure::ServerIdentityUnavailable, "Stable server identity is unavailable"),
		];

		for (failure, detail) in cases {
			assert_eq!(startup_failure(failure), detail);
		}
		assert_eq!(startup_failure(ClientFailure::ServiceVersionMismatch), "Restart Decodex.");
	}

	#[test]
	fn account_quota_hides_unavailable_windows() {
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
					..quota(AccountQuotaStateDto::NotApplicable)
				}
			)
			.is_none()
		);
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
	fn quarantine_reasons_present_one_recovery_action() {
		let cases = [
			QuarantineReason::StableServerIdentity,
			QuarantineReason::CacheCorrupt,
			QuarantineReason::ApplicationOrder,
			QuarantineReason::ApplicationConfirmation,
			QuarantineReason::StaleConnectionGeneration,
		];

		for reason in cases {
			let view = ConnectionView::Quarantined {
				reason,
				recovery: QuarantineRecovery::OperatorRequired,
			};
			assert_eq!(connection_presentation(view).label, "Restart Decodex");
		}
	}

	#[test]
	fn route_rejections_preserve_the_quit_and_login_actions() {
		assert_eq!(
			account_rejection_label(AccountCommandRejectionDto::CodexIsRunning),
			"Quit ChatGPT or Codex, then try switching again."
		);
		assert_eq!(
			account_rejection_label(AccountCommandRejectionDto::CredentialNeedsLogin),
			"This account needs you to sign in again."
		);
	}

	#[test]
	fn health_distinguishes_core_readiness_from_deferred_capabilities() {
		let checks = DoctorComponent::ALL
			.into_iter()
			.map(|component| {
				let status = match component {
					DoctorComponent::AppServerCapability(_) | DoctorComponent::BlobIntegrity =>
						DoctorStatus::Unknown(DoctorIssue::NotProbed),
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
	fn recorded_turn_acknowledgement_preserves_later_and_failed_input(cx: &mut TestAppContext) {
		use decodex_protocol::ConversationTurnOutcomeState as Outcome;
		let (shell, visual) = open_shell(cx);
		for (outcome, text, expected) in [
			(Outcome::Completed, "Original message", ""),
			(Outcome::Completed, "Later unsent input", "Later unsent input"),
			(Outcome::Failed, "Original message", "Original message"),
			(Outcome::NotSubmitted, "Original message", "Original message"),
		] {
			let (conversations, server, original) =
				crate::conversations::tests::recorded_turn_fixture(outcome);
			shell.update(visual, |s, cx| {
				s.conversations = conversations.clone();
				s.selected = Destination::Conversations;
				s.ordinary_owner = conversations.ordinary_editor_owner();
				s.composer.update(cx, |input, cx| input.set_content(text, cx));
				s.synchronize_conversations(cx);
			});
			visual.update(|window, cx| {
				window.resize(size(px(1440.), px(1000.)));
				window.draw(cx).clear();
			});
			let button = visual
				.debug_bounds("ordinary-turn-acknowledge-0")
				.expect("acknowledge terminal outcome");
			visual.simulate_click(button.center(), gpui::Modifiers::default());
			shell.read_with(visual, |s, cx| {
				assert_eq!(s.composer.read(cx).content(), expected);
				assert!(s.conversations.ordinary_turn_outcomes().is_empty());
			});
			assert_eq!(conversations.confirmed_ordinary_commands(), vec![original]);
			assert!(
				crate::conversations::tests::take_ready_command(&conversations, &server).is_none()
			);
		}
	}

	#[gpui::test]
	fn recorded_creation_open_button_retains_later_input_without_replay(cx: &mut TestAppContext) {
		let (shell, visual) = open_shell(cx);
		for text in ["Original creation input", "Later unsent input"] {
			let (conversations, server, original) =
				crate::conversations::tests::recorded_creation_fixture(text);
			shell.update(visual, |s, cx| {
				s.conversations = conversations.clone();
				s.selected = Destination::Conversations;
				s.ordinary_owner = None;
				s.composer.update(cx, |input, cx| input.set_content(text, cx));
				s.synchronize_conversations(cx);
			});
			visual.update(|window, cx| {
				window.resize(size(px(1440.), px(1000.)));
				window.draw(cx).clear();
			});
			let button = visual
				.debug_bounds("ordinary-creation-open-0")
				.expect("open saved creation button");
			visual.simulate_click(button.center(), gpui::Modifiers::default());
			let decodex_protocol::CommandPayload::CreateConversation { conversation_id, .. } =
				&original.payload
			else {
				panic!("creation")
			};
			shell.read_with(visual, |s, cx| {
				assert_eq!(
					s.composer.read(cx).content(),
					if text == "Original creation input" { "" } else { text }
				);
				assert_eq!(s.conversations.ordinary_editor_owner().as_ref(), Some(conversation_id));
				assert!(s.conversations.ordinary_creation_receipts().is_empty());
				assert!(
					!s.conversations.snapshot().can_submit,
					"missing task must first be read back"
				);
			});
			assert_eq!(conversations.confirmed_ordinary_commands(), vec![original]);
			assert!(
				crate::conversations::tests::take_ready_command(&conversations, &server).is_none()
			);
		}
	}

	#[gpui::test]
	fn ordinary_creation_waits_for_defaults_then_sends_the_rendered_selection(
		cx: &mut TestAppContext,
	) {
		let (shell, visual) = open_shell(cx);
		let (conversations, server, _) = crate::conversations::tests::catalog_conversations();
		conversations.begin_new();
		shell.update(visual, |s, cx| {
			s.conversations = conversations.clone();
			s.selected = Destination::Conversations;
			s.creating_new = true;
			s.composer.update(cx, |input, cx| input.set_content("Keep my input", cx));
			s.synchronize_conversations(cx);
		});
		visual.update(|window, cx| {
			window.resize(size(px(1440.), px(1000.)));
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("conversation-defaults-pending").is_some());
		let send = visual.debug_bounds("conversation-send").unwrap();
		visual.simulate_click(send.center(), gpui::Modifiers::default());
		shell.update(visual, |s, cx| {
			assert!(s.pending_submission.is_none());
			assert_eq!(s.composer.read(cx).content(), "Keep my input");
		});
		crate::conversations::creation_defaults_tests::reply_defaults(
			&conversations,
			&server,
			decodex_protocol::InitialModelDefaults {
				configured: decodex_protocol::InitialExecutionDefaults {
					model: Some(
						decodex_protocol::ConversationModel::new("configured-model").unwrap(),
					),
					reasoning_effort: None,
					service_tier: Some(decodex_protocol::ServiceTier::new("flex").unwrap()),
				},
				managed: Default::default(),
				catalog_model: None,
			},
		);
		shell.update(visual, |s, cx| s.synchronize_conversations(cx));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("conversation-defaults-pending").is_none());
		assert!(conversations.snapshot().execution.reasoning_effort.is_none());
		let send = visual.debug_bounds("conversation-send").unwrap();
		visual.simulate_click(send.center(), gpui::Modifiers::default());
		let command = crate::conversations::tests::dispatched_command(&conversations, &server);
		let decodex_protocol::CommandPayload::CreateConversation { message, execution, .. } =
			command.payload
		else {
			panic!("creation")
		};
		assert_eq!(message.as_str(), "Keep my input");
		assert_eq!(execution.model.as_str(), "configured-model");
		assert!(execution.reasoning_effort.is_none());
		assert_eq!(execution.effective_service_tier().as_str(), "flex");
	}

	#[gpui::test]
	fn ordinary_catalog_tier_buttons_update_the_submitted_execution_settings(
		cx: &mut TestAppContext,
	) {
		let (shell, visual) = open_shell(cx);
		let (conversations, server_id, _) = crate::conversations::tests::catalog_conversations();
		shell.update(visual, |s, cx| {
			s.conversations = conversations.clone();
			s.synchronize_conversations(cx);
			s.select_destination(Destination::Conversations, cx);
		});
		visual.update(|window, cx| {
			window.resize(size(px(1440.), px(1000.)));
			window.draw(cx).clear();
		});
		for tier in ["ultrafast", "default", "ultrafast"] {
			let bounds = visual
				.debug_bounds(if tier == "default" {
					"conversation-tier-default"
				} else {
					"conversation-tier-ultrafast"
				})
				.expect("advertised tier is rendered");
			visual.simulate_click(bounds.center(), gpui::Modifiers::default());
			assert_eq!(conversations.snapshot().execution.effective_service_tier().as_str(), tier);
		}
		conversations.submit("Use the selected tier").unwrap();
		let command = crate::conversations::tests::dispatched_command(&conversations, &server_id);
		let encoded = serde_json::to_value(command).unwrap();
		assert!(encoded.to_string().contains("ultrafast"));
	}

	#[gpui::test]
	fn keyboard_focus_and_activation_cover_rendered_workbench_destinations(
		cx: &mut TestAppContext,
	) {
		let (shell, visual) = open_shell(cx);
		for expected in [Destination::Settings, Destination::Accounts, Destination::Health] {
			let focused = shell.read_with(visual, |shell, _| {
				let index = Destination::ALL
					.iter()
					.position(|value| *value == expected)
					.expect("test operation must succeed");
				shell.destination_focus[index].clone()
			});
			shell.update(visual, |shell, cx| shell.select_destination(Destination::Settings, cx));
			visual.update(|window, cx| window.focus(&focused, cx));
			assert!(visual.update(|window, _| focused.is_focused(window)));
			visual.simulate_keystrokes("enter");
			assert_eq!(shell.read_with(visual, |shell, _| shell.selected), expected);
		}
	}

	#[gpui::test]
	fn settings_window_preserves_workspace_and_reuses_one_window(cx: &mut TestAppContext) {
		let (shell, visual) = open_shell(cx);
		let before = shell.read_with(visual, |s, cx| s.chief.read(cx).workspace_panels());
		visual.simulate_keystrokes("cmd-,");
		let handle = shell.read_with(visual, |s, _| {
			assert_eq!(s.selected, Destination::Chief);
			s.settings_window.expect("settings window")
		});
		shell.update(visual, |s, cx| s.open_settings_window(Destination::Accounts, cx));
		shell.read_with(visual, |s, cx| {
			assert_eq!(s.selected, Destination::Chief);
			assert_eq!(s.settings_selected, Destination::Accounts);
			assert_eq!(s.settings_window.unwrap(), handle);
			assert_eq!(s.chief.read(cx).workspace_panels(), before);
		});
		handle.update(visual, |_, window, _| window.remove_window()).unwrap();
		shell.read_with(visual, |s, _| assert!(s.settings_window.is_none()));
		visual.update(|window, cx| {
			assert_eq!(
				cx.active_window(),
				Some(window.window_handle()),
				"closing Settings must reactivate the workspace"
			);
		});
		shell.update(visual, |s, cx| s.open_settings_window(Destination::Settings, cx));
		shell.read_with(visual, |s, _| {
			assert_ne!(s.settings_window.unwrap(), handle);
			assert_eq!(s.selected, Destination::Chief);
		});
	}

	#[gpui::test]
	fn back_forward_restore_worker_and_settings_without_duplicate_history(cx: &mut TestAppContext) {
		let (shell, visual) = open_shell(cx);
		shell.update(visual, |s, cx| {
			s.chief.update(cx, |chief, cx| chief.visual_workspace_fixture(cx))
		});
		shell.update(visual, |s, cx| {
			s.chief.update(cx, |chief, cx| chief.restore_work(Some("verify"), cx));
			s.record_navigation(cx);
		});
		shell.update(visual, |s, cx| s.select_destination(Destination::Settings, cx));
		visual.simulate_keystrokes("cmd-[");
		shell.read_with(visual, |s, cx| {
			assert_eq!(s.selected, Destination::Chief);
			assert_eq!(s.chief.read(cx).navigation_work().as_deref(), Some("verify"));
		});
		visual.simulate_keystrokes("cmd-[");
		assert_eq!(shell.read_with(visual, |s, cx| s.chief.read(cx).navigation_work()), None);
		visual.simulate_keystrokes("cmd-]");
		assert_eq!(
			shell.read_with(visual, |s, cx| s.chief.read(cx).navigation_work()),
			Some("verify".into())
		);
		visual.simulate_keystrokes("cmd-]");
		assert_eq!(shell.read_with(visual, |s, _| s.selected), Destination::Settings);
	}

	#[gpui::test]
	fn chief_panel_shortcuts_control_the_current_work_page(cx: &mut TestAppContext) {
		let (shell, visual) = open_shell(cx);
		shell.update(visual, |s, cx| {
			s.chief.update(cx, |chief, cx| chief.visual_workspace_fixture(cx))
		});
		let panels = |visual: &mut VisualTestContext| {
			shell.read_with(visual, |s, cx| s.chief.read(cx).workspace_panels())
		};
		assert_eq!(panels(visual), [(true, true), (true, true), (true, true), (true, true)]);
		visual.simulate_keystrokes("cmd-j");
		assert_eq!(panels(visual), [(true, true), (false, true), (true, true), (true, true)]);
		visual.simulate_keystrokes("cmd-e");
		assert_eq!(panels(visual), [(false, true), (false, true), (true, true), (true, true)]);
		visual.simulate_keystrokes("cmd-b");
		assert_eq!(panels(visual), [(false, true), (false, true), (true, true), (false, true)]);
		visual.simulate_keystrokes("cmd-e cmd-j cmd-b");
		assert_eq!(panels(visual), [(true, true), (true, true), (true, true), (true, true)]);
	}

	#[gpui::test]
	fn panel_resize_keyboard_bindings_reach_the_focused_panel(cx: &mut TestAppContext) {
		let (shell, visual) = open_shell(cx);
		visual.simulate_resize(gpui::size(px(1400.), px(1000.)));
		shell.update(visual, |s, cx| s.chief.update(cx, |a, cx| a.visual_workspace_fixture(cx)));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let dimensions = |visual: &mut VisualTestContext| {
			shell.read_with(visual, |s, cx| s.chief.read(cx).panel_dimensions())
		};
		let initial = dimensions(visual);
		visual.simulate_click(gpui::point(px(50.), px(170.)), Default::default());
		visual.simulate_keystrokes("ctrl-alt-=");
		assert_eq!(dimensions(visual), (initial.0 + 24., initial.1, initial.2));
		visual.simulate_keystrokes("ctrl-alt--");
		assert_eq!(dimensions(visual), initial);
		visual.simulate_keystrokes("ctrl-alt-shift-=");
		assert_eq!(dimensions(visual), (initial.0 + 24., initial.1 + 24., initial.2 + 24.));
		visual.simulate_keystrokes("ctrl-alt-_");
		assert_eq!(dimensions(visual), initial);
		visual.simulate_keystrokes("ctrl-alt-+");
		assert_eq!(dimensions(visual), (initial.0 + 24., initial.1 + 24., initial.2 + 24.));
		visual.simulate_keystrokes("ctrl-alt-)");
		let defaults = crate::panel_preferences::PanelDefaults::configured();
		assert_eq!(
			dimensions(visual),
			(defaults.sidebar.into(), defaults.sidebar.into(), defaults.dock.into())
		);
	}

	#[gpui::test]
	fn panel_shortcuts_toggle_both_workbench_sidebars(cx: &mut TestAppContext) {
		let (shell, visual) = open_shell(cx);
		shell.update(visual, |shell, cx| shell.select_destination(Destination::Conversations, cx));
		assert!(shell.read_with(visual, |shell, _| shell.left_sidebar_visible));
		assert!(shell.read_with(visual, |shell, _| shell.inspector_visible));

		visual.simulate_keystrokes("cmd-e");
		visual.simulate_keystrokes("cmd-b");

		assert!(!shell.read_with(visual, |shell, _| shell.left_sidebar_visible));
		assert!(!shell.read_with(visual, |shell, _| shell.inspector_visible));
		assert!(shell.read_with(visual, |shell, _| shell.left_sidebar_mounted));
		assert!(shell.read_with(visual, |shell, _| shell.inspector_mounted));

		visual.executor().advance_clock(ui_theme::MOTION_PANEL + Duration::from_millis(24));
		visual.run_until_parked();
		assert!(!shell.read_with(visual, |shell, _| shell.left_sidebar_mounted));
		assert!(!shell.read_with(visual, |shell, _| shell.inspector_mounted));

		visual.simulate_keystrokes("cmd-e");
		visual.simulate_keystrokes("cmd-b");
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
				assert_eq!(WINDOW_CONTROLS_CLEARANCE, 44.0);
				assert_eq!(WORKBENCH_SESSION_SIDEBAR_WIDTH, 248.0);
				assert_eq!(WORKBENCH_INSPECTOR_WIDTH, 344.0);
			});
		}
	}
}
