//! Short-lived, selected-account native notification using existing attested control ownership.
use std::{sync::Arc, time::Duration};

use tokio::{runtime::Handle, task};

use crate::{
	account_observation::AccountObservationService,
	conversation::{
		self, AccountBinding, AccountId, AccountRefreshCallback, AttestedAppServerLaunch,
		ConversationCredentialVault, ConversationRefreshCallback, ConversationRuntime,
		ProcessGenerationId, SelectedWorkingDirectory,
	},
};
use decodex_codex::app_server_client::{AccountNudgeCreditType, AccountNudgeOutcome, ServerEvent};
use decodex_protocol::{
	AccountRecoveryAction, AccountRecoveryDestination, AccountRecoveryPreparation,
	AccountRecoveryResult,
};

impl ConversationRuntime {
	pub(crate) async fn send_account_recovery_nudge(
		&self,
		source: AccountRecoveryResult,
		action: AccountRecoveryAction,
		key: String,
		observations: AccountObservationService,
	) -> decodex_protocol::AccountRecoveryNudgeStatus {
		let Ok(account) = AccountId::new(source.account_id.as_str()) else {
			return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable;
		};
		let Ok(revision) = i64::try_from(source.account_revision.0) else {
			return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable;
		};

		if self.is_shutting_down()
			|| !self
				.inner
				.store
				.account_is_ready_at_revision(&account, revision)
				.await
				.unwrap_or(false)
		{
			return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable;
		}

		let Ok(credential) = self.inner.accounts.process_credential(&account, revision).await
		else {
			return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable;
		};
		let Ok(generation_id) = ProcessGenerationId::new(conversation::derived_uuid(
			"account-nudge-process",
			&[&key, account.as_str()],
		)) else {
			return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable;
		};
		let runtime = Handle::current();
		let callback: Arc<dyn AccountRefreshCallback> = Arc::new(ConversationRefreshCallback {
			last_projected: std::sync::Mutex::new(None),
			accounts: self.inner.accounts.clone(),
			runtime: runtime.clone(),
			generation_id,
		});
		let owner = self.clone();

		task::spawn_blocking(move || {
			let directory = owner.inner.launch_profile.control_working_directory();
			let Some(directory_text) = directory.to_str() else { return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable; };
			let Ok(selected) = SelectedWorkingDirectory::acquire(directory_text) else { return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable; };
			let Ok(binding) = AccountBinding::shared_home_bound(account.clone(), credential.binding, callback).and_then(|binding| binding.with_credential(&credential.stored)) else { return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable; };
			let vault = ConversationCredentialVault { account_id: account.clone(), stored: credential.stored };
			let Ok(permit) = owner.inner.capacity.reserve(account.clone(), revision) else { return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable; };
			let Ok(launch) = AttestedAppServerLaunch::bind_selected_control_working_directory(owner.inner.launch_profile.clone(), directory, binding, Duration::from_secs(8), permit, Arc::new(selected)) else { return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable; };
			let Ok(mut child) = launch.spawn() else { return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable; };
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
						let Some(purpose) = purpose else { client.close(); return decodex_protocol::AccountRecoveryNudgeStatus::Unavailable; };
						let sending = client.send_account_nudge(purpose);

						tokio::pin!(sending);

						let outcome = loop {
							tokio::select! {
								outcome = &mut sending => break outcome,
								event = events.recv() => if !matches!(event, Some(ServerEvent::Notification { .. })) { break AccountNudgeOutcome::Uncertain; },
							}
						};

						client.close();

						match outcome { AccountNudgeOutcome::Sent => decodex_protocol::AccountRecoveryNudgeStatus::Sent, AccountNudgeOutcome::CooldownActive => decodex_protocol::AccountRecoveryNudgeStatus::CooldownActive, AccountNudgeOutcome::Unsupported => decodex_protocol::AccountRecoveryNudgeStatus::Unsupported, AccountNudgeOutcome::Uncertain => decodex_protocol::AccountRecoveryNudgeStatus::Uncertain }
					}),
					Err(_) => decodex_protocol::AccountRecoveryNudgeStatus::Unavailable,
				}
			} else { decodex_protocol::AccountRecoveryNudgeStatus::Unavailable };
			// Shutdown transfers unproved cleanup to the existing owned reaper. Do not
			// erase a confirmed notification result because process cleanup is delayed.
			let _ = child.shutdown();

			result
		}).await.unwrap_or(decodex_protocol::AccountRecoveryNudgeStatus::Uncertain)
	}
}
