//! Stable semantic operation identities across observation refresh, clients and restarts.
use decodex_protocol::{
	AccountRecoveryAction, AccountRecoveryNudgeResult as R, AccountRecoveryNudgeStatus as S,
	AccountRecoveryResult, AccountRecoveryState, IdempotencyKey,
};
use sha2::{Digest as _, Sha256};
pub(super) fn operation_key(
	source: &AccountRecoveryResult,
	action: AccountRecoveryAction,
	prior: &R,
	acknowledged: Option<&IdempotencyKey>,
) -> Option<IdempotencyKey> {
	let AccountRecoveryState::Current(banner) = &source.state else {
		return None;
	};
	if !source.allows_nudge(action) {
		return None;
	}
	let predecessor = match prior {
		R::Unavailable => return None,
		R::NotFound => acknowledged,
		R::Found(operation) => {
			if operation.account_id != source.account_id || operation.action != action {
				return None;
			}
			if operation.outcome == S::Uncertain && acknowledged != Some(&operation.operation_key) {
				return None;
			}
			if acknowledged.is_some_and(|key| key != &operation.operation_key) {
				return None;
			}
			Some(&operation.operation_key)
		},
	};
	let bytes = serde_json::to_vec(&(
		"account-nudge-v1",
		&source.account_id,
		source.account_revision,
		banner,
		action,
		predecessor,
	))
	.ok()?;
	let digest = Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
	IdempotencyKey::new(format!("account-nudge-{digest}")).ok()
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::{
		AccountRecoveryBanner, AccountRecoveryCta, AccountRecoveryNudgeOperation, EntityId,
		EntityRevision, WireText,
	};
	fn source() -> AccountRecoveryResult {
		AccountRecoveryResult {
			account_id: EntityId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			account_revision: EntityRevision(1),
			observed_at_unix_micros: Some(100),
			state: AccountRecoveryState::Current(Box::new(AccountRecoveryBanner {
				banner_type: WireText::new("limit").unwrap(),
				title: WireText::new("Limit").unwrap(),
				description: WireText::new("Description").unwrap(),
				reset_at: None,
				model_slug: None,
				blocked_model_slug: None,
				fallback_model_slugs: Vec::new(),
				dismissible: false,
				actions: vec![
					AccountRecoveryCta {
						action: AccountRecoveryAction::NotifyOwner,
						label: WireText::new("Notify").unwrap(),
					},
					AccountRecoveryCta {
						action: AccountRecoveryAction::RequestIncrease,
						label: WireText::new("Increase").unwrap(),
					},
				],
				request_url: None,
			})),
		}
	}
	fn prior(source: &AccountRecoveryResult, key: IdempotencyKey, outcome: S) -> R {
		R::Found(AccountRecoveryNudgeOperation {
			account_id: source.account_id.clone(),
			account_revision: source.account_revision,
			action: AccountRecoveryAction::NotifyOwner,
			operation_key: key,
			reserved_at_unix_micros: 100,
			outcome,
		})
	}
	#[test]
	fn first_attempt_identity_survives_observation_refresh_and_separates_purposes() {
		let mut source = source();
		let first =
			operation_key(&source, AccountRecoveryAction::NotifyOwner, &R::NotFound, None).unwrap();
		source.observed_at_unix_micros = Some(200);
		assert_eq!(
			operation_key(&source, AccountRecoveryAction::NotifyOwner, &R::NotFound, None),
			Some(first.clone())
		);
		assert_ne!(
			operation_key(&source, AccountRecoveryAction::RequestIncrease, &R::NotFound, None),
			Some(first.clone())
		);
		source.account_revision = EntityRevision(2);
		assert_ne!(
			operation_key(&source, AccountRecoveryAction::NotifyOwner, &R::NotFound, None),
			Some(first)
		);
	}
	#[test]
	fn uncertain_attempt_needs_exact_explicit_acknowledgement_and_unknown_state_never_sends() {
		let source = source();
		let key = IdempotencyKey::new("previous").unwrap();
		let prior = prior(&source, key.clone(), S::Uncertain);
		assert!(operation_key(&source, AccountRecoveryAction::NotifyOwner, &prior, None).is_none());
		let next =
			operation_key(&source, AccountRecoveryAction::NotifyOwner, &prior, Some(&key)).unwrap();
		assert_ne!(next, key);
		assert_eq!(
			operation_key(&source, AccountRecoveryAction::NotifyOwner, &prior, Some(&key)),
			Some(next)
		);
		assert!(
			operation_key(
				&source,
				AccountRecoveryAction::NotifyOwner,
				&prior,
				Some(&IdempotencyKey::new("older").unwrap())
			)
			.is_none()
		);
		assert!(
			operation_key(&source, AccountRecoveryAction::NotifyOwner, &R::Unavailable, Some(&key))
				.is_none()
		);
	}
	#[test]
	fn peer_completed_attempts_share_one_successor_identity_and_foreign_history_is_rejected() {
		let source = source();
		let prior = prior(&source, IdempotencyKey::new("completed").unwrap(), S::Sent);
		let a = operation_key(&source, AccountRecoveryAction::NotifyOwner, &prior, None).unwrap();
		let b = operation_key(&source, AccountRecoveryAction::NotifyOwner, &prior, None).unwrap();
		assert_eq!(a, b);
		let mut foreign = prior.clone();
		let R::Found(operation) = &mut foreign else { unreachable!() };
		operation.account_id = EntityId::new("10000000-0000-4000-8000-000000000002").unwrap();
		assert!(
			operation_key(&source, AccountRecoveryAction::NotifyOwner, &foreign, None).is_none()
		);
		assert!(
			operation_key(&source, AccountRecoveryAction::RequestIncrease, &prior, None).is_none()
		);
	}
}
