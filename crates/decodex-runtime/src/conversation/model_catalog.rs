//! One bounded account-bound metadata process, with the same owner as thread controls.
use std::{
	sync::{Arc, Mutex},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use tokio::{runtime::Handle, sync::oneshot, task, time};

use crate::{
	agent::AgentConfig,
	agent_capabilities::ModelCatalogPages,
	conversation::{
		self, AccountBinding, AccountId, AccountProcessCredential, AccountRefreshCallback,
		AttestedAppServerLaunch, AttestedProcessChild, ConversationCredentialVault,
		ConversationRefreshCallback, ConversationRuntime, ProcessGenerationId,
		SelectedWorkingDirectory,
	},
	native_preparation::PreparationState,
};
use decodex_codex::app_server_client::NativeExecutionDefaults;
use decodex_protocol::{
	AgentModelDto, ConversationReasoningEffort, ConversationWorkingDirectory, EntityId,
	InitialExecutionDefaults, InitialModelCatalogRequest, InitialModelCatalogResult,
	InitialModelDefaults, ModelCatalogPurpose, ServiceTier,
};

pub(crate) struct StoredThread {
	pub(crate) account: AccountId,
	pub(crate) candidate: decodex_protocol::NativeThreadCandidate,
	pub(crate) row: serde_json::Value,
	pub(crate) config: Option<AgentConfig>,
}

impl ConversationRuntime {
	async fn thread_metadata<T: Send + 'static>(
		&self,
		read: impl FnOnce(&mut AttestedProcessChild, &str) -> Option<T> + Send + 'static,
	) -> Option<(EntityId, T)> {
		let directory = super::effective_user_home(unsafe { libc::geteuid() }).ok()?;
		let request = InitialModelCatalogRequest {
			working_directory: ConversationWorkingDirectory::new(directory.to_str()?).ok()?,
			purpose: ModelCatalogPurpose::Agent,
			account_id: None,
		};
		let _permit = self.inner.initial_catalog.clone().lock_owned().await;
		self.discover_initial_metadata("native-threads", request, read)
			.await
			.map(|(account, _, _, value)| (account, value))
	}

	pub(crate) async fn offline_thread_page(
		&self,
		cursor: Option<&str>,
	) -> Option<decodex_protocol::NativeThreadPage> {
		let params = crate::agent::registration::list_params(cursor);
		let (_, page) = self
			.thread_metadata(move |child, _| {
				crate::agent::registration::page(&child.catalog_request("thread/list", params)?)
			})
			.await?;
		Some(page)
	}

	pub(crate) async fn visible_stored_thread(
		&self,
		thread: &str,
		need_settings: bool,
	) -> Result<StoredThread, &'static str> {
		const UNAVAILABLE: &str = "This conversation is unavailable in the current account.";
		let thread = thread.to_owned();
		let (account, (candidate, row, config)) = self
			.thread_metadata(move |child, _| {
				let deadline = Instant::now() + Duration::from_secs(12);
				let mut seen = std::collections::HashSet::new();
				let mut cursor = None;
				while Instant::now() < deadline {
					let raw = child.catalog_request(
						"thread/list",
						crate::agent::registration::list_params(cursor.as_deref()),
					)?;
					let page = crate::agent::registration::page(&raw)?;
					if let Some(candidate) =
						page.threads.into_iter().find(|c| c.thread_id == thread)
					{
						let mut raw = child.catalog_request(
							"thread/read",
							serde_json::json!({"threadId":thread}),
						)?;
						let row = raw.get_mut("thread")?.take();
						if crate::agent::registration::candidate(&row)?.thread_id != thread {
							return None;
						}
						let config =
							need_settings.then(|| stored_thread_config(child, &row)).transpose();
						return Some((candidate, row, config));
					}
					let next = page.next_cursor?;
					if !seen.insert(next.clone()) {
						break;
					}
					cursor = Some(next);
				}
				None
			})
			.await
			.ok_or(UNAVAILABLE)?;
		Ok(StoredThread {
			account: AccountId::new(account.as_str()).map_err(|_| UNAVAILABLE)?,
			candidate,
			row,
			config: config?,
		})
	}

	pub(crate) async fn initial_model_catalog(
		&self,
		key: &str,
		request: InitialModelCatalogRequest,
	) -> InitialModelCatalogResult {
		let Ok(permit) = self.inner.initial_catalog.clone().try_lock_owned() else {
			return InitialModelCatalogResult::Unavailable;
		};
		let (reply, received) = oneshot::channel();
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

		time::timeout(Duration::from_secs(35), received)
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
		let runtime = self.clone();
		let (account_id, account_revision, working_directory, (models, defaults)) = self
			.discover_initial_metadata(key, request, move |child, directory| {
				read_initial_defaults(child, directory, || runtime.is_shutting_down())
			})
			.await?;
		let result = InitialModelCatalogResult::Available {
			account_id,
			account_revision,
			working_directory,
			models,
			defaults: Some(Box::new(defaults)),
		};

		(serde_json::to_vec(&result).ok()?.len() <= 128 * 1_024).then_some(result)
	}

	/// Share account selection, process lifetime and final source checks for initial catalogs.
	pub(super) async fn discover_initial_metadata<T>(
		&self,
		key: &str,
		request: InitialModelCatalogRequest,
		read: impl FnOnce(&mut AttestedProcessChild, &str) -> Option<T> + Send + 'static,
	) -> Option<(EntityId, i64, ConversationWorkingDirectory, T)>
	where
		T: Send + 'static,
	{
		self.inner.launch_profile.profile().await?;

		let preferred =
			request.account_id.as_ref().map(|id| AccountId::new(id.as_str())).transpose().ok()?;
		let now =
			i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_micros()).ok()?;
		let selected = match request.purpose {
			ModelCatalogPurpose::Agent =>
				self.select_agent_account(preferred.as_ref(), now).await.ok()?,
			ModelCatalogPurpose::Conversation if preferred.is_none() =>
				self.inner.accounts.select_initial(now).await.ok()?,
			ModelCatalogPurpose::Conversation => return None,
		};
		let account = selected.account.account_id;
		let revision = selected.account.revision;
		let credential = time::timeout(
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
		let callback: Arc<dyn AccountRefreshCallback> = Arc::new(ConversationRefreshCallback {
			last_projected: Mutex::new(None),
			accounts: self.inner.accounts.clone(),
			runtime: Handle::current(),
			generation_id: ProcessGenerationId::new(conversation::derived_uuid(
				"model-catalog-process",
				&[key, account.as_str()],
			))
			.ok()?,
		});
		let source = account.clone();
		let directory = request.working_directory.as_str().to_owned();
		let value = task::spawn_blocking(move || {
			runtime.read_metadata_process(
				&source,
				revision,
				&directory,
				credential,
				callback,
				|child| read(child, &directory),
			)
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
			ModelCatalogPurpose::Agent =>
				self.select_agent_account(preferred.as_ref(), now).await.ok()?,
			ModelCatalogPurpose::Conversation =>
				self.inner.accounts.select_initial(now).await.ok()?,
		};

		if current.account.account_id != account || current.account.revision != revision {
			return None;
		}

		Some((EntityId::new(account.as_str()).ok()?, revision, request.working_directory, value))
	}

	pub(super) fn read_metadata_process<T>(
		&self,
		account: &AccountId,
		revision: i64,
		directory: &str,
		credential: AccountProcessCredential,
		callback: Arc<dyn AccountRefreshCallback>,
		read: impl FnOnce(&mut AttestedProcessChild) -> Option<T>,
	) -> Option<T> {
		let PreparationState::Ready(profile) = self.inner.launch_profile.state() else {
			return None;
		};
		let selected = Arc::new(SelectedWorkingDirectory::acquire(directory).ok()?);
		let binding =
			AccountBinding::shared_home_bound(account.clone(), credential.binding, callback)
				.and_then(|binding| binding.with_credential(&credential.stored))
				.ok()?;
		let vault =
			ConversationCredentialVault { account_id: account.clone(), stored: credential.stored };
		let permit = self.inner.capacity.reserve(account.clone(), revision).ok()?;

		if self.is_shutting_down() {
			return None;
		}

		let launch = AttestedAppServerLaunch::bind_selected_control_working_directory(
			profile.as_ref().clone(),
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

fn stored_thread_config(
	child: &mut AttestedProcessChild,
	row: &serde_json::Value,
) -> Result<AgentConfig, &'static str> {
	const DIRECTORY: &str =
		"The first conversation needs an accessible working directory under your home directory.";
	const MODEL: &str = "The conversation's model settings are unavailable in the current account.";
	let cwd = ConversationWorkingDirectory::new(row["cwd"].as_str().ok_or(DIRECTORY)?)
		.map_err(|_| DIRECTORY)?;
	SelectedWorkingDirectory::acquire(cwd.as_str()).map_err(|()| DIRECTORY)?;
	let model = match row["model"].as_str() {
		Some(model) => decodex_protocol::ConversationModel::new(model).map_err(|_| MODEL)?,
		None => {
			// Older or newly materialized threads can omit model. Use the account's
			// native defaults without starting, resuming, or changing the thread.
			let (_, defaults) =
				read_initial_defaults(child, cwd.as_str(), || false).ok_or(MODEL)?;
			defaults
				.managed
				.model
				.or(defaults.configured.model)
				.or(defaults.catalog_model)
				.ok_or(MODEL)?
		},
	};
	Ok(AgentConfig::with_optional_effort(
		model.as_str().into(),
		row["reasoningEffort"].as_str().map(str::to_owned),
		Some(cwd.as_str().into()),
	))
}

#[cfg(test)]
fn read_catalog(
	child: &mut AttestedProcessChild,
	cancelled: impl Fn() -> bool,
) -> Option<Vec<AgentModelDto>> {
	read_catalog_with_default(child, cancelled).map(|(models, _)| models)
}

fn read_catalog_with_default(
	child: &mut AttestedProcessChild,
	cancelled: impl Fn() -> bool,
) -> Option<(Vec<AgentModelDto>, Option<decodex_protocol::ConversationModel>)> {
	let (mut pages, mut cursor, deadline) =
		(ModelCatalogPages::default(), None, Instant::now() + Duration::from_secs(8));

	for _ in 0..8 {
		if cancelled() || Instant::now() >= deadline {
			return None;
		}

		let (page, events) = child.read_ordinary_model_page(cursor.as_deref());

		child.retain_ordinary_events(events).ok()?;

		let page = page.ok()?;

		match pages.push(&page).ok()? {
			Some(next) => cursor = Some(next),
			None => return Some((pages.models, pages.default_model)),
		}
	}

	None
}

fn project_native_defaults(value: NativeExecutionDefaults) -> Option<InitialExecutionDefaults> {
	Some(InitialExecutionDefaults {
		model: value.model.map(decodex_protocol::ConversationModel::new).transpose().ok()?,
		reasoning_effort: value
			.reasoning_effort
			.map(ConversationReasoningEffort::new)
			.transpose()
			.ok()?,
		service_tier: value.service_tier.map(ServiceTier::new).transpose().ok()?,
	})
}

fn read_initial_defaults(
	child: &mut AttestedProcessChild,
	directory: &str,
	cancelled: impl Fn() -> bool,
) -> Option<(Vec<AgentModelDto>, InitialModelDefaults)> {
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

	use crate::{account_launch::process::tests, conversation::model_catalog};
	use decodex_codex::app_server_client::NativeExecutionDefaults;

	#[test]
	fn defaults_projection_distinguishes_absent_from_invalid() {
		let project_defaults = |value: &serde_json::Value, managed| {
			let native = if managed {
				NativeExecutionDefaults::from_requirements_response(value)
			} else {
				NativeExecutionDefaults::from_config_response(value)
			};

			native.ok().and_then(super::project_native_defaults)
		};

		assert_eq!(
			project_defaults(&serde_json::json!({"requirements":null}), true),
			Some(decodex_protocol::InitialExecutionDefaults::default())
		);
		assert_eq!(
			project_defaults(&serde_json::json!({"config":{}}), false),
			Some(decodex_protocol::InitialExecutionDefaults::default())
		);

		for invalid in [
			serde_json::json!({}),
			serde_json::json!({"requirements":false}),
			serde_json::json!({"requirements":{"models":false}}),
			serde_json::json!({"requirements":{"models":{"newThread":{"model":42}}}}),
		] {
			assert!(project_defaults(&invalid, true).is_none());
		}

		let custom = project_defaults(
			&serde_json::json!({"config":{"model_reasoning_effort":"provider-defined-effort"}}),
			false,
		)
		.expect("native custom default");

		assert_eq!(
			custom.reasoning_effort.expect("default effort").as_str(),
			"provider-defined-effort"
		);

		for config in [
			serde_json::json!({"model":"bad\nmodel"}),
			serde_json::json!({"model_reasoning_effort":"bad\neffort"}),
			serde_json::json!({"service_tier":42}),
		] {
			assert!(project_defaults(&serde_json::json!({"config":config}), false).is_none());
		}
	}

	#[test]
	fn selected_native_effort_reaches_turn_start_without_fallback() {
		for value in ["none", "minimal", "persistent", "provider-defined-effort", "xhigh"] {
			let effort: decodex_protocol::ConversationReasoningEffort =
				serde_json::from_value(serde_json::json!(value)).expect("protocol effort");
			let request = decodex_codex::ConversationTurnStartRequest::new(
				decodex_codex::ExactThreadId::new("fixture-thread").expect("thread"),
				decodex_codex::ConversationTurnInput::text("Use the selected effort")
					.expect("input"),
				"fixture-model",
				effort.as_str(),
			)
			.expect("native turn request");
			let encoded = serde_json::to_value(request).expect("native wire");

			assert_eq!(encoded["effort"], value);
		}
	}

	#[test]
	fn stored_thread_bootstrap_uses_native_defaults_only_when_the_model_is_absent() {
		let (_temp, mut child) = tests::ordinary_catalog_child("stored-thread-defaults");
		let cwd = crate::conversation::effective_user_home(unsafe { libc::geteuid() }).unwrap();
		let stored = serde_json::json!({"cwd":cwd,"model":"thread-model","reasoningEffort":"low"});
		let config = super::stored_thread_config(&mut child, &stored).unwrap();
		assert_eq!(config.model, "thread-model");
		assert_eq!(config.agent_effort.as_deref(), Some("low"));
		let config =
			super::stored_thread_config(&mut child, &serde_json::json!({"cwd":cwd})).unwrap();
		assert_eq!(config.model, "managed-model");
		assert_eq!(config.cwd.as_deref(), cwd.to_str());
		assert!(config.agent_effort.is_none());
		let unavailable = serde_json::json!({"cwd":"/","model":"thread-model"});
		assert!(
			super::stored_thread_config(&mut child, &unavailable)
				.unwrap_err()
				.contains("working directory")
		);
		child.shutdown().unwrap();
	}

	#[test]
	fn initial_defaults_preserve_distinct_sources_and_interleaved_events() {
		let (_temp, mut child) = tests::ordinary_catalog_child("exact");
		let (models, defaults) =
			super::read_initial_defaults(&mut child, "/tmp", || false).expect("complete defaults");

		assert_eq!(models.len(), 1);
		assert_eq!(defaults.configured.model.as_ref().unwrap().as_str(), "configured-model");
		assert_eq!(defaults.managed.model.as_ref().unwrap().as_str(), "managed-model");
		assert_eq!(defaults.catalog_model.as_ref().unwrap().as_str(), "catalog-model");
		assert_eq!(defaults.configured.service_tier.as_ref().unwrap().as_str(), "flex");
		assert_eq!(defaults.managed.service_tier.as_ref().unwrap().as_str(), "priority");
		assert!(!serde_json::to_string(&defaults).unwrap().contains("not-public"));
		assert!(child.next_ordinary_turn_event(std::time::Duration::ZERO).unwrap().is_some());

		child.shutdown().unwrap();

		for mode in ["exact-defaults-rejected", "exact-config-defaults-rejected"] {
			let (_temp, mut child) = tests::ordinary_catalog_child(mode);

			assert!(super::read_initial_defaults(&mut child, "/tmp", || false).is_none());

			child.shutdown().unwrap();
		}
	}

	#[test]
	fn metadata_catalog_preserves_tiers_and_interleaved_events() {
		let (_temp, mut child) = tests::ordinary_catalog_child("exact");
		let models =
			model_catalog::read_catalog(&mut child, || false).expect("complete native catalog");

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
		let (_temp, mut child) = tests::ordinary_catalog_child("exact");

		assert!(model_catalog::read_catalog(&mut child, || true).is_none());
		assert!(
			child
				.next_ordinary_turn_event(std::time::Duration::ZERO)
				.expect("no query was sent")
				.is_none()
		);

		child.shutdown().expect("cancelled process closes");

		let (_temp, mut child) = tests::ordinary_catalog_child("exact-catalog-rejected");

		assert!(model_catalog::read_catalog(&mut child, || false).is_none());

		child.shutdown().expect("rejected process closes");
	}
}
