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
	voice: crate::chief_voice::VoiceGateway,
	dictation: crate::dictation::DictationGateway,
	store: SqliteStore,
	runtime: ConversationRuntime,
	sender: mpsc::Sender<Request>,
	receiver: Arc<Mutex<Option<mpsc::Receiver<Request>>>>,
}

impl ChiefHost {
	pub(crate) fn new(store: SqliteStore, runtime: ConversationRuntime) -> Self {
		let (sender, receiver) = mpsc::channel(32);
		Self {
			voice: crate::chief_voice::VoiceGateway::new(),
			dictation: Default::default(),
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

	pub(crate) async fn activity_detail(
		&self,
		work: &str,
		turn: &str,
		item: &str,
	) -> decodex_protocol::ChiefActivityDetailResult {
		let unavailable = decodex_protocol::ChiefActivityDetailResult::Unavailable;
		let Some(client) = self.runtime.chief_client() else {
			return unavailable;
		};
		let Ok(work) = self.store.get_chief_work_item(work.into()).await else {
			return unavailable;
		};
		let Some(thread) = work.codex_thread_id else {
			return unavailable;
		};
		crate::chief_detail::read(&client, &thread, turn, item).await
	}

	pub(crate) async fn capabilities(&self) -> decodex_protocol::ChiefCapabilitiesResult {
		let Some(client) = self.runtime.chief_client() else {
			return decodex_protocol::ChiefCapabilitiesResult::Unavailable;
		};
		crate::chief_capabilities::read(&client).await
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

	async fn handle(
		&self,
		key: String,
		action: ChiefActionDto,
		active: &mut Option<(String, ChiefCoordinator, mpsc::Receiver<ServerEvent>)>,
	) -> Result<String, ChiefHostError> {
		let (action, input_options) = normalize_input(action)?;

		match action {
			ChiefActionDto::StartConfigured { .. } | ChiefActionDto::SendConfigured { .. } =>
				unreachable!("normalized input"),
			ChiefActionDto::Steer { work_id, turn_id, text, attachments } => {
				validate_attachments(&attachments)?;
				let (_, chief, _) = active.as_mut().ok_or("Chief is not connected")?;
				chief.steer_work(work_id.as_str(),turn_id.as_str(),&key,text.as_str(),&attachments).await.map_err(|error| match error {
					ChiefError::Invalid(ref message) if message.starts_with("The running turn changed") || message.starts_with("Steer was rejected:") => ChiefHostError::Rejected("The running turn changed or rejected this input. Your draft is preserved; refresh before sending again."),
					_ => ChiefHostError::Unknown("Steer acceptance could not be confirmed. Inspect the conversation before sending again."),
				})?;
				Ok(work_id.as_str().into())
			},
			ChiefActionDto::CancelCapacityRetry { work_id, event_id } =>
				self.cancel_capacity_retry(work_id, event_id).await,
			ChiefActionDto::Respond { work_id, event_id, response_json } => {
				let event = self
					.store
					.get_chief_inbox_event(event_id)
					.await
					.map_err(|_| "pending request is unavailable; refresh state")?;
				if event.work_item_id != work_id.as_str()
					|| event.disposition.is_some()
					|| !["permission_pending", "user_input_pending", "server_request_pending"]
						.contains(&event.event_kind.as_str())
				{
					return Err("request identity or state changed; refresh state".into());
				}
				let response: serde_json::Value = serde_json::from_str(response_json.as_str())
					.map_err(|_| "response must be valid JSON")?;
				if !response.is_object() {
					return Err("response must be a JSON object".into());
				}
				let (_, chief, _) = active
					.as_mut()
					.ok_or("Chief is not connected; stale requests cannot be replayed")?;
				chief.respond_pending_event(event_id, response).await.map_err(|_| {
					ChiefHostError::Unknown(
						"request response could not be confirmed; refresh state before retrying",
					)
				})?;
				Ok(work_id.as_str().into())
			},
			ChiefActionDto::Start(draft) => {
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
				persist_input(
					&self.store,
					&root,
					&key,
					draft.prompt.as_str(),
					input_options.as_ref(),
				)
				.await?;
				if active.is_none() {
					match self.connect(&root, key.clone(), config, account_id).await {
						Ok(connection) => *active = Some(connection),
						Err(_) => {
							self.record_error(&root, "reconnection_needs_attention").await;
						},
					}
				}
				Ok(root)
			},
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

	async fn record_delivery(&self, root: &str, result: Result<(), ChiefError>) {
		match result {
			Ok(()) => {
				let _ = self.store.resolve_chief_delivery_failure(root.into()).await;
			},
			Err(ChiefError::ThreadOwnedElsewhere) => {
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
		ChiefError::ThreadOwnedElsewhere => "This Chief conversation is open in Codex or another application. Release it there; saved messages will continue automatically.".into(),
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
		ChiefActionDto::StartConfigured { start, execution, attachments } => {
			validate_attachments(&attachments)?;
			(
				ChiefActionDto::Start(start),
				Some(json!({"execution":execution,"attachments":attachments})),
			)
		},
		ChiefActionDto::SendConfigured { root_id, text, execution, attachments } => {
			validate_attachments(&attachments)?;
			(
				ChiefActionDto::Send { root_id, text },
				Some(json!({"execution":execution,"attachments":attachments})),
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
	store
		.enqueue_chief_event(EnqueueChiefEvent {
			source_event_id: json!(["user_message", root, key]).to_string(),
			work_item_id: root.into(),
			event_kind: "user_message".into(),
			payload: json!({"text":text,"source":"user","options":options}).to_string(),
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
	let mut config = ChiefConfig::new(
		draft.model.as_str().into(),
		draft.effort.as_str().into(),
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

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_core::DecodexRoot;

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
