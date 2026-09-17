//! Chief conversation and work overview. The service owns records and execution.

#[path = "chief_activity.rs"] mod activity;
#[path = "chief_tree.rs"] mod agent_tree;
#[path = "chief_capabilities.rs"] mod capabilities;
#[path = "chief_composer.rs"] mod composer;
#[path = "chief_detail.rs"] mod detail;
#[path = "chief_dictation.rs"] mod dictation;
#[path = "chief_graph.rs"] mod graph;
#[path = "chief_markdown.rs"] mod markdown;
#[path = "chief_progress.rs"] mod progress;
#[path = "chief_prompts.rs"] mod prompts;
#[path = "chief_requests.rs"] mod requests;
#[path = "chief_voice.rs"] mod voice;
#[path = "chief_workspace.rs"] mod workspace;
#[path = "chief_workspace_size.rs"] mod workspace_size;

use decodex_protocol::{
	ChiefActionDto, ChiefClient, ChiefCommandResponse, ChiefDispatchStateDto, ChiefHistoryResult,
	ChiefRequestResult, ChiefSandboxDto, ChiefSnapshotDto, ChiefSnapshotResult, ChiefStartDto,
	ChiefWorkItemDto, ChiefWorkStatusDto, ClientProfile, ConversationModel,
	ConversationReasoningEffort, ConversationWorkingDirectory, EntityId, HistoryText,
	IdempotencyKey, WireText,
};
use gpui::{
	ClipboardItem, Context, Entity, FocusHandle, FontWeight, Render, Role, SharedString, Task,
	Window, div, prelude::*, px, rgb, rgba,
};

use crate::{
	composer_input::{ComposerInput, SubmitComposer},
	ui_motion::{SmoothControl, disclosure},
	ui_theme,
};

#[derive(Clone, Debug, Eq, PartialEq)]
enum LoadState {
	Idle,
	Loading,
	Ready,
	Unavailable,
	Stale,
	Capacity { work: u64, edges: u64, events: u64 },
}

fn should_poll_snapshot(has_profile: bool, state: &LoadState, _active: bool) -> bool {
	has_profile && *state != LoadState::Loading
}

pub(crate) struct ChiefSurface {
	voice: Option<voice::VoiceUi>,
	voice_task: Option<Task<()>>,
	audio_inputs: Vec<String>,
	audio_input: String,
	dictation: Option<dictation::DictationUi>,
	dictation_task: Option<Task<()>>,
	activity_detail: Option<(String, Option<decodex_protocol::ChiefActivityDetailResult>)>,
	activity_detail_task: Option<Task<()>>,
	capabilities: Option<decodex_protocol::ChiefCapabilitiesResult>,
	capabilities_checked: Option<std::time::Instant>,
	capability_task: Option<Task<()>>,
	expanded_progress: std::collections::BTreeSet<String>,
	pages: Vec<String>,
	graph_visible: bool,
	graph_expanded: bool,
	page_views: std::collections::BTreeMap<String, workspace::PageView>,
	timeline_visible: bool,
	graph_scope: Option<String>,
	graph_selected: Option<String>,
	graph_zoom: f32,
	graph_display_zoom: f32,
	graph_pan: (f32, f32),
	graph_inset: (f32, f32),
	graph_drag: Option<gpui::Point<gpui::Pixels>>,
	history_marks: std::collections::BTreeMap<i64, activity::HistoryMark>,
	history_marks_work: Option<String>,
	history_hover: Option<usize>,
	history_navigation: Option<activity::HistoryNavigation>,
	agent_tree_visible: bool,
	agent_tree_collapsed: std::collections::BTreeSet<String>,
	sidebar_visible: bool,
	sidebar_width: f32,
	sidebar_drag: Option<(f32, f32)>,
	history_cache: std::collections::BTreeMap<String, ChiefHistoryResult>,
	transcript_scroll: std::collections::BTreeMap<String, gpui::ScrollHandle>,
	profile: Option<ClientProfile>,
	snapshot: Option<ChiefSnapshotDto>,
	state: LoadState,
	status_before_refresh: Option<LoadState>,
	selected: Option<String>,
	task: Option<Task<()>>,

	copy_focus: FocusHandle,
	generation: u64,
	composer: Entity<ComposerInput>,
	fast: bool,
	steer: bool,
	composer_menu: Option<&'static str>,
	composer_menu_content: Option<&'static str>,
	attachments: Vec<decodex_protocol::ChiefAttachmentDto>,
	attachment_drafts:
		std::collections::BTreeMap<String, Vec<decodex_protocol::ChiefAttachmentDto>>,
	manager_drafts: std::collections::BTreeMap<String, String>,
	composer_manager: Option<String>,
	model: Entity<ComposerInput>,
	cwd: Entity<ComposerInput>,
	account: Entity<ComposerInput>,
	effort: ConversationReasoningEffort,
	sandbox: ChiefSandboxDto,
	command_task: Option<Task<()>>,
	sending: bool,
	uncertain: bool,
	feedback: String,
	history: Option<(String, ChiefHistoryResult)>,
	history_task: Option<Task<()>>,
	history_requested_for: Option<String>,
	older_history: std::collections::BTreeMap<
		String,
		(Vec<decodex_protocol::ChiefHistoryEntryDto>, Option<i64>),
	>,
	older_task: Option<Task<()>>,
	loading_older: bool,
	older_scroll_anchor: Option<(String, f32, f32)>,
	poll_task: Option<Task<()>>,
	request: Option<ChiefRequestResult>,
	request_task: Option<Task<()>>,
	question_inputs: std::collections::BTreeMap<String, Entity<ComposerInput>>,
	details_visible: bool,
	accounts: Vec<(String, String)>,
	setup_expanded: bool,
}

impl ChiefSurface {
	#[cfg(feature = "visual-capture")]
	#[allow(dead_code, reason = "capture-only interaction proof shares the main module")]
	pub(crate) fn visual_prepare_send(
		&mut self,
		profile: ClientProfile,
		message: &str,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		use gpui::Focusable;
		self.profile = Some(profile);
		self.composer.update(cx, |input, cx| input.set_content(message, cx));
		window.focus(&self.composer.focus_handle(cx), cx);
	}

	#[cfg(feature = "visual-capture")]
	#[allow(dead_code, reason = "capture-only interaction proof shares the main module")]
	pub(crate) fn visual_send_evidence(&self, cx: &Context<Self>) -> serde_json::Value {
		serde_json::json!({"snapshot":self.snapshot,"history":self.history,"feedback":self.feedback,"sending":self.sending,"uncertain":self.uncertain,"draft":self.composer.read(cx).content()})
	}

	/// Render only protocol-read observations; the capture has no command profile.
	#[cfg(feature = "visual-capture")]
	#[allow(
		dead_code,
		reason = "the main binary shares this module and feature with the capture binary"
	)]
	pub(crate) fn visual_from_service(
		snapshot: ChiefSnapshotResult,
		selected: Option<String>,
		history: Option<ChiefHistoryResult>,
		request: Option<ChiefRequestResult>,
		cx: &mut Context<Self>,
	) -> Self {
		let mut surface = Self::new(cx);
		surface.request = request;
		surface.apply_result(Ok(snapshot));
		if let Some(selected) = selected {
			surface.selected = Some(selected);
		}
		if let (Some(selected), Some(history)) = (surface.selected.clone(), history) {
			surface.history = Some((selected, history));
		}
		surface.feedback = "Read-only capture of a disposable service projection".into();
		surface
	}

	pub(crate) fn new(cx: &mut Context<Self>) -> Self {
		Self::refresh_prompt(cx);
		let model =
			cx.new(|cx| ComposerInput::with_placeholder(31, "Exact model ID", "Chief model", cx));
		model.update(cx, |input, cx| input.set_content("gpt-6-astra", cx));
		let cwd = cx.new(|cx| {
			ComposerInput::with_placeholder(
				32,
				"Absolute working directory",
				"Chief working directory",
				cx,
			)
		});
		let composer =
			cx.new(|cx| ComposerInput::message(35, prompts::next(), "Chief message", cx));
		cx.subscribe(&composer, |s, _, event, cx| {
			if let crate::composer_input::ComposerEvent::Attach(item) = event {
				s.attach_clipboard(item, cx);
			}
			cx.notify();
		})
		.detach();
		Self {
			voice: None,
			voice_task: None,
			audio_inputs: Vec::new(),
			audio_input: String::new(),
			dictation: None,
			dictation_task: None,
			activity_detail: None,
			activity_detail_task: None,
			capabilities: None,
			capabilities_checked: None,
			capability_task: None,
			fast: false,
			steer: true,
			composer_menu: None,
			composer_menu_content: None,
			attachments: vec![],
			attachment_drafts: Default::default(),
			manager_drafts: Default::default(),
			composer_manager: None,
			pages: vec![],
			graph_visible: true,
			graph_expanded: false,
			page_views: Default::default(),
			timeline_visible: true,
			graph_scope: None,
			graph_selected: None,
			graph_zoom: 0.85,
			graph_display_zoom: 0.85,
			graph_pan: (0.0, 0.0),
			graph_inset: (0.0, 0.0),
			graph_drag: None,
			history_marks: Default::default(),
			history_marks_work: None,
			history_hover: None,
			history_navigation: None,
			agent_tree_visible: true,
			agent_tree_collapsed: Default::default(),
			sidebar_visible: true,
			sidebar_width: 192.0,
			sidebar_drag: None,
			history_cache: Default::default(),
			expanded_progress: Default::default(),
			transcript_scroll: Default::default(),
			details_visible: false,
			accounts: vec![],
			setup_expanded: false,
			composer,
			model,
			cwd,
			account: cx.new(|cx| {
				ComposerInput::with_placeholder(
					33,
					"Automatic account routing",
					"Optional exact account ID",
					cx,
				)
			}),
			effort: ConversationReasoningEffort::High,
			sandbox: ChiefSandboxDto::ReadOnly,
			command_task: None,
			sending: false,
			uncertain: false,
			feedback: String::new(),
			history: None,
			history_task: None,
			history_requested_for: None,
			older_history: Default::default(),
			older_task: None,
			loading_older: false,
			older_scroll_anchor: None,
			poll_task: None,
			request: None,
			request_task: None,
			question_inputs: Default::default(),
			profile: None,
			snapshot: None,
			state: LoadState::Idle,
			status_before_refresh: None,
			selected: None,
			task: None,

			copy_focus: cx.focus_handle().tab_index(22).tab_stop(true),
			generation: 0,
		}
	}

	fn load_history(&mut self, cx: &mut Context<Self>) {
		if self.history.as_ref().is_some_and(|(id, _)| self.selected.as_ref() != Some(id)) {
			self.history = None;
		}
		let (Some(profile), Some(id)) = (self.profile.clone(), self.selected.clone()) else {
			return;
		};
		if self.history_task.is_some() && self.history_requested_for.as_ref() == Some(&id) {
			return;
		}
		self.history_requested_for = Some(id.clone());
		let Ok(work_id) = EntityId::new(id.clone()) else {
			return;
		};
		let request = cx.background_executor().spawn(async move {
			let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build()
			else {
				return ChiefHistoryResult::Unavailable;
			};
			runtime
				.block_on(ChiefClient::new(profile).history(work_id))
				.unwrap_or(ChiefHistoryResult::Unavailable)
		});
		self.history_task = Some(cx.spawn(async move |surface, cx| {
			let history = request.await;
			let _ = surface.update(cx, |surface, cx| {
				surface.history_task = None;
				if surface.selected.as_ref() == Some(&id) {
					if matches!(history, ChiefHistoryResult::Available { .. })
						|| !surface.history.as_ref().is_some_and(|(current, saved)| {
							current == &id && matches!(saved, ChiefHistoryResult::Available { .. })
						}) {
						if let Some(scroll) = surface.transcript_scroll.get(&id)
							&& surface.voice.is_none()
							&& (scroll.offset().y + scroll.max_offset().y).abs() < px(24.0)
						{
							scroll.scroll_to_bottom();
						}
						surface.history_cache.insert(id.clone(), history.clone());
						surface.history = Some((id, history));
					}
					cx.notify();
				}
			});
		}));
	}

	pub(crate) fn seed_context(
		&mut self,
		cwd: Option<ConversationWorkingDirectory>,
		accounts: Vec<(String, String)>,
		cx: &mut Context<Self>,
	) {
		if self.cwd.read(cx).content().is_empty()
			&& let Some(cwd) = cwd
		{
			self.cwd.update(cx, |input, cx| input.set_content(cwd.as_str(), cx));
		}
		self.accounts = accounts;
		cx.notify();
	}

	fn cycle_model(&mut self, cx: &mut Context<Self>) {
		if let Some(decodex_protocol::ChiefCapabilitiesResult::Available { models, .. }) =
			&self.capabilities
		{
			if models.is_empty() {
				return;
			}
			let next = models
				.iter()
				.position(|model| model.model.as_str() == self.model.read(cx).content())
				.map_or(0, |index| (index + 1) % models.len());
			let model = models[next].model.as_str().to_owned();
			self.model.update(cx, |input, cx| input.set_content(&model, cx));
			self.reconcile_model_options(cx);
		} else {
			self.load_capabilities(cx);
		}
		cx.notify();
	}

	fn cycle_account(&mut self, cx: &mut Context<Self>) {
		let current = self.account.read(cx).content();
		let next = if current.is_empty() {
			self.accounts.first().map(|(id, _)| id.clone())
		} else {
			self.accounts
				.iter()
				.position(|(id, _)| id == current)
				.and_then(|index| self.accounts.get(index + 1))
				.map(|(id, _)| id.clone())
		};
		self.account.update(cx, |input, cx| input.set_content(next.as_deref().unwrap_or(""), cx));
		cx.notify();
	}

	fn load_request(&mut self, event_id: i64, cx: &mut Context<Self>) {
		let Some(profile) = self.profile.clone() else {
			return;
		};
		self.request = None;
		let selected = self.selected.clone();

		let request = cx.background_executor().spawn(async move {
			let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build()
			else {
				return ChiefRequestResult::Unavailable;
			};
			runtime
				.block_on(ChiefClient::new(profile).request(event_id))
				.unwrap_or(ChiefRequestResult::Unavailable)
		});
		self.request_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				surface.request_task = None;
				if surface.selected == selected {
					if let Some(scroll) =
						selected.as_ref().and_then(|id| surface.transcript_scroll.get(id))
						&& surface.voice.is_none()
						&& (scroll.offset().y + scroll.max_offset().y).abs() < px(24.0)
					{
						scroll.scroll_to_bottom();
					}
					surface.prepare_question_inputs(&result, cx);
					surface.request = Some(result);
					surface.feedback.clear();
					cx.notify();
				}
			});
		}));
		cx.notify();
	}

	fn respond(&mut self, json: String, cx: &mut Context<Self>) {
		let Some(ChiefRequestResult::Available { event_id, work_id, .. }) = &self.request else {
			return;
		};
		if self.selected.as_ref() != Some(work_id) {
			return;
		}
		if !serde_json::from_str::<serde_json::Value>(&json).is_ok_and(|value| value.is_object()) {
			self.feedback =
				"Response must be a JSON object that matches the displayed provider request."
					.into();
			cx.notify();
			return;
		}
		let (Ok(work_id), Ok(response_json)) =
			(EntityId::new(work_id.clone()), HistoryText::new(json))
		else {
			return;
		};
		self.execute(
			ChiefActionDto::Respond { work_id, event_id: *event_id, response_json },
			None,
			cx,
		);
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		if self.voice.is_some() {
			return;
		}
		if self.dictation.is_some() {
			self.finish_dictation(cx);
			return;
		}
		if let Some(error) = self.composer_capability_error(cx) {
			self.feedback = error.into();
			cx.notify();
			return;
		}
		if self.sending || self.uncertain {
			return;
		}
		let text = self.composer.read(cx).content().to_owned();
		if text.trim().is_empty() && self.attachments.is_empty() {
			return;
		}
		let build = || -> Result<ChiefActionDto, String> {
			let prompt = HistoryText::new(if text.trim().is_empty() {
				"Please inspect the attached files.".into()
			} else {
				text.clone()
			})
			.map_err(|_| "Message is too long")?;
			if let Some(root) = self.snapshot.as_ref().and_then(|snapshot| {
				snapshot
					.work_items
					.iter()
					.find(|work| {
						Some(&work.id) == self.selected.as_ref()
							&& work.kind == decodex_protocol::ChiefWorkKindDto::Manager
					})
					.or_else(|| {
						snapshot.work_items.iter().find(|work| work.parent_goal_id.is_none())
					})
			}) {
				return Ok(ChiefActionDto::Send {
					root_id: EntityId::new(root.id.clone())
						.map_err(|_| "Invalid Chief identity")?,
					text: prompt,
				});
			}
			if self.state != LoadState::Ready {
				return Err("Refresh to confirm whether a Chief already exists.".into());
			}
			Ok(ChiefActionDto::Start(ChiefStartDto {
				root_id: EntityId::new(format!("chief-{}", unique_command()))
					.map_err(|_| "Invalid Chief identity")?,
				prompt,
				model: ConversationModel::new(self.model.read(cx).content().trim())
					.map_err(|_| "Enter an exact model ID.")?,
				cwd: ConversationWorkingDirectory::new(self.cwd.read(cx).content().trim())
					.map_err(|_| "Enter an absolute working directory.")?,
				account_id: if self.account.read(cx).content().trim().is_empty() {
					None
				} else {
					Some(
						EntityId::new(self.account.read(cx).content().trim())
							.map_err(|_| "Invalid account ID")?,
					)
				},
				effort: self.effort,
				sandbox: self.sandbox,
			}))
		};
		match build() {
			Ok(action) => {
				let Ok(model) = ConversationModel::new(self.model.read(cx).content()) else {
					return;
				};
				let execution = decodex_protocol::ConversationExecutionSettings {
					model,
					reasoning_effort: self.effort,
					fast: self.fast,
				};
				let attachments = self.attachments.clone();
				let action = match action {
					ChiefActionDto::Start(start) =>
						ChiefActionDto::StartConfigured { start, execution, attachments },
					ChiefActionDto::Send { root_id, text } =>
						self.configured_send(root_id, text, execution, attachments),
					action => action,
				};
				self.execute(action, Some(text), cx);
			},
			Err(message) => {
				self.feedback = message;
				cx.notify();
			},
		}
	}

	fn execute(&mut self, action: ChiefActionDto, draft: Option<String>, cx: &mut Context<Self>) {
		if self.sending || (self.uncertain && !matches!(&action, ChiefActionDto::Interrupt { .. }))
		{
			return;
		}
		let Some(profile) = self.profile.clone() else {
			self.feedback = "No service profile is configured.".into();
			cx.notify();
			return;
		};
		let sent_attachments = draft.as_ref().map(|_| self.attachments.clone());
		let draft_owner = self.composer_manager.clone().or_else(|| self.root_id());
		self.sending = true;
		self.feedback = "Waiting for durable acceptance…".into();
		let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let request = cx.background_executor().spawn(async move {
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "Cannot create client runtime".to_string())?;
			runtime
				.block_on(ChiefClient::new(profile).execute(action, key))
				.map_err(|error| format!("Request failed before dispatch: {error:?}"))
		});
		self.command_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				surface.sending = false;
				let current_owner = surface.composer_manager.clone().or_else(|| surface.root_id());
				let same_owner = current_owner == draft_owner || (draft_owner.is_none() && matches!(&result, Ok(ChiefCommandResponse::Accepted { work_id }) if current_owner.as_deref()==Some(work_id.as_str())));
				if !same_owner
					&& matches!(&result, Ok(ChiefCommandResponse::Accepted { .. }))
					&& let Some(owner) = &draft_owner
					&& surface.manager_drafts.get(owner).map(String::as_str) == draft.as_deref()
				{
					surface.manager_drafts.remove(owner);
				}
                if matches!(&result, Ok(ChiefCommandResponse::Accepted { .. }))
                    && let Some(sent) = &sent_attachments {
                        if same_owner { surface.attachments.retain(|file| !sent.contains(file)); }
                        else if let Some(files) = draft_owner.as_ref().and_then(|id|surface.attachment_drafts.get_mut(id)) {
                            files.retain(|file| !sent.contains(file));
                        }
                }
                surface.apply_command_result(
					result,
					if same_owner { draft.as_deref() } else { None },
					cx,
				);
				surface.refresh(cx);
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn apply_command_result(
		&mut self,
		result: Result<ChiefCommandResponse, String>,
		draft: Option<&str>,
		cx: &mut Context<Self>,
	) {
		let surface = self;
		match result {
			Ok(ChiefCommandResponse::Accepted { work_id }) => {
				let _ = work_id;
				surface.feedback.clear();
				if draft == Some(surface.composer.read(cx).content()) {
					surface.composer.update(cx, |input, cx| {
						input.clear(cx);
						input.set_placeholder(prompts::next(), cx);
					});
					Self::refresh_prompt(cx);
				}
			},
			Ok(ChiefCommandResponse::Rejected { error }) =>
				surface.feedback = format!("Not accepted: {error:?}. Draft retained."),
			Ok(ChiefCommandResponse::PotentiallyDispatched { failure }) => {
				surface.uncertain = true;
				surface.feedback = format!(
					"Acceptance unknown: {failure:?}. Draft retained. Sending is blocked to prevent duplicate effects; inspect history and service state."
				);
			},
			Err(message) => surface.feedback = message,
		}
	}

	pub(crate) fn bind_profile(&mut self, profile: Option<ClientProfile>, cx: &mut Context<Self>) {
		self.generation += 1;
		self.task = None;
		self.profile = profile;
		self.activity_detail = None;
		self.activity_detail_task = None;
		self.capability_task = None;
		self.capabilities = None;
		self.capabilities_checked = None;
		self.snapshot = None;
		self.pages.clear();
		self.page_views.clear();
		self.graph_expanded = false;
		self.manager_drafts.clear();
		self.composer_manager = None;
		self.history_cache.clear();
		self.history_marks.clear();
		self.history_marks_work = None;
		self.older_history.clear();
		self.older_task = None;
		self.loading_older = false;
		self.transcript_scroll.clear();
		self.graph_scope = None;
		self.graph_selected = None;
		self.history = None;
		self.history_task = None;
		self.request = None;
		self.request_task = None;
		self.selected = None;
		self.state = LoadState::Idle;
		self.poll_task = Some(cx.spawn(async move |surface, cx| {
			loop {
				cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
				if surface
					.update(cx, |surface, cx| {
						let active = surface.snapshot.as_ref().is_some_and(|snapshot| {
							snapshot.work_items.iter().any(|work| {
								work.dispatch_state != ChiefDispatchStateDto::Idle
									|| work.next_check_at_micros.is_some()
							}) || !snapshot.pending_events.is_empty()
						});
						if should_poll_snapshot(surface.profile.is_some(), &surface.state, active) {
							surface.refresh(cx);
						}
					})
					.is_err()
				{
					break;
				}
			}
		}));
		cx.notify();
	}

	pub(crate) fn mark_stale(&mut self, cx: &mut Context<Self>) {
		self.generation += 1;
		self.task = None;
		self.state =
			if self.snapshot.is_some() { LoadState::Stale } else { LoadState::Unavailable };
		cx.notify();
	}

	pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
		if self.state == LoadState::Loading {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			self.state = LoadState::Unavailable;
			cx.notify();
			return;
		};
		self.status_before_refresh = match self.state {
			LoadState::Stale | LoadState::Unavailable | LoadState::Capacity { .. } =>
				Some(self.state.clone()),
			_ => None,
		};
		self.state = LoadState::Loading;
		self.generation += 1;
		let generation = self.generation;
		let request = cx.background_executor().spawn(async move {
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| ())?;
			runtime.block_on(ChiefClient::new(profile).query()).map_err(|_| ())
		});
		self.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				if surface.generation != generation {
					return;
				}
				surface.apply_result(result);
				if surface.capabilities_checked.is_none_or(|at| at.elapsed().as_secs() >= 60) {
					surface.load_capabilities(cx);
				}
				surface.load_history(cx);

				surface.sync_request(cx);
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn apply_result(&mut self, result: Result<ChiefSnapshotResult, ()>) {
		match result {
			Ok(ChiefSnapshotResult::Available(snapshot)) => {
				if !self
					.selected
					.as_ref()
					.is_some_and(|id| snapshot.work_items.iter().any(|work| &work.id == id))
				{
					self.selected = snapshot
						.work_items
						.iter()
						.find(|work| work.parent_goal_id.is_none())
						.map(|work| work.id.clone());
				}
				self.snapshot = Some(snapshot);
				self.state = LoadState::Ready;
			},
			Ok(ChiefSnapshotResult::CapacityExceeded {
				work_items,
				dependencies,
				pending_events,
			}) => {
				self.snapshot = None;
				self.selected = None;
				self.state = LoadState::Capacity {
					work: work_items,
					edges: dependencies,
					events: pending_events,
				};
			},
			Ok(ChiefSnapshotResult::Unavailable) | Err(()) => {
				self.state =
					if self.snapshot.is_some() { LoadState::Stale } else { LoadState::Unavailable };
			},
		}
	}

	fn displayed_load_state(&self) -> &LoadState {
		if self.state == LoadState::Loading {
			self.status_before_refresh.as_ref().unwrap_or(&self.state)
		} else {
			&self.state
		}
	}

	pub(crate) fn recent_service_event(&self) -> Option<String> {
		let (id, ChiefHistoryResult::Available { entries, .. }) = self.history.as_ref()? else {
			return None;
		};
		if self.selected.as_ref() != Some(id) {
			return None;
		}
		entries.iter().rev().find(|entry| entry.kind == "system").map(|entry| entry.text.clone())
	}

	fn thread_in_use(&self, work: &str) -> bool {
		self.snapshot.as_ref().is_some_and(|snapshot| {
			snapshot.pending_events.iter().any(|event| {
				event.work_item_id == work && event.event_kind == "thread_in_use_needs_attention"
			})
		})
	}

	pub(crate) fn status_notice(&self) -> Option<(&'static str, String, bool)> {
		if !self.feedback.is_empty() {
			return Some((
				if self.sending {
					"Sending"
				} else if self.uncertain {
					"Check delivery"
				} else {
					"Message status"
				},
				self.feedback.clone(),
				false,
			));
		}
		if let Some(event) = self.snapshot.as_ref().and_then(|snapshot| {
			snapshot
				.pending_events
				.iter()
				.find(|event| event.event_kind == "thread_in_use_needs_attention")
		}) {
			let name = self
				.snapshot
				.as_ref()
				.and_then(|snapshot| {
					snapshot.work_items.iter().find(|work| work.id == event.work_item_id)
				})
				.map(|work| self.work_label(work))
				.unwrap_or_else(|| "This conversation".into());
			return Some((
				"In use elsewhere",
				format!(
					"{name} is in use in Codex or another application. Release the conversation there to continue here. Saved messages will continue automatically; history remains readable."
				),
				false,
			));
		}
		let title = match self.displayed_load_state() {
			LoadState::Stale => "Updates paused",
			LoadState::Unavailable => "Connection unavailable",
			LoadState::Capacity { .. } => "Work view unavailable",
			_ => {
				let pending = self.snapshot.as_ref()?.pending_events.iter().find(|event| {
					event.event_kind.ends_with("_needs_attention")
						|| event.event_kind.ends_with("_failed")
				})?;
				return Some((
					"Work needs attention",
					format!(
						"{} · {}. Open Diagnostics for recovery details.",
						pending.work_item_id,
						pending.event_kind.replace('_', " ")
					),
					false,
				));
			},
		};
		Some((
			title,
			self.status_text(),
			self.profile.is_some() && self.state != LoadState::Loading,
		))
	}

	fn status_text(&self) -> String {
		match self.displayed_load_state() {
			LoadState::Idle => "Refresh to read Chief work from the local service.".into(),
			LoadState::Loading => if self.snapshot.is_some() {
				"Refreshing · showing the previous snapshot"
			} else {
				"Loading Chief work…"
			}
			.into(),
			LoadState::Ready => "Saved conversation and work status".into(),
			LoadState::Unavailable => if self.profile.is_none() {
				"No local service profile is configured for this view."
			} else {
				"Reconnecting to Chief. Connection details are in Settings → Diagnostics."
			}
			.into(),
			LoadState::Stale =>
				"Latest work status could not be loaded. Your previous view is still available. Decodex will retry automatically."
					.into(),
			LoadState::Capacity { work, edges, events } => format!(
				"Snapshot capacity exceeded: {work} work items, {edges} dependencies, {events} pending events. No partial graph is shown."
			),
		}
	}

	fn details(
		&self,
		snapshot: &ChiefSnapshotDto,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		div()
			.id("chief-work-detail")
			.flex()
			.flex_col()
			.gap_3()
			.p_5()
			.min_w_0()
			.when(
				snapshot.pending_events.iter().any(|event| {
					event.work_item_id == work.id
						&& ["permission_pending", "user_input_pending", "server_request_pending"]
							.contains(&event.event_kind.as_str())
				}),
				|panel| panel.child(self.pending_panel(snapshot, work, cx)),
			)
			.child(self.request_panel(snapshot, work, cx))
			.child(self.history_panel(work, cx))
			.child(
				div()
					.id("chief-toggle-details")
					.role(Role::Button)
					.tab_index(27)
					.cursor_pointer()
					.text_color(rgb(ui_theme::BLUE))
					.on_click(cx.listener(|surface, _, _, cx| {
						surface.details_visible = !surface.details_visible;
						cx.notify();
					}))
					.on_key_down(cx.listener(|surface, event: &gpui::KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							surface.details_visible = !surface.details_visible;
							cx.notify();
						}
					}))
					.child(if self.details_visible {
						"Hide work details ▾"
					} else {
						"Work details and dependencies ▸"
					})
					.smooth(),
			)
			.child(disclosure(
				"chief-details-motion",
				self.details_visible,
				div()
					.flex()
					.flex_col()
					.gap_3()
					.child(self.work_metadata(snapshot, work, cx))
					.child(self.work_graph(snapshot, work, cx)),
			))
	}

	fn work_metadata(
		&self,
		snapshot: &ChiefSnapshotDto,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let mut panel = div()
			.id("chief-work-metadata")
			.flex()
			.flex_col()
			.gap_3()
			.p_5()
			.min_w_0()
			.child(
				div()
					.text_size(px(ui_theme::HEADING_SIZE))
					.font_weight(FontWeight::SEMIBOLD)
					.child(work.title.clone()),
			)
			.child(detail("Work ID", &work.id))
			.child(detail("Judgment", judgment(work.status)))
			.child(detail("Execution", execution(work.dispatch_state)));
		if let Some(parent) = &work.parent_goal_id {
			panel = panel.child(detail("Parent goal", title(snapshot, parent)));
		}
		if let Some(thread) = &work.codex_thread_id {
			let copied = thread.clone();
			let copied_key = thread.clone();
			panel = panel.child(detail("Codex thread", thread)).child(
				div()
					.id("chief-copy-thread")
					.role(Role::Button)
					.aria_label("Copy exact Codex thread ID")
					.text_size(px(ui_theme::BODY_SIZE))
					.text_color(rgb(ui_theme::BLUE))
					.cursor_pointer()
					.track_focus(&self.copy_focus)
					.on_click(cx.listener(move |_, _, _, cx| {
						cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()))
					}))
					.on_key_down(cx.listener(move |_, event: &gpui::KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							cx.write_to_clipboard(ClipboardItem::new_string(copied_key.clone()));
						}
					}))
					.child("Copy thread ID")
					.smooth(),
			);
		} else {
			panel = panel.child(detail("Codex thread", "No thread is bound"));
		}
		if let Some(turn) = &work.active_turn_id {
			panel = panel.child(detail("Acknowledged turn", turn));
			if work.dispatch_state == ChiefDispatchStateDto::Running {
				let work_id = work.id.clone();
				let turn_id = turn.clone();
				panel = panel.child(
					div()
						.id("chief-interrupt")
						.role(Role::Button)
						.tab_index(29)
						.cursor_pointer()
						.text_color(rgb(ui_theme::BLUE))
						.on_click(cx.listener(move |surface, _, _, cx| {
							if let (Ok(work_id), Ok(turn_id)) =
								(EntityId::new(work_id.clone()), WireText::new(turn_id.clone()))
							{
								surface.execute(
									ChiefActionDto::Interrupt { work_id, turn_id },
									None,
									cx,
								);
							}
						}))
						.child("Interrupt this acknowledged turn")
						.smooth(),
				);
			}
		}
		if let Some(due) = work.next_check_at_micros {
			panel = panel.child(detail("Next check", &next_check_text(due)));
		}
		panel.child(self.pending_panel(snapshot, work, cx))
	}

	fn work_graph(
		&self,
		snapshot: &ChiefSnapshotDto,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let mut panel = div().flex().flex_col().gap_3().child(
			div()
				.text_size(px(ui_theme::BODY_SIZE))
				.font_weight(FontWeight::SEMIBOLD)
				.child("Work graph"),
		);
		let dependencies: Vec<_> =
			snapshot.dependencies.iter().filter(|edge| edge.work_item_id == work.id).collect();
		if dependencies.is_empty() {
			panel = panel.child(muted("No declared dependencies"));
		}
		for edge in dependencies {
			panel = panel.child(self.relation("Requires", snapshot, &edge.depends_on_id, cx));
		}
		for edge in snapshot.dependencies.iter().filter(|edge| edge.depends_on_id == work.id) {
			panel = panel.child(self.relation("Required by", snapshot, &edge.work_item_id, cx));
		}
		for child in snapshot
			.work_items
			.iter()
			.filter(|child| child.parent_goal_id.as_deref() == Some(&work.id))
		{
			panel = panel.child(self.relation("Coordinates", snapshot, &child.id, cx));
		}
		panel
	}

	fn load_older_history(&mut self, cx: &mut Context<Self>) {
		if self.loading_older {
			return;
		}
		let (Some(profile), Some((id, ChiefHistoryResult::Available { next_before, .. }))) =
			(self.profile.clone(), self.history.as_ref())
		else {
			return;
		};
		let before = self.older_history.get(id).map_or(*next_before, |(_, cursor)| *cursor);
		let Some(before) = before else {
			return;
		};
		let id = id.clone();
		let Ok(work) = EntityId::new(id.clone()) else {
			return;
		};
		self.loading_older = true;
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).history_page(work, Some(before))).ok()
		});
		self.older_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				s.loading_older = false;
				if let Some(ChiefHistoryResult::Available { entries, next_before, .. }) = result {
					if let Some(scroll) = s.transcript_scroll.get(&id) {
						s.older_scroll_anchor = Some((
							id.clone(),
							f32::from(scroll.offset().y),
							f32::from(scroll.max_offset().y),
						));
					}
					let page = s.older_history.entry(id).or_default();
					page.0.extend(entries);
					page.0.sort_by_key(|entry| entry.id);
					page.0.dedup_by_key(|entry| entry.id);
					page.1 = next_before;
				} else {
					s.feedback = "Earlier messages could not be loaded. Try again.".into();
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn history_panel(&self, work: &ChiefWorkItemDto, cx: &mut Context<Self>) -> impl IntoElement {
		let mut panel =
			div().w_full().min_w_0().flex_none().flex().flex_col().gap(px(ui_theme::MESSAGE_GAP));
		match self.history.as_ref().filter(|(id, _)| id == &work.id).map(|(_, history)| history) {
			Some(ChiefHistoryResult::Available {
				entries, has_more, next_before, live, ..
			}) => {
				let older = self.older_history.get(&work.id);
				let cursor = older.map_or(*next_before, |(_, cursor)| *cursor);
				if cursor.is_some() {
					panel = panel.child(
						div()
							.id("chief-earlier-history")
							.role(Role::Button)
							.tab_index(26)
							.aria_label("Load earlier messages")
							.cursor_pointer()
							.text_color(rgb(ui_theme::BLUE))
							.on_click(cx.listener(|s, _, _, cx| s.load_older_history(cx)))
							.child(if self.loading_older {
								"Loading earlier messages…"
							} else {
								"Load earlier messages"
							})
							.smooth(),
					);
				}
				if *has_more && next_before.is_none() {
					panel = panel.child(muted("Some saved message text was shortened."));
				}
				let mut saved = std::collections::BTreeMap::new();
				if let Some((entries, _)) = older {
					for entry in entries {
						saved.insert(entry.id, entry);
					}
				}
				for entry in entries {
					saved.insert(entry.id, entry);
				}
				panel = panel.children(self.progress_history(
					saved.values().copied().collect(),
					work,
					cx,
				));
				for message in live {
					panel = panel.child(
						div()
							.w_full()
							.py_2()
							.child(markdown::render(
								&message.text,
								&format!("live-{}", message.item_id),
							))
							.when(message.truncated, |row| {
								row.child(muted(
									"Partial output shortened; waiting for saved result.",
								))
							}),
					);
				}
				if entries.is_empty() && live.is_empty() {
					panel = panel.child(muted("Waiting for the first message…"));
				}
			},
			Some(ChiefHistoryResult::Unavailable) =>
				panel = panel.child(muted("Messages could not be loaded. Retrying…")),
			None => panel = panel.child(muted("Loading messages…")),
		}
		panel.children(self.live_chat_caption())
	}

	fn pending_panel(
		&self,
		snapshot: &ChiefSnapshotDto,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let mut panel = div().flex().flex_col().gap_2();
		for event in snapshot
			.pending_events
			.iter()
			.filter(|event| event.work_item_id == work.id && event.event_kind.ends_with("_pending"))
		{
			let id = event.id;
			panel = panel.child(
				div()
					.id(SharedString::from(format!("review-request-{id}")))
					.role(Role::Button)
					.tab_index(0)
					.aria_label("Review request")
					.cursor_pointer()
					.text_color(rgb(ui_theme::BLUE))
					.on_click(cx.listener(move |s, _, _, cx| s.load_request(id, cx)))
					.child(if event.event_kind == "user_input_pending" {
						"Answer a question"
					} else {
						"Review requested access"
					})
					.smooth(),
			);
		}
		panel
	}

	fn relation(
		&self,
		label: &str,
		snapshot: &ChiefSnapshotDto,
		id: &str,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let selected = id.to_owned();
		let keyboard_id = selected.clone();
		div()
			.id(SharedString::from(format!("chief-relation-{label}-{id}")))
			.role(Role::Button)
			.tab_index(28)
			.cursor_pointer()
			.text_color(rgb(ui_theme::BLUE))
			.on_click(cx.listener(move |surface, _, _, cx| {
				surface.open_page(&selected, cx);
				cx.notify();
			}))
			.on_key_down(cx.listener(move |surface, event: &gpui::KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					surface.open_page(&keyboard_id, cx);
					cx.notify();
				}
			}))
			.child(format!("{label} → {}", title(snapshot, id)))
	}

	fn cycle_effort(&mut self, cx: &mut Context<Self>) {
		let supported = self.model_efforts(cx);
		if !supported.is_empty() {
			let next = supported
				.iter()
				.position(|level| *level == self.effort)
				.map_or(0, |index| (index + 1) % supported.len());
			self.effort = supported[next];
		}
		cx.notify();
	}

	fn cycle_sandbox(&mut self, cx: &mut Context<Self>) {
		self.sandbox = match self.sandbox {
			ChiefSandboxDto::ReadOnly => ChiefSandboxDto::WorkspaceWrite,
			ChiefSandboxDto::WorkspaceWrite => ChiefSandboxDto::FullAccess,
			ChiefSandboxDto::FullAccess => ChiefSandboxDto::ReadOnly,
		};
		cx.notify();
	}
}

fn title<'a>(snapshot: &'a ChiefSnapshotDto, id: &'a str) -> &'a str {
	snapshot
		.work_items
		.iter()
		.find(|work| work.id == id)
		.map(|work| work.title.as_str())
		.unwrap_or(id)
}

fn unique_command() -> String {
	static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
	let time = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.unwrap_or_default()
		.as_nanos();
	format!(
		"gpui-chief-{}-{time}-{}",
		std::process::id(),
		NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
	)
}

fn offered_decisions(method: &str, request: &str) -> Vec<String> {
	if method != "item/commandExecution/requestApproval" {
		return vec![];
	}
	let Ok(value) = serde_json::from_str::<serde_json::Value>(request) else {
		return vec![];
	};
	if value.get("availableDecisions").is_none_or(serde_json::Value::is_null) {
		return vec!["accept".into(), "decline".into()];
	}
	value
		.get("availableDecisions")
		.and_then(|value| value.as_array())
		.into_iter()
		.flatten()
		.filter_map(|value| value.as_str())
		.filter(|value| ["accept", "acceptForSession", "decline", "cancel"].contains(value))
		.map(str::to_owned)
		.collect()
}

fn next_check_text(due: i64) -> String {
	let now = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map(|time| time.as_micros() as i64)
		.unwrap_or(0);
	let seconds = due.saturating_sub(now) / 1_000_000;
	if seconds <= 0 {
		"Due now".into()
	} else if seconds < 60 {
		format!("In {seconds} seconds")
	} else if seconds < 3600 {
		format!("In {} minutes", seconds / 60)
	} else if seconds < 86400 {
		format!("In {} hours", seconds / 3600)
	} else {
		format!("In {} days", seconds / 86400)
	}
}
fn judgment(status: ChiefWorkStatusDto) -> &'static str {
	match status {
		ChiefWorkStatusDto::Open => "Open",
		ChiefWorkStatusDto::Resolved => "Resolved",
		ChiefWorkStatusDto::FollowUp => "Follow-up required",
		ChiefWorkStatusDto::Wait => "Waiting",
		ChiefWorkStatusDto::UserDecision => "User decision required",
	}
}
fn execution(state: ChiefDispatchStateDto) -> &'static str {
	match state {
		ChiefDispatchStateDto::Idle => "Idle · no active dispatch",
		ChiefDispatchStateDto::Dispatching => "Dispatch claimed · awaiting acknowledgment",
		ChiefDispatchStateDto::Running => "Turn acknowledged · running",
		ChiefDispatchStateDto::Unknown => "Unknown outcome · reconciliation required",
	}
}
fn muted(text: impl Into<SharedString>) -> impl IntoElement {
	div()
		.text_size(px(ui_theme::CAPTION_SIZE))
		.text_color(rgb(ui_theme::TEXT_MUTED))
		.child(text.into())
}
fn detail(label: &str, value: &str) -> impl IntoElement {
	div()
		.flex()
		.flex_col()
		.gap_1()
		.child(muted(label.to_owned()))
		.child(div().text_size(px(ui_theme::BODY_SIZE)).child(value.to_owned()))
}

fn history_entry(entry: &decodex_protocol::ChiefHistoryEntryDto) -> gpui::Div {
	let user = entry.kind == "user";
	if entry.kind == "execution_notice" {
		return div()
			.w_full()
			.py_2()
			.text_size(px(11.))
			.text_color(rgb(ui_theme::AMBER))
			.child(entry.text.clone());
	}
	div()
		.w_full()
		.min_w_0()
		.flex_none()
		.flex()
		.when(user, |row| row.child(div().flex_1().min_w_0()))
		.child(
			div()
				.min_w_0()
				.when(user, |bubble| {
					bubble
						.flex_none()
						.max_w(gpui::relative(0.78))
						.px_4()
						.py_3()
						.rounded(px(18.0))
						.bg(rgba(0xffffff0e))
				})
				.when(!user, |body| body.w_full().py_2())
				.when(entry.kind == "instruction", |body| {
					body.pl_3()
						.border_l_2()
						.border_color(rgb(ui_theme::BLUE))
						.child(muted("Manager instruction"))
				})
				.child(markdown::render(&entry.text, &format!("message-{}", entry.id)))
				.when(!user, |body| body.child(reply_metrics(entry))),
		)
}

fn compact_tokens(value: u64) -> String {
	let (divisor, suffix) = if value >= 999_950 {
		(1_000_000.0, "M")
	} else if value >= 1000 {
		(1000.0, "K")
	} else {
		return value.to_string();
	};
	let text = format!("{:.1}", value as f64 / divisor);
	format!("{}{suffix}", text.trim_end_matches(".0"))
}

fn reply_metrics(entry: &decodex_protocol::ChiefHistoryEntryDto) -> impl IntoElement {
	let mut parts = Vec::new();
	if let Some(duration) = entry.duration_ms {
		parts.push(format!("Worked for {:.1}s", duration as f64 / 1000.0));
	}
	if let Some(usage) = &entry.usage {
		parts.push(format!("In {}", compact_tokens(usage.input_tokens)));
		parts.push(format!("Out {} tokens", compact_tokens(usage.output_tokens)));
	}
	let mut row = div()
		.when(!parts.is_empty(), |row| row.mt_2())
		.flex()
		.items_center()
		.flex_wrap()
		.gap(px(ui_theme::METADATA_GAP))
		.text_size(px(ui_theme::CAPTION_SIZE))
		.text_color(rgb(ui_theme::TEXT_MUTED));
	for (index, text) in parts.into_iter().enumerate() {
		if index > 0 {
			row = row.child(div().flex_none().child("·"));
		}
		row = row.child(div().flex_none().child(text));
	}
	row
}

pub(crate) struct ChiefPreferences {
	chief: Entity<ChiefSurface>,
}

impl ChiefPreferences {
	pub(crate) fn new(chief: Entity<ChiefSurface>, cx: &mut Context<Self>) -> Self {
		cx.observe(&chief, |_, _, cx| cx.notify()).detach();
		Self { chief }
	}
}

impl Render for ChiefPreferences {
	fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.chief.update(cx, |chief, cx| chief.render_preferences(cx).into_any_element())
	}
}

impl ChiefSurface {}

impl Render for ChiefSurface {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.render_workspace(window, cx)
	}
}

impl ChiefSurface {
	fn render_preferences(&self, cx: &mut Context<Self>) -> impl IntoElement {
		ui_theme::settings_group()
			.flex()
			.flex_col()
			.gap_2()
			.px(px(14.0))
			.py(px(10.0))
			.child(
				div()
					.id("chief-advanced-preferences")
					.role(Role::Button)
					.aria_label("Advanced Chief defaults")
					.aria_expanded(self.setup_expanded)
					.tab_index(0)
					.h(px(26.0))
					.flex()
					.items_center()
					.cursor_pointer()
					.on_click(cx.listener(|s, _, _, cx| {
						s.setup_expanded = !s.setup_expanded;
						cx.notify();
					}))
					.on_key_down(cx.listener(|s, event: &gpui::KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							s.setup_expanded = !s.setup_expanded;
							cx.notify();
						}
					}))
					.w_full()
					.justify_between()
					.child("Chief defaults")
					.child(super::workspace_symbols::icon(
						super::workspace_symbols::Symbol::ChevronDown,
					))
					.smooth(),
			)
			.children(match &self.capabilities {
				Some(decodex_protocol::ChiefCapabilitiesResult::Available {
					memory_enabled: Some(enabled),
					..
				}) => Some(
					div()
						.h(px(26.))
						.flex()
						.items_center()
						.justify_between()
						.child(muted("Codex Memory"))
						.child(muted(if *enabled {
							"Enabled in runtime"
						} else {
							"Disabled in runtime"
						})),
				),
				_ => None,
			})
			.child(disclosure(
				"chief-advanced-motion",
				self.setup_expanded,
				self.render_setup_controls(cx),
			))
	}

	fn render_setup_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		div()
			.flex()
			.flex_col()
			.gap_2()
			.child(self.context_choices(cx))
			.child(muted(
				"Model and effort apply to the next turn. Other defaults apply to a new Chief.",
			))
			.children(
				[
					("Model", self.model.clone()),
					("Working directory", self.cwd.clone()),
					("Account ID", self.account.clone()),
				]
				.into_iter()
				.map(|(label, input)| {
					div()
						.w_full()
						.flex()
						.items_center()
						.gap(px(12.0))
						.child(
							div()
								.w(px(112.0))
								.flex_none()
								.text_size(px(ui_theme::CAPTION_SIZE))
								.text_color(rgb(ui_theme::TEXT_MUTED))
								.child(label),
						)
						.child(div().flex_1().min_w_0().child(input))
				}),
			)
			.child(
				div()
					.flex()
					.gap_3()
					.child(
						div()
							.id("chief-effort")
							.role(Role::Button)
							.tab_index(34)
							.cursor_pointer()
							.on_key_down(cx.listener(
								|surface, event: &gpui::KeyDownEvent, _, cx| {
									if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
										surface.cycle_effort(cx);
									}
								},
							))
							.on_click(cx.listener(|surface, _, _, cx| {
								surface.cycle_effort(cx);
							}))
							.child(format!("Reasoning: {} ▸", self.effort.as_str()))
							.smooth(),
					)
					.child(
						div()
							.id("chief-sandbox")
							.role(Role::Button)
							.tab_index(35)
							.cursor_pointer()
							.on_key_down(cx.listener(
								|surface, event: &gpui::KeyDownEvent, _, cx| {
									if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
										surface.cycle_sandbox(cx);
									}
								},
							))
							.on_click(cx.listener(|surface, _, _, cx| {
								surface.cycle_sandbox(cx);
							}))
							.child(format!("Access: {:?} ▸", self.sandbox))
							.smooth(),
					),
			)
	}

	fn context_choices(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let account = self
			.accounts
			.iter()
			.find(|(id, _)| id == self.account.read(cx).content())
			.map_or("Automatic routing", |(_, alias)| alias.as_str());
		div()
			.flex()
			.gap_4()
			.child(
				div()
					.id("chief-model-choice")
					.role(Role::Button)
					.tab_index(30)
					.cursor_pointer()
					.text_color(rgb(ui_theme::BLUE))
					.on_click(cx.listener(|surface, _, _, cx| surface.cycle_model(cx)))
					.on_key_down(cx.listener(|surface, event: &gpui::KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							surface.cycle_model(cx);
						}
					}))
					.child("Model")
					.smooth(),
			)
			.child(
				div()
					.id("chief-account-choice")
					.role(Role::Button)
					.tab_index(31)
					.cursor_pointer()
					.text_color(rgb(ui_theme::BLUE))
					.on_click(cx.listener(|surface, _, _, cx| surface.cycle_account(cx)))
					.on_key_down(cx.listener(|surface, event: &gpui::KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							surface.cycle_account(cx);
						}
					}))
					.child(format!("Account: {account} ▸"))
					.smooth(),
			)
			.child(muted(""))
	}
}

#[cfg(test)]
mod tests {
	struct BubbleGeometry {
		text: String,
		bounds: std::rc::Rc<std::cell::RefCell<Vec<gpui::Bounds<gpui::Pixels>>>>,
	}
	impl gpui::Render for BubbleGeometry {
		fn render(
			&mut self,
			_: &mut gpui::Window,
			_: &mut gpui::Context<Self>,
		) -> impl gpui::IntoElement {
			let bounds = self.bounds.clone();
			super::history_entry(&decodex_protocol::ChiefHistoryEntryDto {
				activity: None,
				id: 1,
				kind: "user".into(),
				text: self.text.clone(),
				created_at_micros: 1,
				duration_ms: None,
				usage: None,
			})
			.on_children_prepainted(move |value, _, _| *bounds.borrow_mut() = value)
		}
	}
	#[gpui::test]
	fn user_bubbles_stay_right_aligned_at_multiple_widths(cx: &mut gpui::TestAppContext) {
		for width in [640.0, 1248.0] {
			for text in ["1".to_string(), "请检查布局，保持消息上下衔接。".repeat(60)]
			{
				let bounds = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
				let measured = bounds.clone();
				let (_, visual) = cx.add_window_view(|_, _| BubbleGeometry { text, bounds });
				visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(900.0)));
				visual.update(|window, cx| {
					window.draw(cx).clear();
				});
				let measured = measured.borrow();
				let bubble = measured.last().unwrap();
				assert!((f32::from(bubble.right()) - width).abs() < 2.0, "{bubble:?}");
				assert!(f32::from(bubble.size.width) <= width * 0.78 + 2.0);
			}
		}
	}
	#[test]
	fn token_counts_use_compact_units() {
		assert_eq!(super::compact_tokens(0), "0");
		assert_eq!(super::compact_tokens(999), "999");
		assert_eq!(super::compact_tokens(1000), "1K");
		assert_eq!(super::compact_tokens(24860), "24.9K");
		assert_eq!(super::compact_tokens(999950), "1M");
		assert_eq!(super::compact_tokens(1280000), "1.3M");
	}
	#[gpui::test]
	fn context_is_hidden_without_reported_usage(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(super::ChiefSurface::new);
		surface.update(cx, |s, cx| {
			assert!(s.usage_line().is_none());
			s.visual_workspace_fixture(cx);
			assert!(s.usage_line().is_none());
			s.visual_workspace_page("markdown", cx);
			assert!(s.usage_line().is_some());
		});
	}
	use super::*;
	use decodex_protocol::ChiefWorkKindDto;
	use gpui::Focusable;
	#[test]
	fn snapshot_poll_recovers_initial_failure_without_repeating_ready_idle_reads() {
		assert!(should_poll_snapshot(true, &LoadState::Unavailable, false));
		assert!(should_poll_snapshot(true, &LoadState::Stale, false));
		assert!(should_poll_snapshot(true, &LoadState::Idle, false));
		assert!(!should_poll_snapshot(false, &LoadState::Unavailable, false));
		assert!(!should_poll_snapshot(true, &LoadState::Loading, true));
		assert!(should_poll_snapshot(true, &LoadState::Ready, false));
		assert!(should_poll_snapshot(true, &LoadState::Ready, true));
	}

	#[gpui::test]
	fn command_enter_submits_chief_composer_and_keeps_unaccepted_draft(
		cx: &mut gpui::TestAppContext,
	) {
		cx.update(crate::composer_input::bind_keys);
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		let input = surface.update(visual, |surface, cx| {
			surface.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				workspaces: vec![],
				work_items: vec![],
				dependencies: vec![],
				pending_events: vec![],
			})));
			surface.cycle_model(cx);
			surface.seed_context(
				Some(ConversationWorkingDirectory::new("/Users/tester").unwrap()),
				vec![],
				cx,
			);
			surface
				.composer
				.update(cx, |input, cx| input.set_content("Please coordinate this goal", cx));
			surface.composer.clone()
		});
		visual.update(|window, cx| {
			window.focus(&input.focus_handle(cx), cx);
			window.draw(cx).clear();
		});
		visual.simulate_keystrokes("cmd-enter");
		surface.update(visual, |surface, cx| {
			assert_eq!(surface.feedback, "No service profile is configured.");
			assert_eq!(surface.composer.read(cx).content(), "Please coordinate this goal");
			assert!(!surface.sending);
		});
	}

	#[gpui::test]
	fn context_seed_preserves_user_edits_and_account_choice_uses_real_ids(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |surface, cx| {
			surface.cwd.update(cx, |input, cx| input.set_content("/Users/chosen", cx));
			surface.seed_context(
				Some(ConversationWorkingDirectory::new("/Users/default").unwrap()),
				vec![("exact-id".into(), "My account".into())],
				cx,
			);
			assert_eq!(surface.cwd.read(cx).content(), "/Users/chosen");
			assert_eq!(surface.model.read(cx).content(), "gpt-6-astra");
			surface.cycle_account(cx);
			assert_eq!(surface.account.read(cx).content(), "exact-id");
			surface.cycle_account(cx);
			assert!(surface.account.read(cx).content().is_empty());
			assert!(!surface.details_visible);
		});
	}
	#[test]
	fn approval_buttons_use_only_the_exact_command_request_choices() {
		assert_eq!(
			offered_decisions(
				"item/commandExecution/requestApproval",
				r#"{"availableDecisions":["decline","accept","acceptForSession",{"acceptWithExecpolicyAmendment":{}}]}"#
			),
			vec!["decline", "accept", "acceptForSession"]
		);
		assert_eq!(
			offered_decisions("item/commandExecution/requestApproval", "{}"),
			vec!["accept", "decline"]
		);
		assert!(
			offered_decisions(
				"item/permissions/requestApproval",
				r#"{"availableDecisions":["accept"]}"#
			)
			.is_empty()
		);
	}

	#[gpui::test]
	fn selected_work_history_and_pending_request_render_together(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |surface, _| {
			surface.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				workspaces: vec![],
				work_items: vec![ChiefWorkItemDto {
					id: "root".into(),
					parent_goal_id: None,
					kind: ChiefWorkKindDto::Goal,
					title: "Chief".into(),
					codex_thread_id: Some("thread-real".into()),
					active_turn_id: Some("turn-real".into()),
					dispatch_state: ChiefDispatchStateDto::Running,
					status: ChiefWorkStatusDto::UserDecision,
					next_check_at_micros: None,
					created_at_micros: 1,
					updated_at_micros: 1,
				}],
				dependencies: vec![],
				pending_events: vec![decodex_protocol::ChiefPendingEventDto {
					id: 1,
					source_event_id: "request-source".into(),
					work_item_id: "root".into(),
					event_kind: "permission_pending".into(),
					created_at_micros: 1,
					delivery_claimed: false,
				}],
			})));
			surface.history = Some((
				"root".into(),
				ChiefHistoryResult::Available {
					usage: None,
					entries: vec![decodex_protocol::ChiefHistoryEntryDto {
						activity: None,
						usage: None,
						duration_ms: None,
						id: 1,
						kind: "assistant".into(),
						text: "Waiting for your decision".into(),
						created_at_micros: 1,
					}],
					has_more: false,
					next_before: None,
					live: vec![],
				},
			));
			surface.request = Some(ChiefRequestResult::Available {
				work_id: "root".into(),
				event_id: 1,
				method: "item/commandExecution/requestApproval".into(),
				request_json: HistoryText::new(
					r#"{"command":"pwd","availableDecisions":["accept","decline"]}"#,
				)
				.unwrap(),
			});
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(720.0)));
			window.draw(cx).clear();
		});
	}
	#[gpui::test]
	fn durable_acceptance_clears_only_the_submitted_draft(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |surface, cx| {
			surface.composer.update(cx, |input, cx| input.set_content("edited while sending", cx));
			surface.apply_command_result(
				Ok(ChiefCommandResponse::Accepted { work_id: EntityId::new("root").unwrap() }),
				Some("original"),
				cx,
			);
			assert_eq!(surface.composer.read(cx).content(), "edited while sending");
			surface.apply_command_result(
				Ok(ChiefCommandResponse::Accepted { work_id: EntityId::new("root").unwrap() }),
				Some("edited while sending"),
				cx,
			);
			assert!(surface.composer.read(cx).content().is_empty());
		});
	}

	#[gpui::test]
	fn unknown_acceptance_preserves_draft_and_blocks_another_send(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |surface, cx| {
			surface.composer.update(cx, |input, cx| input.set_content("do work", cx));
			surface.apply_command_result(
				Ok(ChiefCommandResponse::PotentiallyDispatched {
					failure: decodex_protocol::ClientFailure::ProtocolTimeout,
				}),
				Some("do work"),
				cx,
			);
			assert!(surface.uncertain);
			assert_eq!(surface.composer.read(cx).content(), "do work");
			surface.submit(cx);
			assert!(surface.command_task.is_none());
			assert!(surface.feedback.contains("Acceptance unknown"));
		});
	}
	#[gpui::test]
	fn failed_refresh_retains_stale_snapshot_but_capacity_never_shows_partial_graph(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |surface, _| {
			surface.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				workspaces: vec![],
				work_items: vec![],
				dependencies: vec![],
				pending_events: vec![],
			})));
			assert_eq!(surface.state, LoadState::Ready);
			surface.apply_result(Err(()));
			assert_eq!(surface.state, LoadState::Stale);
			assert_eq!(surface.status_notice().unwrap().0, "Updates paused");
			surface.status_before_refresh = Some(LoadState::Stale);
			surface.state = LoadState::Loading;
			let notice = surface.status_notice().unwrap();
			assert_eq!(notice.0, "Updates paused");
			assert!(!notice.2, "a running refresh must not offer another retry");
			surface.apply_result(Ok(ChiefSnapshotResult::Available(
				surface.snapshot.clone().unwrap(),
			)));
			assert!(surface.status_notice().is_none(), "recovery clears the status");
			assert!(surface.snapshot.is_some());
			surface.apply_result(Ok(ChiefSnapshotResult::CapacityExceeded {
				work_items: 101,
				dependencies: 0,
				pending_events: 0,
			}));
			assert!(surface.snapshot.is_none());
			assert!(surface.status_text().contains("No partial graph"));
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(720.0)));
			window.draw(cx).clear();
		});
	}

	#[gpui::test]
	fn saved_service_events_do_not_create_current_alerts_or_cross_conversations(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, _| {
			s.selected = Some("chief".into());
			s.history = Some((
				"chief".into(),
				ChiefHistoryResult::Available {
					usage: None,
					entries: vec![decodex_protocol::ChiefHistoryEntryDto {
						activity: None,
						usage: None,
						duration_ms: None,
						id: 1,
						kind: "system".into(),
						text: "Recovered service event".into(),
						created_at_micros: 1,
					}],
					has_more: false,
					next_before: None,
					live: vec![],
				},
			));
			s.snapshot = Some(ChiefSnapshotDto {
				workspaces: vec![],
				work_items: vec![],
				dependencies: vec![],
				pending_events: vec![],
			});
			assert_eq!(s.recent_service_event().as_deref(), Some("Recovered service event"));
			assert!(s.status_notice().is_none());
			s.snapshot.as_mut().unwrap().pending_events.push(
				decodex_protocol::ChiefPendingEventDto {
					id: 2,
					source_event_id: "failure".into(),
					work_item_id: "chief".into(),
					event_kind: "recovery_needs_attention".into(),
					created_at_micros: 2,
					delivery_claimed: false,
				},
			);
			assert_eq!(s.status_notice().unwrap().0, "Work needs attention");
			s.snapshot.as_mut().unwrap().pending_events[0].event_kind =
				"thread_in_use_needs_attention".into();
			assert_eq!(s.status_notice().unwrap().0, "In use elsewhere");
			assert!(s.thread_in_use("chief"));
			assert!(!s.thread_in_use("another-chief"));
			s.snapshot.as_mut().unwrap().pending_events.clear();
			assert!(!s.thread_in_use("chief"));
			assert!(s.status_notice().is_none());
			s.selected = Some("another-chief".into());
			assert!(s.recent_service_event().is_none());
		});
	}

	#[gpui::test]
	fn unconfigured_refresh_has_explicit_unavailable_state(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |surface, cx| {
			surface.refresh(cx);
			assert_eq!(surface.state, LoadState::Unavailable);
			assert!(surface.snapshot.is_none());
			assert!(surface.task.is_none());
		});
	}
}
