//! Presentation-neutral ownership of the narrow internal WorkItem factory slice.

use std::{
	path::Path,
	sync::{
		Arc, Mutex, MutexGuard,
		atomic::{AtomicU64, Ordering},
	},
	time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use decodex_protocol::{
	CURRENT_VERSION, CausationId, ClientCommandId, CommandEnvelope, CommandOutcome, CommandPayload,
	CommandReceipt, CommandResultEnvelope, CorrelationId, EntityId, EntityRevision, EventEnvelope,
	EventPayload, IdempotencyKey, MAX_PROJECT_LIST_ITEMS, ProjectListResult, ProjectSummary,
	QueryEnvelope, QueryId, QueryPayload, QueryResultEnvelope, QueryResultPayload,
	QuickTaskSummary, ReceiptDisposition, ResultPayload, ServerId, WireText, WorkItemBoardCard,
	WorkItemBoardLeadId, WorkItemBoardPageSize, WorkItemBoardProjectId, WorkItemBoardResult,
	WorkItemBoardTitle, WorkItemBoardWorkItemId, WorkItemState,
};

const BOARD_PAGE_SIZE: u16 = 64;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Current bounded state rendered by the Factory destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkItemsSnapshot {
	pub(crate) load: WorkItemsLoadState,
	pub(crate) command: WorkItemCommandState,
	pub(crate) projects: Vec<ProjectSummary>,
	pub(crate) selected_project: Option<WorkItemBoardProjectId>,
	pub(crate) cards: Vec<WorkItemBoardCard>,
	pub(crate) can_mutate: bool,
}

impl WorkItemsSnapshot {
	pub(crate) fn selected_project_summary(&self) -> Option<&ProjectSummary> {
		let selected = self.selected_project.as_ref()?;
		self.projects.iter().find(|project| project.project_id() == selected)
	}
}

/// Finite readback state. The daemon-owned product store remains authoritative.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkItemsLoadState {
	NeverRequested,
	LoadingProjects,
	LoadingBoard,
	Ready,
	NoProjects,
	Offline,
	Unavailable,
	Refused,
}

/// Finite state for the one possibly side-effecting WorkItem command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkItemCommandState {
	Idle,
	Sending,
	AwaitingResult,
	Accepted,
	OutcomeUnknown,
	Refused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkItemInputError {
	Offline,
	Busy,
	NoProject,
	InvalidTitle,
	InvalidDescription,
	InvalidRepository,
	InvalidState,
	IdentityUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkItemRouteOutcome {
	Fresh,
	Unmatched,
	Refused,
}

/// Exactly one query or command reserved for one retained-session generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkItemDispatch {
	Query(QueryEnvelope),
	Command(CommandEnvelope),
}

impl WorkItemDispatch {
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

/// Cloneable controller. It owns no transport, task, or product authority.
#[derive(Clone)]
pub(crate) struct WorkItems {
	inner: Arc<WorkItemsInner>,
}

struct WorkItemsInner {
	state: Mutex<State>,
	notify: Notify,
}

impl WorkItems {
	pub(crate) fn production() -> Self {
		Self {
			inner: Arc::new(WorkItemsInner {
				state: Mutex::new(State::new()),
				notify: Notify::new(),
			}),
		}
	}

	#[cfg(feature = "visual-capture")]
	#[allow(dead_code)]
	pub(crate) fn visual_no_projects() -> Self {
		let work_items = Self::production();
		{
			let mut state = work_items.lock();
			state.active = true;
			state.session = Some(SessionBinding {
				generation: 1,
				server_id: ServerId::new("visual-capture")
					.expect("visual capture server identity is bounded"),
			});
			state.load = WorkItemsLoadState::NoProjects;
		}
		work_items
	}

	pub(crate) fn activate(&self) {
		let mut state = self.lock();
		state.active = true;
		let queued = state.queue_projects();
		if state.session.is_none() {
			state.load = WorkItemsLoadState::Offline;
		}
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn deactivate(&self) {
		self.lock().active = false;
	}

	pub(crate) fn snapshot(&self) -> WorkItemsSnapshot {
		self.lock().snapshot()
	}

	pub(crate) fn select_project(&self, project_id: WorkItemBoardProjectId) -> bool {
		let mut state = self.lock();
		if !state.projects.iter().any(|project| project.project_id() == &project_id) {
			return false;
		}
		if state.selected_project.as_ref() == Some(&project_id) {
			return true;
		}
		state.selected_project = Some(project_id);
		state.cards.clear();
		let queued = state.queue_board();
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
		true
	}

	pub(crate) fn create(&self, title: &str, description: &str) -> Result<(), WorkItemInputError> {
		let title = title.trim();
		let description = description.trim();
		let title = WorkItemBoardTitle::new(title.to_owned())
			.map_err(|_| WorkItemInputError::InvalidTitle)?;
		if description.is_empty() || description.chars().any(char::is_control) {
			return Err(WorkItemInputError::InvalidDescription);
		}
		let description = WireText::new(description.to_owned())
			.map_err(|_| WorkItemInputError::InvalidDescription)?;
		let project_id =
			self.lock().selected_project.clone().ok_or(WorkItemInputError::NoProject)?;
		let work_item_id = WorkItemBoardWorkItemId::new(canonical_uuid_v4()?)
			.map_err(|_| WorkItemInputError::IdentityUnavailable)?;
		self.queue_command(
			CommandPayload::CreateWorkItem { work_item_id, project_id, title, description },
			None,
		)
	}

	pub(crate) fn register_project(&self, repository_root: &str) -> Result<(), WorkItemInputError> {
		let repository_root = normalize_repository_root(repository_root)?;
		let repository_identity = repository_identity(&repository_root)?;
		let project_id = WorkItemBoardProjectId::new(canonical_uuid_v4()?)
			.map_err(|_| WorkItemInputError::IdentityUnavailable)?;
		let lead_id = WorkItemBoardLeadId::new(canonical_uuid_v4()?)
			.map_err(|_| WorkItemInputError::IdentityUnavailable)?;
		let repository_root =
			WireText::new(repository_root).map_err(|_| WorkItemInputError::InvalidRepository)?;
		self.queue_command(
			CommandPayload::RegisterProject {
				project_id,
				lead_id,
				repository_identity,
				repository_root,
			},
			None,
		)
	}

	pub(crate) fn start(&self, card: &WorkItemBoardCard) -> Result<(), WorkItemInputError> {
		if card.state() != WorkItemState::Ready {
			return Err(WorkItemInputError::InvalidState);
		}
		let conversation_id = entity_id()?;
		self.queue_command(
			CommandPayload::StartWorkItem {
				work_item_id: card.work_item_id().clone(),
				project_id: card.project_id().clone(),
				conversation_id,
			},
			Some(card.revision()),
		)
	}

	pub(crate) fn accept(&self, card: &WorkItemBoardCard) -> Result<(), WorkItemInputError> {
		if card.state() != WorkItemState::Review {
			return Err(WorkItemInputError::InvalidState);
		}
		let acceptance_id = entity_id()?;
		let evidence_summary =
			WireText::new("Operator reviewed the persisted Codex result in Decodex.".to_owned())
				.expect("fixed acceptance evidence is bounded");
		self.queue_command(
			CommandPayload::AcceptWorkItem {
				work_item_id: card.work_item_id().clone(),
				project_id: card.project_id().clone(),
				acceptance_id,
				evidence_summary,
			},
			Some(card.revision()),
		)
	}

	/// Take the exact Conversation returned by a successful start command once.
	pub(crate) fn take_started_conversation(&self) -> Option<QuickTaskSummary> {
		self.lock().started_conversation.take()
	}

	fn queue_command(
		&self,
		payload: CommandPayload,
		expected_revision: Option<EntityRevision>,
	) -> Result<(), WorkItemInputError> {
		let mut state = self.lock();
		if state.session.is_none() {
			return Err(WorkItemInputError::Offline);
		}
		if state.pending_command.is_some()
			|| state.in_flight_command.is_some()
			|| state.outcome_unknown_command.is_some()
			|| state.command == WorkItemCommandState::OutcomeUnknown
		{
			return Err(WorkItemInputError::Busy);
		}
		let identity = command_identity()?;
		state.pending_command = Some(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: identity.client_command_id,
			idempotency_key: identity.idempotency_key,
			expected_revision,
			correlation_id: identity.correlation_id,
			causation_id: None::<CausationId>,
			payload,
		});
		state.command = WorkItemCommandState::Sending;
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
		state.reset_queries();
		state.session = Some(binding);
		let command_queued = state.pending_command.is_some();
		let query_queued = state.queue_projects();
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
		state.reset_queries();
		state.session = None;
		state.load = WorkItemsLoadState::Offline;
	}

	pub(crate) async fn next_dispatch(
		&self,
		generation: u64,
		server_id: &ServerId,
	) -> WorkItemDispatch {
		loop {
			let notified = self.inner.notify.notified();
			if let Some(dispatch) = self.try_take_dispatch(generation, server_id) {
				return dispatch;
			}
			notified.await;
		}
	}

	fn try_take_dispatch(&self, generation: u64, server_id: &ServerId) -> Option<WorkItemDispatch> {
		let mut state = self.lock();
		let binding = SessionBinding { generation, server_id: server_id.clone() };
		if state.session.as_ref() != Some(&binding) {
			return None;
		}
		if state.in_flight_command.is_none()
			&& let Some(envelope) = state.pending_command.take()
		{
			state.in_flight_command = Some(InFlightCommand { envelope: envelope.clone(), binding });
			return Some(WorkItemDispatch::Command(envelope));
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
			return Some(WorkItemDispatch::Query(envelope));
		}
		None
	}

	pub(crate) fn command_send_failed(&self, dispatch: &WorkItemDispatch) {
		let Some(command) = dispatch.command() else {
			return;
		};
		let mut state = self.lock();
		if state.in_flight_command.as_ref().is_some_and(|in_flight| {
			in_flight.envelope.client_command_id == command.client_command_id
		}) {
			state.latch_in_flight_outcome_unknown();
		}
	}

	pub(crate) fn command_sent(&self, dispatch: &WorkItemDispatch) {
		let Some(command) = dispatch.command() else {
			return;
		};
		let mut state = self.lock();
		if state.in_flight_command.as_ref().is_some_and(|in_flight| {
			in_flight.envelope.client_command_id == command.client_command_id
		}) {
			state.command = WorkItemCommandState::AwaitingResult;
		}
	}

	pub(crate) fn apply_event(&self, event: &EventEnvelope) {
		let mut state = self.lock();
		let mut query_queued = false;
		match &event.payload {
			EventPayload::ProjectChanged { project } => {
				state.upsert_project(project.clone());
				if state.selected_project.is_none() {
					state.selected_project = Some(project.project_id().clone());
					query_queued = state.queue_board();
				}
			},
			EventPayload::WorkItemChanged { work_item } => state.upsert_card(work_item.clone()),
			EventPayload::QuickTaskTurnFinished { conversation, .. }
				if state
					.cards
					.iter()
					.any(|card| card.conversation_id() == Some(&conversation.conversation_id)) =>
			{
				query_queued = state.queue_board();
			},
			_ => {},
		}
		drop(state);
		if query_queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn route_query_result(
		&self,
		generation: u64,
		server_id: &ServerId,
		result: &QueryResultEnvelope,
	) -> WorkItemRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_query.as_ref() else {
			return WorkItemRouteOutcome::Unmatched;
		};
		if in_flight.query_id != result.query_id {
			return WorkItemRouteOutcome::Unmatched;
		}
		let purpose = in_flight.purpose.clone();
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| result.version != CURRENT_VERSION
			|| result.server_id != *server_id
		{
			state.in_flight_query = None;
			state.load = WorkItemsLoadState::Refused;
			return WorkItemRouteOutcome::Refused;
		}
		state.in_flight_query = None;
		let (outcome, query_queued) = state.route_query_payload(purpose, &result.payload);
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
	) -> WorkItemRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_command.as_ref() else {
			return WorkItemRouteOutcome::Unmatched;
		};
		if in_flight.envelope.client_command_id != receipt.client_command_id {
			return WorkItemRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| receipt.version != CURRENT_VERSION
			|| receipt.server_id != *server_id
			|| receipt.idempotency_key != in_flight.envelope.idempotency_key
		{
			state.latch_in_flight_outcome_unknown();
			return WorkItemRouteOutcome::Refused;
		}
		match receipt.disposition {
			ReceiptDisposition::Executed | ReceiptDisposition::Duplicate => {
				state.command = WorkItemCommandState::AwaitingResult;
			},
			ReceiptDisposition::Refused => {
				state.in_flight_command = None;
				state.outcome_unknown_command = None;
				state.command = WorkItemCommandState::Refused;
			},
		}
		WorkItemRouteOutcome::Fresh
	}

	pub(crate) fn route_command_result(
		&self,
		generation: u64,
		server_id: &ServerId,
		result: &CommandResultEnvelope,
	) -> WorkItemRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_command.as_ref() else {
			return WorkItemRouteOutcome::Unmatched;
		};
		if in_flight.envelope.client_command_id != result.client_command_id {
			return WorkItemRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| result.version != CURRENT_VERSION
			|| result.server_id != *server_id
			|| result.idempotency_key != in_flight.envelope.idempotency_key
		{
			state.latch_in_flight_outcome_unknown();
			return WorkItemRouteOutcome::Refused;
		}
		let in_flight =
			state.in_flight_command.take().expect("matching WorkItem command remains in flight");
		match result.outcome {
			CommandOutcome::Succeeded => {
				let Some(accepted) = accepted_result(&in_flight.envelope, result) else {
					state.command = WorkItemCommandState::Refused;
					return WorkItemRouteOutcome::Refused;
				};
				let query_queued = match accepted {
					AcceptedResult::Project(project) => {
						state.upsert_project(project.clone());
						state.selected_project = Some(project.project_id().clone());
						state.cards.clear();
						state.queue_board()
					},
					AcceptedResult::WorkItem { work_item, conversation } => {
						state.upsert_card(*work_item);
						state.started_conversation = conversation;
						false
					},
				};
				state.outcome_unknown_command = None;
				state.command = WorkItemCommandState::Accepted;
				drop(state);
				if query_queued {
					self.inner.notify.notify_one();
				}
				WorkItemRouteOutcome::Fresh
			},
			CommandOutcome::AcceptanceUnknown => {
				state.outcome_unknown_command = Some(in_flight.envelope);
				state.command = WorkItemCommandState::OutcomeUnknown;
				let queued = if state.outcome_unknown_command.as_ref().is_some_and(|command| {
					matches!(command.payload, CommandPayload::RegisterProject { .. })
				}) {
					state.queue_projects()
				} else {
					state.queue_board()
				};
				drop(state);
				if queued {
					self.inner.notify.notify_one();
				}
				WorkItemRouteOutcome::Fresh
			},
			CommandOutcome::Rejected => {
				state.outcome_unknown_command = None;
				state.command = WorkItemCommandState::Refused;
				WorkItemRouteOutcome::Fresh
			},
		}
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
	load: WorkItemsLoadState,
	command: WorkItemCommandState,
	projects: Vec<ProjectSummary>,
	selected_project: Option<WorkItemBoardProjectId>,
	cards: Vec<WorkItemBoardCard>,
	pending_query: Option<PendingQuery>,
	in_flight_query: Option<InFlightQuery>,
	pending_command: Option<CommandEnvelope>,
	in_flight_command: Option<InFlightCommand>,
	outcome_unknown_command: Option<CommandEnvelope>,
	next_query_sequence: u64,
	started_conversation: Option<QuickTaskSummary>,
}

impl State {
	fn new() -> Self {
		Self {
			session: None,
			active: false,
			load: WorkItemsLoadState::NeverRequested,
			command: WorkItemCommandState::Idle,
			projects: Vec::new(),
			selected_project: None,
			cards: Vec::new(),
			pending_query: None,
			in_flight_query: None,
			pending_command: None,
			in_flight_command: None,
			outcome_unknown_command: None,
			next_query_sequence: 0,
			started_conversation: None,
		}
	}

	fn reset_queries(&mut self) {
		self.pending_query = None;
		self.in_flight_query = None;
	}

	fn queue_projects(&mut self) -> bool {
		if !self.active || self.pending_query.is_some() || self.in_flight_query.is_some() {
			return false;
		}
		let queued = self.queue_query(QueryPayload::ListProjects, QueryPurpose::Projects);
		if queued {
			self.load = WorkItemsLoadState::LoadingProjects;
		}
		queued
	}

	fn queue_board(&mut self) -> bool {
		let Some(project_id) = self.selected_project.clone() else {
			return false;
		};
		if !self.active || self.pending_query.is_some() || self.in_flight_query.is_some() {
			return false;
		}
		let page_size =
			WorkItemBoardPageSize::new(BOARD_PAGE_SIZE).expect("constant board page size is valid");
		let queued = self.queue_query(
			QueryPayload::GetWorkItemBoardPage {
				project_id: project_id.clone(),
				state: None,
				after: None,
				page_size,
			},
			QueryPurpose::Board { project_id, page_size },
		);
		if queued {
			self.load = WorkItemsLoadState::LoadingBoard;
		}
		queued
	}

	fn queue_query(&mut self, payload: QueryPayload, purpose: QueryPurpose) -> bool {
		let Some(generation) = self.session.as_ref().map(|session| session.generation) else {
			return false;
		};
		if !self.active || self.pending_query.is_some() || self.in_flight_query.is_some() {
			return false;
		}
		let Some(sequence) = self.next_query_sequence.checked_add(1) else {
			self.load = WorkItemsLoadState::Refused;
			return false;
		};
		self.next_query_sequence = sequence;
		self.pending_query = Some(PendingQuery {
			envelope: QueryEnvelope {
				version: CURRENT_VERSION,
				query_id: QueryId::new(format!("gpui-work-items/{generation}/{sequence}"))
					.expect("bounded numeric WorkItem query identity"),
				payload,
			},
			purpose,
		});
		true
	}

	fn route_query_payload(
		&mut self,
		purpose: QueryPurpose,
		payload: &QueryResultPayload,
	) -> (WorkItemRouteOutcome, bool) {
		match (purpose, payload) {
			(
				QueryPurpose::Projects,
				QueryResultPayload::Projects(ProjectListResult::Available(list)),
			) => {
				self.projects = list.projects().to_vec();
				let command_queued = self.reconcile_outcome_unknown();
				let selected_exists = self.selected_project.as_ref().is_some_and(|selected| {
					self.projects.iter().any(|project| project.project_id() == selected)
				});
				if !selected_exists {
					self.selected_project =
						self.projects.first().map(|project| project.project_id().clone());
					self.cards.clear();
				}
				if command_queued {
					(WorkItemRouteOutcome::Fresh, true)
				} else if self.selected_project.is_none() {
					self.load = WorkItemsLoadState::NoProjects;
					(WorkItemRouteOutcome::Fresh, false)
				} else {
					let queued = self.queue_board();
					(WorkItemRouteOutcome::Fresh, queued)
				}
			},
			(
				QueryPurpose::Projects,
				QueryResultPayload::Projects(ProjectListResult::Unavailable),
			) => {
				self.load = WorkItemsLoadState::Unavailable;
				(WorkItemRouteOutcome::Fresh, false)
			},
			(
				QueryPurpose::Board { project_id, page_size },
				QueryResultPayload::WorkItemBoard(WorkItemBoardResult::Page(page)),
			) if page.matches_request(&project_id, None, None, page_size)
				&& self.selected_project.as_ref() == Some(&project_id) =>
			{
				self.cards = page.cards().to_vec();
				self.load = if page.next_cursor().is_some() {
					WorkItemsLoadState::Refused
				} else {
					WorkItemsLoadState::Ready
				};
				let command_queued = if page.next_cursor().is_none() {
					self.reconcile_outcome_unknown()
				} else {
					false
				};
				(WorkItemRouteOutcome::Fresh, command_queued)
			},
			(
				QueryPurpose::Board { .. },
				QueryResultPayload::WorkItemBoard(WorkItemBoardResult::Unavailable { .. }),
			) => {
				self.load = WorkItemsLoadState::Unavailable;
				(WorkItemRouteOutcome::Fresh, false)
			},
			_ => {
				self.load = WorkItemsLoadState::Refused;
				(WorkItemRouteOutcome::Refused, false)
			},
		}
	}

	fn upsert_card(&mut self, card: WorkItemBoardCard) {
		if self.selected_project.as_ref() != Some(card.project_id()) {
			return;
		}
		if let Some(existing) =
			self.cards.iter_mut().find(|existing| existing.work_item_id() == card.work_item_id())
		{
			if card.revision() >= existing.revision() {
				*existing = card;
			}
		} else {
			self.cards.push(card);
			self.cards.sort_by(|left, right| left.work_item_id().cmp(right.work_item_id()));
		}
	}

	fn upsert_project(&mut self, project: ProjectSummary) {
		if let Some(existing) = self.projects.iter_mut().find(|existing| {
			existing.project_id() == project.project_id()
				|| existing.repository_identity() == project.repository_identity()
		}) {
			*existing = project;
		} else {
			self.projects.push(project);
			self.projects.sort_by(|left, right| left.project_id().cmp(right.project_id()));
		}
	}

	fn latch_in_flight_outcome_unknown(&mut self) {
		if let Some(in_flight) = self.in_flight_command.take() {
			self.pending_command = None;
			self.outcome_unknown_command = Some(in_flight.envelope);
			self.command = WorkItemCommandState::OutcomeUnknown;
		}
	}

	fn reconcile_outcome_unknown(&mut self) -> bool {
		let Some(command) = self.outcome_unknown_command.as_ref() else {
			return false;
		};
		let expected_revision = command.expected_revision;
		let evidence = match &command.payload {
			CommandPayload::RegisterProject { repository_identity, .. } => {
				match self
					.projects
					.iter()
					.find(|project| project.repository_identity() == repository_identity)
				{
					Some(project) => {
						self.selected_project = Some(project.project_id().clone());
						ReconciliationEvidence::Committed
					},
					None if self.projects.len() < MAX_PROJECT_LIST_ITEMS => {
						ReconciliationEvidence::RetryExact
					},
					None => ReconciliationEvidence::Conflicted,
				}
			},
			CommandPayload::CreateWorkItem { work_item_id, project_id, title, description } => {
				match self.cards.iter().find(|card| card.work_item_id() == work_item_id) {
					Some(card)
						if card.project_id() == project_id
							&& card.title() == title
							&& card.description() == description =>
					{
						ReconciliationEvidence::Committed
					},
					Some(_) => ReconciliationEvidence::Conflicted,
					None => ReconciliationEvidence::RetryExact,
				}
			},
			CommandPayload::StartWorkItem { work_item_id, project_id, conversation_id } => {
				match self.cards.iter().find(|card| card.work_item_id() == work_item_id) {
					Some(card)
						if card.project_id() == project_id
							&& card.conversation_id() == Some(conversation_id)
							&& matches!(
								card.state(),
								WorkItemState::Running
									| WorkItemState::Review | WorkItemState::Done
							) =>
					{
						ReconciliationEvidence::Committed
					},
					Some(card)
						if card.project_id() == project_id
							&& card.state() == WorkItemState::Ready
							&& card.conversation_id().is_none()
							&& Some(card.revision()) == expected_revision =>
					{
						ReconciliationEvidence::RetryExact
					},
					_ => ReconciliationEvidence::Conflicted,
				}
			},
			CommandPayload::AcceptWorkItem { work_item_id, project_id, .. } => {
				match self.cards.iter().find(|card| card.work_item_id() == work_item_id) {
					Some(card)
						if card.project_id() == project_id
							&& card.state() == WorkItemState::Done
							&& card.accepted_revision() == expected_revision =>
					{
						ReconciliationEvidence::Committed
					},
					Some(card)
						if card.project_id() == project_id
							&& card.state() == WorkItemState::Review
							&& Some(card.revision()) == expected_revision =>
					{
						ReconciliationEvidence::RetryExact
					},
					_ => ReconciliationEvidence::Conflicted,
				}
			},
			_ => ReconciliationEvidence::Conflicted,
		};
		match evidence {
			ReconciliationEvidence::Committed => {
				self.outcome_unknown_command = None;
				self.command = WorkItemCommandState::Accepted;
				false
			},
			ReconciliationEvidence::RetryExact => {
				self.pending_command = self.outcome_unknown_command.take();
				self.command = WorkItemCommandState::Sending;
				true
			},
			ReconciliationEvidence::Conflicted => {
				self.outcome_unknown_command = None;
				self.command = WorkItemCommandState::Refused;
				false
			},
		}
	}

	fn snapshot(&self) -> WorkItemsSnapshot {
		WorkItemsSnapshot {
			load: self.load,
			command: self.command,
			projects: self.projects.clone(),
			selected_project: self.selected_project.clone(),
			cards: self.cards.clone(),
			can_mutate: self.session.is_some()
				&& self.pending_command.is_none()
				&& self.in_flight_command.is_none()
				&& self.outcome_unknown_command.is_none()
				&& self.command != WorkItemCommandState::OutcomeUnknown,
		}
	}
}

struct PendingQuery {
	envelope: QueryEnvelope,
	purpose: QueryPurpose,
}

struct InFlightQuery {
	query_id: QueryId,
	binding: SessionBinding,
	purpose: QueryPurpose,
}

struct InFlightCommand {
	envelope: CommandEnvelope,
	binding: SessionBinding,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum QueryPurpose {
	Projects,
	Board { project_id: WorkItemBoardProjectId, page_size: WorkItemBoardPageSize },
}

enum ReconciliationEvidence {
	Committed,
	RetryExact,
	Conflicted,
}

enum AcceptedResult {
	Project(ProjectSummary),
	WorkItem { work_item: Box<WorkItemBoardCard>, conversation: Option<QuickTaskSummary> },
}

fn accepted_result(
	command: &CommandEnvelope,
	result: &CommandResultEnvelope,
) -> Option<AcceptedResult> {
	if let (
		CommandPayload::RegisterProject { repository_identity, .. },
		Some(ResultPayload::ProjectRegistered { project }),
	) = (&command.payload, result.payload.as_ref())
	{
		return (project.repository_identity() == repository_identity
			&& result.entity_revision.is_some_and(|revision| revision.0 > 0))
		.then(|| AcceptedResult::Project(project.clone()));
	}
	let (work_item, conversation) = match (&command.payload, result.payload.as_ref()) {
		(
			CommandPayload::CreateWorkItem { work_item_id, project_id, .. },
			Some(ResultPayload::WorkItemChanged { work_item }),
		) if work_item.work_item_id() == work_item_id
			&& work_item.project_id() == project_id
			&& work_item.state() == WorkItemState::Ready =>
		{
			(work_item, None)
		},
		(
			CommandPayload::StartWorkItem { work_item_id, project_id, conversation_id },
			Some(ResultPayload::WorkItemStarted { work_item, conversation }),
		) if work_item.work_item_id() == work_item_id
			&& work_item.project_id() == project_id
			&& work_item.state() == WorkItemState::Running
			&& work_item.conversation_id() == Some(conversation_id)
			&& conversation.conversation_id == *conversation_id =>
		{
			(work_item, Some(conversation.clone()))
		},
		(
			CommandPayload::AcceptWorkItem { work_item_id, project_id, .. },
			Some(ResultPayload::WorkItemChanged { work_item }),
		) if work_item.work_item_id() == work_item_id
			&& work_item.project_id() == project_id
			&& work_item.state() == WorkItemState::Done =>
		{
			(work_item, None)
		},
		_ => return None,
	};
	(result.entity_revision == Some(work_item.revision()))
		.then(|| AcceptedResult::WorkItem {
			work_item: Box::new(work_item.clone()),
			conversation,
		})
}

fn normalize_repository_root(value: &str) -> Result<String, WorkItemInputError> {
	let mut value = value.trim().to_owned();
	while value.len() > 1 && value.ends_with('/') {
		value.pop();
	}
	let path = Path::new(&value);
	if value.len() > 4_096
		|| value.chars().any(char::is_control)
		|| !path.is_absolute()
		|| path.parent().is_none()
		|| value.contains("//")
		|| value.contains('\\')
		|| path.components().any(|component| {
			matches!(component, std::path::Component::CurDir | std::path::Component::ParentDir)
		}) {
		Err(WorkItemInputError::InvalidRepository)
	} else {
		Ok(value)
	}
}

fn repository_identity(repository_root: &str) -> Result<WireText, WorkItemInputError> {
	let name = Path::new(repository_root)
		.file_name()
		.and_then(|value| value.to_str())
		.ok_or(WorkItemInputError::InvalidRepository)?;
	let mut slug = String::new();
	let mut separator = false;
	for byte in name.bytes() {
		let byte = byte.to_ascii_lowercase();
		if byte.is_ascii_alphanumeric() {
			slug.push(char::from(byte));
			separator = false;
		} else if !slug.is_empty() && !separator {
			slug.push('-');
			separator = true;
		}
		if slug.len() >= 48 {
			break;
		}
	}
	while slug.ends_with('-') {
		slug.pop();
	}
	if slug.is_empty() {
		slug.push_str("repository");
	}
	let digest = Sha256::digest(repository_root.as_bytes());
	let suffix = digest[..6].iter().map(|byte| format!("{byte:02x}")).collect::<String>();
	WireText::new(format!("local/{slug}-{suffix}"))
		.map_err(|_| WorkItemInputError::InvalidRepository)
}

struct CommandIdentity {
	client_command_id: ClientCommandId,
	idempotency_key: IdempotencyKey,
	correlation_id: CorrelationId,
}

fn command_identity() -> Result<CommandIdentity, WorkItemInputError> {
	let value = canonical_uuid_v4()?;
	Ok(CommandIdentity {
		client_command_id: ClientCommandId::new(format!("gpui/{value}"))
			.expect("canonical command identity is bounded"),
		idempotency_key: IdempotencyKey::new(format!("work-item/{value}"))
			.expect("canonical idempotency key is bounded"),
		correlation_id: CorrelationId::new(value)
			.expect("canonical correlation identity is bounded"),
	})
}

fn entity_id() -> Result<EntityId, WorkItemInputError> {
	EntityId::new(canonical_uuid_v4()?).map_err(|_| WorkItemInputError::IdentityUnavailable)
}

fn canonical_uuid_v4() -> Result<String, WorkItemInputError> {
	let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_err(|_| WorkItemInputError::IdentityUnavailable)?
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

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::{WorkItemBoardLeadId, WorkItemPriority};

	const PROJECT_ID: &str = "11000000-0000-4000-8000-000000000001";
	const LEAD_ID: &str = "21000000-0000-4000-8000-000000000001";
	const WORK_ITEM_ID: &str = "31000000-0000-4000-8000-000000000001";
	const CONVERSATION_ID: &str = "41000000-0000-4000-8000-000000000001";

	fn envelope(payload: CommandPayload, expected_revision: Option<u64>) -> CommandEnvelope {
		CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: ClientCommandId::new("gpui/test-command").unwrap(),
			idempotency_key: IdempotencyKey::new("work-item/test-command").unwrap(),
			expected_revision: expected_revision.map(EntityRevision),
			correlation_id: CorrelationId::new("51000000-0000-4000-8000-000000000001").unwrap(),
			causation_id: None,
			payload,
		}
	}

	fn card(
		state: WorkItemState,
		revision: u64,
		accepted_revision: Option<u64>,
		conversation_id: Option<&str>,
	) -> WorkItemBoardCard {
		WorkItemBoardCard::new(
			WorkItemBoardWorkItemId::new(WORK_ITEM_ID).unwrap(),
			WorkItemBoardProjectId::new(PROJECT_ID).unwrap(),
			WorkItemBoardLeadId::new(LEAD_ID).unwrap(),
			None,
			Vec::new(),
			Vec::new(),
			Vec::new(),
			WorkItemBoardTitle::new("Implement one exact slice").unwrap(),
			WireText::new("Deliver persisted evidence.").unwrap(),
			WorkItemPriority::Medium,
			state,
			EntityRevision(revision),
			accepted_revision.map(EntityRevision),
			conversation_id.map(|value| EntityId::new(value).unwrap()),
		)
		.unwrap()
	}

	fn project() -> ProjectSummary {
		ProjectSummary::new(
			WorkItemBoardProjectId::new(PROJECT_ID).unwrap(),
			WorkItemBoardLeadId::new(LEAD_ID).unwrap(),
			WireText::new("local/decodex-0123456789ab").unwrap(),
		)
		.unwrap()
	}

	#[test]
	fn unknown_project_registration_requeues_only_after_complete_absence_readback() {
		let mut state = State::new();
		let command = envelope(
			CommandPayload::RegisterProject {
				project_id: WorkItemBoardProjectId::new(PROJECT_ID).unwrap(),
				lead_id: WorkItemBoardLeadId::new(LEAD_ID).unwrap(),
				repository_identity: WireText::new("local/decodex-0123456789ab").unwrap(),
				repository_root: WireText::new("/Users/x/code/acg-box/decodex").unwrap(),
			},
			None,
		);
		state.command = WorkItemCommandState::OutcomeUnknown;
		state.outcome_unknown_command = Some(command.clone());

		assert!(state.reconcile_outcome_unknown());
		assert_eq!(state.pending_command, Some(command));
		assert!(state.outcome_unknown_command.is_none());
	}

	#[test]
	fn project_readback_proves_unknown_registration_committed() {
		let mut state = State::new();
		state.projects.push(project());
		state.command = WorkItemCommandState::OutcomeUnknown;
		state.outcome_unknown_command = Some(envelope(
			CommandPayload::RegisterProject {
				project_id: WorkItemBoardProjectId::new("11000000-0000-4000-8000-000000000099")
					.unwrap(),
				lead_id: WorkItemBoardLeadId::new("21000000-0000-4000-8000-000000000099").unwrap(),
				repository_identity: WireText::new("local/decodex-0123456789ab").unwrap(),
				repository_root: WireText::new("/Users/x/code/acg-box/decodex").unwrap(),
			},
			None,
		));

		assert!(!state.reconcile_outcome_unknown());
		assert_eq!(state.command, WorkItemCommandState::Accepted);
		assert_eq!(state.selected_project, Some(WorkItemBoardProjectId::new(PROJECT_ID).unwrap()));
	}

	#[test]
	fn unknown_create_absence_requeues_the_exact_idempotent_command() {
		let mut state = State::new();
		let command = envelope(
			CommandPayload::CreateWorkItem {
				work_item_id: WorkItemBoardWorkItemId::new(WORK_ITEM_ID).unwrap(),
				project_id: WorkItemBoardProjectId::new(PROJECT_ID).unwrap(),
				title: WorkItemBoardTitle::new("Implement one exact slice").unwrap(),
				description: WireText::new("Deliver persisted evidence.").unwrap(),
			},
			None,
		);
		state.command = WorkItemCommandState::OutcomeUnknown;
		state.outcome_unknown_command = Some(command.clone());

		assert!(state.reconcile_outcome_unknown());
		assert_eq!(state.command, WorkItemCommandState::Sending);
		assert_eq!(state.pending_command, Some(command));
		assert!(state.outcome_unknown_command.is_none());
	}

	#[test]
	fn bound_running_card_proves_an_unknown_start_committed() {
		let mut state = State::new();
		state.cards.push(card(WorkItemState::Running, 3, None, Some(CONVERSATION_ID)));
		state.command = WorkItemCommandState::OutcomeUnknown;
		state.outcome_unknown_command = Some(envelope(
			CommandPayload::StartWorkItem {
				work_item_id: WorkItemBoardWorkItemId::new(WORK_ITEM_ID).unwrap(),
				project_id: WorkItemBoardProjectId::new(PROJECT_ID).unwrap(),
				conversation_id: EntityId::new(CONVERSATION_ID).unwrap(),
			},
			Some(2),
		));

		assert!(!state.reconcile_outcome_unknown());
		assert_eq!(state.command, WorkItemCommandState::Accepted);
		assert!(state.outcome_unknown_command.is_none());
		assert!(state.pending_command.is_none());
	}

	#[test]
	fn done_card_with_exact_accepted_revision_proves_acceptance_committed() {
		let mut state = State::new();
		state.cards.push(card(WorkItemState::Done, 5, Some(4), Some(CONVERSATION_ID)));
		state.command = WorkItemCommandState::OutcomeUnknown;
		state.outcome_unknown_command = Some(envelope(
			CommandPayload::AcceptWorkItem {
				work_item_id: WorkItemBoardWorkItemId::new(WORK_ITEM_ID).unwrap(),
				project_id: WorkItemBoardProjectId::new(PROJECT_ID).unwrap(),
				acceptance_id: EntityId::new("61000000-0000-4000-8000-000000000001").unwrap(),
				evidence_summary: WireText::new("Reviewed persisted evidence.").unwrap(),
			},
			Some(4),
		));

		assert!(!state.reconcile_outcome_unknown());
		assert_eq!(state.command, WorkItemCommandState::Accepted);
		assert!(state.outcome_unknown_command.is_none());
	}
}
