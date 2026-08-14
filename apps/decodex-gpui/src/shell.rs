//! Production GPUI window, navigation, focus, and lifecycle rendering boundary.

use std::{future::Future, pin::Pin, sync::mpsc::Receiver, time::Duration};

use gpui::{
	Animation, AnimationExt, AnyElement, App, BoxShadow, Context, Entity, FocusHandle, Focusable,
	FontWeight, Global, Hsla, KeyBinding, MouseButton, Render, Role, SharedString, Subscription,
	Task, WeakEntity, Window, WindowControlArea, WindowHandle, WindowId, actions, div, ease_in_out,
	img, prelude::*, px, rgb, rgba,
};

use decodex_protocol::{
	AppServerCapability, ClientFailure, DoctorComponent, DoctorStatus, EntityId,
	HistoryItemKindDto, HistoryItemStatusDto, HistoryPayloadDto, HistoryTurnRole,
	QuickTaskRecoveryAction, QuickTaskState, WorkItemBoardCard, WorkItemState,
};

use crate::{
	client_lifecycle::{
		ClientLifecycle, CompatibilityReason, ConnectionView, LifecycleCancellation,
		QuarantineReason, QuarantineRecovery,
	},
	composer_input::{self, ComposerEvent, ComposerInput, MAX_COMPOSER_BYTES, SubmitComposer},
	factory_surface::{FactoryEvent, FactoryRoute, FactorySurface, app_icon_path},
	health_query::{HealthLoadState, HealthQuery, HealthSnapshot},
	history_pager::{HistoryLoadState, HistoryPager, HistorySnapshot},
	quick_tasks::{
		QuickTaskCommandState, QuickTaskInputError, QuickTasks, QuickTasksLoadState,
		QuickTasksSnapshot,
	},
	settings_surface::SettingsSurface,
	ui_theme,
	work_items::{WorkItems, WorkItemsSnapshot},
};

const WORKBENCH_TOPBAR_HEIGHT: f32 = 48.0;
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
				CompatibilityReason::PublicationIdentityUnavailable => {
					"Publication identity is unavailable"
				},
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
		ClientFailure::RemoteMutationUnsupported => {
			"Reset-card operations require a local pinned profile"
		},
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
	health_query: HealthQuery,
	health: HealthSnapshot,
	quick_tasks: QuickTasks,
	quick: QuickTasksSnapshot,
	work_items: WorkItems,
	work: WorkItemsSnapshot,
	history_pager: Option<HistoryPager>,
	history: Option<HistorySnapshot>,
	opened_history: Option<EntityId>,
	creating_new: bool,
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
		let health_query = HealthQuery::production();
		let health = health_query.snapshot();
		let quick_tasks = QuickTasks::production();
		quick_tasks.activate();
		let quick = quick_tasks.snapshot();
		let work_items = WorkItems::production();
		work_items.activate();
		let work = work_items.snapshot();
		factory.update(cx, |factory, cx| factory.bind_work_items(work_items.clone(), cx));
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
			health_query,
			health,
			quick_tasks,
			quick,
			work_items,
			work,
			history_pager: None,
			history: None,
			opened_history: None,
			creating_new: true,
			input_status: None,
			titlebar_drag_pending: false,
		}
	}

	#[cfg(feature = "visual-capture")]
	#[allow(dead_code)]
	pub(crate) fn visual_workbench(window: &mut Window, cx: &mut Context<Self>) -> Self {
		use decodex_protocol::{
			ConversationHistoryPage, EntityRevision, ProjectSummary, QuickTaskSummary, WireText,
			WorkItemBoardLeadId, WorkItemBoardProjectId, WorkItemBoardTitle,
			WorkItemBoardWorkItemId, WorkItemPriority,
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
				self.creating_new = true;
				self.opened_history = None;
				let prompt = format!("Decodex factory context: {context}\n\n{message}");
				match self.quick_tasks.create(&prompt) {
					Ok(()) => self.input_status = None,
					Err(error) => self.input_status = Some(input_error_label(error).into()),
				}
				self.synchronize_quick_tasks();
				self.select_destination(Destination::QuickTasks, cx);
			},
			FactoryEvent::OpenWorkItemConversation { conversation_id } => {
				self.quick_tasks.select_when_available(conversation_id.clone());
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
		if matches!(self.selected, Destination::Factory | Destination::QuickTasks)
			&& !matches!(destination, Destination::Factory | Destination::QuickTasks)
		{
			self.work_items.deactivate();
		}

		self.selected = destination;
		if destination == Destination::Health {
			self.health_query.activate();
		}
		if destination == Destination::QuickTasks {
			self.quick_tasks.activate();
		}
		if matches!(destination, Destination::Factory | Destination::QuickTasks) {
			self.work_items.activate();
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
		cx.notify();
	}

	fn bind_work_items(&mut self, work_items: WorkItems, cx: &mut Context<Self>) {
		self.work_items.deactivate();
		self.work_items = work_items;
		if matches!(self.selected, Destination::Factory | Destination::QuickTasks) {
			self.work_items.activate();
		}
		self.factory.update(cx, |factory, cx| factory.bind_work_items(self.work_items.clone(), cx));
		self.synchronize_work_items(cx);
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

	fn synchronize_quick_tasks(&mut self) {
		let snapshot = self.quick_tasks.snapshot();
		let selected = snapshot.selected.clone();
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
	}

	fn start_new_quick_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.quick_tasks.begin_new();
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
		let result = if creating {
			self.quick_tasks.create(&message)
		} else {
			self.quick_tasks.submit(&message)
		};
		match result {
			Ok(()) => {
				self.composer.update(cx, |composer, cx| composer.clear(cx));
				self.creating_new = creating;
				self.input_status = None;
			},
			Err(error) => self.input_status = Some(input_error_label(error).into()),
		}
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
		QuickTaskInputError::WorkingDirectoryUnavailable => {
			"The local Quick Task working directory is unavailable."
		},
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
	let health_query = lifecycle.health_query();
	let quick_tasks = lifecycle.quick_tasks();
	let work_items = lifecycle.work_items();
	let history_pager = lifecycle.history_pager();
	shell.update(cx, |shell, cx| {
		shell.bind_health_query(health_query, cx);
		shell.bind_quick_tasks(quick_tasks, history_pager, cx);
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
		let health = shell.health_query.snapshot();
		let quick = shell.quick_tasks.snapshot();
		let work = shell.work_items.snapshot();
		let history = shell.history_pager.as_ref().map(HistoryPager::snapshot);

		if health != shell.health {
			shell.health = health;
			cx.notify();
		}
		if quick != shell.quick || history != shell.history {
			shell.synchronize_quick_tasks();
			cx.notify();
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
		HealthLoadState::Ready => HealthPresentation {
			label: "Current",
			detail: "The bounded health report is current.",
			color: 0x22c55e,
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
		Some(DoctorStatus::Ready) => {
			HealthPresentation { label: "Ready", detail: "", color: 0x22c55e }
		},
		Some(DoctorStatus::Unavailable(_)) => {
			HealthPresentation { label: "Unavailable", detail: "", color: 0xef4444 }
		},
		Some(DoctorStatus::Unknown(_)) => {
			HealthPresentation { label: "Unknown", detail: "", color: 0xf59e0b }
		},
		None => HealthPresentation { label: "No report", detail: "", color: 0x64748b },
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
		.h(px(44.0))
		.min_h(px(44.0))
		.flex()
		.items_center()
		.justify_between()
		.gap_4()
		.border_b_1()
		.border_color(rgba(0xffffff0a))
		.text_size(px(10.5))
		.text_color(rgb(WB_TEXT_MUTED))
		.child(div().min_w_0().child(label))
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

fn accounts_content(cx: &mut Context<Shell>) -> AnyElement {
	div()
		.flex_1()
		.min_h_0()
		.p_7()
		.flex()
		.justify_center()
		.child(
			div()
				.w_full()
				.max_w(px(820.0))
				.flex()
				.flex_col()
				.gap_4()
				.child(
					div()
						.p_6()
						.flex()
						.items_center()
						.justify_between()
						.gap_6()
						.rounded(px(14.0))
						.border_1()
						.border_color(rgba(0xffffff12))
						.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
						.child(
							div()
								.min_w_0()
								.flex()
								.flex_col()
								.gap_2()
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
												.child("Codex account operations"),
										),
								)
								.child(
									div()
										.max_w(px(580.0))
										.text_size(px(11.0))
										.line_height(px(17.0))
										.text_color(rgb(WB_TEXT_MUTED))
										.child("Multi-account quota, routing, reauthentication, and recovery remain available through the signed menu bar companion."),
								),
						)
						.child(
							div()
								.id("accounts-open-settings")
								.role(Role::Button)
								.aria_label("Manage desktop surfaces")
								.h(px(30.0))
								.px_3()
								.flex()
								.items_center()
								.rounded(px(8.0))
								.border_1()
								.border_color(rgba(0xffffff18))
								.text_size(px(9.5))
								.text_color(rgb(WB_TEXT_MUTED))
								.cursor_pointer()
								.hover(|element| {
									element.bg(rgba(0xffffff0e)).text_color(rgb(WB_TEXT))
								})
								.active(|element| element.bg(rgba(0xffffff1c)).opacity(0.82))
								.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
								.on_click(cx.listener(|shell, _, _, cx| {
									shell.select_destination(Destination::Settings, cx);
								}))
								.child("Desktop settings"),
						),
				)
				.child(
					div()
						.p_5()
						.rounded(px(12.0))
						.border_1()
						.border_color(rgba(0xffffff0d))
						.bg(rgba(0x0000001f))
						.font_family("SF Mono")
						.text_size(px(8.5))
						.line_height(px(14.0))
						.text_color(rgb(WB_TEXT_FAINT))
						.child("AUTHORITY BOUNDARY  ·  Decodex renders account readiness; the Codex app-server remains the account authority."),
				),
		)
		.into_any_element()
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
		QuickTaskState::OutcomeUnknown => "Outcome unknown",
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
		QuickTaskState::ManualRecovery | QuickTaskState::OutcomeUnknown => 0xef4444,
	}
}

fn command_status(command: QuickTaskCommandState) -> Option<&'static str> {
	match command {
		QuickTaskCommandState::Idle => None,
		QuickTaskCommandState::Sending => Some("Sending command"),
		QuickTaskCommandState::AwaitingResult => Some("Waiting for durable result"),
		QuickTaskCommandState::Accepted => Some("Command accepted"),
		QuickTaskCommandState::ManualRecovery(action) => Some(recovery_action_label(action)),
		QuickTaskCommandState::OutcomeUnknown => {
			Some("Outcome unknown. Readback will resume after reconnect; do not resend.")
		},
		QuickTaskCommandState::Refused => Some("The command was refused."),
	}
}

fn recovery_action_label(action: QuickTaskRecoveryAction) -> &'static str {
	match action {
		QuickTaskRecoveryAction::ResumeRouting => "Resume the pending account route.",
		QuickTaskRecoveryAction::CreateRoutingSuccessor => {
			"Create a new conversation and route it explicitly."
		},
		QuickTaskRecoveryAction::ResumeEstablishment => {
			"Resume the selected account session establishment."
		},
		QuickTaskRecoveryAction::ConfigureAccount => "Configure an account before continuing.",
		QuickTaskRecoveryAction::EnableAccount => "Enable the selected account before continuing.",
		QuickTaskRecoveryAction::EnrollCredentials => {
			"Enroll account credentials before continuing."
		},
		QuickTaskRecoveryAction::ResolveAccountOperation => {
			"Resolve the unsettled account operation before continuing."
		},
		QuickTaskRecoveryAction::RepairCredentialStore => {
			"Repair the protected credential store before continuing."
		},
		QuickTaskRecoveryAction::RestoreProviderAgreement => {
			"Restore provider account agreement before continuing."
		},
		QuickTaskRecoveryAction::RefreshQuota => "Refresh account quota before continuing.",
		QuickTaskRecoveryAction::UpgradeCodex => {
			"Use a Codex build with the required app-server methods."
		},
		QuickTaskRecoveryAction::SelectWorkingDirectory => {
			"Select an owned local working directory before continuing."
		},
		QuickTaskRecoveryAction::StartNewConversation => {
			"This thread cannot resume. Start a new conversation."
		},
		QuickTaskRecoveryAction::ResolvePriorActiveTurn => {
			"Resolve the prior active turn before continuing."
		},
		QuickTaskRecoveryAction::ResolvePriorAttempt => {
			"Resolve the prior provider attempt before continuing."
		},
		QuickTaskRecoveryAction::RestoreProcessReadiness => {
			"Restore process readiness before continuing."
		},
		QuickTaskRecoveryAction::WaitForCurrentCommand => {
			"Wait for the current command or turn to settle."
		},
		QuickTaskRecoveryAction::RefreshConversation => {
			"Refresh this conversation before continuing."
		},
	}
}

fn quick_task_load_status(load: QuickTasksLoadState) -> &'static str {
	match load {
		QuickTasksLoadState::NeverRequested => "Quick Tasks have not loaded.",
		QuickTasksLoadState::Loading => "Loading Quick Tasks",
		QuickTasksLoadState::Ready => "Codex state is current.",
		QuickTasksLoadState::Offline => "Offline. Retained conversation state remains visible.",
		QuickTasksLoadState::Unavailable => "Quick Task state is temporarily unavailable.",
		QuickTasksLoadState::Refused => "Quick Task readback was refused.",
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
						.font_weight(FontWeight::SEMIBOLD)
						.text_size(px(10.0))
						.text_color(rgb(WB_TEXT))
						.child("Sessions"),
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
						.hover(|element| element.bg(rgba(0xffffff0a)).text_color(rgb(WB_TEXT)))
						.active(|element| element.bg(rgba(0xffffff18)).opacity(0.82))
						.focus_visible(|element| element.border_color(rgb(WB_BLUE)))
						.cursor_pointer()
						.on_click(cx.listener(|shell, _, window, cx| {
							shell.start_new_quick_task(window, cx);
						}))
						.child("+ New"),
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
) -> AnyElement {
	let selected = snapshot.selected.as_ref();
	let persisted_inline_ids = history
		.and_then(|history| history.visible.as_ref())
		.into_iter()
		.flat_map(|page| page.items.iter())
		.filter(|item| item.payload.inline_text().is_some())
		.map(|item| &item.history_item_id)
		.collect::<Vec<_>>();
	let history_rows = history
		.and_then(|history| history.visible.as_ref())
		.into_iter()
		.flat_map(|page| page.items.iter())
		.map(|item| {
			let text = match &item.payload {
				HistoryPayloadDto::Inline { text } => text.as_str().to_owned(),
				HistoryPayloadDto::Blob(reference) => format!(
					"Stored content: {} bytes; SHA-256 {}...",
					reference.byte_length.get(),
					&reference.sha256.as_str()[..12],
				),
			};
			let content = match item.turn_role {
				HistoryTurnRole::User => div()
					.w_full()
					.flex()
					.justify_end()
					.child(
						div()
							.max_w(px(560.0))
							.px_3()
							.py_2()
							.rounded(px(10.0))
							.bg(rgba(0xffffff0e))
							.border_1()
							.border_color(rgba(0xffffff0b))
							.text_size(px(11.0))
							.text_color(rgb(WB_TEXT))
							.whitespace_normal()
							.child(text),
					)
					.into_any_element(),
				HistoryTurnRole::Assistant if item.kind == HistoryItemKindDto::Message => div()
					.w_full()
					.flex()
					.flex_col()
					.gap_2()
					.child(
						div()
							.text_size(px(9.0))
							.font_weight(FontWeight::MEDIUM)
							.text_color(rgb(WB_TEXT_FAINT))
							.child("CODEX"),
					)
					.child(
						div()
							.text_size(px(11.0))
							.text_color(rgb(WB_TEXT))
							.whitespace_normal()
							.child(text),
					)
					.into_any_element(),
				HistoryTurnRole::Tool | HistoryTurnRole::System | HistoryTurnRole::Assistant => {
					div()
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
								.text_color(rgb(if item.status == HistoryItemStatusDto::Failed {
									WB_AMBER
								} else {
									WB_TEXT_FAINT
								}))
								.child(history_kind_label(item.kind, item.status)),
						)
						.child(
							div()
								.min_w_0()
								.overflow_hidden()
								.whitespace_nowrap()
								.text_ellipsis()
								.child(text),
						)
						.into_any_element()
				},
			};
			div()
				.w_full()
				.py_2()
				.flex()
				.justify_center()
				.child(div().w_full().max_w(px(760.0)).child(content))
		});
	let live_rows = snapshot
		.live_deltas
		.iter()
		.filter(|delta| {
			selected == Some(&delta.conversation_id)
				&& !persisted_inline_ids.contains(&&delta.history_item_id)
		})
		.map(|delta| {
			div().w_full().py_2().flex().justify_center().child(
				div()
					.w_full()
					.max_w(px(760.0))
					.flex()
					.flex_col()
					.gap_2()
					.child(
						div().text_size(px(9.0)).text_color(rgb(WB_ACCENT)).child("CODEX · LIVE"),
					)
					.child(
						div()
							.text_size(px(11.0))
							.text_color(rgb(WB_TEXT))
							.whitespace_normal()
							.child(delta.text.as_str().to_owned()),
					),
			)
		});
	let history_status =
		history.map_or("Conversation history is not connected.", |history| match history.load {
			HistoryLoadState::Inactive => "Select a conversation or start a new conversation.",
			HistoryLoadState::InitialLoading | HistoryLoadState::RefreshingVisible => {
				"Loading conversation history"
			},
			HistoryLoadState::PrefetchingAdjacent | HistoryLoadState::Visible => "",
			HistoryLoadState::RetryableUnavailable(_) => {
				"History is temporarily unavailable. Reconnect or retry."
			},
			HistoryLoadState::ClosedUnavailable(_) => "History readback was refused.",
		});

	div()
		.id("quick-task-transcript")
		.role(Role::Log)
		.aria_label("Quick Task conversation")
		.flex_1()
		.min_h_0()
		.overflow_y_scroll()
		.px_5()
		.py_5()
		.when(!history_status.is_empty(), |element| {
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
		.children(history_rows)
		.children(live_rows)
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
	let has_pre_session_recovery = task.is_some_and(|task| {
		matches!(
			task.recovery_action,
			Some(
				QuickTaskRecoveryAction::ResumeRouting
					| QuickTaskRecoveryAction::CreateRoutingSuccessor
					| QuickTaskRecoveryAction::ResumeEstablishment
			)
		)
	});
	let can_continue = shell.creating_new
		|| task.is_none()
		|| task.is_some_and(|task| task.state == QuickTaskState::Ready || has_pre_session_recovery);
	let composer = shell.composer.read(cx);
	let composer_len = composer.len();
	let has_message = !composer.content().trim().is_empty();
	let can_send =
		shell.quick.can_submit && can_continue && (has_message || has_pre_session_recovery);
	let can_interrupt =
		shell.quick.can_submit && task.is_some_and(|task| task.state == QuickTaskState::Running);

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
	let context = task.map_or_else(
		|| "New local session".to_owned(),
		|task| {
			format!(
				"{} · conversation r{}",
				quick_task_state_label(task.state),
				task.conversation_revision.0
			)
		},
	);
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
								.gap_2()
								.text_size(px(8.5))
								.text_color(rgb(WB_TEXT_FAINT))
								.child(context)
								.when(composer_len > 0, |element| {
									element.child(format!("· {composer_len}/{MAX_COMPOSER_BYTES}"))
								}),
						)
						.child(div().flex().gap_2().child(interrupt).child(send)),
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
						.child(quick_task_transcript(&shell.quick, shell.history.as_ref()))
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
	let rows = DoctorComponent::ALL.into_iter().enumerate().map(|(index, component)| {
		let status = snapshot
			.report
			.as_ref()
			.and_then(|report| report.check(component))
			.map(|check| check.status);

		health_component_row(index, component, status)
	});

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
				.rounded_t(px(12.0))
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
				.role(Role::List)
				.aria_label("Health components")
				.px_4()
				.border_1()
				.border_t_0()
				.border_color(rgba(0xffffff10))
				.rounded_b(px(12.0))
				.bg(rgba(0x00000024))
				.children(rows),
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
		accounts_content(cx)
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
				assert_eq!(WORKBENCH_TOPBAR_HEIGHT, 48.0);
				assert_eq!(WORKBENCH_SESSION_SIDEBAR_WIDTH, 248.0);
				assert_eq!(WORKBENCH_INSPECTOR_WIDTH, 344.0);
			});
		}
	}
}
