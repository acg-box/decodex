//! Deterministic lifecycle tests over the one private session/time seam.

use std::{
	collections::VecDeque,
	fs,
	os::unix::fs::{MetadataExt as _, PermissionsExt as _},
	path::{Path, PathBuf},
	sync::{Arc, Mutex},
	time::Duration,
};

use tempfile::TempDir;

use decodex_protocol::{
	CURRENT_VERSION, Channel, CorrelationId, Cursor, EntityId, EntityRevision, EventEnvelope,
	EventPayload, RetainedSessionFailure, ServerId, ServerInstanceId, SessionCheckpoint,
	SnapshotEnvelope, SnapshotItem, WireText,
};

use crate::client_lifecycle::{
	AppliedEntity, CacheLimits, ClientCache, ClientLifecycle, CompatibilityReason, ConnectionView,
	Delivery, LifecycleCancellation, LifecycleIo, QuarantineReason, QuarantineRecovery, RunResult,
};

const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
const OTHER_SERVER: &str = "028f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
const INSTANCE: &str = "publication-a";

#[derive(Clone, Debug)]
struct FakeConfirmation {
	cursor: Cursor,
	cache_root: PathBuf,
	previous_current: Option<Vec<u8>>,
}

enum SessionAction {
	Snapshot(SnapshotEnvelope),
	Event(EventEnvelope),
	Fail(RetainedSessionFailure),
	AwaitCancellation,
	Cancel,
}

struct FakeSession {
	actions: VecDeque<SessionAction>,
	checkpoint: Option<SessionCheckpoint>,
	cancellation: LifecycleCancellation,
	closed: Arc<Mutex<usize>>,
	confirmations: Arc<Mutex<Vec<Cursor>>>,
	cache_root: PathBuf,
	server_id: ServerId,
	instance_id: ServerInstanceId,
}

impl FakeSession {
	async fn next(&mut self) -> Result<Delivery<FakeConfirmation>, RetainedSessionFailure> {
		match self.actions.pop_front().expect("fake session action is scripted") {
			SessionAction::Snapshot(snapshot) => {
				let confirmation = FakeConfirmation {
					cursor: snapshot.cursor,
					cache_root: self.cache_root.clone(),
					previous_current: None,
				};

				Ok(Delivery::Snapshot { snapshot, confirmation })
			},
			SessionAction::Event(event) => {
				let confirmation = FakeConfirmation {
					cursor: event.cursor,
					previous_current: fs::read(self.cache_root.join("current")).ok(),
					cache_root: self.cache_root.clone(),
				};

				Ok(Delivery::Event { event, confirmation })
			},
			SessionAction::Fail(failure) => Err(failure),
			SessionAction::AwaitCancellation => {
				self.cancellation.cancelled().await;

				Err(RetainedSessionFailure::Cancelled)
			},
			SessionAction::Cancel => {
				self.cancellation.cancel();

				Err(RetainedSessionFailure::Cancelled)
			},
		}
	}

	fn confirm_applied(
		&mut self,
		confirmation: FakeConfirmation,
	) -> Result<SessionCheckpoint, RetainedSessionFailure> {
		assert!(
			confirmation.cache_root.join("current").is_file(),
			"cache publication must complete before confirmation"
		);
		if let Some(previous) = confirmation.previous_current {
			assert_ne!(
				fs::read(confirmation.cache_root.join("current"))
					.expect("event generation pointer is readable"),
				previous,
				"event generation must publish before confirmation"
			);
		}
		self.confirmations.lock().expect("confirmation log is available").push(confirmation.cursor);
		let checkpoint = SessionCheckpoint::new(
			self.server_id.clone(),
			self.instance_id.clone(),
			confirmation.cursor,
		);

		self.checkpoint = Some(checkpoint.clone());

		Ok(checkpoint)
	}

	async fn close(self) -> Result<(), RetainedSessionFailure> {
		*self.closed.lock().expect("close count is available") += 1;

		Ok(())
	}
}

#[gpui::test]
fn production_owner_closes_connected_session_and_reaches_terminal_shutdown(
	cx: &mut gpui::TestAppContext,
) {
	use crate::shell::{Shell, retain_lifecycle_task};

	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);
	let cancellation = lifecycle.cancellation();
	let views = lifecycle.observe_views();
	let initial_view = lifecycle.view();
	let io = FakeIo::new(root, vec![connected(vec![SessionAction::AwaitCancellation], None)]);
	let closed = io.closed.clone();
	let (shell, visual) = cx.add_window_view(|window, cx| {
		Shell::new(window, cx, ConnectionView::Connecting { attempt: 1 })
	});
	let window_id = visual.update(|window, _| window.window_handle().window_id());
	let background = visual.spawn(|_| async move {
		let mut io = io;

		lifecycle.run_with_io(&mut io).await
	});
	let owner = visual.update(|_, cx| {
		retain_lifecycle_task(
			window_id,
			shell.downgrade(),
			cancellation,
			views,
			initial_view,
			background,
			cx,
		)
	});

	visual.run_until_parked();
	assert!(owner.read_with(visual, |owner, _| owner.is_running()));
	visual.update(|window, _| window.remove_window());
	for _ in 0..4 {
		visual.executor().advance_clock(Duration::from_millis(40));
		visual.run_until_parked();
	}

	assert_eq!(*closed.lock().expect("close count is available"), 1);
	assert_eq!(owner.read_with(visual, |owner, _| owner.last_view()), ConnectionView::Stopped);
	assert!(owner.read_with(visual, |owner, _| {
		owner
			.observed_views()
			.windows(2)
			.any(|views| views == [ConnectionView::ShuttingDown, ConnectionView::Stopped])
	}));
	assert!(!owner.read_with(visual, |owner, _| owner.is_running()));
}

enum ConnectAction {
	Session {
		actions: VecDeque<SessionAction>,
		checkpoint: Option<SessionCheckpoint>,
		server_id: &'static str,
		instance_id: &'static str,
	},
	Fail(RetainedSessionFailure),
	Cancel,
}

struct FakeIo {
	connections: VecDeque<ConnectAction>,
	delays: Vec<Duration>,
	attempts: usize,
	active: usize,
	maximum_active: usize,
	closed: Arc<Mutex<usize>>,
	confirmations: Arc<Mutex<Vec<Cursor>>>,
	cache_root: PathBuf,
	cancel_backoff: bool,
	session: Option<FakeSession>,
	requested_checkpoints: Vec<Option<SessionCheckpoint>>,
}

impl FakeIo {
	fn new(cache_root: PathBuf, connections: Vec<ConnectAction>) -> Self {
		Self {
			connections: connections.into(),
			delays: Vec::new(),
			attempts: 0,
			active: 0,
			maximum_active: 0,
			closed: Arc::new(Mutex::new(0)),
			confirmations: Arc::new(Mutex::new(Vec::new())),
			cache_root,
			cancel_backoff: false,
			session: None,
			requested_checkpoints: Vec::new(),
		}
	}
}

impl LifecycleIo for FakeIo {
	type Confirmation = FakeConfirmation;

	async fn connect(
		&mut self,
		_config: &decodex_protocol::RetainedSessionConfig,
		checkpoint: Option<SessionCheckpoint>,
		cancellation: &LifecycleCancellation,
	) -> Result<Option<SessionCheckpoint>, RetainedSessionFailure> {
		assert!(self.session.is_none(), "one caller-owned session must close before replacement");
		self.attempts += 1;
		self.active += 1;
		self.maximum_active = self.maximum_active.max(self.active);
		self.requested_checkpoints.push(checkpoint);
		let action = self.connections.pop_front().expect("fake connection is scripted");
		self.active -= 1;

		match action {
			ConnectAction::Session { actions, checkpoint, server_id, instance_id } => {
				let initial_checkpoint = checkpoint.clone();

				self.session = Some(FakeSession {
					actions,
					checkpoint,
					cancellation: cancellation.clone(),
					closed: self.closed.clone(),
					confirmations: self.confirmations.clone(),
					cache_root: self.cache_root.clone(),
					server_id: server(server_id),
					instance_id: ServerInstanceId::new(instance_id)
						.expect("instance identity is bounded"),
				});

				Ok(initial_checkpoint)
			},
			ConnectAction::Fail(failure) => Err(failure),
			ConnectAction::Cancel => {
				cancellation.cancel();

				Err(RetainedSessionFailure::Cancelled)
			},
		}
	}

	async fn next(&mut self) -> Result<Delivery<Self::Confirmation>, RetainedSessionFailure> {
		self.session.as_mut().expect("fake session is connected").next().await
	}

	fn confirm_applied(
		&mut self,
		confirmation: Self::Confirmation,
	) -> Result<SessionCheckpoint, RetainedSessionFailure> {
		self.session.as_mut().expect("fake session is connected").confirm_applied(confirmation)
	}

	async fn close(&mut self) -> Result<(), RetainedSessionFailure> {
		let Some(session) = self.session.take() else {
			return Ok(());
		};

		session.close().await
	}

	async fn backoff(
		&mut self,
		delay: Duration,
		cancellation: &LifecycleCancellation,
	) -> Result<(), RetainedSessionFailure> {
		self.delays.push(delay);
		if self.cancel_backoff {
			cancellation.cancel();

			return Err(RetainedSessionFailure::Cancelled);
		}

		Ok(())
	}
}

fn connected(actions: Vec<SessionAction>, checkpoint: Option<SessionCheckpoint>) -> ConnectAction {
	ConnectAction::Session {
		actions: actions.into(),
		checkpoint,
		server_id: SERVER,
		instance_id: INSTANCE,
	}
}

fn server(value: &str) -> ServerId {
	ServerId::new(value).expect("server identity is bounded")
}

fn checkpoint(instance: &str, cursor: u64) -> SessionCheckpoint {
	SessionCheckpoint::new(
		server(SERVER),
		ServerInstanceId::new(instance).expect("instance identity is bounded"),
		Cursor(cursor),
	)
}

fn entity(value: &str) -> EntityId {
	EntityId::new(value).expect("entity identity is bounded")
}

fn snapshot(cursor: u64, entity_id: &str, revision: u64) -> SnapshotEnvelope {
	snapshot_for(SERVER, cursor, entity_id, revision)
}

fn snapshot_for(server_id: &str, cursor: u64, entity_id: &str, revision: u64) -> SnapshotEnvelope {
	SnapshotEnvelope {
		version: CURRENT_VERSION,
		server_id: server(server_id),
		cursor: Cursor(cursor),
		items: vec![SnapshotItem::SystemState {
			entity_id: entity(entity_id),
			revision: EntityRevision(revision),
			status: WireText::new("ready").expect("status is bounded"),
		}],
	}
}

fn event(cursor: u64, entity_id: &str, revision: u64) -> EventEnvelope {
	EventEnvelope {
		version: CURRENT_VERSION,
		server_id: server(SERVER),
		cursor: Cursor(cursor),
		channel: Channel::SystemHealth,
		entity_id: entity(entity_id),
		entity_revision: EntityRevision(revision),
		correlation_id: CorrelationId::new("correlation").expect("correlation is bounded"),
		causation_id: None,
		payload: EventPayload::SystemObservationRefreshed {
			status: WireText::new("updated").expect("status is bounded"),
		},
	}
}

fn cache_root(temporary: &TempDir) -> PathBuf {
	temporary.path().canonicalize().expect("temporary root canonicalizes").join("cache")
}

fn lifecycle(root: &Path) -> ClientLifecycle {
	lifecycle_for(root, SERVER, 1)
}

fn lifecycle_for(root: &Path, server_id: &str, schema_generation: u64) -> ClientLifecycle {
	let transport_root = root.parent().expect("cache fixture has a parent").join("transport");

	fs::create_dir_all(&transport_root).expect("create local transport fixture root");
	fs::set_permissions(&transport_root, fs::Permissions::from_mode(0o700))
		.expect("scope transport fixture root");

	let service_owner_uid =
		fs::metadata(&transport_root).expect("read transport root metadata").uid();
	let config_file = transport_root.join("config.toml");
	let config = format!(
		r#"version = 1
active_profile = "selected"
server_host = {{}}
postgres = {{}}
cache = {{}}

[profiles.selected]
kind = "local"
policy = "same_uid"
service_owner_uid = {service_owner_uid}
expected_server_identity = "{server_id}"
"#
	);

	fs::write(&config_file, config).expect("write transport fixture config");
	fs::set_permissions(&config_file, fs::Permissions::from_mode(0o600))
		.expect("scope transport fixture config");

	let config = decodex_protocol::ClientProfile::load(&transport_root, None)
		.expect("load local fixture profile")
		.retained_session_config()
		.expect("fixture retained session config is valid");

	ClientLifecycle::new(
		config,
		root,
		CacheLimits::new(2 * 1_024 * 1_024, 32, 16).expect("limits are valid"),
		schema_generation,
	)
	.expect("lifecycle constructs")
}

#[tokio::test]
async fn retry_progression_is_capped_and_uses_only_fake_time() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);
	let failures =
		(0..5).map(|_| ConnectAction::Fail(RetainedSessionFailure::Disconnected)).collect();
	let mut io = FakeIo::new(root, failures);

	assert_eq!(lifecycle.run_with_io(&mut io).await, RunResult::RetryExhausted);
	assert_eq!(io.attempts, 5);
	assert_eq!(io.maximum_active, 1);
	assert_eq!(
		io.delays,
		vec![
			Duration::from_millis(100),
			Duration::from_millis(250),
			Duration::from_millis(500),
			Duration::from_secs(1),
		]
	);
	assert_eq!(lifecycle.view(), ConnectionView::Stopped);
}

#[tokio::test]
async fn observer_receives_the_complete_bounded_retry_progression() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);
	let views = lifecycle.observe_views();
	let failures =
		(0..5).map(|_| ConnectAction::Fail(RetainedSessionFailure::Disconnected)).collect();
	let mut io = FakeIo::new(root, failures);

	assert_eq!(lifecycle.run_with_io(&mut io).await, RunResult::RetryExhausted);
	assert_eq!(
		views.try_iter().collect::<Vec<_>>(),
		vec![
			ConnectionView::Stopped,
			ConnectionView::Connecting { attempt: 1 },
			ConnectionView::OfflineRetrying { next_attempt: 2, delay: Duration::from_millis(100) },
			ConnectionView::Connecting { attempt: 2 },
			ConnectionView::OfflineRetrying { next_attempt: 3, delay: Duration::from_millis(250) },
			ConnectionView::Connecting { attempt: 3 },
			ConnectionView::OfflineRetrying { next_attempt: 4, delay: Duration::from_millis(500) },
			ConnectionView::Connecting { attempt: 4 },
			ConnectionView::OfflineRetrying { next_attempt: 5, delay: Duration::from_secs(1) },
			ConnectionView::Connecting { attempt: 5 },
			ConnectionView::Stopped,
		]
	);
}

#[tokio::test]
async fn cancellation_is_terminal_during_connect_backoff_and_receive() {
	let connect_temporary = TempDir::new().expect("temporary directory is available");
	let connect_root = cache_root(&connect_temporary);
	let mut connect_lifecycle = lifecycle(&connect_root);
	let mut connect_io = FakeIo::new(connect_root, vec![ConnectAction::Cancel]);

	assert_eq!(connect_lifecycle.run_with_io(&mut connect_io).await, RunResult::Stopped);
	assert_eq!(connect_lifecycle.view(), ConnectionView::Stopped);

	let backoff_temporary = TempDir::new().expect("temporary directory is available");
	let backoff_root = cache_root(&backoff_temporary);
	let mut backoff_lifecycle = lifecycle(&backoff_root);
	let mut backoff_io =
		FakeIo::new(backoff_root, vec![ConnectAction::Fail(RetainedSessionFailure::Disconnected)]);
	backoff_io.cancel_backoff = true;

	assert_eq!(backoff_lifecycle.run_with_io(&mut backoff_io).await, RunResult::Stopped);

	let receive_temporary = TempDir::new().expect("temporary directory is available");
	let receive_root = cache_root(&receive_temporary);
	let mut receive_lifecycle = lifecycle(&receive_root);
	let mut receive_io =
		FakeIo::new(receive_root, vec![connected(vec![SessionAction::Cancel], None)]);

	assert_eq!(receive_lifecycle.run_with_io(&mut receive_io).await, RunResult::Stopped);
	assert_eq!(*receive_io.closed.lock().expect("close count is available"), 1);
}

#[tokio::test]
async fn cache_and_state_application_precede_checkpoint_confirmation() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);
	let mut io = FakeIo::new(
		root.clone(),
		vec![connected(
			vec![SessionAction::Snapshot(snapshot(7, "system", 3)), SessionAction::Cancel],
			None,
		)],
	);

	assert_eq!(lifecycle.run_with_io(&mut io).await, RunResult::Stopped);
	assert!(root.join("current").is_file());
	assert_eq!(lifecycle.last_cursor, Some(Cursor(7)));
	assert_eq!(lifecycle.state["system"].revision, EntityRevision(3));
	assert_eq!(*io.confirmations.lock().expect("confirmation log is available"), vec![Cursor(7)]);
	assert_eq!(
		lifecycle.binding.as_ref().expect("checkpoint is bound").checkpoint.cursor(),
		Cursor(7)
	);
}

#[tokio::test]
async fn event_publication_retains_the_complete_authoritative_state_before_confirmation() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);
	let snapshot = SnapshotEnvelope {
		version: CURRENT_VERSION,
		server_id: server(SERVER),
		cursor: Cursor(5),
		items: vec![
			SnapshotItem::SystemState {
				entity_id: entity("first"),
				revision: EntityRevision(1),
				status: WireText::new("first").expect("status is bounded"),
			},
			SnapshotItem::SystemState {
				entity_id: entity("second"),
				revision: EntityRevision(2),
				status: WireText::new("second").expect("status is bounded"),
			},
		],
	};
	let mut io = FakeIo::new(
		root,
		vec![connected(
			vec![
				SessionAction::Snapshot(snapshot),
				SessionAction::Event(event(6, "first", 3)),
				SessionAction::Cancel,
			],
			None,
		)],
	);

	assert_eq!(lifecycle.run_with_io(&mut io).await, RunResult::Stopped);
	assert_eq!(lifecycle.state.len(), 2);
	assert_eq!(lifecycle.state["first"].revision, EntityRevision(3));
	assert_eq!(lifecycle.state["second"].revision, EntityRevision(2));
	assert_eq!(
		lifecycle
			.cache
			.as_ref()
			.expect("test operation must succeed")
			.inspect_current()
			.expect("test operation must succeed")
			.expect("test operation must succeed")
			.records,
		2
	);
	assert_eq!(
		*io.confirmations.lock().expect("confirmation log is available"),
		vec![Cursor(5), Cursor(6)]
	);
}

#[tokio::test]
async fn resume_reuses_only_the_attested_checkpoint_and_fallback_rebuilds_state() {
	let resume_temporary = TempDir::new().expect("temporary directory is available");
	let resume_root = cache_root(&resume_temporary);
	let mut resumed = lifecycle(&resume_root);
	let mut resume_io = FakeIo::new(
		resume_root,
		vec![
			connected(
				vec![
					SessionAction::Snapshot(snapshot(7, "system", 3)),
					SessionAction::Fail(RetainedSessionFailure::Disconnected),
				],
				None,
			),
			connected(
				vec![SessionAction::Event(event(8, "system", 4)), SessionAction::Cancel],
				Some(checkpoint(INSTANCE, 7)),
			),
		],
	);

	assert_eq!(resumed.run_with_io(&mut resume_io).await, RunResult::Stopped);
	assert_eq!(resume_io.requested_checkpoints[0], None);
	assert_eq!(
		resume_io.requested_checkpoints[1].as_ref().map(SessionCheckpoint::cursor),
		Some(Cursor(7))
	);
	assert_eq!(resumed.last_cursor, Some(Cursor(8)));
	assert_eq!(resumed.state["system"].revision, EntityRevision(4));

	let fallback_temporary = TempDir::new().expect("temporary directory is available");
	let fallback_root = cache_root(&fallback_temporary);
	let mut fallback = lifecycle(&fallback_root);
	let mut fallback_io = FakeIo::new(
		fallback_root,
		vec![
			connected(
				vec![
					SessionAction::Snapshot(snapshot(3, "old", 1)),
					SessionAction::Fail(RetainedSessionFailure::Disconnected),
				],
				None,
			),
			ConnectAction::Session {
				actions: vec![
					SessionAction::Snapshot(snapshot(9, "replacement", 1)),
					SessionAction::Cancel,
				]
				.into(),
				checkpoint: None,
				server_id: SERVER,
				instance_id: "publication-b",
			},
		],
	);

	assert_eq!(fallback.run_with_io(&mut fallback_io).await, RunResult::Stopped);
	assert_eq!(
		fallback_io.requested_checkpoints[1].as_ref().map(SessionCheckpoint::cursor),
		Some(Cursor(3))
	);
	assert!(!fallback.state.contains_key("old"));
	assert_eq!(fallback.state["replacement"].revision, EntityRevision(1));
	assert_eq!(
		fallback
			.binding
			.as_ref()
			.expect("replacement checkpoint is bound")
			.checkpoint
			.instance_id(),
		&ServerInstanceId::new("publication-b").expect("instance identity is bounded")
	);
	assert_eq!(
		fallback
			.cache
			.as_ref()
			.expect("cache is available")
			.inspect_current()
			.expect("test operation must succeed")
			.expect("test operation must succeed")
			.records,
		1
	);
}

#[tokio::test]
async fn checkpoint_mismatch_quarantines_then_recovers_only_from_a_fresh_snapshot() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);
	let mut io = FakeIo::new(
		root,
		vec![
			connected(
				vec![
					SessionAction::Snapshot(snapshot(2, "old", 1)),
					SessionAction::Fail(RetainedSessionFailure::Disconnected),
				],
				None,
			),
			ConnectAction::Fail(RetainedSessionFailure::CheckpointIdentityMismatch),
			connected(
				vec![SessionAction::Snapshot(snapshot(7, "replacement", 1)), SessionAction::Cancel],
				None,
			),
		],
	);

	assert_eq!(lifecycle.run_with_io(&mut io).await, RunResult::Stopped);
	assert_eq!(
		io.requested_checkpoints[1].as_ref().map(SessionCheckpoint::cursor),
		Some(Cursor(2))
	);
	assert_eq!(io.requested_checkpoints[2], None);
	assert!(!lifecycle.state.contains_key("old"));
	assert!(lifecycle.state.contains_key("replacement"));
	assert!(lifecycle.quarantine.is_none());
}

#[tokio::test]
async fn snapshot_fallback_rejects_events_until_verified_rebuild_completes() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);
	let mut io = FakeIo::new(
		root,
		vec![
			connected(
				vec![
					SessionAction::Snapshot(snapshot(2, "old", 1)),
					SessionAction::Fail(RetainedSessionFailure::Disconnected),
				],
				None,
			),
			ConnectAction::Session {
				actions: vec![SessionAction::Event(event(3, "old", 2))].into(),
				checkpoint: None,
				server_id: SERVER,
				instance_id: "publication-b",
			},
			ConnectAction::Session {
				actions: vec![
					SessionAction::Snapshot(snapshot(5, "replacement", 1)),
					SessionAction::Cancel,
				]
				.into(),
				checkpoint: None,
				server_id: SERVER,
				instance_id: "publication-b",
			},
		],
	);

	assert_eq!(lifecycle.run_with_io(&mut io).await, RunResult::Stopped);
	assert_eq!(
		*io.confirmations.lock().expect("confirmation log is available"),
		vec![Cursor(2), Cursor(5)]
	);
	assert!(!lifecycle.state.contains_key("old"));
	assert!(lifecycle.state.contains_key("replacement"));
}

#[tokio::test]
async fn material_failure_exhaustion_preserves_quarantine_authority_state() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);
	let failures = (0..5)
		.map(|_| ConnectAction::Fail(RetainedSessionFailure::CheckpointIdentityMismatch))
		.collect();
	let mut io = FakeIo::new(root, failures);

	assert_eq!(lifecycle.run_with_io(&mut io).await, RunResult::Quarantined);
	assert_eq!(
		lifecycle.view(),
		ConnectionView::Quarantined {
			reason: QuarantineReason::CheckpointMismatch,
			recovery: QuarantineRecovery::VerifiedSnapshotReplacement,
		}
	);
}

#[test]
fn snapshot_fallback_stays_quarantined_until_checkpoint_binding() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);
	lifecycle.connection_generation = 1;
	lifecycle.connection_established(1, true, None);

	assert_eq!(
		lifecycle.view(),
		ConnectionView::Quarantined {
			reason: QuarantineReason::PublicationInstanceChanged,
			recovery: QuarantineRecovery::VerifiedSnapshotReplacement,
		}
	);

	let inspection = lifecycle
		.apply_snapshot(1, snapshot(8, "replacement", 1))
		.expect("verified replacement publishes");
	assert!(matches!(lifecycle.view(), ConnectionView::Quarantined { .. }));
	lifecycle
		.bind_checkpoint(1, Cursor(8), checkpoint("publication-b", 8), inspection)
		.expect("verified replacement checkpoint binds");

	assert_eq!(
		lifecycle.view(),
		ConnectionView::Online { generation: 1, applied: Some(Cursor(8)) }
	);
}

#[tokio::test]
async fn stable_server_and_schema_switches_require_verified_snapshot_replacement() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut original = lifecycle(&root);
	original.connection_generation = 1;
	let original_inspection =
		original.apply_snapshot(1, snapshot(1, "old", 1)).expect("original snapshot publishes");
	original
		.bind_checkpoint(1, Cursor(1), checkpoint(INSTANCE, 1), original_inspection.clone())
		.expect("original checkpoint binds");
	drop(original);

	let mut server_switched = lifecycle_for(&root, OTHER_SERVER, 1);
	assert_eq!(
		server_switched.view(),
		ConnectionView::Quarantined {
			reason: QuarantineReason::AuthorityChanged,
			recovery: QuarantineRecovery::VerifiedSnapshotReplacement,
		}
	);
	let mut switch_io = FakeIo::new(
		root.clone(),
		vec![ConnectAction::Session {
			actions: vec![
				SessionAction::Snapshot(snapshot_for(OTHER_SERVER, 2, "new", 1)),
				SessionAction::Cancel,
			]
			.into(),
			checkpoint: None,
			server_id: OTHER_SERVER,
			instance_id: "publication-b",
		}],
	);

	assert_eq!(server_switched.run_with_io(&mut switch_io).await, RunResult::Stopped);
	assert!(server_switched.quarantine.is_none());
	assert!(!server_switched.state.contains_key("old"));
	assert_eq!(server_switched.state["new"].revision, EntityRevision(1));
	assert!(
		server_switched
			.cache
			.as_ref()
			.expect("cache is available")
			.inspect_generation(&original_inspection.generation)
			.is_ok(),
		"old authority remains inspection-only"
	);
	drop(server_switched);

	let schema_switched = lifecycle_for(&root, OTHER_SERVER, 2);
	assert_eq!(
		schema_switched.view(),
		ConnectionView::Quarantined {
			reason: QuarantineReason::AuthorityChanged,
			recovery: QuarantineRecovery::VerifiedSnapshotReplacement,
		}
	);
}

#[test]
fn corrupt_cache_is_disposed_and_rebuilt_while_unsafe_root_requires_an_operator() {
	let corrupt_temporary = TempDir::new().expect("temporary directory is available");
	let corrupt_root = cache_root(&corrupt_temporary);
	let mut original = lifecycle(&corrupt_root);
	original.connection_generation = 1;
	original.apply_snapshot(1, snapshot(1, "system", 1)).expect("cache publishes");
	let generation = fs::read_dir(corrupt_root.join("generations"))
		.expect("generation directory is readable")
		.next()
		.expect("one generation exists")
		.expect("generation entry is readable")
		.path();
	let object = fs::read_dir(generation.join("objects"))
		.expect("object directory is readable")
		.next()
		.expect("one object exists")
		.expect("object entry is readable")
		.path();
	fs::write(object, b"tampered").expect("test corrupts cache content");
	drop(original);

	let rebuilt = lifecycle(&corrupt_root);
	assert_eq!(
		rebuilt.view(),
		ConnectionView::Quarantined {
			reason: QuarantineReason::CacheCorrupt,
			recovery: QuarantineRecovery::DisposedBeforeRebuild,
		}
	);
	assert_eq!(
		rebuilt
			.cache
			.as_ref()
			.expect("rebuilt cache is available")
			.inspect_current()
			.expect("test operation must succeed"),
		None
	);

	let unsafe_temporary = TempDir::new().expect("temporary directory is available");
	let unsafe_root =
		unsafe_temporary.path().canonicalize().expect("test operation must succeed").join("cache");
	fs::write(&unsafe_root, b"not a directory").expect("unsafe cache root is created");
	let unsafe_lifecycle = lifecycle(&unsafe_root);
	assert_eq!(
		unsafe_lifecycle.view(),
		ConnectionView::Quarantined {
			reason: QuarantineReason::CacheRootUnsafe,
			recovery: QuarantineRecovery::OperatorRequired,
		}
	);
	assert!(unsafe_lifecycle.cache.is_none());
}

#[test]
fn checkpoint_reuse_requires_current_content_attestation() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);
	lifecycle.connection_generation = 1;
	let inspection =
		lifecycle.apply_snapshot(1, snapshot(4, "system", 2)).expect("snapshot publishes");
	lifecycle
		.bind_checkpoint(1, Cursor(4), checkpoint(INSTANCE, 4), inspection)
		.expect("checkpoint binds");
	fs::write(root.join("current"), b"corrupt").expect("current attestation is corrupted");

	assert_eq!(lifecycle.reusable_checkpoint(), None);
	assert_eq!(
		lifecycle.view(),
		ConnectionView::Quarantined {
			reason: QuarantineReason::ContentAttestation,
			recovery: QuarantineRecovery::VerifiedSnapshotReplacement,
		}
	);
}

#[test]
fn complete_cache_deletion_cannot_remove_applied_product_state() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);
	lifecycle.connection_generation = 1;
	lifecycle.apply_snapshot(1, snapshot(1, "system", 5)).expect("snapshot publishes");

	ClientCache::dispose_all(&root).expect("disposable cache is removed");

	assert!(!root.exists());
	assert_eq!(lifecycle.state["system"].revision, EntityRevision(5));
	assert_eq!(lifecycle.last_cursor, Some(Cursor(1)));
}

#[test]
fn event_order_revision_and_connection_generation_are_fenced() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);

	lifecycle.connection_generation = 2;
	lifecycle.last_cursor = Some(Cursor(4));
	lifecycle.state.insert(
		"system".to_owned(),
		AppliedEntity {
			entity_id: entity("system"),
			revision: EntityRevision(2),
			bytes: Vec::new(),
		},
	);

	assert_eq!(
		lifecycle.apply_event(1, event(5, "system", 3)),
		Err(RetainedSessionFailure::PublicationOrder)
	);
	assert_eq!(
		lifecycle.view(),
		ConnectionView::Quarantined {
			reason: QuarantineReason::StaleConnectionGeneration,
			recovery: QuarantineRecovery::VerifiedSnapshotReplacement,
		}
	);

	lifecycle.connection_generation = 3;
	lifecycle.last_cursor = Some(Cursor(4));
	assert_eq!(
		lifecycle.apply_event(3, event(5, "system", 2)),
		Err(RetainedSessionFailure::PublicationOrder)
	);
}

#[tokio::test]
async fn transient_incompatible_and_stable_identity_failures_are_distinct() {
	let incompatible_temporary = TempDir::new().expect("temporary directory is available");
	let incompatible_root = cache_root(&incompatible_temporary);
	let mut incompatible = lifecycle(&incompatible_root);
	let mut incompatible_io = FakeIo::new(
		incompatible_root,
		vec![ConnectAction::Fail(RetainedSessionFailure::ProtocolMajorMismatch)],
	);

	assert_eq!(incompatible.run_with_io(&mut incompatible_io).await, RunResult::Incompatible);
	assert_eq!(
		incompatible.view(),
		ConnectionView::Incompatible(CompatibilityReason::ProtocolMajor)
	);

	let identity_temporary = TempDir::new().expect("temporary directory is available");
	let identity_root = cache_root(&identity_temporary);
	let mut identity = lifecycle(&identity_root);
	let mut identity_io = FakeIo::new(
		identity_root,
		vec![ConnectAction::Fail(RetainedSessionFailure::ServerIdentityMismatch)],
	);

	assert_eq!(identity.run_with_io(&mut identity_io).await, RunResult::Quarantined);
	assert_eq!(
		identity.view(),
		ConnectionView::Quarantined {
			reason: QuarantineReason::StableServerIdentity,
			recovery: QuarantineRecovery::OperatorRequired,
		}
	);
}

#[test]
fn publication_instances_are_part_of_checkpoint_identity() {
	assert_ne!(checkpoint(INSTANCE, 1), checkpoint("publication-b", 1));
}

#[test]
fn test_fixture_uses_only_typed_bounded_cache_content() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_root(&temporary);
	let mut lifecycle = lifecycle(&root);

	lifecycle.connection_generation = 1;
	let inspection = lifecycle
		.apply_snapshot(1, snapshot(1, "../../not-a-path", 1))
		.expect("typed snapshot publishes");

	assert_eq!(inspection.records, 1);
	assert!(!temporary.path().join("not-a-path").exists());
	assert!(fs::read(root.join("current")).is_ok());
}
