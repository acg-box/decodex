//! Production GPUI window, navigation, focus, and lifecycle rendering boundary.

use std::{future::Future, pin::Pin, sync::mpsc::Receiver, time::Duration};

use gpui::{
	AnyElement, App, Context, Entity, FocusHandle, Focusable, Global, KeyBinding, Render, Role,
	SharedString, Subscription, Task, WeakEntity, Window, WindowHandle, WindowId, actions, div,
	prelude::*, px, rgb,
};

use decodex_protocol::{
	AppServerCapability, ClientFailure, DoctorComponent, DoctorStatus, EntityId, HistoryPayloadDto,
	HistoryTurnRole, QuickTaskRecoveryAction, QuickTaskState,
};

use crate::{
	client_lifecycle::{
		ClientLifecycle, CompatibilityReason, ConnectionView, LifecycleCancellation,
		QuarantineReason, QuarantineRecovery,
	},
	composer_input::{self, ComposerEvent, ComposerInput, MAX_COMPOSER_BYTES, SubmitComposer},
	health_query::{HealthLoadState, HealthQuery, HealthSnapshot},
	history_pager::{HistoryLoadState, HistoryPager, HistorySnapshot},
	quick_tasks::{
		QuickTaskCommandState, QuickTaskInputError, QuickTasks, QuickTasksLoadState,
		QuickTasksSnapshot,
	},
};

const SIDEBAR_WIDTH: f32 = 224.0;
const HEADER_HEIGHT: f32 = 64.0;
const STATUS_HEIGHT: f32 = 72.0;
const TASK_LIST_WIDTH: f32 = 216.0;
const LIFECYCLE_POLL: Duration = Duration::from_millis(40);

actions!(decodex_shell, [FocusNext, FocusPrevious, ActivateDestination, RefreshHealth]);

/// Stable shell destinations. Each live destination remains issue-owned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Destination {
	Advisor,
	Projects,
	QuickTasks,
	Runs,
	Automations,
	Accounts,
	Health,
}

impl Destination {
	pub(crate) const ALL: [Self; 7] = [
		Self::Advisor,
		Self::Projects,
		Self::QuickTasks,
		Self::Runs,
		Self::Automations,
		Self::Accounts,
		Self::Health,
	];

	pub(crate) const fn label(self) -> &'static str {
		match self {
			Self::Advisor => "Advisor",
			Self::Projects => "Projects",
			Self::QuickTasks => "Quick Tasks",
			Self::Runs => "Runs",
			Self::Automations => "Automations",
			Self::Accounts => "Accounts",
			Self::Health => "Health",
		}
	}

	const fn description(self) -> &'static str {
		match self {
			Self::Advisor => "Guidance and decisions will appear here.",
			Self::Projects => "Project workspaces will appear here.",
			Self::QuickTasks => "Quick task dispatch will appear here.",
			Self::Runs => "Managed run activity will appear here.",
			Self::Automations => "Automation operations will appear here.",
			Self::Accounts => "Account readiness will appear here.",
			Self::Health => "Health",
		}
	}
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
	cx.bind_keys([
		KeyBinding::new("tab", FocusNext, None),
		KeyBinding::new("shift-tab", FocusPrevious, None),
		KeyBinding::new("enter", ActivateDestination, Some("Destination")),
		KeyBinding::new("space", ActivateDestination, Some("Destination")),
		KeyBinding::new("enter", RefreshHealth, Some("HealthRefresh")),
		KeyBinding::new("space", RefreshHealth, Some("HealthRefresh")),
	]);
}

/// One window-owned production shell. Connection ownership lives at application scope.
pub(crate) struct Shell {
	selected: Destination,
	connection: ConnectionView,
	root_focus: FocusHandle,
	destination_focus: Vec<FocusHandle>,
	refresh_focus: FocusHandle,
	composer: Entity<ComposerInput>,
	health_query: HealthQuery,
	health: HealthSnapshot,
	quick_tasks: QuickTasks,
	quick: QuickTasksSnapshot,
	history_pager: Option<HistoryPager>,
	history: Option<HistorySnapshot>,
	opened_history: Option<EntityId>,
	creating_new: bool,
	input_status: Option<SharedString>,
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
		let composer = cx.new(|cx| ComposerInput::new(Destination::ALL.len() as isize + 1, cx));
		cx.subscribe(&composer, |shell, _, _: &ComposerEvent, cx| {
			shell.input_status = None;
			cx.notify();
		})
		.detach();
		let health_query = HealthQuery::production();
		let health = health_query.snapshot();
		let quick_tasks = QuickTasks::production();
		quick_tasks.activate();
		let quick = quick_tasks.snapshot();
		window.focus(&composer.focus_handle(cx), cx);

		Self {
			selected: Destination::QuickTasks,
			connection,
			root_focus: cx.focus_handle(),
			destination_focus,
			refresh_focus,
			composer,
			health_query,
			health,
			quick_tasks,
			quick,
			history_pager: None,
			history: None,
			opened_history: None,
			creating_new: true,
			input_status: None,
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

		self.selected = destination;
		if destination == Destination::Health {
			self.health_query.activate();
		}
		if destination == Destination::QuickTasks {
			self.quick_tasks.activate();
		}
		self.health = self.health_query.snapshot();
		cx.notify();
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
	let health_query = lifecycle.health_query();
	let quick_tasks = lifecycle.quick_tasks();
	let history_pager = lifecycle.history_pager();
	shell.update(cx, |shell, cx| {
		shell.bind_health_query(health_query, cx);
		shell.bind_quick_tasks(quick_tasks, history_pager, cx);
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
		let history = shell.history_pager.as_ref().map(HistoryPager::snapshot);

		if health != shell.health {
			shell.health = health;
			cx.notify();
		}
		if quick != shell.quick || history != shell.history {
			shell.synchronize_quick_tasks();
			cx.notify();
		}
	});
}

fn navigation_item(
	index: usize,
	destination: Destination,
	is_selected: bool,
	focus: FocusHandle,
	window: &Window,
	cx: &Context<Shell>,
) -> AnyElement {
	div()
		.id(("destination", index))
		.role(Role::Tab)
		.aria_label(destination.label())
		.aria_selected(is_selected)
		.key_context("Destination")
		.track_focus(&focus)
		.on_action(cx.listener(Shell::focus_next))
		.on_action(cx.listener(Shell::focus_previous))
		.on_action(cx.listener(Shell::activate_destination))
		.h(px(44.0))
		.w_full()
		.px_3()
		.flex()
		.items_center()
		.rounded_md()
		.text_color(if is_selected { rgb(0xffffff) } else { rgb(0xa8b3c7) })
		.bg(if is_selected { rgb(0x25324a) } else { rgb(0x121a2a) })
		.border_1()
		.border_color(if focus.is_focused(window) { rgb(0x60a5fa) } else { rgb(0x121a2a) })
		.on_click(cx.listener(move |shell, _, _, cx| {
			shell.select_destination(destination, cx);
		}))
		.child(destination.label())
		.into_any_element()
}

struct RefreshTooltip;

impl Render for RefreshTooltip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
			.px_2()
			.py_1()
			.rounded_sm()
			.bg(rgb(0x25324a))
			.text_sm()
			.text_color(rgb(0xffffff))
			.child("Refresh health")
	}
}

struct ControlTooltip(&'static str);

impl Render for ControlTooltip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
			.px_2()
			.py_1()
			.rounded_sm()
			.bg(rgb(0x25324a))
			.text_sm()
			.text_color(rgb(0xffffff))
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
		Some(DoctorStatus::Ready) =>
			HealthPresentation { label: "Ready", detail: "", color: 0x22c55e },
		Some(DoctorStatus::Unavailable(_)) =>
			HealthPresentation { label: "Unavailable", detail: "", color: 0xef4444 },
		Some(DoctorStatus::Unknown(_)) =>
			HealthPresentation { label: "Unknown", detail: "", color: 0xf59e0b },
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
		.border_color(rgb(0x263249))
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
		.size(px(36.0))
		.min_w(px(36.0))
		.min_h(px(36.0))
		.flex()
		.items_center()
		.justify_center()
		.rounded_md()
		.border_1()
		.border_color(if can_refresh && focus.is_focused(window) {
			rgb(0x60a5fa)
		} else {
			rgb(0x3b4962)
		})
		.bg(if can_refresh { rgb(0x1b263b) } else { rgb(0x121a2a) })
		.text_color(if can_refresh { rgb(0xe5e7eb) } else { rgb(0x64748b) })
		.when(can_refresh, |element| {
			element
				.key_context("HealthRefresh")
				.track_focus(&focus)
				.on_action(cx.listener(Shell::focus_next))
				.on_action(cx.listener(Shell::focus_previous))
				.on_action(cx.listener(Shell::refresh_health))
				.on_click(cx.listener(|shell, _, _, cx| shell.request_health_refresh(cx)))
				.cursor_pointer()
				.hover(|element| element.bg(rgb(0x25324a)))
		})
		.text_xl()
		.child("↻")
		.into_any_element()
}

fn destination_header(
	selected: Destination,
	health: &HealthSnapshot,
	refresh_focus: FocusHandle,
	window: &Window,
	cx: &mut Context<Shell>,
) -> AnyElement {
	let title = match selected {
		Destination::QuickTasks => div()
			.id("quick-tasks-heading")
			.role(Role::Heading)
			.aria_level(1)
			.aria_label("Quick Tasks workspace")
			.child(selected.label())
			.into_any_element(),
		Destination::Health => div()
			.id("health-heading")
			.role(Role::Heading)
			.aria_level(1)
			.aria_label("Health workspace")
			.child(selected.label())
			.into_any_element(),
		_ => div().child(selected.label()).into_any_element(),
	};
	let header = div()
		.h(px(HEADER_HEIGHT))
		.min_h(px(HEADER_HEIGHT))
		.px_6()
		.flex()
		.items_center()
		.justify_between()
		.border_b_1()
		.border_color(rgb(0x263249))
		.text_xl()
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
		.p_6()
		.flex()
		.flex_col()
		.gap_3()
		.child(
			div()
				.id("destination-heading")
				.role(Role::Heading)
				.aria_level(1)
				.aria_label(format!("{} workspace", selected.label()))
				.text_2xl()
				.child(selected.label()),
		)
		.child(selected.description())
		.child(
			"This operational destination is intentionally bounded until its owning feature gate lands.",
		)
		.into_any_element()
}

fn quick_task_state_label(state: QuickTaskState) -> &'static str {
	match state {
		QuickTaskState::RoutingPending => "Routing pending",
		QuickTaskState::QuotaExhausted => "Quota exhausted",
		QuickTaskState::WaitingReconciliation => "Waiting for reconciliation",
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
		| QuickTaskState::QuotaExhausted
		| QuickTaskState::WaitingReconciliation
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
		QuickTaskCommandState::OutcomeUnknown =>
			Some("Outcome unknown. Readback will resume after reconnect; do not resend."),
		QuickTaskCommandState::Refused => Some("The command was refused."),
	}
}

fn recovery_action_label(action: QuickTaskRecoveryAction) -> &'static str {
	match action {
		QuickTaskRecoveryAction::RetryRouting =>
			"Review the message and retry account routing explicitly.",
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
		QuickTaskRecoveryAction::UpgradeCodex => "Install the accepted Codex build.",
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
		QuickTasksLoadState::Ready => "Quick Tasks are current.",
		QuickTasksLoadState::Offline => "Offline. Retained conversation state remains visible.",
		QuickTasksLoadState::Unavailable => "Quick Task state is temporarily unavailable.",
		QuickTasksLoadState::Refused => "Quick Task readback was refused.",
	}
}

fn quick_task_sidebar(snapshot: &QuickTasksSnapshot, cx: &mut Context<Shell>) -> AnyElement {
	let selected = snapshot.selected.clone();
	let rows = snapshot.tasks.iter().enumerate().map(|(index, task)| {
		let conversation_id = task.conversation_id.clone();
		let short_id = task.conversation_id.as_str().chars().take(8).collect::<String>();
		let is_selected = selected.as_ref() == Some(&task.conversation_id);
		let state = task.state;
		div()
			.id(("quick-task-row", index))
			.role(Role::ListItem)
			.aria_label(format!("Conversation {short_id}, {}", quick_task_state_label(state)))
			.h(px(54.0))
			.min_h(px(54.0))
			.px_3()
			.flex()
			.flex_col()
			.justify_center()
			.gap_1()
			.border_b_1()
			.border_color(rgb(0x263249))
			.bg(if is_selected { rgb(0x1b263b) } else { rgb(0x101827) })
			.hover(|element| element.bg(rgb(0x202d44)))
			.cursor_pointer()
			.on_click(cx.listener(move |shell, _, window, cx| {
				shell.choose_quick_task(conversation_id.clone(), window, cx);
			}))
			.child(div().text_sm().child(format!("Conversation {short_id}")))
			.child(
				div()
					.flex()
					.items_center()
					.gap_2()
					.text_xs()
					.text_color(rgb(0x94a3b8))
					.child(
						div().size(px(7.0)).rounded_full().bg(rgb(quick_task_state_color(state))),
					)
					.child(quick_task_state_label(state)),
			)
	});

	div()
		.w(px(TASK_LIST_WIDTH))
		.min_w(px(TASK_LIST_WIDTH))
		.h_full()
		.flex()
		.flex_col()
		.border_r_1()
		.border_color(rgb(0x263249))
		.bg(rgb(0x101827))
		.child(
			div()
				.h(px(48.0))
				.min_h(px(48.0))
				.px_3()
				.flex()
				.items_center()
				.justify_between()
				.border_b_1()
				.border_color(rgb(0x263249))
				.child(div().text_sm().child("Conversations"))
				.child(
					div()
						.id("new-quick-task")
						.role(Role::Button)
						.aria_label("New conversation")
						.tooltip(|_, cx| cx.new(|_| ControlTooltip("New conversation")).into())
						.size(px(30.0))
						.flex()
						.items_center()
						.justify_center()
						.rounded_sm()
						.hover(|element| element.bg(rgb(0x25324a)))
						.cursor_pointer()
						.on_click(cx.listener(|shell, _, window, cx| {
							shell.start_new_quick_task(window, cx);
						}))
						.text_xl()
						.child("+"),
				),
		)
		.child(
			div()
				.id("quick-task-list")
				.role(Role::List)
				.aria_label("Quick Task conversations")
				.flex_1()
				.min_h_0()
				.overflow_y_scroll()
				.children(rows),
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
			div()
				.w_full()
				.py_3()
				.border_b_1()
				.border_color(rgb(0x202b3f))
				.child(
					div()
						.text_xs()
						.text_color(rgb(0x94a3b8))
						.child(history_role_label(item.turn_role)),
				)
				.child(div().mt_1().whitespace_normal().child(text))
		});
	let live_rows = snapshot
		.live_deltas
		.iter()
		.filter(|delta| {
			selected == Some(&delta.conversation_id)
				&& !persisted_inline_ids.contains(&&delta.history_item_id)
		})
		.map(|delta| {
			div()
				.w_full()
				.py_3()
				.border_b_1()
				.border_color(rgb(0x202b3f))
				.child(div().text_xs().text_color(rgb(0x60a5fa)).child("Codex"))
				.child(div().mt_1().whitespace_normal().child(delta.text.as_str().to_owned()))
		});
	let history_status =
		history.map_or("Conversation history is not connected.", |history| match history.load {
			HistoryLoadState::Inactive => "Select a conversation or start a new conversation.",
			HistoryLoadState::InitialLoading | HistoryLoadState::RefreshingVisible =>
				"Loading conversation history",
			HistoryLoadState::PrefetchingAdjacent | HistoryLoadState::Visible => "",
			HistoryLoadState::RetryableUnavailable(_) =>
				"History is temporarily unavailable. Reconnect or retry.",
			HistoryLoadState::ClosedUnavailable(_) => "History readback was refused.",
		});

	div()
		.id("quick-task-transcript")
		.role(Role::Log)
		.aria_label("Quick Task conversation")
		.flex_1()
		.min_h_0()
		.overflow_y_scroll()
		.px_6()
		.py_3()
		.when(!history_status.is_empty(), |element| {
			element.child(div().py_3().text_sm().text_color(rgb(0x94a3b8)).child(history_status))
		})
		.children(history_rows)
		.children(live_rows)
		.into_any_element()
}

fn history_page_controls(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
	let can_previous = shell.history.as_ref().is_some_and(|history| history.can_show_previous);
	let can_next = shell.history.as_ref().is_some_and(|history| history.can_show_next);
	let can_retry = shell.history.as_ref().is_some_and(|history| history.can_retry);
	let previous = div()
		.id("quick-task-history-previous")
		.role(Role::Button)
		.aria_label("Show less conversation history")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Show less history")).into())
		.size(px(28.0))
		.flex()
		.items_center()
		.justify_center()
		.rounded_sm()
		.text_color(if can_previous { rgb(0xe5e7eb) } else { rgb(0x64748b) })
		.when(can_previous, |element| {
			element.cursor_pointer().hover(|element| element.bg(rgb(0x25324a))).on_click(
				cx.listener(|shell, _, window, cx| {
					shell.show_previous_history(window, cx);
				}),
			)
		})
		.child("<");
	let retry = div()
		.id("quick-task-history-retry")
		.role(Role::Button)
		.aria_label("Retry conversation history")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Retry conversation history")).into())
		.size(px(28.0))
		.flex()
		.items_center()
		.justify_center()
		.rounded_sm()
		.text_color(if can_retry { rgb(0xe5e7eb) } else { rgb(0x64748b) })
		.when(can_retry, |element| {
			element.cursor_pointer().hover(|element| element.bg(rgb(0x25324a))).on_click(
				cx.listener(|shell, _, window, cx| {
					shell.retry_history(window, cx);
				}),
			)
		})
		.child("↻");
	let next = div()
		.id("quick-task-history-next")
		.role(Role::Button)
		.aria_label("Load more conversation history")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Load more history")).into())
		.size(px(28.0))
		.flex()
		.items_center()
		.justify_center()
		.rounded_sm()
		.text_color(if can_next { rgb(0xe5e7eb) } else { rgb(0x64748b) })
		.when(can_next, |element| {
			element.cursor_pointer().hover(|element| element.bg(rgb(0x25324a))).on_click(
				cx.listener(|shell, _, window, cx| {
					shell.show_next_history(window, cx);
				}),
			)
		})
		.child(">");

	div()
		.w(px(92.0))
		.min_w(px(92.0))
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
	let can_continue = shell.creating_new
		|| task.is_none()
		|| task.is_some_and(|task| {
			task.state == QuickTaskState::Ready
				|| task.recovery_action == Some(QuickTaskRecoveryAction::RetryRouting)
		});
	let composer = shell.composer.read(cx);
	let composer_len = composer.len();
	let has_message = !composer.content().trim().is_empty();
	let can_send = shell.quick.can_submit && can_continue && has_message;
	let can_interrupt =
		shell.quick.can_submit && task.is_some_and(|task| task.state == QuickTaskState::Running);

	let send = div()
		.id("quick-task-send")
		.role(Role::Button)
		.aria_label("Send message")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Send message")).into())
		.h(px(34.0))
		.min_h(px(34.0))
		.px_4()
		.flex()
		.items_center()
		.justify_center()
		.rounded_sm()
		.bg(if can_send { rgb(0x2563eb) } else { rgb(0x25324a) })
		.text_color(if can_send { rgb(0xffffff) } else { rgb(0x64748b) })
		.when(can_send, |element| {
			element.cursor_pointer().hover(|element| element.bg(rgb(0x1d4ed8))).on_click(
				cx.listener(|shell, _, window, cx| {
					shell.submit_quick_task(window, cx);
				}),
			)
		})
		.child("Send");
	let interrupt = div()
		.id("quick-task-interrupt")
		.role(Role::Button)
		.aria_label("Interrupt active turn")
		.tooltip(|_, cx| cx.new(|_| ControlTooltip("Interrupt active turn")).into())
		.h(px(34.0))
		.min_h(px(34.0))
		.px_3()
		.flex()
		.items_center()
		.justify_center()
		.rounded_sm()
		.border_1()
		.border_color(rgb(0x3b4962))
		.text_color(if can_interrupt { rgb(0xf8fafc) } else { rgb(0x64748b) })
		.when(can_interrupt, |element| {
			element.cursor_pointer().hover(|element| element.bg(rgb(0x25324a))).on_click(
				cx.listener(|shell, _, window, cx| {
					shell.interrupt_quick_task(window, cx);
				}),
			)
		})
		.child("Stop");
	div()
		.min_h(px(142.0))
		.px_4()
		.py_3()
		.flex()
		.flex_col()
		.gap_2()
		.border_t_1()
		.border_color(rgb(0x263249))
		.bg(rgb(0x0e1524))
		.child(div().h(px(64.0)).min_h(px(64.0)).child(shell.composer.clone()))
		.child(
			div()
				.h(px(34.0))
				.min_h(px(34.0))
				.flex()
				.items_center()
				.justify_between()
				.child(
					div()
						.min_w_0()
						.text_xs()
						.text_color(rgb(0x94a3b8))
						.child(format!("{composer_len} / {MAX_COMPOSER_BYTES} bytes")),
				)
				.child(div().flex().gap_2().child(interrupt).child(send)),
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
	let state_color = selected_task.map_or(0x60a5fa, |task| quick_task_state_color(task.state));
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
		.min_h_0()
		.flex()
		.child(quick_task_sidebar(&shell.quick, cx))
		.child(
			div()
				.flex_1()
				.min_w_0()
				.min_h_0()
				.flex()
				.flex_col()
				.child(
					div()
						.h(px(44.0))
						.min_h(px(44.0))
						.px_6()
						.flex()
						.items_center()
						.gap_3()
						.border_b_1()
						.border_color(rgb(0x263249))
						.child(div().size(px(8.0)).rounded_full().bg(rgb(state_color)))
						.child(div().w(px(132.0)).min_w(px(132.0)).text_sm().child(state_label))
						.child(
							div()
								.flex_1()
								.min_w_0()
								.overflow_hidden()
								.whitespace_nowrap()
								.text_ellipsis()
								.text_sm()
								.text_color(rgb(0x94a3b8))
								.child(detail),
						)
						.child(history_page_controls(shell, cx)),
				)
				.child(quick_task_transcript(&shell.quick, shell.history.as_ref()))
				.child(quick_task_composer(shell, cx)),
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
		.px_6()
		.py_4()
		.child(
			div()
				.id("health-query-status")
				.role(Role::Status)
				.aria_label(format!("Health report: {}", presentation.label))
				.h(px(52.0))
				.min_h(px(52.0))
				.flex()
				.items_center()
				.gap_3()
				.child(
					div().size(px(10.0)).min_w(px(10.0)).rounded_full().bg(rgb(presentation.color)),
				)
				.child(div().w(px(144.0)).min_w(px(144.0)).child(presentation.label))
				.child(div().min_w_0().text_color(rgb(0xa8b3c7)).child(presentation.detail)),
		)
		.child(
			div()
				.id("health-components")
				.role(Role::List)
				.aria_label("Health components")
				.border_t_1()
				.border_color(rgb(0x263249))
				.children(rows),
		)
		.into_any_element()
}

fn connection_status(presentation: ConnectionPresentation) -> AnyElement {
	div()
		.id("connection-status")
		.role(Role::Status)
		.aria_label(format!("Connection: {}", presentation.label))
		.h(px(STATUS_HEIGHT))
		.min_h(px(STATUS_HEIGHT))
		.px_6()
		.flex()
		.items_center()
		.gap_3()
		.border_t_1()
		.border_color(rgb(0x263249))
		.child(div().w(px(10.0)).h(px(10.0)).rounded_full().bg(rgb(presentation.color)))
		.child(div().w(px(184.0)).min_w(px(184.0)).child(presentation.label))
		.child(div().min_w_0().child(presentation.detail))
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
	let content = match selected {
		Destination::QuickTasks => quick_tasks_content(shell, cx),
		Destination::Health => health_content(&shell.health),
		_ => placeholder_content(selected),
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
		.child(destination_header(selected, &shell.health, refresh_focus, window, cx))
		.child(content)
		.child(connection_status(presentation))
		.into_any_element()
}

impl Render for Shell {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let presentation = connection_presentation(self.connection);
		let selected = self.selected;
		let navigation = Destination::ALL.into_iter().enumerate().map(|(index, destination)| {
			navigation_item(
				index,
				destination,
				selected == destination,
				self.destination_focus[index].clone(),
				window,
				cx,
			)
		});

		div()
			.id("decodex-shell")
			.role(Role::Application)
			.aria_label("Decodex operational shell")
			.track_focus(&self.root_focus)
			.on_action(cx.listener(Self::focus_next))
			.on_action(cx.listener(Self::focus_previous))
			.on_action(cx.listener(Self::submit_composer))
			.size_full()
			.min_w(px(1024.0))
			.min_h(px(620.0))
			.flex()
			.bg(rgb(0x0b1020))
			.text_color(rgb(0xe5e7eb))
			.child(
				div()
					.id("primary-navigation")
					.role(Role::Navigation)
					.aria_label("Primary navigation")
					.w(px(SIDEBAR_WIDTH))
					.min_w(px(SIDEBAR_WIDTH))
					.h_full()
					.p_3()
					.flex()
					.flex_col()
					.gap_2()
					.border_r_1()
					.border_color(rgb(0x263249))
					.bg(rgb(0x121a2a))
					.child(
						div().h(px(HEADER_HEIGHT)).flex().items_center().text_xl().child("Decodex"),
					)
					.children(navigation),
			)
			.child(destination_content(self, presentation, self.refresh_focus.clone(), window, cx))
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
				matches!(destination, Destination::QuickTasks | Destination::Health),
			)),
			[
				("Advisor", false),
				("Projects", false),
				("Quick Tasks", true),
				("Runs", false),
				("Automations", false),
				("Accounts", false),
				("Health", true),
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
	fn keyboard_focus_and_activation_cover_all_destinations(cx: &mut TestAppContext) {
		let (shell, visual) = open_shell(cx);
		visual.simulate_keystrokes("tab");
		for expected in Destination::ALL {
			let focused = shell.read_with(visual, |shell, _| {
				let index = Destination::ALL
					.iter()
					.position(|value| *value == expected)
					.expect("test operation must succeed");
				shell.destination_focus[index].clone()
			});
			assert!(visual.update(|window, _| focused.is_focused(window)));
			visual.simulate_keystrokes("enter");
			assert_eq!(shell.read_with(visual, |shell, _| shell.selected), expected);
			visual.simulate_keystrokes("tab");
		}

		visual.simulate_keystrokes("shift-tab");
		let system_focus = shell.read_with(visual, |shell, _| shell.destination_focus[6].clone());
		assert!(visual.update(|window, _| system_focus.is_focused(window)));
	}

	#[gpui::test]
	fn compact_and_normal_sizes_preserve_fixed_core_dimensions(cx: &mut TestAppContext) {
		let (_shell, visual) = open_shell(cx);
		for (width, height) in [(760.0, 520.0), (1180.0, 760.0)] {
			visual.update(|window, cx| {
				window.resize(size(px(width), px(height)));
				window.draw(cx).clear();
				assert_eq!(SIDEBAR_WIDTH, 224.0);
				assert_eq!(HEADER_HEIGHT, 64.0);
				assert_eq!(STATUS_HEIGHT, 72.0);
			});
		}
	}
}
