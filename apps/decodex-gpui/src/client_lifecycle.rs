//! Private bounded composition of the retained session and disposable client cache.

use std::{
	collections::{HashMap, HashSet},
	path::{Path, PathBuf},
	sync::{
		Arc,
		atomic::{AtomicBool, Ordering},
		mpsc::{self, Receiver, Sender},
	},
	time::Duration,
};

use tokio::sync::Notify;

use decodex_protocol::{
	ApplicationConfirmation, CURRENT_VERSION, ClientFailure, CommandEnvelope, CommandReceipt,
	CommandResultEnvelope, Cursor, EntityId, EntityRevision, EventEnvelope, EventPayload,
	QueryEnvelope, QueryResultEnvelope, RetainedSession, RetainedSessionConfig,
	RetainedSessionFailure, ServerId, SessionCancellation, SessionCheckpoint, SessionDelivery,
	SnapshotEnvelope, SnapshotItem,
};

use crate::{
	client_cache::{
		CacheAuthority, CacheError, CacheLimits, ClientCache, GenerationInspection,
		ObjectCertainty, ObjectInput,
	},
	health_query::{HealthDispatch, HealthQuery, HealthRouteOutcome},
	history_pager::{HistoryDispatch, HistoryPager, HistoryRouteOutcome},
	quick_tasks::{QuickTaskDispatch, QuickTaskRouteOutcome, QuickTasks},
};

const RETRY_DELAYS: [Duration; 4] = [
	Duration::from_millis(100),
	Duration::from_millis(250),
	Duration::from_millis(500),
	Duration::from_secs(1),
];
const MAX_CONNECTION_ATTEMPTS: u8 = 5;
const PRODUCTION_CACHE_BYTES: u64 = 64 * 1_024 * 1_024;
const PRODUCTION_CACHE_OBJECTS: usize = 2_048;
const PRODUCTION_CACHE_GENERATIONS: usize = 16;
const CLIENT_CACHE_SCHEMA_GENERATION: u64 = 1;
const HISTORY_PAGE_CACHE_SCHEMA_GENERATION: u32 = 1;

/// State rendered by the later shell without exposing transport or cache internals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionView {
	/// One caller-owned connection attempt is in progress.
	Connecting { attempt: u8 },
	/// A verified retained session is online.
	Online { generation: u64, applied: Option<Cursor> },
	/// A transient failure is waiting for its deterministic retry.
	OfflineRetrying { next_attempt: u8, delay: Duration },
	/// Protocol compatibility is closed until client or server replacement.
	Incompatible(CompatibilityReason),
	/// Data or identity is isolated under an explicit recovery lifetime.
	Quarantined { reason: QuarantineReason, recovery: QuarantineRecovery },
	/// Cooperative terminal shutdown is closing the owned session.
	ShuttingDown,
	/// No connection attempt or retained session remains owned.
	Stopped,
}

/// Closed compatibility states that deterministic retry cannot repair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompatibilityReason {
	Startup(ClientFailure),
	InvalidEndpoint,
	ProtocolMajor,
	ProtocolMinor,
	PublicationIdentityUnavailable,
}

/// Why checkpoint and cache reuse lost authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuarantineReason {
	StableServerIdentity,
	AuthorityChanged,
	PublicationInstanceChanged,
	CheckpointMismatch,
	CacheCorrupt,
	CacheRootUnsafe,
	ContentAttestation,
	ApplicationOrder,
	ApplicationConfirmation,
	StaleConnectionGeneration,
}

/// Exact lifetime and replacement rule for quarantined material.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuarantineRecovery {
	/// Keep the old immutable generation for inspection, but never reuse its checkpoint.
	VerifiedSnapshotReplacement,
	/// The complete disposable cache was removed before an empty rebuild.
	DisposedBeforeRebuild,
	/// The filesystem root or stable identity is unsafe and requires operator replacement.
	OperatorRequired,
}

/// Terminal result of the caller-owned lifecycle future.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RunResult {
	Stopped,
	RetryExhausted,
	Incompatible,
	Quarantined,
}

/// Construction failures that occur before any connection attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleBuildError {
	Cache(CacheError),
}

/// Cloneable cooperative shutdown handle for the one caller-owned run future.
#[derive(Clone, Debug)]
pub(crate) struct LifecycleCancellation {
	inner: Arc<CancellationInner>,
}

#[derive(Debug)]
struct CancellationInner {
	cancelled: AtomicBool,
	notify: Notify,
	session: SessionCancellation,
}

impl LifecycleCancellation {
	fn new() -> Self {
		Self {
			inner: Arc::new(CancellationInner {
				cancelled: AtomicBool::new(false),
				notify: Notify::new(),
				session: SessionCancellation::new(),
			}),
		}
	}

	/// Request terminal shutdown during connect, receive, or backoff.
	pub(crate) fn cancel(&self) {
		if !self.inner.cancelled.swap(true, Ordering::AcqRel) {
			self.inner.session.cancel();
			self.inner.notify.notify_waiters();
		}
	}

	fn is_cancelled(&self) -> bool {
		self.inner.cancelled.load(Ordering::Acquire)
	}

	fn session(&self) -> SessionCancellation {
		self.inner.session.clone()
	}

	async fn cancelled(&self) {
		loop {
			let notified = self.inner.notify.notified();

			if self.is_cancelled() {
				return;
			}

			notified.await;
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AppliedEntity {
	entity_id: EntityId,
	revision: EntityRevision,
	bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CacheBinding {
	checkpoint: SessionCheckpoint,
	generation: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Quarantine {
	reason: QuarantineReason,
	recovery: QuarantineRecovery,
}

struct CachedFact {
	entity_id: EntityId,
	revision: EntityRevision,
	bytes: Vec<u8>,
}

enum Delivery<C> {
	Snapshot { snapshot: SnapshotEnvelope, confirmation: C },
	Event { event: EventEnvelope, confirmation: C },
	QueryResult(QueryResultEnvelope),
	CommandReceipt(CommandReceipt),
	CommandResult(CommandResultEnvelope),
}

enum SessionStep<C> {
	Delivery(Result<Delivery<C>, RetainedSessionFailure>),
	Health(HealthDispatch),
	History(HistoryDispatch),
	QuickTask(QuickTaskDispatch),
}

/// The single private seam around retained-session operations and retry time.
trait LifecycleIo {
	type Confirmation;

	async fn connect(
		&mut self,
		config: &RetainedSessionConfig,
		checkpoint: Option<SessionCheckpoint>,
		cancellation: &LifecycleCancellation,
	) -> Result<Option<SessionCheckpoint>, RetainedSessionFailure>;
	async fn next(&mut self) -> Result<Delivery<Self::Confirmation>, RetainedSessionFailure>;
	async fn send_command(
		&mut self,
		command: CommandEnvelope,
	) -> Result<(), RetainedSessionFailure>;
	async fn send_query(&mut self, query: QueryEnvelope) -> Result<(), RetainedSessionFailure>;
	fn confirm_applied(
		&mut self,
		confirmation: Self::Confirmation,
	) -> Result<SessionCheckpoint, RetainedSessionFailure>;
	async fn close(&mut self) -> Result<(), RetainedSessionFailure>;

	async fn backoff(
		&mut self,
		delay: Duration,
		cancellation: &LifecycleCancellation,
	) -> Result<(), RetainedSessionFailure>;
}

struct TokioIo {
	session: Option<RetainedSession>,
}

impl LifecycleIo for TokioIo {
	type Confirmation = ApplicationConfirmation;

	async fn connect(
		&mut self,
		config: &RetainedSessionConfig,
		checkpoint: Option<SessionCheckpoint>,
		cancellation: &LifecycleCancellation,
	) -> Result<Option<SessionCheckpoint>, RetainedSessionFailure> {
		let session =
			RetainedSession::connect(config.clone(), checkpoint, cancellation.session()).await?;
		let checkpoint = session.checkpoint().cloned();

		self.session = Some(session);

		Ok(checkpoint)
	}

	async fn next(&mut self) -> Result<Delivery<Self::Confirmation>, RetainedSessionFailure> {
		match self.session.as_mut().ok_or(RetainedSessionFailure::Closed)?.next().await? {
			SessionDelivery::Snapshot { snapshot, confirmation } =>
				Ok(Delivery::Snapshot { snapshot, confirmation }),
			SessionDelivery::Event { event, confirmation } =>
				Ok(Delivery::Event { event, confirmation }),
			SessionDelivery::QueryResult(result) => Ok(Delivery::QueryResult(result)),
			SessionDelivery::CommandReceipt(receipt) => Ok(Delivery::CommandReceipt(receipt)),
			SessionDelivery::CommandResult(result) => Ok(Delivery::CommandResult(result)),
		}
	}

	async fn send_command(
		&mut self,
		command: CommandEnvelope,
	) -> Result<(), RetainedSessionFailure> {
		self.session.as_mut().ok_or(RetainedSessionFailure::Closed)?.send_command(command).await
	}

	async fn send_query(&mut self, query: QueryEnvelope) -> Result<(), RetainedSessionFailure> {
		self.session.as_mut().ok_or(RetainedSessionFailure::Closed)?.send_query(query).await
	}

	fn confirm_applied(
		&mut self,
		confirmation: Self::Confirmation,
	) -> Result<SessionCheckpoint, RetainedSessionFailure> {
		self.session
			.as_mut()
			.ok_or(RetainedSessionFailure::Closed)?
			.confirm_applied(confirmation)
			.cloned()
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
		tokio::select! {
			() = tokio::time::sleep(delay) => Ok(()),
			() = cancellation.cancelled() => Err(RetainedSessionFailure::Cancelled),
		}
	}
}

/// Private lifecycle owner retained by the production shell.
pub(crate) struct ClientLifecycle {
	config: RetainedSessionConfig,
	server_id: ServerId,
	cache_parent: PathBuf,
	cache_limits: CacheLimits,
	cache_authority: CacheAuthority,
	cache: Option<ClientCache>,
	health_query: HealthQuery,
	history_pager: HistoryPager,
	quick_tasks: QuickTasks,
	state: HashMap<String, AppliedEntity>,
	last_cursor: Option<Cursor>,
	binding: Option<CacheBinding>,
	quarantine: Option<Quarantine>,
	connection_generation: u64,
	view: ConnectionView,
	view_observer: Option<Sender<ConnectionView>>,
	cancellation: LifecycleCancellation,
}

impl ClientLifecycle {
	/// Construct the production lifecycle without exposing disposable-cache policy to the shell.
	pub(crate) fn production(config: RetainedSessionConfig) -> Result<Self, LifecycleBuildError> {
		Self::production_with_temp_dir(config, &std::env::temp_dir())
	}

	fn production_with_temp_dir(
		config: RetainedSessionConfig,
		temp_dir: &Path,
	) -> Result<Self, LifecycleBuildError> {
		let cache_limits = CacheLimits::new(
			PRODUCTION_CACHE_BYTES,
			PRODUCTION_CACHE_OBJECTS,
			PRODUCTION_CACHE_GENERATIONS,
		)
		.expect("fixed production cache limits are valid");
		let cache_parent = production_cache_parent(temp_dir)?;

		Self::new(config, cache_parent, cache_limits, CLIENT_CACHE_SCHEMA_GENERATION)
	}

	/// Bind the lifecycle to one exact stable server, protocol/schema generation, and cache parent.
	fn new(
		config: RetainedSessionConfig,
		cache_parent: impl Into<PathBuf>,
		cache_limits: CacheLimits,
		schema_generation: u64,
	) -> Result<Self, LifecycleBuildError> {
		let server_id = config.expected_server_id().clone();
		let cache_authority = CacheAuthority::new(&server_id, CURRENT_VERSION, schema_generation)
			.map_err(LifecycleBuildError::Cache)?;
		let cache_parent = cache_parent.into();
		let client_cache_root = cache_parent.join("client-cache");
		let (cache, quarantine) =
			initialize_cache(&client_cache_root, cache_limits, cache_authority.clone());
		let history_pager =
			HistoryPager::production(&cache_parent, HISTORY_PAGE_CACHE_SCHEMA_GENERATION);
		let view = quarantine.map_or(ConnectionView::Stopped, |quarantine| {
			ConnectionView::Quarantined { reason: quarantine.reason, recovery: quarantine.recovery }
		});

		Ok(Self {
			config,
			server_id,
			cache_parent,
			cache_limits,
			cache_authority,
			cache,
			health_query: HealthQuery::production(),
			history_pager,
			quick_tasks: QuickTasks::production(),
			state: HashMap::new(),
			last_cursor: None,
			binding: None,
			quarantine,
			connection_generation: 0,
			view,
			view_observer: None,
			cancellation: LifecycleCancellation::new(),
		})
	}

	/// Current narrow shell-facing state.
	pub(crate) const fn view(&self) -> ConnectionView {
		self.view
	}

	/// Observe every subsequent shell-facing state transition from one bounded run.
	pub(crate) fn observe_views(&mut self) -> Receiver<ConnectionView> {
		let (sender, receiver) = mpsc::channel();
		let _ = sender.send(self.view);
		self.view_observer = Some(sender);

		receiver
	}

	/// Cooperative handle that can terminate the caller-owned run future.
	pub(crate) fn cancellation(&self) -> LifecycleCancellation {
		self.cancellation.clone()
	}

	/// Clone the presentation-neutral history controller before moving the lifecycle task.
	#[cfg_attr(
		not(test),
		allow(
			dead_code,
			reason = "XY-1429 exposes this handle for the later Conversation destination"
		)
	)]
	pub(crate) fn history_pager(&self) -> HistoryPager {
		self.history_pager.clone()
	}

	/// Clone the presentation-neutral Health controller before moving the lifecycle task.
	pub(crate) fn health_query(&self) -> HealthQuery {
		self.health_query.clone()
	}

	/// Clone the presentation-neutral ordinary Quick Tasks controller.
	pub(crate) fn quick_tasks(&self) -> QuickTasks {
		self.quick_tasks.clone()
	}

	/// Run the bounded lifecycle without spawning or detaching any work.
	pub(crate) async fn run(&mut self) -> RunResult {
		self.run_with_io(&mut TokioIo { session: None }).await
	}

	async fn run_with_io<I>(&mut self, io: &mut I) -> RunResult
	where
		I: LifecycleIo,
	{
		if self.cache.is_none() {
			return RunResult::Quarantined;
		}
		if self.cancellation.is_cancelled() {
			return self.finish_shutdown().await;
		}

		for attempt in 1..=MAX_CONNECTION_ATTEMPTS {
			let generation = match self.begin_attempt(attempt) {
				Ok(generation) => generation,
				Err(()) => return RunResult::Quarantined,
			};
			let checkpoint = self.reusable_checkpoint();
			let requested_checkpoint = checkpoint.clone();
			let connected_checkpoint =
				io.connect(&self.config, checkpoint, &self.cancellation).await;

			if self.ensure_generation(generation).is_err() {
				return RunResult::Quarantined;
			}

			let connected_checkpoint = match connected_checkpoint {
				Ok(checkpoint) => checkpoint,
				Err(failure) => {
					if let Some(result) = self.closed_failure(failure) {
						return result;
					}

					if !self.retry(io, attempt, generation).await {
						return self.retry_terminal(attempt);
					}

					continue;
				},
			};

			self.connection_established(
				generation,
				requested_checkpoint.is_some(),
				connected_checkpoint.as_ref(),
			);
			self.health_query.bind_session(generation, self.server_id.clone());
			self.history_pager.bind_session(generation, self.server_id.clone());
			self.quick_tasks.bind_session(generation, self.server_id.clone());
			let failure =
				self.run_connected_session(io, generation, connected_checkpoint.is_none()).await;

			self.health_query.session_ended(generation);
			self.history_pager.session_ended(generation);
			self.quick_tasks.session_ended(generation);
			if self.quarantine.is_none() {
				self.set_view(ConnectionView::ShuttingDown);
			}
			let _ = io.close().await;

			if self.ensure_generation(generation).is_err() {
				return RunResult::Quarantined;
			}
			if let Some(result) = self.closed_failure(failure) {
				return result;
			}
			if !self.retry(io, attempt, generation).await {
				return self.retry_terminal(attempt);
			}
		}

		if self.quarantine.is_some() {
			RunResult::Quarantined
		} else {
			self.set_view(ConnectionView::Stopped);

			RunResult::RetryExhausted
		}
	}

	async fn run_connected_session<I>(
		&mut self,
		io: &mut I,
		generation: u64,
		mut requires_snapshot: bool,
	) -> RetainedSessionFailure
	where
		I: LifecycleIo,
	{
		loop {
			let health_query = self.health_query.clone();
			let history_pager = self.history_pager.clone();
			let quick_tasks = self.quick_tasks.clone();
			let server_id = self.server_id.clone();
			let step = tokio::select! {
				delivery = io.next() => SessionStep::Delivery(delivery),
				dispatch = health_query.next_dispatch(generation, &server_id),
					if !requires_snapshot => SessionStep::Health(dispatch),
				dispatch = history_pager.next_dispatch(generation, &server_id),
					if !requires_snapshot => SessionStep::History(dispatch),
				dispatch = quick_tasks.next_dispatch(generation, &server_id),
					if !requires_snapshot => SessionStep::QuickTask(dispatch),
			};

			if self.ensure_generation(generation).is_err() {
				return RetainedSessionFailure::PublicationOrder;
			}

			match step {
				SessionStep::Health(dispatch) => {
					if let Err(failure) = io.send_query(dispatch.envelope().clone()).await {
						return failure;
					}
				},
				SessionStep::History(dispatch) => {
					let Some(send_token) = self.history_pager.begin_send(&dispatch) else {
						continue;
					};
					let send_result = io.send_query(dispatch.envelope().clone()).await;

					let remains_current = self.history_pager.finish_send(&send_token);
					if let Err(failure) = send_result {
						return failure;
					}
					if remains_current {
						self.history_pager.lookup_sent_request(&send_token);
					}
				},
				SessionStep::QuickTask(dispatch) =>
					if let Some(command) = dispatch.command() {
						let send_result = io.send_command(command.clone()).await;
						if let Err(failure) = send_result {
							self.quick_tasks.command_send_failed(&dispatch);
							return failure;
						}
						self.quick_tasks.command_sent(&dispatch);
					} else if let Some(query) = dispatch.query()
						&& let Err(failure) = io.send_query(query.clone()).await
					{
						return failure;
					},
				SessionStep::Delivery(delivery) => match delivery {
					Ok(Delivery::Snapshot { snapshot, confirmation }) => {
						let cursor = snapshot.cursor;
						let inspection = match self.apply_snapshot(generation, snapshot) {
							Ok(inspection) => inspection,
							Err(failure) => return failure,
						};
						let checkpoint = match io.confirm_applied(confirmation) {
							Ok(checkpoint) => checkpoint,
							Err(_) => return self.confirmation_failure(),
						};

						if self.bind_checkpoint(generation, cursor, checkpoint, inspection).is_err()
						{
							return RetainedSessionFailure::ApplicationConfirmationMismatch;
						}
						requires_snapshot = false;
					},
					Ok(Delivery::Event { event, confirmation }) => {
						if requires_snapshot {
							self.enter_quarantine(
								QuarantineReason::ApplicationOrder,
								QuarantineRecovery::VerifiedSnapshotReplacement,
							);

							return RetainedSessionFailure::PublicationOrder;
						}
						let cursor = event.cursor;
						let quick_task_event = event.clone();
						let inspection = match self.apply_event(generation, event) {
							Ok(inspection) => inspection,
							Err(failure) => return failure,
						};
						if let EventPayload::QuickTaskTurnFinished { conversation, .. } =
							&quick_task_event.payload
						{
							let _ =
								self.history_pager.reload_if_open(&conversation.conversation_id);
						}
						self.quick_tasks.apply_event(&quick_task_event);
						let checkpoint = match io.confirm_applied(confirmation) {
							Ok(checkpoint) => checkpoint,
							Err(_) => return self.confirmation_failure(),
						};

						if self.bind_checkpoint(generation, cursor, checkpoint, inspection).is_err()
						{
							return RetainedSessionFailure::ApplicationConfirmationMismatch;
						}
					},
					Ok(Delivery::QueryResult(result)) => {
						if let Err(failure) = self.route_query_result(generation, result) {
							return failure;
						}
					},
					Ok(Delivery::CommandReceipt(receipt)) => {
						let _ =
							self.quick_tasks.route_receipt(generation, &self.server_id, &receipt);
					},
					Ok(Delivery::CommandResult(result)) => {
						let _ = self.quick_tasks.route_command_result(
							generation,
							&self.server_id,
							&result,
						);
					},
					Err(failure) => return failure,
				},
			}
		}
	}

	fn begin_attempt(&mut self, attempt: u8) -> Result<u64, ()> {
		self.connection_generation =
			self.connection_generation.checked_add(1).ok_or_else(|| {
				self.enter_quarantine(
					QuarantineReason::StaleConnectionGeneration,
					QuarantineRecovery::OperatorRequired,
				);
			})?;
		if self.quarantine.is_none() {
			self.set_view(ConnectionView::Connecting { attempt });
		}

		Ok(self.connection_generation)
	}

	fn connection_established(
		&mut self,
		generation: u64,
		requested_checkpoint: bool,
		connected_checkpoint: Option<&SessionCheckpoint>,
	) {
		if requested_checkpoint && connected_checkpoint.is_none() {
			self.enter_quarantine(
				QuarantineReason::PublicationInstanceChanged,
				QuarantineRecovery::VerifiedSnapshotReplacement,
			);
		}
		if self.quarantine.is_none() {
			self.set_view(ConnectionView::Online {
				generation,
				applied: connected_checkpoint.map(SessionCheckpoint::cursor),
			});
		}
	}

	fn reusable_checkpoint(&mut self) -> Option<SessionCheckpoint> {
		let binding = self.binding.clone()?;
		let inspection = self.cache.as_ref()?.inspect_current();

		match inspection {
			Ok(Some(inspection))
				if inspection.generation == binding.generation
					&& inspection.authority == self.cache_authority =>
				Some(binding.checkpoint),
			_ => {
				self.enter_quarantine(
					QuarantineReason::ContentAttestation,
					QuarantineRecovery::VerifiedSnapshotReplacement,
				);

				None
			},
		}
	}

	fn route_history_result(
		&mut self,
		generation: u64,
		result: QueryResultEnvelope,
	) -> Result<(), RetainedSessionFailure> {
		match self.history_pager.route_result(generation, &self.server_id, result) {
			HistoryRouteOutcome::Fresh => {
				let snapshot = self.history_pager.snapshot();
				if let (Some(conversation_id), Some(page)) =
					(snapshot.conversation_id.as_ref(), snapshot.visible.as_ref())
				{
					self.quick_tasks.reconcile_durable_history(conversation_id, page);
				}
				Ok(())
			},
			HistoryRouteOutcome::Unavailable
			| HistoryRouteOutcome::Closed
			| HistoryRouteOutcome::Stale
			| HistoryRouteOutcome::Unmatched
			| HistoryRouteOutcome::ProtocolMismatch => Ok(()),
		}
	}

	fn route_query_result(
		&mut self,
		generation: u64,
		result: QueryResultEnvelope,
	) -> Result<(), RetainedSessionFailure> {
		match self.quick_tasks.route_query_result(generation, &self.server_id, &result) {
			QuickTaskRouteOutcome::Fresh | QuickTaskRouteOutcome::Refused => return Ok(()),
			QuickTaskRouteOutcome::Unmatched => {},
		}
		match self.health_query.route_result(generation, &self.server_id, &result) {
			HealthRouteOutcome::Unmatched => self.route_history_result(generation, result),
			HealthRouteOutcome::Fresh | HealthRouteOutcome::Refused | HealthRouteOutcome::Stale =>
				Ok(()),
		}
	}

	fn apply_snapshot(
		&mut self,
		generation: u64,
		snapshot: SnapshotEnvelope,
	) -> Result<GenerationInspection, RetainedSessionFailure> {
		self.ensure_generation(generation)?;
		if snapshot.version != CURRENT_VERSION || snapshot.server_id != self.server_id {
			return self.application_failure(QuarantineReason::ApplicationOrder);
		}

		let mut seen = HashSet::new();
		let mut next_state = HashMap::new();
		let mut facts = Vec::with_capacity(snapshot.items.len());

		for item in snapshot.items {
			let (entity_id, revision) = snapshot_identity(&item);
			if !seen.insert(entity_id.as_str().to_owned()) {
				return self.application_failure(QuarantineReason::ApplicationOrder);
			}
			let bytes = serde_json::to_vec(&item).map_err(|_| RetainedSessionFailure::Malformed)?;

			next_state.insert(
				entity_id.as_str().to_owned(),
				AppliedEntity { entity_id: entity_id.clone(), revision, bytes: bytes.clone() },
			);
			facts.push(CachedFact { entity_id, revision, bytes });
		}

		let inspection = self.publish(&facts)?;

		self.state = next_state;
		self.last_cursor = Some(snapshot.cursor);

		Ok(inspection)
	}

	fn apply_event(
		&mut self,
		generation: u64,
		event: EventEnvelope,
	) -> Result<GenerationInspection, RetainedSessionFailure> {
		self.ensure_generation(generation)?;
		if event.version != CURRENT_VERSION
			|| event.server_id != self.server_id
			|| self.last_cursor.and_then(next_cursor) != Some(event.cursor)
			|| self
				.state
				.get(event.entity_id.as_str())
				.is_some_and(|current| event.entity_revision <= current.revision)
		{
			return self.application_failure(QuarantineReason::ApplicationOrder);
		}

		let bytes =
			serde_json::to_vec(&event.payload).map_err(|_| RetainedSessionFailure::Malformed)?;
		let mut next_state = self.state.clone();

		next_state.insert(
			event.entity_id.as_str().to_owned(),
			AppliedEntity { entity_id: event.entity_id, revision: event.entity_revision, bytes },
		);
		let facts = next_state
			.values()
			.map(|entity| CachedFact {
				entity_id: entity.entity_id.clone(),
				revision: entity.revision,
				bytes: entity.bytes.clone(),
			})
			.collect::<Vec<_>>();
		let inspection = self.publish(&facts)?;

		self.state = next_state;
		self.last_cursor = Some(event.cursor);

		Ok(inspection)
	}

	fn publish(
		&mut self,
		facts: &[CachedFact],
	) -> Result<GenerationInspection, RetainedSessionFailure> {
		let inputs = facts
			.iter()
			.map(|fact| {
				ObjectInput::new(
					&fact.entity_id,
					fact.revision,
					&fact.bytes,
					ObjectCertainty::Authoritative,
				)
			})
			.collect::<Vec<_>>();
		let result =
			self.cache.as_ref().ok_or(RetainedSessionFailure::Closed)?.publish(&inputs, &[]);

		match result {
			Ok(inspection) => Ok(inspection),
			Err(error) => {
				self.handle_cache_failure(error);

				Err(RetainedSessionFailure::Malformed)
			},
		}
	}

	fn bind_checkpoint(
		&mut self,
		generation: u64,
		cursor: Cursor,
		checkpoint: SessionCheckpoint,
		inspection: GenerationInspection,
	) -> Result<(), RetainedSessionFailure> {
		self.ensure_generation(generation)?;
		if checkpoint.server_id() != &self.server_id
			|| checkpoint.cursor() != cursor
			|| inspection.authority != self.cache_authority
			|| self
				.cache
				.as_ref()
				.and_then(|cache| cache.inspect_current().ok().flatten())
				.as_ref()
				.is_none_or(|current| current.generation != inspection.generation)
		{
			return self.application_failure(QuarantineReason::ContentAttestation);
		}

		self.binding = Some(CacheBinding { checkpoint, generation: inspection.generation });
		self.quarantine = None;
		self.set_view(ConnectionView::Online { generation, applied: Some(cursor) });

		Ok(())
	}

	fn ensure_generation(&mut self, generation: u64) -> Result<(), RetainedSessionFailure> {
		if generation == self.connection_generation {
			return Ok(());
		}

		self.enter_quarantine(
			QuarantineReason::StaleConnectionGeneration,
			QuarantineRecovery::VerifiedSnapshotReplacement,
		);

		Err(RetainedSessionFailure::PublicationOrder)
	}

	fn application_failure<T>(
		&mut self,
		reason: QuarantineReason,
	) -> Result<T, RetainedSessionFailure> {
		self.enter_quarantine(reason, QuarantineRecovery::VerifiedSnapshotReplacement);

		Err(RetainedSessionFailure::PublicationOrder)
	}

	fn confirmation_failure(&mut self) -> RetainedSessionFailure {
		self.enter_quarantine(
			QuarantineReason::ApplicationConfirmation,
			QuarantineRecovery::VerifiedSnapshotReplacement,
		);

		RetainedSessionFailure::ApplicationConfirmationMismatch
	}

	fn enter_quarantine(&mut self, reason: QuarantineReason, recovery: QuarantineRecovery) {
		self.binding = None;
		self.quarantine = Some(Quarantine { reason, recovery });
		self.set_view(ConnectionView::Quarantined { reason, recovery });
	}

	fn handle_cache_failure(&mut self, error: CacheError) {
		let cache_root = self.cache_parent.join("client-cache");
		let recovery = if is_disposable_corruption(error)
			&& ClientCache::dispose_all(&cache_root).is_ok()
		{
			match ClientCache::open(&cache_root, self.cache_limits, self.cache_authority.clone()) {
				Ok(cache) => {
					self.cache = Some(cache);

					QuarantineRecovery::DisposedBeforeRebuild
				},
				Err(_) => {
					self.cache = None;

					QuarantineRecovery::OperatorRequired
				},
			}
		} else {
			self.cache = None;

			QuarantineRecovery::OperatorRequired
		};

		self.enter_quarantine(QuarantineReason::CacheCorrupt, recovery);
	}

	fn closed_failure(&mut self, failure: RetainedSessionFailure) -> Option<RunResult> {
		match failure {
			RetainedSessionFailure::Cancelled => Some(self.finish_shutdown_now()),
			RetainedSessionFailure::LocalTransportDisabled
			| RetainedSessionFailure::RemoteTransportDisabled
			| RetainedSessionFailure::LocalTransportUnsupported
			| RetainedSessionFailure::UnsafeLocalEndpoint
			| RetainedSessionFailure::LocalPeerIdentityUnavailable
			| RetainedSessionFailure::LocalPeerUidMismatch => {
				self.set_view(ConnectionView::Incompatible(CompatibilityReason::InvalidEndpoint));

				Some(RunResult::Incompatible)
			},
			RetainedSessionFailure::ProtocolMajorMismatch => {
				self.set_view(ConnectionView::Incompatible(CompatibilityReason::ProtocolMajor));

				Some(RunResult::Incompatible)
			},
			RetainedSessionFailure::ProtocolMinorMismatch => {
				self.set_view(ConnectionView::Incompatible(CompatibilityReason::ProtocolMinor));

				Some(RunResult::Incompatible)
			},
			RetainedSessionFailure::PublicationIdentityUnavailable => {
				self.set_view(ConnectionView::Incompatible(
					CompatibilityReason::PublicationIdentityUnavailable,
				));

				Some(RunResult::Incompatible)
			},
			RetainedSessionFailure::ServerIdentityMismatch => {
				self.enter_quarantine(
					QuarantineReason::StableServerIdentity,
					QuarantineRecovery::OperatorRequired,
				);

				Some(RunResult::Quarantined)
			},
			RetainedSessionFailure::CheckpointIdentityMismatch => {
				self.enter_quarantine(
					QuarantineReason::CheckpointMismatch,
					QuarantineRecovery::VerifiedSnapshotReplacement,
				);

				None
			},
			RetainedSessionFailure::Malformed
			| RetainedSessionFailure::ProtocolViolation
			| RetainedSessionFailure::PublicationOrder
			| RetainedSessionFailure::ApplicationConfirmationRequired
			| RetainedSessionFailure::ApplicationConfirmationMismatch => {
				if self.quarantine.is_none() {
					self.enter_quarantine(
						QuarantineReason::ApplicationOrder,
						QuarantineRecovery::VerifiedSnapshotReplacement,
					);
				}

				None
			},
			RetainedSessionFailure::OperationTimeout
			| RetainedSessionFailure::Closed
			| RetainedSessionFailure::Disconnected
			| RetainedSessionFailure::Backpressure => None,
		}
	}

	async fn retry<I>(&mut self, io: &mut I, attempt: u8, generation: u64) -> bool
	where
		I: LifecycleIo,
	{
		if attempt >= MAX_CONNECTION_ATTEMPTS {
			return false;
		}
		let delay = RETRY_DELAYS[usize::from(attempt - 1)];

		if self.quarantine.is_none() {
			self.set_view(ConnectionView::OfflineRetrying { next_attempt: attempt + 1, delay });
		}

		io.backoff(delay, &self.cancellation).await.is_ok()
			&& self.ensure_generation(generation).is_ok()
	}

	fn retry_terminal(&mut self, attempt: u8) -> RunResult {
		if self.cancellation.is_cancelled() {
			return self.finish_shutdown_now();
		}
		if self.quarantine.is_some() {
			return RunResult::Quarantined;
		}
		if attempt >= MAX_CONNECTION_ATTEMPTS {
			self.set_view(ConnectionView::Stopped);

			return RunResult::RetryExhausted;
		}

		RunResult::Quarantined
	}

	async fn finish_shutdown(&mut self) -> RunResult {
		self.finish_shutdown_now()
	}

	fn finish_shutdown_now(&mut self) -> RunResult {
		if self.quarantine.is_some() {
			return RunResult::Quarantined;
		}
		self.set_view(ConnectionView::ShuttingDown);
		self.set_view(ConnectionView::Stopped);

		RunResult::Stopped
	}

	fn set_view(&mut self, view: ConnectionView) {
		self.view = view;
		if let Some(observer) = &self.view_observer {
			let _ = observer.send(view);
		}
	}
}

fn production_cache_parent(os_temp_dir: &Path) -> Result<PathBuf, LifecycleBuildError> {
	#[cfg(target_os = "macos")]
	let platform_temp_dir = normalize_macos_var_prefix(os_temp_dir, validate_macos_var_mapping)?;
	#[cfg(not(target_os = "macos"))]
	let platform_temp_dir = os_temp_dir.to_path_buf();

	Ok(platform_temp_dir.join("box.acg.decodex"))
}

#[cfg(target_os = "macos")]
fn normalize_macos_var_prefix(
	os_temp_dir: &Path,
	validate_var_mapping: fn() -> Result<(), CacheError>,
) -> Result<PathBuf, LifecycleBuildError> {
	let Ok(relative_temp_dir) = os_temp_dir.strip_prefix("/var") else {
		return Ok(os_temp_dir.to_path_buf());
	};

	validate_var_mapping().map_err(LifecycleBuildError::Cache)?;

	Ok(Path::new("/private/var").join(relative_temp_dir))
}

#[cfg(target_os = "macos")]
fn validate_macos_var_mapping() -> Result<(), CacheError> {
	use std::os::unix::fs::MetadataExt as _;

	let alias_metadata = std::fs::symlink_metadata("/var")?;
	if alias_metadata.uid() != 0
		|| !alias_metadata.file_type().is_symlink()
		|| std::fs::read_link("/var")? != Path::new("private/var")
	{
		return Err(CacheError::UnsafeRoot);
	}

	let physical_metadata = std::fs::symlink_metadata("/private/var")?;
	if physical_metadata.uid() != 0
		|| !physical_metadata.is_dir()
		|| physical_metadata.mode() & 0o022 != 0
	{
		return Err(CacheError::UnsafeRoot);
	}

	Ok(())
}

fn initialize_cache(
	root: &Path,
	limits: CacheLimits,
	authority: CacheAuthority,
) -> (Option<ClientCache>, Option<Quarantine>) {
	match ClientCache::open(root, limits, authority.clone()) {
		Ok(cache) => (Some(cache), None),
		Err(CacheError::AuthorityMismatch) => {
			match ClientCache::prepare_authority_switch(root, limits, authority) {
				Ok(cache) => (
					Some(cache),
					Some(Quarantine {
						reason: QuarantineReason::AuthorityChanged,
						recovery: QuarantineRecovery::VerifiedSnapshotReplacement,
					}),
				),
				Err(_) => unsafe_cache_quarantine(),
			}
		},
		Err(error) if is_disposable_corruption(error) =>
			if ClientCache::dispose_all(root).is_ok() {
				match ClientCache::open(root, limits, authority) {
					Ok(cache) => (
						Some(cache),
						Some(Quarantine {
							reason: QuarantineReason::CacheCorrupt,
							recovery: QuarantineRecovery::DisposedBeforeRebuild,
						}),
					),
					Err(_) => unsafe_cache_quarantine(),
				}
			} else {
				unsafe_cache_quarantine()
			},
		Err(_) => unsafe_cache_quarantine(),
	}
}

fn unsafe_cache_quarantine() -> (Option<ClientCache>, Option<Quarantine>) {
	(
		None,
		Some(Quarantine {
			reason: QuarantineReason::CacheRootUnsafe,
			recovery: QuarantineRecovery::OperatorRequired,
		}),
	)
}

fn is_disposable_corruption(error: CacheError) -> bool {
	matches!(
		error,
		CacheError::CrashRemnant
			| CacheError::InvalidMetadata
			| CacheError::IntegrityMismatch
			| CacheError::OrphanGeneration
	)
}

fn snapshot_identity(item: &SnapshotItem) -> (EntityId, EntityRevision) {
	match item {
		SnapshotItem::SystemState { entity_id, revision, .. } => (entity_id.clone(), *revision),
	}
}

fn next_cursor(cursor: Cursor) -> Option<Cursor> {
	cursor.0.checked_add(1).map(Cursor)
}

#[cfg(test)] mod tests;
