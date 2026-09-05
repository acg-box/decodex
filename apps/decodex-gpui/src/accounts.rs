//! Presentation-neutral ownership of the bounded GPUI account-pool surface.

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
	AccountCommandRejectionDto, AccountDto, AccountRoutingControlDto, AccountSelectionModeDto,
	AccountsResult, CURRENT_VERSION, CausationId, ClientCommandId, CommandEnvelope, CommandError,
	CommandOutcome, CommandPayload, CommandReceipt, CommandResultEnvelope, CorrelationId, EntityId,
	EntityRevision, EventEnvelope, EventPayload, IdempotencyKey, QueryEnvelope, QueryId,
	QueryPayload, QueryResultEnvelope, QueryResultPayload, ReceiptDisposition, ResultPayload,
	ServerId,
};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Current bounded state rendered by the Accounts destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountsSnapshot {
	pub(crate) load: AccountsLoadState,
	pub(crate) command: AccountCommandState,
	pub(crate) accounts: Vec<AccountDto>,
	pub(crate) routing: Option<AccountRoutingControlDto>,
	pub(crate) rejection: Option<AccountCommandRejectionDto>,
	pub(crate) can_manage: bool,
	pub(crate) can_route: bool,
	pub(crate) route_reopen_notice: bool,
}

/// Finite account-pool readback state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountsLoadState {
	NeverRequested,
	Loading,
	Ready,
	Offline,
	Stale,
	Unavailable,
	Refused,
}

/// Finite state for the one possibly side-effecting account command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountCommandState {
	Idle,
	Sending,
	AwaitingResult,
	Accepted,
	OutcomeUnknown,
	Refused,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountInputError {
	Offline,
	Busy,
	AccountMissing,
	RoutingUnavailable,
	IdentityUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccountRouteOutcome {
	Fresh,
	Unmatched,
	Refused,
}

/// Exactly one account query or command reserved for one retained-session generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AccountDispatch {
	Query(QueryEnvelope),
	Command(CommandEnvelope),
}

impl AccountDispatch {
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

/// Cloneable account-pool controller. It owns no transport or credential authority.
#[derive(Clone)]
pub(crate) struct AccountsController {
	inner: Arc<AccountsInner>,
}

struct AccountsInner {
	state: Mutex<State>,
	notify: Notify,
}

impl AccountsController {
	pub(crate) fn production() -> Self {
		Self {
			inner: Arc::new(AccountsInner {
				state: Mutex::new(State::new()),
				notify: Notify::new(),
			}),
		}
	}

	pub(crate) fn activate(&self) {
		let mut state = self.lock();
		if state.active {
			return;
		}
		state.active = true;
		state.ever_activated = true;
		let queued = state.queue_list();
		if state.session.is_none() {
			state.load = AccountsLoadState::Offline;
		}
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
	}

	pub(crate) fn deactivate(&self) {
		self.lock().active = false;
	}

	pub(crate) fn refresh(&self) -> bool {
		let mut state = self.lock();
		let queued = state.active && state.queue_list();
		drop(state);
		if queued {
			self.inner.notify.notify_one();
		}
		queued
	}

	pub(crate) fn snapshot(&self) -> AccountsSnapshot {
		self.lock().snapshot()
	}

	pub(crate) fn set_enabled(
		&self,
		account_id: &EntityId,
		enabled: bool,
	) -> Result<(), AccountInputError> {
		let mut state = self.lock();
		let account = state
			.accounts
			.iter()
			.find(|account| &account.account_id == account_id)
			.ok_or(AccountInputError::AccountMissing)?;
		let revision = account.account_revision;
		if account.enabled == enabled {
			return Ok(());
		}
		state.queue_command(
			CommandPayload::SetAccountEnabled { account_id: account_id.clone(), enabled },
			Some(revision),
		)?;
		drop(state);
		self.inner.notify.notify_one();
		Ok(())
	}

	pub(crate) fn select_fixed(&self, account_id: &EntityId) -> Result<(), AccountInputError> {
		let mut state = self.lock();
		if !state.accounts.iter().any(|account| &account.account_id == account_id) {
			return Err(AccountInputError::AccountMissing);
		}
		if state.routing.is_none() {
			return Err(AccountInputError::RoutingUnavailable);
		}
		if state.routing.as_ref().is_some_and(|routing| {
			matches!(&routing.mode, AccountSelectionModeDto::Fixed(current) if current == account_id)
		}) {
			return Ok(());
		}
		state
			.queue_command(CommandPayload::RouteAccount { account_id: account_id.clone() }, None)?;
		drop(state);
		self.inner.notify.notify_one();
		Ok(())
	}

	pub(crate) fn select_balanced(&self) -> Result<(), AccountInputError> {
		let mut state = self.lock();
		let routing_revision = state
			.routing
			.as_ref()
			.map(|routing| routing.revision)
			.ok_or(AccountInputError::RoutingUnavailable)?;
		state.queue_command(CommandPayload::SetBalancedAccountSelection, Some(routing_revision))?;
		drop(state);
		self.inner.notify.notify_one();
		Ok(())
	}

	pub(crate) fn logout(&self, account_id: &EntityId) -> Result<(), AccountInputError> {
		let mut state = self.lock();
		let revision = state
			.accounts
			.iter()
			.find(|account| &account.account_id == account_id)
			.map(|account| account.account_revision)
			.ok_or(AccountInputError::AccountMissing)?;
		let operation_id = EntityId::new(canonical_uuid_v4()?)
			.map_err(|_| AccountInputError::IdentityUnavailable)?;
		state.queue_command(
			CommandPayload::LogoutAccount { operation_id, account_id: account_id.clone() },
			Some(revision),
		)?;
		drop(state);
		self.inner.notify.notify_one();
		Ok(())
	}

	pub(crate) fn move_account(
		&self,
		account_id: &EntityId,
		offset: isize,
	) -> Result<(), AccountInputError> {
		let mut state = self.lock();
		let (routing_revision, mut order) = state
			.routing
			.as_ref()
			.map(|routing| (routing.revision, routing.order.clone()))
			.ok_or(AccountInputError::RoutingUnavailable)?;
		let index = order
			.iter()
			.position(|current| current == account_id)
			.ok_or(AccountInputError::AccountMissing)?;
		let Some(target) = index.checked_add_signed(offset) else {
			return Ok(());
		};
		if target >= order.len() {
			return Ok(());
		}
		order.swap(index, target);
		state.queue_command(CommandPayload::SetAccountOrder { order }, Some(routing_revision))?;
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
		state.reset_query();
		state.session = Some(binding);
		let command_queued = state.pending_command.is_some();
		let query_queued = state.active && state.queue_list();
		if !state.active {
			state.load = if state.ever_activated {
				AccountsLoadState::Stale
			} else {
				AccountsLoadState::NeverRequested
			};
		}
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
		if state.pending_command.take().is_some()
			&& state.command != AccountCommandState::OutcomeUnknown
		{
			state.command = AccountCommandState::Refused;
		}
		state.reset_query();
		state.session = None;
		state.load = if state.ever_activated {
			AccountsLoadState::Offline
		} else {
			AccountsLoadState::NeverRequested
		};
	}

	pub(crate) async fn next_dispatch(
		&self,
		generation: u64,
		server_id: &ServerId,
	) -> AccountDispatch {
		loop {
			let notified = self.inner.notify.notified();
			if let Some(dispatch) = self.try_take_dispatch(generation, server_id) {
				return dispatch;
			}
			notified.await;
		}
	}

	fn try_take_dispatch(&self, generation: u64, server_id: &ServerId) -> Option<AccountDispatch> {
		let mut state = self.lock();
		let binding = SessionBinding { generation, server_id: server_id.clone() };
		if state.session.as_ref() != Some(&binding) {
			return None;
		}
		if state.in_flight_command.is_none()
			&& let Some(envelope) = state.pending_command.take()
		{
			state.in_flight_command = Some(InFlightCommand { envelope: envelope.clone(), binding });
			return Some(AccountDispatch::Command(envelope));
		}
		if state.in_flight_query.is_none()
			&& let Some(envelope) = state.pending_query.take()
		{
			state.in_flight_query =
				Some(InFlightQuery { query_id: envelope.query_id.clone(), binding });
			return Some(AccountDispatch::Query(envelope));
		}
		None
	}

	pub(crate) fn command_send_failed(&self, dispatch: &AccountDispatch) {
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

	pub(crate) fn command_sent(&self, dispatch: &AccountDispatch) {
		let Some(command) = dispatch.command() else {
			return;
		};
		let mut state = self.lock();
		if state.in_flight_command.as_ref().is_some_and(|in_flight| {
			in_flight.envelope.client_command_id == command.client_command_id
		}) {
			state.command = AccountCommandState::AwaitingResult;
		}
	}

	pub(crate) fn apply_event(&self, event: &EventEnvelope) {
		let mut state = self.lock();
		match &event.payload {
			EventPayload::AccountChanged { account }
				if account.account_id == event.entity_id
					&& account.account_revision == event.entity_revision =>
			{
				state.upsert_account((**account).clone());
			},
			EventPayload::AccountLoggedOut { account_id, tombstone_revision }
				if account_id == &event.entity_id
					&& tombstone_revision == &event.entity_revision =>
			{
				state.accounts.retain(|account| account.account_id != *account_id);
			},
			EventPayload::AccountRoutingChanged { routing }
				if routing.revision == event.entity_revision =>
			{
				state.routing = Some(routing.clone());
				state.sort_accounts();
			},
			EventPayload::AccountRouted { account, routing, .. }
				if matches!(&routing.mode, AccountSelectionModeDto::Fixed(selected) if selected == &account.account_id)
					&& routing.revision == event.entity_revision =>
			{
				state.upsert_account((**account).clone());
				state.routing = Some(routing.clone());
				state.route_reopen_notice = true;
				state.command = AccountCommandState::Accepted;
				state.sort_accounts();
			},
			_ => {},
		}
	}

	pub(crate) fn route_query_result(
		&self,
		generation: u64,
		server_id: &ServerId,
		result: &QueryResultEnvelope,
	) -> AccountRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_query.as_ref() else {
			return AccountRouteOutcome::Unmatched;
		};
		if in_flight.query_id != result.query_id {
			return AccountRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| result.version != CURRENT_VERSION
			|| result.server_id != *server_id
		{
			state.in_flight_query = None;
			state.load = AccountsLoadState::Refused;
			return AccountRouteOutcome::Refused;
		}
		state.in_flight_query = None;
		match &result.payload {
			QueryResultPayload::Accounts(AccountsResult::Available { accounts, routing }) => {
				state.accounts = accounts.clone();
				state.routing = routing.clone();
				state.sort_accounts();
				state.load = AccountsLoadState::Ready;
				AccountRouteOutcome::Fresh
			},
			QueryResultPayload::Accounts(AccountsResult::Unavailable) => {
				state.load = AccountsLoadState::Unavailable;
				AccountRouteOutcome::Fresh
			},
			_ => {
				state.load = AccountsLoadState::Refused;
				AccountRouteOutcome::Refused
			},
		}
	}

	pub(crate) fn route_receipt(
		&self,
		generation: u64,
		server_id: &ServerId,
		receipt: &CommandReceipt,
	) -> AccountRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_command.as_ref() else {
			return AccountRouteOutcome::Unmatched;
		};
		if in_flight.envelope.client_command_id != receipt.client_command_id {
			return AccountRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| receipt.version != CURRENT_VERSION
			|| receipt.server_id != *server_id
			|| receipt.idempotency_key != in_flight.envelope.idempotency_key
		{
			state.latch_in_flight_outcome_unknown();
			return AccountRouteOutcome::Refused;
		}
		match receipt.disposition {
			ReceiptDisposition::Executed | ReceiptDisposition::Duplicate => {
				state.command = AccountCommandState::AwaitingResult;
			},
			ReceiptDisposition::Refused => {
				state.in_flight_command = None;
				state.command = AccountCommandState::Refused;
			},
		}
		AccountRouteOutcome::Fresh
	}

	pub(crate) fn route_command_result(
		&self,
		generation: u64,
		server_id: &ServerId,
		result: &CommandResultEnvelope,
	) -> AccountRouteOutcome {
		let mut state = self.lock();
		let Some(in_flight) = state.in_flight_command.as_ref() else {
			return AccountRouteOutcome::Unmatched;
		};
		if in_flight.envelope.client_command_id != result.client_command_id {
			return AccountRouteOutcome::Unmatched;
		}
		let expected = SessionBinding { generation, server_id: server_id.clone() };
		if in_flight.binding != expected
			|| state.session.as_ref() != Some(&expected)
			|| result.version != CURRENT_VERSION
			|| result.server_id != *server_id
			|| result.idempotency_key != in_flight.envelope.idempotency_key
		{
			state.latch_in_flight_outcome_unknown();
			return AccountRouteOutcome::Refused;
		}
		let in_flight =
			state.in_flight_command.take().expect("matching account command remains in flight");
		let (outcome, query_queued) = match result.outcome {
			CommandOutcome::Succeeded => {
				state.rejection = None;
				let logout_succeeded =
					matches!(&in_flight.envelope.payload, CommandPayload::LogoutAccount { .. });
				if state.apply_success(&in_flight.envelope, result) {
					state.command = AccountCommandState::Accepted;
					let query_queued = if logout_succeeded {
						state.reset_query();
						state.queue_list()
					} else {
						false
					};
					(AccountRouteOutcome::Fresh, query_queued)
				} else {
					state.command = AccountCommandState::Refused;
					(AccountRouteOutcome::Refused, false)
				}
			},
			CommandOutcome::AcceptanceUnknown => {
				state.command = AccountCommandState::OutcomeUnknown;
				(AccountRouteOutcome::Fresh, false)
			},
			CommandOutcome::Rejected => {
				state.rejection = match result.error.as_ref() {
					Some(CommandError::AccountCommandRejected { rejection, .. }) =>
						Some(*rejection),
					_ => None,
				};
				state.command = AccountCommandState::Refused;
				(AccountRouteOutcome::Fresh, false)
			},
		};
		drop(state);
		if query_queued {
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
	active: bool,
	ever_activated: bool,
	next_query_sequence: u64,
	pending_query: Option<QueryEnvelope>,
	in_flight_query: Option<InFlightQuery>,
	pending_command: Option<CommandEnvelope>,
	in_flight_command: Option<InFlightCommand>,
	load: AccountsLoadState,
	command: AccountCommandState,
	accounts: Vec<AccountDto>,
	routing: Option<AccountRoutingControlDto>,
	rejection: Option<AccountCommandRejectionDto>,
	route_reopen_notice: bool,
}

impl State {
	const fn new() -> Self {
		Self {
			session: None,
			active: false,
			ever_activated: false,
			next_query_sequence: 0,
			pending_query: None,
			in_flight_query: None,
			pending_command: None,
			in_flight_command: None,
			load: AccountsLoadState::NeverRequested,
			command: AccountCommandState::Idle,
			accounts: Vec::new(),
			routing: None,
			rejection: None,
			route_reopen_notice: false,
		}
	}

	fn snapshot(&self) -> AccountsSnapshot {
		let idle = self.session.is_some()
			&& self.load == AccountsLoadState::Ready
			&& self.pending_command.is_none()
			&& self.in_flight_command.is_none()
			&& !matches!(
				self.command,
				AccountCommandState::Sending
					| AccountCommandState::AwaitingResult
					| AccountCommandState::OutcomeUnknown
			);
		AccountsSnapshot {
			load: self.load,
			command: self.command,
			accounts: self.accounts.clone(),
			routing: self.routing.clone(),
			rejection: self.rejection,
			can_manage: idle,
			can_route: idle,
			route_reopen_notice: self.route_reopen_notice,
		}
	}

	fn queue_list(&mut self) -> bool {
		let Some(generation) = self.session.as_ref().map(|session| session.generation) else {
			return false;
		};
		if self.pending_query.is_some() || self.in_flight_query.is_some() {
			return false;
		}
		let Some(sequence) = self.next_query_sequence.checked_add(1) else {
			self.load = AccountsLoadState::Refused;
			return false;
		};
		self.next_query_sequence = sequence;
		self.pending_query = Some(QueryEnvelope {
			version: CURRENT_VERSION,
			query_id: QueryId::new(format!("gpui-accounts/{generation}/{sequence}"))
				.expect("bounded numeric account query identity"),
			payload: QueryPayload::ListAccounts,
		});
		self.load = AccountsLoadState::Loading;
		true
	}

	fn queue_command(
		&mut self,
		payload: CommandPayload,
		expected_revision: Option<EntityRevision>,
	) -> Result<(), AccountInputError> {
		if self.session.is_none() {
			return Err(AccountInputError::Offline);
		}
		if self.pending_command.is_some()
			|| self.in_flight_command.is_some()
			|| matches!(
				self.command,
				AccountCommandState::Sending
					| AccountCommandState::AwaitingResult
					| AccountCommandState::OutcomeUnknown
			) {
			return Err(AccountInputError::Busy);
		}
		let identity = command_identity()?;
		self.pending_command = Some(CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: identity.client_command_id,
			idempotency_key: identity.idempotency_key,
			expected_revision,
			correlation_id: identity.correlation_id,
			causation_id: None::<CausationId>,
			payload,
		});
		self.route_reopen_notice = false;
		self.rejection = None;
		self.command = AccountCommandState::Sending;
		Ok(())
	}

	fn reset_query(&mut self) {
		self.pending_query = None;
		self.in_flight_query = None;
	}

	fn latch_in_flight_outcome_unknown(&mut self) {
		if self.in_flight_command.take().is_some() {
			self.command = AccountCommandState::OutcomeUnknown;
		}
	}

	fn upsert_account(&mut self, account: AccountDto) {
		if let Some(existing) =
			self.accounts.iter_mut().find(|existing| existing.account_id == account.account_id)
		{
			if account.account_revision >= existing.account_revision {
				*existing = account;
			}
		} else {
			self.accounts.push(account);
		}
		self.sort_accounts();
	}

	fn sort_accounts(&mut self) {
		if let Some(routing) = self.routing.as_ref() {
			self.accounts.sort_by_key(|account| {
				routing
					.order
					.iter()
					.position(|account_id| account_id == &account.account_id)
					.unwrap_or(usize::MAX)
			});
		} else {
			self.accounts.sort_by(|left, right| left.alias.as_str().cmp(right.alias.as_str()));
		}
	}

	fn apply_success(&mut self, command: &CommandEnvelope, result: &CommandResultEnvelope) -> bool {
		match (&command.payload, result.payload.as_ref()) {
			(
				CommandPayload::LogoutAccount { account_id, .. },
				Some(ResultPayload::AccountLoggedOut {
					account_id: logged_out,
					tombstone_revision,
				}),
			) if account_id == logged_out
				&& result.entity_revision == Some(*tombstone_revision) =>
			{
				self.accounts.retain(|account| &account.account_id != account_id);
				true
			},
			(
				CommandPayload::SetAccountEnabled { account_id, enabled },
				Some(ResultPayload::AccountChanged { account }),
			) if &account.account_id == account_id
				&& account.enabled == *enabled
				&& result.entity_revision == Some(account.account_revision) =>
			{
				self.upsert_account((**account).clone());
				true
			},
			(
				CommandPayload::RouteAccount { account_id, .. },
				Some(ResultPayload::AccountRouted { account, routing, .. }),
			) if account.account_id == *account_id
				&& matches!(&routing.mode, AccountSelectionModeDto::Fixed(selected) if selected == account_id)
				&& result.entity_revision == Some(routing.revision) =>
			{
				self.upsert_account((**account).clone());
				self.routing = Some(routing.clone());
				self.route_reopen_notice = true;
				self.sort_accounts();
				true
			},
			(
				CommandPayload::SetBalancedAccountSelection,
				Some(ResultPayload::AccountRoutingChanged { routing }),
			) if routing.mode == AccountSelectionModeDto::Balanced
				&& result.entity_revision == Some(routing.revision) =>
			{
				self.routing = Some(routing.clone());
				self.sort_accounts();
				true
			},
			(
				CommandPayload::SetAccountOrder { order },
				Some(ResultPayload::AccountRoutingChanged { routing }),
			) if &routing.order == order && result.entity_revision == Some(routing.revision) => {
				self.routing = Some(routing.clone());
				self.sort_accounts();
				true
			},
			_ => false,
		}
	}
}

struct CommandIdentity {
	client_command_id: ClientCommandId,
	idempotency_key: IdempotencyKey,
	correlation_id: CorrelationId,
}

fn command_identity() -> Result<CommandIdentity, AccountInputError> {
	let value = canonical_uuid_v4()?;
	Ok(CommandIdentity {
		client_command_id: ClientCommandId::new(format!("gpui-account/{value}"))
			.expect("canonical account command identity is bounded"),
		idempotency_key: IdempotencyKey::new(format!("account/{value}"))
			.expect("canonical account idempotency key is bounded"),
		correlation_id: CorrelationId::new(value)
			.expect("canonical account correlation identity is bounded"),
	})
}

pub(crate) fn canonical_uuid_v4() -> Result<String, AccountInputError> {
	let sequence = NEXT_ID.fetch_add(1, Ordering::Relaxed);
	let nanos = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_err(|_| AccountInputError::IdentityUnavailable)?
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
	use decodex_protocol::{
		AccountLifecycleReadinessDto, AccountObservedStateDto, AccountQuotaStateDto,
		AccountQuotaWindowDto, Sha256Digest, WireText,
	};

	use super::*;

	fn server() -> ServerId {
		ServerId::new("018f0f9e-7b6e-4a31-8f4c-1d2e3f405162")
			.expect("test server identity is bounded")
	}

	fn account(id: &str, alias: &str, enabled: bool, revision: u64) -> AccountDto {
		let quota = |duration_minutes| AccountQuotaWindowDto {
			duration_minutes,
			observed_at_unix_micros: None,
			result: AccountQuotaStateDto::Unknown,
		};
		AccountDto {
			account_id: EntityId::new(id).expect("test account identity is canonical"),
			alias: WireText::new(alias).expect("test alias is bounded"),
			enabled,
			account_revision: EntityRevision(revision),
			observed_state: AccountObservedStateDto::Available,
			lifecycle_readiness: AccountLifecycleReadinessDto::Ready,
			credential_binding: None,
			unsettled_operation: None,
			five_hour_quota: quota(300),
			seven_day_quota: quota(10_080),
		}
	}

	#[test]
	fn activation_dispatches_one_account_list_for_the_bound_session() {
		let controller = AccountsController::production();
		let server = server();
		controller.bind_session(4, server.clone());

		assert!(controller.try_take_dispatch(4, &server).is_none());
		controller.activate();

		let dispatch = controller
			.try_take_dispatch(4, &server)
			.expect("activation queues exactly one account read");
		assert!(matches!(
			dispatch.query().map(|query| &query.payload),
			Some(QueryPayload::ListAccounts)
		));
		assert!(controller.try_take_dispatch(4, &server).is_none());
	}

	#[test]
	fn account_list_enables_revision_guarded_pool_management() {
		let controller = AccountsController::production();
		let server = server();
		let first = account("10000000-0000-4000-8000-000000000001", "Primary", true, 3);
		let second = account("10000000-0000-4000-8000-000000000002", "Reserve", true, 7);
		controller.bind_session(2, server.clone());
		controller.activate();
		let AccountDispatch::Query(query) =
			controller.try_take_dispatch(2, &server).expect("account query is reserved")
		else {
			panic!("account activation must query")
		};
		let routing = AccountRoutingControlDto {
			revision: EntityRevision(5),
			mode: AccountSelectionModeDto::Balanced,
			order: vec![first.account_id.clone(), second.account_id.clone()],
		};
		assert_eq!(
			controller.route_query_result(
				2,
				&server,
				&QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					query_id: query.query_id,
					payload: QueryResultPayload::Accounts(AccountsResult::Available {
						accounts: vec![second.clone(), first.clone()],
						routing: Some(routing.clone()),
					}),
				},
			),
			AccountRouteOutcome::Fresh
		);
		let snapshot = controller.snapshot();
		assert!(snapshot.can_manage);
		assert_eq!(snapshot.accounts, vec![first.clone(), second.clone()]);

		controller
			.select_fixed(&second.account_id)
			.expect("ready account can become the fixed route");
		let dispatch =
			controller.try_take_dispatch(2, &server).expect("fixed selection queues one command");
		let command = dispatch.command().expect("fixed selection dispatches a command").clone();
		assert_eq!(command.expected_revision, None);
		assert!(matches!(
			&command.payload,
			CommandPayload::RouteAccount { account_id } if account_id == &second.account_id
		));
		controller.command_sent(&dispatch);
		let routed = AccountRoutingControlDto {
			revision: EntityRevision(6),
			mode: AccountSelectionModeDto::Fixed(second.account_id.clone()),
			order: routing.order,
		};
		assert_eq!(
			controller.route_command_result(
				2,
				&server,
				&CommandResultEnvelope {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					client_command_id: command.client_command_id,
					idempotency_key: command.idempotency_key,
					outcome: CommandOutcome::Succeeded,
					entity_revision: Some(routed.revision),
					payload: Some(ResultPayload::AccountRouted {
						account: Box::new(second.clone()),
						routing: routed.clone(),
						projection_digest: Sha256Digest::new("a".repeat(64)).unwrap(),
					}),
					error: None,
				},
			),
			AccountRouteOutcome::Fresh
		);
		let routed_snapshot = controller.snapshot();
		assert_eq!(routed_snapshot.routing, Some(routed));
		assert!(routed_snapshot.route_reopen_notice);
	}

	#[test]
	fn reorder_and_logout_use_revision_guarded_daemon_commands() {
		let setup = || {
			let controller = AccountsController::production();
			let server = server();
			let first = account("10000000-0000-4000-8000-000000000001", "Primary", true, 3);
			let second = account("10000000-0000-4000-8000-000000000002", "Reserve", true, 7);
			controller.bind_session(2, server.clone());
			controller.activate();
			let AccountDispatch::Query(query) = controller.try_take_dispatch(2, &server).unwrap()
			else {
				panic!("account activation must query")
			};
			controller.route_query_result(
				2,
				&server,
				&QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					query_id: query.query_id,
					payload: QueryResultPayload::Accounts(AccountsResult::Available {
						accounts: vec![first.clone(), second.clone()],
						routing: Some(AccountRoutingControlDto {
							revision: EntityRevision(5),
							mode: AccountSelectionModeDto::Balanced,
							order: vec![first.account_id.clone(), second.account_id.clone()],
						}),
					}),
				},
			);
			(controller, server, first, second)
		};

		let (controller, server, first, second) = setup();
		controller.move_account(&second.account_id, -1).expect("queue order replacement");
		let AccountDispatch::Command(order) = controller.try_take_dispatch(2, &server).unwrap()
		else {
			panic!("reorder must dispatch a command")
		};
		assert_eq!(order.expected_revision, Some(EntityRevision(5)));
		assert!(matches!(
			order.payload,
			CommandPayload::SetAccountOrder { order }
				if order == vec![second.account_id.clone(), first.account_id.clone()]
		));

		let (controller, server, first, _) = setup();
		controller.logout(&first.account_id).expect("queue account logout");
		let AccountDispatch::Command(logout) = controller.try_take_dispatch(2, &server).unwrap()
		else {
			panic!("logout must dispatch a command")
		};
		assert_eq!(logout.expected_revision, Some(EntityRevision(3)));
		assert!(matches!(
			logout.payload,
			CommandPayload::LogoutAccount { account_id, .. } if account_id == first.account_id
		));
	}

	#[test]
	fn logout_success_requires_fresh_routing_before_management_resumes() {
		let controller = AccountsController::production();
		let server = server();
		let first = account("10000000-0000-4000-8000-000000000001", "Primary", true, 3);
		let second = account("10000000-0000-4000-8000-000000000002", "Reserve", true, 7);
		controller.bind_session(2, server.clone());
		controller.activate();
		let AccountDispatch::Query(initial_query) =
			controller.try_take_dispatch(2, &server).unwrap()
		else {
			panic!("account activation must query")
		};
		let stale_routing = AccountRoutingControlDto {
			revision: EntityRevision(5),
			mode: AccountSelectionModeDto::Fixed(first.account_id.clone()),
			order: vec![first.account_id.clone(), second.account_id.clone()],
		};
		assert_eq!(
			controller.route_query_result(
				2,
				&server,
				&QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					query_id: initial_query.query_id,
					payload: QueryResultPayload::Accounts(AccountsResult::Available {
						accounts: vec![first.clone(), second.clone()],
						routing: Some(stale_routing.clone()),
					}),
				},
			),
			AccountRouteOutcome::Fresh
		);

		controller.logout(&first.account_id).expect("queue account logout");
		let logout_dispatch = controller.try_take_dispatch(2, &server).expect("logout dispatch");
		let logout = logout_dispatch.command().expect("logout command").clone();
		controller.command_sent(&logout_dispatch);
		assert_eq!(
			controller.route_command_result(
				2,
				&server,
				&CommandResultEnvelope {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					client_command_id: logout.client_command_id,
					idempotency_key: logout.idempotency_key,
					outcome: CommandOutcome::Succeeded,
					entity_revision: Some(EntityRevision(4)),
					payload: Some(ResultPayload::AccountLoggedOut {
						account_id: first.account_id.clone(),
						tombstone_revision: EntityRevision(4),
					}),
					error: None,
				},
			),
			AccountRouteOutcome::Fresh
		);
		let awaiting_readback = controller.snapshot();
		assert_eq!(awaiting_readback.load, AccountsLoadState::Loading);
		assert!(!awaiting_readback.can_manage);
		assert_eq!(awaiting_readback.routing, Some(stale_routing));
		assert_eq!(awaiting_readback.accounts, vec![second.clone()]);

		let AccountDispatch::Query(readback) = controller.try_take_dispatch(2, &server).unwrap()
		else {
			panic!("logout success must queue an account readback")
		};
		assert_eq!(readback.payload, QueryPayload::ListAccounts);
		let refreshed_routing = AccountRoutingControlDto {
			revision: EntityRevision(6),
			mode: AccountSelectionModeDto::Balanced,
			order: vec![second.account_id.clone()],
		};
		assert_eq!(
			controller.route_query_result(
				2,
				&server,
				&QueryResultEnvelope {
					version: CURRENT_VERSION,
					server_id: server.clone(),
					query_id: readback.query_id,
					payload: QueryResultPayload::Accounts(AccountsResult::Available {
						accounts: vec![second.clone()],
						routing: Some(refreshed_routing.clone()),
					}),
				},
			),
			AccountRouteOutcome::Fresh
		);
		let refreshed = controller.snapshot();
		assert!(refreshed.can_manage);
		assert_eq!(refreshed.routing, Some(refreshed_routing));
		assert_eq!(refreshed.accounts, vec![second]);
	}
}
