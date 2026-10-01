//! Single service-owned Agent actor. The existing Conversation runtime owns its account process.

#[path = "agent_prompt_edit_host.rs"] mod prompt_edit;
#[path = "agent_prompt_upload_host.rs"] mod prompt_upload;
#[path = "agent_recap/host.rs"] mod recap;

use std::{
	fmt::{Display, Formatter},
	future::{self, Future},
	path::Path,
	sync::Arc,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
	sync::{Mutex, mpsc, oneshot, watch},
	time::{self, MissedTickBehavior},
};

use crate::{
	AgentConfig, AgentCoordinator, AgentError,
	account_observation::AccountObservationService,
	agent::{AgentInputExtras, misalignment, native_subagents, timeline, timeline::media},
	agent_app_exposure, agent_capabilities, agent_detail, agent_hooks, agent_integrations,
	agent_live_settings, agent_model_settings, agent_models, agent_native_goal, agent_permissions,
	agent_recap::Recaps,
	agent_resources, agent_search_settings, agent_skills,
	agent_transcript::Transcripts,
	agent_usage_estimate::{self, SourceKey},
	agent_voice, agent_voice_settings,
	conversation::{ConversationRuntime, StartAgentProcess},
	dictation::DictationGateway,
	mcp_login::{self, McpLoginGateway},
	native_agents, native_config_warning, native_diagnostics,
};
use decodex_codex::app_server_client::{AppServerClient, ClientError, ServerEvent};
use decodex_core::AccountId;
use decodex_database::{
	AgentDispatchState, AgentMisalignment, AgentWorkStatus, EnqueueAgentEvent, SqliteStore,
	StoreError,
};
use decodex_protocol::{
	AgentActionDto, AgentActivityDetailCursor, AgentAppExposureResult, AgentArchiveResult,
	AgentAttachmentDto, AgentCapabilitiesResult, AgentHookSettingsState, AgentIntegrationsResult,
	AgentLiveReviewerState, AgentMediaRequest, AgentMediaResult, AgentModelSelectionState,
	AgentModelSettingsResult, AgentNativeGoalResult, AgentPermissionState, AgentRequestedDecision,
	AgentResourcesResult, AgentSandboxDto, AgentSearchSettingsResult, AgentSkillsResult,
	AgentStartDto, AgentToolExposureSurface, AgentTranscriptRequest, AgentTranscriptResult,
	AgentUsageEstimateResult, AgentVoicePhase, AgentVoiceRequest, AgentVoiceSettingsResult,
	AgentVoiceStatus, DictationRequest, DictationStatus, HistoryText, McpLoginPhase,
	McpLoginRequest, McpLoginStatus, NativeProcessDiagnostics,
};
use prompt_edit::Reviews;
use weather::CachedWeather;

type Reply = oneshot::Sender<Result<String, AgentHostError>>;

const COMMAND_DEADLINE: Duration = Duration::from_secs(60);
const RECOVERY_MIN_DELAY: Duration = Duration::from_secs(15);
const RECOVERY_MAX_DELAY: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub(crate) enum AgentHostError {
	Rejected(&'static str),
	Unknown(&'static str),
}
impl Display for AgentHostError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Rejected(message) | Self::Unknown(message) => formatter.write_str(message),
		}
	}
}

impl From<&'static str> for AgentHostError {
	fn from(message: &'static str) -> Self {
		Self::Rejected(message)
	}
}

#[derive(Clone)]
pub(crate) struct AgentHost {
	skill_roots: Result<crate::agent_skill_roots::RuntimeSkillRoots, &'static str>,
	observations: Option<AccountObservationService>,
	recovery_cursor: Arc<Mutex<Option<String>>>,
	prompt_edits: Reviews,
	recaps: Recaps,
	transcripts: Transcripts,
	weather_cache: Arc<Mutex<Option<CachedWeather>>>,
	voice: crate::agent_voice::VoiceGateway,
	dictation: DictationGateway,
	mcp_login: McpLoginGateway,
	store: SqliteStore,
	runtime: ConversationRuntime,
	sender: mpsc::Sender<Request>,
	receiver: Arc<Mutex<Option<mpsc::Receiver<Request>>>>,
}
impl AgentHost {
	pub(crate) fn new(store: SqliteStore, runtime: ConversationRuntime) -> Self {
		let (sender, receiver) = mpsc::channel(32);

		Self {
			observations: None,
			skill_roots: crate::agent_skill_roots::RuntimeSkillRoots::from_environment(),
			recovery_cursor: Default::default(),
			voice: crate::agent_voice::VoiceGateway::new(),
			recaps: Default::default(),
			transcripts: Default::default(),
			prompt_edits: Default::default(),
			weather_cache: Arc::new(Mutex::new(None)),
			dictation: Default::default(),
			mcp_login: Default::default(),
			store,
			runtime,
			sender,
			receiver: Arc::new(Mutex::new(Some(receiver))),
		}
	}

	pub(crate) fn with_observations(
		mut self,
		observations: Option<AccountObservationService>,
	) -> Self {
		self.observations = observations;

		self
	}

	async fn recover_models(
		&self,
		agent: &AgentCoordinator,
		events: &mpsc::Receiver<ServerEvent>,
	) -> Result<(), AgentError> {
		let Some(observations) = &self.observations else {
			return Ok(());
		};

		if !events.is_empty() {
			return Ok(());
		}

		let mut items: Vec<_> = self
			.store
			.list_agent_work_items()
			.await?
			.into_iter()
			.filter(|work| {
				work.status != AgentWorkStatus::Resolved
					&& work.dispatch_state == AgentDispatchState::Idle
					&& work.codex_thread_id.is_some()
			})
			.collect();

		items.sort_by(|a, b| a.id.cmp(&b.id));

		let previous = self.recovery_cursor.lock().await.clone();
		let next = items
			.iter()
			.position(|work| previous.as_ref().is_none_or(|last| work.id > *last))
			.unwrap_or(0);

		if !items.is_empty() {
			items.rotate_left(next);
		}

		if let Some(work) = items.first() {
			*self.recovery_cursor.lock().await = Some(work.id.clone());

			let thread = work.codex_thread_id.as_deref().expect("filtered native thread");

			time::timeout(
				Duration::from_secs(12),
				agent_models::recover_ordinary_model(
					&self.store,
					|| async {
						let source = self.timeline_source(&work.id, thread).await?;

						(agent.native_generation() == Some(&source.key.generation))
							.then_some(source)
					},
					|account, revision| async move { observations.recovery(&account, revision).await },
					events,
				),
			)
			.await
			.map_err(|_| AgentError::Invalid("model recovery observation timed out".into()))??;
		}

		Ok(())
	}

	pub(crate) fn voice(&self, request: &AgentVoiceRequest) -> AgentVoiceStatus {
		self.voice.exchange(request)
	}

	pub(crate) fn misalignment_review_token(&self, review: &AgentMisalignment) -> Option<String> {
		let (_, client) = self.runtime.agent_catalog_client()?;

		client.live_misalignment_review(&review.thread_id, &review.turn_id).and_then(
			|(error, guard)| {
				(misalignment::details(&error) == review.details_json)
					.then(|| misalignment::review_token(review, &guard))
					.flatten()
			},
		)
	}

	pub(crate) async fn dictation(&self, request: &DictationRequest) -> DictationStatus {
		self.dictation.exchange(request, self.runtime.agent_client()).await
	}

	pub(crate) async fn mcp_login(&self, request: &McpLoginRequest) -> McpLoginStatus {
		let unavailable = || {
			mcp_login::status(
				request,
				McpLoginPhase::Disconnected,
				"The selected task connection is unavailable.",
			)
		};
		let Some((generation, client)) = self.runtime.agent_catalog_client() else {
			return unavailable();
		};
		let Ok(work) = self.store.get_agent_work_item(request.work_id().as_str().into()).await
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
			.get_agent_work_item(request.work_id().as_str().into())
			.await
			.ok()
			.is_some_and(|work| work.codex_thread_id.as_deref() == Some(thread.as_str()));

		if still_owned
			&& self.runtime.agent_catalog_client().is_some_and(|(current, _)| current == generation)
		{
			result
		} else {
			unavailable()
		}
	}

	pub(crate) async fn resources(&self, work: &str) -> AgentResourcesResult {
		let Some((generation, client)) = self.runtime.agent_catalog_client() else {
			return AgentResourcesResult::Unavailable;
		};
		let Ok(owner) = self.store.get_agent_work_item(work.into()).await else {
			return AgentResourcesResult::Unavailable;
		};
		let Some(thread) = owner.codex_thread_id else {
			return AgentResourcesResult::Unavailable;
		};
		let result = agent_resources::read(&client, &thread).await;
		let still_owned = self
			.store
			.get_agent_work_item(work.into())
			.await
			.ok()
			.is_some_and(|owner| owner.codex_thread_id.as_deref() == Some(thread.as_str()));

		if still_owned
			&& self.runtime.agent_catalog_client().is_some_and(|(current, _)| current == generation)
		{
			result
		} else {
			AgentResourcesResult::Unavailable
		}
	}

	pub(crate) async fn archive_state(&self, work: &str) -> AgentArchiveResult {
		use decodex_codex::app_server_client::ThreadArchiveState as State;

		use decodex_protocol::AgentArchiveResult as Result;

		let Some((generation, client)) = self.runtime.agent_catalog_client() else {
			return Result::Unavailable;
		};
		let Ok(owner) = self.store.get_agent_work_item(work.into()).await else {
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
			.get_agent_work_item(work.into())
			.await
			.is_ok_and(|current| current.codex_thread_id.as_deref() == Some(thread.as_str()))
			|| !self
				.runtime
				.agent_catalog_client()
				.is_some_and(|(current, _)| current == generation)
		{
			return Result::Unavailable;
		}

		match observed {
			Ok(State::Active) => Result::Active { thread_id: thread },
			Ok(State::Archived) => Result::Archived { thread_id: thread },
			Ok(State::NotFound | State::Changed) => Result::Unconfirmed,
			Err(ClientError::Remote(e)) if e.code == -32_601 => Result::Unsupported,
			Err(ClientError::CapacityExceeded) => Result::CapacityExceeded,
			Err(_) => Result::Unavailable,
		}
	}

	pub(crate) fn guardian_generation(&self) -> Option<String> {
		self.runtime.agent_catalog_client().map(|(generation, _)| generation.as_str().to_owned())
	}

	pub(crate) async fn process_diagnostics(&self) -> NativeProcessDiagnostics {
		native_diagnostics::read(|| self.runtime.agent_catalog_client()).await
	}

	pub(crate) async fn runtime_source(&self) -> Option<decodex_protocol::EntityId> {
		let (generation, account, revision, client) = self.runtime.agent_usage_source().await?;
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

	pub(crate) async fn usage_estimate(&self, work: &str) -> AgentUsageEstimateResult {
		agent_usage_estimate::read(|| async {
			let (generation, account, revision, client) = self.runtime.agent_usage_source().await?;
			let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

			Some(crate::agent_usage_estimate::Source {
				key: SourceKey {
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

	pub(crate) async fn native_goal(&self, work: &str, thread: &str) -> AgentNativeGoalResult {
		agent_native_goal::read(
			&self.store,
			|| async {
				let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			thread,
		)
		.await
	}

	pub(crate) async fn hook_settings(&self, work: &str) -> AgentHookSettingsState {
		agent_hooks::read(&self.store, || async {
			let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

			self.timeline_source(work, &owner.codex_thread_id?).await
		})
		.await
	}

	async fn set_hook_setting(
		&self,
		work: &str,
		change: crate::agent_hooks::Selection<'_>,
	) -> Result<String, AgentHostError> {
		agent_hooks::write(
			&self.store,
			|| async {
				let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			change,
		)
		.await?;

		Ok(work.into())
	}

	pub(crate) async fn model_selection(&self, work: &str) -> AgentModelSelectionState {
		agent_models::read(&self.store, || async {
			let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

			self.timeline_source(work, &owner.codex_thread_id?).await
		})
		.await
	}

	async fn set_task_model(
		&self,
		work: &str,
		change: crate::agent_models::Change<'_>,
	) -> Result<String, AgentHostError> {
		agent_models::write(
			&self.store,
			|| async {
				let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			change,
		)
		.await?;

		Ok(work.into())
	}

	pub(crate) async fn permission_profiles(&self, work: &str) -> AgentPermissionState {
		agent_permissions::read(&self.store, || async {
			let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

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
	) -> Result<String, AgentHostError> {
		agent_permissions::write(
			&self.store,
			|| async {
				let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

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
	) -> AgentAppExposureResult {
		agent_app_exposure::read(
			&self.store,
			|| async {
				let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

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
		omit: Option<Vec<AgentToolExposureSurface>>,
		attempt: &str,
	) -> Result<String, AgentHostError> {
		let (work, connector, review) =
			(identity.0.as_str(), identity.1.as_str(), identity.2.as_str());
		let change = crate::agent_app_exposure::Change { connector, review, omit, attempt };

		agent_app_exposure::write(
			&self.store,
			|| async {
				let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

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
		include_models: bool,
	) -> AgentLiveReviewerState {
		agent_live_settings::read_options(&self.store, include_models, || async {
			let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

			self.timeline_source(work, &owner.codex_thread_id?).await
		})
		.await
	}

	async fn set_live_settings(
		&self,
		ids: (
			&decodex_protocol::EntityId,
			&decodex_protocol::EntityId,
			&decodex_protocol::WireText,
		),
		edit: crate::agent_live_settings::LiveEdit,
		key: &str,
	) -> Result<String, AgentHostError> {
		let (work, turn, review) = (ids.0.as_str(), ids.1.as_str(), ids.2.as_str());

		agent_live_settings::write(
			&self.store,
			|| async {
				let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

				self.timeline_source(work, &owner.codex_thread_id?).await
			},
			turn,
			review,
			edit,
			key,
		)
		.await?;

		Ok(work.into())
	}

	pub(crate) async fn prompt_input_directory(
		&self,
		work: &str,
		thread: &str,
	) -> Option<decodex_protocol::WireText> {
		let before = self.timeline_source(work, thread).await?;
		let directory = self.runtime.agent_input_directory(&before.key.generation)?;

		if !self
			.store
			.agent_thread_is_owned(
				work.into(),
				thread.into(),
				Some(before.key.generation.as_str().into()),
			)
			.await
			.ok()?
		{
			return None;
		}

		let after = self.timeline_source(work, thread).await?;

		if before.key != after.key || !Path::new(&directory).is_absolute() {
			return None;
		}

		decodex_protocol::WireText::new(directory).ok()
	}

	pub(crate) async fn model_settings(&self, work: &str) -> AgentModelSettingsResult {
		agent_model_settings::read(&self.store, || async {
			let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

			self.timeline_source(work, owner.codex_thread_id.as_deref()?).await
		})
		.await
	}

	async fn timeline_source(
		&self,
		work: &str,
		thread: &str,
	) -> Option<crate::agent_usage_estimate::Source> {
		let (generation, account, revision, client) = self.runtime.agent_usage_source().await?;
		let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

		if owner.codex_thread_id.as_deref() != Some(thread) {
			return None;
		}

		Some(crate::agent_usage_estimate::Source {
			key: SourceKey {
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
	) -> decodex_protocol::AgentTimelineResult {
		let mut result =
			timeline::read(Some(&self.store), || self.timeline_source(work, thread), cursor).await;

		if let decodex_protocol::AgentTimelineResult::Available { page, .. } = &mut result {
			self.enrich_timeline_weather(page).await;
		}

		result
	}

	async fn acknowledge_app_ui_call(
		&self,
		work: &str,
		operation: &str,
		reservation: i64,
	) -> Result<String, AgentHostError> {
		let receipt = self
			.store
			.agent_app_ui_call_receipt(work.into(), operation.into())
			.await
			.map_err(|_| AgentHostError::Unknown("App call receipt could not be read."))?
			.ok_or(AgentHostError::Rejected("App call receipt is unavailable."))?;

		if receipt.id != reservation || receipt.state != "unknown" {
			return Err(AgentHostError::Rejected("Review the exact unknown app call first."));
		}
		if !receipt.uncertainty_acknowledged
			&& !self
				.store
				.acknowledge_agent_app_ui_uncertainty(work.into(), reservation, operation.into())
				.await
				.map_err(|_| AgentHostError::Unknown("App call acknowledgment is unconfirmed."))?
		{
			return Err(AgentHostError::Unknown("Read the saved app call acknowledgment."));
		}

		Ok(work.into())
	}

	pub(crate) async fn transcript(
		&self,
		request: &AgentTranscriptRequest,
	) -> AgentTranscriptResult {
		self.transcripts
			.read(
				|| async {
					let work = request.work_id.as_str();
					let thread = request.thread_id.as_str();
					let owner = self.store.get_agent_work_item(work.into()).await.ok()?;
					let mut source =
						self.timeline_source(work, owner.codex_thread_id.as_deref()?).await?;

					if source.key.thread != thread {
						let child_owner =
							native_subagents::request_owner(&self.store, &source.client, thread)
								.await
								.ok()?;

						if child_owner.id != work {
							return None;
						}

						source.key.thread = thread.into();
					}

					Some(source)
				},
				request,
			)
			.await
	}

	pub(crate) async fn media(&self, request: &AgentMediaRequest) -> AgentMediaResult {
		media::read(
			|| self.timeline_source(request.work_id.as_str(), request.thread_id.as_str()),
			|key| self.runtime.agent_input_directory(&key.generation),
			request,
		)
		.await
	}

	pub(crate) async fn integrations(&self, work: &str) -> AgentIntegrationsResult {
		let Some((generation, client)) = self.runtime.agent_catalog_client() else {
			return AgentIntegrationsResult::Unavailable;
		};
		let Ok(owner) = self.store.get_agent_work_item(work.into()).await else {
			return AgentIntegrationsResult::Unavailable;
		};
		let Some(thread) = owner.codex_thread_id else {
			return AgentIntegrationsResult::Unavailable;
		};
		let result = agent_integrations::read(&client, &thread).await;
		let still_owned = self
			.store
			.get_agent_work_item(work.into())
			.await
			.ok()
			.is_some_and(|owner| owner.codex_thread_id.as_deref() == Some(thread.as_str()));

		if still_owned
			&& self.runtime.agent_catalog_client().is_some_and(|(current, _)| current == generation)
		{
			result
		} else {
			AgentIntegrationsResult::Unavailable
		}
	}

	pub(crate) async fn native_agents(
		&self,
		work: &str,
		thread: Option<&str>,
		cursor: Option<&str>,
	) -> decodex_protocol::NativeAgentsResult {
		let Some((generation, client)) = self.runtime.agent_catalog_client() else {
			return decodex_protocol::NativeAgentsResult::Unavailable;
		};
		let result = native_agents::read(&self.store, &client, work, thread, cursor).await;

		if self.runtime.agent_catalog_client().is_some_and(|(current, _)| current == generation) {
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
		cursor: Option<&AgentActivityDetailCursor>,
	) -> decodex_protocol::AgentActivityDetailResult {
		agent_detail::read_bound(
			|| async {
				let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

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
	) -> decodex_protocol::AgentActivityDetailResult {
		let Some(client) = self.runtime.agent_client() else {
			return decodex_protocol::AgentActivityDetailResult::Unavailable;
		};

		agent_detail::read_file_changes(&client, thread, turn, item).await
	}

	pub(crate) fn request_is_live(&self, payload: &Value) -> bool {
		self.runtime.agent_client().is_some_and(|client| request_is_live_on(&client, payload))
	}

	pub(crate) async fn capabilities(&self) -> AgentCapabilitiesResult {
		agent_capabilities::read_scoped(|| async {
			let (generation, account, revision, client) = self.runtime.agent_usage_source().await?;

			if !self.store.account_is_ready_at_revision(&account, revision).await.ok()? {
				return None;
			}

			Some(((generation, account, revision), client))
		})
		.await
	}

	pub(crate) async fn submit(
		&self,
		key: String,
		action: AgentActionDto,
	) -> Result<String, AgentHostError> {
		let (reply, result) = oneshot::channel();

		time::timeout(COMMAND_DEADLINE, self.sender.send(Request { key, action, reply }))
			.await
			.map_err(|_| AgentHostError::Rejected("Agent command queue is full"))?
			.map_err(|_| AgentHostError::Rejected("Agent service is stopped"))?;

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
			let mut tick = time::interval(Duration::from_secs(15));

			tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

			loop {
				tokio::select! {
					voice_request=voice_requests.recv()=> {
						if let Some(request)=voice_request {self.handle_voice(request,&mut active).await;}
					},
					request = requests.recv() => {
						let Some(request) = request else {break;};
						let history_edit = matches!(&request.action,AgentActionDto::PreparePromptEdit{..}|AgentActionDto::ConfirmPromptEdit{..}|AgentActionDto::ForkPromptEdit{..}|AgentActionDto::RecoverPromptFork{..}|AgentActionDto::RecoverPromptEdit{..}|AgentActionDto::AcknowledgePromptEditDraft{..}|AgentActionDto::UploadPromptInput{..}|AgentActionDto::CompletePromptInputUpload{..});

						if !history_edit && !matches!(&request.action, AgentActionDto::SendPromptInput { .. }) { self.rotate_exhausted(&mut active).await; }

						let suppress_wake = history_edit || matches!(&request.action,AgentActionDto::GenerateRecap{..}|AgentActionDto::CancelRecap{..});

						self.recaps.note_input(&request.action);

						let outcome = self.handle(request.key,request.action,&mut active).await;
						let _ = request.reply.send(outcome);

						if !suppress_wake && let Some((root,agent,_)) = active.as_mut() {
							self.record_delivery(root, agent.wake_pending().await).await;
						}
					},
					event = receive(&mut active) => {
						if let Some((root,agent,_)) = active.as_mut() {
							agent.pause_dispatch(self.runtime.agent_account_exhausted(root).await);

							let closed = event.is_none() || matches!(&event,Some(ServerEvent::Closed(_)));

							if closed { self.recaps.stop(); }

							let event = event.and_then(|event|self.recaps.route(event));

							if let Some(event)=event.as_ref() && let Some(generation)=agent.native_generation() {
								self.mcp_login.observe(generation,event).await;

								if let ServerEvent::Notification { method, params } = event
									&& matches!(method.as_str(), "configWarning" | "warning" | "mcpServer/startupStatus/updated")
									&& native_config_warning::record_notification(&self.store, root, generation, method, params).await.is_err() {
									self.record_error(root,"event_processing_failed").await;
								}
							}

							if closed && let Some(generation)=agent.native_generation() {self.mcp_login.disconnect(Some(generation)).await;}

							if let Some(event) = event
								&& let Err(error) = agent.handle_event(event).await
								&& event_failure_needs_attention(closed, &error) {
								self.record_error(root,"event_processing_failed").await;
							}

							if closed {
								let root = root.clone();
								let _ = self.runtime.close_agent_connection(&root).await;

								active = None;
								recovery = RecoverySchedule::new();
							}
						}
					},
					_ = tick.tick() => {
						self.dictation.expire().await;
						self.mcp_login.expire().await;

						if let Some(request)=self.voice.expire() {self.handle_voice(request,&mut active).await;}
						if let Some((root, agent, events)) = active.as_mut()
							&& self.recover_models(agent, events).await.is_err() {
							self.record_error(root, "recovery_needs_attention").await;
						}

						self.rotate_exhausted(&mut active).await;
						recovery.restore_if_due(
							&mut active, tokio::time::Instant::now(), self.restore()
						).await;

						if let Some((root,agent,_)) = active.as_mut() {
							self.record_delivery(root, agent.check_due_followups(now()).await).await;
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

		self.recaps.stop();
		requests.close();

		while let Ok(request) = requests.try_recv() {
			let _ = request.reply.send(Err(AgentHostError::Rejected("Agent service is stopped")));
		}
		// Attach can be cancelled before `active` is assigned. Close the persisted
		// root as well so its admitted process cannot escape the actor lifecycle.
		let root = match active {
			Some((root, _, _)) => Some(root),
			None => self.store.list_agent_work_items().await.ok().and_then(|items| {
				items.into_iter().find(|item| item.parent_goal_id.is_none()).map(|item| item.id)
			}),
		};

		if let Some(root) = root {
			let _ = self.runtime.close_agent_connection(&root).await;
		}
	}

	async fn handle_voice(
		&self,
		request: AgentVoiceRequest,
		active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) {
		let id = request.session_id().as_str().to_owned();
		let speech = matches!(&request, AgentVoiceRequest::Speak { .. });
		let result = match active.as_mut() {
			Some((_, agent, _)) =>
				time::timeout(Duration::from_secs(30), agent.voice_request(request))
					.await
					.unwrap_or_else(|_| {
						Err(AgentError::Invalid(
							"voice signaling timed out; do not replay input".into(),
						))
					}),
			None => Err(AgentError::Invalid("Agent is reconnecting".into())),
		};

		if let Err(error) = result {
			if speech {
				self.voice.notice(&id, "Read-aloud could not be confirmed. It was not retried.");

				return;
			}

			let detail = match error {
				AgentError::Transport(ClientError::Remote(error)) =>
					agent_voice::provider_error_message(&error.message).into(),
				AgentError::Invalid(message) => message,
				AgentError::Store(_) => "Voice session state could not be saved.".into(),
				_ => "Voice could not connect to this Agent. Check connection status.".into(),
			};

			self.voice.update(&id, AgentVoicePhase::Failed, None, Some(&detail));
		}
	}

	async fn rotate_exhausted(
		&self,
		active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) {
		let Some((root, agent, _)) = active.as_mut() else {
			return;
		};

		if self.dictation.active().await {
			return;
		}
		if self.store.open_agent_voice_calls().await.map_or(true, |calls| !calls.is_empty()) {
			return;
		}

		let exhausted = self.runtime.agent_account_exhausted(root).await;

		agent.pause_dispatch(exhausted);

		let Ok(work) = self.store.list_agent_work_items().await else {
			return;
		};

		if work.iter().any(|item| item.dispatch_state != AgentDispatchState::Idle) || !exhausted {
			return;
		}

		let root = root.clone();

		*active = None;
		// Existing process death must be positively established before the store permits
		// another account. No uncertain or active turn is replayed during this handover.
		if self.runtime.close_agent_connection(&root).await.is_ok() {
			*active = self.restore().await;
		}
	}

	async fn accept_message(
		&self,
		root_id: &decodex_protocol::EntityId,
		text: &HistoryText,
		key: &str,
		input_options: Option<&Value>,
		active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, AgentHostError> {
		let root = self
			.store
			.get_agent_work_item(root_id.as_str().into())
			.await
			.map_err(|_| "Agent root is unavailable")?;
		let managers =
			self.store.agent_manager_ids().await.map_err(|_| "Manager state unavailable")?;

		if !managers.contains(&root.id) {
			return Err("Agent identity differs".into());
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
	) -> Result<String, AgentHostError> {
		self.store
			.cancel_agent_capacity_retry(work_id.as_str().into(), event_id)
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
		active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, AgentHostError> {
		let (work, thread, question) =
			(identity.0.as_str(), identity.1.as_str(), identity.2.as_str());
		let (_, agent, _) = active.as_mut().ok_or("Agent is not connected")?;

		agent.skip_async_question(work, thread, question).await.map_err(|_| {
			AgentHostError::Rejected(
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
	) -> Result<String, AgentHostError> {
		let (work, thread) = ids;
		let client = self.runtime.agent_client().ok_or("Agent connection unavailable")?;
		let result = native_agents::read(&self.store, &client, work, Some(thread), None).await;
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
				AgentHostError::Rejected(
					"Message was not sent: the local connection refused this request. Your draft is preserved.",
				),
			_ => AgentHostError::Unknown(
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
		active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, AgentHostError> {
		let review = self
			.store
			.agent_misalignment(work_id.as_str().into())
			.await
			.map_err(|_| "Provider findings unavailable")?
			.ok_or("Provider precaution is no longer current")?;

		if self.misalignment_review_token(&review).as_deref() != Some(review_id.as_str()) {
			return Err("Provider findings changed; review them again".into());
		}

		let (_, agent, _) = active.as_mut().ok_or("Agent is not connected")?;

		agent.continue_misalignment(work_id.as_str(),review,key,review_id.as_str()).await.map_err(|error| match error { AgentError::Rejected(_) => AgentHostError::Rejected("Continuation was rejected or the findings changed. Review the latest findings before trying again."), _ => AgentHostError::Unknown("Continuation was not confirmed. Inspect the latest conversation state before trying again.") })?;

		Ok(work_id.as_str().into())
	}

	pub(crate) async fn voice_settings(&self, work: &str) -> AgentVoiceSettingsResult {
		agent_voice_settings::read(|| async {
			let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

			self.timeline_source(work, owner.codex_thread_id.as_deref()?).await
		})
		.await
	}

	async fn set_voice_preference(
		&self,
		work: &str,
		review: &str,
		voice: &str,
	) -> Result<String, AgentHostError> {
		agent_voice_settings::write(
			|| async {
				let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

				self.timeline_source(work, owner.codex_thread_id.as_deref()?).await
			},
			review,
			voice,
		)
		.await?;

		Ok(work.into())
	}

	pub(crate) fn skill_roots(&self) -> Option<crate::agent_skill_roots::RuntimeSkillRoots> {
		self.skill_roots.as_ref().ok().cloned()
	}

	pub(crate) async fn skills(&self, work: &str, filter: &str) -> AgentSkillsResult {
		agent_skills::read(
			|| async {
				let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

				self.timeline_source(work, owner.codex_thread_id.as_deref()?).await
			},
			filter,
		)
		.await
	}

	pub(crate) async fn search_settings(&self, work: &str) -> AgentSearchSettingsResult {
		agent_search_settings::read(|| async {
			let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

			self.timeline_source(work, owner.codex_thread_id.as_deref()?).await
		})
		.await
	}

	async fn set_search_preference(
		&self,
		work: &str,
		review: &str,
		mode: &str,
	) -> Result<String, AgentHostError> {
		agent_search_settings::write(
			|| async {
				let owner = self.store.get_agent_work_item(work.into()).await.ok()?;

				self.timeline_source(work, owner.codex_thread_id.as_deref()?).await
			},
			review,
			mode,
		)
		.await?;

		Ok(work.into())
	}

	async fn handle_settings(
		&self,
		key: &str,
		action: AgentActionDto,
	) -> Result<String, AgentHostError> {
		match action {
			AgentActionDto::SetSavedAppSetting { .. } | AgentActionDto::SetAppSetting { .. } =>
				Err(AgentHostError::Rejected(
					"Configure connector approvals in Codex for this account.",
				)),
			AgentActionDto::EditNativeGoal { work_id, thread_id, review_token, edit } => {
				agent_native_goal::write(
					&self.store,
					|| async {
						let owner =
							self.store.get_agent_work_item(work_id.as_str().into()).await.ok()?;

						self.timeline_source(work_id.as_str(), owner.codex_thread_id.as_deref()?)
							.await
					},
					thread_id.as_str(),
					review_token.as_str(),
					&edit,
				)
				.await?;

				Ok(work_id.as_str().into())
			},
			AgentActionDto::SetVoicePreference { work_id, review_token, voice } =>
				self.set_voice_preference(work_id.as_str(), review_token.as_str(), voice.as_str())
					.await,
			AgentActionDto::SetSearchPreference { work_id, review_token, mode } =>
				self.set_search_preference(work_id.as_str(), review_token.as_str(), mode.as_str())
					.await,
			AgentActionDto::AcknowledgeAppUiCall { work_id, operation_id, reservation_id } =>
				self.acknowledge_app_ui_call(
					work_id.as_str(),
					operation_id.as_str(),
					reservation_id,
				)
				.await,
			AgentActionDto::ConfirmAppUiTool { .. } =>
				Err(AgentHostError::Rejected("Embedded app views are no longer supported.")),
			AgentActionDto::SetAppToolExposure { work_id, connector_id, review_token, omit } =>
				self.set_app_tool_exposure((&work_id, &connector_id, &review_token), omit, key)
					.await,
			AgentActionDto::SetHookSetting {
				work_id,
				thread_id,
				review_token,
				hook_key,
				change,
			} =>
				self.set_hook_setting(
					work_id.as_str(),
					crate::agent_hooks::Selection {
						thread: thread_id.as_str(),
						review: review_token.as_str(),
						hook: hook_key.as_str(),
						change,
						attempt_id: key,
					},
				)
				.await,
			AgentActionDto::SetTaskPlugin { .. } =>
				Err(AgentHostError::Rejected("Configure plugins in Codex for this account.")),
			AgentActionDto::SetTaskModel { work_id, thread_id, review_token, model, effort } =>
				self.set_task_model(
					work_id.as_str(),
					crate::agent_models::Change {
						thread: thread_id.as_str(),
						review: review_token.as_str(),
						model: model.as_str(),
						effort: effort.as_ref().map(|e| e.as_str()),
						attempt_id: key,
					},
				)
				.await,
			AgentActionDto::SelectPermissions { work_id, thread_id, review_token, profile_id } =>
				self.select_permissions(
					work_id.as_str(),
					thread_id.as_str(),
					review_token.as_str(),
					profile_id.as_str(),
					key,
				)
				.await,
			AgentActionDto::SetLiveReviewer { work_id, turn_id, review_token, reviewer } =>
				self.set_live_settings((&work_id, &turn_id, &review_token), reviewer.into(), key)
					.await,
			AgentActionDto::SetLiveModel { work_id, turn_id, review_token, model, effort } =>
				self.set_live_settings(
					(&work_id, &turn_id, &review_token),
					crate::agent_live_settings::LiveEdit::Model { model, effort },
					key,
				)
				.await,
			_ => Err(AgentHostError::Rejected("Unsupported settings action.")),
		}
	}

	async fn handle(
		&self,
		key: String,
		action: AgentActionDto,
		active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, AgentHostError> {
		use decodex_protocol::AgentActionDto as Action;

		let (action, input_options) = normalize_input(action)?;

		match action {
			action @ Action::SendPromptInput { .. } =>
				self.send_prompt_input(&key, action, active).await,
			action @ (Action::UploadPromptInput { .. }
			| Action::CompletePromptInputUpload { .. }) => self.handle_prompt_upload(action).await,
			action @ (Action::PreparePromptEdit { .. }
			| Action::ConfirmPromptEdit { .. }
			| Action::ForkPromptEdit { .. }
			| Action::RecoverPromptFork { .. }
			| Action::RecoverPromptEdit { .. }
			| Action::AcknowledgePromptEditDraft { .. }) =>
				self.handle_prompt_edit(&key, action, active.as_mut().map(|(_, agent, _)| agent))
					.await,
			action @ (Action::GenerateRecap { .. } | Action::CancelRecap { .. }) =>
				self.handle_recap(&key, action).await,
			action @ (Action::EditNativeGoal { .. }
			| Action::SetVoicePreference { .. }
			| Action::SetSearchPreference { .. }
			| Action::SetAppToolExposure { .. }
			| Action::ConfirmAppUiTool { .. }
			| Action::AcknowledgeAppUiCall { .. }
			| Action::SetSavedAppSetting { .. }
			| Action::SetAppSetting { .. }
			| Action::SetHookSetting { .. }
			| Action::SetTaskPlugin { .. }
			| Action::SetTaskModel { .. }
			| Action::SelectPermissions { .. }
			| Action::SetLiveReviewer { .. }
			| Action::SetLiveModel { .. }) => self.handle_settings(key.as_str(), action).await,
			Action::NativeAgentInput { work_id, thread_id, text, expected_turn } =>
				self.native_agent_input(
					(work_id.as_str(), thread_id.as_str()),
					text.as_str(),
					expected_turn.as_ref().map(|turn| turn.as_str()),
				)
				.await,
			Action::InstallSuggestedPlugin { .. } =>
				Err(AgentHostError::Rejected("Install plugins in Codex for this account.")),
			Action::RestoreArchivedThread { work_id, thread_id } => {
				let (_, agent, _) = active.as_mut().ok_or("Agent is not connected")?;

				agent.restore_archived_thread(work_id.as_str(),thread_id.as_str()).await.map_err(|error|match error {
                    AgentError::Rejected(_)=>AgentHostError::Rejected("Restoration was not accepted. Refresh the task archive state before trying again."),
                    _=>AgentHostError::Unknown("Restoration is not confirmed. Refresh archive state; the restore request will not be repeated automatically."),
                })?;

				Ok(work_id.as_str().into())
			},
			Action::RefreshIntegrations { .. } =>
				Err(AgentHostError::Rejected("Update plugins and connections in Codex.")),
			action @ (Action::AddResourceLink { .. } | Action::RemoveResource { .. }) =>
				Self::handle_resource_change(action, active).await,

			Action::StartConfigured { .. } | Action::SendConfigured { .. } =>
				unreachable!("normalized input"),
			action @ Action::Steer { .. } => Self::steer_action(action, &key, active).await,
			Action::ContinueMisalignment { work_id, review_id } =>
				self.continue_reviewed_misalignment(work_id, review_id, &key, active).await,

			Action::ApproveGuardianDenial { work_id, review_row, review_digest } => {
				let (_, agent, _) = active.as_mut().ok_or("Agent is not connected")?;

				agent.approve_guardian_denial(work_id.as_str(),review_row,review_digest.as_str(),&key).await
					.map_err(|error| match error {
						AgentError::Rejected(_) => AgentHostError::Rejected("Approval was rejected or the review is no longer current. Refresh the review before trying again."),
						_ => AgentHostError::Unknown("Approval submission was not confirmed. It will not be sent again automatically."),
					})?;

				Ok(work_id.as_str().into())
			},
			Action::AnswerQuestion { work_id, question_id, answer } => {
				let (_, agent, _) = active.as_mut().ok_or("Agent is not connected")?;

				agent
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
			Action::SkipQuestion { work_id, thread_id, question_id } =>
				Self::skip_question((&work_id, &thread_id, &question_id), active).await,
			Action::CancelCapacityRetry { work_id, event_id } =>
				self.cancel_capacity_retry(work_id, event_id).await,
			Action::Respond { work_id, event_id, response_json } =>
				self.respond(work_id.as_str(), event_id, response_json.as_str(), active).await,
			Action::RespondWithRequestedDecision { work_id, event_id, decision } =>
				self.respond_requested(work_id.as_str(), event_id, &decision, active).await,
			Action::Start(draft) => self.start(draft, &key, input_options.as_ref(), active).await,
			Action::Send { root_id, text } =>
				self.accept_message(&root_id, &text, &key, input_options.as_ref(), active).await,
			Action::Interrupt { work_id, turn_id } => {
				let (_, agent, _) = active.as_mut().ok_or("Agent is not connected")?;

				agent.interrupt_work(work_id.as_str(), turn_id.as_str()).await.map_err(|_| {
					AgentHostError::Unknown(
						"exact work turn interrupt could not be confirmed; refresh state",
					)
				})?;

				Ok(work_id.as_str().into())
			},
			Action::AutomationResult { work_id, source_event_id, payload } =>
				self.accept_automation_result(work_id, source_event_id, payload, active).await,
		}
	}

	async fn handle_resource_change(
		action: AgentActionDto,
		active: &Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, AgentHostError> {
		use decodex_protocol::AgentActionDto as Action;

		match action {
			Action::AddResourceLink { work_id, title, url } => {
				let (_, agent, _) = active.as_ref().ok_or("Agent is not connected")?;

				agent
					.add_resource_link(work_id.as_str(), title.as_str(), url.as_str())
					.await
					.map_err(resource_error)?;

				Ok(work_id.as_str().into())
			},
			Action::RemoveResource { work_id, attachment_type, identity_key } => {
				let (_, agent, _) = active.as_ref().ok_or("Agent is not connected")?;

				agent
					.remove_resource(
						work_id.as_str(),
						attachment_type.as_str(),
						identity_key.as_str(),
					)
					.await
					.map_err(resource_error)?;

				Ok(work_id.as_str().into())
			},
			_ => unreachable!("resource action"),
		}
	}

	async fn accept_automation_result(
		&self,
		work_id: decodex_protocol::EntityId,
		source_event_id: decodex_protocol::WireText,
		payload: HistoryText,
		active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, AgentHostError> {
		self.store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: json!(["automation", source_event_id.as_str()]).to_string(),
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
	}

	async fn steer_action(
		action: AgentActionDto,
		key: &str,
		active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, AgentHostError> {
		let AgentActionDto::Steer { work_id, turn_id, text, attachments, task_references } = action
		else {
			unreachable!("steer action")
		};

		validate_attachments(&attachments)?;

		let (_, agent, _) = active.as_mut().ok_or("Agent is not connected")?;

		agent
			.steer_work_with_references(
				work_id.as_str(),
				turn_id.as_str(),
				key,
				text.as_str(),
				AgentInputExtras { attachments: &attachments, task_references: &task_references },
			)
			.await
			.map_err(steer_input_error)?;

		Ok(work_id.as_str().into())
	}

	async fn respond_requested(
		&self,
		work: &str,
		event_id: i64,
		decision: &AgentRequestedDecision,
		active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, AgentHostError> {
		let event = self
			.store
			.get_agent_inbox_event(event_id)
			.await
			.map_err(|_| "pending request is unavailable; refresh state")?;

		if event.work_item_id != work || event.disposition.is_some() {
			return Err("request identity or state changed; refresh state".into());
		}

		let payload: Value =
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
		active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, AgentHostError> {
		let event = self
			.store
			.get_agent_inbox_event(event_id)
			.await
			.map_err(|_| "pending request is unavailable; refresh state")?;

		if event.work_item_id != work
			|| event.disposition.is_some()
			|| !["permission_pending", "user_input_pending", "server_request_pending"]
				.contains(&event.event_kind.as_str())
		{
			return Err("request identity or state changed; refresh state".into());
		}

		let response: Value =
			serde_json::from_str(response_json).map_err(|_| "response must be valid JSON")?;

		if !response.is_object() {
			return Err("response must be a JSON object".into());
		}

		let (_, agent, _) =
			active.as_mut().ok_or("Agent is not connected; stale requests cannot be replayed")?;

		agent.respond_pending_event(event_id, response).await.map_err(|error| {
			if matches!(error, AgentError::Rejected(_)) {
				return AgentHostError::Rejected(
					"The response does not match the current provider request. Review the form before submitting.",
				);
			}

			AgentHostError::Unknown(
				"request response could not be confirmed; refresh state before retrying",
			)
		})?;

		Ok(work.into())
	}

	async fn start(
		&self,
		draft: AgentStartDto,
		key: &str,
		input_options: Option<&Value>,
		active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, AgentHostError> {
		let root = draft.root_id.as_str().to_owned();
		let account_id = draft
			.account_id
			.as_ref()
			.map(|id| AccountId::new(id.as_str()))
			.transpose()
			.map_err(|_| "invalid account identity")?;

		if active.as_ref().is_some_and(|(current, _, _)| current != &root) {
			return Err("another Agent is active".into());
		}

		let config = config(&draft);

		AgentCoordinator::reserve_root(&self.store, &root, draft.prompt.as_str())
			.await
			.map_err(|_| "Agent root could not be reserved")?;

		let mut settings =
			serde_json::to_value(&config).map_err(|_| "invalid Agent configuration")?;

		if let Some(account) = &draft.account_id {
			settings["account_id"] = json!(account.as_str());
		}

		let encoded =
			serde_json::to_string(&settings).map_err(|_| "invalid Agent configuration")?;

		self.store
			.bind_agent_root_settings(&root, &encoded)
			.await
			.map_err(|_| "Agent configuration differs from its saved execution context")?;
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

	async fn record_delivery(&self, root: &str, result: Result<(), AgentError>) {
		match result {
			Ok(()) => {
				let _ = self.store.resolve_agent_delivery_failure(root.into()).await;
			},
			Err(AgentError::InputNotSent(_)) => {
				// The retained input already has a user-decision receipt. Do not block
				// its editor with a false connection failure or requeue it.
				let _ = self.store.resolve_agent_delivery_failure(root.into()).await;
			},
			Err(AgentError::ThreadOwnedElsewhere) => {
				let _ = self.store.hold_agent_unsent_input(root.into()).await;
				let _ = self
					.store
					.record_agent_thread_in_use(
						root.into(),
						diagnostic(&AgentError::ThreadOwnedElsewhere),
					)
					.await;
			},
			Err(error) => {
				let _ =
					self.store.record_agent_delivery_failure(root.into(), diagnostic(&error)).await;
			},
		}
	}

	async fn record_error(&self, root: &str, kind: &str) {
		if kind == "reconnection_needs_attention" {
			let _ = self
				.store
				.record_agent_connection_failure(
					root.into(),
					"Inspect account and process readiness.".into(),
				)
				.await;

			return;
		}

		let _ = self
			.store
			.enqueue_agent_event(EnqueueAgentEvent {
				source_event_id: json!(["agent_host", root, kind]).to_string(),
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
		config: AgentConfig,
		account_id: Option<AccountId>,
	) -> Result<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>), &'static str> {
		let skill_roots = self.skill_roots.as_ref().map_err(|error| *error)?;
		let connection = match self
			.runtime
			.open_agent_connection(StartAgentProcess {
				operation_key,
				root_id: root.into(),
				working_directory: config.cwd.clone(),
				account_id,
			})
			.await
		{
			Ok(connection) => connection,
			Err(error) => {
				// AgentLaunchError contains only typed, credential-negative readiness facts.
				let _ = self
					.store
					.record_agent_connection_failure(root.into(), error.to_string())
					.await;

				return Err(
					"Agent account process is unavailable; inspect account and process readiness",
				);
			},
		};
		let binding = match self.store.read_agent_process_binding(root).await {
			Ok(binding) => binding,
			Err(_) => {
				let _ = self.runtime.close_agent_connection(root).await;

				return Err("Agent process binding is unavailable");
			},
		};

		if !binding.is_some_and(|binding| {
			binding.account_id == connection.account_id
				&& binding.generation_id == connection.process_generation_id
		}) {
			let _ = self.runtime.close_agent_connection(root).await;

			return Err("Agent process binding did not match the admitted account");
		}

		if let Err(message) = skill_roots.apply(&connection.client).await {
			let _ = self.runtime.close_agent_connection(root).await;
			let _ = self.store.record_agent_connection_failure(root.into(), message.into()).await;

			return Err(message);
		}

		let mut coordinator =
			match AgentCoordinator::new(self.store.clone(), connection.client, config) {
				Ok(coordinator) => coordinator,
				Err(_) => {
					let _ = self.runtime.close_agent_connection(root).await;

					return Err("invalid Agent configuration");
				},
			};

		coordinator.bind_native_generation(connection.process_generation_id.clone());
		coordinator.attach_voice_host(
			connection.process_generation_id.as_str().into(),
			self.voice.clone(),
		);

		if self.store.resolve_agent_connection_failure(root.into()).await.is_err() {
			let _ = self.runtime.close_agent_connection(root).await;

			return Err("Agent connection recovery receipt could not be saved");
		}

		Ok((root.into(), coordinator, connection.events))
	}

	async fn restore(&self) -> Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)> {
		let root = self
			.store
			.list_agent_work_items()
			.await
			.ok()?
			.into_iter()
			.find(|work| work.parent_goal_id.is_none())?;
		let settings = match self.store.read_agent_root_settings(&root.id).await {
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
		restore: impl Future<Output = Option<T>>,
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
	action: AgentActionDto,
	reply: Reply,
}

#[cfg(test)]
pub(crate) async fn queue_start_for_native_test(
	store: &SqliteStore,
	action: AgentActionDto,
) -> AgentConfig {
	let wire = serde_json::to_vec(&action).expect("public creation wire");
	let (action, options) =
		normalize_input(serde_json::from_slice(&wire).expect("public creation decode"))
			.expect("normalize public start");
	let AgentActionDto::Start(start) = action else { panic!("start action") };
	let config = config(&start);

	AgentCoordinator::reserve_root(store, start.root_id.as_str(), start.prompt.as_str())
		.await
		.expect("reserve root");

	store
		.bind_agent_root_settings(
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

fn request_is_live_on(client: &AppServerClient, payload: &Value) -> bool {
	if payload["connectionId"].as_str() != Some(client.connection_identity()) {
		return false;
	}

	let Ok(id) = serde_json::from_value(payload["id"].clone()) else { return false };
	let Some(method) = payload["method"].as_str() else { return false };

	client.server_request_guard(&id, method, &payload["params"]).is_some()
}

fn diagnostic(error: &AgentError) -> String {
	match error {
        AgentError::ThreadArchived => "This conversation is archived. Unarchive it to continue. Your saved messages remain queued.".into(),
		AgentError::ThreadOwnedElsewhere => "This Agent conversation is open in Codex or another application. Sending is unavailable here while it is in use. Your history remains readable.".into(),
		AgentError::Store(_) => "Agent delivery could not access its saved state.".into(),
		AgentError::DependenciesPending(_) => "Agent is waiting for prerequisite work.".into(),
		_ => error.to_string(),
	}
}

fn event_failure_needs_attention(closed: bool, error: &AgentError) -> bool {
	// Closed deliberately returns its transport error after saving dispatch fences.
	// A failed fence/store operation is still a real processing failure.
	!closed || !matches!(error, AgentError::Transport(_))
}

fn decode_settings(encoded: &str) -> Option<(AgentConfig, Option<AccountId>)> {
	let settings: Value = serde_json::from_str(encoded).ok()?;
	let config = serde_json::from_value::<AgentConfig>(settings.clone()).ok()?;
	let account = match settings.get("account_id") {
		None => None,
		Some(value) => Some(AccountId::new(value.as_str()?).ok()?),
	};

	Some((config, account))
}

fn normalize_input(
	action: AgentActionDto,
) -> Result<(AgentActionDto, Option<Value>), AgentHostError> {
	let normalized = match action {
		AgentActionDto::StartConfigured { start, execution, attachments, task_references } => {
			validate_attachments(&attachments)?;

			(
				AgentActionDto::Start(start),
				Some(
					json!({"execution":execution,"attachments":attachments,"taskReferences":task_references}),
				),
			)
		},
		AgentActionDto::SendConfigured {
			root_id,
			text,
			execution,
			attachments,
			task_references,
		} => {
			validate_attachments(&attachments)?;

			(
				AgentActionDto::Send { root_id, text },
				Some(
					json!({"execution":execution,"attachments":attachments,"taskReferences":task_references}),
				),
			)
		},
		other => (other, None),
	};

	Ok(normalized)
}

fn validate_attachments(files: &[AgentAttachmentDto]) -> Result<(), &'static str> {
	if files.len() > 16 {
		return Err("Select at most 16 files, folders or skills");
	}

	for file in files {
		if let Some(name) = &file.skill_name
			&& (file.image
				|| name.as_str().trim().is_empty()
				|| name.as_str().chars().any(char::is_control))
		{
			return Err("The selected skill reference is invalid");
		}

		let path = Path::new(file.path.as_str());

		if file.skill_name.is_some() && !path.is_file() {
			return Err("The selected skill is no longer available");
		}
		if !path.is_absolute() || !(path.is_file() || (!file.image && path.is_dir())) {
			return Err("An attached file or folder is no longer available");
		}
	}

	Ok(())
}

fn config(draft: &AgentStartDto) -> AgentConfig {
	let mut config = AgentConfig::with_optional_effort(
		draft.model.as_str().into(),
		draft.effort.as_ref().map(|effort| effort.as_str().into()),
		draft.cwd.as_str().into(),
	);

	config.sandbox = match draft.sandbox {
		AgentSandboxDto::ReadOnly => "read-only",
		AgentSandboxDto::WorkspaceWrite => "workspace-write",
		AgentSandboxDto::FullAccess => "danger-full-access",
	}
	.into();

	if draft.sandbox == AgentSandboxDto::FullAccess {
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

fn steer_input_error(error: AgentError) -> AgentHostError {
	match error {
		AgentError::InputNotSent(_) => AgentHostError::Rejected(
			"Input was not sent: the local connection refused this request. Your draft is preserved.",
		),
		AgentError::Rejected(_) => AgentHostError::Rejected(
			"Task references could not be accepted in this turn. Your draft is preserved; refresh the task and its selected references.",
		),
		AgentError::Invalid(ref message)
			if message.starts_with("The running turn changed")
				|| message.starts_with("Steer was rejected:") =>
			AgentHostError::Rejected(
				"The running turn changed or rejected this input. Your draft is preserved; refresh before sending again.",
			),
		_ => AgentHostError::Unknown(
			"Steer acceptance could not be confirmed. Inspect the conversation before sending again.",
		),
	}
}

fn question_input_error(error: AgentError) -> AgentHostError {
	match error {
		AgentError::InputNotSent(_) | AgentError::Transport(ClientError::StaleHistory) =>
			AgentHostError::Rejected(
				"Question reply was not sent. Your draft is preserved; refresh the question before sending again.",
			),
		_ => AgentHostError::Unknown(
			"Question reply acceptance could not be confirmed. Inspect the current conversation before sending again.",
		),
	}
}

fn resource_error(error: AgentError) -> AgentHostError {
	match error {
		AgentError::Rejected(_) | AgentError::Transport(ClientError::Remote(_)) =>
			AgentHostError::Rejected(
				"The resource change was not accepted. Check the link and current task resources.",
			),
		_ => AgentHostError::Unknown(
			"The resource change could not be confirmed. Refresh task resources before trying again.",
		),
	}
}

async fn await_acceptance(
	result: oneshot::Receiver<Result<String, AgentHostError>>,
	deadline: Duration,
) -> Result<String, AgentHostError> {
	time::timeout(deadline, result)
		.await
		.map_err(|_| {
			AgentHostError::Unknown(
				"Agent command acceptance is unknown; inspect persisted work before retrying",
			)
		})?
		.map_err(|_| AgentHostError::Unknown("Agent command acceptance is unknown"))?
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

async fn persist_input(
	store: &SqliteStore,
	root: &str,
	key: &str,
	text: &str,
	options: Option<&Value>,
) -> Result<(), &'static str> {
	store
  .enqueue_agent_event(EnqueueAgentEvent {
			source_event_id: json!(["user_message", root, key]).to_string(),
			work_item_id: root.into(),
			event_kind: "user_message".into(),
			payload: json!({"text":text,"source":"user","asyncQuestionReply":decodex_protocol::parse_agent_async_question_replies(text).is_some(),"options":options}).to_string(),
		})
		.await
		.map_err(|error| match error {
			StoreError::AgentThreadInUse =>
				"This conversation is in use in another app. No message was queued.",
			_ => "Agent input could not be accepted",
		})?;

	Ok(())
}

async fn receive(
	active: &mut Option<(String, AgentCoordinator, mpsc::Receiver<ServerEvent>)>,
) -> Option<ServerEvent> {
	match active {
		Some((_, _, events)) =>
			Some(events.recv().await.unwrap_or(ServerEvent::Closed(ClientError::Closed))),
		None => future::pending().await,
	}
}

#[cfg(test)]
mod tests {
	use std::{fs, future};

	use tokio::{io, time};

	use crate::agent_host::{
		self, AgentConfig, AgentCoordinator, AgentError, AgentHostError, ClientError, Duration,
		EnqueueAgentEvent, RECOVERY_MIN_DELAY, RecoverySchedule, ServerEvent, SqliteStore, mpsc,
		oneshot, question_input_error, steer_input_error, watch,
	};
	use decodex_codex::app_server_client::AppServerClient;
	use decodex_core::DecodexRoot;

	#[test]
	fn directory_references_remain_paths_and_cannot_be_sent_as_images() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("folder.png");

		fs::create_dir(&path).unwrap();

		let mut reference = decodex_protocol::AgentAttachmentDto {
			path: decodex_protocol::ConversationWorkingDirectory::new(path.to_str().unwrap())
				.unwrap(),
			image: false,
			skill_name: None,
		};

		assert!(super::validate_attachments(&[reference.clone()]).is_ok());

		reference.image = true;

		assert!(super::validate_attachments(&[reference.clone()]).is_err());

		reference.image = false;

		fs::remove_dir(&path).unwrap();

		assert!(super::validate_attachments(&[reference]).is_err());
	}

	#[test]
	fn input_rejection_requires_released_durable_claims_not_just_a_transport_code() {
		for classify in [steer_input_error, question_input_error] {
			assert!(matches!(
				classify(AgentError::InputNotSent(
					decodex_database::AgentDispatchRefusal::RequestQueueFull
				)),
				AgentHostError::Rejected(_)
			));

			for error in [
				ClientError::RequestTooLarge,
				ClientError::RequestQueueFull,
				ClientError::FrameTooLarge,
				ClientError::CapacityExceeded,
				ClientError::Closed,
			] {
				assert!(
					matches!(classify(AgentError::Transport(error)), AgentHostError::Unknown(_)),
					"a transport error alone does not rule out earlier context writes"
				);
			}
		}
	}

	#[test]
	fn expected_transport_close_does_not_hide_real_processing_failures() {
		assert!(!agent_host::event_failure_needs_attention(
			true,
			&AgentError::Transport(ClientError::Closed)
		));
		assert!(!agent_host::event_failure_needs_attention(
			true,
			&AgentError::Transport(ClientError::Io)
		));
		assert!(agent_host::event_failure_needs_attention(
			false,
			&AgentError::Transport(ClientError::Closed)
		));
		assert!(agent_host::event_failure_needs_attention(
			true,
			&AgentError::Store("fence write failed".into())
		));
		assert!(agent_host::event_failure_needs_attention(
			true,
			&AgentError::Invalid("bad evidence".into())
		));
	}

	#[tokio::test]
	async fn request_liveness_rejects_reused_rpc_identity_after_reconnect() {
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
				_ = agent_host::stopped(&mut receiver) => {},
				_ = schedule.restore_if_due(
					&mut active, schedule.next, future::pending()
				) => panic!("pending recovery completed"),
			}
		});

		sender.send(true).unwrap();

		time::timeout(Duration::from_secs(1), actor).await.unwrap().unwrap();
	}

	#[tokio::test]
	async fn requested_account_survives_restart_before_any_process_binding() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();

		AgentCoordinator::reserve_root(&store, "agent", "Coordinate").await.unwrap();

		let config = AgentConfig::new("gpt-6-astra".into(), "medium".into(), "/tmp".into());
		let mut settings = serde_json::to_value(&config).unwrap();

		settings["account_id"] = agent_host::json!("00000000-0000-4000-8000-000000000001");

		store.bind_agent_root_settings("agent", &settings.to_string()).await.unwrap();

		drop(store);

		let store = SqliteStore::open(&root.paths()).unwrap();
		let (_, account) = agent_host::decode_settings(
			&store.read_agent_root_settings("agent").await.unwrap().unwrap(),
		)
		.unwrap();

		assert_eq!(account.unwrap().as_str(), "00000000-0000-4000-8000-000000000001");
		assert!(store.read_agent_process_binding("agent").await.unwrap().is_none());
		assert!(
			agent_host::decode_settings(&serde_json::to_string(&config).unwrap())
				.unwrap()
				.1
				.is_none()
		);

		settings["account_id"] = agent_host::json!("invalid");

		assert!(agent_host::decode_settings(&settings.to_string()).is_none());
	}

	#[tokio::test]
	async fn lost_or_delayed_acceptance_is_never_a_definite_rejection() {
		let (sender, receiver) = oneshot::channel();

		drop(sender);

		assert!(matches!(
			agent_host::await_acceptance(receiver, Duration::from_secs(1)).await,
			Err(AgentHostError::Unknown(_))
		));

		let (_sender, receiver) = oneshot::channel();

		assert!(matches!(
			agent_host::await_acceptance(receiver, Duration::from_millis(10)).await,
			Err(AgentHostError::Unknown(_))
		));

		let (sender, receiver) = oneshot::channel();

		sender.send(Err(AgentHostError::Rejected("invalid identity"))).unwrap();

		assert!(matches!(
			agent_host::await_acceptance(receiver, Duration::from_secs(1)).await,
			Err(AgentHostError::Rejected("invalid identity"))
		));
	}

	#[tokio::test]
	async fn event_stream_eof_preserves_uncertain_dispatch() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();

		AgentCoordinator::reserve_root(&store, "agent", "Coordinate").await.unwrap();

		store.bind_agent_thread("agent".into(), "opaque-thread".into()).await.unwrap();
		store.begin_agent_dispatch("agent".into()).await.unwrap();

		let (io, _server) = io::duplex(4_096);
		let (reader, writer) = io::split(io);
		let (client, _) =
			decodex_codex::app_server_client::AppServerClient::from_io(reader, writer);
		let coordinator = AgentCoordinator::new(
			store.clone(),
			client,
			AgentConfig::new("gpt-6-astra".into(), "medium".into(), "/tmp".into()),
		)
		.unwrap();
		let (sender, events) = mpsc::channel(1);

		drop(sender);

		let mut active = Some(("agent".into(), coordinator, events));
		let event = agent_host::receive(&mut active).await.unwrap();

		assert!(matches!(event, ServerEvent::Closed(ClientError::Closed)));

		let error = active.as_mut().unwrap().1.handle_event(event).await.unwrap_err();

		assert!(!agent_host::event_failure_needs_attention(true, &error));
		assert_eq!(
			store.get_agent_work_item("agent".into()).await.unwrap().dispatch_state,
			decodex_database::AgentDispatchState::Unknown
		);
	}

	#[tokio::test]
	async fn occupied_conversation_rejects_input_without_queuing() {
		let directory = tempfile::tempdir().unwrap();
		let root = DecodexRoot::new(directory.path().canonicalize().unwrap()).unwrap();
		let store = SqliteStore::open(&root.paths()).unwrap();

		AgentCoordinator::reserve_root(&store, "agent", "Coordinate").await.unwrap();

		for index in 0..1_000 {
			store
				.enqueue_agent_event(EnqueueAgentEvent {
					source_event_id: format!("older-diagnostic-{index}"),
					work_item_id: "agent".into(),
					event_kind: "diagnostic".into(),
					payload: "{}".into(),
				})
				.await
				.unwrap();
		}

		store.record_agent_thread_in_use("agent".into(), "Open elsewhere".into()).await.unwrap();

		assert!(
			agent_host::persist_input(&store, "agent", "blocked", "Do this", None).await.is_err()
		);
		assert!(
			!store
				.list_agent_wake_events("agent".into(), 100)
				.await
				.unwrap()
				.iter()
				.any(|e| e.event_kind == "user_message")
		);

		store.resolve_agent_delivery_failure("agent".into()).await.unwrap();

		agent_host::persist_input(&store, "agent", "new-send", "Do this", None).await.unwrap();

		assert_eq!(
			store
				.list_agent_wake_events("agent".into(), 100)
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

		AgentCoordinator::reserve_root(&store, "personal-agent", "Original input").await.unwrap();

		let config = AgentConfig::new("gpt-6-astra".into(), "medium".into(), "/tmp".into());

		store
			.bind_agent_root_settings("personal-agent", &serde_json::to_string(&config).unwrap())
			.await
			.unwrap();

		agent_host::persist_input(
			&store,
			"personal-agent",
			"start-command",
			"Original input",
			None,
		)
		.await
		.unwrap();

		drop(store);

		let store = SqliteStore::open(&root.paths()).unwrap();
		// A retried command cannot duplicate the crash-surviving input.
		agent_host::persist_input(
			&store,
			"personal-agent",
			"start-command",
			"Original input",
			None,
		)
		.await
		.unwrap();

		let events = store.list_undelivered_agent_events(20).await.unwrap();

		assert_eq!(events.len(), 1);
		assert_eq!(events[0].event_kind, "user_message");
		assert_eq!(
			serde_json::from_str::<serde_json::Value>(&events[0].payload).unwrap()["text"],
			"Original input"
		);
		assert!(store.read_agent_process_binding("personal-agent").await.unwrap().is_none());
	}

	#[tokio::test]
	async fn stop_cancels_pending_actor_operation() {
		let (sender, mut receiver) = watch::channel(false);
		let actor = tokio::spawn(async move {
			tokio::select! {
				_ = agent_host::stopped(&mut receiver) => {},
				_ = future::pending::<()>() => panic!("pending operation completed"),
			}
		});

		sender.send(true).unwrap();

		time::timeout(Duration::from_secs(1), actor).await.unwrap().unwrap();
	}

	#[tokio::test]
	async fn stop_ignores_false_updates_and_accepts_owner_drop() {
		let (sender, mut receiver) = watch::channel(false);

		sender.send(false).unwrap();

		assert!(
			time::timeout(Duration::from_millis(10), agent_host::stopped(&mut receiver))
				.await
				.is_err()
		);

		drop(sender);

		time::timeout(Duration::from_secs(1), agent_host::stopped(&mut receiver)).await.unwrap();
	}
}
#[path = "agent_weather.rs"] mod weather;
