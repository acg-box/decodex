//! Presentation-neutral ownership of the ordinary Quick Tasks destination.

use std::{
	collections::VecDeque,
	sync::{
		Arc, Mutex, MutexGuard,
		atomic::{AtomicU64, Ordering},
	},
	time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use decodex_protocol::{
	CURRENT_VERSION, CausationId, ClientCommandId, CommandEnvelope, CommandError, CommandOutcome,
	CommandPayload, CommandReceipt, CommandResultEnvelope, ConversationHistoryPage, CorrelationId,
	EntityId, EntityRevision, EventEnvelope, EventPayload, HistoryText, IdempotencyKey,
	QueryEnvelope, QueryId, QueryPayload, QueryResultEnvelope, QueryResultPayload,
	QuickTaskExecutionSettings, QuickTaskListCursor, QuickTaskListResult, QuickTaskListSize,
	QuickTaskModel, QuickTaskReasoningEffort, QuickTaskRecoveryAction, QuickTaskResult,
	QuickTaskState, QuickTaskSummary, QuickTaskWorkingDirectory, ReceiptDisposition, ResultPayload,
	ServerId,
};

const MAX_LIVE_DELTAS: usize = 64;
const MAX_LIVE_DELTA_BYTES: usize = 64 * 1_024;
const MAX_LIST_PAGES: usize = 32;
const QUICK_TASK_WORKING_DIRECTORY_ENV: &str = "DECODEX_QUICK_TASK_WORKING_DIRECTORY";
const QUICK_TASK_MODELS: &[&str] =
	&["gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna", "gpt-5.5", "gpt-5.4"];
const QUICK_TASK_EFFORTS: &[QuickTaskReasoningEffort] = &[
	QuickTaskReasoningEffort::Low,
	QuickTaskReasoningEffort::Medium,
	QuickTaskReasoningEffort::High,
	QuickTaskReasoningEffort::XHigh,
	QuickTaskReasoningEffort::Max,
	QuickTaskReasoningEffort::Ultra,
];

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Current bounded state rendered by the Quick Tasks destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QuickTasksSnapshot {
	pub(crate) load: QuickTasksLoadState,
	pub(crate) command: QuickTaskCommandState,
	/// Monotonic terminal result for Create/Submit commands only.
	pub(crate) submission_result_generation: u64,
	pub(crate) last_submission_accepted: bool,
	pub(crate) refresh: QuickTaskRefreshState,
	pub(crate) tasks: Vec<QuickTaskSummary>,
	pub(crate) selected: Option<EntityId>,
	pub(crate) live_deltas: Vec<QuickTaskLiveDelta>,
	pub(crate) can_submit: bool,
	pub(crate) execution: QuickTaskExecutionSettings,
}

impl QuickTasksSnapshot {
	pub(crate) fn selected_task(&self) -> Option<&QuickTaskSummary> {
		let selected = self.selected.as_ref()?;
		self.tasks.iter().find(|task| &task.conversation_id == selected)
	}
}

/// Finite list/readback state. The daemon-owned product store remains authoritative.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickTasksLoadState {
	NeverRequested,
	Loading,
	Ready,
	Offline,
	Unavailable,
	Refused,
}

/// Finite state for the one possibly side-effecting command owned by the destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickTaskCommandState {
	Idle,
	Sending,
	AwaitingResult,
	Accepted,
	ManualRecovery(QuickTaskRecoveryAction),
	OutcomeUnknown,
	Refused,
}

/// Bounded progress for an explicit sidebar-wide provider reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickTaskRefreshState {
	Idle,
	Refreshing { completed: usize, total: usize, archived: usize, failed: usize },
	Complete { checked: usize, archived: usize, failed: usize },
	Stopped { checked: usize, total: usize, archived: usize, failed: usize },
}

/// One bounded normalized assistant delta already persisted by the daemon.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QuickTaskLiveDelta {
	pub(crate) history_item_id: EntityId,
	pub(crate) conversation_id: EntityId,
	pub(crate) turn_id: EntityId,
	pub(crate) text: HistoryText,
}

/// Exact local identity of one user submission queued for transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct QueuedQuickTaskSubmission {
	pub(crate) conversation_id: EntityId,
	/// Later turns carry their caller-selected identity. Initial-turn identity remains
	/// daemon-owned.
	pub(crate) turn_id: Option<EntityId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickTaskInputError {
	Offline,
	Busy,
	InvalidMessage,
	NoSelection,
	NotReady,
	NotInterruptible,
	IdentityUnavailable,
	WorkingDirectoryUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuickTaskRouteOutcome {
	Fresh,
	Unmatched,
	Refused,
}

/// Exactly one query or command reserved for one retained-session generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum QuickTaskDispatch {
	Query(QueryEnvelope),
	Command(CommandEnvelope),
}

impl QuickTaskDispatch {
	pub(crate) fn query(&self) -> Option<&QueryEnvelope> {
		match self {
			Self::Query(envelope) => Some(envelope),
			Self::Command(_) => None,
		}
	}

	pub(crate) fn command(&self) -> Option<&CommandEnvelope> {
		match self {
			Self::Command(envelope) => Some(envelope),
			Self::Query(_) => None,
		}
	}
}

/// Cloneable controller. It owns no task, transport, cache, or product authority.
#[derive(Clone)]
pub(crate) struct QuickTasks {
	inner: Arc<QuickTasksInner>,
}

struct QuickTasksInner {
	state: Mutex<State>,
	notify: Notify,
	working_directory: Option<QuickTaskWorkingDirectory>,
}

impl QuickTasks {
	pub(crate) fn production() -> Self {
		let configured_working_directory = std::env::var(QUICK_TASK_WORKING_DIRECTORY_ENV).ok();
		let home = std::env::var("HOME").ok();
		Self {
			inner: Arc::new(QuickTasksInner {
				state: Mutex::new(State::new()),
				notify: Notify::new(),
				working_directory: production_working_directory(configured_working_directory, home),
			}),
		}
	}

	pub(crate) fn activate(&self) {
		let mut state = self.lock();
		state.active = true;
		let queued = state.queue_list();
		if state.session.is_none() {
			state.load = QuickTasksLoadState::Offline;
		}
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn deactivate(&self) {
		self.lock().active = false;
	}

	pub(crate) fn snapshot(&self) -> QuickTasksSnapshot {
		self.lock().snapshot()
	}

	pub(crate) fn select(&self, conversation_id: EntityId) -> bool {
		let mut state = self.lock();
		if !state.tasks.iter().any(|task| task.conversation_id == conversation_id) {
			return false;
		}
		state.selected = Some(conversation_id);
		state.selection_suppressed = false;
		true
	}

	/// Insert one verified command result and make its Conversation current.
	pub(crate) fn adopt_and_select(&self, conversation: QuickTaskSummary) {
		let mut state = self.lock();
		let conversation_id = conversation.conversation_id.clone();
		state.upsert_task(conversation);
		state.selected = Some(conversation_id);
		state.selection_suppressed = false;
	}

	/// Select an existing Conversation after authoritative list readback finds it.
	pub(crate) fn select_when_available(&self, conversation_id: EntityId) {
		let mut state = self.lock();
		if state.tasks.iter().any(|task| task.conversation_id == conversation_id) {
			state.selected = Some(conversation_id);
			state.selection_suppressed = false;
			return;
		}
		state.requested_selection = Some(conversation_id);
		state.selection_suppressed = false;
		let queued = state.queue_list();
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn begin_new(&self) {
		let mut state = self.lock();
		state.selected = None;
		state.selection_suppressed = true;
		if state.pending_command.is_none()
			&& state.in_flight_command.is_none()
			&& state.command != QuickTaskCommandState::OutcomeUnknown
		{
			state.command = QuickTaskCommandState::Idle;
		}
	}

	pub(crate) fn cycle_model(&self) {
		let mut state = self.lock();
		let current = state.execution.model.as_str();
		let next = QUICK_TASK_MODELS
			.iter()
			.position(|model| *model == current)
			.map_or(0, |index| (index + 1) % QUICK_TASK_MODELS.len());
		state.execution.model = QuickTaskModel::new(QUICK_TASK_MODELS[next])
			.expect("curated model identifier is valid");
		state.clamp_effort_for_model();
	}

	pub(crate) fn cycle_reasoning_effort(&self) {
		let mut state = self.lock();
		let supported = supported_efforts(state.execution.model.as_str());
		let next = supported
			.iter()
			.position(|effort| *effort == state.execution.reasoning_effort)
			.map_or(0, |index| (index + 1) % supported.len());
		state.execution.reasoning_effort = supported[next];
	}

	pub(crate) fn toggle_fast(&self) {
		let mut state = self.lock();
		state.execution.fast = !state.execution.fast;
	}

	/// Re-observe the selected provider thread by exact ID.
	pub(crate) fn refresh_selected(&self) -> Result<(), QuickTaskInputError> {
		let state = self.lock();
		let selected = state.selected_task().cloned();
		drop(state);
		let task = selected.ok_or(QuickTaskInputError::NoSelection)?;
		if !task_needs_reconciliation(&task) {
			return Err(QuickTaskInputError::NotReady);
		}
		self.queue_command(
			CommandPayload::RefreshQuickTask { conversation_id: task.conversation_id },
			Some(task.conversation_revision),
			None,
		)
	}

	/// Reconcile every local provider-backed conversation against Codex, one at a time.
	pub(crate) fn refresh_all(&self) -> Result<(), QuickTaskInputError> {
		let mut state = self.lock();
		if state.session.is_none() {
			return Err(QuickTaskInputError::Offline);
		}
		if state.refresh_batch.is_some()
			|| state.pending_command.is_some()
			|| state.in_flight_command.is_some()
			|| state.in_flight_query.is_some()
			|| state.command == QuickTaskCommandState::OutcomeUnknown
		{
			return Err(QuickTaskInputError::Busy);
		}
		if state.load != QuickTasksLoadState::Ready {
			return Err(QuickTaskInputError::NotReady);
		}

		// A queued list has not left this controller, so supersede it with the final
		// list readback that follows the exact provider observations below.
		state.reset_pagination();
		let mut commands = state
			.tasks
			.iter()
			.filter(|task| task_needs_reconciliation(task))
			.map(|task| {
				pending_command(
					CommandPayload::RefreshQuickTask {
						conversation_id: task.conversation_id.clone(),
					},
					Some(task.conversation_revision),
					None,
				)
			})
			.collect::<Result<VecDeque<_>, _>>()?;
		let total = commands.len();
		if total == 0 {
			state.refresh = QuickTaskRefreshState::Complete { checked: 0, archived: 0, failed: 0 };
			state.next_list_cursor = None;
			let queued = state.queue_list();
			drop(state);
			if queued {
				self.inner.notify.notify_one();
			}
			return Ok(());
		}

		state.pending_command = commands.pop_front();
		state.refresh_batch = Some(RefreshBatch {
			remaining: commands,
			completed: 0,
			total,
			archived: 0,
			failed: 0,
			reloading: false,
		});
		state.refresh =
			QuickTaskRefreshState::Refreshing { completed: 0, total, archived: 0, failed: 0 };
		state.command = QuickTaskCommandState::Sending;
		drop(state);
		self.inner.notify.notify_one();
		Ok(())
	}

	pub(crate) fn archive_selected(&self) -> Result<(), QuickTaskInputError> {
		let state = self.lock();
		let task = state.selected_task().ok_or(QuickTaskInputError::NoSelection)?.clone();
		drop(state);
		if task.state != QuickTaskState::Ready && !task_has_safe_stale_reconciliation(&task) {
			return Err(QuickTaskInputError::NotReady);
		}
		self.queue_command(
			CommandPayload::ArchiveQuickTask { conversation_id: task.conversation_id },
			Some(task.conversation_revision),
			None,
		)
	}

	pub(crate) fn create(
		&self,
		message: &str,
	) -> Result<QueuedQuickTaskSubmission, QuickTaskInputError> {
		let working_directory = self
			.inner
			.working_directory
			.clone()
			.ok_or(QuickTaskInputError::WorkingDirectoryUnavailable)?;
		self.create_with_cause(message, None, working_directory)
	}

	/// Create the one ordinary Quick Task that executes an exact persisted Program WorkItem.
	pub(crate) fn create_for_program_work_item(
		&self,
		message: &str,
		work_item_id: EntityId,
		working_directory: QuickTaskWorkingDirectory,
	) -> Result<QueuedQuickTaskSubmission, QuickTaskInputError> {
		self.create_with_cause(message, Some(work_item_id), working_directory)
	}

	fn create_with_cause(
		&self,
		message: &str,
		work_item_id: Option<EntityId>,
		working_directory: QuickTaskWorkingDirectory,
	) -> Result<QueuedQuickTaskSubmission, QuickTaskInputError> {
		let conversation_id = entity_id()?;
		let payload = CommandPayload::CreateQuickTask {
			conversation_id: conversation_id.clone(),
			work_item_id,
			message: message_text(message)?,
			working_directory,
			execution: self.lock().execution.clone(),
		};
		self.queue_command(payload, None, Some(conversation_id.clone()))?;
		Ok(QueuedQuickTaskSubmission { conversation_id, turn_id: None })
	}

	pub(crate) fn submit(
		&self,
		message: &str,
	) -> Result<QueuedQuickTaskSubmission, QuickTaskInputError> {
		let state = self.lock();
		if state.command == QuickTaskCommandState::OutcomeUnknown
			|| state.pending_command.is_some()
			|| state.in_flight_command.is_some()
		{
			return Err(QuickTaskInputError::Busy);
		}
		let task = state.selected_task().ok_or(QuickTaskInputError::NoSelection)?.clone();
		if !task_accepts_turn(&task) {
			return Err(QuickTaskInputError::NotReady);
		}
		drop(state);
		let turn_id = entity_id()?;
		let message = message_text(message)?;
		let working_directory = self
			.inner
			.working_directory
			.clone()
			.ok_or(QuickTaskInputError::WorkingDirectoryUnavailable)?;
		let submission = QueuedQuickTaskSubmission {
			conversation_id: task.conversation_id.clone(),
			turn_id: Some(turn_id.clone()),
		};
		let payload = CommandPayload::SubmitQuickTaskTurn {
			conversation_id: task.conversation_id,
			turn_id,
			message,
			working_directory,
			execution: self.lock().execution.clone(),
		};
		self.queue_command(payload, Some(task.conversation_revision), None)?;
		Ok(submission)
	}

	/// Execute one explicit pre-session recovery action without consuming composer text.
	pub(crate) fn recover_selected(&self) -> Result<(), QuickTaskInputError> {
		let state = self.lock();
		if state.command == QuickTaskCommandState::OutcomeUnknown
			|| state.pending_command.is_some()
			|| state.in_flight_command.is_some()
		{
			return Err(QuickTaskInputError::Busy);
		}
		let task = state.selected_task().ok_or(QuickTaskInputError::NoSelection)?.clone();
		let payload = task_recovery_command(&task).ok_or(QuickTaskInputError::NotReady)?;
		drop(state);
		self.queue_command(payload, Some(task.conversation_revision), None)
	}

	pub(crate) fn take_history_reload_request(&self) -> Option<EntityId> {
		self.lock().history_reload_requested.take()
	}

	pub(crate) fn interrupt(&self) -> Result<(), QuickTaskInputError> {
		let state = self.lock();
		let task = state.selected_task().ok_or(QuickTaskInputError::NoSelection)?.clone();
		let turn_id = task.active_turn_id.clone().ok_or(QuickTaskInputError::NotInterruptible)?;
		if task.state != QuickTaskState::Running {
			return Err(QuickTaskInputError::NotInterruptible);
		}
		drop(state);
		self.queue_command(
			CommandPayload::InterruptQuickTask { conversation_id: task.conversation_id, turn_id },
			Some(task.conversation_revision),
			None,
		)
	}

	fn queue_command(
		&self,
		payload: CommandPayload,
		expected_revision: Option<decodex_protocol::EntityRevision>,
		select_after_acceptance: Option<EntityId>,
	) -> Result<(), QuickTaskInputError> {
		let mut state = self.lock();
		if state.session.is_none() {
			return Err(QuickTaskInputError::Offline);
		}
		if state.pending_command.is_some() || state.in_flight_command.is_some() {
			return Err(QuickTaskInputError::Busy);
		}
		if state.refresh_batch.is_some() {
			return Err(QuickTaskInputError::Busy);
		}
		if state.command == QuickTaskCommandState::OutcomeUnknown {
			return Err(QuickTaskInputError::Busy);
		}
		let identity = command_identity()?;
		let envelope = CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: identity.client_command_id,
			idempotency_key: identity.idempotency_key,
			expected_revision,
			correlation_id: identity.correlation_id,
			causation_id: None::<CausationId>,
			payload,
		};
		let routing_successor_reconciliation = match (&envelope.payload, envelope.expected_revision)
		{
			(
				CommandPayload::CreateQuickTaskRoutingSuccessor { conversation_id },
				Some(expected_source_revision),
			) => Some(RoutingSuccessorReconciliation {
				source_conversation_id: conversation_id.clone(),
				expected_source_revision,
			}),
			_ => None,
		};
		state.outcome_unknown_readback_generation = None;
		state.refresh = QuickTaskRefreshState::Idle;
		state.routing_successor_reconciliation = routing_successor_reconciliation;
		state.pending_command = Some(PendingCommand { envelope, select_after_acceptance });
		state.command = QuickTaskCommandState::Sending;
		drop(state);
		self.inner.notify.notify_one();
		Ok(())
	}

	pub(crate) fn bind_session(&self, generation: u64, server_id: ServerId) {
		let mut state = self.lock();
		let binding = SessionBinding { generation, server_id };
		if state.session.as_ref() == Some(&binding) {
			return;
		}
		state.latch_in_flight_outcome_unknown();
		state.cancel_refresh_batch();
		state.reset_pagination();
		state.session = Some(binding);
		state.outcome_unknown_readback_generation = (state.command
			== QuickTaskCommandState::OutcomeUnknown
			&& state.pending_command.is_none()
			&& state.in_flight_command.is_none())
		.then_some(generation);
		let command_queued = state.pending_command.is_some();
		let query_queued = state.queue_list();
		drop(state);
		if command_queued || query_queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn session_ended(&self, generation: u64) {
		let mut state = self.lock();
		if !state.session.as_ref().is_some_and(|binding| binding.generation == generation) {
			return;
		}
		state.latch_in_flight_outcome_unknown();
		state.cancel_refresh_batch();
		state.reset_pagination();
		state.outcome_unknown_readback_generation = None;
		state.session = None;
		state.load = QuickTasksLoadState::Offline;
		drop(state);
		self.inner.notify.notify_one();
	}

	pub(crate) async fn next_dispatch(
		&self,
		generation: u64,
		server_id: &ServerId,
	) -> QuickTaskDispatch {
		loop {
			let notified = self.inner.notify.notified();
			if let Some(dispatch) = self.try_take_dispatch(generation, server_id) {
				return dispatch;
			}
			notified.await;
		}
	}

	fn try_take_dispatch(
		&self,
		generation: u64,
		server_id: &ServerId,
	) -> Option<QuickTaskDispatch> {
		let mut state = self.lock();
		let binding = SessionBinding { generation, server_id: server_id.clone() };
		if state.session.as_ref() != Some(&binding) {
			return None;
		}
		if state.in_flight_command.is_none()
			&& let Some(pending) = state.pending_command.take()
		{
			let envelope = pending.envelope.clone();
			state.in_flight_command = Some(InFlightCommand {
				envelope: pending.envelope,
				binding,
				select_after_acceptance: pending.select_after_acceptance,
			});
			return Some(QuickTaskDispatch::Command(envelope));
		}
		if state.in_flight_query.is_none()
			&& let Some(pending) = state.pending_query.take()
		{
			let envelope = pending.envelope.clone();
			state.in_flight_query = Some(InFlightQuery {
				query_id: pending.envelope.query_id,
				binding,
				purpose: pending.purpose,
			});
			return Some(QuickTaskDispatch::Query(envelope));
		}
		None
	}

	pub(crate) fn command_send_failed(&self, dispatch: &QuickTaskDispatch) {
		let Some(command) = dispatch.command() else {
			return;
		};
		let mut state = self.lock();
		let matches = state.in_flight_command.as_ref().is_some_and(|in_flight| {
			in_flight.envelope.client_command_id == command.client_command_id
		});
		let mut query_queued = false;
		if matches {
			state.latch_in_flight_outcome_unknown();
			state.cancel_refresh_batch();
			query_queued = state.queue_routing_successor_readback();
		}
		drop(state);
		if query_queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn command_sent(&self, dispatch: &QuickTaskDispatch) {
		let Some(command) = dispatch.command() else {
			return;
		};
		let mut state = self.lock();
		if state.in_flight_command.as_ref().is_some_and(|in_flight| {
			in_flight.envelope.client_command_id == command.client_command_id
		}) {
			state.command = QuickTaskCommandState::AwaitingResult;
		}
	}

	pub(crate) fn apply_event(&self, event: &EventEnvelope) {
		let mut state = self.lock();
		if matches!(&event.payload, EventPayload::QuickTaskTurnFinished { .. })
			&& state.command == QuickTaskCommandState::Accepted
		{
			state.command = QuickTaskCommandState::Idle;
		}

		match &event.payload {
			EventPayload::QuickTaskConversationChanged { conversation }
			| EventPayload::QuickTaskTurnFinished { conversation, .. } => {
				state.upsert_task(conversation.clone());
				if state.command == QuickTaskCommandState::Accepted {
					state.command = QuickTaskCommandState::Idle;
				}
			},
			EventPayload::QuickTaskMessageDelta { conversation_id, turn_id, delta } => {
				state.push_delta(QuickTaskLiveDelta {
					history_item_id: event.entity_id.clone(),
					conversation_id: conversation_id.clone(),
					turn_id: turn_id.clone(),
					text: delta.clone(),
				});
			},
			EventPayload::QuickTaskArchived { conversation_id, .. } => {
				state.remove_task(conversation_id);
				if state.command == QuickTaskCommandState::Accepted {
					state.command = QuickTaskCommandState::Idle;
				}
			},
			_ => {},
		}
	}

	pub(crate) fn reconcile_durable_history(
		&self,
		conversation_id: &EntityId,
		page: &ConversationHistoryPage,
	) {
		let mut state = self.lock();
		state.live_deltas.retain(|delta| {
			&delta.conversation_id != conversation_id
				|| !page.items.iter().any(|item| {
					item.history_item_id == delta.history_item_id
						&& item.payload.inline_text().is_some()
				})
		});
		state.live_delta_bytes =
			state.live_deltas.iter().map(|delta| delta.text.as_str().len()).sum();
	}

	pub(crate) fn route_query_result(
		&self,
		generation: u64,
		server_id: &ServerId,
		result: &QueryResultEnvelope,
	) -> QuickTaskRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_query.as_ref() else {
			return QuickTaskRouteOutcome::Unmatched;
		};
		if in_flight.query_id != result.query_id {
			return QuickTaskRouteOutcome::Unmatched;
		}
		let purpose = in_flight.purpose.clone();
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| result.version != CURRENT_VERSION
			|| result.server_id != *server_id
		{
			state.in_flight_query = None;
			state.load = QuickTasksLoadState::Refused;
			state.cancel_refresh_batch();
			return QuickTaskRouteOutcome::Refused;
		}
		state.in_flight_query = None;
		let (outcome, query_queued) = match purpose {
			QuickTaskQueryPurpose::List { after } =>
				state.route_list_query_result(generation, after, &result.payload),
			QuickTaskQueryPurpose::RoutingSuccessorSource { reconciliation } => state
				.route_routing_successor_source_result(generation, reconciliation, &result.payload),
			QuickTaskQueryPurpose::RoutingSuccessorProjection { reconciliation, successor } =>
				state.route_routing_successor_projection_result(
					generation,
					reconciliation,
					successor,
					&result.payload,
				),
		};
		drop(state);
		if query_queued {
			self.inner.notify.notify_one();
		}
		outcome
	}

	pub(crate) fn route_receipt(
		&self,
		generation: u64,
		server_id: &ServerId,
		receipt: &CommandReceipt,
	) -> QuickTaskRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_command.as_ref() else {
			return QuickTaskRouteOutcome::Unmatched;
		};
		if in_flight.envelope.client_command_id != receipt.client_command_id {
			return QuickTaskRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| receipt.version != CURRENT_VERSION
			|| receipt.server_id != *server_id
			|| receipt.idempotency_key != in_flight.envelope.idempotency_key
		{
			state.in_flight_command = None;
			state.command = QuickTaskCommandState::OutcomeUnknown;
			state.cancel_refresh_batch();
			let query_queued = state.queue_routing_successor_readback();
			drop(state);
			if query_queued {
				self.inner.notify.notify_one();
			}
			return QuickTaskRouteOutcome::Refused;
		}
		let submission = is_submission_command(&in_flight.envelope.payload);
		let dispatch_queued = match receipt.disposition {
			ReceiptDisposition::Executed | ReceiptDisposition::Duplicate => {
				state.command = QuickTaskCommandState::AwaitingResult;
				false
			},
			ReceiptDisposition::Refused => {
				if submission {
					state.submission_result_generation =
						state.submission_result_generation.saturating_add(1);
					state.last_submission_accepted = false;
				}
				state.in_flight_command = None;
				state.routing_successor_reconciliation = None;
				state.outcome_unknown_readback_generation = None;
				if state.refresh_batch.is_some() {
					state.advance_refresh_batch(false, true)
				} else {
					state.command = QuickTaskCommandState::Refused;
					false
				}
			},
		};
		drop(state);
		if dispatch_queued {
			self.inner.notify.notify_one();
		}
		QuickTaskRouteOutcome::Fresh
	}

	pub(crate) fn route_command_result(
		&self,
		generation: u64,
		server_id: &ServerId,
		result: &CommandResultEnvelope,
	) -> QuickTaskRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_command.as_ref() else {
			return QuickTaskRouteOutcome::Unmatched;
		};
		if in_flight.envelope.client_command_id != result.client_command_id {
			return QuickTaskRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| result.version != CURRENT_VERSION
			|| result.server_id != *server_id
			|| result.idempotency_key != in_flight.envelope.idempotency_key
		{
			state.in_flight_command = None;
			let successor_commit_is_ambiguous = state.routing_successor_reconciliation.is_some();
			state.command = if successor_commit_is_ambiguous || state.refresh_batch.is_some() {
				QuickTaskCommandState::OutcomeUnknown
			} else {
				QuickTaskCommandState::Refused
			};
			state.cancel_refresh_batch();
			let query_queued =
				successor_commit_is_ambiguous && state.queue_routing_successor_readback();
			drop(state);
			if query_queued {
				self.inner.notify.notify_one();
			}
			return QuickTaskRouteOutcome::Refused;
		}
		let in_flight =
			state.in_flight_command.take().expect("matching Quick Task command remains in flight");
		let submission = is_submission_command(&in_flight.envelope.payload);
		if submission {
			state.submission_result_generation =
				state.submission_result_generation.saturating_add(1);
			state.last_submission_accepted = false;
			state.history_reload_requested =
				Some(command_conversation_id(&in_flight.envelope.payload));
		}
		let select_after_acceptance = in_flight.select_after_acceptance.clone();
		let select_returned_successor = matches!(
			&in_flight.envelope.payload,
			CommandPayload::CreateQuickTaskRoutingSuccessor { .. }
		);
		let batch_refresh = state.refresh_batch.is_some()
			&& matches!(&in_flight.envelope.payload, CommandPayload::RefreshQuickTask { .. });
		let mut dispatch_queued = false;
		let outcome = match result.outcome {
			CommandOutcome::Succeeded => {
				if let Some(conversation_id) = accepted_archive_result(&in_flight, result) {
					state.remove_task(&conversation_id);
					state.routing_successor_reconciliation = None;
					state.outcome_unknown_readback_generation = None;
					if batch_refresh {
						dispatch_queued = state.advance_refresh_batch(true, false);
					} else {
						state.command = QuickTaskCommandState::Accepted;
					}
					drop(state);
					if dispatch_queued {
						self.inner.notify.notify_one();
					}
					return QuickTaskRouteOutcome::Fresh;
				}
				let task = accepted_result_task(&in_flight, result);
				let Some(task) = task else {
					if batch_refresh {
						dispatch_queued = state.advance_refresh_batch(false, true);
					} else if state.routing_successor_reconciliation.is_some() {
						state.command = QuickTaskCommandState::OutcomeUnknown;
						dispatch_queued = state.queue_routing_successor_readback();
					} else {
						state.command = QuickTaskCommandState::Refused;
					}
					drop(state);
					if dispatch_queued {
						self.inner.notify.notify_one();
					}
					return QuickTaskRouteOutcome::Refused;
				};
				if select_returned_successor
					&& let CommandPayload::CreateQuickTaskRoutingSuccessor { conversation_id } =
						&in_flight.envelope.payload
				{
					state.apply_routing_successor_transition(conversation_id, task);
				} else {
					state.upsert_task(task);
				}
				if let Some(conversation_id) = select_after_acceptance {
					state.selected = Some(conversation_id);
					state.selection_suppressed = false;
				}
				state.routing_successor_reconciliation = None;
				state.outcome_unknown_readback_generation = None;
				if submission {
					state.last_submission_accepted = true;
				}
				if batch_refresh {
					dispatch_queued = state.advance_refresh_batch(false, false);
				} else {
					state.command = QuickTaskCommandState::Accepted;
				}
				QuickTaskRouteOutcome::Fresh
			},
			CommandOutcome::AcceptanceUnknown => {
				state.command = QuickTaskCommandState::OutcomeUnknown;
				state.cancel_refresh_batch();
				dispatch_queued = state.queue_routing_successor_readback();
				QuickTaskRouteOutcome::Fresh
			},
			CommandOutcome::Rejected => {
				if matches!(result.error.as_ref(), Some(CommandError::AcceptanceUnknown)) {
					state.command = QuickTaskCommandState::OutcomeUnknown;
					state.cancel_refresh_batch();
					dispatch_queued = state.queue_routing_successor_readback();
				} else if batch_refresh {
					state.routing_successor_reconciliation = None;
					state.outcome_unknown_readback_generation = None;
					dispatch_queued = state.advance_refresh_batch(false, true);
				} else {
					state.routing_successor_reconciliation = None;
					state.outcome_unknown_readback_generation = None;
					state.command = match result.error.as_ref() {
						Some(CommandError::QuickTaskRecoveryRequired { action }) =>
							QuickTaskCommandState::ManualRecovery(*action),
						_ => QuickTaskCommandState::Refused,
					};
					if matches!(
						result.error.as_ref(),
						Some(CommandError::QuickTaskRecoveryRequired { .. })
					) {
						state.reset_pagination();
						dispatch_queued = state.queue_list();
					}
				}
				QuickTaskRouteOutcome::Fresh
			},
		};
		drop(state);
		if dispatch_queued {
			self.inner.notify.notify_one();
		}
		outcome
	}

	fn lock(&self) -> MutexGuard<'_, State> {
		self.inner.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SessionBinding {
	generation: u64,
	server_id: ServerId,
}

struct State {
	session: Option<SessionBinding>,
	active: bool,
	load: QuickTasksLoadState,
	command: QuickTaskCommandState,
	submission_result_generation: u64,
	last_submission_accepted: bool,
	history_reload_requested: Option<EntityId>,
	refresh: QuickTaskRefreshState,
	refresh_batch: Option<RefreshBatch>,
	tasks: Vec<QuickTaskSummary>,
	selected: Option<EntityId>,
	selection_suppressed: bool,
	requested_selection: Option<EntityId>,
	pending_query: Option<PendingQuery>,
	in_flight_query: Option<InFlightQuery>,
	pending_command: Option<PendingCommand>,
	in_flight_command: Option<InFlightCommand>,
	next_query_sequence: u64,
	next_list_cursor: Option<QuickTaskListCursor>,
	seen_list_cursors: Vec<QuickTaskListCursor>,
	list_pages_accepted: usize,
	list_accumulator: Vec<QuickTaskSummary>,
	outcome_unknown_readback_generation: Option<u64>,
	routing_successor_reconciliation: Option<RoutingSuccessorReconciliation>,
	live_deltas: VecDeque<QuickTaskLiveDelta>,
	live_delta_bytes: usize,
	execution: QuickTaskExecutionSettings,
}

impl State {
	fn new() -> Self {
		Self {
			session: None,
			active: false,
			load: QuickTasksLoadState::NeverRequested,
			command: QuickTaskCommandState::Idle,
			submission_result_generation: 0,
			last_submission_accepted: false,
			history_reload_requested: None,
			refresh: QuickTaskRefreshState::Idle,
			refresh_batch: None,
			tasks: Vec::new(),
			selected: None,
			selection_suppressed: true,
			requested_selection: None,
			pending_query: None,
			in_flight_query: None,
			pending_command: None,
			in_flight_command: None,
			next_query_sequence: 0,
			next_list_cursor: None,
			seen_list_cursors: Vec::new(),
			list_pages_accepted: 0,
			list_accumulator: Vec::new(),
			outcome_unknown_readback_generation: None,
			routing_successor_reconciliation: None,
			live_deltas: VecDeque::new(),
			live_delta_bytes: 0,
			execution: QuickTaskExecutionSettings::new(
				QuickTaskModel::new("gpt-5.6-sol").expect("default model is valid"),
				QuickTaskReasoningEffort::High,
				false,
			),
		}
	}

	fn clamp_effort_for_model(&mut self) {
		let supported = supported_efforts(self.execution.model.as_str());
		if !supported.contains(&self.execution.reasoning_effort) {
			self.execution.reasoning_effort =
				*supported.last().expect("every curated model has a reasoning effort");
		}
	}

	fn reset_pagination(&mut self) {
		self.pending_query = None;
		self.in_flight_query = None;
		self.next_list_cursor = None;
		self.seen_list_cursors.clear();
		self.list_pages_accepted = 0;
		self.list_accumulator.clear();
	}

	fn queue_list(&mut self) -> bool {
		if !self.active || self.pending_query.is_some() || self.in_flight_query.is_some() {
			return false;
		}
		if self.next_list_cursor.is_none() {
			self.seen_list_cursors.clear();
			self.list_pages_accepted = 0;
			self.list_accumulator.clear();
		}
		let after = self.next_list_cursor.clone();
		let queued = self.queue_query(
			QueryPayload::ListQuickTasks {
				after: after.clone(),
				page_size: QuickTaskListSize::new(32)
					.expect("constant Quick Task page size is valid"),
			},
			QuickTaskQueryPurpose::List { after },
		);
		if !queued {
			return false;
		}
		self.load = QuickTasksLoadState::Loading;
		true
	}

	fn queue_routing_successor_source_readback(&mut self, generation: u64) -> bool {
		if self.outcome_unknown_readback_generation != Some(generation)
			|| self.command != QuickTaskCommandState::OutcomeUnknown
			|| self.pending_command.is_some()
			|| self.in_flight_command.is_some()
		{
			return false;
		}
		let Some(reconciliation) = self.routing_successor_reconciliation.clone() else {
			return false;
		};
		self.queue_query(
			QueryPayload::GetQuickTask {
				conversation_id: reconciliation.source_conversation_id.clone(),
			},
			QuickTaskQueryPurpose::RoutingSuccessorSource { reconciliation },
		)
	}

	fn queue_routing_successor_projection_readback(
		&mut self,
		generation: u64,
		reconciliation: RoutingSuccessorReconciliation,
		successor: RoutingSuccessorBinding,
	) -> bool {
		if self.outcome_unknown_readback_generation != Some(generation)
			|| self.command != QuickTaskCommandState::OutcomeUnknown
			|| self.routing_successor_reconciliation.as_ref() != Some(&reconciliation)
			|| self.pending_command.is_some()
			|| self.in_flight_command.is_some()
		{
			return false;
		}
		self.queue_query(
			QueryPayload::GetQuickTask { conversation_id: successor.conversation_id.clone() },
			QuickTaskQueryPurpose::RoutingSuccessorProjection { reconciliation, successor },
		)
	}

	fn queue_routing_successor_readback(&mut self) -> bool {
		if self.command != QuickTaskCommandState::OutcomeUnknown
			|| self.routing_successor_reconciliation.is_none()
			|| self.pending_command.is_some()
			|| self.in_flight_command.is_some()
		{
			return false;
		}
		let Some(generation) = self.session.as_ref().map(|session| session.generation) else {
			return false;
		};
		self.outcome_unknown_readback_generation = Some(generation);
		if self.pending_query.is_some() || self.in_flight_query.is_some() {
			return false;
		}
		self.next_list_cursor = None;
		self.queue_list()
	}

	fn queue_query(&mut self, payload: QueryPayload, purpose: QuickTaskQueryPurpose) -> bool {
		let Some(generation) = self.session.as_ref().map(|session| session.generation) else {
			return false;
		};
		if !self.active || self.pending_query.is_some() || self.in_flight_query.is_some() {
			return false;
		}
		let Some(sequence) = self.next_query_sequence.checked_add(1) else {
			self.load = QuickTasksLoadState::Refused;
			return false;
		};
		self.next_query_sequence = sequence;
		self.pending_query = Some(PendingQuery {
			envelope: QueryEnvelope {
				version: CURRENT_VERSION,
				query_id: QueryId::new(format!("gpui-quick-tasks/{generation}/{sequence}"))
					.expect("bounded numeric Quick Task query identity"),
				payload,
			},
			purpose,
		});
		true
	}

	fn route_list_query_result(
		&mut self,
		generation: u64,
		requested_after: Option<QuickTaskListCursor>,
		payload: &QueryResultPayload,
	) -> (QuickTaskRouteOutcome, bool) {
		match payload {
			QueryResultPayload::QuickTasks(QuickTaskListResult::Available(page)) => {
				if requested_after
					.as_ref()
					.is_some_and(|cursor| !self.seen_list_cursors.iter().any(|seen| seen == cursor))
				{
					self.next_list_cursor = None;
					self.list_accumulator.clear();
					self.load = QuickTasksLoadState::Refused;
					return (QuickTaskRouteOutcome::Refused, false);
				}
				let Some(page_count) = self.list_pages_accepted.checked_add(1) else {
					self.next_list_cursor = None;
					self.list_accumulator.clear();
					self.load = QuickTasksLoadState::Refused;
					return (QuickTaskRouteOutcome::Refused, false);
				};
				self.list_pages_accepted = page_count;
				self.accumulate_tasks(page.conversations.clone());
				match page.next_cursor.clone() {
					Some(next_cursor)
						if page_count >= MAX_LIST_PAGES
							|| self.seen_list_cursors.iter().any(|seen| seen == &next_cursor) =>
					{
						self.next_list_cursor = None;
						self.list_accumulator.clear();
						self.load = QuickTasksLoadState::Refused;
						(QuickTaskRouteOutcome::Refused, false)
					},
					Some(next_cursor) => {
						self.seen_list_cursors.push(next_cursor.clone());
						self.next_list_cursor = Some(next_cursor);
						let query_queued = self.queue_list();
						(QuickTaskRouteOutcome::Fresh, query_queued)
					},
					None => {
						self.next_list_cursor = None;
						let tasks = std::mem::take(&mut self.list_accumulator);
						self.replace_tasks(tasks);
						self.load = QuickTasksLoadState::Ready;
						if self.refresh_batch.as_ref().is_some_and(|batch| batch.reloading) {
							self.finish_refresh_batch();
						}
						if self.routing_successor_reconciliation.is_some()
							&& self.command == QuickTaskCommandState::OutcomeUnknown
							&& self.outcome_unknown_readback_generation == Some(generation)
						{
							let query_queued =
								self.queue_routing_successor_source_readback(generation);
							if query_queued {
								(QuickTaskRouteOutcome::Fresh, true)
							} else {
								self.load = QuickTasksLoadState::Refused;
								(QuickTaskRouteOutcome::Refused, false)
							}
						} else {
							self.finish_outcome_unknown_readback(generation);
							(QuickTaskRouteOutcome::Fresh, false)
						}
					},
				}
			},
			QueryResultPayload::QuickTasks(QuickTaskListResult::Unavailable { .. }) => {
				self.next_list_cursor = None;
				self.list_accumulator.clear();
				self.load = QuickTasksLoadState::Unavailable;
				self.finish_refresh_batch();
				(QuickTaskRouteOutcome::Fresh, false)
			},
			_ => {
				self.next_list_cursor = None;
				self.list_accumulator.clear();
				self.load = QuickTasksLoadState::Refused;
				self.finish_refresh_batch();
				(QuickTaskRouteOutcome::Refused, false)
			},
		}
	}

	fn route_routing_successor_source_result(
		&mut self,
		generation: u64,
		reconciliation: RoutingSuccessorReconciliation,
		payload: &QueryResultPayload,
	) -> (QuickTaskRouteOutcome, bool) {
		match payload {
			QueryResultPayload::QuickTask(QuickTaskResult::Available(source))
				if self.routing_successor_reconciliation_matches(generation, &reconciliation)
					&& source.conversation_id == reconciliation.source_conversation_id =>
			{
				self.upsert_task(source.clone());
				self.finish_routing_successor_reconciliation();
				(QuickTaskRouteOutcome::Fresh, false)
			},
			QueryResultPayload::QuickTask(QuickTaskResult::RoutingSuccessorRedirect {
				source_conversation_id,
				source_conversation_revision,
				successor_conversation_id,
				successor_conversation_revision,
			}) if self.routing_successor_reconciliation_matches(generation, &reconciliation)
				&& source_conversation_id == &reconciliation.source_conversation_id
				&& reconciliation.expected_source_revision.0.checked_add(1)
					== Some(source_conversation_revision.0)
				&& successor_conversation_id != source_conversation_id
				&& successor_conversation_revision.0 > 0 =>
			{
				let query_queued = self.queue_routing_successor_projection_readback(
					generation,
					reconciliation,
					RoutingSuccessorBinding {
						conversation_id: successor_conversation_id.clone(),
						conversation_revision: *successor_conversation_revision,
					},
				);
				if query_queued {
					(QuickTaskRouteOutcome::Fresh, true)
				} else {
					self.load = QuickTasksLoadState::Refused;
					(QuickTaskRouteOutcome::Refused, false)
				}
			},
			QueryResultPayload::QuickTask(QuickTaskResult::Unavailable { .. }) => {
				self.load = QuickTasksLoadState::Unavailable;
				(QuickTaskRouteOutcome::Fresh, false)
			},
			_ => {
				self.load = QuickTasksLoadState::Refused;
				(QuickTaskRouteOutcome::Refused, false)
			},
		}
	}

	fn route_routing_successor_projection_result(
		&mut self,
		generation: u64,
		reconciliation: RoutingSuccessorReconciliation,
		successor: RoutingSuccessorBinding,
		payload: &QueryResultPayload,
	) -> (QuickTaskRouteOutcome, bool) {
		match payload {
			QueryResultPayload::QuickTask(QuickTaskResult::Available(projection))
				if self.routing_successor_reconciliation_matches(generation, &reconciliation)
					&& projection.conversation_id == successor.conversation_id
					&& projection.conversation_revision == successor.conversation_revision =>
			{
				self.apply_routing_successor_transition(
					&reconciliation.source_conversation_id,
					projection.clone(),
				);
				self.finish_routing_successor_reconciliation();
				(QuickTaskRouteOutcome::Fresh, false)
			},
			QueryResultPayload::QuickTask(QuickTaskResult::Unavailable { .. }) => {
				self.load = QuickTasksLoadState::Unavailable;
				(QuickTaskRouteOutcome::Fresh, false)
			},
			_ => {
				self.load = QuickTasksLoadState::Refused;
				(QuickTaskRouteOutcome::Refused, false)
			},
		}
	}

	fn selected_task(&self) -> Option<&QuickTaskSummary> {
		let selected = self.selected.as_ref()?;
		self.tasks.iter().find(|task| &task.conversation_id == selected)
	}

	fn advance_refresh_batch(&mut self, archived: bool, failed: bool) -> bool {
		let Some(mut batch) = self.refresh_batch.take() else {
			return false;
		};
		batch.completed = batch.completed.saturating_add(1);
		batch.archived = batch.archived.saturating_add(usize::from(archived));
		batch.failed = batch.failed.saturating_add(usize::from(failed));
		self.refresh = QuickTaskRefreshState::Refreshing {
			completed: batch.completed,
			total: batch.total,
			archived: batch.archived,
			failed: batch.failed,
		};

		if let Some(next) = batch.remaining.pop_front() {
			self.pending_command = Some(next);
			self.command = QuickTaskCommandState::Sending;
			self.refresh_batch = Some(batch);
			return true;
		}

		self.command = QuickTaskCommandState::Idle;
		batch.reloading = true;
		self.refresh_batch = Some(batch);
		self.next_list_cursor = None;
		let queued = self.queue_list();
		if !queued {
			self.finish_refresh_batch();
		}
		queued
	}

	fn finish_refresh_batch(&mut self) {
		let Some(batch) = self.refresh_batch.take() else {
			return;
		};
		self.refresh = QuickTaskRefreshState::Complete {
			checked: batch.completed,
			archived: batch.archived,
			failed: batch.failed,
		};
	}

	fn cancel_refresh_batch(&mut self) {
		let Some(batch) = self.refresh_batch.take() else {
			return;
		};
		self.pending_command = None;
		self.refresh = QuickTaskRefreshState::Stopped {
			checked: batch.completed,
			total: batch.total,
			archived: batch.archived,
			failed: batch.failed,
		};
	}

	fn replace_tasks(&mut self, mut tasks: Vec<QuickTaskSummary>) {
		for task in &mut tasks {
			let Some(existing) =
				self.tasks.iter().find(|existing| existing.conversation_id == task.conversation_id)
			else {
				continue;
			};
			if !task_can_replace(existing, task) {
				*task = existing.clone();
			}
		}
		if let Some(requested) = self.requested_selection.as_ref()
			&& tasks.iter().any(|task| &task.conversation_id == requested)
		{
			self.selected = self.requested_selection.take();
		}
		let selected_is_present = self
			.selected
			.as_ref()
			.is_some_and(|selected| tasks.iter().any(|task| &task.conversation_id == selected));
		let preserves_unknown_successor_source =
			self.routing_successor_reconciliation.as_ref().is_some_and(|reconciliation| {
				self.selected.as_ref() == Some(&reconciliation.source_conversation_id)
			});
		if !selected_is_present && !preserves_unknown_successor_source {
			self.selected = if self.selection_suppressed {
				None
			} else {
				tasks.first().map(|task| task.conversation_id.clone())
			};
		}
		self.tasks = tasks;
	}

	fn accumulate_tasks(&mut self, tasks: Vec<QuickTaskSummary>) {
		for task in tasks {
			if let Some(existing) = self
				.list_accumulator
				.iter_mut()
				.find(|existing| existing.conversation_id == task.conversation_id)
			{
				if task_can_replace(existing, &task) {
					*existing = task;
				}
			} else {
				self.list_accumulator.push(task);
			}
		}
	}

	fn upsert_task(&mut self, task: QuickTaskSummary) {
		let requested = self.requested_selection.as_ref() == Some(&task.conversation_id);
		if let Some(existing) =
			self.tasks.iter_mut().find(|existing| existing.conversation_id == task.conversation_id)
		{
			if task_can_replace(existing, &task) {
				*existing = task;
			}
		} else {
			self.tasks.insert(0, task.clone());
			if self.selected.is_none() && !self.selection_suppressed {
				self.selected = Some(task.conversation_id);
			}
		}
		if requested {
			self.selected = self.requested_selection.take();
			self.selection_suppressed = false;
		}
	}

	fn apply_routing_successor_transition(
		&mut self,
		source_conversation_id: &EntityId,
		successor: QuickTaskSummary,
	) {
		let successor_conversation_id = successor.conversation_id.clone();
		self.tasks.retain(|task| &task.conversation_id != source_conversation_id);
		self.upsert_task(successor);
		self.selected = Some(successor_conversation_id);
		self.selection_suppressed = false;
	}

	fn remove_task(&mut self, conversation_id: &EntityId) {
		self.tasks.retain(|task| &task.conversation_id != conversation_id);
		self.live_deltas.retain(|delta| &delta.conversation_id != conversation_id);
		if self.selected.as_ref() == Some(conversation_id) {
			// Never retarget an unsent composer draft to a different provider thread.
			self.selected = None;
			self.selection_suppressed = true;
		}
	}

	fn push_delta(&mut self, delta: QuickTaskLiveDelta) {
		self.live_delta_bytes = self.live_delta_bytes.saturating_add(delta.text.as_str().len());
		self.live_deltas.push_back(delta);
		while self.live_deltas.len() > MAX_LIVE_DELTAS
			|| self.live_delta_bytes > MAX_LIVE_DELTA_BYTES
		{
			let Some(removed) = self.live_deltas.pop_front() else {
				break;
			};
			self.live_delta_bytes =
				self.live_delta_bytes.saturating_sub(removed.text.as_str().len());
		}
	}

	fn latch_in_flight_outcome_unknown(&mut self) {
		if self.in_flight_command.take().is_some() {
			self.pending_command = None;
			self.command = QuickTaskCommandState::OutcomeUnknown;
		}
	}

	fn finish_outcome_unknown_readback(&mut self, generation: u64) {
		if self.routing_successor_reconciliation.is_none()
			&& self.outcome_unknown_readback_generation.take() == Some(generation)
			&& self.command == QuickTaskCommandState::OutcomeUnknown
			&& self.pending_command.is_none()
			&& self.in_flight_command.is_none()
		{
			self.command = QuickTaskCommandState::Idle;
		}
	}

	fn routing_successor_reconciliation_matches(
		&self,
		generation: u64,
		reconciliation: &RoutingSuccessorReconciliation,
	) -> bool {
		self.command == QuickTaskCommandState::OutcomeUnknown
			&& self.pending_command.is_none()
			&& self.in_flight_command.is_none()
			&& self.outcome_unknown_readback_generation == Some(generation)
			&& self.routing_successor_reconciliation.as_ref() == Some(reconciliation)
	}

	fn finish_routing_successor_reconciliation(&mut self) {
		self.routing_successor_reconciliation = None;
		self.outcome_unknown_readback_generation = None;
		self.command = QuickTaskCommandState::Idle;
	}

	fn snapshot(&self) -> QuickTasksSnapshot {
		QuickTasksSnapshot {
			load: self.load,
			command: self.command,
			submission_result_generation: self.submission_result_generation,
			last_submission_accepted: self.last_submission_accepted,
			refresh: self.refresh,
			tasks: self.tasks.clone(),
			selected: self.selected.clone(),
			live_deltas: self.live_deltas.iter().cloned().collect(),
			can_submit: self.session.is_some()
				&& self.refresh_batch.is_none()
				&& self.pending_command.is_none()
				&& self.in_flight_command.is_none()
				&& self.command != QuickTaskCommandState::OutcomeUnknown,
			execution: self.execution.clone(),
		}
	}
}

fn supported_efforts(model: &str) -> &'static [QuickTaskReasoningEffort] {
	match model {
		"gpt-5.6-sol" | "gpt-5.6-terra" => QUICK_TASK_EFFORTS,
		"gpt-5.6-luna" => &QUICK_TASK_EFFORTS[..5],
		_ => &QUICK_TASK_EFFORTS[..4],
	}
}

struct PendingCommand {
	envelope: CommandEnvelope,
	select_after_acceptance: Option<EntityId>,
}

struct RefreshBatch {
	remaining: VecDeque<PendingCommand>,
	completed: usize,
	total: usize,
	archived: usize,
	failed: usize,
	reloading: bool,
}

struct PendingQuery {
	envelope: QueryEnvelope,
	purpose: QuickTaskQueryPurpose,
}

struct InFlightCommand {
	envelope: CommandEnvelope,
	binding: SessionBinding,
	select_after_acceptance: Option<EntityId>,
}

struct InFlightQuery {
	query_id: QueryId,
	binding: SessionBinding,
	purpose: QuickTaskQueryPurpose,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum QuickTaskQueryPurpose {
	List {
		after: Option<QuickTaskListCursor>,
	},
	RoutingSuccessorSource {
		reconciliation: RoutingSuccessorReconciliation,
	},
	RoutingSuccessorProjection {
		reconciliation: RoutingSuccessorReconciliation,
		successor: RoutingSuccessorBinding,
	},
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RoutingSuccessorReconciliation {
	source_conversation_id: EntityId,
	expected_source_revision: EntityRevision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RoutingSuccessorBinding {
	conversation_id: EntityId,
	conversation_revision: EntityRevision,
}

fn accepted_result_task(
	in_flight: &InFlightCommand,
	result: &CommandResultEnvelope,
) -> Option<QuickTaskSummary> {
	if let (
		CommandPayload::CreateQuickTaskRoutingSuccessor { conversation_id },
		Some(ResultPayload::QuickTaskRoutingSuccessorAccepted {
			source_conversation_id,
			source_conversation_revision,
			successor,
		}),
	) = (&in_flight.envelope.payload, result.payload.as_ref())
	{
		return (source_conversation_id == conversation_id
			&& in_flight.envelope.expected_revision?.0.checked_add(1)
				== Some(source_conversation_revision.0)
			&& successor.conversation_id != *conversation_id
			&& result.entity_revision == Some(successor.conversation_revision))
		.then(|| successor.clone());
	}
	let conversation = match (&in_flight.envelope.payload, result.payload.as_ref()) {
		(
			CommandPayload::CreateQuickTask { .. }
			| CommandPayload::ResumeQuickTaskRouting { .. }
			| CommandPayload::ResumeQuickTaskEstablishment { .. }
			| CommandPayload::SubmitQuickTaskTurn { .. }
			| CommandPayload::RefreshQuickTask { .. },
			Some(ResultPayload::QuickTaskConversationAccepted { conversation }),
		) => conversation,
		(
			CommandPayload::InterruptQuickTask { .. },
			Some(ResultPayload::QuickTaskInterruptAccepted { conversation }),
		) => conversation,
		_ => return None,
	};
	let expected_conversation = command_conversation_id(&in_flight.envelope.payload);
	if conversation.conversation_id != expected_conversation
		|| conversation.conversation_revision.0 == 0
		|| result.entity_revision != Some(conversation.conversation_revision)
	{
		return None;
	}
	Some(conversation.clone())
}

fn accepted_archive_result(
	in_flight: &InFlightCommand,
	result: &CommandResultEnvelope,
) -> Option<EntityId> {
	let command_conversation = match &in_flight.envelope.payload {
		CommandPayload::ArchiveQuickTask { conversation_id }
		| CommandPayload::RefreshQuickTask { conversation_id } => conversation_id,
		_ => return None,
	};
	let Some(ResultPayload::QuickTaskArchived { conversation_id, conversation_revision }) =
		result.payload.as_ref()
	else {
		return None;
	};
	(command_conversation == conversation_id
		&& in_flight.envelope.expected_revision?.0.checked_add(1) == Some(conversation_revision.0)
		&& result.entity_revision == Some(*conversation_revision))
	.then(|| conversation_id.clone())
}

struct CommandIdentity {
	client_command_id: ClientCommandId,
	idempotency_key: IdempotencyKey,
	correlation_id: CorrelationId,
}

fn pending_command(
	payload: CommandPayload,
	expected_revision: Option<EntityRevision>,
	select_after_acceptance: Option<EntityId>,
) -> Result<PendingCommand, QuickTaskInputError> {
	let identity = command_identity()?;
	Ok(PendingCommand {
		envelope: CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: identity.client_command_id,
			idempotency_key: identity.idempotency_key,
			expected_revision,
			correlation_id: identity.correlation_id,
			causation_id: None::<CausationId>,
			payload,
		},
		select_after_acceptance,
	})
}

fn command_identity() -> Result<CommandIdentity, QuickTaskInputError> {
	let value = canonical_uuid_v4()?;
	Ok(CommandIdentity {
		client_command_id: ClientCommandId::new(format!("gpui/{value}"))
			.expect("canonical command identity is bounded"),
		idempotency_key: IdempotencyKey::new(format!("quick-task/{value}"))
			.expect("canonical idempotency key is bounded"),
		correlation_id: CorrelationId::new(value)
			.expect("canonical correlation identity is bounded"),
	})
}

fn entity_id() -> Result<EntityId, QuickTaskInputError> {
	EntityId::new(canonical_uuid_v4()?).map_err(|_| QuickTaskInputError::IdentityUnavailable)
}

fn canonical_uuid_v4() -> Result<String, QuickTaskInputError> {
	let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_err(|_| QuickTaskInputError::IdentityUnavailable)?
		.as_nanos();
	let mut digest = Sha256::new();
	digest.update(std::process::id().to_be_bytes());
	digest.update(nanos.to_be_bytes());
	digest.update(sequence.to_be_bytes());
	let mut bytes: [u8; 16] =
		digest.finalize()[..16].try_into().expect("SHA-256 always contains sixteen identity bytes");
	bytes[6] = (bytes[6] & 0x0f) | 0x40;
	bytes[8] = (bytes[8] & 0x3f) | 0x80;
	Ok(format!(
		"{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
		bytes[0],
		bytes[1],
		bytes[2],
		bytes[3],
		bytes[4],
		bytes[5],
		bytes[6],
		bytes[7],
		bytes[8],
		bytes[9],
		bytes[10],
		bytes[11],
		bytes[12],
		bytes[13],
		bytes[14],
		bytes[15],
	))
}

fn message_text(value: &str) -> Result<HistoryText, QuickTaskInputError> {
	if value.trim().is_empty() {
		return Err(QuickTaskInputError::InvalidMessage);
	}
	HistoryText::new(value).map_err(|_| QuickTaskInputError::InvalidMessage)
}

fn command_conversation_id(payload: &CommandPayload) -> EntityId {
	match payload {
		CommandPayload::CreateQuickTask { conversation_id, .. }
		| CommandPayload::ResumeQuickTaskRouting { conversation_id }
		| CommandPayload::CreateQuickTaskRoutingSuccessor { conversation_id }
		| CommandPayload::ResumeQuickTaskEstablishment { conversation_id }
		| CommandPayload::SubmitQuickTaskTurn { conversation_id, .. }
		| CommandPayload::RefreshQuickTask { conversation_id }
		| CommandPayload::ArchiveQuickTask { conversation_id }
		| CommandPayload::InterruptQuickTask { conversation_id, .. } => conversation_id.clone(),
		_ => unreachable!("Quick Tasks queues only ordinary Quick Task commands"),
	}
}

fn production_working_directory(
	configured: Option<String>,
	home: Option<String>,
) -> Option<QuickTaskWorkingDirectory> {
	match configured {
		Some(configured) => QuickTaskWorkingDirectory::new(configured).ok(),
		None => home.and_then(|home| QuickTaskWorkingDirectory::new(home).ok()),
	}
}

fn task_accepts_turn(task: &QuickTaskSummary) -> bool {
	task.state == QuickTaskState::Ready
}

fn task_needs_reconciliation(task: &QuickTaskSummary) -> bool {
	matches!(
		task.state,
		QuickTaskState::Ready | QuickTaskState::Running | QuickTaskState::OutcomeUnknown
	) || task_has_safe_stale_reconciliation(task)
}

fn task_has_safe_stale_reconciliation(task: &QuickTaskSummary) -> bool {
	matches!(
		(task.state, task.recovery_action),
		(QuickTaskState::Establishing, Some(QuickTaskRecoveryAction::ResumeEstablishment),)
			| (
				QuickTaskState::ManualRecovery,
				Some(
					QuickTaskRecoveryAction::ResolvePriorActiveTurn
						| QuickTaskRecoveryAction::SelectWorkingDirectory
						| QuickTaskRecoveryAction::StartNewConversation,
				),
			)
	)
}

fn is_submission_command(payload: &CommandPayload) -> bool {
	matches!(
		payload,
		CommandPayload::CreateQuickTask { .. } | CommandPayload::SubmitQuickTaskTurn { .. }
	)
}

fn task_recovery_command(task: &QuickTaskSummary) -> Option<CommandPayload> {
	let conversation_id = task.conversation_id.clone();
	match (task.state, task.recovery_action) {
		(QuickTaskState::RoutingPending, Some(QuickTaskRecoveryAction::ResumeRouting)) =>
			Some(CommandPayload::ResumeQuickTaskRouting { conversation_id }),
		(
			QuickTaskState::EstablishmentPending | QuickTaskState::Establishing,
			Some(QuickTaskRecoveryAction::ResumeEstablishment),
		) => Some(CommandPayload::ResumeQuickTaskEstablishment { conversation_id }),
		(
			QuickTaskState::QuotaExhausted | QuickTaskState::NoRoute,
			Some(QuickTaskRecoveryAction::CreateRoutingSuccessor),
		) => Some(CommandPayload::CreateQuickTaskRoutingSuccessor { conversation_id }),
		_ => None,
	}
}

fn task_can_replace(existing: &QuickTaskSummary, task: &QuickTaskSummary) -> bool {
	task.projection_updated_at_micros > existing.projection_updated_at_micros
		|| task.projection_updated_at_micros == existing.projection_updated_at_micros
			&& task == existing
}

#[cfg(test)]
mod tests {
	use decodex_protocol::{EntityRevision, QuickTaskListPage};

	use super::*;

	#[test]
	fn explicit_production_working_directory_overrides_home() {
		let working_directory = production_working_directory(
			Some("/private/tmp/decodex-empty-project".to_owned()),
			Some("/Users/tester".to_owned()),
		)
		.expect("explicit working directory is valid");

		assert_eq!(working_directory.as_str(), "/private/tmp/decodex-empty-project");
	}

	#[test]
	fn production_working_directory_uses_home_without_an_override() {
		let working_directory =
			production_working_directory(None, Some("/Users/tester".to_owned()))
				.expect("home working directory is valid");

		assert_eq!(working_directory.as_str(), "/Users/tester");
	}

	#[test]
	fn invalid_explicit_production_working_directory_does_not_fall_back() {
		let working_directory = production_working_directory(
			Some("relative/project".to_owned()),
			Some("/Users/tester".to_owned()),
		);

		assert_eq!(working_directory, None);
	}

	#[test]
	fn outcome_unknown_is_a_readback_candidate_and_never_a_turn_submission_state() {
		let task = QuickTaskSummary::new(
			EntityId::new("00000000-0000-4000-8000-000000000091").expect("conversation ID"),
			EntityRevision(3),
			1,
			Some(EntityId::new("00000000-0000-4000-8000-000000000092").expect("session ID")),
			Some(EntityRevision(4)),
			QuickTaskState::OutcomeUnknown,
			None,
			None,
		)
		.expect("outcome-unknown Quick Task");

		assert!(task_needs_reconciliation(&task));
		assert!(!task_accepts_turn(&task));
	}

	fn connected_quick_tasks() -> (QuickTasks, ServerId, QuickTaskSummary) {
		let quick_tasks = QuickTasks {
			inner: Arc::new(QuickTasksInner {
				state: Mutex::new(State::new()),
				notify: Notify::new(),
				working_directory: Some(
					QuickTaskWorkingDirectory::new("/tmp")
						.expect("test working directory is valid"),
				),
			}),
		};
		let server_id = ServerId::new("quick-task-test-server").expect("test server ID is valid");
		let conversation_id =
			EntityId::new("00000000-0000-4000-8000-000000000001").expect("test ID is valid");
		let task = QuickTaskSummary::new(
			conversation_id.clone(),
			EntityRevision(1),
			1,
			Some(
				EntityId::new("00000000-0000-4000-8000-000000000002")
					.expect("test session ID is valid"),
			),
			Some(EntityRevision(1)),
			QuickTaskState::Ready,
			None,
			None,
		)
		.expect("test Quick Task is valid");

		quick_tasks.activate();
		quick_tasks.bind_session(1, server_id.clone());
		{
			let mut state = quick_tasks.lock();
			state.tasks = vec![task.clone()];
			state.selected = Some(conversation_id.clone());
			state.selection_suppressed = false;
		}
		(quick_tasks, server_id, task)
	}

	fn take_and_mark_command_sent(
		quick_tasks: &QuickTasks,
		server_id: &ServerId,
	) -> QuickTaskDispatch {
		assert!(quick_tasks.submit("possibly accepted").is_ok());
		let dispatch =
			quick_tasks.try_take_dispatch(1, server_id).expect("queued command is dispatchable");
		assert!(matches!(&dispatch, QuickTaskDispatch::Command(_)));
		quick_tasks.command_sent(&dispatch);
		assert_eq!(quick_tasks.snapshot().command, QuickTaskCommandState::AwaitingResult);
		dispatch
	}

	fn send_routing_successor_and_reconnect_without_result(
		quick_tasks: &QuickTasks,
		server_id: &ServerId,
		source: &QuickTaskSummary,
	) {
		{
			let mut state = quick_tasks.lock();
			state.tasks = vec![source.clone()];
			state.selected = Some(source.conversation_id.clone());
		}
		assert_eq!(
			quick_tasks.submit("must remain in the composer"),
			Err(QuickTaskInputError::NotReady)
		);
		assert_eq!(quick_tasks.recover_selected(), Ok(()));
		let dispatch = quick_tasks
			.try_take_dispatch(1, server_id)
			.expect("routing successor command is dispatchable");
		let command = dispatch.command().expect("recovery dispatch is a command");
		assert!(matches!(
			&command.payload,
			CommandPayload::CreateQuickTaskRoutingSuccessor { conversation_id }
				if conversation_id == &source.conversation_id
		));
		quick_tasks.command_sent(&dispatch);
		quick_tasks.session_ended(1);
		quick_tasks.bind_session(2, server_id.clone());
	}

	#[test]
	fn complete_list_refresh_removes_a_projection_missing_from_current_authority() {
		let (quick_tasks, server_id, task) = connected_quick_tasks();
		let list = quick_tasks
			.try_take_dispatch(1, &server_id)
			.and_then(|dispatch| dispatch.query().cloned())
			.expect("session binding queues an authoritative list read");
		assert!(matches!(list.payload, QueryPayload::ListQuickTasks { .. }));
		let result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: list.query_id,
			payload: QueryResultPayload::QuickTasks(QuickTaskListResult::Available(
				QuickTaskListPage::new(Vec::new(), None).expect("empty list page is valid"),
			)),
		};

		assert_eq!(
			quick_tasks.route_query_result(1, &server_id, &result),
			QuickTaskRouteOutcome::Fresh
		);
		let snapshot = quick_tasks.snapshot();
		assert!(snapshot.tasks.is_empty());
		assert_eq!(snapshot.selected, None);
		assert_eq!(snapshot.load, QuickTasksLoadState::Ready);
		assert_ne!(snapshot.selected, Some(task.conversation_id));
	}

	#[test]
	fn paginated_list_refresh_replaces_only_after_the_complete_readback() {
		let (quick_tasks, server_id, selected) = connected_quick_tasks();
		let first_page_task = QuickTaskSummary::new(
			EntityId::new("00000000-0000-4000-8000-000000000003").expect("test ID is valid"),
			EntityRevision(1),
			2,
			Some(
				EntityId::new("00000000-0000-4000-8000-000000000004")
					.expect("test session ID is valid"),
			),
			Some(EntityRevision(1)),
			QuickTaskState::Ready,
			None,
			None,
		)
		.expect("test Quick Task is valid");
		let cursor = QuickTaskListCursor::new(
			first_page_task.projection_updated_at_micros,
			first_page_task.conversation_id.clone(),
		)
		.expect("test list cursor is valid");
		let first_query = quick_tasks
			.try_take_dispatch(1, &server_id)
			.and_then(|dispatch| dispatch.query().cloned())
			.expect("session binding queues the first list page");
		let first_result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: first_query.query_id,
			payload: QueryResultPayload::QuickTasks(QuickTaskListResult::Available(
				QuickTaskListPage::new(vec![first_page_task.clone()], Some(cursor.clone()))
					.expect("first list page is valid"),
			)),
		};

		assert_eq!(
			quick_tasks.route_query_result(1, &server_id, &first_result),
			QuickTaskRouteOutcome::Fresh
		);
		let partial = quick_tasks.snapshot();
		assert_eq!(partial.tasks, vec![selected.clone()]);
		assert_eq!(partial.selected, Some(selected.conversation_id.clone()));

		let second_query = quick_tasks
			.try_take_dispatch(1, &server_id)
			.and_then(|dispatch| dispatch.query().cloned())
			.expect("the cursor queues the second list page");
		assert!(matches!(
			second_query.payload,
			QueryPayload::ListQuickTasks { after: Some(ref after), .. } if after == &cursor
		));
		let second_result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: second_query.query_id,
			payload: QueryResultPayload::QuickTasks(QuickTaskListResult::Available(
				QuickTaskListPage::new(vec![selected.clone()], None)
					.expect("final list page is valid"),
			)),
		};

		assert_eq!(
			quick_tasks.route_query_result(1, &server_id, &second_result),
			QuickTaskRouteOutcome::Fresh
		);
		let complete = quick_tasks.snapshot();
		assert_eq!(complete.tasks, vec![first_page_task, selected.clone()]);
		assert_eq!(complete.selected, Some(selected.conversation_id));
		assert_eq!(complete.load, QuickTasksLoadState::Ready);
	}

	#[test]
	fn archive_result_removes_the_exact_selected_task() {
		let (quick_tasks, server_id, task) = connected_quick_tasks();
		assert_eq!(quick_tasks.archive_selected(), Ok(()));
		let dispatch =
			quick_tasks.try_take_dispatch(1, &server_id).expect("archive command is dispatchable");
		let command = dispatch.command().expect("archive dispatch is a command");
		assert!(matches!(
			&command.payload,
			CommandPayload::ArchiveQuickTask { conversation_id }
				if conversation_id == &task.conversation_id
		));
		assert_eq!(command.expected_revision, Some(task.conversation_revision));
		quick_tasks.command_sent(&dispatch);
		let result = CommandResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			client_command_id: command.client_command_id.clone(),
			idempotency_key: command.idempotency_key.clone(),
			outcome: CommandOutcome::Succeeded,
			entity_revision: Some(EntityRevision(2)),
			payload: Some(ResultPayload::QuickTaskArchived {
				conversation_id: task.conversation_id.clone(),
				conversation_revision: EntityRevision(2),
			}),
			error: None,
		};

		assert_eq!(
			quick_tasks.route_command_result(1, &server_id, &result),
			QuickTaskRouteOutcome::Fresh
		);
		let snapshot = quick_tasks.snapshot();
		assert!(snapshot.tasks.is_empty());
		assert_eq!(snapshot.selected, None);
	}

	#[test]
	fn definite_working_directory_failure_can_be_reconciled_or_archived_explicitly() {
		let (quick_tasks, server_id, existing) = connected_quick_tasks();
		let failed = QuickTaskSummary::new(
			existing.conversation_id.clone(),
			existing.conversation_revision,
			existing.projection_updated_at_micros,
			existing.runtime_session_id,
			existing.runtime_session_revision,
			QuickTaskState::ManualRecovery,
			None,
			Some(QuickTaskRecoveryAction::SelectWorkingDirectory),
		)
		.expect("working-directory failure projection is valid");
		{
			let mut state = quick_tasks.lock();
			state.tasks = vec![failed.clone()];
			state.selected = Some(failed.conversation_id.clone());
		}

		assert_eq!(
			quick_tasks.submit("this draft must not become a recovery command"),
			Err(QuickTaskInputError::NotReady)
		);
		assert_eq!(quick_tasks.archive_selected(), Ok(()));
		let archive = quick_tasks
			.try_take_dispatch(1, &server_id)
			.and_then(|dispatch| dispatch.command().cloned())
			.expect("the explicit archive is dispatchable");
		assert!(matches!(
			archive.payload,
			CommandPayload::ArchiveQuickTask { conversation_id }
				if conversation_id == failed.conversation_id
		));
	}

	#[test]
	fn sidebar_refresh_reconciles_each_provider_thread_then_reloads_the_local_list() {
		let (quick_tasks, server_id, current) = connected_quick_tasks();
		let archived = QuickTaskSummary::new(
			EntityId::new("00000000-0000-4000-8000-000000000003").expect("test ID is valid"),
			EntityRevision(4),
			4,
			Some(
				EntityId::new("00000000-0000-4000-8000-000000000004")
					.expect("test session ID is valid"),
			),
			Some(EntityRevision(4)),
			QuickTaskState::Ready,
			None,
			None,
		)
		.expect("archived provider task is valid");
		let busy = QuickTaskSummary::new(
			EntityId::new("00000000-0000-4000-8000-000000000005").expect("test ID is valid"),
			EntityRevision(2),
			2,
			Some(
				EntityId::new("00000000-0000-4000-8000-000000000006")
					.expect("test session ID is valid"),
			),
			Some(EntityRevision(2)),
			QuickTaskState::Running,
			Some(
				EntityId::new("00000000-0000-4000-8000-000000000007")
					.expect("test turn ID is valid"),
			),
			None,
		)
		.expect("busy provider task is valid");
		let establishing = QuickTaskSummary::new(
			EntityId::new("00000000-0000-4000-8000-000000000008").expect("test ID is valid"),
			EntityRevision(2),
			2,
			Some(
				EntityId::new("00000000-0000-4000-8000-000000000009")
					.expect("test session ID is valid"),
			),
			Some(EntityRevision(2)),
			QuickTaskState::Establishing,
			None,
			Some(QuickTaskRecoveryAction::ResumeEstablishment),
		)
		.expect("establishing task is valid");
		{
			let mut state = quick_tasks.lock();
			state.reset_pagination();
			state.load = QuickTasksLoadState::Ready;
			state.tasks =
				vec![current.clone(), archived.clone(), busy.clone(), establishing.clone()];
		}

		assert_eq!(quick_tasks.refresh_all(), Ok(()));
		assert_eq!(
			quick_tasks.snapshot().refresh,
			QuickTaskRefreshState::Refreshing { completed: 0, total: 4, archived: 0, failed: 0 }
		);

		let first =
			quick_tasks.try_take_dispatch(1, &server_id).expect("first provider refresh is queued");
		let first_command = first.command().expect("first refresh is a command");
		assert!(matches!(
			&first_command.payload,
			CommandPayload::RefreshQuickTask { conversation_id }
				if conversation_id == &current.conversation_id
		));
		quick_tasks.command_sent(&first);
		let first_result = CommandResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			client_command_id: first_command.client_command_id.clone(),
			idempotency_key: first_command.idempotency_key.clone(),
			outcome: CommandOutcome::Succeeded,
			entity_revision: Some(current.conversation_revision),
			payload: Some(ResultPayload::QuickTaskConversationAccepted {
				conversation: current.clone(),
			}),
			error: None,
		};
		assert_eq!(
			quick_tasks.route_command_result(1, &server_id, &first_result),
			QuickTaskRouteOutcome::Fresh
		);

		let second = quick_tasks
			.try_take_dispatch(1, &server_id)
			.expect("second provider refresh is queued");
		let second_command = second.command().expect("second refresh is a command");
		assert!(matches!(
			&second_command.payload,
			CommandPayload::RefreshQuickTask { conversation_id }
				if conversation_id == &archived.conversation_id
		));
		quick_tasks.command_sent(&second);
		let archived_revision = EntityRevision(archived.conversation_revision.0 + 1);
		let second_result = CommandResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			client_command_id: second_command.client_command_id.clone(),
			idempotency_key: second_command.idempotency_key.clone(),
			outcome: CommandOutcome::Succeeded,
			entity_revision: Some(archived_revision),
			payload: Some(ResultPayload::QuickTaskArchived {
				conversation_id: archived.conversation_id.clone(),
				conversation_revision: archived_revision,
			}),
			error: None,
		};
		assert_eq!(
			quick_tasks.route_command_result(1, &server_id, &second_result),
			QuickTaskRouteOutcome::Fresh
		);
		let third = quick_tasks
			.try_take_dispatch(1, &server_id)
			.expect("busy provider refresh is still attempted");
		let third_command = third.command().expect("third refresh is a command");
		assert!(matches!(
			&third_command.payload,
			CommandPayload::RefreshQuickTask { conversation_id }
				if conversation_id == &busy.conversation_id
		));
		quick_tasks.command_sent(&third);
		let third_result = CommandResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			client_command_id: third_command.client_command_id.clone(),
			idempotency_key: third_command.idempotency_key.clone(),
			outcome: CommandOutcome::Rejected,
			entity_revision: None,
			payload: None,
			error: Some(CommandError::QuickTaskRecoveryRequired {
				action: QuickTaskRecoveryAction::WaitForCurrentCommand,
			}),
		};
		assert_eq!(
			quick_tasks.route_command_result(1, &server_id, &third_result),
			QuickTaskRouteOutcome::Fresh
		);
		let fourth = quick_tasks
			.try_take_dispatch(1, &server_id)
			.expect("stale establishment refresh is attempted after live work");
		let fourth_command = fourth.command().expect("fourth refresh is a command");
		assert!(matches!(
			&fourth_command.payload,
			CommandPayload::RefreshQuickTask { conversation_id }
				if conversation_id == &establishing.conversation_id
		));
		quick_tasks.command_sent(&fourth);
		let establishing_revision = EntityRevision(establishing.conversation_revision.0 + 1);
		let fourth_result = CommandResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			client_command_id: fourth_command.client_command_id.clone(),
			idempotency_key: fourth_command.idempotency_key.clone(),
			outcome: CommandOutcome::Succeeded,
			entity_revision: Some(establishing_revision),
			payload: Some(ResultPayload::QuickTaskArchived {
				conversation_id: establishing.conversation_id.clone(),
				conversation_revision: establishing_revision,
			}),
			error: None,
		};
		assert_eq!(
			quick_tasks.route_command_result(1, &server_id, &fourth_result),
			QuickTaskRouteOutcome::Fresh
		);

		let list = quick_tasks
			.try_take_dispatch(1, &server_id)
			.and_then(|dispatch| dispatch.query().cloned())
			.expect("the provider batch ends with authoritative local list readback");
		let list_result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: list.query_id,
			payload: QueryResultPayload::QuickTasks(QuickTaskListResult::Available(
				QuickTaskListPage::new(vec![current.clone(), busy.clone()], None)
					.expect("final list page is valid"),
			)),
		};
		assert_eq!(
			quick_tasks.route_query_result(1, &server_id, &list_result),
			QuickTaskRouteOutcome::Fresh
		);
		let snapshot = quick_tasks.snapshot();
		assert_eq!(snapshot.tasks, vec![current, busy]);
		assert_eq!(
			snapshot.refresh,
			QuickTaskRefreshState::Complete { checked: 4, archived: 2, failed: 1 }
		);
		assert!(snapshot.can_submit);
	}

	#[test]
	fn selected_execution_controls_are_carried_by_each_subsequent_turn() {
		let (quick_tasks, server_id, task) = connected_quick_tasks();
		quick_tasks.cycle_model();
		quick_tasks.cycle_reasoning_effort();
		quick_tasks.toggle_fast();
		let selected = quick_tasks.snapshot().execution;
		assert_eq!(selected.model.as_str(), "gpt-5.6-terra");
		assert_eq!(selected.reasoning_effort, QuickTaskReasoningEffort::XHigh);
		assert!(selected.fast);

		assert!(quick_tasks.submit("Use the selected execution settings.").is_ok());
		let command = quick_tasks
			.try_take_dispatch(1, &server_id)
			.and_then(|dispatch| dispatch.command().cloned())
			.expect("turn command is dispatchable");
		assert_eq!(command.expected_revision, Some(task.conversation_revision));
		assert!(matches!(
			command.payload,
			CommandPayload::SubmitQuickTaskTurn {
				conversation_id,
				execution,
				..
			} if conversation_id == task.conversation_id && execution == selected
		));
	}

	#[test]
	fn accepted_submission_advances_confirmation_and_requests_exact_history_reload() {
		let (quick_tasks, server_id, task) = connected_quick_tasks();
		assert!(quick_tasks.submit("Keep this draft until accepted.").is_ok());
		let dispatch =
			quick_tasks.try_take_dispatch(1, &server_id).expect("submission is dispatchable");
		let command = dispatch.command().expect("submission dispatch is a command").clone();
		let turn_id = match &command.payload {
			CommandPayload::SubmitQuickTaskTurn { turn_id, .. } => turn_id.clone(),
			other => panic!("unexpected submission command: {other:?}"),
		};
		quick_tasks.command_sent(&dispatch);
		let accepted = QuickTaskSummary::new(
			task.conversation_id.clone(),
			EntityRevision(2),
			2,
			task.runtime_session_id.clone(),
			Some(EntityRevision(2)),
			QuickTaskState::Running,
			Some(turn_id),
			None,
		)
		.expect("accepted submission projection is valid");
		let result = CommandResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			client_command_id: command.client_command_id,
			idempotency_key: command.idempotency_key,
			outcome: CommandOutcome::Succeeded,
			entity_revision: Some(accepted.conversation_revision),
			payload: Some(ResultPayload::QuickTaskConversationAccepted { conversation: accepted }),
			error: None,
		};

		assert_eq!(
			quick_tasks.route_command_result(1, &server_id, &result),
			QuickTaskRouteOutcome::Fresh
		);
		let snapshot = quick_tasks.snapshot();
		assert_eq!(snapshot.submission_result_generation, 1);
		assert!(snapshot.last_submission_accepted);
		assert_eq!(quick_tasks.take_history_reload_request(), Some(task.conversation_id));
		assert_eq!(quick_tasks.take_history_reload_request(), None);
	}

	#[test]
	fn archived_submission_rejection_keeps_confirmation_false_and_reloads_authority() {
		let (quick_tasks, server_id, task) = connected_quick_tasks();
		assert!(quick_tasks.submit("Do not lose this draft.").is_ok());
		let dispatch =
			quick_tasks.try_take_dispatch(1, &server_id).expect("submission is dispatchable");
		let command = dispatch.command().expect("submission dispatch is a command").clone();
		quick_tasks.command_sent(&dispatch);
		let result = CommandResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			client_command_id: command.client_command_id,
			idempotency_key: command.idempotency_key,
			outcome: CommandOutcome::Rejected,
			entity_revision: None,
			payload: None,
			error: Some(CommandError::QuickTaskRecoveryRequired {
				action: QuickTaskRecoveryAction::StartNewConversation,
			}),
		};

		assert_eq!(
			quick_tasks.route_command_result(1, &server_id, &result),
			QuickTaskRouteOutcome::Fresh
		);
		let snapshot = quick_tasks.snapshot();
		assert_eq!(snapshot.submission_result_generation, 1);
		assert!(!snapshot.last_submission_accepted);
		assert_eq!(
			snapshot.command,
			QuickTaskCommandState::ManualRecovery(QuickTaskRecoveryAction::StartNewConversation)
		);
		assert_eq!(quick_tasks.take_history_reload_request(), Some(task.conversation_id));
		assert!(matches!(
			quick_tasks.try_take_dispatch(1, &server_id),
			Some(QuickTaskDispatch::Query(QueryEnvelope {
				payload: QueryPayload::ListQuickTasks { .. },
				..
			}))
		));
	}

	#[test]
	fn definite_submission_receipt_refusal_retains_the_draft_without_waiting_for_a_result() {
		let (quick_tasks, server_id, _) = connected_quick_tasks();
		assert!(quick_tasks.submit("Keep this refused draft.").is_ok());
		let dispatch =
			quick_tasks.try_take_dispatch(1, &server_id).expect("submission is dispatchable");
		let command = dispatch.command().expect("submission dispatch is a command").clone();
		quick_tasks.command_sent(&dispatch);
		let receipt = CommandReceipt {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			client_command_id: command.client_command_id.clone(),
			idempotency_key: command.idempotency_key,
			disposition: ReceiptDisposition::Refused,
			original_client_command_id: command.client_command_id,
		};

		assert_eq!(
			quick_tasks.route_receipt(1, &server_id, &receipt),
			QuickTaskRouteOutcome::Fresh
		);
		let snapshot = quick_tasks.snapshot();
		assert_eq!(snapshot.command, QuickTaskCommandState::Refused);
		assert_eq!(snapshot.submission_result_generation, 1);
		assert!(!snapshot.last_submission_accepted);
		assert_eq!(quick_tasks.take_history_reload_request(), None);
	}

	#[test]
	fn sent_command_disconnect_requires_readback_before_explicit_retry() {
		let (quick_tasks, server_id, task) = connected_quick_tasks();
		drop(take_and_mark_command_sent(&quick_tasks, &server_id));

		quick_tasks.session_ended(1);
		{
			let state = quick_tasks.lock();
			assert_eq!(state.command, QuickTaskCommandState::OutcomeUnknown);
			assert!(state.pending_command.is_none());
			assert!(state.in_flight_command.is_none());
		}
		quick_tasks.bind_session(2, server_id.clone());

		assert_eq!(quick_tasks.snapshot().command, QuickTaskCommandState::OutcomeUnknown);
		assert_eq!(quick_tasks.submit("retry explicitly"), Err(QuickTaskInputError::Busy));
		quick_tasks.begin_new();
		assert_eq!(quick_tasks.snapshot().command, QuickTaskCommandState::OutcomeUnknown);
		assert_eq!(quick_tasks.create("new conversation"), Err(QuickTaskInputError::Busy));
		let query = match quick_tasks
			.try_take_dispatch(2, &server_id)
			.expect("reconnect queues authoritative list readback")
		{
			QuickTaskDispatch::Query(query) => query,
			QuickTaskDispatch::Command(_) => panic!("unknown outcome must not auto-resubmit"),
		};
		let result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: query.query_id,
			payload: QueryResultPayload::QuickTasks(QuickTaskListResult::Available(
				QuickTaskListPage::new(vec![task.clone()], None).expect("test page is valid"),
			)),
		};

		assert_eq!(
			quick_tasks.route_query_result(2, &server_id, &result),
			QuickTaskRouteOutcome::Fresh
		);
		let reconciled = quick_tasks.snapshot();
		assert_eq!(reconciled.command, QuickTaskCommandState::Idle);
		assert_eq!(reconciled.tasks, vec![task.clone()]);
		assert!(reconciled.can_submit);
		assert!(quick_tasks.select(task.conversation_id));
		assert!(quick_tasks.submit("retry explicitly").is_ok());
	}

	#[test]
	fn send_failure_requires_readback_before_explicit_retry() {
		let (quick_tasks, server_id, task) = connected_quick_tasks();
		assert!(quick_tasks.submit("possibly accepted").is_ok());
		let dispatch =
			quick_tasks.try_take_dispatch(1, &server_id).expect("queued command is dispatchable");
		assert!(matches!(&dispatch, QuickTaskDispatch::Command(_)));

		quick_tasks.command_send_failed(&dispatch);
		{
			let state = quick_tasks.lock();
			assert_eq!(state.command, QuickTaskCommandState::OutcomeUnknown);
			assert!(state.pending_command.is_none());
			assert!(state.in_flight_command.is_none());
		}

		quick_tasks.session_ended(1);
		quick_tasks.bind_session(2, server_id.clone());
		assert_eq!(quick_tasks.submit("retry explicitly"), Err(QuickTaskInputError::Busy));
		let query = match quick_tasks
			.try_take_dispatch(2, &server_id)
			.expect("reconnect queues authoritative list readback")
		{
			QuickTaskDispatch::Query(query) => query,
			QuickTaskDispatch::Command(_) => panic!("failed send must not auto-resubmit"),
		};
		let result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: query.query_id,
			payload: QueryResultPayload::QuickTasks(QuickTaskListResult::Available(
				QuickTaskListPage::new(vec![task.clone()], None).expect("test page is valid"),
			)),
		};

		assert_eq!(
			quick_tasks.route_query_result(2, &server_id, &result),
			QuickTaskRouteOutcome::Fresh
		);
		assert_eq!(quick_tasks.snapshot().command, QuickTaskCommandState::Idle);
		assert!(quick_tasks.select(task.conversation_id));
		assert!(quick_tasks.submit("retry explicitly").is_ok());
	}

	#[test]
	fn acceptance_unknown_result_retains_the_readback_fence() {
		let (quick_tasks, server_id, _task) = connected_quick_tasks();
		let dispatch = take_and_mark_command_sent(&quick_tasks, &server_id);
		let command = dispatch.command().expect("the sent dispatch is a command");
		let result = CommandResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			client_command_id: command.client_command_id.clone(),
			idempotency_key: command.idempotency_key.clone(),
			outcome: CommandOutcome::AcceptanceUnknown,
			entity_revision: None,
			payload: None,
			error: Some(CommandError::AcceptanceUnknown),
		};

		assert_eq!(
			quick_tasks.route_command_result(1, &server_id, &result),
			QuickTaskRouteOutcome::Fresh
		);
		{
			let state = quick_tasks.lock();
			assert_eq!(state.command, QuickTaskCommandState::OutcomeUnknown);
			assert!(state.pending_command.is_none());
			assert!(state.in_flight_command.is_none());
		}
		assert_eq!(quick_tasks.submit("retry explicitly"), Err(QuickTaskInputError::Busy));
		quick_tasks.begin_new();
		assert_eq!(quick_tasks.snapshot().command, QuickTaskCommandState::OutcomeUnknown);
	}

	#[test]
	fn waiting_recovery_replaces_the_archived_source_with_its_successor() {
		let (quick_tasks, server_id, existing) = connected_quick_tasks();
		let source = QuickTaskSummary::new(
			existing.conversation_id.clone(),
			EntityRevision(1),
			1,
			None,
			None,
			QuickTaskState::QuotaExhausted,
			None,
			Some(QuickTaskRecoveryAction::CreateRoutingSuccessor),
		)
		.expect("waiting source projection is valid");
		{
			let mut state = quick_tasks.lock();
			state.tasks = vec![source.clone()];
			state.selected = Some(source.conversation_id.clone());
		}

		assert_eq!(
			quick_tasks.submit("must remain in the composer"),
			Err(QuickTaskInputError::NotReady)
		);
		assert_eq!(quick_tasks.recover_selected(), Ok(()));
		let dispatch = quick_tasks
			.try_take_dispatch(1, &server_id)
			.expect("routing successor command is dispatchable");
		let command = dispatch.command().expect("recovery dispatch is a command");
		assert!(matches!(
			&command.payload,
			CommandPayload::CreateQuickTaskRoutingSuccessor { conversation_id }
				if conversation_id == &source.conversation_id
		));
		assert_eq!(command.expected_revision, Some(source.conversation_revision));
		quick_tasks.command_sent(&dispatch);

		let successor_id =
			EntityId::new("00000000-0000-4000-8000-000000000003").expect("test ID is valid");
		let successor = QuickTaskSummary::new(
			successor_id.clone(),
			EntityRevision(1),
			1,
			None,
			None,
			QuickTaskState::RoutingPending,
			None,
			Some(QuickTaskRecoveryAction::ResumeRouting),
		)
		.expect("successor projection is valid");
		let result = CommandResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			client_command_id: command.client_command_id.clone(),
			idempotency_key: command.idempotency_key.clone(),
			outcome: CommandOutcome::Succeeded,
			entity_revision: Some(successor.conversation_revision),
			payload: Some(ResultPayload::QuickTaskRoutingSuccessorAccepted {
				source_conversation_id: source.conversation_id.clone(),
				source_conversation_revision: EntityRevision(2),
				successor: successor.clone(),
			}),
			error: None,
		};

		assert_eq!(
			quick_tasks.route_command_result(1, &server_id, &result),
			QuickTaskRouteOutcome::Fresh
		);
		let snapshot = quick_tasks.snapshot();
		assert_eq!(snapshot.tasks, vec![successor]);
		assert_eq!(snapshot.selected, Some(successor_id));
		assert_eq!(snapshot.command, QuickTaskCommandState::Accepted);
	}

	#[test]
	fn routing_successor_lost_response_reconnect_selects_exact_redirect_and_removes_archived_source()
	 {
		let (quick_tasks, server_id, existing) = connected_quick_tasks();
		let source = QuickTaskSummary::new(
			existing.conversation_id,
			EntityRevision(1),
			1,
			None,
			None,
			QuickTaskState::QuotaExhausted,
			None,
			Some(QuickTaskRecoveryAction::CreateRoutingSuccessor),
		)
		.expect("waiting source projection is valid");
		send_routing_successor_and_reconnect_without_result(&quick_tasks, &server_id, &source);

		let successor_id =
			EntityId::new("00000000-0000-4000-8000-000000000003").expect("test ID is valid");
		let successor = QuickTaskSummary::new(
			successor_id.clone(),
			EntityRevision(1),
			2,
			None,
			None,
			QuickTaskState::RoutingPending,
			None,
			Some(QuickTaskRecoveryAction::ResumeRouting),
		)
		.expect("successor projection is valid");
		let list = match quick_tasks
			.try_take_dispatch(2, &server_id)
			.expect("reconnect queues list readback")
		{
			QuickTaskDispatch::Query(query) => query,
			QuickTaskDispatch::Command(_) => panic!("unknown outcome must not resend the command"),
		};
		assert!(matches!(&list.payload, QueryPayload::ListQuickTasks { .. }));
		let list_result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: list.query_id,
			payload: QueryResultPayload::QuickTasks(QuickTaskListResult::Available(
				QuickTaskListPage::new(vec![successor.clone()], None).expect("test page is valid"),
			)),
		};
		assert_eq!(
			quick_tasks.route_query_result(2, &server_id, &list_result),
			QuickTaskRouteOutcome::Fresh
		);
		let listed = quick_tasks.snapshot();
		assert_eq!(listed.selected, Some(source.conversation_id.clone()));
		assert_eq!(listed.command, QuickTaskCommandState::OutcomeUnknown);
		assert!(!listed.can_submit);
		assert_eq!(quick_tasks.submit("must remain fenced"), Err(QuickTaskInputError::Busy));

		let source_query = match quick_tasks
			.try_take_dispatch(2, &server_id)
			.expect("complete list queues the exact source read")
		{
			QuickTaskDispatch::Query(query) => query,
			QuickTaskDispatch::Command(_) => panic!("reconciliation must not resend the command"),
		};
		assert!(matches!(
			&source_query.payload,
			QueryPayload::GetQuickTask { conversation_id }
				if conversation_id == &source.conversation_id
		));
		let redirect_result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: source_query.query_id,
			payload: QueryResultPayload::QuickTask(QuickTaskResult::RoutingSuccessorRedirect {
				source_conversation_id: source.conversation_id.clone(),
				source_conversation_revision: EntityRevision(2),
				successor_conversation_id: successor_id.clone(),
				successor_conversation_revision: successor.conversation_revision,
			}),
		};
		assert_eq!(
			quick_tasks.route_query_result(2, &server_id, &redirect_result),
			QuickTaskRouteOutcome::Fresh
		);

		let successor_query = match quick_tasks
			.try_take_dispatch(2, &server_id)
			.expect("exact redirect queues the successor projection read")
		{
			QuickTaskDispatch::Query(query) => query,
			QuickTaskDispatch::Command(_) => panic!("reconciliation must not resend the command"),
		};
		assert!(matches!(
			&successor_query.payload,
			QueryPayload::GetQuickTask { conversation_id } if conversation_id == &successor_id
		));
		let successor_result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: successor_query.query_id,
			payload: QueryResultPayload::QuickTask(QuickTaskResult::Available(successor.clone())),
		};
		assert_eq!(
			quick_tasks.route_query_result(2, &server_id, &successor_result),
			QuickTaskRouteOutcome::Fresh
		);

		let reconciled = quick_tasks.snapshot();
		assert_eq!(reconciled.tasks, vec![successor]);
		assert_eq!(reconciled.selected, Some(successor_id));
		assert_eq!(reconciled.command, QuickTaskCommandState::Idle);
		assert!(reconciled.can_submit);
	}

	#[test]
	fn routing_successor_mismatched_redirect_keeps_outcome_unknown_and_prohibits_resend() {
		let (quick_tasks, server_id, existing) = connected_quick_tasks();
		let source = QuickTaskSummary::new(
			existing.conversation_id,
			EntityRevision(1),
			1,
			None,
			None,
			QuickTaskState::QuotaExhausted,
			None,
			Some(QuickTaskRecoveryAction::CreateRoutingSuccessor),
		)
		.expect("waiting source projection is valid");
		send_routing_successor_and_reconnect_without_result(&quick_tasks, &server_id, &source);

		let list = quick_tasks
			.try_take_dispatch(2, &server_id)
			.and_then(|dispatch| dispatch.query().cloned())
			.expect("reconnect queues list readback without resending the command");
		let list_result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: list.query_id,
			payload: QueryResultPayload::QuickTasks(QuickTaskListResult::Available(
				QuickTaskListPage::new(Vec::new(), None).expect("test page is valid"),
			)),
		};
		assert_eq!(
			quick_tasks.route_query_result(2, &server_id, &list_result),
			QuickTaskRouteOutcome::Fresh
		);
		let source_query = quick_tasks
			.try_take_dispatch(2, &server_id)
			.and_then(|dispatch| dispatch.query().cloned())
			.expect("complete list queues the exact source read");
		let mismatched_source =
			EntityId::new("00000000-0000-4000-8000-000000000004").expect("test ID is valid");
		let successor_id =
			EntityId::new("00000000-0000-4000-8000-000000000003").expect("test ID is valid");
		let redirect_result = QueryResultEnvelope {
			version: CURRENT_VERSION,
			server_id: server_id.clone(),
			query_id: source_query.query_id,
			payload: QueryResultPayload::QuickTask(QuickTaskResult::RoutingSuccessorRedirect {
				source_conversation_id: mismatched_source,
				source_conversation_revision: EntityRevision(2),
				successor_conversation_id: successor_id,
				successor_conversation_revision: EntityRevision(1),
			}),
		};
		assert_eq!(
			quick_tasks.route_query_result(2, &server_id, &redirect_result),
			QuickTaskRouteOutcome::Refused
		);

		let snapshot = quick_tasks.snapshot();
		assert_eq!(snapshot.selected, Some(source.conversation_id));
		assert_eq!(snapshot.command, QuickTaskCommandState::OutcomeUnknown);
		assert!(!snapshot.can_submit);
		assert_eq!(quick_tasks.submit("must remain fenced"), Err(QuickTaskInputError::Busy));
		assert!(quick_tasks.try_take_dispatch(2, &server_id).is_none());
	}
}
