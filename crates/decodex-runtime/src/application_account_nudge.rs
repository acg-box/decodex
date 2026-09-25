//! Durable one-attempt account notification boundary.
#[cfg(all(test, target_os = "macos"))]
#[path = "application_account_nudge_native_tests.rs"]
mod native_tests;
use super::{ApplicationPublication, ProductStore, ServiceApplication, application_unavailable};
use decodex_database::{AccountCommandKind, AccountCommandReceiptClaim, CommandIdentity};
use decodex_protocol::{
	AccountRecoveryAction, AccountRecoveryNudgeStatus as Status, AccountRecoveryResult, Channel,
	CommandEnvelope, CommandError, EventPayload, ResultPayload,
};
impl ServiceApplication {
	pub(super) async fn execute_recovery_nudge(
		&self,
		command: &CommandEnvelope,
		source: &AccountRecoveryResult,
		action: AccountRecoveryAction,
	) -> Result<ApplicationPublication, CommandError> {
		if command.expected_revision != Some(source.account_revision)
			|| !source.allows_nudge(action)
		{
			return Err(application_unavailable("account notification source changed"));
		}
		let ProductStore::Available(store) = &self.store else {
			return Err(application_unavailable("account state unavailable"));
		};
		let request = serde_json::to_vec(&command.payload)
			.map_err(|_| application_unavailable("invalid account notification"))?;
		let identity = CommandIdentity::new(command.idempotency_key.as_str(), &request)
			.map_err(|_| application_unavailable("invalid account notification identity"))?;
		let revision = i64::try_from(source.account_revision.0)
			.map_err(|_| application_unavailable("invalid account revision"))?;
		let claim = store
			.reserve_account_command(
				&identity,
				nudge_kind(action),
				source.account_id.as_str(),
				Some(revision),
			)
			.await
			.map_err(|_| application_unavailable("account notification could not be reserved"))?;
		let status = match claim {
			AccountCommandReceiptClaim::Pending(_) => Status::Uncertain,
			AccountCommandReceiptClaim::Replayed(value) => serde_json::from_value(
				value.get("status").cloned().unwrap_or_default(),
			)
			.map_err(|_| application_unavailable("account notification result unavailable"))?,
			AccountCommandReceiptClaim::Owned(lease) => {
				let status = match (self.conversations.runtime(), self.account_observations.clone())
				{
					(Some(runtime), Some(observations)) =>
						runtime
							.send_account_recovery_nudge(
								source.clone(),
								action,
								command.idempotency_key.as_str().to_owned(),
								observations,
							)
							.await,
					_ => Status::Unavailable,
				};
				store
					.complete_account_command(lease, &serde_json::json!({"status":status}))
					.await
					.map_err(|_| CommandError::AcceptanceUnknown)?;
				status
			},
		};
		Ok(ApplicationPublication {
			channel: Channel::AccountsHealth,
			entity_id: source.account_id.clone(),
			entity_revision: source.account_revision,
			result: ResultPayload::AccountRecoveryNudge {
				account_id: source.account_id.clone(),
				operation_key: command.idempotency_key.clone(),
				status,
			},
			event: EventPayload::AccountRecoveryNudge {
				account_id: source.account_id.clone(),
				operation_key: command.idempotency_key.clone(),
				status,
			},
		})
	}
}

fn nudge_kind(action: AccountRecoveryAction) -> AccountCommandKind {
	if action == AccountRecoveryAction::RequestIncrease {
		AccountCommandKind::RequestWorkspaceUsageIncrease
	} else {
		AccountCommandKind::NotifyWorkspaceOwner
	}
}
impl ServiceApplication {
	pub(super) async fn account_nudge_status(
		&self,
		account_id: &decodex_protocol::EntityId,
		action: AccountRecoveryAction,
		key: Option<&decodex_protocol::IdempotencyKey>,
	) -> decodex_protocol::QueryResultPayload {
		use decodex_protocol::{AccountRecoveryNudgeResult as R, QueryResultPayload};
		let result = async {
			let ProductStore::Available(store) = &self.store else {
				return R::Unavailable;
			};
			let Ok(account) = decodex_core::AccountId::new(account_id.as_str()) else {
				return R::Unavailable;
			};
			let receipt = match store
				.read_account_nudge_receipt(&account, nudge_kind(action), key.map(|k| k.as_str()))
				.await
			{
				Ok(Some(receipt)) => receipt,
				Ok(None) => return R::NotFound,
				Err(_) => return R::Unavailable,
			};
			let (Ok(operation_key), Ok(revision)) = (
				decodex_protocol::IdempotencyKey::new(receipt.operation_key),
				u64::try_from(receipt.account_revision),
			) else {
				return R::Unavailable;
			};
			let outcome = match receipt.response {
				None => Status::Uncertain,
				Some(value) =>
					match serde_json::from_value(value.get("status").cloned().unwrap_or_default()) {
						Ok(status) => status,
						Err(_) => return R::Unavailable,
					},
			};
			R::Found(decodex_protocol::AccountRecoveryNudgeOperation {
				account_id: account_id.clone(),
				account_revision: decodex_protocol::EntityRevision(revision),
				action,
				operation_key,
				reserved_at_unix_micros: receipt.reserved_at_unix_micros,
				outcome,
			})
		}
		.await;
		QueryResultPayload::AccountRecoveryNudge(result)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::{
		AccountRecoveryBanner, AccountRecoveryCta, AccountRecoveryState, CURRENT_VERSION,
		ClientCommandId, CommandPayload, CorrelationId, DoctorCheck, DoctorComponent, DoctorIssue,
		DoctorReport, DoctorStatus, EntityId, EntityRevision, IdempotencyKey, ServerId, WireText,
	};
	pub(super) fn application(store: decodex_database::SqliteStore) -> ServiceApplication {
		let doctor = DoctorReport::new(
			ServerId::new("20000000-0000-4000-8000-000000000001").unwrap(),
			CURRENT_VERSION,
			DoctorComponent::ALL
				.into_iter()
				.map(|component| {
					DoctorCheck::new(component, DoctorStatus::Unavailable(DoctorIssue::NotProbed))
				})
				.collect(),
		)
		.unwrap();
		ServiceApplication::new(
			ProductStore::Available(store),
			None,
			None,
			decodex_codex::CodexAdapter::unavailable(),
			None,
			crate::conversation::ConversationCapability::Unavailable(
				decodex_protocol::ConversationUnavailableReason::AppServerProfile,
			),
			doctor,
		)
	}
	#[tokio::test]
	async fn unavailable_notification_is_durable_and_a_conflicting_retry_is_refused() {
		let directory = tempfile::tempdir().unwrap();
		let root =
			decodex_core::DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = decodex_database::SqliteStore::open(&root.paths()).unwrap();
		let app = application(store.clone());
		let source = AccountRecoveryResult {
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
				actions: vec![AccountRecoveryCta {
					action: AccountRecoveryAction::NotifyOwner,
					label: WireText::new("Notify owner").unwrap(),
				}],
				request_url: None,
			})),
		};
		let mut command = CommandEnvelope {
			version: CURRENT_VERSION,
			client_command_id: ClientCommandId::new("nudge-attempt").unwrap(),
			idempotency_key: IdempotencyKey::new("nudge-key").unwrap(),
			expected_revision: Some(EntityRevision(1)),
			correlation_id: CorrelationId::new("nudge-correlation").unwrap(),
			causation_id: None,
			payload: CommandPayload::SendAccountRecoveryNudge {
				source: Box::new(source.clone()),
				action: AccountRecoveryAction::NotifyOwner,
			},
		};
		let result = app
			.execute_recovery_nudge(&command, &source, AccountRecoveryAction::NotifyOwner)
			.await
			.unwrap();
		assert!(matches!(
			result.result,
			ResultPayload::AccountRecoveryNudge { status: Status::Unavailable, .. }
		));
		drop(app);
		drop(store);
		let app = application(decodex_database::SqliteStore::open(&root.paths()).unwrap());
		let result = app
			.execute_recovery_nudge(&command, &source, AccountRecoveryAction::NotifyOwner)
			.await
			.unwrap();
		assert!(matches!(
			result.result,
			ResultPayload::AccountRecoveryNudge { status: Status::Unavailable, .. }
		));
		let status = app
			.account_nudge_status(
				&source.account_id,
				AccountRecoveryAction::NotifyOwner,
				Some(&command.idempotency_key),
			)
			.await;
		assert!(
			matches!(status, decodex_protocol::QueryResultPayload::AccountRecoveryNudge(decodex_protocol::AccountRecoveryNudgeResult::Found(operation)) if operation.outcome == Status::Unavailable && operation.operation_key == command.idempotency_key)
		);
		let other_purpose = app
			.account_nudge_status(&source.account_id, AccountRecoveryAction::RequestIncrease, None)
			.await;
		assert!(matches!(
			other_purpose,
			decodex_protocol::QueryResultPayload::AccountRecoveryNudge(
				decodex_protocol::AccountRecoveryNudgeResult::NotFound
			)
		));
		command.expected_revision = Some(EntityRevision(2));
		assert!(
			app.execute_recovery_nudge(&command, &source, AccountRecoveryAction::NotifyOwner)
				.await
				.is_err()
		);
		command.expected_revision = Some(EntityRevision(1));
		let mut changed = source.clone();
		changed.observed_at_unix_micros = Some(101);
		command.payload = CommandPayload::SendAccountRecoveryNudge {
			source: Box::new(changed.clone()),
			action: AccountRecoveryAction::NotifyOwner,
		};
		assert!(
			app.execute_recovery_nudge(&command, &changed, AccountRecoveryAction::NotifyOwner)
				.await
				.is_err()
		);
	}
}
