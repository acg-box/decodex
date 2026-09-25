//! Short-lived, selected-account native notification using existing attested control ownership.
use super::{
	AccountBinding, AccountId, AttestedAppServerLaunch, ConversationCredentialVault,
	ConversationRefreshCallback, ConversationRuntime, ProcessAccountRefreshCallback,
	ProcessGenerationId, SelectedWorkingDirectory, derived_uuid,
};
use crate::account_observation::AccountObservationService;
use decodex_codex::app_server_client::{AccountNudgeCreditType, AccountNudgeOutcome, ServerEvent};
use decodex_protocol::{
	AccountRecoveryAction, AccountRecoveryDestination, AccountRecoveryNudgeStatus as Status,
	AccountRecoveryPreparation, AccountRecoveryResult,
};
use std::{sync::Arc, time::Duration};

impl ConversationRuntime {
	pub(crate) async fn send_account_recovery_nudge(
		&self,
		source: AccountRecoveryResult,
		action: AccountRecoveryAction,
		key: String,
		observations: AccountObservationService,
	) -> Status {
		let Ok(account) = AccountId::new(source.account_id.as_str()) else {
			return Status::Unavailable;
		};
		let Ok(revision) = i64::try_from(source.account_revision.0) else {
			return Status::Unavailable;
		};
		if self.is_shutting_down()
			|| !self
				.inner
				.store
				.account_is_ready_at_revision(&account, revision)
				.await
				.unwrap_or(false)
		{
			return Status::Unavailable;
		}
		let Ok(credential) = self.inner.accounts.process_credential(&account, revision).await
		else {
			return Status::Unavailable;
		};
		let Ok(generation_id) = ProcessGenerationId::new(derived_uuid(
			"account-nudge-process",
			&[&key, account.as_str()],
		)) else {
			return Status::Unavailable;
		};
		let runtime = tokio::runtime::Handle::current();
		let callback: Arc<dyn ProcessAccountRefreshCallback> =
			Arc::new(ConversationRefreshCallback {
				accounts: self.inner.accounts.clone(),
				runtime: runtime.clone(),
				generation_id,
			});
		let owner = self.clone();
		tokio::task::spawn_blocking(move || {
			let directory = owner.inner.launch_profile.control_working_directory();
			let Some(directory_text) = directory.to_str() else { return Status::Unavailable; };
			let Ok(selected) = SelectedWorkingDirectory::acquire(directory_text) else { return Status::Unavailable; };
			let Ok(binding) = AccountBinding::shared_home_bound(account.clone(), credential.binding, callback) else { return Status::Unavailable; };
			let vault = ConversationCredentialVault { account_id: account.clone(), stored: credential.stored };
			let Ok(permit) = owner.inner.capacity.reserve(account.clone(), revision) else { return Status::Unavailable; };
			let Ok(launch) = AttestedAppServerLaunch::bind_selected_control_working_directory(owner.inner.launch_profile.clone(), directory, binding, Duration::from_secs(8), permit, Arc::new(selected)) else { return Status::Unavailable; };
			let Ok(mut child) = launch.spawn() else { return Status::Unavailable; };
			let initialized = child.initialize_ordinary_turns(&vault);
			drop(credential.launch_guard);
			let result = if initialized.is_ok() && !owner.is_shutting_down() {
				match child.retain_account_control_connection() {
					Ok((client, mut events)) => runtime.block_on(async {
						let prepared = observations.prepare_recovery(&source, action).await;
						let purpose = match prepared {
							AccountRecoveryPreparation::Ready { destination: AccountRecoveryDestination::RequestCredits, .. } => Some(AccountNudgeCreditType::Credits),
							AccountRecoveryPreparation::Ready { destination: AccountRecoveryDestination::RequestUsageIncrease, .. } => Some(AccountNudgeCreditType::UsageLimit),
							_ => None,
						};
						let Some(purpose) = purpose else { client.close(); return Status::Unavailable; };
						let sending = client.send_account_nudge(purpose);
						tokio::pin!(sending);
						let outcome = loop {
							tokio::select! {
								outcome = &mut sending => break outcome,
								event = events.recv() => if !matches!(event, Some(ServerEvent::Notification { .. })) { break AccountNudgeOutcome::Uncertain; },
							}
						};
						client.close();
						match outcome { AccountNudgeOutcome::Sent => Status::Sent, AccountNudgeOutcome::CooldownActive => Status::CooldownActive, AccountNudgeOutcome::Unsupported => Status::Unsupported, AccountNudgeOutcome::Uncertain => Status::Uncertain }
					}),
					Err(_) => Status::Unavailable,
				}
			} else { Status::Unavailable };
			// Shutdown transfers unproved cleanup to the existing owned reaper. Do not
			// erase a confirmed notification result because process cleanup is delayed.
			let _ = child.shutdown();
			result
		}).await.unwrap_or(Status::Uncertain)
	}
}
