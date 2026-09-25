//! Single service-owned Chief actor. The existing Conversation runtime owns its account process.

use std::{
	sync::Arc,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use decodex_codex::app_server_client::{ClientError, ServerEvent};
use decodex_core::AccountId;
use decodex_database::{EnqueueChiefEvent, SqliteStore};
use decodex_protocol::{ChiefActionDto, ChiefSandboxDto, ChiefStartDto};
use serde_json::json;
use tokio::sync::{Mutex, mpsc, oneshot, watch};

use crate::{
	ChiefConfig, ChiefCoordinator, ChiefError,
	conversation::{ConversationRuntime, StartChiefProcess},
};

#[derive(Debug)]
pub(crate) enum ChiefHostError {
	Rejected(&'static str),
	Unknown(&'static str),
}
impl From<&'static str> for ChiefHostError {
	fn from(message: &'static str) -> Self {
		Self::Rejected(message)
	}
}
impl std::fmt::Display for ChiefHostError {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Rejected(message) | Self::Unknown(message) => formatter.write_str(message),
		}
	}
}
type Reply = oneshot::Sender<Result<String, ChiefHostError>>;
const COMMAND_DEADLINE: Duration = Duration::from_secs(60);
const RECOVERY_MIN_DELAY: Duration = Duration::from_secs(15);
const RECOVERY_MAX_DELAY: Duration = Duration::from_secs(60);

/// Retry connection admission, never a command or an uncertain provider turn.
struct RecoverySchedule {
	next: tokio::time::Instant,
	delay: Duration,
}

impl RecoverySchedule {
	fn new() -> Self {
		Self { next: tokio::time::Instant::now() + RECOVERY_MIN_DELAY, delay: RECOVERY_MIN_DELAY }
	}

	async fn restore_if_due<T>(
		&mut self,
		active: &mut Option<T>,
		now: tokio::time::Instant,
		restore: impl std::future::Future<Output = Option<T>>,
	) {
		if active.is_some() {
			*self = Self::new();
			return;
		}
		if now < self.next {
			return;
		}
		*active = restore.await;
		if active.is_some() {
			*self = Self::new();
		} else {
			self.delay = (self.delay * 2).min(RECOVERY_MAX_DELAY);
			// Base the next retry on completion, so a slow failure cannot hot-loop.
			self.next = tokio::time::Instant::now() + self.delay;
		}
	}
}
struct Request {
	key: String,
	action: ChiefActionDto,
	reply: Reply,
}

#[derive(Clone)]
pub(crate) struct ChiefHost {
	weather_cache: Arc<Mutex<Option<weather::CachedWeather>>>,
	voice: crate::chief_voice::VoiceGateway,
	dictation: crate::dictation::DictationGateway,
	mcp_login: crate::mcp_login::McpLoginGateway,
	store: SqliteStore,
	runtime: ConversationRuntime,
	sender: mpsc::Sender<Request>,
	receiver: Arc<Mutex<Option<mpsc::Receiver<Request>>>>,
}

fn request_is_live_on(
	client: &decodex_codex::app_server_client::AppServerClient,
	payload: &serde_json::Value,
) -> bool {
	if payload["connectionId"].as_str() != Some(client.connection_identity()) {
		return false;
	}
	let Ok(id) = serde_json::from_value(payload["id"].clone()) else { return false };
	let Some(method) = payload["method"].as_str() else { return false };
	client.server_request_guard(&id, method, &payload["params"]).is_some()
}

impl ChiefHost {
	pub(crate) fn new(store: SqliteStore, runtime: ConversationRuntime) -> Self {
		let (sender, receiver) = mpsc::channel(32);
		Self {
			voice: crate::chief_voice::VoiceGateway::new(),
			weather_cache: Arc::new(Mutex::new(None)),
			dictation: Default::default(),
			mcp_login: Default::default(),
			store,
			runtime,
			sender,
			receiver: Arc::new(Mutex::new(Some(receiver))),
		}
	}

	pub(crate) fn voice(
		&self,
		request: &decodex_protocol::ChiefVoiceRequest,
	) -> decodex_protocol::ChiefVoiceStatus {
		self.voice.exchange(request)
	}

	pub(crate) async fn dictation(
		&self,
		request: &decodex_protocol::DictationRequest,
	) -> decodex_protocol::DictationStatus {
		self.dictation.exchange(request, self.runtime.chief_client()).await
	}

	pub(crate) async fn mcp_login(
		&self,
		request: &decodex_protocol::McpLoginRequest,
	) -> decodex_protocol::McpLoginStatus {
		use decodex_protocol::McpLoginPhase;
		let unavailable = || {
			crate::mcp_login::status(
				request,
				McpLoginPhase::Disconnected,
				"The selected task connection is unavailable.",
			)
		};
		let Some((generation, client)) = self.runtime.chief_catalog_client() else {
			return unavailable();
		};
		let Ok(work) = self.store.get_chief_work_item(request.work_id().as_str().into()).await
		else {
			return unavailable();
		};
		let Some(thread) = work.codex_thread_id else {
			return unavailable();
		};
		let result = self
			.mcp_login
			.exchange(
				request,
				Some(crate::mcp_login::Source {
					generation: generation.clone(),
					thread: thread.clone(),
					client,
				}),
			)
			.await;
		let still_owned = self
			.store
			.get_chief_work_item(request.work_id().as_str().into())
			.await
			.ok()
			.is_some_and(|work| work.codex_thread_id.as_deref() == Some(thread.as_str()));
		if still_owned
			&& self.runtime.chief_catalog_client().is_some_and(|(current, _)| current == generation)
		{
			result
		} else {
			unavailable()
		}
	}

	pub(crate) async fn resources(&self, work: &str) -> decodex_protocol::ChiefResourcesResult {
		use decodex_protocol::ChiefResourcesResult;
		let Some((generation, client)) = self.runtime.chief_catalog_client() else {
			return ChiefResourcesResult::Unavailable;
		};
		let Ok(owner) = self.store.get_chief_work_item(work.into()).await else {
			return ChiefResourcesResult::Unavailable;
		};
		let Some(thread) = owner.codex_thread_id else {
			return ChiefResourcesResult::Unavailable;
		};
		let result = crate::chief_resources::read(&client, &thread).await;
		let still_owned = self
			.store
			.get_chief_work_item(work.into())
			.await
			.ok()
			.is_some_and(|owner| owner.codex_thread_id.as_deref() == Some(thread.as_str()));
		if still_owned
			&& self.runtime.chief_catalog_client().is_some_and(|(current, _)| current == generation)
		{
			result
		} else {
			ChiefResourcesResult::Unavailable
		}
	}

	pub(crate) async fn archive_state(&self, work: &str) -> decodex_protocol::ChiefArchiveResult {
		use decodex_codex::app_server_client::ThreadArchiveState as State;
		use decodex_protocol::ChiefArchiveResult as Result;
		let Some((generation, client)) = self.runtime.chief_catalog_client() else {
			return Result::Unavailable;
		};
		let Ok(owner) = self.store.get_chief_work_item(work.into()).await else {
			return Result::Unavailable;
		};
		let Some(thread) = owner.codex_thread_id else {
			return Result::Unbound;
		};
		// Archive membership is a read-only observation of the shared native catalog.
		// A subordinate manager need not run in the root's process to be inspected.
		// Mutations still require exact process ownership in the coordinator.

		let observed = client.thread_archive_state(&thread).await;
		if !self
			.store
			.get_chief_work_item(work.into())
			.await
			.is_ok_and(|current| current.codex_thread_id.as_deref() == Some(thread.as_str()))
			|| !self
				.runtime
				.chief_catalog_client()
				.is_some_and(|(current, _)| current == generation)
		{
			return Result::Unavailable;
		}
		match observed {
			Ok(State::Active) => Result::Active { thread_id: thread },
			Ok(State::Archived) => Result::Archived { thread_id: thread },
			Ok(State::NotFound | State::Changed) => Result::Unconfirmed,
			Err(ClientError::Remote(e)) if e.code == -32601 => Result::Unsupported,
			Err(ClientError::CapacityExceeded) => Result::CapacityExceeded,
			Err(_) => Result::Unavailable,
		}
	}

	pub(crate) async fn install_state(
		&self,
		work: &str,
		event: i64,
	) -> decodex_protocol::ChiefInstallState {
		use decodex_protocol::ChiefInstallState;
		let Some((generation, client)) = self.runtime.chief_catalog_client() else {
			return ChiefInstallState::Unavailable;
		};
		let Some(thread) =
			self.store.get_chief_work_item(work.into()).await.ok().and_then(|w| w.codex_thread_id)
		else {
			return ChiefInstallState::Unavailable;
		};
		let owned = || {
			self.store.chief_thread_is_owned(
				work.into(),
				thread.clone(),
				Some(generation.as_str().into()),
			)
		};
		if !owned().await.unwrap_or(false) {
			return ChiefInstallState::Unavailable;
		}
		let result = tokio::time::timeout(
			Duration::from_secs(40),
			crate::chief_install::inspect(&self.store, &client, work, event),
		)
		.await
		.ok()
		.flatten();
		if !owned().await.unwrap_or(false)
			|| !self
				.runtime
				.chief_catalog_client()
				.is_some_and(|(current, _)| current == generation)
		{
			return ChiefInstallState::Unavailable;
		}
		result.map(|v| v.state).unwrap_or(ChiefInstallState::Unavailable)
	}

	pub(crate) fn guardian_generation(&self) -> Option<String> {
		self.runtime.chief_catalog_client().map(|(generation, _)| generation.as_str().to_owned())
	}

	pub(crate) async fn runtime_source(&self) -> Option<decodex_protocol::EntityId> {
		use sha2::{Digest, Sha256};
		let (generation, account, revision, client) = self.runtime.chief_usage_source().await?;
		let value = serde_json::to_vec(&(
			generation.as_str(),
			account.as_str(),
			revision,
			client.history_revision(),
		))
		.ok()?;
		let digest =
			Sha256::digest(value).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
		decodex_protocol::EntityId::new(digest).ok()
	}

	pub(crate) async fn usage_estimate(
		&self,
		work: &str,
	) -> decodex_protocol::ChiefUsageEstimateResult {
		crate::chief_usage_estimate::read(|| async {
			let (generation, account, revision, client) = self.runtime.chief_usage_source().await?;
			let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
			Some(crate::chief_usage_estimate::Source {
				key: crate::chief_usage_estimate::SourceKey {
					history_revision: client.history_revision(),
					generation,
					account,
					revision,
					thread: owner.codex_thread_id?,
					work: work.into(),
				},
				client,
			})
		})
		.await
	}

	pub(crate) async fn native_goal(
		&self,
		work: &str,
		thread: &str,
	) -> decodex_protocol::ChiefNativeGoalResult {
		crate::chief_native_goal::read(
			&self.store,
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			thread,
		)
		.await
	}

	pub(crate) async fn app_settings(
		&self,
		work: &str,
		event: i64,
	) -> decodex_protocol::ChiefAppSettingsResult {
		crate::chief_app_settings::read(
			&self.store,
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			event,
		)
		.await
	}

	async fn set_app_setting(
		&self,
		work: &str,
		change: crate::chief_app_settings::Selection<'_>,
	) -> Result<String, ChiefHostError> {
		crate::chief_app_settings::write(
			&self.store,
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			change,
		)
		.await?;
		Ok(work.into())
	}

	pub(crate) async fn saved_app_settings(
		&self,
		work: &str,
	) -> decodex_protocol::ChiefSavedAppSettingsResult {
		crate::chief_app_settings::read_saved(&self.store, || async {
			let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
			self.timeline_source(work, &owner.codex_thread_id?).await
		})
		.await
	}

	async fn set_saved_app_setting(
		&self,
		work: &str,
		change: crate::chief_app_settings::SavedSelection<'_>,
	) -> Result<String, ChiefHostError> {
		crate::chief_app_settings::write_saved(
			&self.store,
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			change,
		)
		.await?;
		Ok(work.into())
	}

	pub(crate) async fn hook_settings(
		&self,
		work: &str,
	) -> decodex_protocol::ChiefHookSettingsState {
		crate::chief_hooks::read(&self.store, || async {
			let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
			self.timeline_source(work, &owner.codex_thread_id?).await
		})
		.await
	}

	async fn set_hook_setting(
		&self,
		work: &str,
		change: crate::chief_hooks::Selection<'_>,
	) -> Result<String, ChiefHostError> {
		crate::chief_hooks::write(
			&self.store,
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			change,
		)
		.await?;
		Ok(work.into())
	}

	pub(crate) async fn plugin_selection(
		&self,
		work: &str,
	) -> decodex_protocol::ChiefPluginSelectionState {
		crate::chief_plugins::read(&self.store, || async {
			let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
			self.timeline_source(work, &owner.codex_thread_id?).await
		})
		.await
	}

	async fn set_task_plugin(
		&self,
		work: &str,
		change: crate::chief_plugins::Change<'_>,
	) -> Result<String, ChiefHostError> {
		crate::chief_plugins::write(
			&self.store,
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			change,
		)
		.await?;
		Ok(work.into())
	}

	pub(crate) async fn model_selection(
		&self,
		work: &str,
	) -> decodex_protocol::ChiefModelSelectionState {
		crate::chief_models::read(&self.store, || async {
			let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
			self.timeline_source(work, &owner.codex_thread_id?).await
		})
		.await
	}

	async fn set_task_model(
		&self,
		work: &str,
		change: crate::chief_models::Change<'_>,
	) -> Result<String, ChiefHostError> {
		crate::chief_models::write(
			&self.store,
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			change,
		)
		.await?;
		Ok(work.into())
	}

	pub(crate) async fn permission_profiles(
		&self,
		work: &str,
	) -> decodex_protocol::ChiefPermissionState {
		crate::chief_permissions::read(&self.store, || async {
			let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
			self.timeline_source(work, &owner.codex_thread_id?).await
		})
		.await
	}

	async fn select_permissions(
		&self,
		work: &str,
		thread: &str,
		review: &str,
		profile: &str,
		key: &str,
	) -> Result<String, ChiefHostError> {
		crate::chief_permissions::write(
			&self.store,
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			thread,
			review,
			profile,
			key,
		)
		.await?;
		Ok(work.into())
	}

	pub(crate) async fn app_tool_exposure(
		&self,
		work: &str,
		connector: &str,
	) -> decodex_protocol::ChiefAppExposureResult {
		crate::chief_app_exposure::read(
			&self.store,
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			connector,
		)
		.await
	}

	async fn set_app_tool_exposure(
		&self,
		identity: (
			&decodex_protocol::EntityId,
			&decodex_protocol::WireText,
			&decodex_protocol::WireText,
		),
		omit: Option<Vec<decodex_protocol::ChiefToolExposureSurface>>,
		attempt: &str,
	) -> Result<String, ChiefHostError> {
		let (work, connector, review) =
			(identity.0.as_str(), identity.1.as_str(), identity.2.as_str());
		let change = crate::chief_app_exposure::Change { connector, review, omit, attempt };
		crate::chief_app_exposure::write(
			&self.store,
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			change,
		)
		.await?;
		Ok(work.into())
	}

	pub(crate) async fn live_reviewer(
		&self,
		work: &str,
	) -> decodex_protocol::ChiefLiveReviewerState {
		crate::chief_live_settings::read(&self.store, || async {
			let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
			self.timeline_source(work, &owner.codex_thread_id?).await
		})
		.await
	}

	async fn set_live_reviewer(
		&self,
		ids: (
			&decodex_protocol::EntityId,
			&decodex_protocol::EntityId,
			&decodex_protocol::WireText,
		),
		reviewer: decodex_protocol::ChiefReviewer,
		key: &str,
	) -> Result<String, ChiefHostError> {
		let (work, turn, review) = (ids.0.as_str(), ids.1.as_str(), ids.2.as_str());
		crate::chief_live_settings::write(
			&self.store,
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			turn,
			review,
			reviewer,
			key,
		)
		.await?;
		Ok(work.into())
	}

	pub(crate) async fn model_settings(
		&self,
		work: &str,
	) -> decodex_protocol::ChiefModelSettingsResult {
		crate::chief_model_settings::read(&self.store, || async {
			let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
			self.timeline_source(work, owner.codex_thread_id.as_deref()?).await
		})
		.await
	}

	async fn timeline_source(
		&self,
		work: &str,
		thread: &str,
	) -> Option<crate::chief_usage_estimate::Source> {
		let (generation, account, revision, client) = self.runtime.chief_usage_source().await?;
		let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
		if owner.codex_thread_id.as_deref() != Some(thread) {
			return None;
		}
		Some(crate::chief_usage_estimate::Source {
			key: crate::chief_usage_estimate::SourceKey {
				history_revision: client.history_revision(),
				generation,
				account,
				revision,
				thread: thread.into(),
				work: work.into(),
			},
			client,
		})
	}

	pub(crate) async fn timeline(
		&self,
		work: &str,
		thread: &str,
		cursor: Option<&str>,
	) -> decodex_protocol::ChiefTimelineResult {
		crate::chief::timeline::read(
			Some(&self.store),
			|| self.timeline_source(work, thread),
			cursor,
		)
		.await
	}

	pub(crate) async fn media(
		&self,
		request: &decodex_protocol::ChiefMediaRequest,
	) -> decodex_protocol::ChiefMediaResult {
		crate::chief::timeline::media::read(
			|| self.timeline_source(request.work_id.as_str(), request.thread_id.as_str()),
			request,
		)
		.await
	}

	pub(crate) async fn integrations(
		&self,
		work: &str,
	) -> decodex_protocol::ChiefIntegrationsResult {
		use decodex_protocol::ChiefIntegrationsResult;
		let Some((generation, client)) = self.runtime.chief_catalog_client() else {
			return ChiefIntegrationsResult::Unavailable;
		};
		let Ok(owner) = self.store.get_chief_work_item(work.into()).await else {
			return ChiefIntegrationsResult::Unavailable;
		};
		let Some(thread) = owner.codex_thread_id else {
			return ChiefIntegrationsResult::Unavailable;
		};
		let result = crate::chief_integrations::read(&client, &thread).await;
		let still_owned = self
			.store
			.get_chief_work_item(work.into())
			.await
			.ok()
			.is_some_and(|owner| owner.codex_thread_id.as_deref() == Some(thread.as_str()));
		if still_owned
			&& self.runtime.chief_catalog_client().is_some_and(|(current, _)| current == generation)
		{
			result
		} else {
			ChiefIntegrationsResult::Unavailable
		}
	}

	pub(crate) async fn native_agents(
		&self,
		work: &str,
		thread: Option<&str>,
		cursor: Option<&str>,
	) -> decodex_protocol::NativeAgentsResult {
		let Some((generation, client)) = self.runtime.chief_catalog_client() else {
			return decodex_protocol::NativeAgentsResult::Unavailable;
		};
		let result = crate::native_agents::read(&self.store, &client, work, thread, cursor).await;
		if self.runtime.chief_catalog_client().is_some_and(|(current, _)| current == generation) {
			result
		} else {
			decodex_protocol::NativeAgentsResult::Unavailable
		}
	}

	pub(crate) async fn activity_detail(
		&self,
		work: &str,
		turn: &str,
		item: &str,
		cursor: Option<&decodex_protocol::ChiefActivityDetailCursor>,
	) -> decodex_protocol::ChiefActivityDetailResult {
		crate::chief_detail::read_bound(
			|| async {
				let owner = self.store.get_chief_work_item(work.into()).await.ok()?;
				self.timeline_source(work, owner.codex_thread_id.as_deref()?).await
			},
			turn,
			item,
			cursor,
		)
		.await
	}

	pub(crate) async fn file_approval_detail(
		&self,
		thread: &str,
		turn: &str,
		item: &str,
	) -> decodex_protocol::ChiefActivityDetailResult {
		let Some(client) = self.runtime.chief_client() else {
			return decodex_protocol::ChiefActivityDetailResult::Unavailable;
		};
		crate::chief_detail::read_file_changes(&client, thread, turn, item).await
	}

	pub(crate) fn request_is_live(&self, payload: &serde_json::Value) -> bool {
		self.runtime.chief_client().is_some_and(|client| request_is_live_on(&client, payload))
	}

	pub(crate) async fn capabilities(&self) -> decodex_protocol::ChiefCapabilitiesResult {
		let Some((generation, client)) = self.runtime.chief_catalog_client() else {
			return decodex_protocol::ChiefCapabilitiesResult::Unavailable;
		};
		let result = crate::chief_capabilities::read(&client).await;
		if self.runtime.chief_catalog_client().is_some_and(|(current, _)| current == generation) {
			result
		} else {
			decodex_protocol::ChiefCapabilitiesResult::Unavailable
		}
	}

	pub(crate) async fn submit(
		&self,
		key: String,
		action: ChiefActionDto,
	) -> Result<String, ChiefHostError> {
		let (reply, result) = oneshot::channel();
		tokio::time::timeout(COMMAND_DEADLINE, self.sender.send(Request { key, action, reply }))
			.await
			.map_err(|_| ChiefHostError::Rejected("Chief command queue is full"))?
			.map_err(|_| ChiefHostError::Rejected("Chief service is stopped"))?;
		await_acceptance(result, COMMAND_DEADLINE).await
	}

	pub(crate) async fn serve(self, mut stop: watch::Receiver<bool>) {
		let Some(mut requests) = self.receiver.lock().await.take() else {
			return;
		};
		let Some(mut voice_requests) = self.voice.take_receiver().await else {
			return;
		};
		let mut active = None;
		// The stop receiver must remain polled while attach, recovery, and RPCs await.
		let drive = async {
			active = self.restore().await;
			let mut recovery = RecoverySchedule::new();
			let mut tick = tokio::time::interval(Duration::from_secs(15));
			tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
			loop {
				tokio::select! {
					voice_request=voice_requests.recv()=> {
						if let Some(request)=voice_request {self.handle_voice(request,&mut active).await;}
					},
					request = requests.recv() => {
						let Some(request) = request else {break;};
						self.rotate_exhausted(&mut active).await;
						let outcome = self.handle(request.key,request.action,&mut active).await;
						let _ = request.reply.send(outcome);
						if let Some((root,chief,_)) = active.as_mut() {
							self.record_delivery(root, chief.wake_pending().await).await;
						}
					},
					event = receive(&mut active) => {
						if let Some((root,chief,_)) = active.as_mut() {
							chief.pause_dispatch(self.runtime.chief_account_exhausted(root).await);
							let closed = event.is_none() || matches!(&event,Some(ServerEvent::Closed(_)));
							if let Some(event)=event.as_ref() && let Some(generation)=chief.native_generation() {
								self.mcp_login.observe(generation,event).await;
								if let ServerEvent::Notification { method, params } = event
									&& matches!(method.as_str(), "configWarning" | "warning")
									&& crate::native_config_warning::record_notification(&self.store, root, generation, method, params).await.is_err() {
									self.record_error(root,"event_processing_failed").await;
								}
							}
							if closed && let Some(generation)=chief.native_generation() {self.mcp_login.disconnect(Some(generation)).await;}

							if let Some(event) = event
								&& let Err(error) = chief.handle_event(event).await
								&& event_failure_needs_attention(closed, &error) {
								self.record_error(root,"event_processing_failed").await;
							}
							if closed {
								let root = root.clone();
								let _ = self.runtime.close_chief_connection(&root).await;
								active = None;
								recovery = RecoverySchedule::new();
							}
						}
					},
					_ = tick.tick() => {
						self.dictation.expire().await;
						self.mcp_login.expire().await;
						if let Some(request)=self.voice.expire() {self.handle_voice(request,&mut active).await;}
						self.rotate_exhausted(&mut active).await;
						recovery.restore_if_due(
							&mut active, tokio::time::Instant::now(), self.restore()
						).await;
						if let Some((root,chief,_)) = active.as_mut() {
							self.record_delivery(root, chief.check_due_followups(now()).await).await;
						}
					},
				}
			}
		};
		tokio::select! {
			biased;
			_ = stopped(&mut stop) => {},
			_ = drive => {},
		}
		requests.close();
		while let Ok(request) = requests.try_recv() {
			let _ = request.reply.send(Err(ChiefHostError::Rejected("Chief service is stopped")));
		}
		// Attach can be cancelled before `active` is assigned. Close the persisted
		// root as well so its admitted process cannot escape the actor lifecycle.
		let root = match active {
			Some((root, _, _)) => Some(root),
			None => self.store.list_chief_work_items().await.ok().and_then(|items| {
				items.into_iter().find(|item| item.parent_goal_id.is_none()).map(|item| item.id)
			}),
		};
		if let Some(root) = root {
			let _ = self.runtime.close_chief_connection(&root).await;
		}
	}

	async fn handle_voice(
		&self,
		request: decodex_protocol::ChiefVoiceRequest,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) {
		let id = request.session_id().as_str().to_owned();
		let result = match active.as_mut() {
			Some((_, chief, _)) =>
				tokio::time::timeout(Duration::from_secs(30), chief.voice_request(request))
					.await
					.unwrap_or_else(|_| {
						Err(ChiefError::Invalid(
							"voice signaling timed out; do not replay input".into(),
						))
					}),
			None => Err(ChiefError::Invalid("Chief is reconnecting".into())),
		};
		if let Err(error) = result {
			let detail = match error {
				ChiefError::Transport(ClientError::Remote(error)) =>
					crate::chief_voice::provider_error_message(&error.message).into(),
				ChiefError::Invalid(message) => message,
				ChiefError::Store(_) => "Voice session state could not be saved.".into(),
				_ => "Voice could not connect to this Chief. Check connection status.".into(),
			};
			self.voice.update(&id, decodex_protocol::ChiefVoicePhase::Failed, None, Some(&detail));
		}
	}

	async fn rotate_exhausted(
		&self,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) {
		let Some((root, chief, _)) = active.as_mut() else {
			return;
		};
		if self.dictation.active().await {
			return;
		}
		if self.store.open_chief_voice_calls().await.map_or(true, |calls| !calls.is_empty()) {
			return;
		}
		let exhausted = self.runtime.chief_account_exhausted(root).await;
		chief.pause_dispatch(exhausted);
		let Ok(work) = self.store.list_chief_work_items().await else {
			return;
		};
		if work.iter().any(|item| item.dispatch_state != decodex_database::ChiefDispatchState::Idle)
			|| !exhausted
		{
			return;
		}
		let root = root.clone();
		*active = None;
		// Existing process death must be positively established before the store permits
		// another account. No uncertain or active turn is replayed during this handover.
		if self.runtime.close_chief_connection(&root).await.is_ok() {
			*active = self.restore().await;
		}
	}

	async fn accept_message(
		&self,
		root_id: &decodex_protocol::EntityId,
		text: &decodex_protocol::HistoryText,
		key: &str,
		input_options: Option<&serde_json::Value>,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, ChiefHostError> {
		let root = self
			.store
			.get_chief_work_item(root_id.as_str().into())
			.await
			.map_err(|_| "Chief root is unavailable")?;
		let managers =
			self.store.chief_manager_ids().await.map_err(|_| "Manager state unavailable")?;
		if !managers.contains(&root.id) {
			return Err("Chief identity differs".into());
		}

		persist_input(&self.store, &root.id, key, text.as_str(), input_options).await?;
		if active.is_none() {
			*active = self.restore().await;
		}
		Ok(root.id)
	}

	async fn cancel_capacity_retry(
		&self,
		work_id: decodex_protocol::EntityId,
		event_id: i64,
	) -> Result<String, ChiefHostError> {
		self.store
			.cancel_chief_capacity_retry(work_id.as_str().into(), event_id)
			.await
			.map_err(|_| "capacity retry is no longer pending; refresh state")?;
		Ok(work_id.as_str().into())
	}

	async fn skip_question(
		identity: (
			&decodex_protocol::EntityId,
			&decodex_protocol::WireText,
			&decodex_protocol::WireText,
		),
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, ChiefHostError> {
		let (work, thread, question) =
			(identity.0.as_str(), identity.1.as_str(), identity.2.as_str());
		let (_, chief, _) = active.as_mut().ok_or("Chief is not connected")?;
		chief.skip_async_question(work, thread, question).await.map_err(|_| {
			ChiefHostError::Rejected(
				"Question could not be skipped. Refresh the connected conversation before trying again.",
			)
		})?;
		Ok(work.into())
	}

	async fn native_agent_input(
		&self,
		ids: (&str, &str),
		text: &str,
		expected_turn: Option<&str>,
	) -> Result<String, ChiefHostError> {
		let (work, thread) = ids;
		let client = self.runtime.chief_client().ok_or("Agent connection unavailable")?;
		let result =
			crate::native_agents::read(&self.store, &client, work, Some(thread), None).await;
		let decodex_protocol::NativeAgentsResult::Conversation {
			can_input: true, active_turn, ..
		} = result
		else {
			return Err("This native agent does not accept direct input. Ask its parent agent to follow up.".into());
		};
		if active_turn.as_deref() != expected_turn {
			return Err("Agent state changed. Review the conversation before sending.".into());
		}
		let input = json!([{"type":"text","text":text}]);
		let result = if let Some(turn) = active_turn {
			client
				.request(
					"turn/steer",
					json!({"threadId":thread,"expectedTurnId":turn,"input":input}),
				)
				.await
		} else {
			client
				.request(
					"turn/start",
					json!({"threadId":thread,"input":input,"turnTrigger":"user"}),
				)
				.await
		};
		result.map_err(|error| match error {
			ClientError::RequestTooLarge | ClientError::RequestQueueFull =>
				ChiefHostError::Rejected(
					"Message was not sent: the local connection refused this request. Your draft is preserved.",
				),
			_ => ChiefHostError::Unknown(
				"Native message delivery could not be confirmed. Inspect history before sending again.",
			),
		})?;
		Ok(work.into())
	}

	async fn continue_reviewed_misalignment(
		&self,
		work_id: decodex_protocol::EntityId,
		review_id: decodex_protocol::WireText,
		key: &str,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, ChiefHostError> {
		let review = self
			.store
			.chief_misalignment(work_id.as_str().into())
			.await
			.map_err(|_| "Provider findings unavailable")?
			.ok_or("Provider precaution is no longer current")?;
		if review.review_id() != review_id.as_str() {
			return Err("Provider findings changed; review them again".into());
		}
		let (_, chief, _) = active.as_mut().ok_or("Chief is not connected")?;
		chief.continue_misalignment(work_id.as_str(),review,key).await.map_err(|error| match error { ChiefError::Rejected(_) => ChiefHostError::Rejected("Continuation was rejected or the findings changed. Review the latest findings before trying again."), _ => ChiefHostError::Unknown("Continuation was not confirmed. Inspect the latest conversation state before trying again.") })?;
		Ok(work_id.as_str().into())
	}

	async fn handle_settings(
		&self,
		key: &str,
		action: ChiefActionDto,
	) -> Result<String, ChiefHostError> {
		match action {
			ChiefActionDto::SetSavedAppSetting {
				work_id,
				thread_id,
				connector_id,
				link_id,
				review_token,
				edit,
			} =>
				self.set_saved_app_setting(
					work_id.as_str(),
					crate::chief_app_settings::SavedSelection {
						thread: thread_id.as_str(),
						connector: connector_id.as_str(),
						link: link_id.as_str(),
						review: review_token.as_str(),
						edit: &edit,
						attempt_id: key,
					},
				)
				.await,
			ChiefActionDto::SetAppToolExposure { work_id, connector_id, review_token, omit } =>
				self.set_app_tool_exposure((&work_id, &connector_id, &review_token), omit, key)
					.await,
			ChiefActionDto::SetAppSetting { work_id, event_id, review_token, edit } =>
				self.set_app_setting(
					work_id.as_str(),
					crate::chief_app_settings::Selection {
						event: event_id,
						review: review_token.as_str(),
						edit: &edit,
						attempt_id: key,
					},
				)
				.await,
			ChiefActionDto::SetHookSetting {
				work_id,
				thread_id,
				review_token,
				hook_key,
				change,
			} =>
				self.set_hook_setting(
					work_id.as_str(),
					crate::chief_hooks::Selection {
						thread: thread_id.as_str(),
						review: review_token.as_str(),
						hook: hook_key.as_str(),
						change,
						attempt_id: key,
					},
				)
				.await,
			ChiefActionDto::SetTaskPlugin {
				work_id,
				thread_id,
				review_token,
				plugin_id,
				enabled,
			} =>
				self.set_task_plugin(
					work_id.as_str(),
					crate::chief_plugins::Change {
						thread: thread_id.as_str(),
						review: review_token.as_str(),
						plugin: plugin_id.as_str(),
						enabled,
						attempt_id: key,
					},
				)
				.await,
			ChiefActionDto::SetTaskModel { work_id, thread_id, review_token, model, effort } =>
				self.set_task_model(
					work_id.as_str(),
					crate::chief_models::Change {
						thread: thread_id.as_str(),
						review: review_token.as_str(),
						model: model.as_str(),
						effort: effort.as_ref().map(|e| e.as_str()),
						attempt_id: key,
					},
				)
				.await,
			ChiefActionDto::SelectPermissions { work_id, thread_id, review_token, profile_id } =>
				self.select_permissions(
					work_id.as_str(),
					thread_id.as_str(),
					review_token.as_str(),
					profile_id.as_str(),
					key,
				)
				.await,
			ChiefActionDto::SetLiveReviewer { work_id, turn_id, review_token, reviewer } =>
				self.set_live_reviewer((&work_id, &turn_id, &review_token), reviewer, key).await,
			_ => Err(ChiefHostError::Rejected("Unsupported settings action.")),
		}
	}

	async fn handle(
		&self,
		key: String,
		action: ChiefActionDto,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, ChiefHostError> {
		let (action, input_options) = normalize_input(action)?;

		match action {
			action @ (ChiefActionDto::SetAppToolExposure { .. }
			| ChiefActionDto::SetSavedAppSetting { .. }
			| ChiefActionDto::SetAppSetting { .. }
			| ChiefActionDto::SetHookSetting { .. }
			| ChiefActionDto::SetTaskPlugin { .. }
			| ChiefActionDto::SetTaskModel { .. }
			| ChiefActionDto::SelectPermissions { .. }
			| ChiefActionDto::SetLiveReviewer { .. }) => self.handle_settings(key.as_str(), action).await,
			ChiefActionDto::NativeAgentInput { work_id, thread_id, text, expected_turn } =>
				self.native_agent_input(
					(work_id.as_str(), thread_id.as_str()),
					text.as_str(),
					expected_turn.as_ref().map(|turn| turn.as_str()),
				)
				.await,
			ChiefActionDto::InstallSuggestedPlugin { work_id, event_id, review_token } =>
				self.install_plugin(work_id.as_str(), event_id, review_token.as_str(), &key, active)
					.await,
			ChiefActionDto::RestoreArchivedThread { work_id, thread_id } => {
				let (_, chief, _) = active.as_mut().ok_or("Chief is not connected")?;
				chief.restore_archived_thread(work_id.as_str(),thread_id.as_str()).await.map_err(|error|match error {
                    ChiefError::Rejected(_)=>ChiefHostError::Rejected("Restoration was not accepted. Refresh the task archive state before trying again."),
                    _=>ChiefHostError::Unknown("Restoration is not confirmed. Refresh archive state; the restore request will not be repeated automatically."),
                })?;
				Ok(work_id.as_str().into())
			},
			ChiefActionDto::RefreshIntegrations { work_id } =>
				self.refresh_integrations(work_id.as_str(), active).await,
			ChiefActionDto::AddResourceLink { work_id, title, url } => {
				let (_, chief, _) = active.as_ref().ok_or("Chief is not connected")?;
				chief
					.add_resource_link(work_id.as_str(), title.as_str(), url.as_str())
					.await
					.map_err(resource_error)?;
				Ok(work_id.as_str().into())
			},
			ChiefActionDto::RemoveResource { work_id, attachment_type, identity_key } => {
				let (_, chief, _) = active.as_ref().ok_or("Chief is not connected")?;
				chief
					.remove_resource(
						work_id.as_str(),
						attachment_type.as_str(),
						identity_key.as_str(),
					)
					.await
					.map_err(resource_error)?;
				Ok(work_id.as_str().into())
			},

			ChiefActionDto::StartConfigured { .. } | ChiefActionDto::SendConfigured { .. } =>
				unreachable!("normalized input"),
			action @ ChiefActionDto::Steer { .. } => Self::steer_action(action, &key, active).await,
			ChiefActionDto::ContinueMisalignment { work_id, review_id } =>
				self.continue_reviewed_misalignment(work_id, review_id, &key, active).await,

			ChiefActionDto::ApproveGuardianDenial { work_id, review_row, review_digest } => {
				let (_, chief, _) = active.as_mut().ok_or("Chief is not connected")?;
				chief.approve_guardian_denial(work_id.as_str(),review_row,review_digest.as_str(),&key).await
					.map_err(|error| match error {
						ChiefError::Rejected(_) => ChiefHostError::Rejected("Approval was rejected or the review is no longer current. Refresh the review before trying again."),
						_ => ChiefHostError::Unknown("Approval submission was not confirmed. It will not be sent again automatically."),
					})?;
				Ok(work_id.as_str().into())
			},
			ChiefActionDto::AnswerQuestion { work_id, question_id, answer } => {
				let (_, chief, _) = active.as_mut().ok_or("Chief is not connected")?;
				chief
					.answer_async_question(
						work_id.as_str(),
						question_id.as_str(),
						answer.as_str(),
						&key,
					)
					.await
					.map_err(question_input_error)?;
				Ok(work_id.as_str().into())
			},
			ChiefActionDto::SkipQuestion { work_id, thread_id, question_id } =>
				Self::skip_question((&work_id, &thread_id, &question_id), active).await,
			ChiefActionDto::CancelCapacityRetry { work_id, event_id } =>
				self.cancel_capacity_retry(work_id, event_id).await,
			ChiefActionDto::Respond { work_id, event_id, response_json } =>
				self.respond(work_id.as_str(), event_id, response_json.as_str(), active).await,
			ChiefActionDto::RespondWithRequestedDecision { work_id, event_id, decision } =>
				self.respond_requested(work_id.as_str(), event_id, &decision, active).await,
			ChiefActionDto::Start(draft) =>
				self.start(draft, &key, input_options.as_ref(), active).await,
			ChiefActionDto::Send { root_id, text } =>
				self.accept_message(&root_id, &text, &key, input_options.as_ref(), active).await,
			ChiefActionDto::Interrupt { work_id, turn_id } => {
				let (_, chief, _) = active.as_mut().ok_or("Chief is not connected")?;
				chief.interrupt_work(work_id.as_str(), turn_id.as_str()).await.map_err(|_| {
					ChiefHostError::Unknown(
						"exact work turn interrupt could not be confirmed; refresh state",
					)
				})?;
				Ok(work_id.as_str().into())
			},
			ChiefActionDto::AutomationResult { work_id, source_event_id, payload } => {
				self.store
					.enqueue_chief_event(EnqueueChiefEvent {
						source_event_id: json!(["automation", source_event_id.as_str()])
							.to_string(),
						work_item_id: work_id.as_str().into(),
						event_kind: "automation_result".into(),
						payload: payload.as_str().into(),
					})
					.await
					.map_err(|_| "automation result could not be accepted")?;
				if active.is_none() {
					*active = self.restore().await;
				}
				Ok(work_id.as_str().into())
			},
		}
	}

	async fn steer_action(
		action: ChiefActionDto,
		key: &str,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, ChiefHostError> {
		let ChiefActionDto::Steer { work_id, turn_id, text, attachments, task_references } = action
		else {
			unreachable!("steer action")
		};
		validate_attachments(&attachments)?;
		let (_, chief, _) = active.as_mut().ok_or("Chief is not connected")?;
		chief
			.steer_work_with_references(
				work_id.as_str(),
				turn_id.as_str(),
				key,
				text.as_str(),
				crate::chief::ChiefInputExtras {
					attachments: &attachments,
					task_references: &task_references,
				},
			)
			.await
			.map_err(steer_input_error)?;
		Ok(work_id.as_str().into())
	}

	async fn install_plugin(
		&self,
		work: &str,
		event_id: i64,
		review: &str,
		key: &str,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, ChiefHostError> {
		let (_, chief, _) = active.as_ref().ok_or("Chief is not connected")?;
		chief.install_suggested_plugin(work, event_id, review, key).await.map_err(|error| {
			match error {
				ChiefError::Rejected(_) => ChiefHostError::Rejected(
					"Installation was not started. Refresh the suggestion and review its current details.",
				),
				_ => ChiefHostError::Unknown(
					"Installation is not confirmed. Read its current status; do not repeat the installation.",
				),
			}
		})?;
		Ok(work.into())
	}

	async fn refresh_integrations(
		&self,
		work: &str,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, ChiefHostError> {
		let (_, chief, _) = active.as_ref().ok_or("Chief is not connected")?;
		match chief.refresh_integrations(work).await {
			Ok(true) => Ok(work.into()),
			Ok(false) => Err(ChiefHostError::Rejected(
				"Some plugin updates failed and cached versions may remain. MCP reload was acknowledged; read the refreshed status before retrying.",
			)),
			Err(ChiefError::Rejected(_)) =>
				Err(ChiefHostError::Rejected("The task no longer has a native thread.")),
			Err(ChiefError::Transport(ClientError::Remote(_))) => Err(ChiefHostError::Unknown(
				"Native refresh returned an error and may have partly applied. Inspect the current integration status before retrying.",
			)),
			Err(_) => Err(ChiefHostError::Unknown(
				"Integration refresh could not be confirmed. Read current status; do not automatically retry.",
			)),
		}
	}

	async fn respond_requested(
		&self,
		work: &str,
		event_id: i64,
		decision: &decodex_protocol::ChiefRequestedDecision,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, ChiefHostError> {
		let event = self
			.store
			.get_chief_inbox_event(event_id)
			.await
			.map_err(|_| "pending request is unavailable; refresh state")?;
		if event.work_item_id != work || event.disposition.is_some() {
			return Err("request identity or state changed; refresh state".into());
		}
		let payload: serde_json::Value =
			serde_json::from_str(&event.payload).map_err(|_| "stored request is unavailable")?;
		let response = decodex_protocol::requested_decision_response(
			payload["method"].as_str().unwrap_or_default(),
			&payload["params"],
			decision,
		)
		.ok_or("the selected decision is not offered by this request")?;
		self.respond(work, event_id, &response.to_string(), active).await
	}

	async fn respond(
		&self,
		work: &str,
		event_id: i64,
		response_json: &str,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, ChiefHostError> {
		let event = self
			.store
			.get_chief_inbox_event(event_id)
			.await
			.map_err(|_| "pending request is unavailable; refresh state")?;
		if event.work_item_id != work
			|| event.disposition.is_some()
			|| !["permission_pending", "user_input_pending", "server_request_pending"]
				.contains(&event.event_kind.as_str())
		{
			return Err("request identity or state changed; refresh state".into());
		}
		let response: serde_json::Value =
			serde_json::from_str(response_json).map_err(|_| "response must be valid JSON")?;
		if !response.is_object() {
			return Err("response must be a JSON object".into());
		}
		let (_, chief, _) =
			active.as_mut().ok_or("Chief is not connected; stale requests cannot be replayed")?;
		chief.respond_pending_event(event_id, response).await.map_err(|error| {
			if matches!(error, ChiefError::Rejected(_)) {
				return ChiefHostError::Rejected(
					"The response does not match the current provider request. Review the form before submitting.",
				);
			}
			ChiefHostError::Unknown(
				"request response could not be confirmed; refresh state before retrying",
			)
		})?;
		Ok(work.into())
	}

	async fn start(
		&self,
		draft: ChiefStartDto,
		key: &str,
		input_options: Option<&serde_json::Value>,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, ChiefHostError> {
		let root = draft.root_id.as_str().to_owned();
		let account_id = draft
			.account_id
			.as_ref()
			.map(|id| AccountId::new(id.as_str()))
			.transpose()
			.map_err(|_| "invalid account identity")?;
		if active.as_ref().is_some_and(|(current, _, _)| current != &root) {
			return Err("another Chief is active".into());
		}
		let config = config(&draft);
		ChiefCoordinator::reserve_root(&self.store, &root, draft.prompt.as_str())
			.await
			.map_err(|_| "Chief root could not be reserved")?;
		let mut settings =
			serde_json::to_value(&config).map_err(|_| "invalid Chief configuration")?;
		if let Some(account) = &draft.account_id {
			settings["account_id"] = json!(account.as_str());
		}
		let encoded =
			serde_json::to_string(&settings).map_err(|_| "invalid Chief configuration")?;
		self.store
			.bind_chief_root_settings(&root, &encoded)
			.await
			.map_err(|_| "Chief configuration differs from its saved execution context")?;
		// Persist the user input before any external process or thread effect.
		persist_input(&self.store, &root, key, draft.prompt.as_str(), input_options).await?;
		if active.is_none() {
			match self.connect(&root, key.to_owned(), config, account_id).await {
				Ok(connection) => *active = Some(connection),
				Err(_) => {
					self.record_error(&root, "reconnection_needs_attention").await;
				},
			}
		}
		Ok(root)
	}

	async fn record_delivery(&self, root: &str, result: Result<(), ChiefError>) {
		match result {
			Ok(()) => {
				let _ = self.store.resolve_chief_delivery_failure(root.into()).await;
			},
			Err(ChiefError::InputNotSent(_)) => {
				// The retained input already has a user-decision receipt. Do not block
				// its editor with a false connection failure or requeue it.
				let _ = self.store.resolve_chief_delivery_failure(root.into()).await;
			},
			Err(ChiefError::ThreadOwnedElsewhere) => {
				let _ = self.store.hold_chief_unsent_input(root.into()).await;
				let _ = self
					.store
					.record_chief_thread_in_use(
						root.into(),
						diagnostic(&ChiefError::ThreadOwnedElsewhere),
					)
					.await;
			},
			Err(error) => {
				let _ =
					self.store.record_chief_delivery_failure(root.into(), diagnostic(&error)).await;
			},
		}
	}

	async fn record_error(&self, root: &str, kind: &str) {
		if kind == "reconnection_needs_attention" {
			let _ = self
				.store
				.record_chief_connection_failure(
					root.into(),
					"Inspect account and process readiness.".into(),
				)
				.await;
			return;
		}
		let _ = self
			.store
			.enqueue_chief_event(EnqueueChiefEvent {
				source_event_id: json!(["chief_host", root, kind]).to_string(),
				work_item_id: root.into(),
				event_kind: kind.into(),
				payload: json!({"recovery":"inspect persisted work before retrying"}).to_string(),
			})
			.await;
	}

	async fn connect(
		&self,
		root: &str,
		operation_key: String,
		config: ChiefConfig,
		account_id: Option<AccountId>,
	) -> Result<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>), &'static str> {
		let connection = match self
			.runtime
			.open_chief_connection(StartChiefProcess {
				operation_key,
				root_id: root.into(),
				working_directory: config.cwd.clone(),
				account_id,
			})
			.await
		{
			Ok(connection) => connection,
			Err(error) => {
				// ChiefLaunchError contains only typed, credential-negative readiness facts.
				let _ = self
					.store
					.record_chief_connection_failure(root.into(), error.to_string())
					.await;
				return Err(
					"Chief account process is unavailable; inspect account and process readiness",
				);
			},
		};
		let binding = match self.store.read_chief_process_binding(root).await {
			Ok(binding) => binding,
			Err(_) => {
				let _ = self.runtime.close_chief_connection(root).await;
				return Err("Chief process binding is unavailable");
			},
		};
		if !binding.is_some_and(|binding| {
			binding.account_id == connection.account_id
				&& binding.generation_id == connection.process_generation_id
		}) {
			let _ = self.runtime.close_chief_connection(root).await;
			return Err("Chief process binding did not match the admitted account");
		}
		let mut coordinator =
			match ChiefCoordinator::new(self.store.clone(), connection.client, config) {
				Ok(coordinator) => coordinator,
				Err(_) => {
					let _ = self.runtime.close_chief_connection(root).await;
					return Err("invalid Chief configuration");
				},
			};
		coordinator.bind_native_generation(connection.process_generation_id.clone());
		coordinator.attach_voice_host(
			connection.process_generation_id.as_str().into(),
			self.voice.clone(),
		);
		if self.store.resolve_chief_connection_failure(root.into()).await.is_err() {
			let _ = self.runtime.close_chief_connection(root).await;
			return Err("Chief connection recovery receipt could not be saved");
		}
		Ok((root.into(), coordinator, connection.events))
	}

	async fn restore(&self) -> Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)> {
		let root = self
			.store
			.list_chief_work_items()
			.await
			.ok()?
			.into_iter()
			.find(|work| work.parent_goal_id.is_none())?;
		let settings = match self.store.read_chief_root_settings(&root.id).await {
			Ok(Some(encoded)) => decode_settings(&encoded),
			_ => None,
		};
		let Some((config, account)) = settings else {
			self.record_error(&root.id, "configuration_needs_attention").await;
			return None;
		};
		match self.connect(&root.id, format!("restore-{}", now()), config, account).await {
			Ok(mut active) => {
				if active.1.recover_persisted().await.is_err() {
					self.record_error(&root.id, "recovery_needs_attention").await;
				}
				self.record_delivery(&root.id, active.1.wake_pending().await).await;
				Some(active)
			},
			Err(_) => None,
		}
	}
}

fn diagnostic(error: &ChiefError) -> String {
	match error {
        ChiefError::ThreadArchived => "This conversation is archived. Unarchive it to continue. Your saved messages remain queued.".into(),
		ChiefError::ThreadOwnedElsewhere => "This Chief conversation is open in Codex or another application. Sending is unavailable here while it is in use. Your history remains readable.".into(),
		ChiefError::Store(_) => "Chief delivery could not access its saved state.".into(),
		ChiefError::DependenciesPending(_) => "Chief is waiting for prerequisite work.".into(),
		_ => error.to_string(),
	}
}

fn event_failure_needs_attention(closed: bool, error: &ChiefError) -> bool {
	// Closed deliberately returns its transport error after saving dispatch fences.
	// A failed fence/store operation is still a real processing failure.
	!closed || !matches!(error, ChiefError::Transport(_))
}

fn decode_settings(encoded: &str) -> Option<(ChiefConfig, Option<AccountId>)> {
	let settings: serde_json::Value = serde_json::from_str(encoded).ok()?;
	let config = serde_json::from_value::<ChiefConfig>(settings.clone()).ok()?;
	let account = match settings.get("account_id") {
		None => None,
		Some(value) => Some(AccountId::new(value.as_str()?).ok()?),
	};
	Some((config, account))
}

async fn await_acceptance(
	result: oneshot::Receiver<Result<String, ChiefHostError>>,
	deadline: Duration,
) -> Result<String, ChiefHostError> {
	tokio::time::timeout(deadline, result)
		.await
		.map_err(|_| {
			ChiefHostError::Unknown(
				"Chief command acceptance is unknown; inspect persisted work before retrying",
			)
		})?
		.map_err(|_| ChiefHostError::Unknown("Chief command acceptance is unknown"))?
}

async fn stopped(stop: &mut watch::Receiver<bool>) {
	loop {
		if *stop.borrow_and_update() {
			return;
		}
		if stop.changed().await.is_err() {
			return;
		}
	}
}

fn normalize_input(
	action: ChiefActionDto,
) -> Result<(ChiefActionDto, Option<serde_json::Value>), ChiefHostError> {
	let normalized = match action {
		ChiefActionDto::StartConfigured { start, execution, attachments, task_references } => {
			validate_attachments(&attachments)?;
			(
				ChiefActionDto::Start(start),
				Some(
					json!({"execution":execution,"attachments":attachments,"taskReferences":task_references}),
				),
			)
		},
		ChiefActionDto::SendConfigured {
			root_id,
			text,
			execution,
			attachments,
			task_references,
		} => {
			validate_attachments(&attachments)?;
			(
				ChiefActionDto::Send { root_id, text },
				Some(
					json!({"execution":execution,"attachments":attachments,"taskReferences":task_references}),
				),
			)
		},
		other => (other, None),
	};
	Ok(normalized)
}

fn validate_attachments(
	files: &[decodex_protocol::ChiefAttachmentDto],
) -> Result<(), &'static str> {
	if files.len() > 16 {
		return Err("Attach at most 16 files");
	}
	for file in files {
		let path = std::path::Path::new(file.path.as_str());
		if !path.is_absolute() || !path.is_file() {
			return Err("An attached file is no longer available");
		}
	}
	Ok(())
}

async fn persist_input(
	store: &SqliteStore,
	root: &str,
	key: &str,
	text: &str,
	options: Option<&serde_json::Value>,
) -> Result<(), &'static str> {
	let pending = store
		.list_pending_chief_events(1000)
		.await
		.map_err(|_| "Conversation state is unavailable")?;
	if pending.iter().any(|event| {
		event.work_item_id == root && event.event_kind == "thread_in_use_needs_attention"
	}) {
		return Err("This conversation is in use in another app. No message was queued.");
	}
	store
  .enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: json!(["user_message", root, key]).to_string(),
			work_item_id: root.into(),
			event_kind: "user_message".into(),
			payload: json!({"text":text,"source":"user","asyncQuestionReply":decodex_protocol::parse_chief_async_question_replies(text).is_some(),"options":options}).to_string(),
		})
		.await
		.map_err(|_| "Chief input could not be accepted")?;
	Ok(())
}

async fn receive(
	active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
) -> Option<ServerEvent> {
	match active {
		Some((_, _, events)) =>
			Some(events.recv().await.unwrap_or(ServerEvent::Closed(ClientError::Closed))),
		None => std::future::pending().await,
	}
}

fn config(draft: &ChiefStartDto) -> ChiefConfig {
	let mut config = ChiefConfig::with_optional_effort(
		draft.model.as_str().into(),
		draft.effort.as_ref().map(|effort| effort.as_str().into()),
		draft.cwd.as_str().into(),
	);
	config.sandbox = match draft.sandbox {
		ChiefSandboxDto::ReadOnly => "read-only",
		ChiefSandboxDto::WorkspaceWrite => "workspace-write",
		ChiefSandboxDto::FullAccess => "danger-full-access",
	}
	.into();
	if draft.sandbox == ChiefSandboxDto::FullAccess {
		config.approval_policy = json!("never");
	}
	config
}

fn now() -> i64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|t| t.as_micros().min(i64::MAX as u128) as i64)
		.unwrap_or(0)
}

fn steer_input_error(error: ChiefError) -> ChiefHostError {
	match error {
		ChiefError::InputNotSent(_) => ChiefHostError::Rejected(
			"Input was not sent: the local connection refused this request. Your draft is preserved.",
		),
		ChiefError::Rejected(_) => ChiefHostError::Rejected(
			"Task references could not be accepted in this turn. Your draft is preserved; refresh the task and its selected references.",
		),
		ChiefError::Invalid(ref message)
			if message.starts_with("The running turn changed")
				|| message.starts_with("Steer was rejected:") =>
			ChiefHostError::Rejected(
				"The running turn changed or rejected this input. Your draft is preserved; refresh before sending again.",
			),
		_ => ChiefHostError::Unknown(
			"Steer acceptance could not be confirmed. Inspect the conversation before sending again.",
		),
	}
}

fn question_input_error(error: ChiefError) -> ChiefHostError {
	match error {
		ChiefError::InputNotSent(_) | ChiefError::Transport(ClientError::StaleHistory) =>
			ChiefHostError::Rejected(
				"Question reply was not sent. Your draft is preserved; refresh the question before sending again.",
			),
		_ => ChiefHostError::Unknown(
			"Question reply acceptance could not be confirmed. Inspect the current conversation before sending again.",
		),
	}
}

fn resource_error(error: ChiefError) -> ChiefHostError {
	match error {
		ChiefError::Rejected(_) | ChiefError::Transport(ClientError::Remote(_)) =>
			ChiefHostError::Rejected(
				"The resource change was not accepted. Check the link and current task resources.",
			),
		_ => ChiefHostError::Unknown(
			"The resource change could not be confirmed. Refresh task resources before trying again.",
		),
	}
}

#[cfg(test)]
pub(crate) async fn queue_start_for_native_test(
	store: &SqliteStore,
	action: ChiefActionDto,
) -> ChiefConfig {
	let wire = serde_json::to_vec(&action).expect("public creation wire");
	let (action, options) =
		normalize_input(serde_json::from_slice(&wire).expect("public creation decode"))
			.expect("normalize public start");
	let ChiefActionDto::Start(start) = action else { panic!("start action") };
	let config = config(&start);
	ChiefCoordinator::reserve_root(store, start.root_id.as_str(), start.prompt.as_str())
		.await
		.expect("reserve root");
	store
		.bind_chief_root_settings(
			start.root_id.as_str(),
			&serde_json::to_string(&config).expect("root settings"),
		)
		.await
		.expect("persist root settings");
	persist_input(
		store,
		start.root_id.as_str(),
		"native-start",
		start.prompt.as_str(),
		options.as_ref(),
	)
	.await
	.expect("persist first input");
	config
}

#[cfg(test)]
mod tests {
	#[tokio::test]
	async fn request_liveness_rejects_reused_rpc_identity_after_reconnect() {
		use decodex_codex::app_server_client::AppServerClient;
		let params = serde_json::json!({"threadId":"thread","turnId":"turn","command":"pwd"});
		let mut retained = None;
		for _ in 0..2 {
			let (send, incoming) = tokio::sync::mpsc::channel(8);
			let (outgoing, _sent) = tokio::sync::mpsc::channel(8);
			let (client, mut events) = AppServerClient::from_framed(1, incoming, outgoing).unwrap();
			let mut payload = serde_json::json!({"id":7,"method":"item/commandExecution/requestApproval","params":params});
			send.send(Ok(payload.clone())).await.unwrap();
			events.recv().await.unwrap();
			assert!(!super::request_is_live_on(&client, &payload));
			payload["connectionId"] = serde_json::json!(client.connection_identity());
			assert!(super::request_is_live_on(&client, &payload));
			assert!(super::request_is_live_on(&client.clone(), &payload));
			if let Some(old) = retained {
				assert!(!super::request_is_live_on(&client, &old));
			}
			retained = Some(payload.clone());
			send.send(Ok(serde_json::json!({"method":"serverRequest/resolved","params":{"threadId":"thread","requestId":7}}))).await.unwrap();
			events.recv().await.unwrap();
			assert!(!super::request_is_live_on(&client, &payload));
		}
	}
	use super::*;
	use decodex_core::DecodexRoot;

	#[test]
	fn input_rejection_requires_released_durable_claims_not_just_a_transport_code() {
		for classify in [steer_input_error, question_input_error] {
			assert!(matches!(
				classify(ChiefError::InputNotSent(
					decodex_database::ChiefDispatchRefusal::RequestQueueFull
				)),
				ChiefHostError::Rejected(_)
			));
			for error in [
				ClientError::RequestTooLarge,
				ClientError::RequestQueueFull,
				ClientError::FrameTooLarge,
				ClientError::CapacityExceeded,
				ClientError::Closed,
			] {
				assert!(
					matches!(classify(ChiefError::Transport(error)), ChiefHostError::Unknown(_)),
					"a transport error alone does not rule out earlier context writes"
				);
			}
		}
	}

	#[test]
	fn expected_transport_close_does_not_hide_real_processing_failures() {
		assert!(!event_failure_needs_attention(true, &ChiefError::Transport(ClientError::Closed)));
		assert!(!event_failure_needs_attention(true, &ChiefError::Transport(ClientError::Io)));
		assert!(event_failure_needs_attention(false, &ChiefError::Transport(ClientError::Closed)));
		assert!(event_failure_needs_attention(
			true,
			&ChiefError::Store("fence write failed".into())
		));
		assert!(event_failure_needs_attention(true, &ChiefError::Invalid("bad evidence".into())));
	}

	#[tokio::test]
	async fn timer_restores_disconnected_owner_without_user_input() {
		let mut schedule = RecoverySchedule::new();
		let mut active = None;
		let deadline = schedule.next;
		schedule.restore_if_due(&mut active, deadline, async { Some("original-owner") }).await;
		assert_eq!(active, Some("original-owner"));
		// An attached owner cannot be replaced by another timer tick.
		schedule
			.restore_if_due(&mut active, schedule.next, async {
				panic!("must not reconnect a live owner")
			})
			.await;
		assert_eq!(active, Some("original-owner"));
	}

	#[tokio::test]
	async fn failed_recovery_is_rate_limited_and_success_resets_backoff() {
		let mut schedule = RecoverySchedule::new();
		let mut active = None::<()>;
		for expected_delay in [30, 60, 60, 60] {
			let due = schedule.next;
			schedule
				.restore_if_due(&mut active, due - Duration::from_nanos(1), async {
					panic!("must not retry before the recovery deadline")
				})
				.await;
			let started = tokio::time::Instant::now();
			schedule.restore_if_due(&mut active, due, async { None }).await;
			assert!(active.is_none());
			assert_eq!(schedule.delay, Duration::from_secs(expected_delay));
			assert!(schedule.next >= started + schedule.delay);
		}
		schedule.restore_if_due(&mut active, schedule.next, async { Some(()) }).await;
		assert!(active.is_some());
		assert_eq!(schedule.delay, RECOVERY_MIN_DELAY);
	}

	#[tokio::test]
	async fn stop_cancels_pending_timer_recovery() {
		let (sender, mut receiver) = watch::channel(false);
		let actor = tokio::spawn(async move {
			let mut schedule = RecoverySchedule::new();
			let mut active = None::<()>;
			tokio::select! {
				_ = stopped(&mut receiver) => {},
				_ = schedule.restore_if_due(
					&mut active, schedule.next, std::future::pending()
				) => panic!("pending recovery completed"),
			}
		});
		sender.send(true).unwrap();
		tokio::time::timeout(Duration::from_secs(1), actor).await.unwrap().unwrap();
	}

	#[tokio::test]
	async fn requested_account_survives_restart_before_any_process_binding() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		ChiefCoordinator::reserve_root(&store, "chief", "Coordinate").await.unwrap();
		let config = ChiefConfig::new("gpt-6-astra".into(), "medium".into(), "/tmp".into());
		let mut settings = serde_json::to_value(&config).unwrap();
		settings["account_id"] = json!("00000000-0000-4000-8000-000000000001");
		store.bind_chief_root_settings("chief", &settings.to_string()).await.unwrap();
		drop(store);
		let store = SqliteStore::open(&root.paths()).unwrap();
		let (_, account) =
			decode_settings(&store.read_chief_root_settings("chief").await.unwrap().unwrap())
				.unwrap();
		assert_eq!(account.unwrap().as_str(), "00000000-0000-4000-8000-000000000001");
		assert!(store.read_chief_process_binding("chief").await.unwrap().is_none());
		assert!(decode_settings(&serde_json::to_string(&config).unwrap()).unwrap().1.is_none());
		settings["account_id"] = json!("invalid");
		assert!(decode_settings(&settings.to_string()).is_none());
	}

	#[tokio::test]
	async fn lost_or_delayed_acceptance_is_never_a_definite_rejection() {
		let (sender, receiver) = oneshot::channel();
		drop(sender);
		assert!(matches!(
			await_acceptance(receiver, Duration::from_secs(1)).await,
			Err(ChiefHostError::Unknown(_))
		));
		let (_sender, receiver) = oneshot::channel();
		assert!(matches!(
			await_acceptance(receiver, Duration::from_millis(10)).await,
			Err(ChiefHostError::Unknown(_))
		));
		let (sender, receiver) = oneshot::channel();
		sender.send(Err(ChiefHostError::Rejected("invalid identity"))).unwrap();
		assert!(matches!(
			await_acceptance(receiver, Duration::from_secs(1)).await,
			Err(ChiefHostError::Rejected("invalid identity"))
		));
	}

	#[tokio::test]
	async fn event_stream_eof_preserves_uncertain_dispatch() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		ChiefCoordinator::reserve_root(&store, "chief", "Coordinate").await.unwrap();
		store.bind_chief_thread("chief".into(), "opaque-thread".into()).await.unwrap();
		store.begin_chief_dispatch("chief".into()).await.unwrap();
		let (io, _server) = tokio::io::duplex(4096);
		let (reader, writer) = tokio::io::split(io);
		let (client, _) =
			decodex_codex::app_server_client::AppServerClient::from_io(reader, writer);
		let coordinator = ChiefCoordinator::new(
			store.clone(),
			client,
			ChiefConfig::new("gpt-6-astra".into(), "medium".into(), "/tmp".into()),
		)
		.unwrap();
		let (sender, events) = mpsc::channel(1);
		drop(sender);
		let mut active = Some(("chief".into(), coordinator, events));
		let event = receive(&mut active).await.unwrap();
		assert!(matches!(event, ServerEvent::Closed(ClientError::Closed)));
		let error = active.as_mut().unwrap().1.handle_event(event).await.unwrap_err();
		assert!(!event_failure_needs_attention(true, &error));
		assert_eq!(
			store.get_chief_work_item("chief".into()).await.unwrap().dispatch_state,
			decodex_database::ChiefDispatchState::Unknown
		);
	}

	#[tokio::test]
	async fn occupied_conversation_rejects_input_without_queuing() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		ChiefCoordinator::reserve_root(&store, "chief", "Coordinate").await.unwrap();
		store.record_chief_thread_in_use("chief".into(), "Open elsewhere".into()).await.unwrap();
		assert!(persist_input(&store, "chief", "blocked", "Do this", None).await.is_err());
		assert!(
			!store
				.list_pending_chief_events(100)
				.await
				.unwrap()
				.iter()
				.any(|e| e.event_kind == "user_message")
		);
		store.resolve_chief_delivery_failure("chief".into()).await.unwrap();
		persist_input(&store, "chief", "new-send", "Do this", None).await.unwrap();
		assert_eq!(
			store
				.list_pending_chief_events(100)
				.await
				.unwrap()
				.iter()
				.filter(|e| e.event_kind == "user_message")
				.count(),
			1
		);
	}

	#[tokio::test]
	async fn accepted_input_survives_restart_before_process_attach() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();
		ChiefCoordinator::reserve_root(&store, "personal-chief", "Original input").await.unwrap();
		let config = ChiefConfig::new("gpt-6-astra".into(), "medium".into(), "/tmp".into());
		store
			.bind_chief_root_settings("personal-chief", &serde_json::to_string(&config).unwrap())
			.await
			.unwrap();
		persist_input(&store, "personal-chief", "start-command", "Original input", None)
			.await
			.unwrap();
		drop(store);
		let store = SqliteStore::open(&root.paths()).unwrap();
		// A retried command cannot duplicate the crash-surviving input.
		persist_input(&store, "personal-chief", "start-command", "Original input", None)
			.await
			.unwrap();
		let events = store.list_undelivered_chief_events(20).await.unwrap();
		assert_eq!(events.len(), 1);
		assert_eq!(events[0].event_kind, "user_message");
		assert_eq!(
			serde_json::from_str::<serde_json::Value>(&events[0].payload).unwrap()["text"],
			"Original input"
		);
		assert!(store.read_chief_process_binding("personal-chief").await.unwrap().is_none());
	}

	#[tokio::test]
	async fn stop_cancels_pending_actor_operation() {
		let (sender, mut receiver) = watch::channel(false);
		let actor = tokio::spawn(async move {
			tokio::select! {
				_ = stopped(&mut receiver) => {},
				_ = std::future::pending::<()>() => panic!("pending operation completed"),
			}
		});
		sender.send(true).unwrap();
		tokio::time::timeout(Duration::from_secs(1), actor).await.unwrap().unwrap();
	}

	#[tokio::test]
	async fn stop_ignores_false_updates_and_accepts_owner_drop() {
		let (sender, mut receiver) = watch::channel(false);
		sender.send(false).unwrap();
		assert!(
			tokio::time::timeout(Duration::from_millis(10), stopped(&mut receiver)).await.is_err()
		);
		drop(sender);
		tokio::time::timeout(Duration::from_secs(1), stopped(&mut receiver)).await.unwrap();
	}
}

#[path = "chief_weather.rs"] mod weather;
