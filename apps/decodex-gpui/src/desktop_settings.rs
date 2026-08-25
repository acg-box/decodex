//! Presentation-neutral GPUI controller for daemon-owned desktop settings.

use std::{
	sync::{
		Arc, Mutex, MutexGuard,
		atomic::{AtomicU64, Ordering},
	},
	time::{SystemTime, UNIX_EPOCH},
};

use sha2::{Digest as _, Sha256};
use tokio::sync::Notify;

use decodex_protocol::{
	CURRENT_VERSION, CausationId, ClientCommandId, CommandEnvelope, CommandOutcome, CommandPayload,
	CommandReceipt, CommandResultEnvelope, CorrelationId, DESKTOP_SETTINGS_ENTITY_ID,
	DesktopSettingsDto, DesktopSettingsResult, EntityRevision, EventEnvelope, EventPayload,
	IdempotencyKey, QueryEnvelope, QueryId, QueryPayload, QueryResultEnvelope, QueryResultPayload,
	ReceiptDisposition, ResultPayload, ServerId,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Complete bounded state rendered by the Settings destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DesktopSettingsSnapshot {
	pub(crate) load: DesktopSettingsLoadState,
	pub(crate) command: DesktopSettingsCommandState,
	pub(crate) settings: Option<DesktopSettingsDto>,
	pub(crate) can_toggle: bool,
}

/// Finite persistent-settings readback state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopSettingsLoadState {
	NeverRequested,
	Loading,
	Ready,
	Offline,
	Unavailable,
	Refused,
}

/// Finite state for the one persistent-settings command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopSettingsCommandState {
	Idle,
	Sending,
	AwaitingResult,
	Accepted,
	OutcomeUnknown,
	Refused,
}

/// Closed local refusal before a settings command enters the retained session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopSettingsInputError {
	Offline,
	Busy,
	NotLoaded,
	IdentityUnavailable,
}

/// Result disposition used to preserve every other retained-session owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DesktopSettingsRouteOutcome {
	Fresh,
	Unmatched,
	Refused,
}

/// Exactly one settings query or command reserved for one retained-session generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DesktopSettingsDispatch {
	Query(QueryEnvelope),
	Command(CommandEnvelope),
}

impl DesktopSettingsDispatch {
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

/// Cloneable settings controller. It owns no transport, platform UI, or product state.
#[derive(Clone)]
pub(crate) struct DesktopSettingsController {
	inner: Arc<DesktopSettingsInner>,
}

struct DesktopSettingsInner {
	state: Mutex<State>,
	notify: Notify,
}

impl DesktopSettingsController {
	pub(crate) fn production() -> Self {
		Self {
			inner: Arc::new(DesktopSettingsInner {
				state: Mutex::new(State::new()),
				notify: Notify::new(),
			}),
		}
	}

	pub(crate) fn snapshot(&self) -> DesktopSettingsSnapshot {
		self.lock().snapshot()
	}

	pub(crate) fn set_show_in_menu_bar(
		&self,
		show_in_menu_bar: bool,
	) -> Result<(), DesktopSettingsInputError> {
		let mut state = self.lock();
		let settings = state.settings.ok_or(DesktopSettingsInputError::NotLoaded)?;
		if settings.show_in_menu_bar == show_in_menu_bar {
			return Ok(());
		}
		state.queue_command(show_in_menu_bar, settings.revision)?;
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
		state.pending_query = None;
		state.in_flight_query = None;
		state.session = Some(binding);
		let query_queued = state.queue_query();
		let command_queued = state.pending_command.is_some();
		drop(state);
		if query_queued || command_queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn session_ended(&self, generation: u64) {
		let mut state = self.lock();
		if !state.session.as_ref().is_some_and(|binding| binding.generation == generation) {
			return;
		}
		state.latch_in_flight_outcome_unknown();
		if state.pending_command.take().is_some()
			&& state.command != DesktopSettingsCommandState::OutcomeUnknown
		{
			state.command = DesktopSettingsCommandState::Refused;
		}
		state.pending_query = None;
		state.in_flight_query = None;
		state.session = None;
		state.load = DesktopSettingsLoadState::Offline;
	}

	pub(crate) async fn next_dispatch(
		&self,
		generation: u64,
		server_id: &ServerId,
	) -> DesktopSettingsDispatch {
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
	) -> Option<DesktopSettingsDispatch> {
		let mut state = self.lock();
		let binding = SessionBinding { generation, server_id: server_id.clone() };
		if state.session.as_ref() != Some(&binding) {
			return None;
		}
		if state.in_flight_command.is_none()
			&& let Some(envelope) = state.pending_command.take()
		{
			state.in_flight_command = Some(InFlightCommand { envelope: envelope.clone(), binding });
			return Some(DesktopSettingsDispatch::Command(envelope));
		}
		if state.in_flight_query.is_none()
			&& let Some(envelope) = state.pending_query.take()
		{
			state.in_flight_query =
				Some(InFlightQuery { query_id: envelope.query_id.clone(), binding });
			return Some(DesktopSettingsDispatch::Query(envelope));
		}
		None
	}

	pub(crate) fn command_send_failed(&self, dispatch: &DesktopSettingsDispatch) {
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

	pub(crate) fn command_sent(&self, dispatch: &DesktopSettingsDispatch) {
		let Some(command) = dispatch.command() else {
			return;
		};
		let mut state = self.lock();
		if state.in_flight_command.as_ref().is_some_and(|in_flight| {
			in_flight.envelope.client_command_id == command.client_command_id
		}) {
			state.command = DesktopSettingsCommandState::AwaitingResult;
		}
	}

	pub(crate) fn apply_event(&self, event: &EventEnvelope) {
		let EventPayload::DesktopSettingsChanged { settings } = &event.payload else {
			return;
		};
		if event.entity_id.as_str() != DESKTOP_SETTINGS_ENTITY_ID
			|| settings.revision != event.entity_revision
			|| !settings.is_valid()
		{
			return;
		}
		let mut state = self.lock();
		if state.settings.is_none_or(|current| settings.revision >= current.revision) {
			state.settings = Some(*settings);
			state.load = DesktopSettingsLoadState::Ready;
		}
	}

	pub(crate) fn route_query_result(
		&self,
		generation: u64,
		server_id: &ServerId,
		result: &QueryResultEnvelope,
	) -> DesktopSettingsRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_query.as_ref() else {
			return DesktopSettingsRouteOutcome::Unmatched;
		};
		if in_flight.query_id != result.query_id {
			return DesktopSettingsRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| result.version != CURRENT_VERSION
			|| result.server_id != *server_id
		{
			state.in_flight_query = None;
			state.load = DesktopSettingsLoadState::Refused;
			return DesktopSettingsRouteOutcome::Refused;
		}
		state.in_flight_query = None;
		match &result.payload {
			QueryResultPayload::DesktopSettings(DesktopSettingsResult::Available(settings))
				if settings.is_valid() =>
			{
				state.settings = Some(*settings);
				state.load = DesktopSettingsLoadState::Ready;
				if state.in_flight_command.is_none() && state.pending_command.is_none() {
					state.command = DesktopSettingsCommandState::Idle;
				}
				DesktopSettingsRouteOutcome::Fresh
			},
			QueryResultPayload::DesktopSettings(DesktopSettingsResult::Unavailable) => {
				state.load = DesktopSettingsLoadState::Unavailable;
				DesktopSettingsRouteOutcome::Fresh
			},
			_ => {
				state.load = DesktopSettingsLoadState::Refused;
				DesktopSettingsRouteOutcome::Refused
			},
		}
	}

	pub(crate) fn route_receipt(
		&self,
		generation: u64,
		server_id: &ServerId,
		receipt: &CommandReceipt,
	) -> DesktopSettingsRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_command.as_ref() else {
			return DesktopSettingsRouteOutcome::Unmatched;
		};
		if in_flight.envelope.client_command_id != receipt.client_command_id {
			return DesktopSettingsRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| receipt.version != CURRENT_VERSION
			|| receipt.server_id != *server_id
			|| receipt.idempotency_key != in_flight.envelope.idempotency_key
		{
			state.latch_in_flight_outcome_unknown();
			return DesktopSettingsRouteOutcome::Refused;
		}
		match receipt.disposition {
			ReceiptDisposition::Executed | ReceiptDisposition::Duplicate => {
				state.command = DesktopSettingsCommandState::AwaitingResult;
			},
			ReceiptDisposition::Refused => {
				state.in_flight_command = None;
				state.command = DesktopSettingsCommandState::Refused;
			},
		}
		DesktopSettingsRouteOutcome::Fresh
	}

	pub(crate) fn route_command_result(
		&self,
		generation: u64,
		server_id: &ServerId,
		result: &CommandResultEnvelope,
	) -> DesktopSettingsRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_command.as_ref() else {
			return DesktopSettingsRouteOutcome::Unmatched;
		};
		if in_flight.envelope.client_command_id != result.client_command_id {
			return DesktopSettingsRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| result.version != CURRENT_VERSION
			|| result.server_id != *server_id
			|| result.idempotency_key != in_flight.envelope.idempotency_key
		{
			state.latch_in_flight_outcome_unknown();
			return DesktopSettingsRouteOutcome::Refused;
		}
		let in_flight = state
			.in_flight_command
			.take()
			.expect("matching desktop settings command remains in flight");
		match result.outcome {
			CommandOutcome::Succeeded => {
				if let (
					CommandPayload::SetDesktopSettings { show_in_menu_bar },
					Some(ResultPayload::DesktopSettingsChanged { settings }),
				) = (&in_flight.envelope.payload, result.payload.as_ref())
					&& settings.show_in_menu_bar == *show_in_menu_bar
					&& settings.is_valid()
					&& result.entity_revision == Some(settings.revision)
				{
					state.settings = Some(*settings);
					state.load = DesktopSettingsLoadState::Ready;
					state.command = DesktopSettingsCommandState::Accepted;
					DesktopSettingsRouteOutcome::Fresh
				} else {
					state.command = DesktopSettingsCommandState::Refused;
					DesktopSettingsRouteOutcome::Refused
				}
			},
			CommandOutcome::AcceptanceUnknown => {
				state.command = DesktopSettingsCommandState::OutcomeUnknown;
				let _ = state.queue_query();
				DesktopSettingsRouteOutcome::Fresh
			},
			CommandOutcome::Rejected => {
				state.command = DesktopSettingsCommandState::Refused;
				let _ = state.queue_query();
				DesktopSettingsRouteOutcome::Fresh
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

struct InFlightQuery {
	query_id: QueryId,
	binding: SessionBinding,
}

struct InFlightCommand {
	envelope: CommandEnvelope,
	binding: SessionBinding,
}

struct State {
	session: Option<SessionBinding>,
	next_query_sequence: u64,
	pending_query: Option<QueryEnvelope>,
	in_flight_query: Option<InFlightQuery>,
	pending_command: Option<CommandEnvelope>,
	in_flight_command: Option<InFlightCommand>,
	load: DesktopSettingsLoadState,
	command: DesktopSettingsCommandState,
	settings: Option<DesktopSettingsDto>,
}

impl State {
	const fn new() -> Self {
		Self {
			session: None,
			next_query_sequence: 0,
			pending_query: None,
			in_flight_query: None,
			pending_command: None,
			in_flight_command: None,
			load: DesktopSettingsLoadState::NeverRequested,
			command: DesktopSettingsCommandState::Idle,
			settings: None,
		}
	}

	fn snapshot(&self) -> DesktopSettingsSnapshot {
		let can_toggle = self.session.is_some()
			&& self.load == DesktopSettingsLoadState::Ready
			&& self.settings.is_some()
			&& self.pending_query.is_none()
			&& self.in_flight_query.is_none()
			&& self.pending_command.is_none()
			&& self.in_flight_command.is_none()
			&& !matches!(
				self.command,
				DesktopSettingsCommandState::Sending
					| DesktopSettingsCommandState::AwaitingResult
					| DesktopSettingsCommandState::OutcomeUnknown
			);
		DesktopSettingsSnapshot {
			load: self.load,
			command: self.command,
			settings: self.settings,
			can_toggle,
		}
	}

	fn queue_query(&mut self) -> bool {
		let Some(generation) = self.session.as_ref().map(|session| session.generation) else {
			self.load = DesktopSettingsLoadState::Offline;
			return false;
		};
		if self.pending_query.is_some() || self.in_flight_query.is_some() {
			return false;
		}
		let Some(sequence) = self.next_query_sequence.checked_add(1) else {
			self.load = DesktopSettingsLoadState::Refused;
			return false;
		};
		self.next_query_sequence = sequence;
		self.pending_query = Some(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: QueryId::new(format!("gpui-desktop-settings/{generation}/{sequence}"))
				.expect("bounded numeric desktop settings query identity"),
			payload: QueryPayload::GetDesktopSettings,
		});
		self.load = DesktopSettingsLoadState::Loading;
		true
	}

	fn queue_command(
		&mut self,
		show_in_menu_bar: bool,
		expected_revision: EntityRevision,
	) -> Result<(), DesktopSettingsInputError> {
		if self.session.is_none() {
			return Err(DesktopSettingsInputError::Offline);
		}
		if self.pending_query.is_some()
			|| self.in_flight_query.is_some()
			|| self.pending_command.is_some()
			|| self.in_flight_command.is_some()
			|| matches!(
				self.command,
				DesktopSettingsCommandState::Sending
					| DesktopSettingsCommandState::AwaitingResult
					| DesktopSettingsCommandState::OutcomeUnknown
			) {
			return Err(DesktopSettingsInputError::Busy);
		}
		let identity = command_identity()?;
		self.pending_command = Some(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: identity.client_command_id,
			idempotency_key: identity.idempotency_key,
			expected_revision: Some(expected_revision),
			correlation_id: identity.correlation_id,
			causation_id: None::<CausationId>,
			payload: CommandPayload::SetDesktopSettings { show_in_menu_bar },
		});
		self.command = DesktopSettingsCommandState::Sending;
		Ok(())
	}

	fn latch_in_flight_outcome_unknown(&mut self) {
		if self.in_flight_command.take().is_some() {
			self.command = DesktopSettingsCommandState::OutcomeUnknown;
		}
	}
}

struct CommandIdentity {
	client_command_id: ClientCommandId,
	idempotency_key: IdempotencyKey,
	correlation_id: CorrelationId,
}

fn command_identity() -> Result<CommandIdentity, DesktopSettingsInputError> {
	let value = canonical_uuid_v4()?;
	Ok(CommandIdentity {
		client_command_id: ClientCommandId::new(format!("gpui-desktop-settings/{value}"))
			.expect("canonical desktop settings command identity is bounded"),
		idempotency_key: IdempotencyKey::new(format!("desktop-settings/{value}"))
			.expect("canonical desktop settings idempotency key is bounded"),
		correlation_id: CorrelationId::new(value)
			.expect("canonical desktop settings correlation identity is bounded"),
	})
}

fn canonical_uuid_v4() -> Result<String, DesktopSettingsInputError> {
	let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_err(|_| DesktopSettingsInputError::IdentityUnavailable)?
		.as_nanos();
	let mut digest = Sha256::new();
	digest.update(std::process::id().to_be_bytes());
	digest.update(sequence.to_be_bytes());
	digest.update(nanos.to_be_bytes());
	let mut bytes: [u8; 16] = digest.finalize()[..16]
		.try_into()
		.map_err(|_| DesktopSettingsInputError::IdentityUnavailable)?;
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
		CURRENT_VERSION, CommandOutcome, CommandReceipt, CommandResultEnvelope, DesktopSettingsDto,
		DesktopSettingsResult, EntityRevision, QueryResultEnvelope, QueryResultPayload,
		ReceiptDisposition, ResultPayload, ServerId,
	};

	use super::{
		DesktopSettingsCommandState, DesktopSettingsController, DesktopSettingsDispatch,
		DesktopSettingsLoadState, DesktopSettingsRouteOutcome,
	};

	fn server() -> ServerId {
		ServerId::new("10000000-0000-4000-8000-000000000001").expect("server identity is bounded")
	}

	#[tokio::test]
	async fn connection_queries_daemon_owned_settings_before_enabling_the_toggle() {
		let controller = DesktopSettingsController::production();
		let server = server();
		controller.bind_session(4, server.clone());
		let DesktopSettingsDispatch::Query(query) = controller.next_dispatch(4, &server).await
		else {
			panic!("first dispatch must query settings")
		};
		assert!(matches!(query.payload, decodex_protocol::QueryPayload::GetDesktopSettings));
		assert_eq!(controller.snapshot().load, DesktopSettingsLoadState::Loading);

		assert_eq!(
			controller.route_query_result(
				4,
				&server,
				&QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					query_id: query.query_id,
					payload: QueryResultPayload::DesktopSettings(DesktopSettingsResult::Available(
						DesktopSettingsDto { show_in_menu_bar: true, revision: EntityRevision(3) }
					),),
				},
			),
			DesktopSettingsRouteOutcome::Fresh
		);
		let snapshot = controller.snapshot();
		assert_eq!(snapshot.load, DesktopSettingsLoadState::Ready);
		assert!(snapshot.can_toggle);
		assert!(snapshot.settings.expect("settings are available").show_in_menu_bar);
	}

	#[tokio::test]
	async fn toggle_applies_only_the_matching_daemon_result() {
		let controller = DesktopSettingsController::production();
		let server = server();
		controller.bind_session(7, server.clone());
		let DesktopSettingsDispatch::Query(query) = controller.next_dispatch(7, &server).await
		else {
			panic!("first dispatch must query settings")
		};
		controller.route_query_result(
			7,
			&server,
			&QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: server.clone(),
				query_id: query.query_id,
				payload: QueryResultPayload::DesktopSettings(DesktopSettingsResult::Available(
					DesktopSettingsDto { show_in_menu_bar: true, revision: EntityRevision(8) },
				)),
			},
		);

		controller.set_show_in_menu_bar(false).expect("queue menu-bar preference");
		let dispatch = controller.next_dispatch(7, &server).await;
		let command = dispatch.command().expect("toggle dispatch is a command").clone();
		controller.command_sent(&dispatch);
		assert_eq!(controller.snapshot().command, DesktopSettingsCommandState::AwaitingResult);
		assert_eq!(
			controller.route_receipt(
				7,
				&server,
				&CommandReceipt {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					client_command_id: command.client_command_id.clone(),
					idempotency_key: command.idempotency_key.clone(),
					disposition: ReceiptDisposition::Executed,
					original_client_command_id: command.client_command_id.clone(),
				},
			),
			DesktopSettingsRouteOutcome::Fresh
		);
		let settings = DesktopSettingsDto { show_in_menu_bar: false, revision: EntityRevision(9) };
		assert_eq!(
			controller.route_command_result(
				7,
				&server,
				&CommandResultEnvelope {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					client_command_id: command.client_command_id,
					idempotency_key: command.idempotency_key,
					outcome: CommandOutcome::Succeeded,
					entity_revision: Some(settings.revision),
					payload: Some(ResultPayload::DesktopSettingsChanged { settings }),
					error: None,
				},
			),
			DesktopSettingsRouteOutcome::Fresh
		);
		let snapshot = controller.snapshot();
		assert_eq!(snapshot.command, DesktopSettingsCommandState::Accepted);
		assert!(!snapshot.settings.expect("settings remain available").show_in_menu_bar);
	}
}
