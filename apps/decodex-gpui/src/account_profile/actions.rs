//! Source-bound preparation tickets for explicit Accounts recovery interactions.
use super::{AccountProfileController, SessionBinding};
use decodex_protocol::{
	AccountRecoveryAction, AccountRecoveryDestination, AccountRecoveryPreparation,
	AccountRecoveryResult, AccountRecoveryState,
};

#[derive(Clone)]
pub(crate) struct RecoveryActionTicket {
	pub(crate) source: AccountRecoveryResult,
	pub(crate) action: AccountRecoveryAction,
	binding: SessionBinding,
	interaction_epoch: u64,
	created_at: std::time::Instant,
}
impl AccountProfileController {
	pub(crate) fn apply_event(&self, event: &decodex_protocol::EventEnvelope) {
		if let decodex_protocol::EventPayload::AccountRecoveryNudge { account_id, .. } =
			&event.payload
		{
			let mut state = self.lock();
			if state.selected.as_ref() == Some(account_id) {
				state.notification_generation = state.notification_generation.saturating_add(1);
			}
		}
	}

	pub(crate) fn begin_recovery_action(
		&self,
		source: &AccountRecoveryResult,
		action: AccountRecoveryAction,
	) -> Option<RecoveryActionTicket> {
		let state = self.lock();
		if state.interaction_epoch == u64::MAX || state.snapshot().recovery.as_ref() != Some(source)
		{
			return None;
		}
		let AccountRecoveryState::Current(banner) = &source.state else {
			return None;
		};
		if !banner.actions.iter().any(|cta| cta.action == action) {
			return None;
		}
		Some(RecoveryActionTicket {
			source: source.clone(),
			action,
			binding: state.session.clone()?,
			interaction_epoch: state.interaction_epoch,
			created_at: std::time::Instant::now(),
		})
	}

	pub(crate) fn finish_recovery_action(
		&self,
		ticket: &RecoveryActionTicket,
		prepared: AccountRecoveryPreparation,
	) -> Option<AccountRecoveryDestination> {
		let state = self.lock();
		let snapshot = state.snapshot();
		if ticket.created_at.elapsed() > std::time::Duration::from_secs(20)
			|| state.session.as_ref() != Some(&ticket.binding)
			|| state.interaction_epoch != ticket.interaction_epoch
			|| !same_recovery_source(snapshot.recovery.as_ref(), Some(&ticket.source))
			|| snapshot.recovery.as_ref().is_some_and(|current| {
				current.observed_at_unix_micros < ticket.source.observed_at_unix_micros
			}) || !matches!(
			snapshot.recovery.as_ref().map(|r| &r.state),
			Some(AccountRecoveryState::Current(_))
		) || !prepared.valid_for(&ticket.source, ticket.action)
		{
			return None;
		}
		match prepared {
			AccountRecoveryPreparation::Ready { destination, .. } => Some(destination),
			AccountRecoveryPreparation::Unavailable => None,
		}
	}
}

pub(super) fn same_recovery_source(
	left: Option<&AccountRecoveryResult>,
	right: Option<&AccountRecoveryResult>,
) -> bool {
	match (left, right) {
		(Some(left), Some(right))
			if left.account_id == right.account_id
				&& left.account_revision == right.account_revision =>
			match (&left.state, &right.state) {
				(
					AccountRecoveryState::Current(a) | AccountRecoveryState::Stale(a),
					AccountRecoveryState::Current(b) | AccountRecoveryState::Stale(b),
				) => a == b,
				(a, b) => a == b,
			},
		(None, None) => true,
		_ => false,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::{
		AccountRecoveryBanner, AccountRecoveryCta, EntityId, EntityRevision, ServerId, WireText,
	};
	fn fixture()
	-> (AccountProfileController, AccountRecoveryResult, AccountRecoveryPreparation, ServerId) {
		let source = AccountRecoveryResult {
			account_id: EntityId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			account_revision: EntityRevision(1),
			observed_at_unix_micros: Some(
				i64::try_from(
					std::time::SystemTime::now()
						.duration_since(std::time::UNIX_EPOCH)
						.unwrap()
						.as_micros(),
				)
				.unwrap(),
			),
			state: AccountRecoveryState::Current(Box::new(AccountRecoveryBanner {
				banner_type: WireText::new("limit").unwrap(),
				title: WireText::new("Limit").unwrap(),
				description: WireText::new("Description").unwrap(),
				reset_at: None,
				model_slug: None,
				blocked_model_slug: None,
				fallback_model_slugs: Vec::new(),
				dismissible: true,
				actions: vec![AccountRecoveryCta {
					action: AccountRecoveryAction::ViewUsage,
					label: WireText::new("Usage").unwrap(),
				}],
				request_url: None,
			})),
		};
		let controller = AccountProfileController::production();
		let server = ServerId::new("20000000-0000-4000-8000-000000000001").unwrap();
		controller.bind_session(1, server.clone());
		controller.select_at_revision(source.account_id.clone(), source.account_revision);
		controller.lock().recovery = Some(source.clone());
		let prepared = AccountRecoveryPreparation::Ready {
			source: Box::new(source.clone()),
			action: AccountRecoveryAction::ViewUsage,
			destination: AccountRecoveryDestination::OpenUrl(
				WireText::new("https://chatgpt.com/codex/settings/usage").unwrap(),
			),
		};
		(controller, source, prepared, server)
	}
	#[test]
	fn action_ticket_rejects_refresh_close_reopen_and_new_session() {
		for transition in 0..3 {
			let (controller, source, prepared, server) = fixture();
			let ticket = controller
				.begin_recovery_action(&source, AccountRecoveryAction::ViewUsage)
				.unwrap();
			assert!(controller.finish_recovery_action(&ticket, prepared.clone()).is_some());
			match transition {
				0 => {
					controller.refresh();
				},
				1 => {
					controller.close();
					controller
						.select_at_revision(source.account_id.clone(), source.account_revision);
				},
				_ => {
					controller.session_ended(1);
					controller.bind_session(2, server);
				},
			}
			// Even byte-identical reloaded copy cannot revive the previous interaction.
			controller.lock().recovery = Some(source);
			assert!(controller.finish_recovery_action(&ticket, prepared).is_none());
		}
	}
	#[test]
	fn background_query_sequence_and_same_copy_timestamp_do_not_cancel_explicit_action() {
		let (controller, source, prepared, _) = fixture();
		let mut ticket =
			controller.begin_recovery_action(&source, AccountRecoveryAction::ViewUsage).unwrap();
		{
			let mut state = controller.lock();
			state.next_sequence += 10;
			state.recovery.as_mut().unwrap().observed_at_unix_micros = Some(
				i64::try_from(
					std::time::SystemTime::now()
						.duration_since(std::time::UNIX_EPOCH)
						.unwrap()
						.as_micros(),
				)
				.unwrap(),
			);
		}
		assert!(controller.finish_recovery_action(&ticket, prepared.clone()).is_some());
		ticket.created_at = std::time::Instant::now() - std::time::Duration::from_secs(21);
		assert!(controller.finish_recovery_action(&ticket, prepared).is_none());
	}

	#[test]
	fn changed_observation_generation_rejects_prior_action_even_if_copy_returns() {
		let (controller, source, prepared, server) = fixture();
		let ticket =
			controller.begin_recovery_action(&source, AccountRecoveryAction::ViewUsage).unwrap();
		let mut state = controller.lock();
		let query_id = decodex_protocol::QueryId::new("watch").unwrap();
		state.observation =
			Some((query_id.clone(), SessionBinding { generation: 1, server_id: server.clone() }));
		state.accept_observation(
			1,
			&server,
			&decodex_protocol::QueryResultEnvelope {
				version: decodex_protocol::CURRENT_VERSION,
				server_id: server.clone(),
				query_id,
				payload: decodex_protocol::QueryResultPayload::AccountObservation(
					decodex_protocol::AccountObservationSignal::new(2),
				),
			},
		);
		state.recovery = Some(source);
		drop(state);
		assert!(controller.finish_recovery_action(&ticket, prepared).is_none());
	}

	#[test]
	fn action_ticket_rejects_unoffered_and_dismissed_actions() {
		let (controller, source, prepared, _) = fixture();
		assert!(
			controller.begin_recovery_action(&source, AccountRecoveryAction::ResetUsage).is_none()
		);
		let ticket =
			controller.begin_recovery_action(&source, AccountRecoveryAction::ViewUsage).unwrap();
		assert!(controller.dismiss_recovery(&source));
		assert!(controller.finish_recovery_action(&ticket, prepared).is_none());
	}
}
