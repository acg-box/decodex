//! Production GPUI window, navigation, focus, and lifecycle rendering boundary.

use std::{future::Future, pin::Pin, sync::mpsc::Receiver, time::Duration};

use gpui::{
	AnyElement, App, Context, Entity, FocusHandle, Global, KeyBinding, Render, Role, SharedString,
	Subscription, Task, WeakEntity, Window, WindowHandle, WindowId, actions, div, prelude::*, px,
	rgb,
};

use decodex_protocol::ClientFailure;

use crate::client_lifecycle::{
	ClientLifecycle, CompatibilityReason, ConnectionView, LifecycleCancellation, QuarantineReason,
	QuarantineRecovery,
};

const SIDEBAR_WIDTH: f32 = 224.0;
const HEADER_HEIGHT: f32 = 64.0;
const STATUS_HEIGHT: f32 = 72.0;
const LIFECYCLE_POLL: Duration = Duration::from_millis(40);

actions!(decodex_shell, [FocusNext, FocusPrevious, ActivateDestination]);

/// Stable shell destinations. Product behavior belongs to later issues.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Destination {
	Advisor,
	Projects,
	QuickTasks,
	Runs,
	Automations,
	Accounts,
	System,
}

impl Destination {
	pub(crate) const ALL: [Self; 7] = [
		Self::Advisor,
		Self::Projects,
		Self::QuickTasks,
		Self::Runs,
		Self::Automations,
		Self::Accounts,
		Self::System,
	];

	pub(crate) const fn label(self) -> &'static str {
		match self {
			Self::Advisor => "Advisor",
			Self::Projects => "Projects",
			Self::QuickTasks => "Quick Tasks",
			Self::Runs => "Runs",
			Self::Automations => "Automations",
			Self::Accounts => "Accounts",
			Self::System => "System",
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
			Self::System => "System health and diagnostics will appear here.",
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
	cx.bind_keys([
		KeyBinding::new("tab", FocusNext, None),
		KeyBinding::new("shift-tab", FocusPrevious, None),
		KeyBinding::new("enter", ActivateDestination, None),
		KeyBinding::new("space", ActivateDestination, None),
	]);
}

/// One window-owned production shell. Connection ownership lives at application scope.
pub(crate) struct Shell {
	selected: Destination,
	connection: ConnectionView,
	root_focus: FocusHandle,
	destination_focus: Vec<FocusHandle>,
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
		window.focus(&destination_focus[0], cx);

		Self {
			selected: Destination::Advisor,
			connection,
			root_focus: cx.focus_handle(),
			destination_focus,
		}
	}

	fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
		window.focus_next(cx);
	}

	fn focus_previous(&mut self, _: &FocusPrevious, window: &mut Window, cx: &mut Context<Self>) {
		window.focus_prev(cx);
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
			self.selected = Destination::ALL[index];
			cx.notify();
		}
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
	let background = cx.background_executor().spawn(async move {
		let runtime = tokio::runtime::Builder::new_current_thread()
			.enable_all()
			.build()
			.expect("build the bounded client runtime");

		runtime.block_on(lifecycle.run())
	});
	let shell = window.entity(cx).expect("the production shell window remains open").downgrade();
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
}

fn navigation_item(
	index: usize,
	destination: Destination,
	is_selected: bool,
	focus: FocusHandle,
	window: &Window,
	cx: &mut Context<Shell>,
) -> AnyElement {
	div()
		.id(("destination", index))
		.role(Role::Tab)
		.aria_label(destination.label())
		.aria_selected(is_selected)
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
			shell.selected = destination;
			cx.notify();
		}))
		.child(destination.label())
		.into_any_element()
}

fn destination_content(
	selected: Destination,
	presentation: ConnectionPresentation,
) -> impl IntoElement {
	div()
		.id("destination-content")
		.role(Role::Main)
		.aria_label(format!("{} destination", selected.label()))
		.flex_1()
		.min_w_0()
		.h_full()
		.flex()
		.flex_col()
		.child(
			div()
				.h(px(HEADER_HEIGHT))
				.min_h(px(HEADER_HEIGHT))
				.px_6()
				.flex()
				.items_center()
				.border_b_1()
				.border_color(rgb(0x263249))
				.text_xl()
				.child(selected.label()),
		)
		.child(
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
				),
		)
		.child(
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
				.child(div().min_w_0().child(presentation.detail)),
		)
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
			.size_full()
			.min_w(px(760.0))
			.min_h(px(520.0))
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
			.child(destination_content(selected, presentation))
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
	fn destinations_are_exact_stable_placeholders() {
		assert_eq!(
			Destination::ALL.map(Destination::label),
			["Advisor", "Projects", "Quick Tasks", "Runs", "Automations", "Accounts", "System"]
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
