//! Deterministic lifecycle tests over the one private session/time seam.

use std::{
	collections::VecDeque,
	fs,
	os::unix::fs::{MetadataExt as _, PermissionsExt as _},
	path::{Path, PathBuf},
	sync::{
		Arc, Mutex,
		atomic::{AtomicUsize, Ordering},
	},
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use tempfile::TempDir;

use decodex_protocol::{
	CURRENT_VERSION, Channel, ClientProfile, CommandEnvelope, ConversationHistoryPage,
	ConversationHistoryResult, CorrelationId, Cursor, DEVELOPMENT_DOMAIN_PACK_ID, DoctorReport,
	EntityId, EntityRevision, EventEnvelope, EventPayload, HistoryCursorToken,
	PAPER_INVESTMENT_DOMAIN_PACK_ID, ProgramCycleDraftDto, ProgramEvidenceDraftDto,
	ProgramNodeKind, ProgramReviewClassification, ProgramReviewDraftDto, QueryEnvelope,
	QueryPayload, QueryResultEnvelope, QueryResultPayload, QuickTaskState,
	QuickTaskWorkingDirectory, RetainedSessionConfig, RetainedSessionFailure, ServerId,
	ServerInstanceId, SessionCheckpoint, SnapshotEnvelope, SnapshotItem, WireText,
};

use crate::{
	client_lifecycle::{
		AppliedEntity, CLIENT_CACHE_SCHEMA_GENERATION, CacheAuthority, CacheError, CacheLimits,
		ClientCache, ClientLifecycle, CompatibilityReason, ConnectionView, Delivery,
		LifecycleBuildError, LifecycleCancellation, LifecycleIo, QuarantineReason,
		QuarantineRecovery, RunResult, production_cache_parent,
	},
	history_pager::{
		HistoryCacheProbeEvent, HistoryCursorObservation, HistoryDispatch, HistoryLoadState,
		HistoryNavigationResult, HistoryPageSource, HistoryRetryReason, HistoryStaleReason,
	},
};

const SERVER: &str = "018f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
const OTHER_SERVER: &str = "028f0f9e-7b6e-4a31-8f4c-1d2e3f405162";
const INSTANCE: &str = "publication-a";

#[test]
fn production_cache_parent_normalizes_only_fixed_platform_prefix() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let physical_root = temporary.path().canonicalize().expect("fixture root canonicalizes");
	let physical_temp = physical_root.join("physical-temp");
	let arbitrary_alias = physical_root.join("arbitrary-alias");

	fs::create_dir(&physical_temp).expect("physical temporary directory is created");
	std::os::unix::fs::symlink(&physical_temp, &arbitrary_alias)
		.expect("arbitrary temporary directory alias is created");

	assert_eq!(
		production_cache_parent(&physical_temp).expect("physical temporary directory is accepted"),
		physical_temp.join("box.acg.decodex")
	);
	let aliased_cache_parent =
		production_cache_parent(&arbitrary_alias).expect("non-platform alias remains lexical");
	assert_eq!(
		aliased_cache_parent,
		arbitrary_alias.join("box.acg.decodex"),
		"arbitrary aliases must not be resolved"
	);
	assert!(matches!(
		ClientCache::open(
			aliased_cache_parent.join("client-cache"),
			CacheLimits::new(1_024, 4, 2).expect("test cache limits are valid"),
			CacheAuthority::new(&server(SERVER), CURRENT_VERSION, 1)
				.expect("test cache authority is valid"),
		),
		Err(CacheError::UnsafeRoot)
	));

	#[cfg(target_os = "macos")]
	{
		use crate::client_lifecycle::normalize_macos_var_prefix;

		fn reject_drifted_mapping() -> Result<(), CacheError> {
			Err(CacheError::UnsafeRoot)
		}

		let logical_temp = Path::new("/var/folders/decodex-test/T");
		assert_eq!(
			production_cache_parent(logical_temp).expect("fixed macOS mapping is valid"),
			Path::new("/private/var/folders/decodex-test/T/box.acg.decodex")
		);
		assert_eq!(
			normalize_macos_var_prefix(logical_temp, reject_drifted_mapping),
			Err(LifecycleBuildError::Cache(CacheError::UnsafeRoot))
		);
	}

	#[cfg(not(target_os = "macos"))]
	assert_eq!(
		production_cache_parent(Path::new("/var/folders/decodex-test/T"))
			.expect("non-macOS temporary path remains lexical"),
		Path::new("/var/folders/decodex-test/T/box.acg.decodex")
	);
}

#[test]
fn production_client_cache_authority_is_valid_at_protocol_v2_10() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let fixture_temp_dir =
		temporary.path().canonicalize().expect("fixture temporary directory canonicalizes");
	let config = retained_config(&fixture_temp_dir.join("config-cache-parent"), SERVER);
	let lifecycle = ClientLifecycle::production_with_temp_dir(config, &fixture_temp_dir)
		.expect("production lifecycle constructs at protocol V2.10");

	assert_eq!(CURRENT_VERSION.major, 2);
	assert_eq!(CURRENT_VERSION.minor, 10);
	assert_eq!(CLIENT_CACHE_SCHEMA_GENERATION, 1);
	assert!(lifecycle.cache.is_some(), "the production client cache opens");
	let encoded =
		serde_json::to_value(&lifecycle.cache_authority).expect("cache authority serializes");

	assert_eq!(encoded["protocol_major"].as_u64(), Some(u64::from(CURRENT_VERSION.major)));
	assert_eq!(encoded["protocol_minor"].as_u64(), Some(u64::from(CURRENT_VERSION.minor)));
	assert_eq!(encoded["schema_generation"].as_u64(), Some(CLIENT_CACHE_SCHEMA_GENERATION));
}

#[derive(Clone, Debug)]
struct FakeConfirmation {
	cursor: Cursor,
	cache_root: PathBuf,
	previous_current: Option<Vec<u8>>,
}

struct PendingSendControl {
	attempts: AtomicUsize,
	completed: AtomicUsize,
	entered: tokio::sync::Semaphore,
	release: tokio::sync::Semaphore,
}

impl PendingSendControl {
	fn new() -> Self {
		Self {
			attempts: AtomicUsize::new(0),
			completed: AtomicUsize::new(0),
			entered: tokio::sync::Semaphore::new(0),
			release: tokio::sync::Semaphore::new(0),
		}
	}

	async fn wait_until_entered(&self) {
		self.entered.acquire().await.expect("pending-send signal remains open").forget();
	}

	fn release_first(&self) {
		self.release.add_permits(1);
	}

	fn attempts(&self) -> usize {
		self.attempts.load(Ordering::SeqCst)
	}

	fn completed(&self) -> usize {
		self.completed.load(Ordering::SeqCst)
	}
}

enum SessionAction {
	Snapshot(SnapshotEnvelope),
	Event(Box<EventEnvelope>),
	HistoryPage { next_cursor: Option<&'static str> },
	Fail(RetainedSessionFailure),
	FailAfterQuery(RetainedSessionFailure),
	AwaitCancellation,
	Cancel,
	CancelAfterQuery,
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
	sent_queries: Arc<Mutex<Vec<QueryEnvelope>>>,
	query_cursor: usize,
	query_notify: Arc<tokio::sync::Notify>,
}

impl FakeSession {
	async fn next_query(&mut self) -> QueryEnvelope {
		loop {
			let notified = self.query_notify.notified();
			let query = self
				.sent_queries
				.lock()
				.expect("query log is available")
				.get(self.query_cursor)
				.cloned();

			if let Some(query) = query {
				self.query_cursor += 1;
				if matches!(query.payload, QueryPayload::GetConversationHistory { .. }) {
					return query;
				}
				continue;
			}

			notified.await;
		}
	}

	async fn next(&mut self) -> Result<Delivery<FakeConfirmation>, RetainedSessionFailure> {
		if matches!(self.actions.front(), Some(SessionAction::AwaitCancellation)) {
			self.cancellation.cancelled().await;
		}

		let query = if matches!(
			self.actions.front(),
			Some(
				SessionAction::HistoryPage { .. }
					| SessionAction::FailAfterQuery(_)
					| SessionAction::CancelAfterQuery
			)
		) {
			Some(self.next_query().await)
		} else {
			None
		};

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

				Ok(Delivery::Event { event: *event, confirmation })
			},
			SessionAction::HistoryPage { next_cursor } => {
				let query = query.expect("history response waits for one outbound query");

				Ok(Delivery::QueryResult(QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: self.server_id.clone(),
					query_id: query.query_id,
					payload: QueryResultPayload::ConversationHistory(
						ConversationHistoryResult::Page(history_page(next_cursor)),
					),
				}))
			},
			SessionAction::Fail(failure) => Err(failure),
			SessionAction::FailAfterQuery(failure) => {
				let _ = query.expect("scripted failure waits for one outbound query");

				Err(failure)
			},
			SessionAction::AwaitCancellation => Err(RetainedSessionFailure::Cancelled),
			SessionAction::Cancel => {
				self.cancellation.cancel();

				Err(RetainedSessionFailure::Cancelled)
			},
			SessionAction::CancelAfterQuery => {
				let _ = query.expect("scripted cancellation waits for one outbound query");
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
fn production_owner_keeps_the_session_after_window_close_and_stops_only_on_app_quit(
	cx: &mut gpui::TestAppContext,
) {
	use crate::shell::{Shell, retain_lifecycle_task};

	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);
	let cancellation = lifecycle.cancellation();
	let views = lifecycle.observe_views();
	let io = FakeIo::new(root, vec![connected(vec![SessionAction::AwaitCancellation], None)]);
	let closed = io.closed.clone();
	let (shell, visual) = cx.add_window_view(|window, cx| {
		Shell::new(window, cx, ConnectionView::Connecting { attempt: 1 })
	});
	let background = visual.spawn(|_| async move {
		let mut io = io;

		lifecycle.run_with_io(&mut io).await
	});
	let owner = visual.update(|_, cx| {
		retain_lifecycle_task(
			shell.downgrade(),
			cancellation,
			views,
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
	assert_eq!(*closed.lock().expect("close count is available"), 0);
	assert!(owner.read_with(visual, |owner, _| owner.is_running()));

	visual.cx.update(|cx| cx.shutdown());
	for _ in 0..4 {
		visual.executor().advance_clock(Duration::from_millis(40));
		visual.run_until_parked();
	}

	assert_eq!(*closed.lock().expect("close count is available"), 1);
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
	sent_queries: Arc<Mutex<Vec<QueryEnvelope>>>,
	query_notify: Arc<tokio::sync::Notify>,
	pending_send: Option<Arc<PendingSendControl>>,
	send_failures: VecDeque<RetainedSessionFailure>,
}

impl FakeIo {
	fn new(cache_parent: PathBuf, connections: Vec<ConnectAction>) -> Self {
		Self {
			connections: connections.into(),
			delays: Vec::new(),
			attempts: 0,
			active: 0,
			maximum_active: 0,
			closed: Arc::new(Mutex::new(0)),
			confirmations: Arc::new(Mutex::new(Vec::new())),
			cache_root: client_cache_root(&cache_parent),
			cancel_backoff: false,
			session: None,
			requested_checkpoints: Vec::new(),
			sent_queries: Arc::new(Mutex::new(Vec::new())),
			query_notify: Arc::new(tokio::sync::Notify::new()),
			pending_send: None,
			send_failures: VecDeque::new(),
		}
	}

	fn hold_first_send(&mut self, control: Arc<PendingSendControl>) {
		self.pending_send = Some(control);
	}

	fn fail_next_send(&mut self, failure: RetainedSessionFailure) {
		self.send_failures.push_back(failure);
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
				let query_cursor = self.sent_queries.lock().expect("query log is available").len();

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
					sent_queries: self.sent_queries.clone(),
					query_cursor,
					query_notify: self.query_notify.clone(),
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

	async fn send_command(
		&mut self,
		command: CommandEnvelope,
	) -> Result<(), RetainedSessionFailure> {
		assert_eq!(command.version, CURRENT_VERSION);
		if let Some(control) = self.pending_send.as_ref() {
			let attempt = control.attempts.fetch_add(1, Ordering::SeqCst) + 1;

			if attempt == 1 {
				control.entered.add_permits(1);
				control
					.release
					.acquire()
					.await
					.expect("pending-send release remains open")
					.forget();
			}
		}
		if let Some(failure) = self.send_failures.pop_front() {
			return Err(failure);
		}
		if let Some(control) = self.pending_send.as_ref() {
			control.completed.fetch_add(1, Ordering::SeqCst);
		}

		Ok(())
	}

	async fn send_query(&mut self, query: QueryEnvelope) -> Result<(), RetainedSessionFailure> {
		assert_eq!(query.version, CURRENT_VERSION);
		let controlled = matches!(query.payload, QueryPayload::GetConversationHistory { .. });
		if controlled && let Some(control) = self.pending_send.as_ref() {
			let attempt = control.attempts.fetch_add(1, Ordering::SeqCst) + 1;

			if attempt == 1 {
				control.entered.add_permits(1);
				control
					.release
					.acquire()
					.await
					.expect("pending-send release remains open")
					.forget();
			}
		}
		if let Some(failure) = self.send_failures.pop_front() {
			return Err(failure);
		}
		self.sent_queries.lock().expect("query log is available").push(query);
		self.query_notify.notify_waiters();
		if controlled && let Some(control) = self.pending_send.as_ref() {
			control.completed.fetch_add(1, Ordering::SeqCst);
		}

		Ok(())
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

#[tokio::test]
async fn fake_session_await_cancellation_survives_a_dropped_receive() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
	let config = retained_config(&root, SERVER);
	let cancellation = LifecycleCancellation::new();
	let mut io = FakeIo::new(root, vec![connected(vec![SessionAction::AwaitCancellation], None)]);

	io.connect(&config, None, &cancellation).await.expect("fake session connects");
	let mut receive = Box::pin(io.next());
	let poll = std::future::poll_fn(|context| {
		std::task::Poll::Ready(std::future::Future::poll(receive.as_mut(), context))
	})
	.await;

	assert!(matches!(poll, std::task::Poll::Pending));
	drop(receive);
	assert!(matches!(
		io.session.as_ref().expect("fake session remains connected").actions.front(),
		Some(SessionAction::AwaitCancellation)
	));

	cancellation.cancel();
	assert!(matches!(io.next().await, Err(RetainedSessionFailure::Cancelled)));
	assert!(
		io.session.as_ref().expect("fake session remains connected").actions.is_empty(),
		"the cancellation sentinel is consumed exactly once"
	);
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

fn history_cursor(value: &str) -> HistoryCursorToken {
	HistoryCursorToken::new(value).expect("history cursor is bounded")
}

fn history_page(next_cursor: Option<&str>) -> ConversationHistoryPage {
	ConversationHistoryPage { items: Vec::new(), next_cursor: next_cursor.map(history_cursor) }
}

fn history_result(
	dispatch: &HistoryDispatch,
	server_id: &ServerId,
	next_cursor: Option<&str>,
) -> QueryResultEnvelope {
	QueryResultEnvelope {
		version: CURRENT_VERSION,
		server_id: server_id.clone(),
		query_id: dispatch.envelope().query_id.clone(),
		payload: QueryResultPayload::ConversationHistory(ConversationHistoryResult::Page(
			history_page(next_cursor),
		)),
	}
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

fn maximum_snapshot(cursor: u64) -> SnapshotEnvelope {
	SnapshotEnvelope {
		version: CURRENT_VERSION,
		server_id: server(SERVER),
		cursor: Cursor(cursor),
		items: (0..1_024)
			.map(|index| SnapshotItem::SystemState {
				entity_id: entity(&format!("system-{index:04}")),
				revision: EntityRevision(1),
				status: WireText::new("ready").expect("status is bounded"),
			})
			.collect(),
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

fn cache_parent(temporary: &TempDir) -> PathBuf {
	temporary.path().canonicalize().expect("temporary root canonicalizes").join("cache")
}

fn client_cache_root(cache_parent: &Path) -> PathBuf {
	cache_parent.join("client-cache")
}

fn lifecycle(cache_parent: &Path) -> ClientLifecycle {
	lifecycle_for(cache_parent, SERVER, 1)
}

fn lifecycle_for(cache_parent: &Path, server_id: &str, schema_generation: u64) -> ClientLifecycle {
	let config = retained_config(cache_parent, server_id);

	ClientLifecycle::new(
		config,
		cache_parent,
		CacheLimits::new(2 * 1_024 * 1_024, 32, 16).expect("limits are valid"),
		schema_generation,
	)
	.expect("lifecycle constructs")
}

fn retained_config(cache_parent: &Path, server_id: &str) -> RetainedSessionConfig {
	let transport_root =
		cache_parent.parent().expect("cache parent has a parent").join("client-transport");
	let server_root = transport_root.join("server");

	fs::create_dir_all(&server_root).expect("fixed local transport namespace is available");
	fs::set_permissions(&transport_root, fs::Permissions::from_mode(0o700))
		.expect("transport root is owner-only");
	fs::set_permissions(&server_root, fs::Permissions::from_mode(0o700))
		.expect("transport server directory is owner-only");

	let service_owner_uid =
		fs::metadata(&transport_root).expect("transport root metadata is available").uid();
	let config = format!(
		r#"version = 1
active_profile = "local"
cache = {{}}

[profiles.local]
kind = "local"
policy = "same_uid"
service_owner_uid = {service_owner_uid}
expected_server_identity = "{server_id}"
"#,
	);
	let config_path = transport_root.join("config.toml");

	fs::write(&config_path, config).expect("fixture client configuration is written");
	fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600))
		.expect("fixture client configuration is owner-only");

	ClientProfile::load(&transport_root, None)
		.expect("fixture local profile is valid")
		.retained_session_config()
		.expect("fixture retained session config is local")
}

#[tokio::test]
async fn retry_progression_is_capped_and_uses_only_fake_time() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
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
async fn run_with_io_dispatches_history_and_restarts_from_head_after_reconnect() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);
	let pager = lifecycle.history_pager();

	pager.open(entity("conversation-run")).expect("history view opens");

	let mut io = FakeIo::new(
		root,
		vec![
			connected(
				vec![
					SessionAction::Snapshot(snapshot(1, "system", 1)),
					SessionAction::HistoryPage { next_cursor: Some("cursor-1") },
					SessionAction::FailAfterQuery(RetainedSessionFailure::Disconnected),
				],
				None,
			),
			connected(vec![SessionAction::CancelAfterQuery], Some(checkpoint(INSTANCE, 1))),
		],
	);
	let sent_queries = io.sent_queries.clone();

	assert_eq!(lifecycle.run_with_io(&mut io).await, RunResult::Stopped);

	let queries = sent_queries.lock().expect("query log is available");

	let routes = queries
		.iter()
		.filter_map(|query| match &query.payload {
			QueryPayload::GetConversationHistory { conversation_id, after, .. } => {
				Some((query.query_id.clone(), conversation_id.clone(), after.clone()))
			},
			_ => None,
		})
		.collect::<Vec<_>>();
	assert_eq!(routes.len(), 3);
	assert_ne!(routes[0].0, routes[1].0);
	assert_ne!(routes[0].0, routes[2].0);
	assert_ne!(routes[1].0, routes[2].0);

	assert_eq!(
		routes
			.into_iter()
			.map(|(_, conversation_id, after)| (conversation_id, after))
			.collect::<Vec<_>>(),
		vec![
			(entity("conversation-run"), None),
			(entity("conversation-run"), Some(history_cursor("cursor-1"))),
			(entity("conversation-run"), None),
		]
	);
	assert!(lifecycle.quarantine.is_none());
}

#[tokio::test]
async fn history_cache_io_begins_only_after_send_and_fresh_admission() {
	let failed_temporary = TempDir::new().expect("temporary directory is available");
	let failed_root = cache_parent(&failed_temporary);
	let mut failed_lifecycle = lifecycle(&failed_root);
	let failed_pager = failed_lifecycle.history_pager();

	failed_pager.open(entity("conversation-failed-send")).expect("failed-send history view opens");
	let mut failed_io = FakeIo::new(
		failed_root.clone(),
		vec![connected(
			vec![
				SessionAction::Snapshot(snapshot(1, "failed-system", 1)),
				SessionAction::AwaitCancellation,
			],
			None,
		)],
	);

	failed_io.fail_next_send(RetainedSessionFailure::Backpressure);
	failed_io.cancel_backoff = true;

	assert_eq!(failed_lifecycle.run_with_io(&mut failed_io).await, RunResult::Stopped);
	assert!(failed_pager.cache_probe_events().is_empty());
	assert!(!failed_root.join("history-page-cache-v1").exists());

	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);
	let pager = lifecycle.history_pager();
	let cancellation = lifecycle.cancellation();
	let control = Arc::new(PendingSendControl::new());

	pager.open(entity("conversation-cache-order")).expect("history view opens");
	let mut io = FakeIo::new(
		root.clone(),
		vec![connected(
			vec![
				SessionAction::Snapshot(snapshot(1, "system", 1)),
				SessionAction::AwaitCancellation,
			],
			None,
		)],
	);
	let sent_queries = io.sent_queries.clone();

	io.hold_first_send(control.clone());
	assert!(pager.cache_probe_events().is_empty());
	assert!(!root.join("history-page-cache-v1").exists());

	let run = lifecycle.run_with_io(&mut io);

	tokio::pin!(run);
	tokio::select! {
		result = &mut run => panic!("lifecycle stopped before held send: {result:?}"),
		_ = control.wait_until_entered() => {},
	}

	assert_eq!(control.completed(), 0);
	assert!(pager.cache_probe_events().is_empty());
	assert!(!root.join("history-page-cache-v1").exists());

	control.release_first();

	let mut lookup_started = false;
	for _ in 0..64 {
		if pager.cache_probe_events() == [HistoryCacheProbeEvent::LookupStarted] {
			lookup_started = true;

			break;
		}

		tokio::select! {
			result = &mut run => panic!("lifecycle stopped before cache lookup: {result:?}"),
			_ = tokio::task::yield_now() => {},
		}
	}

	assert!(lookup_started, "successful send must start the matching cache lookup");
	assert_eq!(control.completed(), 1);

	let query = sent_queries
		.lock()
		.expect("query log is available")
		.iter()
		.find(|query| matches!(query.payload, QueryPayload::GetConversationHistory { .. }))
		.cloned()
		.expect("successful history send is recorded");
	let server_id = server(SERVER);

	assert!(matches!(
		pager.route_result(
			1,
			&server_id,
			QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: server_id.clone(),
				query_id: query.query_id,
				payload: QueryResultPayload::ConversationHistory(ConversationHistoryResult::Page(
					history_page(None)
				),),
			},
		),
		crate::history_pager::HistoryRouteOutcome::Fresh
	));
	assert_eq!(
		pager.cache_probe_events(),
		[HistoryCacheProbeEvent::LookupStarted, HistoryCacheProbeEvent::PublicationStarted,]
	);
	let fresh = pager.snapshot();

	assert_eq!(fresh.visible, Some(history_page(None)));
	assert_eq!(fresh.visible_source, Some(HistoryPageSource::FreshServer));

	cancellation.cancel();

	assert_eq!(run.await, RunResult::Stopped);
}

#[tokio::test]
async fn pending_history_send_blocks_replacement_until_settlement() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);
	let pager = lifecycle.history_pager();
	let cancellation = lifecycle.cancellation();
	let control = Arc::new(PendingSendControl::new());

	pager.open(entity("conversation-old")).expect("initial history view opens");

	let mut io = FakeIo::new(
		root,
		vec![connected(
			vec![
				SessionAction::Snapshot(snapshot(1, "system", 1)),
				SessionAction::HistoryPage { next_cursor: Some("old-only-cursor") },
				SessionAction::HistoryPage { next_cursor: None },
				SessionAction::AwaitCancellation,
			],
			None,
		)],
	);
	let sent_queries = io.sent_queries.clone();

	io.hold_first_send(control.clone());

	let run = lifecycle.run_with_io(&mut io);

	tokio::pin!(run);
	tokio::select! {
		result = &mut run => panic!("lifecycle stopped before first send settled: {result:?}"),
		_ = control.wait_until_entered() => {},
	}

	pager.open(entity("conversation-new")).expect("replacement history view opens");
	for _ in 0..4 {
		tokio::select! {
			result = &mut run => panic!("lifecycle stopped with send unresolved: {result:?}"),
			_ = tokio::task::yield_now() => {},
		}
	}

	assert_eq!(control.attempts(), 1);
	assert!(
		sent_queries
			.lock()
			.expect("query log is available")
			.iter()
			.all(|query| !matches!(query.payload, QueryPayload::GetConversationHistory { .. }))
	);

	control.release_first();

	let mut upgraded = false;

	for _ in 0..64 {
		let snapshot = pager.snapshot();

		if snapshot.conversation_id == Some(entity("conversation-new"))
			&& snapshot.visible_source == Some(HistoryPageSource::FreshServer)
		{
			assert_eq!(
				snapshot
					.last_stale_cancellation
					.expect("superseded send remains a stale request")
					.reason,
				HistoryStaleReason::ConversationChanged
			);
			assert_eq!(snapshot.visible.expect("replacement page is visible").next_cursor, None);
			upgraded = true;

			break;
		}

		tokio::select! {
			result = &mut run => panic!("lifecycle stopped before replacement result: {result:?}"),
			_ = tokio::task::yield_now() => {},
		}
	}

	assert!(upgraded, "matching replacement result must become current");
	assert_eq!(control.attempts(), 2);

	let queries = sent_queries
		.lock()
		.expect("query log is available")
		.iter()
		.filter(|query| matches!(query.payload, QueryPayload::GetConversationHistory { .. }))
		.cloned()
		.collect::<Vec<_>>();

	assert_eq!(queries.len(), 2);
	assert_ne!(queries[0].query_id, queries[1].query_id);
	assert!(matches!(
		&queries[0].payload,
		QueryPayload::GetConversationHistory { conversation_id, after: None, .. }
			if conversation_id == &entity("conversation-old")
	));
	assert!(matches!(
		&queries[1].payload,
		QueryPayload::GetConversationHistory { conversation_id, after: None, .. }
			if conversation_id == &entity("conversation-new")
	));

	cancellation.cancel();

	assert_eq!(run.await, RunResult::Stopped);
}

#[tokio::test]
async fn history_results_route_only_to_the_exact_current_view_request() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);
	let pager = lifecycle.history_pager();
	let server_id = server(SERVER);

	pager.bind_session(1, server_id.clone());
	pager.open(entity("conversation-a")).expect("first view opens");

	let first = pager.next_dispatch(1, &server_id).await;

	lifecycle
		.route_history_result(
			1,
			QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: server_id.clone(),
				query_id: first.envelope().query_id.clone(),
				payload: QueryResultPayload::ConversationHistory(ConversationHistoryResult::Page(
					ConversationHistoryPage { items: Vec::new(), next_cursor: None },
				)),
			},
		)
		.expect("exact result routes");

	assert_eq!(pager.snapshot().visible_source, Some(HistoryPageSource::FreshServer));

	pager.open(entity("conversation-b")).expect("second view opens");
	let stale = pager.next_dispatch(1, &server_id).await;
	pager.open(entity("conversation-c")).expect("replacement view opens");

	lifecycle
		.route_history_result(
			1,
			QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id,
				query_id: stale.envelope().query_id.clone(),
				payload: QueryResultPayload::ConversationHistory(ConversationHistoryResult::Page(
					ConversationHistoryPage { items: Vec::new(), next_cursor: None },
				)),
			},
		)
		.expect("stale result is ignored without failing the session");

	let snapshot = pager.snapshot();

	assert_eq!(snapshot.conversation_id, Some(entity("conversation-c")));
	assert!(snapshot.visible.is_none());
	assert_eq!(
		snapshot.last_stale_cancellation.expect("stale request is recorded").reason,
		HistoryStaleReason::ConversationChanged
	);
}

#[tokio::test]
async fn history_pages_leave_maximum_authoritative_snapshot_inventory_unchanged() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
	let config = retained_config(&root, SERVER);
	let mut lifecycle = ClientLifecycle::new(
		config,
		&root,
		CacheLimits::new(8 * 1_024 * 1_024, 1_024, 16).expect("limits are valid"),
		1,
	)
	.expect("lifecycle constructs");

	lifecycle.connection_generation = 1;
	let inspection = lifecycle
		.apply_snapshot(1, maximum_snapshot(9))
		.expect("maximum authoritative snapshot publishes");
	lifecycle
		.bind_checkpoint(1, Cursor(9), checkpoint(INSTANCE, 9), inspection)
		.expect("maximum snapshot checkpoint binds");

	let inspection_before = lifecycle
		.cache
		.as_ref()
		.expect("cache is available")
		.inspect_current()
		.expect("cache inspection succeeds")
		.expect("maximum snapshot generation exists");
	let binding_before = lifecycle.binding.clone();
	let inventory_before = lifecycle.state.clone();
	let cursor_before = lifecycle.last_cursor;
	let pager = lifecycle.history_pager();
	let server_id = server(SERVER);

	pager.bind_session(1, server_id.clone());
	pager.open(entity("conversation-large")).expect("history view opens");
	let head = pager.next_dispatch(1, &server_id).await;

	lifecycle
		.route_history_result(1, history_result(&head, &server_id, Some("cursor-1")))
		.expect("history head routes");
	let continuation = pager.next_dispatch(1, &server_id).await;

	lifecycle
		.route_history_result(1, history_result(&continuation, &server_id, None))
		.expect("history continuation routes");

	assert_eq!(pager.snapshot().retained_pages, 2);
	assert_eq!(
		lifecycle
			.cache
			.as_ref()
			.expect("cache is available")
			.inspect_current()
			.expect("cache inspection succeeds")
			.expect("maximum snapshot generation remains"),
		inspection_before
	);
	assert_eq!(lifecycle.binding, binding_before);
	assert_eq!(lifecycle.state, inventory_before);
	assert_eq!(lifecycle.last_cursor, cursor_before);
	assert!(lifecycle.quarantine.is_none());
}

#[derive(Clone, Copy, Debug)]
enum HistoryCacheFailurePhase {
	ParentResolution,
	InitialValidation,
	PostOpenOperation,
}

#[tokio::test]
async fn history_cache_failure_phases_preserve_fresh_page_and_client_cache_authority() {
	let cases = [
		("parent resolution", HistoryCacheFailurePhase::ParentResolution),
		("initial validation", HistoryCacheFailurePhase::InitialValidation),
		("post-open operation", HistoryCacheFailurePhase::PostOpenOperation),
	];
	let generation_inventory = |path: &Path| {
		let mut names = fs::read_dir(path.join("generations"))
			.expect("generation inventory is readable")
			.map(|entry| entry.expect("generation entry is readable").file_name())
			.collect::<Vec<_>>();

		names.sort();
		names
	};

	for (name, phase) in cases {
		let temporary = TempDir::new().expect("temporary directory is available");
		let root = cache_parent(&temporary);
		let mut lifecycle = lifecycle(&root);

		lifecycle.connection_generation = 1;
		let inspection = lifecycle
			.apply_snapshot(1, snapshot(11, "system", 4))
			.expect("authoritative snapshot publishes");
		lifecycle
			.bind_checkpoint(1, Cursor(11), checkpoint(INSTANCE, 11), inspection)
			.expect("authoritative checkpoint binds");

		let client_root = client_cache_root(&root);
		let history_root = root.join("history-page-cache-v1");
		let inspection_before = lifecycle
			.cache
			.as_ref()
			.expect("client cache is available")
			.inspect_current()
			.expect("client cache inspection succeeds")
			.expect("current client generation exists");
		let pointer_before =
			fs::read(client_root.join("current")).expect("current pointer is readable");
		let generations_before = generation_inventory(&client_root);
		let binding_before = lifecycle.binding.clone();
		let inventory_before = lifecycle.state.clone();
		let cursor_before = lifecycle.last_cursor;
		let quarantine_before = lifecycle.quarantine;
		let lexical_parent_before = lifecycle.cache_parent.clone();
		let pager = lifecycle.history_pager();
		let server_id = server(SERVER);

		pager.bind_session(1, server_id.clone());
		pager.open(entity("conversation-isolated")).expect("history view opens");
		let dispatch = pager.next_dispatch(1, &server_id).await;
		let send = pager.begin_send(&dispatch).expect("history request enters the send phase");

		assert!(pager.finish_send(&send));
		match phase {
			HistoryCacheFailurePhase::ParentResolution => {
				fs::set_permissions(
					root.parent().expect("cache parent has an external base"),
					fs::Permissions::from_mode(0o770),
				)
				.expect("external base is made unsafe");
				pager.lookup_sent_request(&send);
			},
			HistoryCacheFailurePhase::InitialValidation => {
				fs::create_dir(&history_root).expect("history cache root is created");
				fs::set_permissions(&history_root, fs::Permissions::from_mode(0o755))
					.expect("history cache root is made unsafe");
				pager.lookup_sent_request(&send);
			},
			HistoryCacheFailurePhase::PostOpenOperation => {
				pager.lookup_sent_request(&send);
				assert_eq!(pager.snapshot().cache_diagnostic, None, "{name}");
				fs::write(history_root.join("foreign"), b"foreign")
					.expect("foreign post-open artifact is created");
			},
		}

		lifecycle
			.route_history_result(1, history_result(&dispatch, &server_id, None))
			.expect("fresh history result remains request-local and usable");

		let fresh = pager.snapshot();

		assert_eq!(fresh.visible, Some(history_page(None)), "{name}");
		assert_eq!(fresh.visible_source, Some(HistoryPageSource::FreshServer), "{name}",);
		assert_eq!(fresh.cursor, HistoryCursorObservation::NoContinuationObserved, "{name}",);
		assert_eq!(fresh.load, HistoryLoadState::Visible, "{name}");
		assert_eq!(
			fresh.cache_diagnostic,
			Some(crate::history_pager::HistoryCacheDiagnostic::Unavailable),
			"{name}",
		);
		assert_eq!(
			lifecycle
				.cache
				.as_ref()
				.expect("client cache remains available")
				.inspect_current()
				.expect("client cache inspection still succeeds")
				.expect("current client generation remains"),
			inspection_before,
			"{name}",
		);
		assert_eq!(
			fs::read(client_root.join("current")).expect("current pointer remains readable"),
			pointer_before,
			"{name}",
		);
		assert_eq!(generation_inventory(&client_root), generations_before, "{name}");
		assert_eq!(lifecycle.binding, binding_before, "{name}");
		assert_eq!(lifecycle.state, inventory_before, "{name}");
		assert_eq!(lifecycle.last_cursor, cursor_before, "{name}");
		assert_eq!(lifecycle.quarantine, quarantine_before, "{name}");
		assert_eq!(lifecycle.cache_parent, lexical_parent_before, "{name}");
		assert_eq!(lifecycle.cache_parent, root, "{name}");
		assert_eq!(
			lifecycle.view(),
			ConnectionView::Online { generation: 1, applied: Some(Cursor(11)) },
			"{name}",
		);

		if matches!(phase, HistoryCacheFailurePhase::ParentResolution) {
			fs::set_permissions(
				root.parent().expect("cache parent has an external base"),
				fs::Permissions::from_mode(0o700),
			)
			.expect("external base permissions are restored");
		}
	}
}

#[tokio::test]
async fn wrong_history_payload_is_request_local_and_preserves_authoritative_state() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);

	lifecycle.connection_generation = 1;
	let inspection = lifecycle
		.apply_snapshot(1, snapshot(7, "system", 3))
		.expect("authoritative snapshot publishes");
	lifecycle
		.bind_checkpoint(1, Cursor(7), checkpoint(INSTANCE, 7), inspection)
		.expect("authoritative checkpoint binds");

	let inspection_before = lifecycle
		.cache
		.as_ref()
		.expect("cache is available")
		.inspect_current()
		.expect("cache inspection succeeds")
		.expect("authoritative generation exists");
	let binding_before = lifecycle.binding.clone();
	let inventory_before = lifecycle.state.clone();
	let cursor_before = lifecycle.last_cursor;
	let pager = lifecycle.history_pager();
	let server_id = server(SERVER);

	pager.bind_session(1, server_id.clone());
	pager.open(entity("conversation-protocol")).expect("history view opens");
	let dispatch = pager.next_dispatch(1, &server_id).await;
	let wrong_payload = DoctorReport::new(server_id.clone(), CURRENT_VERSION, Vec::new())
		.expect("bounded doctor report constructs");

	lifecycle
		.route_history_result(
			1,
			QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id,
				query_id: dispatch.envelope().query_id.clone(),
				payload: QueryResultPayload::DoctorStatus(wrong_payload),
			},
		)
		.expect("wrong history payload closes only the request");

	assert_eq!(
		pager.snapshot().load,
		HistoryLoadState::ClosedUnavailable(
			crate::history_pager::HistoryClosedReason::ProtocolMismatch,
		)
	);
	assert_eq!(
		lifecycle
			.cache
			.as_ref()
			.expect("cache is available")
			.inspect_current()
			.expect("cache inspection succeeds")
			.expect("authoritative generation remains"),
		inspection_before
	);
	assert_eq!(lifecycle.binding, binding_before);
	assert_eq!(lifecycle.state, inventory_before);
	assert_eq!(lifecycle.last_cursor, cursor_before);
	assert!(lifecycle.quarantine.is_none());
	assert_eq!(
		lifecycle.view(),
		ConnectionView::Online { generation: 1, applied: Some(Cursor(7)) }
	);
}

#[tokio::test]
async fn session_replacement_requires_a_fresh_head_before_retained_topology_returns() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);
	let pager = lifecycle.history_pager();
	let server_id = server(SERVER);

	pager.bind_session(1, server_id.clone());
	pager.open(entity("conversation-session")).expect("history view opens");
	let head = pager.next_dispatch(1, &server_id).await;

	lifecycle
		.route_history_result(1, history_result(&head, &server_id, Some("old-cursor")))
		.expect("old session head routes");
	let retained_prefetch = pager.next_dispatch(1, &server_id).await;

	lifecycle
		.route_history_result(
			1,
			history_result(&retained_prefetch, &server_id, Some("old-next-cursor")),
		)
		.expect("old session prefetch is retained");

	let retained = pager.snapshot();

	assert_eq!(retained.retained_pages, 2);
	assert_eq!(retained.visible_source, Some(HistoryPageSource::FreshServer));
	assert_eq!(retained.cursor, HistoryCursorObservation::ContinuationAvailable);

	pager.bind_session(2, server_id.clone());

	let invalidated = pager.snapshot();

	assert!(invalidated.visible.is_none());
	assert_eq!(invalidated.cursor, HistoryCursorObservation::Unknown);
	assert_eq!(invalidated.retained_pages, 0);
	assert_eq!(pager.show_next(), HistoryNavigationResult::BoundaryUnknown);

	let replacement = pager.next_dispatch(2, &server_id).await;

	assert!(matches!(
		&replacement.envelope().payload,
		QueryPayload::GetConversationHistory {
			conversation_id,
			after: None,
			..
		} if conversation_id == &entity("conversation-session")
	));
	assert_eq!(pager.show_next(), HistoryNavigationResult::BoundaryUnknown);

	lifecycle
		.route_history_result(2, history_result(&replacement, &server_id, Some("new-cursor")))
		.expect("matching new-session head restores authority");

	let upgraded = pager.snapshot();

	assert_eq!(upgraded.visible_source, Some(HistoryPageSource::FreshServer));
	assert_eq!(upgraded.cursor, HistoryCursorObservation::ContinuationAvailable);

	let new_prefetch = pager.next_dispatch(2, &server_id).await;

	assert!(matches!(
		&new_prefetch.envelope().payload,
		QueryPayload::GetConversationHistory {
			after: Some(after),
			..
		} if after == &history_cursor("new-cursor")
	));
}

#[tokio::test]
async fn prior_session_result_cannot_replace_session_invalidated_view() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);
	let pager = lifecycle.history_pager();
	let server_id = server(SERVER);

	pager.bind_session(1, server_id.clone());
	pager.open(entity("conversation-session")).expect("history view opens");
	let head = pager.next_dispatch(1, &server_id).await;

	lifecycle
		.route_history_result(1, history_result(&head, &server_id, Some("old-cursor")))
		.expect("old-session head routes");
	let prior = pager.next_dispatch(1, &server_id).await;

	pager.session_ended(1);

	let offline = pager.snapshot();

	assert!(offline.visible.is_none());
	assert_eq!(offline.visible_source, None);
	assert_eq!(offline.cursor, HistoryCursorObservation::Unknown);
	assert_eq!(
		offline.load,
		HistoryLoadState::RetryableUnavailable(HistoryRetryReason::SessionUnavailable)
	);
	assert_eq!(pager.show_next(), HistoryNavigationResult::BoundaryUnknown);

	pager.bind_session(2, server_id.clone());
	let current = pager.next_dispatch(2, &server_id).await;

	assert!(matches!(
		&current.envelope().payload,
		QueryPayload::GetConversationHistory {
			conversation_id,
			after: None,
			..
		} if conversation_id == &entity("conversation-session")
	));
	lifecycle
		.route_history_result(1, history_result(&prior, &server_id, Some("prior-cursor")))
		.expect("prior-session delivery is ignored");

	let still_invalidated = pager.snapshot();

	assert!(still_invalidated.visible.is_none());
	assert_eq!(still_invalidated.visible_source, None);
	assert_eq!(still_invalidated.cursor, HistoryCursorObservation::Unknown);
	assert!(pager.dispatch_is_current(&current));

	lifecycle
		.route_history_result(2, history_result(&current, &server_id, None))
		.expect("current-session result repopulates the invalidated view");

	let fresh = pager.snapshot();

	assert_eq!(fresh.visible_source, Some(HistoryPageSource::FreshServer));
	assert_eq!(fresh.cursor, HistoryCursorObservation::NoContinuationObserved);
}

#[tokio::test]
async fn observer_receives_the_complete_bounded_retry_progression() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
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
	let connect_root = cache_parent(&connect_temporary);
	let mut connect_lifecycle = lifecycle(&connect_root);
	let mut connect_io = FakeIo::new(connect_root, vec![ConnectAction::Cancel]);

	assert_eq!(connect_lifecycle.run_with_io(&mut connect_io).await, RunResult::Stopped);
	assert_eq!(connect_lifecycle.view(), ConnectionView::Stopped);

	let backoff_temporary = TempDir::new().expect("temporary directory is available");
	let backoff_root = cache_parent(&backoff_temporary);
	let mut backoff_lifecycle = lifecycle(&backoff_root);
	let mut backoff_io =
		FakeIo::new(backoff_root, vec![ConnectAction::Fail(RetainedSessionFailure::Disconnected)]);
	backoff_io.cancel_backoff = true;

	assert_eq!(backoff_lifecycle.run_with_io(&mut backoff_io).await, RunResult::Stopped);

	let receive_temporary = TempDir::new().expect("temporary directory is available");
	let receive_root = cache_parent(&receive_temporary);
	let mut receive_lifecycle = lifecycle(&receive_root);
	let mut receive_io =
		FakeIo::new(receive_root, vec![connected(vec![SessionAction::Cancel], None)]);

	assert_eq!(receive_lifecycle.run_with_io(&mut receive_io).await, RunResult::Stopped);
	assert_eq!(*receive_io.closed.lock().expect("close count is available"), 1);
}

#[tokio::test]
async fn cache_and_state_application_precede_checkpoint_confirmation() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);
	let mut io = FakeIo::new(
		root.clone(),
		vec![connected(
			vec![SessionAction::Snapshot(snapshot(7, "system", 3)), SessionAction::Cancel],
			None,
		)],
	);

	assert_eq!(lifecycle.run_with_io(&mut io).await, RunResult::Stopped);
	assert!(client_cache_root(&root).join("current").is_file());
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
	let root = cache_parent(&temporary);
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
				SessionAction::Event(Box::new(event(6, "first", 3))),
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
	let resume_root = cache_parent(&resume_temporary);
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
				vec![SessionAction::Event(Box::new(event(8, "system", 4))), SessionAction::Cancel],
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
	let fallback_root = cache_parent(&fallback_temporary);
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
	let root = cache_parent(&temporary);
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
	let root = cache_parent(&temporary);
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
				actions: vec![SessionAction::Event(Box::new(event(3, "old", 2)))].into(),
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
	let root = cache_parent(&temporary);
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
	let root = cache_parent(&temporary);
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
	let root = cache_parent(&temporary);
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
	let corrupt_root = cache_parent(&corrupt_temporary);
	let mut original = lifecycle(&corrupt_root);
	original.connection_generation = 1;
	original.apply_snapshot(1, snapshot(1, "system", 1)).expect("cache publishes");
	let corrupt_client_cache_root = client_cache_root(&corrupt_root);
	let generation = fs::read_dir(corrupt_client_cache_root.join("generations"))
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
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);
	lifecycle.connection_generation = 1;
	let inspection =
		lifecycle.apply_snapshot(1, snapshot(4, "system", 2)).expect("snapshot publishes");
	lifecycle
		.bind_checkpoint(1, Cursor(4), checkpoint(INSTANCE, 4), inspection)
		.expect("checkpoint binds");
	fs::write(client_cache_root(&root).join("current"), b"corrupt")
		.expect("current attestation is corrupted");

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
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);
	lifecycle.connection_generation = 1;
	lifecycle.apply_snapshot(1, snapshot(1, "system", 5)).expect("snapshot publishes");

	let client_root = client_cache_root(&root);
	ClientCache::dispose_all(&client_root).expect("disposable cache is removed");

	assert!(!client_root.exists());
	assert_eq!(lifecycle.state["system"].revision, EntityRevision(5));
	assert_eq!(lifecycle.last_cursor, Some(Cursor(1)));
}

#[test]
fn event_order_revision_and_connection_generation_are_fenced() {
	let temporary = TempDir::new().expect("temporary directory is available");
	let root = cache_parent(&temporary);
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
	let incompatible_root = cache_parent(&incompatible_temporary);
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
	let identity_root = cache_parent(&identity_temporary);
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
	let root = cache_parent(&temporary);
	let mut lifecycle = lifecycle(&root);

	lifecycle.connection_generation = 1;
	let inspection = lifecycle
		.apply_snapshot(1, snapshot(1, "../../not-a-path", 1))
		.expect("typed snapshot publishes");

	assert_eq!(inspection.records, 1);
	assert!(!temporary.path().join("not-a-path").exists());
	assert!(fs::read(client_cache_root(&root).join("current")).is_ok());
}

#[tokio::test]
#[ignore = "requires the user's live Decodex daemon and creates two conversations plus one later turn"]
async fn live_daemon_accepts_sequential_quick_tasks_and_returns_history() {
	use crate::quick_tasks::{QuickTaskCommandState, QuickTasksLoadState};

	let profile = ClientProfile::load_default(None).expect("the live profile is configured");
	let config =
		profile.retained_session_config().expect("the live retained session is configured");
	let mut lifecycle =
		ClientLifecycle::production(config).expect("the production lifecycle is available");
	let quick_tasks = lifecycle.quick_tasks();
	let history = lifecycle.history_pager();
	let cancellation = lifecycle.cancellation();
	quick_tasks.activate();

	let run = lifecycle.run();
	tokio::pin!(run);
	let ready_deadline = tokio::time::Instant::now() + Duration::from_secs(20);
	loop {
		let snapshot = quick_tasks.snapshot();
		if snapshot.load == QuickTasksLoadState::Ready && snapshot.can_submit {
			break;
		}
		assert!(tokio::time::Instant::now() < ready_deadline, "Quick Tasks did not become ready");
		tokio::select! {
			result = &mut run => panic!("live lifecycle stopped before Quick Tasks became ready: {result:?}"),
			() = tokio::time::sleep(Duration::from_millis(50)) => {},
		}
	}

	let mut first_conversation = None;
	let mut first_history_items = 0;
	for ordinal in 1..=2 {
		quick_tasks.begin_new();
		quick_tasks
			.create(&format!(
				"Reply briefly with: Decodex sequential live smoke {ordinal} is working."
			))
			.expect("the live composer command is accepted for dispatch");
		let accepted_deadline = tokio::time::Instant::now() + Duration::from_secs(120);
		let conversation_id = loop {
			let snapshot = quick_tasks.snapshot();
			match snapshot.command {
				QuickTaskCommandState::ManualRecovery(action) => {
					panic!("the live daemon requested manual recovery before starting: {action:?}")
				},
				QuickTaskCommandState::OutcomeUnknown => {
					panic!("the live daemon could not determine whether the command was accepted")
				},
				QuickTaskCommandState::Refused => {
					panic!("the live daemon refused the composed Quick Task")
				},
				_ => {},
			}
			if let Some(conversation_id) = snapshot.selected.as_ref() {
				let task = snapshot
					.tasks
					.iter()
					.find(|task| &task.conversation_id == conversation_id)
					.expect("the selected conversation has a projection");
				if task.state == QuickTaskState::Ready {
					break conversation_id.clone();
				}
			}
			assert!(
				tokio::time::Instant::now() < accepted_deadline,
				"the live daemon did not finish the composed Quick Task; load={:?}, command={:?}, state={:?}",
				snapshot.load,
				snapshot.command,
				snapshot.selected_task().map(|task| task.state),
			);
			tokio::select! {
				result = &mut run => panic!("live lifecycle stopped before command acceptance: {result:?}"),
				() = tokio::time::sleep(Duration::from_millis(50)) => {},
			}
		};

		history.open(conversation_id.clone()).expect("the accepted conversation history opens");
		let history_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
		let mut advanced_visible_items = 0;
		let history_items = loop {
			let snapshot = history.snapshot();
			let visible_items = snapshot.visible.as_ref().map_or(0, |page| page.items.len());
			if snapshot.visible_source == Some(HistoryPageSource::FreshServer) && visible_items > 0
			{
				match snapshot.cursor {
					HistoryCursorObservation::NoContinuationObserved => {
						assert_eq!(snapshot.conversation_id, Some(conversation_id.clone()));
						break visible_items;
					},
					HistoryCursorObservation::ContinuationAvailable
						if visible_items > advanced_visible_items =>
					{
						advanced_visible_items = visible_items;
						assert_eq!(history.show_next(), HistoryNavigationResult::Moved);
					},
					_ => {},
				}
			}
			assert!(
				tokio::time::Instant::now() < history_deadline,
				"the accepted conversation remained stuck without complete fresh history; load={:?}, source={:?}, cursor={:?}, visible_items={}, retained_pages={}",
				snapshot.load,
				snapshot.visible_source,
				snapshot.cursor,
				visible_items,
				snapshot.retained_pages,
			);
			tokio::select! {
				result = &mut run => panic!("live lifecycle stopped before history readback: {result:?}"),
				() = tokio::time::sleep(Duration::from_millis(50)) => {},
			}
		};
		if ordinal == 1 {
			first_conversation = Some(conversation_id);
			first_history_items = history_items;
		}
	}

	let first_conversation = first_conversation.expect("the first live conversation completed");
	assert!(quick_tasks.select(first_conversation.clone()));
	let baseline_session_revision = quick_tasks
		.snapshot()
		.selected_task()
		.and_then(|task| task.runtime_session_revision)
		.expect("the completed first conversation has a RuntimeSession revision");
	quick_tasks
		.submit("Reply briefly with: Decodex same-thread rehydration is working.")
		.expect("the later live turn is accepted for dispatch");
	let continuation_deadline = tokio::time::Instant::now() + Duration::from_secs(120);
	loop {
		let snapshot = quick_tasks.snapshot();
		match snapshot.command {
			QuickTaskCommandState::ManualRecovery(action) => {
				panic!("the live daemon requested manual recovery during rehydration: {action:?}")
			},
			QuickTaskCommandState::OutcomeUnknown => {
				panic!("the live daemon could not determine the later-turn outcome")
			},
			QuickTaskCommandState::Refused => panic!("the live daemon refused the later live turn"),
			_ => {},
		}
		if snapshot.selected_task().is_some_and(|task| {
			task.state == QuickTaskState::Ready
				&& task
					.runtime_session_revision
					.is_some_and(|revision| revision > baseline_session_revision)
		}) {
			break;
		}
		assert!(
			tokio::time::Instant::now() < continuation_deadline,
			"the later live turn did not advance its RuntimeSession; load={:?}, command={:?}, state={:?}",
			snapshot.load,
			snapshot.command,
			snapshot.selected_task().map(|task| task.state),
		);
		tokio::select! {
			result = &mut run => panic!("live lifecycle stopped before the later turn completed: {result:?}"),
			() = tokio::time::sleep(Duration::from_millis(50)) => {},
		}
	}

	history.open(first_conversation.clone()).expect("the rehydrated conversation history opens");
	let continuation_history_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
	let mut advanced_visible_items = 0;
	loop {
		let snapshot = history.snapshot();
		let visible_items = snapshot.visible.as_ref().map_or(0, |page| page.items.len());
		if snapshot.visible_source == Some(HistoryPageSource::FreshServer) {
			match snapshot.cursor {
				HistoryCursorObservation::NoContinuationObserved
					if visible_items > first_history_items =>
				{
					assert_eq!(snapshot.conversation_id, Some(first_conversation));
					break;
				},
				HistoryCursorObservation::ContinuationAvailable
					if visible_items > advanced_visible_items =>
				{
					advanced_visible_items = visible_items;
					assert_eq!(history.show_next(), HistoryNavigationResult::Moved);
				},
				_ => {},
			}
		}
		assert!(
			tokio::time::Instant::now() < continuation_history_deadline,
			"the rehydrated conversation history did not grow; load={:?}, source={:?}, cursor={:?}, visible_items={}, baseline_items={}, retained_pages={}",
			snapshot.load,
			snapshot.visible_source,
			snapshot.cursor,
			visible_items,
			first_history_items,
			snapshot.retained_pages,
		);
		tokio::select! {
			result = &mut run => panic!("live lifecycle stopped before rehydrated history readback: {result:?}"),
			() = tokio::time::sleep(Duration::from_millis(50)) => {},
		}
	}

	cancellation.cancel();
	let result = tokio::time::timeout(Duration::from_secs(5), &mut run)
		.await
		.expect("live lifecycle stops after cancellation");
	assert_eq!(result, RunResult::Stopped);
}

#[tokio::test]
#[ignore = "requires the user's live Decodex daemon; binds the dogfood Pack and creates one paper-only Program conversation"]
async fn live_daemon_completes_the_builtin_domain_pack_pressure_test() {
	use crate::{
		programs::{ProgramCommandState, ProgramsLoadState},
		quick_tasks::{QuickTaskCommandState, QuickTasksLoadState},
	};

	const PAPER_PROGRAM_ID: &str = "d1000000-0000-4000-8000-000000000001";
	const PAPER_SIGNAL_ID: &str = "d2000000-0000-4000-8000-000000000001";
	const PAPER_CLAIM_ID: &str = "d3000000-0000-4000-8000-000000000001";
	const PAPER_PROPOSAL_ID: &str = "d4000000-0000-4000-8000-000000000001";
	const PAPER_OBJECTIVE_ID: &str = "d5000000-0000-4000-8000-000000000001";
	const PAPER_WORK_ITEM_ID: &str = "d6000000-0000-4000-8000-000000000001";
	const PAPER_REVIEW_ID: &str = "e1000000-0000-4000-8000-000000000001";
	const PAPER_DETERMINISTIC_EVIDENCE_ID: &str = "e2000000-0000-4000-8000-000000000001";
	const PAPER_EXTERNAL_EVIDENCE_ID: &str = "e3000000-0000-4000-8000-000000000001";

	fn text(value: &str) -> WireText {
		WireText::new(value).expect("live pressure-test text is bounded")
	}

	fn entity(value: &str) -> EntityId {
		EntityId::new(value).expect("live pressure-test identity is canonical")
	}

	fn now_micros() -> i64 {
		i64::try_from(
			SystemTime::now()
				.duration_since(UNIX_EPOCH)
				.expect("system time follows the Unix epoch")
				.as_micros(),
		)
		.expect("current time fits SQLite")
	}

	let working_directory = std::env::current_dir()
		.expect("live test working directory exists")
		.canonicalize()
		.expect("live test working directory canonicalizes");
	let working_directory = QuickTaskWorkingDirectory::new(
		working_directory.to_str().expect("live test working directory is UTF-8").to_owned(),
	)
	.expect("live test working directory is accepted");
	let profile = ClientProfile::load_default(None).expect("the live profile is configured");
	let config =
		profile.retained_session_config().expect("the live retained session is configured");
	let mut lifecycle =
		ClientLifecycle::production(config).expect("the production lifecycle is available");
	let programs = lifecycle.programs();
	let quick_tasks = lifecycle.quick_tasks();
	let cancellation = lifecycle.cancellation();
	programs.activate();
	quick_tasks.activate();

	let run = lifecycle.run();
	tokio::pin!(run);
	let ready_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
	loop {
		let program_snapshot = programs.snapshot();
		let task_snapshot = quick_tasks.snapshot();
		if program_snapshot.load == ProgramsLoadState::Ready
			&& task_snapshot.load == QuickTasksLoadState::Ready
			&& task_snapshot.can_submit
		{
			break;
		}
		assert!(
			tokio::time::Instant::now() < ready_deadline,
			"live Program and Quick Task controllers did not become ready: programs={:?}, tasks={:?}",
			program_snapshot.load,
			task_snapshot.load,
		);
		tokio::select! {
			result = &mut run => panic!("live lifecycle stopped before pressure-test readiness: {result:?}"),
			() = tokio::time::sleep(Duration::from_millis(50)) => {},
		}
	}

	let development = programs
		.snapshot()
		.programs
		.into_iter()
		.find(|program| program.name.as_str() == "Adaptive Factory Spine V1 Live Proof")
		.expect("the three-cycle Development dogfood Program exists");
	assert!(programs.select(development.program_id.clone()));
	let development_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
	loop {
		let snapshot = programs.snapshot();
		if let Some(cycle) = snapshot
			.cycle
			.as_ref()
			.filter(|cycle| cycle.program.program_id == development.program_id)
		{
			match cycle.domain_pack.as_ref() {
				None => programs
					.bind_domain_pack(
						development.program_id.clone(),
						text(DEVELOPMENT_DOMAIN_PACK_ID),
						cycle.program.revision,
					)
					.expect("legacy Development Pack binding queues"),
				Some(pack)
					if pack.descriptor.id.as_str() == DEVELOPMENT_DOMAIN_PACK_ID
						&& pack.entities.len() == 3
						&& pack.relations.len() == 2 =>
				{
					assert_eq!(
						pack.entities.iter().map(|entity| entity.id.as_str()).collect::<Vec<_>>(),
						vec![
							"4ab46bd9-8378-494b-aa14-db4408b814ef",
							"19639395-365c-4060-b160-9833783a33d4",
							"665cac94-4178-4230-9580-813e8fa70c16",
						],
					);
					break;
				},
				Some(_) => {},
			}
		}
		assert!(
			!matches!(
				snapshot.command,
				ProgramCommandState::OutcomeUnknown | ProgramCommandState::Refused
			),
			"Development Pack binding did not settle: {:?}",
			snapshot.command,
		);
		assert!(
			tokio::time::Instant::now() < development_deadline,
			"Development Pack projection did not become ready",
		);
		tokio::select! {
			result = &mut run => panic!("live lifecycle stopped during Development Pack binding: {result:?}"),
			() = tokio::time::sleep(Duration::from_millis(50)) => {},
		}
	}

	let paper_program_id = entity(PAPER_PROGRAM_ID);
	if !programs.snapshot().programs.iter().any(|program| program.program_id == paper_program_id) {
		programs
			.create(ProgramCycleDraftDto {
				program_id: paper_program_id.clone(),
				domain_pack_id: text(PAPER_INVESTMENT_DOMAIN_PACK_ID),
				signal_id: entity(PAPER_SIGNAL_ID),
				claim_id: entity(PAPER_CLAIM_ID),
				proposal_id: entity(PAPER_PROPOSAL_ID),
				objective_id: entity(PAPER_OBJECTIVE_ID),
				work_item_id: entity(PAPER_WORK_ITEM_ID),
				name: text("June Treasury Curve Research"),
				purpose: text(
					"Evaluate one reproducible 2s10s yield-curve thesis through a bounded Program loop.",
				),
				non_goals: vec![text(
					"Do not fetch live market data or place any paper or real order.",
				)],
				review_policy: text(
					"Review after Codex verifies the frozen fixture and cites deterministic results.",
				),
				signal_source: text("Frozen official U.S. Treasury June 2025 fixture"),
				signal_summary: text(
					"The June 2025 2-year and 10-year par yields provide a finite curve sample.",
				),
				signal_observed_at_micros: now_micros(),
				claim_statement: text(
					"The sample can test whether the 2s10s slope stayed positive during the month.",
				),
				proposal_summary: text(
					"Have Codex independently verify the frozen observations and spread bounds.",
				),
				proposal_expected_effect: text(
					"Produce a cited, reproducible conclusion for the June 2025 2s10s slope.",
				),
				proposal_risk: text(
					"An incorrect parser or unit conversion could fabricate the spread bounds.",
				),
				proposal_evidence_need: text(
					"A settled Codex run, deterministic fixture checks, and exact SQLite readback.",
				),
				objective_outcome: text(
					"Produce a cited, reproducible conclusion for the June 2025 2s10s slope.",
				),
				acceptance_criteria: vec![text(
					"The conclusion reports 20 observations and the exact first, last, minimum, maximum, and range spreads.",
				)],
				validation_criteria: vec![text(
					"The bound Quick Task settles without live data or an external action.",
				)],
				work_item_title: text("Verify the June 2025 Treasury 2s10s thesis"),
				work_item_instructions: text(
					"Inspect crates/decodex-runtime/fixtures/us_treasury_yield_curve_2025_06.csv. Recompute observation count, first and last spread, minimum, maximum, and range. Report whether the slope remained positive. Do not use live data or take any external action.",
				),
				working_directory: working_directory.clone(),
			})
			.expect("paper Program creation queues");
	} else {
		assert!(programs.select(paper_program_id.clone()));
	}

	let paper_projection_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
	let mut paper_cycle = loop {
		let snapshot = programs.snapshot();
		if let Some(cycle) =
			snapshot.cycle.filter(|cycle| cycle.program.program_id == paper_program_id)
		{
			let pack = cycle.domain_pack.as_ref().expect("paper Program has an immutable Pack");
			assert_eq!(pack.descriptor.id.as_str(), PAPER_INVESTMENT_DOMAIN_PACK_ID);
			assert_eq!(pack.entities.len(), 4);
			assert_eq!(pack.relations.len(), 3);
			assert_eq!(
				pack.entities.iter().map(|entity| entity.id.as_str()).collect::<Vec<_>>(),
				vec![
					"b8fde6ee-1aaf-4674-b356-b72e3223e21e",
					"78ccfe77-dd4c-49f2-8b2e-9eee80150aeb",
					"d0cd787c-52e8-470b-945e-b22c5dd2e046",
					"1017c447-35e2-4975-877b-0ad2fe13bd6b",
				],
			);
			break cycle;
		}
		assert!(
			!matches!(
				snapshot.command,
				ProgramCommandState::OutcomeUnknown | ProgramCommandState::Refused
			),
			"paper Program command did not settle: {:?}",
			snapshot.command,
		);
		assert!(
			tokio::time::Instant::now() < paper_projection_deadline,
			"paper Program projection did not become ready",
		);
		tokio::select! {
			result = &mut run => panic!("live lifecycle stopped before paper projection: {result:?}"),
			() = tokio::time::sleep(Duration::from_millis(50)) => {},
		}
	};

	if !paper_cycle.nodes.iter().any(|node| node.kind == ProgramNodeKind::Review) {
		let work_item = paper_cycle
			.nodes
			.iter()
			.find(|node| node.id.as_str() == PAPER_WORK_ITEM_ID)
			.expect("paper WorkItem is projected")
			.clone();
		let conversation_id = if let Some(conversation_id) = work_item.conversation_id.clone() {
			quick_tasks.select_when_available(conversation_id.clone());
			conversation_id
		} else {
			quick_tasks.begin_new();
			let submission = quick_tasks
				.create_for_program_work_item(
					&format!(
						"Decodex Program WorkItem {}\n\n{}",
						work_item.id.as_str(),
						work_item.summary.as_str(),
					),
					work_item.id.clone(),
					working_directory.clone(),
				)
				.expect("paper WorkItem Quick Task queues");
			programs.expect_execution(submission.conversation_id.clone());
			submission.conversation_id
		};

		let execution_deadline = tokio::time::Instant::now() + Duration::from_secs(180);
		loop {
			let snapshot = quick_tasks.snapshot();
			match snapshot.command {
				QuickTaskCommandState::ManualRecovery(action) => {
					panic!("paper Quick Task requested manual recovery: {action:?}")
				},
				QuickTaskCommandState::OutcomeUnknown => {
					panic!("paper Quick Task acceptance remained unknown")
				},
				QuickTaskCommandState::Refused => panic!("paper Quick Task was refused"),
				_ => {},
			}
			if snapshot.tasks.iter().any(|task| {
				task.conversation_id == conversation_id && task.state == QuickTaskState::Ready
			}) {
				break;
			}
			assert!(
				tokio::time::Instant::now() < execution_deadline,
				"paper Quick Task did not reach terminal ready state: {:?}",
				snapshot
					.tasks
					.iter()
					.find(|task| task.conversation_id == conversation_id)
					.map(|task| task.state),
			);
			tokio::select! {
				result = &mut run => panic!("live lifecycle stopped during paper execution: {result:?}"),
				() = tokio::time::sleep(Duration::from_millis(50)) => {},
			}
		}

		let review_ready_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
		loop {
			let snapshot = programs.snapshot();
			if let Some(cycle) = snapshot.cycle.as_ref().filter(|cycle| {
				cycle.program.program_id == paper_program_id
					&& cycle.nodes.iter().any(|node| {
						node.id.as_str() == PAPER_WORK_ITEM_ID
							&& node.conversation_id.as_ref() == Some(&conversation_id)
					})
			}) {
				paper_cycle = cycle.clone();
				break;
			}
			let _ = programs.refresh_selected();
			assert!(
				tokio::time::Instant::now() < review_ready_deadline,
				"paper Program did not observe its terminal Conversation",
			);
			tokio::select! {
				result = &mut run => panic!("live lifecycle stopped before paper Review: {result:?}"),
				() = tokio::time::sleep(Duration::from_millis(50)) => {},
			}
		}

		let observed_at_micros = now_micros();
		programs
			.record_review(ProgramReviewDraftDto {
				review_id: entity(PAPER_REVIEW_ID),
				program_id: paper_program_id.clone(),
				work_item_id: entity(PAPER_WORK_ITEM_ID),
				deterministic: ProgramEvidenceDraftDto {
					evidence_id: entity(PAPER_DETERMINISTIC_EVIDENCE_ID),
					source: text("Repository fixture and runtime gates"),
					summary: text(
						"The frozen fixture has 20 observations; 2s10s first and last are 52 bp, minimum 44 bp, maximum 56 bp, range 12 bp, and every spread is positive.",
					),
					observed_at_micros,
				},
				external: ProgramEvidenceDraftDto {
					evidence_id: entity(PAPER_EXTERNAL_EVIDENCE_ID),
					source: text("Codex app-server and SQLite readback"),
					summary: text(
						"The bound paper-only Codex Quick Task reached positive terminal evidence through the ordinary ProviderAttempt path.",
					),
					observed_at_micros,
				},
				classification: ProgramReviewClassification::KnowledgeProgress,
				rationale: text(
					"The same Program kernel produced one reproducible domain conclusion without a second scheduler, live data, or an external action.",
				),
			})
			.expect("paper Program Review queues");
	}

	let review_deadline = tokio::time::Instant::now() + Duration::from_secs(30);
	loop {
		let snapshot = programs.snapshot();
		if let Some(cycle) = snapshot.cycle.as_ref().filter(|cycle| {
			cycle.program.program_id == paper_program_id
				&& cycle.program.revision == EntityRevision(2)
				&& cycle.nodes.iter().any(|node| {
					node.kind == ProgramNodeKind::Review && node.id.as_str() == PAPER_REVIEW_ID
				})
		}) {
			let pack = cycle.domain_pack.as_ref().expect("completed paper Pack projection");
			assert_eq!(pack.descriptor.id.as_str(), PAPER_INVESTMENT_DOMAIN_PACK_ID);
			assert!(
				pack.relations
					.iter()
					.any(|relation| { relation.kind.as_str() == "finance.compared_with" })
			);
			break;
		}
		assert!(
			!matches!(
				snapshot.command,
				ProgramCommandState::OutcomeUnknown | ProgramCommandState::Refused
			),
			"paper Review did not settle: {:?}; prior cycle revision was {}",
			snapshot.command,
			paper_cycle.program.revision.0,
		);
		assert!(
			tokio::time::Instant::now() < review_deadline,
			"paper Review did not become authoritative",
		);
		tokio::select! {
			result = &mut run => panic!("live lifecycle stopped before paper Review readback: {result:?}"),
			() = tokio::time::sleep(Duration::from_millis(50)) => {},
		}
	}

	cancellation.cancel();
	let result = tokio::time::timeout(Duration::from_secs(5), &mut run)
		.await
		.expect("live lifecycle stops after pressure-test cancellation");
	assert_eq!(result, RunResult::Stopped);
}

#[tokio::test]
#[ignore = "requires the user's live Decodex daemon and reconciles local projections with Codex"]
async fn live_daemon_reconciles_archived_quick_tasks() {
	use crate::quick_tasks::{QuickTaskRefreshState, QuickTasksLoadState};

	let profile = ClientProfile::load_default(None).expect("the live profile is configured");
	let config =
		profile.retained_session_config().expect("the live retained session is configured");
	let mut lifecycle =
		ClientLifecycle::production(config).expect("the production lifecycle is available");
	let quick_tasks = lifecycle.quick_tasks();
	let cancellation = lifecycle.cancellation();
	quick_tasks.activate();

	let run = lifecycle.run();
	tokio::pin!(run);
	let ready_deadline = tokio::time::Instant::now() + Duration::from_secs(20);
	let initial_count = loop {
		let snapshot = quick_tasks.snapshot();
		if snapshot.load == QuickTasksLoadState::Ready && snapshot.can_submit {
			break snapshot.tasks.len();
		}
		assert!(tokio::time::Instant::now() < ready_deadline, "Quick Tasks did not become ready");
		tokio::select! {
			result = &mut run => panic!("live lifecycle stopped before Quick Tasks became ready: {result:?}"),
			() = tokio::time::sleep(Duration::from_millis(50)) => {},
		}
	};

	quick_tasks.refresh_all().expect("the live provider reconciliation starts");
	let refresh_deadline = tokio::time::Instant::now() + Duration::from_secs(600);
	let (checked, archived, failed) = loop {
		let snapshot = quick_tasks.snapshot();
		match snapshot.refresh {
			QuickTaskRefreshState::Complete { checked, archived, failed } => {
				break (checked, archived, failed);
			},
			QuickTaskRefreshState::Stopped { checked, total, archived, failed } => panic!(
				"live provider reconciliation stopped at {checked}/{total}; archived={archived}, failed={failed}"
			),
			_ => {},
		}
		assert!(
			tokio::time::Instant::now() < refresh_deadline,
			"live provider reconciliation did not finish; refresh={:?}",
			snapshot.refresh,
		);
		tokio::select! {
			result = &mut run => panic!("live lifecycle stopped during provider reconciliation: {result:?}"),
			() = tokio::time::sleep(Duration::from_millis(50)) => {},
		}
	};
	let final_count = quick_tasks.snapshot().tasks.len();
	eprintln!(
		"live reconciliation: initial={initial_count}, final={final_count}, checked={checked}, archived={archived}, skipped={failed}"
	);
	assert!(checked > 0, "the live database must contain provider-backed conversations");
	assert!(final_count <= initial_count, "reconciliation must not create conversations");

	cancellation.cancel();
	let result = tokio::time::timeout(Duration::from_secs(5), &mut run)
		.await
		.expect("live lifecycle stops after cancellation");
	assert_eq!(result, RunResult::Stopped);
}
