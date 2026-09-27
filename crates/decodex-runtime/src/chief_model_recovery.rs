//! Automatic ordinary fallback uses the existing model journal and native settings owner.
use crate::{ChiefError, chief_usage_estimate::Source};
use decodex_codex::app_server_client::{
	NativeRecoveryAuth, NativeTaskModelSettings, ServerEvent, ThreadModelRecoveryUpdate,
};
use decodex_database::{ChiefModelAttempt, ChiefModelRecoveryContext, SqliteStore};
use decodex_protocol::{
	AccountRecoveryResult, ChiefCapabilitiesResult, ChiefModelDto, EntityId, EntityRevision,
};
use sha2::{Digest as _, Sha256};

/// Evaluate one exact source. The Chief actor owns scheduling and drains native events.
pub(crate) async fn recover_ordinary_model<F, Fut, B, BFut>(
	store: &SqliteStore,
	source: F,
	banner: B,
	events: &tokio::sync::mpsc::Receiver<ServerEvent>,
) -> Result<(), ChiefError>
where
	F: Fn() -> Fut,
	Fut: std::future::Future<Output = Option<Source>>,
	B: Fn(EntityId, EntityRevision) -> BFut,
	BFut: std::future::Future<Output = AccountRecoveryResult>,
{
	if !events.is_empty() {
		return Ok(());
	}
	let Some(before) = source().await else {
		return Ok(());
	};
	let thread = &before.key.thread;
	let Ok(account) = EntityId::new(before.key.account.as_str()) else {
		return Ok(());
	};
	let Ok(revision) = u64::try_from(before.key.revision) else {
		return Ok(());
	};
	let recovery = banner(account.clone(), EntityRevision(revision)).await;
	if !matches!(&recovery.state, decodex_protocol::AccountRecoveryState::Current(b) if !b.fallback_model_slugs.is_empty())
	{
		return Ok(());
	}
	let Some((native, guard)) = before.client.configured_task_models(thread) else {
		return Ok(());
	};
	super::persist_current(
		store,
		&before.client,
		thread,
		Some(before.key.generation.as_str().into()),
	)
	.await?;
	let Some(observed) = store
		.chief_task_models(
			before.key.work.clone(),
			thread.clone(),
			Some(before.key.generation.as_str().into()),
		)
		.await?
	else {
		return Ok(());
	};
	let Some(settings) = observed
		.settings_json
		.as_deref()
		.and_then(|value| serde_json::from_str::<NativeTaskModelSettings>(value).ok())
	else {
		return Ok(());
	};
	if settings != native || !guard.is_live() {
		return Ok(());
	}
	if before.client.native_recovery_auth(guard.clone()).await? != NativeRecoveryAuth::ChatGpt {
		return Ok(());
	}
	let ChiefCapabilitiesResult::Available { models, .. } =
		crate::chief_capabilities::read(&before.client).await
	else {
		return Ok(());
	};
	let Some(target) = recovery.ordinary_fallback_model(
		&account,
		EntityRevision(revision),
		&settings.model,
		&models,
	) else {
		return Ok(());
	};
	let fast =
		crate::chief_capabilities::feature_enabled(&before.client, "fast_mode", Some(thread)).await;
	let Some((update, attempt)) =
		prepare_recovery(&before, observed.id, settings, target, fast, &recovery)?
	else {
		return Ok(());
	};
	if !recovery_source_ready(&guard, events)
		|| source().await.is_none_or(|after| after.key != before.key)
		|| banner(account.clone(), EntityRevision(revision)).await != recovery
	{
		return Ok(());
	}
	let Some(event) = store.reserve_chief_model_selection(attempt.clone()).await? else {
		return Ok(());
	};
	if !recovery_source_ready(&guard, events)
		|| source().await.is_none_or(|after| after.key != before.key)
		|| banner(account, EntityRevision(revision)).await != recovery
	{
		store.finish_chief_model_selection(event, attempt, "rejected".into()).await?;
		return Ok(());
	}
	let state = match before.client.queue_thread_model_recovery(&update, guard).await {
		Ok(_) => "queued",
		Err(
			decodex_codex::app_server_client::ClientError::StaleHistory
			| decodex_codex::app_server_client::ClientError::RequestTooLarge
			| decodex_codex::app_server_client::ClientError::RequestQueueFull,
		) => "rejected",
		Err(decodex_codex::app_server_client::ClientError::Remote(ref error))
			if matches!(error.code, -32602..=-32600) =>
			"rejected",
		Err(_) => "unknown",
	};
	store.finish_chief_model_selection(event, attempt, state.into()).await?;
	Ok(())
}

fn prepare_recovery(
	source: &Source,
	settings_event: i64,
	settings: NativeTaskModelSettings,
	target: &ChiefModelDto,
	fast: Option<bool>,
	recovery: &AccountRecoveryResult,
) -> Result<Option<(ThreadModelRecoveryUpdate, ChiefModelAttempt)>, ChiefError> {
	let thread = &source.key.thread;
	let Some((effort, tier)) = target_settings(&settings, target, fast) else {
		return Ok(None);
	};
	let update = match tier.as_deref() {
		Some(tier) => ThreadModelRecoveryUpdate::new(thread, target.model.as_str(), &effort, tier)?,
		None => ThreadModelRecoveryUpdate::preserving_service_tier(
			thread,
			target.model.as_str(),
			&effort,
		)?,
	};
	let digest: String =
		Sha256::digest(serde_json::to_vec(&recovery.state).expect("serializable banner"))
			.iter()
			.map(|b| format!("{b:02x}"))
			.collect();
	let attempt = ChiefModelAttempt {
		work: source.key.work.clone(),
		thread: thread.clone(),
		generation: Some(source.key.generation.as_str().into()),
		settings_event,
		model: target.model.as_str().into(),
		model_provider: settings.model_provider,
		effort: Some(effort),
		review_token: digest.clone(),
		attempt_id: format!("fallback:{digest}"),
		recovery: Some(ChiefModelRecoveryContext {
			account: source.key.account.as_str().into(),
			account_revision: source.key.revision,
			banner_digest: digest,
			from_model: settings.model,
			service_tier: tier.or(settings.service_tier),
		}),
	};
	Ok(Some((update, attempt)))
}

// A guard captured after a notification is live even while the actor's journal is stale.
// Native metadata replies are read after earlier notifications have entered this queue.
fn recovery_source_ready(
	guard: &decodex_codex::app_server_client::HistoryGuard,
	events: &tokio::sync::mpsc::Receiver<decodex_codex::app_server_client::ServerEvent>,
) -> bool {
	guard.is_live() && events.is_empty()
}

fn target_settings(
	current: &NativeTaskModelSettings,
	target: &ChiefModelDto,
	fast: Option<bool>,
) -> Option<(String, Option<String>)> {
	let effort = current
		.effort
		.as_deref()
		.and_then(|e| target.efforts.iter().find(|v| v.as_str() == e).cloned())
		.or_else(|| target.default_effort.clone())?;
	let tier = if current.service_tier.as_deref() == Some("flex") {
		Some("flex".into())
	} else {
		match fast? {
			false => None,
			true => Some(match current.service_tier.as_deref() {
				Some("default") => "default".into(),
				Some(tier) if target.service_tiers.iter().any(|t| t.id.as_str() == tier) =>
					tier.into(),
				Some(_) => "default".into(),
				None => target
					.default_service_tier
					.as_ref()
					.filter(|t| target.service_tiers.iter().any(|v| &v.id == *t))
					.map(|t| t.as_str().to_owned())
					.unwrap_or_else(|| "default".into()),
			}),
		}
	};
	Some((effort.as_str().into(), tier))
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::ConversationReasoningEffort as Effort;

	fn target() -> ChiefModelDto {
		ChiefModelDto {
			model: decodex_protocol::ConversationModel::new("target").unwrap(),
			name: "Target".into(),
			efforts: vec![Effort::Low, Effort::Medium],
			default_effort: Some(Effort::Medium),
			supports_fast: false,
			service_tiers: vec![],
			default_service_tier: None,
			available_cyber_programs: None,
			supports_images: false,
			availability: None,
			upgrade: None,
		}
	}
	fn current(effort: Option<&str>, tier: Option<&str>) -> NativeTaskModelSettings {
		NativeTaskModelSettings {
			model: "blocked".into(),
			model_provider: "openai".into(),
			effort: effort.map(str::to_owned),
			service_tier: tier.map(str::to_owned),
		}
	}
	#[test]
	fn fallback_preserves_supported_effort_and_explicit_standard_or_flex() {
		for tier in ["default", "flex"] {
			for fast in [Some(false), Some(true)] {
				assert_eq!(
					target_settings(&current(Some("low"), Some(tier)), &target(), fast),
					Some((
						"low".into(),
						if tier == "flex" || fast == Some(true) { Some(tier.into()) } else { None }
					))
				);
			}
		}
		for effort in [None, Some("high"), Some("future")] {
			assert_eq!(
				target_settings(&current(effort, Some("default")), &target(), Some(true)),
				Some(("medium".into(), Some("default".into())))
			);
		}
		let mut unknown = target();
		unknown.default_effort = None;
		assert!(
			target_settings(&current(Some("future"), Some("default")), &unknown, Some(true))
				.is_none()
		);
	}
	#[test]
	fn fallback_resolves_advertised_tier_without_inventing_unknown_feature_support() {
		let mut model = target();
		model.service_tiers.push(decodex_protocol::ChiefServiceTierDto {
			id: decodex_core::ServiceTier::new("priority").unwrap(),
			name: "Fast".into(),
			description: String::new(),
		});
		model.default_service_tier = Some(decodex_core::ServiceTier::new("priority").unwrap());
		assert_eq!(
			target_settings(&current(None, None), &model, Some(true)),
			Some(("medium".into(), Some("priority".into())))
		);
		assert_eq!(
			target_settings(&current(None, Some("withdrawn")), &model, Some(true)),
			Some(("medium".into(), Some("default".into())))
		);
		assert_eq!(
			target_settings(&current(None, None), &model, Some(false)),
			Some(("medium".into(), None))
		);
		assert_eq!(
			target_settings(&current(None, Some("withdrawn")), &model, Some(false)),
			Some(("medium".into(), None))
		);
		model.service_tiers.clear();
		assert_eq!(
			target_settings(&current(None, None), &model, Some(true)),
			Some(("medium".into(), Some("default".into())))
		);
		assert!(target_settings(&current(None, None), &model, None).is_none());
		assert!(target_settings(&current(None, Some("withdrawn")), &model, None).is_none());
	}
	#[tokio::test]
	async fn queued_native_settings_block_a_guard_captured_after_notification() {
		use decodex_codex::app_server_client::AppServerClient;
		use serde_json::{Value, json};
		use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
		let (local, remote) = tokio::io::duplex(4096);
		let (reader, writer) = tokio::io::split(local);
		let (client, mut events) = AppServerClient::from_io(reader, writer);
		let server = tokio::spawn(async move {
			let (reader, mut writer) = tokio::io::split(remote);
			let mut lines = BufReader::new(reader).lines();
			for index in 0..2 {
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "getAuthStatus");
				if index == 0 {
					let event = json!({"method":"thread/settings/updated","params":{"threadId":"thread","threadSettings":{"model":"manual-choice"}}});
					writer.write_all(format!("{event}\n").as_bytes()).await.unwrap();
				}
				let reply = json!({"id":request["id"],"result":{"authMethod":"chatgpt","requiresOpenaiAuth":true}});
				writer.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
			}
			assert!(
				tokio::time::timeout(std::time::Duration::from_millis(25), lines.next_line())
					.await
					.is_err()
			);
		});
		// The transport has published a settings event, but the Chief reducer has not consumed it.
		client.native_recovery_auth(client.history_guard(0).unwrap()).await.unwrap();
		let guard = client.thread_settings_guard("thread").unwrap();
		assert_eq!(
			client.native_recovery_auth(guard.clone()).await.unwrap(),
			NativeRecoveryAuth::ChatGpt
		);
		assert!(guard.is_live(), "a fresh guard alone cannot prove the journal is current");
		assert!(!recovery_source_ready(&guard, &events));
		let event = events.recv().await.unwrap();
		assert!(
			matches!(event, decodex_codex::app_server_client::ServerEvent::Notification { method, .. } if method == "thread/settings/updated")
		);
		assert!(recovery_source_ready(&guard, &events));
		server.await.unwrap();
		client.close();
	}
}
