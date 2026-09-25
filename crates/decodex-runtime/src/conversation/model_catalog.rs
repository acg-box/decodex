//! One bounded account-bound metadata process, with the same owner as thread controls.
use super::{
	AccountBinding, AccountId, AccountProcessCredential, AttestedAppServerLaunch,
	AttestedProcessChild, ConversationCredentialVault, ConversationRefreshCallback,
	ConversationRuntime, ProcessAccountRefreshCallback, ProcessGenerationId,
	SelectedWorkingDirectory, derived_uuid,
};
use decodex_protocol::{
	ChiefModelDto, EntityId, InitialExecutionDefaults, InitialModelCatalogRequest,
	InitialModelCatalogResult, InitialModelDefaults, ModelCatalogPurpose,
};
use std::{
	sync::Arc,
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

impl ConversationRuntime {
	pub(crate) async fn initial_model_catalog(
		&self,
		key: &str,
		request: InitialModelCatalogRequest,
	) -> InitialModelCatalogResult {
		let Ok(permit) = self.inner.initial_catalog.clone().try_lock_owned() else {
			return InitialModelCatalogResult::Unavailable;
		};
		let (reply, received) = tokio::sync::oneshot::channel();
		let mut workers = self.inner.workers.lock().await;
		if self.is_shutting_down() {
			return InitialModelCatalogResult::Unavailable;
		}
		while workers.try_join_next().is_some() {}
		let runtime = self.clone();
		let key = key.to_owned();
		// The runtime owns completion and cleanup even when the requesting client leaves.
		workers.spawn(async move {
			let _permit = permit;
			let result = runtime.discover_initial_models(&key, request).await;
			let _ = reply.send(result.unwrap_or(InitialModelCatalogResult::Unavailable));
		});
		drop(workers);
		tokio::time::timeout(Duration::from_secs(35), received)
			.await
			.ok()
			.and_then(Result::ok)
			.unwrap_or(InitialModelCatalogResult::Unavailable)
	}

	async fn discover_initial_models(
		&self,
		key: &str,
		request: InitialModelCatalogRequest,
	) -> Option<InitialModelCatalogResult> {
		let preferred =
			request.account_id.as_ref().map(|id| AccountId::new(id.as_str())).transpose().ok()?;
		let now =
			i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_micros()).ok()?;
		let selected = match request.purpose {
			ModelCatalogPurpose::Chief =>
				self.select_chief_account(preferred.as_ref(), now).await.ok()?,
			ModelCatalogPurpose::Conversation if preferred.is_none() =>
				self.inner.accounts.select_initial(now).await.ok()?,
			ModelCatalogPurpose::Conversation => return None,
		};
		let account = selected.account.account_id;
		let revision = selected.account.revision;
		let credential = tokio::time::timeout(
			Duration::from_secs(10),
			self.inner.accounts.process_credential(&account, revision),
		)
		.await
		.ok()?
		.ok()?;
		if self.is_shutting_down() {
			return None;
		}
		let runtime = self.clone();
		let callback: Arc<dyn ProcessAccountRefreshCallback> =
			Arc::new(ConversationRefreshCallback {
				accounts: self.inner.accounts.clone(),
				runtime: tokio::runtime::Handle::current(),
				generation_id: ProcessGenerationId::new(derived_uuid(
					"model-catalog-process",
					&[key, account.as_str()],
				))
				.ok()?,
			});
		let source = account.clone();
		let directory = request.working_directory.as_str().to_owned();
		let (models, defaults) = tokio::task::spawn_blocking(move || {
			runtime
				.read_initial_catalog_process(&source, revision, &directory, credential, callback)
		})
		.await
		.ok()??;
		if self.is_shutting_down()
			|| !self.inner.store.account_is_ready_at_revision(&account, revision).await.ok()?
		{
			return None;
		}
		let now =
			i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_micros()).ok()?;
		let current = match request.purpose {
			ModelCatalogPurpose::Chief =>
				self.select_chief_account(preferred.as_ref(), now).await.ok()?,
			ModelCatalogPurpose::Conversation =>
				self.inner.accounts.select_initial(now).await.ok()?,
		};
		if current.account.account_id != account || current.account.revision != revision {
			return None;
		}
		let result = InitialModelCatalogResult::Available {
			account_id: EntityId::new(account.as_str()).ok()?,
			account_revision: revision,
			working_directory: request.working_directory,
			models,
			defaults: Some(Box::new(defaults)),
		};
		// Leave room for the public envelope and transport framing.
		(serde_json::to_vec(&result).ok()?.len() <= 128 * 1024).then_some(result)
	}

	fn read_initial_catalog_process(
		&self,
		account: &AccountId,
		revision: i64,
		directory: &str,
		credential: AccountProcessCredential,
		callback: Arc<dyn ProcessAccountRefreshCallback>,
	) -> Option<(Vec<ChiefModelDto>, InitialModelDefaults)> {
		self.read_metadata_process(account, revision, directory, credential, callback, |child| {
			read_initial_defaults(child, directory, || self.is_shutting_down())
		})
	}

	pub(super) fn read_metadata_process<T>(
		&self,
		account: &AccountId,
		revision: i64,
		directory: &str,
		credential: AccountProcessCredential,
		callback: Arc<dyn ProcessAccountRefreshCallback>,
		read: impl FnOnce(&mut AttestedProcessChild) -> Option<T>,
	) -> Option<T> {
		let selected = Arc::new(SelectedWorkingDirectory::acquire(directory).ok()?);
		let binding =
			AccountBinding::shared_home_bound(account.clone(), credential.binding, callback)
				.ok()?;
		let vault =
			ConversationCredentialVault { account_id: account.clone(), stored: credential.stored };
		let permit = self.inner.capacity.reserve(account.clone(), revision).ok()?;
		if self.is_shutting_down() {
			return None;
		}
		let launch = AttestedAppServerLaunch::bind_selected_control_working_directory(
			self.inner.launch_profile.clone(),
			directory.into(),
			binding,
			Duration::from_secs(8),
			permit,
			selected.clone(),
		)
		.ok()?;
		let mut child = launch.spawn().ok()?;
		let initialized = child.initialize_ordinary_turns(&vault);
		drop(credential.launch_guard);
		let result = if initialized.is_ok() { read(&mut child) } else { None };
		// Cleanup failure cannot produce a successful catalog observation.
		child.shutdown().ok()?;
		selected.revalidate().ok()?;
		result
	}
}

#[cfg(test)]
fn read_catalog(
	child: &mut AttestedProcessChild,
	cancelled: impl Fn() -> bool,
) -> Option<Vec<ChiefModelDto>> {
	read_catalog_with_default(child, cancelled).map(|(models, _)| models)
}

fn read_catalog_with_default(
	child: &mut AttestedProcessChild,
	cancelled: impl Fn() -> bool,
) -> Option<(Vec<ChiefModelDto>, Option<decodex_protocol::ConversationModel>)> {
	let mut pages = crate::chief_capabilities::ModelCatalogPages::default();
	let mut cursor = None;
	let mut default_model = None;
	let deadline = Instant::now() + Duration::from_secs(8);
	for _ in 0..8 {
		if cancelled() || Instant::now() >= deadline {
			return None;
		}
		let (page, events) = child.read_ordinary_model_page(cursor.as_deref());
		child.retain_ordinary_events(events).ok()?;
		let page = page.ok()?;
		for model in page["data"].as_array()? {
			if model["isDefault"] == true {
				if default_model.is_some() {
					return None;
				}
				default_model =
					Some(decodex_protocol::ConversationModel::new(model["model"].as_str()?).ok()?);
			}
		}
		match pages.push(&page).ok()? {
			Some(next) => cursor = Some(next),
			None => return Some((pages.models, default_model)),
		}
	}
	None
}

fn project_native_defaults(
	value: decodex_codex::app_server_client::NativeExecutionDefaults,
) -> Option<InitialExecutionDefaults> {
	Some(InitialExecutionDefaults {
		model: value.model.map(decodex_protocol::ConversationModel::new).transpose().ok()?,
		reasoning_effort: value
			.reasoning_effort
			.map(decodex_protocol::ConversationReasoningEffort::new)
			.transpose()
			.ok()?,
		service_tier: value
			.service_tier
			.map(decodex_protocol::ServiceTier::new)
			.transpose()
			.ok()?,
	})
}

fn read_initial_defaults(
	child: &mut AttestedProcessChild,
	directory: &str,
	cancelled: impl Fn() -> bool,
) -> Option<(Vec<ChiefModelDto>, InitialModelDefaults)> {
	if cancelled() {
		return None;
	}
	let (configured, events) = child.read_ordinary_model_defaults(directory, false);
	child.retain_ordinary_events(events).ok()?;
	let configured = project_native_defaults(configured.ok()?)?;
	if cancelled() {
		return None;
	}
	let (managed, events) = child.read_ordinary_model_defaults(directory, true);
	child.retain_ordinary_events(events).ok()?;
	let managed = project_native_defaults(managed.ok()?)?;
	let (models, catalog_model) = read_catalog_with_default(child, cancelled)?;
	Some((models, InitialModelDefaults { configured, managed, catalog_model }))
}

#[cfg(test)]
mod tests {
	use super::read_catalog;
	use crate::account_launch::process::tests::ordinary_catalog_child;

	#[test]
	fn initial_defaults_preserve_distinct_sources_and_interleaved_events() {
		let (_temp, mut child) = ordinary_catalog_child("exact");
		let (models, defaults) =
			super::read_initial_defaults(&mut child, "/tmp", || false).expect("complete defaults");
		assert_eq!(models.len(), 1);
		assert_eq!(defaults.configured.model.as_ref().unwrap().as_str(), "configured-model");
		assert_eq!(defaults.managed.model.as_ref().unwrap().as_str(), "managed-model");
		assert_eq!(defaults.catalog_model.as_ref().unwrap().as_str(), "catalog-model");
		assert_eq!(defaults.configured.service_tier.as_ref().unwrap().as_str(), "flex");
		assert!(!serde_json::to_string(&defaults).unwrap().contains("not-public"));
		assert!(child.next_ordinary_turn_event(std::time::Duration::ZERO).unwrap().is_some());
		child.shutdown().unwrap();
		let (_temp, mut child) = ordinary_catalog_child("exact-defaults-rejected");
		assert!(super::read_initial_defaults(&mut child, "/tmp", || false).is_none());
		child.shutdown().unwrap();
	}

	#[test]
	fn metadata_catalog_preserves_tiers_and_interleaved_events() {
		let (_temp, mut child) = ordinary_catalog_child("exact");
		let models = read_catalog(&mut child, || false).expect("complete native catalog");
		assert_eq!(models.len(), 1);
		assert_eq!(models[0].model.as_str(), "catalog-model");
		assert_eq!(models[0].service_tiers[0].id.as_str(), "ultrafast");
		assert!(
			child
				.next_ordinary_turn_event(std::time::Duration::ZERO)
				.expect("buffered event")
				.is_some()
		);
		child.shutdown().expect("metadata process closes");
	}

	#[test]
	fn cancelled_or_rejected_catalog_never_returns_partial_models() {
		let (_temp, mut child) = ordinary_catalog_child("exact");
		assert!(read_catalog(&mut child, || true).is_none());
		assert!(
			child
				.next_ordinary_turn_event(std::time::Duration::ZERO)
				.expect("no query was sent")
				.is_none()
		);
		child.shutdown().expect("cancelled process closes");
		let (_temp, mut child) = ordinary_catalog_child("exact-catalog-rejected");
		assert!(read_catalog(&mut child, || false).is_none());
		child.shutdown().expect("rejected process closes");
	}
}
