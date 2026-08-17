//! Presentation-neutral controller for the bounded Adaptive Factory Program cycle.

use std::{
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
	EventPayload, IdempotencyKey, ProgramContinuationDraftDto, ProgramCycleDraftDto,
	ProgramCycleDto, ProgramCycleResult, ProgramListResult, ProgramNodeKind, ProgramReviewDraftDto,
	ProgramSummaryDto, QueryEnvelope, QueryId, QueryPayload, QueryResultEnvelope,
	QueryResultPayload, ReceiptDisposition, ResultPayload, ServerId, WireText,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProgramsSnapshot {
	pub(crate) load: ProgramsLoadState,
	pub(crate) command: ProgramCommandState,
	pub(crate) programs: Vec<ProgramSummaryDto>,
	pub(crate) selected: Option<EntityId>,
	pub(crate) cycle: Option<ProgramCycleDto>,
	pub(crate) can_mutate: bool,
}

impl ProgramsSnapshot {
	pub(crate) fn selected_program(&self) -> Option<&ProgramSummaryDto> {
		let selected = self.selected.as_ref()?;
		self.programs.iter().find(|program| &program.program_id == selected)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProgramsLoadState {
	NeverRequested,
	LoadingPrograms,
	LoadingCycle,
	Ready,
	NoPrograms,
	Offline,
	Unavailable,
	Refused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProgramCommandState {
	Idle,
	Sending,
	AwaitingResult,
	Accepted,
	OutcomeUnknown,
	Refused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProgramInputError {
	Offline,
	Busy,
	InvalidDraft,
	NoSelection,
	IdentityUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProgramRouteOutcome {
	Fresh,
	Unmatched,
	Refused,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProgramDispatch {
	Query(QueryEnvelope),
	Command(CommandEnvelope),
}

impl ProgramDispatch {
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

#[derive(Clone)]
pub(crate) struct Programs {
	inner: Arc<ProgramsInner>,
}

struct ProgramsInner {
	state: Mutex<State>,
	notify: Notify,
}

impl Programs {
	pub(crate) fn production() -> Self {
		Self {
			inner: Arc::new(ProgramsInner {
				state: Mutex::new(State::new()),
				notify: Notify::new(),
			}),
		}
	}

	#[cfg(feature = "visual-capture")]
	pub(crate) fn visual_closed_cycle() -> Self {
		use decodex_protocol::{
			DomainEntityDto, DomainEntityFieldDto, DomainPackCapabilityDto,
			DomainPackCapabilityStatus, DomainPackDescriptorDto, DomainPackProjectionDto,
			DomainPackViewKind, DomainRelationDto, EntityRevision, PAPER_INVESTMENT_DOMAIN_PACK_ID,
			ProgramEdgeDto, ProgramNodeDto, ProgramNodeFieldDto, ProgramRelationKind, ProgramState,
			Sha256Digest, WireText,
		};

		let id = |value: &str| EntityId::new(value).expect("visual Program identity is valid");
		let text = |value: &str| WireText::new(value).expect("visual Program text is bounded");
		let field = |label: &str, value: &str| ProgramNodeFieldDto {
			label: text(label),
			value: text(value),
		};
		let node = |id: EntityId,
		            kind: ProgramNodeKind,
		            title: &str,
		            summary: &str,
		            state: &str,
		            source: Option<&str>,
		            conversation_id: Option<EntityId>,
		            fields: Vec<ProgramNodeFieldDto>,
		            offset: i64| ProgramNodeDto {
			id,
			kind,
			title: text(title),
			summary: text(summary),
			state: text(state),
			source: source.map(text),
			observed_at_micros: Some(1_786_000_000_000_000 + offset),
			conversation_id,
			fields,
		};
		let program_id = id("81000000-0000-4000-8000-000000000001");
		let signal_id = id("81000000-0000-4000-8000-000000000002");
		let claim_id = id("81000000-0000-4000-8000-000000000003");
		let proposal_id = id("81000000-0000-4000-8000-000000000004");
		let objective_id = id("81000000-0000-4000-8000-000000000005");
		let work_item_id = id("81000000-0000-4000-8000-000000000006");
		let conversation_id = id("10000000-0000-4000-8000-000000000001");
		let deterministic_id = id("81000000-0000-4000-8000-000000000007");
		let external_id = id("81000000-0000-4000-8000-000000000008");
		let review_id = id("81000000-0000-4000-8000-000000000009");
		let signal_2_id = id("81000000-0000-4000-8000-000000000010");
		let claim_2_id = id("81000000-0000-4000-8000-000000000011");
		let proposal_2_id = id("81000000-0000-4000-8000-000000000012");
		let objective_2_id = id("81000000-0000-4000-8000-000000000013");
		let work_item_2_id = id("81000000-0000-4000-8000-000000000014");
		let conversation_2_id = id("10000000-0000-4000-8000-000000000002");
		let deterministic_2_id = id("81000000-0000-4000-8000-000000000015");
		let external_2_id = id("81000000-0000-4000-8000-000000000016");
		let review_2_id = id("81000000-0000-4000-8000-000000000017");
		let nodes = vec![
			node(
				signal_id.clone(),
				ProgramNodeKind::Signal,
				"Signal",
				"Direct Codex use loses causal context across several tasks.",
				"observed",
				Some("Decodex dogfood"),
				None,
				Vec::new(),
				0,
			),
			node(
				claim_id.clone(),
				ProgramNodeKind::Claim,
				"Claim",
				"A durable semantic spine can reduce coordination overhead.",
				"current",
				None,
				None,
				Vec::new(),
				1,
			),
			node(
				proposal_id.clone(),
				ProgramNodeKind::Proposal,
				"Proposal",
				"Add one bounded Program cycle above the existing worker path.",
				"non_executable",
				None,
				None,
				vec![
					field("Expected effect", "One explainable closed loop"),
					field("Risk", "Premature workflow generalization"),
				],
				2,
			),
			node(
				objective_id.clone(),
				ProgramNodeKind::Objective,
				"Objective",
				"Persist and reopen the complete causal cycle.",
				"achieved",
				None,
				None,
				vec![field("Validation", "SQLite reopen and protocol readback")],
				3,
			),
			node(
				work_item_id.clone(),
				ProgramNodeKind::WorkItem,
				"Implement Adaptive Factory Spine V1",
				"Build the smallest restart-safe Program cycle.",
				"done",
				None,
				Some(conversation_id.clone()),
				vec![field("Working directory", "/Users/x/code/acg-box/decodex")],
				4,
			),
			node(
				conversation_id.clone(),
				ProgramNodeKind::Run,
				"Codex Quick Task",
				"Execution used the existing app-server worker path.",
				"ready",
				None,
				Some(conversation_id.clone()),
				Vec::new(),
				5,
			),
			node(
				deterministic_id.clone(),
				ProgramNodeKind::Evidence,
				"Deterministic validation",
				"Database, protocol, runtime, and GPUI checks passed.",
				"deterministic_validation",
				Some("Repository gates"),
				None,
				Vec::new(),
				6,
			),
			node(
				external_id.clone(),
				ProgramNodeKind::Evidence,
				"External evidence",
				"The authoritative cycle remained visible after daemon restart.",
				"external",
				Some("Local GPUI dogfood"),
				None,
				Vec::new(),
				7,
			),
			node(
				review_id.clone(),
				ProgramNodeKind::Review,
				"Program Review",
				"The loop is now a reusable coordination capability.",
				"capability_progress",
				None,
				None,
				Vec::new(),
				8,
			),
			node(
				signal_2_id.clone(),
				ProgramNodeKind::Signal,
				"Signal",
				"The first loop was safe, but the Program could not continue in place.",
				"observed",
				Some("First Program Review"),
				None,
				Vec::new(),
				9,
			),
			node(
				claim_2_id.clone(),
				ProgramNodeKind::Claim,
				"Claim",
				"Review-linked continuation can preserve one durable Program identity.",
				"current",
				None,
				None,
				Vec::new(),
				10,
			),
			node(
				proposal_2_id.clone(),
				ProgramNodeKind::Proposal,
				"Proposal",
				"Append one exact next cycle after the terminal Review.",
				"non_executable",
				None,
				None,
				vec![
					field("Expected effect", "Repeatable causal progress in one Program"),
					field("Risk", "Branching or duplicate continuation"),
				],
				11,
			),
			node(
				objective_2_id.clone(),
				ProgramNodeKind::Objective,
				"Objective",
				"Continue, execute, review, and reopen a second cycle.",
				"achieved",
				None,
				None,
				vec![field("Validation", "Exact predecessor, revision, and restart readback")],
				12,
			),
			node(
				work_item_2_id.clone(),
				ProgramNodeKind::WorkItem,
				"Prove Repeatable Program Loop V1",
				"Exercise one additional cycle through the ordinary Quick Task path.",
				"done",
				None,
				Some(conversation_2_id.clone()),
				vec![field("Working directory", "/Users/x/code/acg-box/decodex")],
				13,
			),
			node(
				conversation_2_id.clone(),
				ProgramNodeKind::Run,
				"Codex Quick Task",
				"The second cycle reused the existing app-server worker path.",
				"ready",
				None,
				Some(conversation_2_id.clone()),
				Vec::new(),
				14,
			),
			node(
				deterministic_2_id.clone(),
				ProgramNodeKind::Evidence,
				"Deterministic validation",
				"Continuation, projection, and restart checks passed.",
				"deterministic_validation",
				Some("Repository gates"),
				None,
				Vec::new(),
				15,
			),
			node(
				external_2_id.clone(),
				ProgramNodeKind::Evidence,
				"External evidence",
				"The exact second Codex conversation remained linked after restart.",
				"external",
				Some("Local GPUI dogfood"),
				None,
				Vec::new(),
				16,
			),
			node(
				review_2_id.clone(),
				ProgramNodeKind::Review,
				"Program Review",
				"The Program now preserves repeatable causal progress.",
				"capability_progress",
				None,
				None,
				Vec::new(),
				17,
			),
		];
		let edge = |from: EntityId, to: EntityId, kind| ProgramEdgeDto { from, to, kind };
		let edges = vec![
			edge(program_id.clone(), signal_id.clone(), ProgramRelationKind::Observes),
			edge(signal_id, claim_id.clone(), ProgramRelationKind::Supports),
			edge(claim_id, proposal_id.clone(), ProgramRelationKind::Justifies),
			edge(proposal_id, objective_id.clone(), ProgramRelationKind::Proposes),
			edge(objective_id, work_item_id.clone(), ProgramRelationKind::DecomposesTo),
			edge(work_item_id.clone(), conversation_id, ProgramRelationKind::Executes),
			edge(work_item_id.clone(), deterministic_id.clone(), ProgramRelationKind::Produces),
			edge(work_item_id, external_id.clone(), ProgramRelationKind::Produces),
			edge(deterministic_id, review_id.clone(), ProgramRelationKind::Supports),
			edge(external_id, review_id.clone(), ProgramRelationKind::Supports),
			edge(review_id.clone(), program_id.clone(), ProgramRelationKind::Validates),
			edge(review_id, signal_2_id.clone(), ProgramRelationKind::Continues),
			edge(signal_2_id, claim_2_id.clone(), ProgramRelationKind::Supports),
			edge(claim_2_id, proposal_2_id.clone(), ProgramRelationKind::Justifies),
			edge(proposal_2_id, objective_2_id.clone(), ProgramRelationKind::Proposes),
			edge(objective_2_id, work_item_2_id.clone(), ProgramRelationKind::DecomposesTo),
			edge(work_item_2_id.clone(), conversation_2_id, ProgramRelationKind::Executes),
			edge(work_item_2_id.clone(), deterministic_2_id.clone(), ProgramRelationKind::Produces),
			edge(work_item_2_id, external_2_id.clone(), ProgramRelationKind::Produces),
			edge(deterministic_2_id, review_2_id.clone(), ProgramRelationKind::Supports),
			edge(external_2_id, review_2_id.clone(), ProgramRelationKind::Supports),
			edge(review_2_id, program_id.clone(), ProgramRelationKind::Validates),
		];
		let program = ProgramSummaryDto {
			program_id: program_id.clone(),
			name: text("Adaptive Factory Spine"),
			purpose: text("Make several Codex tasks one explainable feedback system."),
			state: ProgramState::Active,
			revision: EntityRevision(4),
			updated_at_micros: 1_786_000_000_000_017,
		};
		let cycle = ProgramCycleDto::new(
			program.clone(),
			vec![text("Do not add a general workflow engine")],
			text("Review after one settled Codex execution"),
			nodes,
			edges,
		)
		.expect("visual Program cycle is valid");
		let domain_id = |value: &str| id(value);
		let domain_field = |label: &str, value: &str| DomainEntityFieldDto {
			label: text(label),
			value: text(value),
		};
		let two_year = domain_id("91000000-0000-4000-8000-000000000001");
		let ten_year = domain_id("91000000-0000-4000-8000-000000000002");
		let thesis = domain_id("91000000-0000-4000-8000-000000000003");
		let scenario = domain_id("91000000-0000-4000-8000-000000000004");
		let domain_pack = DomainPackProjectionDto::new(
			DomainPackDescriptorDto {
				id: text(PAPER_INVESTMENT_DOMAIN_PACK_ID),
				version: text("1.0.0"),
				digest: Sha256Digest::new(
					"996a5133a30bc968d27a16835bdbdb34736777c9d11ca2a5ed87d221c957e9eb",
				)
				.expect("visual Pack digest"),
				name: text("Paper Investment Research"),
				namespace: text("finance"),
				view: DomainPackViewKind::GraphInspector,
				capabilities: vec![DomainPackCapabilityDto {
					id: text("codex.quick_task"),
					status: DomainPackCapabilityStatus::Granted,
				}],
				entity_types: vec![
					text("finance.asset"),
					text("finance.thesis"),
					text("finance.scenario"),
				],
				relation_types: vec![
					text("finance.compared_with"),
					text("finance.informs"),
					text("finance.tests"),
				],
			},
			vec![
				DomainEntityDto {
					id: two_year.clone(),
					kind: text("finance.asset"),
					title: text("U.S. Treasury 2-Year"),
					summary: text("June 2025 month-end par yield was 3.72%."),
					state: text("observed"),
					source: Some(text("U.S. Treasury frozen June 2025 fixture")),
					fields: vec![domain_field("Last", "3.72%")],
				},
				DomainEntityDto {
					id: ten_year.clone(),
					kind: text("finance.asset"),
					title: text("U.S. Treasury 10-Year"),
					summary: text("June 2025 month-end par yield was 4.24%."),
					state: text("observed"),
					source: Some(text("U.S. Treasury frozen June 2025 fixture")),
					fields: vec![domain_field("Last", "4.24%")],
				},
				DomainEntityDto {
					id: thesis.clone(),
					kind: text("finance.thesis"),
					title: text("Positive 2s10s slope"),
					summary: text("The frozen sample supports a positive 2s10s slope."),
					state: text("supported"),
					source: None,
					fields: vec![domain_field("Last spread", "52 bp")],
				},
				DomainEntityDto {
					id: scenario.clone(),
					kind: text("finance.scenario"),
					title: text("June spread range"),
					summary: text("The spread stayed within 44-56 basis points."),
					state: text("bounded"),
					source: None,
					fields: vec![domain_field("Observations", "20")],
				},
			],
			vec![
				DomainRelationDto {
					from: two_year.clone(),
					to: ten_year,
					kind: text("finance.compared_with"),
				},
				DomainRelationDto {
					from: two_year,
					to: thesis.clone(),
					kind: text("finance.informs"),
				},
				DomainRelationDto { from: thesis, to: scenario, kind: text("finance.tests") },
			],
			&program_id,
		)
		.expect("visual Domain Pack is valid");
		let cycle = cycle.with_domain_pack(domain_pack).expect("visual Pack attaches");
		let programs = Self::production();
		{
			let mut state = programs.lock();
			state.active = true;
			state.session = Some(SessionBinding {
				generation: 1,
				server_id: ServerId::new("visual-programs")
					.expect("visual server identity is bounded"),
			});
			state.load = ProgramsLoadState::Ready;
			state.programs = vec![program];
			state.selected = Some(program_id);
			state.cycle = Some(cycle);
		}
		programs
	}

	pub(crate) fn activate(&self) {
		let mut state = self.lock();
		state.active = true;
		let queued = if state.cycle_dirty && state.selected.is_some() {
			state.queue_selected_cycle()
		} else {
			state.queue_programs()
		};
		if state.session.is_none() {
			state.load = ProgramsLoadState::Offline;
		}
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn deactivate(&self) {
		self.lock().active = false;
	}

	pub(crate) fn snapshot(&self) -> ProgramsSnapshot {
		self.lock().snapshot()
	}

	pub(crate) fn select(&self, program_id: EntityId) -> bool {
		let mut state = self.lock();
		if !state.programs.iter().any(|program| program.program_id == program_id) {
			return false;
		}
		state.selected = Some(program_id);
		state.cycle = None;
		let queued = state.queue_selected_cycle();
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
		true
	}

	/// Remember the exact Conversation that is about to bind the selected Program WorkItem.
	///
	/// The binding commits inside the ordinary Quick Task transaction. The subsequent Quick Task
	/// publication is therefore the first safe point at which to refresh the Program projection.
	pub(crate) fn expect_execution(&self, conversation_id: EntityId) {
		let mut state = self.lock();
		state.pending_execution = Some(conversation_id);
		state.cycle_dirty = true;
	}

	pub(crate) fn refresh_selected(&self) -> Result<(), ProgramInputError> {
		let mut state = self.lock();
		if state.selected.is_none() {
			return Err(ProgramInputError::NoSelection);
		}
		if state.pending_query.is_some() || state.in_flight_query.is_some() {
			return Err(ProgramInputError::Busy);
		}
		let queued = state.queue_selected_cycle();
		drop(state);
		if queued {
			self.inner.notify.notify_one();
			Ok(())
		} else {
			Err(ProgramInputError::Offline)
		}
	}

	pub(crate) fn create(&self, draft: ProgramCycleDraftDto) -> Result<(), ProgramInputError> {
		let program_id = draft.program_id.clone();
		self.queue_command(
			CommandPayload::CreateProgramCycle { draft: Box::new(draft) },
			Some(program_id),
			None,
		)
	}

	pub(crate) fn bind_domain_pack(
		&self,
		program_id: EntityId,
		domain_pack_id: WireText,
		expected_revision: EntityRevision,
	) -> Result<(), ProgramInputError> {
		self.queue_command(
			CommandPayload::BindProgramDomainPack { program_id, domain_pack_id },
			None,
			Some(expected_revision),
		)
	}

	pub(crate) fn continue_program(
		&self,
		continuation: ProgramContinuationDraftDto,
		expected_revision: EntityRevision,
	) -> Result<(), ProgramInputError> {
		self.queue_command(
			CommandPayload::ContinueProgram { continuation: Box::new(continuation) },
			None,
			Some(expected_revision),
		)
	}

	pub(crate) fn record_review(
		&self,
		review: ProgramReviewDraftDto,
	) -> Result<(), ProgramInputError> {
		self.queue_command(
			CommandPayload::RecordProgramReview { review: Box::new(review) },
			None,
			None,
		)
	}

	fn queue_command(
		&self,
		payload: CommandPayload,
		selected: Option<EntityId>,
		expected_revision: Option<EntityRevision>,
	) -> Result<(), ProgramInputError> {
		let mut state = self.lock();
		if state.session.is_none() {
			return Err(ProgramInputError::Offline);
		}
		if state.pending_command.is_some()
			|| state.in_flight_command.is_some()
			|| state.outcome_unknown_command.is_some()
		{
			return Err(ProgramInputError::Busy);
		}
		let identity = command_identity()?;
		if let Some(program_id) = selected {
			state.selected = Some(program_id);
			state.cycle = None;
		}
		state.pending_command = Some(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: identity.client_command_id,
			idempotency_key: identity.idempotency_key,
			expected_revision,
			correlation_id: identity.correlation_id,
			causation_id: None::<CausationId>,
			payload,
		});
		state.command = ProgramCommandState::Sending;
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
		let query_queued = state.queue_programs();
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
		state.load = ProgramsLoadState::Offline;
	}

	pub(crate) async fn next_dispatch(
		&self,
		generation: u64,
		server_id: &ServerId,
	) -> ProgramDispatch {
		loop {
			let notified = self.inner.notify.notified();
			if let Some(dispatch) = self.try_take_dispatch(generation, server_id) {
				return dispatch;
			}
			notified.await;
		}
	}

	fn try_take_dispatch(&self, generation: u64, server_id: &ServerId) -> Option<ProgramDispatch> {
		let mut state = self.lock();
		let binding = SessionBinding { generation, server_id: server_id.clone() };
		if state.session.as_ref() != Some(&binding) {
			return None;
		}
		if state.in_flight_command.is_none()
			&& let Some(envelope) = state.pending_command.take()
		{
			state.in_flight_command = Some(InFlightCommand { envelope: envelope.clone(), binding });
			return Some(ProgramDispatch::Command(envelope));
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
			return Some(ProgramDispatch::Query(envelope));
		}
		None
	}

	pub(crate) fn command_send_failed(&self, dispatch: &ProgramDispatch) {
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

	pub(crate) fn command_sent(&self, dispatch: &ProgramDispatch) {
		let Some(command) = dispatch.command() else {
			return;
		};
		let mut state = self.lock();
		if state.in_flight_command.as_ref().is_some_and(|in_flight| {
			in_flight.envelope.client_command_id == command.client_command_id
		}) {
			state.command = ProgramCommandState::AwaitingResult;
		}
	}

	pub(crate) fn apply_event(&self, event: &EventEnvelope) {
		let mut state = self.lock();
		let mut query_queued = false;
		match &event.payload {
			EventPayload::ProgramCycleChanged { cycle } => state.apply_cycle((**cycle).clone()),
			EventPayload::QuickTaskConversationChanged { conversation }
			| EventPayload::QuickTaskTurnFinished { conversation, .. }
				if state.pending_execution.as_ref() == Some(&conversation.conversation_id)
					|| state.cycle.as_ref().is_some_and(|cycle| {
						cycle.nodes.iter().any(|node| {
							node.conversation_id.as_ref() == Some(&conversation.conversation_id)
						})
					}) =>
			{
				state.cycle_dirty = true;
				query_queued = state.queue_selected_cycle();
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
	) -> ProgramRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_query.as_ref() else {
			return ProgramRouteOutcome::Unmatched;
		};
		if in_flight.query_id != result.query_id {
			return ProgramRouteOutcome::Unmatched;
		}
		let purpose = in_flight.purpose.clone();
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| result.version != CURRENT_VERSION
			|| result.server_id != *server_id
		{
			state.in_flight_query = None;
			state.load = ProgramsLoadState::Refused;
			return ProgramRouteOutcome::Refused;
		}
		state.in_flight_query = None;
		let (outcome, mut query_queued) = state.route_query_payload(purpose, &result.payload);
		if state.cycle_dirty && !query_queued {
			query_queued = state.queue_selected_cycle();
		}
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
	) -> ProgramRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_command.as_ref() else {
			return ProgramRouteOutcome::Unmatched;
		};
		if in_flight.envelope.client_command_id != receipt.client_command_id {
			return ProgramRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| receipt.version != CURRENT_VERSION
			|| receipt.server_id != *server_id
			|| receipt.idempotency_key != in_flight.envelope.idempotency_key
		{
			state.latch_in_flight_outcome_unknown();
			return ProgramRouteOutcome::Refused;
		}
		match receipt.disposition {
			ReceiptDisposition::Executed | ReceiptDisposition::Duplicate => {
				state.command = ProgramCommandState::AwaitingResult;
			},
			ReceiptDisposition::Refused => {
				state.in_flight_command = None;
				state.outcome_unknown_command = None;
				state.command = ProgramCommandState::Refused;
			},
		}
		ProgramRouteOutcome::Fresh
	}

	pub(crate) fn route_command_result(
		&self,
		generation: u64,
		server_id: &ServerId,
		result: &CommandResultEnvelope,
	) -> ProgramRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_command.as_ref() else {
			return ProgramRouteOutcome::Unmatched;
		};
		if in_flight.envelope.client_command_id != result.client_command_id {
			return ProgramRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| result.version != CURRENT_VERSION
			|| result.server_id != *server_id
			|| result.idempotency_key != in_flight.envelope.idempotency_key
		{
			state.latch_in_flight_outcome_unknown();
			return ProgramRouteOutcome::Refused;
		}
		let in_flight = state.in_flight_command.take().expect("matching Program command exists");
		match result.outcome {
			CommandOutcome::Succeeded => {
				let Some(ResultPayload::ProgramCycleChanged { cycle }) = result.payload.as_ref()
				else {
					state.command = ProgramCommandState::Refused;
					return ProgramRouteOutcome::Refused;
				};
				if !command_matches_cycle(&in_flight.envelope, cycle) {
					state.command = ProgramCommandState::Refused;
					return ProgramRouteOutcome::Refused;
				}
				state.apply_cycle((**cycle).clone());
				state.outcome_unknown_command = None;
				state.command = ProgramCommandState::Accepted;
				ProgramRouteOutcome::Fresh
			},
			CommandOutcome::AcceptanceUnknown => {
				state.outcome_unknown_command = Some(in_flight.envelope);
				state.command = ProgramCommandState::OutcomeUnknown;
				let queued = state.queue_selected_cycle();
				drop(state);
				if queued {
					self.inner.notify.notify_one();
				}
				ProgramRouteOutcome::Fresh
			},
			CommandOutcome::Rejected => {
				state.outcome_unknown_command = None;
				state.command = ProgramCommandState::Refused;
				ProgramRouteOutcome::Fresh
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
	load: ProgramsLoadState,
	command: ProgramCommandState,
	programs: Vec<ProgramSummaryDto>,
	selected: Option<EntityId>,
	cycle: Option<ProgramCycleDto>,
	pending_execution: Option<EntityId>,
	cycle_dirty: bool,
	pending_query: Option<PendingQuery>,
	in_flight_query: Option<InFlightQuery>,
	pending_command: Option<CommandEnvelope>,
	in_flight_command: Option<InFlightCommand>,
	outcome_unknown_command: Option<CommandEnvelope>,
	next_query_sequence: u64,
}

impl State {
	fn new() -> Self {
		Self {
			session: None,
			active: false,
			load: ProgramsLoadState::NeverRequested,
			command: ProgramCommandState::Idle,
			programs: Vec::new(),
			selected: None,
			cycle: None,
			pending_execution: None,
			cycle_dirty: false,
			pending_query: None,
			in_flight_query: None,
			pending_command: None,
			in_flight_command: None,
			outcome_unknown_command: None,
			next_query_sequence: 0,
		}
	}

	fn snapshot(&self) -> ProgramsSnapshot {
		ProgramsSnapshot {
			load: self.load,
			command: self.command,
			programs: self.programs.clone(),
			selected: self.selected.clone(),
			cycle: self.cycle.clone(),
			can_mutate: self.session.is_some()
				&& self.pending_command.is_none()
				&& self.in_flight_command.is_none()
				&& self.outcome_unknown_command.is_none(),
		}
	}

	fn reset_queries(&mut self) {
		self.pending_query = None;
		self.in_flight_query = None;
	}

	fn queue_programs(&mut self) -> bool {
		let queued = self.queue_query(QueryPayload::ListPrograms, QueryPurpose::Programs);
		if queued {
			self.load = ProgramsLoadState::LoadingPrograms;
		}
		queued
	}

	fn queue_selected_cycle(&mut self) -> bool {
		let Some(program_id) = self.selected.clone() else {
			return false;
		};
		let queued = self.queue_query(
			QueryPayload::GetProgramCycle { program_id: program_id.clone() },
			QueryPurpose::Cycle(program_id),
		);
		if queued {
			self.load = ProgramsLoadState::LoadingCycle;
			self.cycle_dirty = false;
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
			self.load = ProgramsLoadState::Refused;
			return false;
		};
		self.next_query_sequence = sequence;
		self.pending_query = Some(PendingQuery {
			envelope: QueryEnvelope {
				version: CURRENT_VERSION,
				query_id: QueryId::new(format!("gpui-programs/{generation}/{sequence}"))
					.expect("bounded Program query identity"),
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
	) -> (ProgramRouteOutcome, bool) {
		match (purpose, payload) {
			(
				QueryPurpose::Programs,
				QueryResultPayload::Programs(ProgramListResult::Available(list)),
			) => {
				self.programs = list.clone();
				let selected_exists = self.selected.as_ref().is_some_and(|selected| {
					self.programs.iter().any(|program| &program.program_id == selected)
				});
				if !selected_exists {
					self.selected = self.programs.first().map(|program| program.program_id.clone());
					self.cycle = None;
				}
				if self.selected.is_none() {
					self.load = ProgramsLoadState::NoPrograms;
					(ProgramRouteOutcome::Fresh, false)
				} else {
					let queued = self.queue_selected_cycle();
					(ProgramRouteOutcome::Fresh, queued)
				}
			},
			(
				QueryPurpose::Programs,
				QueryResultPayload::Programs(ProgramListResult::Unavailable),
			) => {
				self.load = ProgramsLoadState::Unavailable;
				(ProgramRouteOutcome::Fresh, false)
			},
			(
				QueryPurpose::Cycle(program_id),
				QueryResultPayload::ProgramCycle(ProgramCycleResult::Available(cycle)),
			) if cycle.program.program_id == program_id
				&& self.selected.as_ref() == Some(&program_id) =>
			{
				self.apply_cycle((**cycle).clone());
				self.reconcile_outcome_unknown();
				(ProgramRouteOutcome::Fresh, false)
			},
			(
				QueryPurpose::Cycle(_),
				QueryResultPayload::ProgramCycle(ProgramCycleResult::NotFound),
			) => {
				self.cycle = None;
				self.load = ProgramsLoadState::NoPrograms;
				(ProgramRouteOutcome::Fresh, false)
			},
			(
				QueryPurpose::Cycle(_),
				QueryResultPayload::ProgramCycle(ProgramCycleResult::Unavailable),
			) => {
				self.load = ProgramsLoadState::Unavailable;
				(ProgramRouteOutcome::Fresh, false)
			},
			_ => {
				self.load = ProgramsLoadState::Refused;
				(ProgramRouteOutcome::Refused, false)
			},
		}
	}

	fn apply_cycle(&mut self, cycle: ProgramCycleDto) {
		if self.pending_execution.as_ref().is_some_and(|conversation_id| {
			cycle.nodes.iter().any(|node| node.conversation_id.as_ref() == Some(conversation_id))
		}) {
			self.pending_execution = None;
		}
		let summary = cycle.program.clone();
		self.upsert_summary(summary.clone());
		self.selected = Some(summary.program_id);
		self.cycle = Some(cycle);
		self.load = ProgramsLoadState::Ready;
	}

	fn upsert_summary(&mut self, summary: ProgramSummaryDto) {
		if let Some(existing) =
			self.programs.iter_mut().find(|program| program.program_id == summary.program_id)
		{
			*existing = summary;
		} else {
			self.programs.push(summary);
		}
		self.programs.sort_by_key(|program| std::cmp::Reverse(program.updated_at_micros));
	}

	fn latch_in_flight_outcome_unknown(&mut self) {
		if let Some(in_flight) = self.in_flight_command.take() {
			self.pending_command = None;
			self.outcome_unknown_command = Some(in_flight.envelope);
			self.command = ProgramCommandState::OutcomeUnknown;
		}
	}

	fn reconcile_outcome_unknown(&mut self) {
		let Some(command) = self.outcome_unknown_command.as_ref() else {
			return;
		};
		let Some(cycle) = self.cycle.as_ref() else {
			return;
		};
		if command_matches_cycle(command, cycle) {
			self.outcome_unknown_command = None;
			self.command = ProgramCommandState::Accepted;
		}
	}
}

#[derive(Clone)]
enum QueryPurpose {
	Programs,
	Cycle(EntityId),
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

fn command_matches_cycle(command: &CommandEnvelope, cycle: &ProgramCycleDto) -> bool {
	match &command.payload {
		CommandPayload::CreateProgramCycle { draft } =>
			cycle.program.program_id == draft.program_id,
		CommandPayload::BindProgramDomainPack { program_id, domain_pack_id } =>
			cycle.program.program_id == *program_id
				&& cycle
					.domain_pack
					.as_ref()
					.is_some_and(|projection| projection.descriptor.id == *domain_pack_id),
		CommandPayload::ContinueProgram { continuation } =>
			cycle.program.program_id == continuation.program_id
				&& cycle.nodes.iter().any(|node| {
					node.kind == ProgramNodeKind::Signal && node.id == continuation.signal_id
				}),
		CommandPayload::RecordProgramReview { review } =>
			cycle.program.program_id == review.program_id
				&& cycle
					.nodes
					.iter()
					.any(|node| node.kind == ProgramNodeKind::Review && node.id == review.review_id),
		_ => false,
	}
}

struct CommandIdentity {
	client_command_id: ClientCommandId,
	idempotency_key: IdempotencyKey,
	correlation_id: CorrelationId,
}

fn command_identity() -> Result<CommandIdentity, ProgramInputError> {
	let value = canonical_uuid_v4()?;
	Ok(CommandIdentity {
		client_command_id: ClientCommandId::new(format!("gpui/{value}"))
			.expect("canonical command identity is bounded"),
		idempotency_key: IdempotencyKey::new(format!("program/{value}"))
			.expect("canonical idempotency key is bounded"),
		correlation_id: CorrelationId::new(value)
			.expect("canonical correlation identity is bounded"),
	})
}

pub(crate) fn entity_id() -> Result<EntityId, ProgramInputError> {
	EntityId::new(canonical_uuid_v4()?).map_err(|_| ProgramInputError::IdentityUnavailable)
}

fn canonical_uuid_v4() -> Result<String, ProgramInputError> {
	let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_err(|_| ProgramInputError::IdentityUnavailable)?
		.as_nanos();
	let mut digest = Sha256::new();
	digest.update(std::process::id().to_be_bytes());
	digest.update(nanos.to_be_bytes());
	digest.update(sequence.to_be_bytes());
	let mut bytes: [u8; 16] =
		digest.finalize()[..16].try_into().expect("SHA-256 contains sixteen identity bytes");
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
	use decodex_protocol::{
		Channel, Cursor, EntityRevision, PAPER_INVESTMENT_DOMAIN_PACK_ID, ProgramState,
		QuickTaskState, QuickTaskSummary, QuickTaskWorkingDirectory, WireText,
	};

	use super::*;

	#[test]
	fn generated_program_identity_is_canonical_uuid_v4() {
		let id = entity_id().expect("identity is available");
		assert_eq!(id.as_str().len(), 36);
		assert_eq!(&id.as_str()[14..15], "4");
		assert!(matches!(&id.as_str()[19..20], "8" | "9" | "a" | "b"));
	}

	#[cfg(feature = "visual-capture")]
	#[test]
	fn visual_program_preserves_two_review_linked_cycles() {
		let snapshot = Programs::visual_closed_cycle().snapshot();
		let cycle = snapshot.cycle.expect("visual Program cycle");

		assert_eq!(
			cycle.nodes.iter().filter(|node| node.kind == ProgramNodeKind::Signal).count(),
			2
		);
		assert_eq!(
			cycle.nodes.iter().filter(|node| node.kind == ProgramNodeKind::Review).count(),
			2
		);
		assert!(cycle.edges.iter().any(|edge| {
			edge.kind == decodex_protocol::ProgramRelationKind::Continues
				&& cycle
					.nodes
					.iter()
					.any(|node| node.id == edge.from && node.kind == ProgramNodeKind::Review)
				&& cycle
					.nodes
					.iter()
					.any(|node| node.id == edge.to && node.kind == ProgramNodeKind::Signal)
		}));
		assert_eq!(cycle.program.revision, EntityRevision(4));
		let pack = cycle.domain_pack.expect("visual Domain Pack");
		assert_eq!(pack.descriptor.id.as_str(), PAPER_INVESTMENT_DOMAIN_PACK_ID);
		assert_eq!(pack.entities.len(), 4);
		assert!(
			pack.relations
				.iter()
				.any(|relation| { relation.kind.as_str() == "finance.compared_with" })
		);
	}

	#[test]
	fn legacy_pack_binding_dispatch_is_revision_fenced_and_exact() {
		let programs = Programs::production();
		let server_id = ServerId::new("program-pack-binding-test").expect("server identity");
		programs.bind_session(9, server_id.clone());
		let program_id =
			EntityId::new("81000000-0000-4000-8000-000000000001").expect("Program identity");
		programs
			.bind_domain_pack(
				program_id.clone(),
				WireText::new(PAPER_INVESTMENT_DOMAIN_PACK_ID).expect("Pack identity"),
				EntityRevision(7),
			)
			.expect("binding queues");
		let dispatch = programs.try_take_dispatch(9, &server_id).expect("binding dispatch");
		let command = dispatch.command().expect("binding command");
		assert_eq!(command.expected_revision, Some(EntityRevision(7)));
		assert!(matches!(
			&command.payload,
			CommandPayload::BindProgramDomainPack { program_id: queued, domain_pack_id }
				if queued == &program_id
					&& domain_pack_id.as_str() == PAPER_INVESTMENT_DOMAIN_PACK_ID
		));
	}

	#[test]
	fn updated_program_returns_to_the_front_of_the_recent_selector() {
		let summary = |id: &str, updated_at_micros| ProgramSummaryDto {
			program_id: EntityId::new(id).expect("canonical Program identity"),
			name: WireText::new("Program").expect("bounded name"),
			purpose: WireText::new("Exercise selector ordering").expect("bounded purpose"),
			state: ProgramState::Active,
			revision: EntityRevision(1),
			updated_at_micros,
		};
		let older = "81000000-0000-4000-8000-000000000001";
		let newer = "81000000-0000-4000-8000-000000000002";
		let mut state = State::new();

		state.upsert_summary(summary(older, 1));
		state.upsert_summary(summary(newer, 2));
		assert_eq!(state.programs[0].program_id.as_str(), newer);

		state.upsert_summary(summary(older, 3));
		assert_eq!(state.programs[0].program_id.as_str(), older);
	}

	#[test]
	fn continuation_dispatch_carries_the_exact_program_revision() {
		let programs = Programs::production();
		let server_id = ServerId::new("program-continuation-test").expect("server identity");
		programs.bind_session(7, server_id.clone());
		let id = |value: &str| EntityId::new(value).expect("canonical Program identity");
		let text = |value: &str| WireText::new(value).expect("bounded Program text");
		let continuation = ProgramContinuationDraftDto {
			program_id: id("81000000-0000-4000-8000-000000000001"),
			predecessor_review_id: id("81000000-0000-4000-8000-000000000002"),
			signal_id: id("81000000-0000-4000-8000-000000000003"),
			claim_id: id("81000000-0000-4000-8000-000000000004"),
			proposal_id: id("81000000-0000-4000-8000-000000000005"),
			objective_id: id("81000000-0000-4000-8000-000000000006"),
			work_item_id: id("81000000-0000-4000-8000-000000000007"),
			signal_source: text("Operator review"),
			signal_summary: text("The first cycle exposed the next finite gap."),
			signal_observed_at_micros: 1,
			claim_statement: text("One next cycle can close that gap."),
			proposal_summary: text("Append one bounded continuation."),
			proposal_expected_effect: text("The Program advances without losing history."),
			proposal_risk: text("The next WorkItem might not settle."),
			proposal_evidence_need: text("A settled run and two evidence classes."),
			objective_outcome: text("The second cycle is complete."),
			acceptance_criteria: vec![text("The second cycle is visible.")],
			validation_criteria: vec![text("Restart readback has no duplicate.")],
			work_item_title: text("Exercise the second Program cycle"),
			work_item_instructions: text("Complete one finite local verification task."),
			working_directory: QuickTaskWorkingDirectory::new("/tmp/decodex")
				.expect("working directory"),
		};

		programs
			.continue_program(continuation.clone(), EntityRevision(2))
			.expect("continuation is queued");
		let dispatch =
			programs.try_take_dispatch(7, &server_id).expect("continuation dispatch is ready");
		let command = dispatch.command().expect("continuation is a command");

		assert_eq!(command.expected_revision, Some(EntityRevision(2)));
		assert!(matches!(
			&command.payload,
			CommandPayload::ContinueProgram { continuation: queued }
				if queued.as_ref() == &continuation
		));
	}

	#[test]
	fn pending_execution_publication_refreshes_after_factory_reactivation() {
		let programs = Programs::production();
		let program_id =
			EntityId::new("81000000-0000-4000-8000-000000000001").expect("Program identity");
		let conversation_id =
			EntityId::new("81000000-0000-4000-8000-000000000002").expect("Conversation identity");
		let server_id = ServerId::new("program-refresh-test").expect("server identity");
		{
			let mut state = programs.lock();
			state.session = Some(SessionBinding { generation: 1, server_id: server_id.clone() });
			state.selected = Some(program_id.clone());
			state.pending_execution = Some(conversation_id.clone());
		}
		let conversation = QuickTaskSummary::new(
			conversation_id.clone(),
			EntityRevision(1),
			1,
			Some(
				EntityId::new("81000000-0000-4000-8000-000000000003")
					.expect("RuntimeSession identity"),
			),
			Some(EntityRevision(1)),
			QuickTaskState::Ready,
			None,
			None,
		)
		.expect("Quick Task projection");
		programs.apply_event(&EventEnvelope {
			version: CURRENT_VERSION,
			server_id,
			cursor: Cursor(1),
			channel: Channel::ConversationStream,
			entity_id: conversation_id,
			entity_revision: EntityRevision(1),
			correlation_id: CorrelationId::new("program-refresh-test")
				.expect("correlation identity"),
			causation_id: None,
			payload: EventPayload::QuickTaskConversationChanged { conversation },
		});
		{
			let state = programs.lock();
			assert!(state.cycle_dirty);
			assert!(state.pending_query.is_none());
		}

		programs.activate();
		let state = programs.lock();
		assert!(!state.cycle_dirty);
		assert!(matches!(
			state.pending_query.as_ref().map(|query| &query.envelope.payload),
			Some(QueryPayload::GetProgramCycle { program_id: queued }) if queued == &program_id
		));
	}
}
