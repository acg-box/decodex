//! Agent conversation and work overview. The service owns records and execution.

#[path = "agent_activity.rs"] mod activity;
#[path = "agent_tree.rs"] mod agent_tree;
#[path = "agent_app_exposure.rs"] mod app_exposure;
#[path = "agent_archive.rs"] mod archive;
#[path = "agent_async_questions.rs"] mod async_questions;
#[path = "agent_capabilities.rs"] mod capabilities;
#[path = "agent_composer.rs"] mod composer;
#[path = "agent_creation_defaults.rs"] mod creation_defaults;
#[path = "agent_creation_setup.rs"] mod creation_setup;
#[path = "agent_detail.rs"] mod detail;
#[path = "agent_dictation.rs"] mod dictation;
#[path = "agent_drafts.rs"] mod drafts;
#[path = "agent_execution_intent.rs"] mod execution_intent;
#[path = "agent_graph.rs"] mod graph;
#[path = "agent_guardian.rs"] mod guardian;
#[path = "agent_hooks.rs"] mod hooks;
#[path = "agent_inspection.rs"] mod inspection;
#[path = "agent_install.rs"] mod install;
#[path = "agent_integrations.rs"] mod integrations;
#[path = "agent_live_settings.rs"] mod live_settings;
#[path = "agent_markdown.rs"] mod markdown;
#[path = "agent_mcp_forms.rs"] mod mcp_forms;
#[path = "agent_misalignment.rs"] mod misalignment;
#[path = "agent_model_settings.rs"] mod model_settings;
#[path = "agent_models.rs"] mod models;
#[path = "agent_native_agents.rs"] mod native_agents;
#[cfg(all(target_os = "macos", not(test)))]
#[path = "agent_native_composer.rs"]
mod native_composer;
#[path = "agent_native_goal.rs"] mod native_goal;
#[path = "agent_timeline.rs"] mod native_timeline;
#[path = "agent_output_stream.rs"] mod output_stream;
#[path = "agent_permissions.rs"] mod permissions;
#[path = "agent_progress.rs"] mod progress;
#[path = "agent_prompt_edit.rs"] mod prompt_edit;
#[path = "agent_prompts.rs"] mod prompts;
#[path = "agent_question_notices.rs"] mod question_notices;
#[path = "agent_recap.rs"] mod recap;
#[path = "agent_requests.rs"] mod requests;
#[path = "agent_resources.rs"] mod resources;
#[path = "agent_response_metrics.rs"] mod response_metrics;
#[path = "agent_search_settings.rs"] mod search_settings;
#[path = "agent_selectable_text.rs"] mod selectable_text;
#[path = "agent_send_preview.rs"] mod send_preview;
#[path = "agent_skills.rs"] mod skills;
#[path = "agent_steer_receipts.rs"] mod steer_receipts;
#[path = "agent_text_reveal.rs"] mod text_reveal;
#[path = "agent_transcript.rs"] mod transcript;
#[path = "agent_usage_estimates.rs"] mod usage_estimates;
#[path = "agent_voice.rs"] mod voice;
#[path = "agent_voice_settings.rs"] mod voice_settings;
#[path = "agent_weather.rs"] mod weather;
#[path = "agent_work_browser.rs"] mod work_browser;
#[path = "agent_workspace.rs"] mod workspace;
#[path = "agent_workspace_size.rs"] mod workspace_size;

use std::{
	collections::HashSet,
	process,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

#[cfg(feature = "visual-capture")] use gpui::Focusable;
use gpui::{
	AnyElement, AppContext as _, Bounds, ClipboardItem, Context, Div, Entity, FocusHandle,
	FontWeight, KeyDownEvent, Pixels, Point, Render, Role, ScrollHandle, SharedString, Task,
	Window,
	prelude::{
		FluentBuilder, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement,
		Styled, StyledImage,
	},
};
use tokio::runtime::Builder;

use crate::{
	composer_input::{ComposerEvent, ComposerInput, SubmitComposer},
	panel_preferences::PanelDefaults,
	shell::{workspace_symbols, workspace_symbols::Symbol},
	ui_loading,
	ui_motion::{self, SmoothControl},
	ui_theme::{
		self, AMBER, BLUE, CAPTION_SIZE, HOVER_FILL, MESSAGE_GAP, METADATA_GAP, TEXT_MUTED,
	},
	ui_working::Working,
};
use activity::{HistoryKey, HistoryMark, HistoryNavigation, WheelScroll};
use async_questions::ChoiceDraft;
use capabilities::CatalogContext;
use creation_setup::DEFAULT_MODEL;
use decodex_protocol::{
	AgentActionDto, AgentAttachmentDto, AgentClient, AgentCommandResponse, AgentDispatchStateDto,
	AgentExecutionOverrides, AgentHistoryEntryDto, AgentHistoryResult, AgentIntegrationsResult,
	AgentLiveMessageKind, AgentPendingEventDto, AgentRequestResult, AgentRequestedDecision,
	AgentResourcesResult, AgentSandboxDto, AgentSnapshotDto, AgentSnapshotResult, AgentStartDto,
	AgentTaskReferenceDto, AgentUsageEstimateResult, AgentWorkItemDto, AgentWorkKindDto,
	AgentWorkStatusDto, ClientProfile, ConversationModel, ConversationReasoningEffort,
	ConversationWorkingDirectory, DesktopCreationIntent, DesktopQuestionDraft,
	DesktopRecoveredDraft, EntityId, HistoryText, IdempotencyKey, InitialModelCatalogResult,
	MAX_HISTORY_INLINE_BYTES, ServiceTier, WireText,
};
use detail::ActivityDetailState;
use dictation::DictationUi;
use drafts::{Profiles, SubmissionState};
use native_agents::NativeAgents;
use native_timeline::Timeline;
use output_stream::OutputStream;
use question_notices::QuestionNotices;
use recap::Automatic;
use requests::{QuestionTimer, RequestReader};
use response_metrics::ResponseMetrics;
use selectable_text::SelectableText;
use skills::Picker;
use text_reveal::StreamingText;
use voice::{CaptionHistory, VoiceUi};
use workspace::PageView;

#[derive(Clone, Debug, Eq, PartialEq)]
enum LoadState {
	Idle,
	Loading,
	Ready,
	Unavailable,
	Stale,
	Capacity { work: u64, edges: u64, events: u64 },
}

pub(crate) struct AgentSurface {
	#[cfg(all(target_os = "macos", not(test)))]
	native_composer: native_composer::NativeComposer,
	voice: Option<VoiceUi>,
	retired_voice_captions: Vec<CaptionHistory>,
	voice_task: Option<Task<()>>,
	voice_settings: voice_settings::Panel,
	search_settings: search_settings::Panel,
	skills: Picker,
	recap: recap::Panel,
	prompt_edit: prompt_edit::Panel,
	automatic_recap: Automatic,
	audio_inputs: Vec<String>,
	audio_input: String,
	dictation: Option<DictationUi>,
	dictation_task: Option<Task<()>>,
	activity_detail: ActivityDetailState,
	resources: Option<(String, Option<AgentResourcesResult>)>,
	resources_task: Option<Task<()>>,
	usage_estimate: Option<(String, Option<AgentUsageEstimateResult>)>,
	usage_estimate_task: Option<Task<()>>,
	usage_estimate_epoch: u64,
	timeline: TimelineView,
	integrations: Option<(String, Option<AgentIntegrationsResult>)>,
	integrations_task: Option<Task<()>>,
	resource_mutation_task: Option<Task<()>>,
	resource_feedback: String,
	resource_title: Entity<ComposerInput>,
	resource_url: Entity<ComposerInput>,
	capabilities: Option<decodex_protocol::AgentCapabilitiesResult>,
	capabilities_context: Option<CatalogContext>,
	capabilities_checked: Option<std::time::Instant>,
	capability_task: Option<Task<()>>,
	capability_generation: u64,
	native_agents: NativeAgents,
	output_stream: OutputStream,
	workspace: WorkspaceView,
	profile: Option<ClientProfile>,
	snapshot: Option<AgentSnapshotDto>,
	state: LoadState,
	status_before_refresh: Option<LoadState>,
	selected: Option<String>,
	task: Option<Task<()>>,

	generation: u64,
	refresh_failures: u8,
	composer: Entity<ComposerInput>,
	fast: bool,
	service_tier: Option<ServiceTier>,
	steer: bool,
	effort_focus: FocusHandle,
	effort_drag: Option<(f32, f32)>,
	effort_pointer: Option<f32>,
	effort_track_bounds: Option<Bounds<Pixels>>,
	menu_trigger_bounds: std::collections::BTreeMap<&'static str, Bounds<Pixels>>,
	composer_menu: Option<&'static str>,
	escape_stop: Option<(String, String, std::time::Instant)>,
	interrupting: Option<(String, String)>,
	interrupt_task: Option<Task<()>>,
	composer_menu_content: Option<&'static str>,
	context_tip_visible: bool,
	attachments: Vec<AgentAttachmentDto>,
	task_references: Vec<AgentTaskReferenceDto>,
	task_reference_search: Entity<ComposerInput>,
	work_search: Entity<ComposerInput>,
	draft_profiles: Profiles,
	composer_manager: Option<String>,
	model_settings: model_settings::Panel,
	live_reviewer: live_settings::Panel,
	permission_profiles: permissions::Panel,
	task_models: models::Panel,
	hook_settings: hooks::Panel,
	app_exposure: app_exposure::Panel,
	native_goal: native_goal::Panel,
	model: Entity<ComposerInput>,
	cwd: Entity<ComposerInput>,
	account: Entity<ComposerInput>,
	effort: ConversationReasoningEffort,
	creation_setup_present: bool,
	creation_inherit_effort: bool,
	creation_intent: DesktopCreationIntent,
	creation_defaults_applied: bool,
	creation_defaults: Option<InitialModelCatalogResult>,
	sandbox: AgentSandboxDto,
	submission: SubmissionState,
	command_epoch: u64,
	sending: bool,
	uncertain: bool,
	feedback: String,
	history: Option<(String, AgentHistoryResult)>,
	history_task: Option<Task<()>>,
	history_requested_for: Option<String>,
	older_task: Option<Task<()>>,
	poll_task: Option<Task<()>>,
	request: Option<AgentRequestResult>,
	request_reader: RequestReader,
	request_task: Option<Task<()>>,
	misalignment_reviewed: Option<(String, String)>,
	guardian: guardian::Panel,
	archive: archive::Panel,
	mcp_form_event: Option<i64>,
	mcp_url_opened: Option<(i64, String)>,
	mcp_inputs: std::collections::BTreeMap<String, Entity<ComposerInput>>,
	mcp_answers: std::collections::BTreeMap<String, serde_json::Value>,
	question_timers: std::collections::BTreeMap<i64, QuestionTimer>,
	question_inputs: std::collections::BTreeMap<String, Entity<ComposerInput>>,
	question_notices: QuestionNotices,
	restored_question_drafts: Vec<DesktopQuestionDraft>,
	collapsed_async_questions: std::collections::BTreeSet<String>,
	async_question_threads: std::collections::BTreeMap<String, String>,
	async_question_choices: std::collections::BTreeMap<(String, String), ChoiceDraft>,
	async_question_inputs: std::collections::BTreeMap<(String, String), Entity<ComposerInput>>,
	accounts: Vec<(String, String)>,
}
impl AgentSurface {
	#[cfg(feature = "visual-capture")]
	#[allow(dead_code, reason = "capture-only interaction proof shares the main module")]
	pub(crate) fn visual_prepare_send(
		&mut self,
		profile: ClientProfile,
		message: &str,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
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
		snapshot: AgentSnapshotResult,
		selected: Option<String>,
		history: Option<AgentHistoryResult>,
		request: Option<AgentRequestResult>,
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
		if let Some(request) = surface.request.clone() {
			surface.prepare_question_inputs(&request, cx);
		}

		surface.feedback = "Read-only capture of a disposable service projection".into();

		surface
	}

	pub(crate) fn new(cx: &mut Context<Self>) -> Self {
		let inputs = Self::new_inputs(cx);
		let mut surface = Self::with_inputs(inputs, cx);

		surface.restore_unbound_draft(cx);

		surface
	}

	fn with_inputs(inputs: AgentInputs, cx: &mut Context<Self>) -> Self {
		let AgentInputs { model, cwd, composer } = inputs;

		Self {
			voice: None,
			retired_voice_captions: Vec::new(),
			voice_task: None,
			voice_settings: Default::default(),
			search_settings: Default::default(),
			skills: Default::default(),
			recap: Default::default(),
			prompt_edit: Default::default(),
			automatic_recap: Default::default(),
			audio_inputs: Vec::new(),
			audio_input: String::new(),
			dictation: None,
			dictation_task: None,
			activity_detail: Default::default(),
			resources: None,
			resources_task: None,
			usage_estimate: None,
			usage_estimate_task: None,
			usage_estimate_epoch: 0,
			timeline: TimelineView::default(),
			integrations: None,
			integrations_task: None,
			resource_mutation_task: None,
			resource_feedback: String::new(),
			resource_title: resource_field("Link title", "Resource title", cx),
			resource_url: resource_field("https://…", "Resource URL", cx),
			capabilities: None,
			capabilities_context: None,
			capabilities_checked: None,
			capability_task: None,
			capability_generation: 0,
			fast: false,
			service_tier: None,
			steer: true,
			effort_focus: cx.focus_handle(),
			effort_drag: None,
			effort_pointer: None,
			effort_track_bounds: None,
			menu_trigger_bounds: Default::default(),
			#[cfg(all(target_os = "macos", not(test)))]
			native_composer: Default::default(),
			composer_menu: None,
			escape_stop: None,
			interrupting: None,
			interrupt_task: None,
			composer_menu_content: None,
			context_tip_visible: false,
			attachments: vec![],
			task_references: vec![],
			task_reference_search: Self::new_task_reference_search(cx),
			work_search: Self::new_work_search(cx),
			draft_profiles: Default::default(),
			composer_manager: None,
			native_agents: Default::default(),
			output_stream: Default::default(),
			workspace: WorkspaceView::default(),
			accounts: vec![],
			composer,
			model,
			model_settings: Default::default(),
			live_reviewer: Default::default(),
			permission_profiles: Default::default(),
			task_models: Default::default(),
			hook_settings: Default::default(),
			app_exposure: Default::default(),
			native_goal: Default::default(),
			cwd,
			account: Self::account_input(cx),
			effort: ConversationReasoningEffort::High,
			creation_setup_present: false,
			creation_inherit_effort: false,
			creation_intent: Default::default(),
			creation_defaults_applied: false,
			creation_defaults: None,
			sandbox: AgentSandboxDto::ReadOnly,
			submission: Default::default(),
			command_epoch: 0,
			sending: false,
			uncertain: false,
			feedback: String::new(),
			history: None,
			history_task: None,
			history_requested_for: None,
			older_task: None,
			poll_task: None,
			request: None,
			request_reader: Default::default(),
			request_task: None,
			misalignment_reviewed: None,
			guardian: Default::default(),
			archive: Default::default(),
			mcp_form_event: None,
			mcp_url_opened: None,
			mcp_inputs: Default::default(),
			mcp_answers: Default::default(),
			question_timers: Default::default(),
			question_inputs: Default::default(),
			question_notices: Default::default(),
			restored_question_drafts: Vec::new(),
			collapsed_async_questions: Default::default(),
			async_question_threads: Default::default(),
			async_question_choices: Default::default(),
			async_question_inputs: Default::default(),
			profile: None,
			snapshot: None,
			state: LoadState::Idle,
			status_before_refresh: None,
			selected: None,
			task: None,

			generation: 0,
			refresh_failures: 0,
		}
	}

	fn new_inputs(cx: &mut Context<Self>) -> AgentInputs {
		Self::refresh_prompt(cx);

		let model =
			cx.new(|cx| ComposerInput::with_placeholder(31, "Select model", "Agent model", cx));

		model.update(cx, |input, cx| input.set_content(DEFAULT_MODEL, cx));

		let cwd = cx.new(|cx| {
			ComposerInput::with_placeholder(
				32,
				"Absolute working directory",
				"Agent working directory",
				cx,
			)
		});
		let composer =
			cx.new(|cx| ComposerInput::message(35, prompts::next(), "Agent message", cx));

		cx.subscribe(&composer, |s, _, event, cx| {
			if let ComposerEvent::Attach(item) = event {
				s.attach_clipboard(item, cx);
			}

			cx.notify();
		})
		.detach();

		AgentInputs { model, cwd, composer }
	}

	fn account_input(cx: &mut Context<Self>) -> Entity<ComposerInput> {
		cx.new(|cx| {
			ComposerInput::with_placeholder(
				33,
				"Automatic account routing",
				"Optional exact account ID",
				cx,
			)
		})
	}

	fn load_history(&mut self, cx: &mut Context<Self>) {
		if self.is_new_conversation() {
			return;
		}
		if self.native_agents.selected.is_none()
			&& self.command_connection_ready()
			&& let Some(work) =
				self.conversation_work().filter(|w| w.kind == AgentWorkKindDto::Task)
			&& let Some(thread) = work.codex_thread_id
		{
			self.enter_native_conversation(&work.id, &thread, cx);
			return;
		}
		if self.native_agents.selected.is_some() {
			self.refresh_open_native_history(cx);
			return;
		}
		self.refresh_open_native_history(cx);
		self.refresh_native_input_receipts(cx);
		self.load_guardian_reviews(cx);
		self.load_archive_state(false, cx);

		if self.history.as_ref().is_some_and(|(id, _)| self.selected.as_ref() != Some(id)) {
			self.history = None;
		}

		let (Some(profile), Some(id)) = (self.profile.clone(), self.selected.clone()) else {
			return;
		};

		if self.history_task.is_some() && self.history_requested_for.as_ref() == Some(&id) {
			return;
		}

		let question_scope = self.question_notice_scope(&id);

		self.question_notices.begin(question_scope.clone());

		self.history_requested_for = Some(id.clone());
		self.timeline.read_at = Some(std::time::Instant::now());

		let Ok(work_id) = EntityId::new(id.clone()) else {
			return;
		};
		let request = cx.background_executor().spawn(async move {
			let Ok(runtime) = Builder::new_current_thread().enable_all().build() else {
				return AgentHistoryResult::Unavailable;
			};

			runtime
				.block_on(AgentClient::new(profile).history(work_id))
				.unwrap_or(AgentHistoryResult::Unavailable)
		});

		self.history_task = Some(cx.spawn(async move |surface, cx| {
			let history = request.await;
			let _ = surface.update(cx, |surface, cx| {
				surface.history_task = None;

				if surface.question_notice_scope(&id) != question_scope {
					return;
				}
				if surface.selected.as_ref() == Some(&id) {
					surface.observe_question_notices(&history);
					cx.notify();

					if surface
						.history
						.as_ref()
						.is_some_and(|(owner, previous)| owner == &id && previous == &history)
					{
						return;
					}
					if matches!(history, AgentHistoryResult::Available { .. })
						|| !surface.history.as_ref().is_some_and(|(current, saved)| {
							current == &id && matches!(saved, AgentHistoryResult::Available { .. })
						}) {
						if let Some(scroll) = surface.timeline.scroll.get(&id)
							&& surface.voice.is_none()
							&& !surface.timeline.follow_paused.contains(&id)
							&& (scroll.offset().y + scroll.max_offset().y).abs() < gpui::px(24.0)
						{
							surface.timeline.latest_follow_work = Some(id.clone());
						}

						if surface.feedback == "Message saved · Waiting for agent…" {
							let last_reply = |history: &AgentHistoryResult| match history {
								AgentHistoryResult::Available { entries, .. } => entries
									.iter()
									.filter(|e| e.kind == "assistant")
									.map(|e| e.id)
									.max(),
								_ => None,
							};
							let previous = surface
								.history
								.as_ref()
								.filter(|(owner, _)| owner == &id)
								.and_then(|(_, old)| last_reply(old));

							if last_reply(&history) > previous {
								surface.feedback.clear();
							}
						}

						surface.reconcile_voice_captions(&id, &history);
						surface.prepare_async_question_inputs(&id, &history, cx);
						surface.timeline.cache.insert(id.clone(), history.clone());

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
		if self.root_id().is_none() && self.creation_setup(cx).is_some() {
			self.creation_setup_present = true;
		}
		if !self.creation_setup_present
			&& self.cwd.read(cx).content().is_empty()
			&& let Some(cwd) = cwd
		{
			self.cwd.update(cx, |input, cx| input.set_content(cwd.as_str(), cx));

			self.creation_setup_present = true;
		}

		self.accounts = accounts;

		cx.notify();
	}

	#[cfg(test)]
	fn cycle_model(&mut self, cx: &mut Context<Self>) {
		if let Some(decodex_protocol::AgentCapabilitiesResult::Available { models, .. }) =
			self.current_model_catalog(cx)
		{
			if models.is_empty() {
				return;
			}

			let next = models
				.iter()
				.position(|model| model.model.as_str() == self.model.read(cx).content())
				.map_or(0, |index| (index + 1) % models.len());
			let model = models[next].model.as_str().to_owned();

			self.select_composer_option("model", &model, cx);
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
		let Some(snapshot) = self.snapshot.as_ref() else { return };
		let Some(event) = snapshot
			.pending_events
			.iter()
			.find(|event| {
				event.id == event_id && self.selected.as_ref() == Some(&event.work_item_id)
			})
			.cloned()
		else {
			return;
		};
		let source = RequestReadSource {
			profile_epoch: self.command_epoch,
			runtime_source: snapshot.runtime_source.clone(),
			event,
		};

		self.request = None;

		let request = cx.background_executor().spawn(async move {
			let Ok(runtime) = Builder::new_current_thread().enable_all().build() else {
				return AgentRequestResult::Unavailable;
			};

			runtime
				.block_on(AgentClient::new(profile).request(event_id))
				.unwrap_or(AgentRequestResult::Unavailable)
		});

		self.request_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				surface.finish_request(source, result, cx);
			});
		}));

		cx.notify();
	}

	fn finish_request(
		&mut self,
		source: RequestReadSource,
		result: AgentRequestResult,
		cx: &mut Context<Self>,
	) {
		if self.command_epoch != source.profile_epoch {
			return;
		}

		self.request_task = None;

		if self.selected.as_ref() == Some(&source.event.work_item_id)
			&& matches!(self.displayed_load_state(), LoadState::Ready | LoadState::Loading)
			&& self.snapshot.as_ref().is_some_and(|snapshot| {
				snapshot.runtime_source == source.runtime_source
					&& snapshot.pending_events.contains(&source.event)
			}) {
			if let Some(scroll) = self.timeline.scroll.get(&source.event.work_item_id)
				&& self.voice.is_none()
				&& !self.timeline.follow_paused.contains(&source.event.work_item_id)
				&& (scroll.offset().y + scroll.max_offset().y).abs() < gpui::px(24.0)
			{
				scroll.scroll_to_bottom();
			}

			self.prepare_question_inputs(&result, cx);

			self.request = Some(result);

			cx.notify();
		}
	}

	fn respond(&mut self, json: String, cx: &mut Context<Self>) {
		let Some(AgentRequestResult::Available { event_id, work_id, method, request_json }) =
			&self.request
		else {
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

		let Ok(work_id) = EntityId::new(work_id.clone()) else { return };

		if json.len() > MAX_HISTORY_INLINE_BYTES
			&& let (Ok(params), Ok(response)) =
				(serde_json::from_str(request_json.as_str()), serde_json::from_str(&json))
			&& let Some(decision) =
				AgentRequestedDecision::matching_response(method, &params, &response)
		{
			self.execute(
				AgentActionDto::RespondWithRequestedDecision {
					work_id,
					event_id: *event_id,
					decision,
				},
				None,
				cx,
			);

			return;
		}

		let Ok(response_json) = HistoryText::new(json) else {
			self.feedback = format!(
				"Response is too large after encoding (limit {} bytes). Shorten your answers and retry; your entries are preserved.",
				decodex_protocol::MAX_HISTORY_INLINE_BYTES
			);

			cx.notify();

			return;
		};

		self.execute(
			AgentActionDto::Respond { work_id, event_id: *event_id, response_json },
			None,
			cx,
		);
	}

	fn build_submission(&self, text: &str, cx: &Context<Self>) -> Result<AgentActionDto, String> {
		let prompt = HistoryText::new(if text.trim().is_empty() {
			"Please use the selected skills and inspect the selected tasks and files.".into()
		} else {
			text.to_owned()
		})
		.map_err(|_| "Message is too long")?;

		if self.is_new_conversation() {
			return Ok(AgentActionDto::NewConversation {
				workspace_id: self
					.workspace
					.new_conversation_workspace
					.as_deref()
					.map(EntityId::new)
					.transpose()
					.map_err(|_| "Invalid workspace")?,
				work_id: EntityId::new(self.selected.clone().expect("draft identity"))
					.map_err(|_| "Invalid conversation identity")?,
				text: prompt,
				execution: self.draft_profiles.execution.choice(self.selected.as_deref().unwrap()),
				attachments: self.attachments.clone(),
				task_references: self.task_references.clone(),
			});
		}

		if !self.draft_owner_available() {
			return Err(
				"This draft's conversation is unavailable. Select a conversation before sending."
					.into(),
			);
		}

		if let Some(root) = self.snapshot.as_ref().and_then(|snapshot| {
			snapshot
				.work_items
				.iter()
				.find(|work| {
					Some(&work.id) == self.composer_manager.as_ref().or(self.selected.as_ref())
						&& work.kind == AgentWorkKindDto::Manager
				})
				.or_else(|| snapshot.work_items.iter().find(|work| work.parent_goal_id.is_none()))
		}) {
			return Ok(AgentActionDto::Send {
				root_id: EntityId::new(root.id.clone()).map_err(|_| "Invalid agent identity")?,
				text: prompt,
			});
		}

		if self.state != LoadState::Ready {
			return Err("Wait for conversations to load before sending.".into());
		}

		Ok(AgentActionDto::Start(AgentStartDto {
			root_id: EntityId::new(format!("agent-{}", unique_command()))
				.map_err(|_| "Invalid agent identity")?,
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
			effort: self.creation_effort(),
			sandbox: self.sandbox,
		}))
	}

	fn submit(&mut self, cx: &mut Context<Self>) {
		if self.native_agents.selected.is_some() {
			self.send_native_agent(cx);
			return;
		}
		// Live uses this slot for the microphone. Do not send a hidden draft.
		if self.voice.is_some() {
			return;
		}
		if self.composer_unavailable_reason().is_some() {
			cx.notify();

			return;
		}
		if self.selected_is_archived() {
			return;
		}
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

		if text.trim().is_empty() && self.attachments.is_empty() && self.task_references.is_empty()
		{
			return;
		}

		match self.build_submission(&text, cx) {
			Ok(action) => {
				let Ok(model) = ConversationModel::new(self.model.read(cx).content()) else {
					return;
				};
				let execution = AgentExecutionOverrides {
					model: Some(model),
					reasoning_effort: self.creation_effort(),
					fast: Some(self.fast),
					service_tier: self.service_tier.clone(),
				};
				let attachments = self.attachments.clone();
				let action = match action {
					AgentActionDto::Start(start) => AgentActionDto::StartConfigured {
						start,
						execution,
						attachments,
						task_references: self.task_references.clone(),
					},
					AgentActionDto::Send { root_id, text } =>
						self.configured_send(root_id, text, attachments),
					action => action,
				};

				if matches!(&action, AgentActionDto::NewConversation { .. }) {
					self.workspace.opening_work = self.selected.clone();
				}
				self.execute(action, Some(text), cx);
			},
			Err(message) => {
				self.feedback = message;

				cx.notify();
			},
		}
	}

	fn command_connection_ready(&self) -> bool {
		*self.displayed_load_state() == LoadState::Ready
			|| (self.state == LoadState::Loading
				&& self.status_before_refresh.is_none()
				&& self.snapshot.is_some())
	}

	pub(super) fn steer_identity(
		&self,
		action: &AgentActionDto,
		key: &IdempotencyKey,
	) -> Option<decodex_protocol::AgentSteerIdentity> {
		let AgentActionDto::Steer { work_id, turn_id, .. } = action else { return None };
		let work =
			self.snapshot.as_ref()?.work_items.iter().find(|work| work.id == work_id.as_str())?;

		Some(decodex_protocol::AgentSteerIdentity {
			work_id: work_id.clone(),
			thread_id: WireText::new(work.codex_thread_id.clone()?).ok()?,
			turn_id: turn_id.clone(),
			submission_id: key.clone(),
		})
	}

	fn execute(&mut self, action: AgentActionDto, draft: Option<String>, cx: &mut Context<Self>) {
		if matches!(
			&action,
			AgentActionDto::Send { .. }
				| AgentActionDto::SendConfigured { .. }
				| AgentActionDto::Steer { .. }
				| AgentActionDto::NativeAgentInput { .. }
				| AgentActionDto::AnswerQuestion { .. }
				| AgentActionDto::SkipQuestion { .. }
		) {
			self.reset_recap();
			self.reset_prompt_edit();
		}
		if self.draft_quit_in_progress() {
			return;
		}

		if let AgentActionDto::Interrupt { work_id, turn_id } = action {
			self.request_interrupt(work_id, turn_id, cx);

			return;
		}

		if self.sending || self.uncertain {
			return;
		}

		let Some(profile) = self.profile.clone() else {
			self.feedback = "No service profile is configured.".into();

			cx.notify();

			return;
		};

		if !self.command_connection_ready() {
			self.feedback =
				"Connection unavailable. Draft retained; refresh before sending.".into();

			cx.notify();

			return;
		}

		let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let recovery = if draft.is_some() {
			match self.command_draft_copy(cx) {
				Ok(copy) => Some(copy),
				Err(message) => {
					self.feedback = message.into();

					cx.notify();

					return;
				},
			}
		} else {
			None
		};
		let mut pending = PendingCommand {
			recovery,
			key: Some(key.clone()),
			steer: self.steer_identity(&action, &key),
			execution_intent: self.draft_profiles.execution.capture(&action),
			epoch: self.command_epoch,
			attachments: draft.as_ref().map(|_| self.attachments.clone()),
			references: draft.as_ref().map(|_| self.task_references.clone()),
			owner: self.composer_manager.clone().or_else(|| self.root_id()),
			draft,
		};

		self.fence_command_draft(&mut pending);
		self.capture_send_preview(&pending);

		if pending.draft.is_some() {
			if self.workspace.preview_page == self.conversation_page() {
				self.workspace.preview_page = None;
			}
			self.follow_latest_after_send(cx);
		}

		self.sending = true;
		self.feedback = "Sending…".into();

		if pending.steer.is_some() {
			self.submission.pending = Some(pending.clone());
		}

		self.submission.unconfirmed.push(key.clone());

		self.submission.waiting = Some(QueuedCommand { profile, action, key, pending });

		self.save_draft_document(cx);
		cx.notify();
	}

	fn dispatch_saved_command(&mut self, queued: QueuedCommand, cx: &mut Context<Self>) {
		let QueuedCommand { profile, action, key, pending } = queued;

		if self.command_epoch != pending.epoch || self.profile.as_ref() != Some(&profile) {
			return;
		}
		if !self.command_connection_ready() {
			self.finish_command(
				pending,
				Err("Connection changed before dispatch. Draft retained.".into()),
				cx,
			);

			return;
		}

		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "Cannot create client runtime".to_string())?;

			runtime
				.block_on(AgentClient::new(profile).execute(action, key))
				.map_err(|error| format!("Request failed before dispatch: {error:?}"))
		});

		self.submission.command = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				surface.finish_command(pending, result, cx);
			});
		}));

		cx.notify();
	}

	fn finish_command(
		&mut self,
		pending: PendingCommand,
		result: Result<AgentCommandResponse, String>,
		cx: &mut Context<Self>,
	) {
		if self.command_epoch != pending.epoch {
			return;
		}

		self.sending = false;

		self.finish_send_preview(
			&pending,
			matches!(&result, Ok(AgentCommandResponse::Accepted { .. })),
		);
		self.remove_command_draft_fence(&pending);

		if !matches!(&result, Ok(AgentCommandResponse::Accepted { .. })) {
			self.retain_failed_command_draft(
				&pending,
				matches!(&result, Ok(AgentCommandResponse::PotentiallyDispatched { .. })),
				cx,
			);
		}
		if !matches!(&result, Ok(AgentCommandResponse::PotentiallyDispatched { .. })) {
			self.submission.unconfirmed.retain(|key| Some(key) != pending.key.as_ref());
		}
		// A different command must not erase an unresolved steering receipt.
		if !matches!(&result, Ok(AgentCommandResponse::PotentiallyDispatched { .. }))
			&& pending.steer.is_some()
			&& self.submission.pending.as_ref().and_then(|saved| saved.steer.as_ref())
				== pending.steer.as_ref()
		{
			self.submission.pending = None;
		}

		let current_owner = self.composer_manager.clone().or_else(|| self.root_id());
		let same_owner = current_owner == pending.owner
			|| (pending.owner.is_none()
				&& matches!(&result, Ok(AgentCommandResponse::Accepted { work_id }) if current_owner.as_deref()==Some(work_id.as_str())));

		if !same_owner
			&& matches!(&result, Ok(AgentCommandResponse::Accepted { .. }))
			&& let Some(owner) = &pending.owner
			&& self.draft_profiles.texts.get(owner).map(String::as_str) == pending.draft.as_deref()
		{
			self.draft_profiles.texts.remove(owner);
		}
		if matches!(&result, Ok(AgentCommandResponse::Accepted { .. }))
			&& let Some(sent) = &pending.attachments
		{
			if same_owner {
				self.attachments.retain(|file| !sent.contains(file));
			} else if let Some(files) =
				pending.owner.as_ref().and_then(|id| self.draft_profiles.files.get_mut(id))
			{
				files.retain(|file| !sent.contains(file));
			}
		}
		if matches!(&result, Ok(AgentCommandResponse::Accepted { .. }))
			&& let Some(sent) = &pending.references
		{
			self.clear_sent_task_references(sent, same_owner, pending.owner.as_deref());
		}
		if matches!(&result, Ok(AgentCommandResponse::Accepted { .. })) {
			self.draft_profiles.execution.accepted(pending.execution_intent.as_ref());
		}

		self.apply_command_result(
			result,
			if same_owner { pending.draft.as_deref() } else { None },
			cx,
		);
		self.save_draft_document(cx);
		self.refresh(cx);
		cx.notify();
	}

	fn apply_command_result(
		&mut self,
		result: Result<AgentCommandResponse, String>,
		draft: Option<&str>,
		cx: &mut Context<Self>,
	) {
		let surface = self;

		match result {
			Ok(AgentCommandResponse::Accepted { .. }) => {
				surface.feedback = if draft.is_some() {
					"Message saved · Waiting for agent…".into()
				} else {
					String::new()
				};

				if draft == Some(surface.composer.read(cx).content()) {
					surface.composer.update(cx, |input, cx| {
						input.clear(cx);
						input.set_placeholder(prompts::next(), cx);
					});

					Self::refresh_prompt(cx);
				}
			},
			Ok(AgentCommandResponse::Rejected { error }) =>
				surface.feedback = format!("Not accepted: {error:?}. Draft retained."),
			Ok(AgentCommandResponse::PotentiallyDispatched { failure }) => {
				surface.uncertain = true;
				surface.feedback = format!(
					"Acceptance unknown: {failure:?}. Draft retained. Sending is blocked to prevent duplicate effects; inspect history and service state."
				);
			},
			Err(message) => surface.feedback = message,
		}
	}

	fn cancel_queued_command(&mut self, cx: &Context<Self>) {
		if let Some(queued) = self.submission.waiting.take() {
			self.remove_command_draft_fence(&queued.pending);
			self.retain_failed_command_draft(&queued.pending, false, cx);
			self.submission.unconfirmed.retain(|key| key != &queued.key);

			if queued.pending.steer.is_some() {
				self.submission.pending = None;
			}

			self.sending = false;
		}
	}

	fn reset_profile_panels(&mut self, cx: &mut Context<Self>) {
		self.clear_activity_detail();
		self.reset_resources();
		self.clear_usage_estimate();
		self.reset_integrations();
		self.resource_feedback.clear();
		self.resource_title.update(cx, |input, cx| input.clear(cx));
		self.resource_url.update(cx, |input, cx| input.clear(cx));
		self.reset_capabilities();

		if self.composer_manager.is_some() {
			self.fast = false;
			self.service_tier = None;
		}

		self.reset_model_settings();
		self.reset_live_reviewer();
		self.reset_permission_profiles();
		self.reset_task_models();
		self.reset_hook_settings();
		self.reset_app_exposure();
		self.reset_voice_settings();
		self.reset_search_settings();
		self.reset_skill_picker();
		self.reset_recap();
		self.reset_prompt_edit();
		self.reset_native_goal();
	}

	pub(crate) fn bind_profile(&mut self, profile: Option<ClientProfile>, cx: &mut Context<Self>) {
		self.reset_automatic_recap();
		self.close_native_agent(cx);
		self.reset_native_agents();
		self.cancel_queued_command(cx);

		self.command_epoch += 1;
		self.submission.command = None;
		self.submission.receipt_task = None;

		if self.sending {
			self.uncertain = true;
			self.sending = false;
			self.feedback = "Service changed before acceptance was confirmed. Draft retained; inspect the previous service before sending again.".into();
		}

		self.question_notices = Default::default();

		self.bind_drafts(profile.as_ref(), cx);

		let epoch = self.timeline.native.epoch + 1;

		self.timeline.native = Default::default();
		self.timeline.native.epoch = epoch;
		self.generation += 1;
		self.refresh_failures = 0;
		self.task = None;
		self.profile = profile;
		self.output_stream = Default::default();
		self.interrupting = None;
		self.interrupt_task = None;
		self.timeline.read_at = None;

		self.reset_profile_panels(cx);

		self.snapshot = None;

		self.workspace.pages.clear();
		self.native_agents.pages.clear();
		self.native_agents.timelines.clear();
		self.workspace.closing_pages.clear();
		self.workspace.page_views.clear();

		self.workspace.graph_expanded = false;

		self.timeline.cache.clear();
		self.timeline.marks.clear();

		self.timeline.marks_work = None;

		self.timeline.older_history.clear();

		self.older_task = None;
		self.timeline.loading_older = false;
		self.timeline.older_retry_after = None;

		self.timeline.scroll.clear();
		self.timeline.follow_paused.clear();

		self.workspace.graph_scope = None;
		self.workspace.graph_selected = None;
		self.history = None;
		self.history_task = None;
		self.request = None;
		self.request_task = None;

		self.question_timers.clear();

		self.mcp_form_event = None;
		self.mcp_url_opened = None;

		self.mcp_inputs.clear();
		self.mcp_answers.clear();

		self.misalignment_reviewed = None;
		self.guardian = Default::default();
		self.archive = Default::default();
		self.selected = self.composer_manager.clone();
		self.state = LoadState::Idle;
		self.poll_task = Some(cx.spawn(async move |surface, cx| {
			loop {
				cx.background_executor().timer(Duration::from_millis(500)).await;

				if surface
					.update(cx, |surface, cx| {
						surface.save_draft_document(cx);

						if should_poll_snapshot(surface.profile.is_some(), &surface.state) {
							surface.refresh(cx);
						}

						surface.load_archive_state(false, cx);

						if surface.guardian_needs_refresh() {
							surface.load_guardian_reviews(cx);
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

	fn disconnect_panels(&mut self) {
		self.interrupt_automatic_recap();
		self.reset_native_agents();
		self.guardian_disconnected();
		self.reset_resources();
		self.reset_integrations();
		self.reset_model_settings();
		self.reset_live_reviewer();
		self.reset_permission_profiles();
		self.reset_task_models();
		self.reset_hook_settings();
		self.reset_app_exposure();
		self.reset_voice_settings();
		self.reset_search_settings();
		self.reset_skill_picker();
		self.reset_recap();
		self.reset_prompt_edit();
		self.reset_native_goal();

		self.question_notices = Default::default();

		self.clear_activity_detail();
		self.clear_usage_estimate();
	}

	pub(crate) fn mark_stale(&mut self, cx: &mut Context<Self>) {
		self.disconnect_panels();
		self.reset_capabilities();

		self.output_stream = Default::default();
		self.generation += 1;

		self.archive_disconnected();

		self.task = None;
		self.state =
			if self.snapshot.is_some() { LoadState::Stale } else { LoadState::Unavailable };

		cx.notify();
	}

	pub(crate) fn refresh(&mut self, cx: &mut Context<Self>) {
		self.refresh_steer_receipt(cx);

		if self.state == LoadState::Loading {
			return;
		}

		let Some(profile) = self.profile.clone() else {
			self.state = LoadState::Unavailable;

			cx.notify();

			return;
		};

		self.status_before_refresh = Some(self.state.clone());
		self.state = LoadState::Loading;
		self.generation += 1;

		let generation = self.generation;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().map_err(|_| ())?;

			runtime.block_on(AgentClient::new(profile).query()).map_err(|_| ())
		});

		self.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				if surface.generation != generation {
					return;
				}

				let changed = match &result {
					Ok(AgentSnapshotResult::Available(snapshot)) =>
						surface.snapshot.as_ref() != Some(snapshot),
					_ => true,
				};

				surface.apply_result(result);
				surface.refresh_native_goal(cx);

				if !surface.connection_initializing()
					&& (surface.current_model_catalog(cx).is_none()
						|| surface
							.capabilities_checked
							.is_none_or(|at| at.elapsed().as_secs() >= 60))
				{
					surface.load_capabilities(cx);
				}
				if changed || surface.timeline.read_at.is_none_or(|at| at.elapsed().as_secs() >= 2)
				{
					surface.load_history(cx);
				}

				surface.sync_request(cx);
				surface.refresh_composer_model_settings(cx);
				surface.tick_question_timeout(cx);
				cx.notify();
			});
		}));

		cx.notify();
	}

	fn apply_result(&mut self, result: Result<AgentSnapshotResult, ()>) {
		if !matches!(&result, Ok(AgentSnapshotResult::Available(_))) {
			self.disconnect_panels();
		}

		match result {
			Ok(AgentSnapshotResult::Available(snapshot)) => {
				self.invalidate_native_agents(&snapshot);
				self.invalidate_guardian(&snapshot);
				self.invalidate_resources(&snapshot);
				self.invalidate_integrations(&snapshot);
				self.invalidate_usage_estimate(&snapshot);
				self.invalidate_model_settings(&snapshot);
				self.invalidate_live_reviewer_for_snapshot(&snapshot);
				self.invalidate_permission_profiles(&snapshot);
				self.invalidate_task_models(&snapshot);
				self.invalidate_hook_settings(&snapshot);
				self.invalidate_app_exposure(&snapshot);
				self.invalidate_voice_settings(&snapshot);
				self.invalidate_search_settings(&snapshot);
				self.invalidate_recap(&snapshot);
				self.invalidate_prompt_edit(&snapshot);
				self.invalidate_native_goal(&snapshot);

				if self.snapshot.as_ref().is_some_and(|old| {
					old.runtime_source != snapshot.runtime_source
						|| old.work_items.iter().any(|work| {
							snapshot
								.work_items
								.iter()
								.find(|new| new.id == work.id)
								.is_none_or(|new| new.codex_thread_id != work.codex_thread_id)
						})
				}) {
					self.clear_activity_detail();
				}

				self.refresh_failures = 0;

				if self.interrupting.as_ref().is_some_and(|(id, turn)| {
					snapshot
						.work_items
						.iter()
						.any(|w| &w.id == id && w.active_turn_id.as_ref() != Some(turn))
				}) {
					self.interrupting = None;
				}
				if self.feedback == "Message saved · Waiting for agent…"
					&& snapshot.work_items.iter().any(|work| {
						Some(&work.id) == self.selected.as_ref()
							&& (work.dispatch_state != AgentDispatchStateDto::Idle
								|| !snapshot.pending_events.iter().any(|event| {
									event.work_item_id == work.id
										&& event.event_kind == "user_message"
										&& !event.delivery_claimed
								}))
					}) {
					self.feedback.clear();
				}
				if self.snapshot.as_ref().and_then(|old| old.runtime_source.as_ref())
					!= snapshot.runtime_source.as_ref()
				{
					self.timeline.native.reset();
				}
				if !self.is_new_conversation()
					&& !self
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

				// A saved first message promotes the local editor to a real conversation,
				// including when acceptance was recovered after restarting the app.
				if self
					.workspace
					.new_conversation
					.as_ref()
					.is_some_and(|id| snapshot.work_items.iter().any(|work| &work.id == id))
				{
					self.workspace.new_conversation = None;
				}

				self.snapshot = Some(snapshot);
				self.state = LoadState::Ready;
			},
			Ok(AgentSnapshotResult::CapacityExceeded {
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
			Ok(AgentSnapshotResult::Unavailable) => {
				self.state =
					if self.snapshot.is_some() { LoadState::Stale } else { LoadState::Unavailable };
			},
			Err(()) => {
				self.refresh_failures = self.refresh_failures.saturating_add(1);

				let confirmed = *self.displayed_load_state() == LoadState::Ready;

				self.state = if self.snapshot.is_some() && confirmed && self.refresh_failures < 3 {
					LoadState::Ready
				} else if self.snapshot.is_some() {
					LoadState::Stale
				} else {
					LoadState::Unavailable
				};
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

	fn thread_in_use(&self, work: &str) -> bool {
		self.snapshot.as_ref().is_some_and(|snapshot| {
			snapshot.pending_events.iter().any(|event| {
				event.work_item_id == work && event.event_kind == "thread_in_use_needs_attention"
			})
		})
	}

	pub(crate) fn operation_notices(&self) -> Vec<(&'static str, String)> {
		let histories =
			self.history.iter().map(|(_, history)| history).chain(self.timeline.cache.values());
		let mut notices: Vec<_> =
			[("Review", &self.guardian.feedback), ("Task resources", &self.resource_feedback)]
				.into_iter()
				.filter(|(_, detail)| !detail.is_empty())
				.map(|(title, detail)| (title, detail.clone()))
				.collect();
		let mut seen = std::collections::BTreeSet::new();

		for history in histories {
			if let AgentHistoryResult::Available { entries, .. } = history {
				for entry in entries.iter().filter(|entry| startup_feature_warning(entry)) {
					if seen.insert(entry.text.clone()) {
						notices.push(("Experimental Codex features", entry.text.clone()));
					}
				}
			}
		}
		for (entries, _) in self.timeline.older_history.values() {
			for entry in entries.iter().filter(|entry| startup_feature_warning(entry)) {
				if seen.insert(entry.text.clone()) {
					notices.push(("Experimental Codex features", entry.text.clone()));
				}
			}
		}

		notices
	}

	pub(crate) fn status_notice(&self) -> Option<(&'static str, String, bool)> {
		if self.connection_initializing() {
			return None;
		}
		if !self.sending
			&& !self.feedback.is_empty()
			&& self.feedback != "Message saved · Waiting for agent…"
		{
			return Some((
				if self.uncertain { "Check delivery" } else { "Message status" },
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
				"In use by another app",
				format!("{name} is in use by another app."),
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
					format!("{} · {}.", pending.work_item_id, pending.event_kind.replace('_', " ")),
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
			LoadState::Idle => "Refresh to load your work.".into(),
			LoadState::Loading => if self.snapshot.is_some() {
				"Refreshing · showing the previous snapshot"
			} else {
				"Loading work…"
			}
			.into(),
			LoadState::Ready => "Saved conversation and work status".into(),
			LoadState::Unavailable => if self.profile.is_none() {
				"No local service profile is configured for this view."
			} else {
				"Reconnecting… Your work may still be running. See Settings → Diagnostics for details."
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

	pub(super) fn work_context(
		&self,
		window: &Window,
		_cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		if self.is_new_conversation() && !self.workspace.browsing {
			return gpui::div().into_any_element();
		}
		let title = if self.workspace.browsing {
			"All work".into()
		} else {
			self.conversation_page()
				.and_then(|id| {
					self.native_page_label(&id).or_else(|| {
						self.snapshot
							.as_ref()?
							.work_items
							.iter()
							.find(|w| w.id == id)
							.map(|w| self.work_label(w))
					})
				})
				.unwrap_or_else(|| "Main".into())
		};
		let tip = title.clone();
		let (left, right) = self.topbar_insets(window);
		let available = (f32::from(window.viewport_size().width) - left - right).min(360.) - 20.;
		let run = gpui::TextRun {
			len: title.len(),
			font: window.text_style().font(),
			color: gpui::rgb(ui_theme::TEXT_MUTED).into(),
			background_color: None,
			underline: None,
			strikethrough: None,
		};
		let truncated = f32::from(
			window
				.text_system()
				.shape_line(title.clone().into(), gpui::px(12.), &[run], None)
				.width,
		) > available;
		gpui::div()
			.id("current-conversation-title")
			.when(truncated, |title| {
				title.tooltip(move |_, cx| {
					cx.new(|_| crate::shell::ControlTooltip(tip.clone())).into()
				})
			})
			.debug_selector(|| "workspace-conversation-header".into())
			.h(gpui::px(ui_theme::CONTROL_GROUP_HEIGHT))
			.w_full()
			.max_w_full()
			.min_w_0()
			.flex()
			.items_center()
			.justify_center()
			.text_center()
			.px(gpui::px(10.))
			.text_size(gpui::px(12.))
			.text_color(gpui::rgb(ui_theme::TEXT_MUTED))
			.child(gpui::div().min_w_0().whitespace_nowrap().text_ellipsis().child(title))
			.into_any_element()
	}

	pub(super) fn work_details_button(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let target = cx.entity();
		gpui::div()
			.relative()
			.flex_none()
			.child(self.workspace_action(
				"inspect-work".into(),
				"⋯".into(),
				|s, cx| {
					s.workspace.details_visible = !s.workspace.details_visible;

					if s.workspace.details_visible
						&& let Some(work) = s.selected.clone()
						&& s.resources.as_ref().is_none_or(|(owner, _)| owner != &work)
					{
						s.toggle_resources(&work, cx);
					}

					cx.notify();
				},
				cx,
			))
			.child(
				gpui::canvas(
					move |bounds, _, cx| {
						target.update(cx, |s, _| {
							s.menu_trigger_bounds.insert("inspect-work", bounds);
						});
					},
					|_, _, _, _| {},
				)
				.absolute()
				.inset_0(),
			)
			.into_any_element()
	}

	fn details(
		&self,
		snapshot: &AgentSnapshotDto,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		gpui::div()
			.id("agent-work-detail")
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
			.child(self.misalignment_panel(work, cx))
			.child(self.guardian_panel(work, cx))
			.child(self.request_panel(snapshot, work, cx))
			.child(self.async_question_panel(work, cx))
			.child(self.history_panel(work, cx))
	}

	pub(super) fn prefetch_older_history(&mut self, cx: &mut Context<Self>) {
		if self.prefetch_native_history(cx) {
			return;
		}
		if self.history_prefetch_needed() {
			self.load_older_history(cx);
		}
	}

	fn history_prefetch_needed(&self) -> bool {
		if self.timeline.loading_older || self.timeline.older_scroll_anchor.is_some() {
			return false;
		}

		let Some(id) =
			self.selected.as_ref().filter(|id| self.timeline.follow_paused.contains(*id))
		else {
			return false;
		};
		let Some((owner, AgentHistoryResult::Available { next_before, .. })) =
			self.history.as_ref()
		else {
			return false;
		};

		if owner != id
			|| self
				.timeline
				.older_history
				.get(id)
				.map_or(*next_before, |(_, cursor)| *cursor)
				.is_none()
		{
			return false;
		}

		self.timeline.scroll.get(id).is_some_and(|scroll| {
			let height = f32::from(scroll.bounds().size.height);

			height > 0. && -f32::from(scroll.offset().y) <= (height * 0.6).clamp(240., 600.)
		})
	}

	fn load_older_history(&mut self, cx: &mut Context<Self>) {
		if self.timeline.loading_older
			|| self.timeline.older_retry_after.is_some_and(|at| at > std::time::Instant::now())
		{
			return;
		}

		let (Some(profile), Some((id, AgentHistoryResult::Available { next_before, .. }))) =
			(self.profile.clone(), self.history.as_ref())
		else {
			return;
		};

		if self.selected.as_ref() != Some(id) {
			return;
		}

		let before =
			self.timeline.older_history.get(id).map_or(*next_before, |(_, cursor)| *cursor);
		let Some(before) = before else {
			return;
		};
		let id = id.clone();
		let Ok(work) = EntityId::new(id.clone()) else {
			return;
		};

		self.timeline.loading_older = true;

		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime.block_on(AgentClient::new(profile).history_page(work, Some(before))).ok()
		});

		self.older_task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				s.timeline.loading_older = false;

				if let Some(AgentHistoryResult::Available { entries, next_before, .. }) = result
					&& next_before.is_none_or(|next| next < before)
				{
					s.timeline.older_retry_after = None;

					if s.selected.as_ref() == Some(&id)
						&& let Some(scroll) = s.timeline.scroll.get(&id)
					{
						s.timeline.older_scroll_anchor = Some(activity::HistoryScrollAnchor {
							work: id.clone(),
							offset: f32::from(scroll.offset().y),
							maximum: f32::from(scroll.max_offset().y),
							message: s
								.timeline
								.marks
								.iter()
								.next()
								.map(|(id, mark)| (id.clone(), mark.position.get())),
						});
					}

					let page = s.timeline.older_history.entry(id).or_default();

					page.0.extend(entries);
					page.0.sort_by_key(|entry| entry.id);
					page.0.dedup_by_key(|entry| entry.id);

					page.1 = next_before;
				} else {
					s.timeline.older_retry_after =
						Some(std::time::Instant::now() + Duration::from_secs(3));
				}

				cx.notify();
			});
		}));

		cx.notify();
	}

	fn history_panel(&self, work: &AgentWorkItemDto, cx: &mut Context<Self>) -> impl IntoElement {
		if self.native_agents.selected.is_some() {
			return self.history_activity(
				gpui::div().w_full().min_w_0().child(self.native_timeline_panel(work, cx)),
				work,
			);
		}
		let panel = gpui::div()
			.w_full()
			.min_w_0()
			.flex_none()
			.flex()
			.flex_col()
			.gap(gpui::px(MESSAGE_GAP))
			.when(self.prompt_editor_visible(&work.id), |panel| {
				panel.child(self.prompt_edit_panel(&work.id, cx))
			})
			.child(self.native_timeline_panel(work, cx));

		if self.native_history_active(work) {
			return self.history_activity(
				panel
					.child(self.native_receipts_panel(work, false, cx))
					.children(self.live_chat_caption(&work.id)),
				work,
			);
		}
		// Local records use a different grouping and must not flash before the
		// native transcript arrives. They remain available after a failed read.
		if self.native_history_loading(work) {
			return panel;
		}

		let mut panel = panel.debug_selector(|| "saved-local-history".into());

		match self.history.as_ref().filter(|(id, _)| id == &work.id).map(|(_, history)| history) {
			Some(AgentHistoryResult::Available {
				entries, has_more, next_before, live, ..
			}) => {
				let older = self.timeline.older_history.get(&work.id);

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

				let live = self.streamed_output(work).unwrap_or(live.as_slice());

				for message in live.iter().filter(|message| {
					work.active_turn_id.as_deref().is_none_or(|turn| turn == message.turn_id)
				}) {
					panel = panel.child(
						gpui::div()
							.w_full()
							.py(gpui::px(2.))
							.children(
								(message.kind == AgentLiveMessageKind::ReasoningSummary)
									.then(|| muted("Reasoning summary")),
							)
							.child(StreamingText {
								text: message.text.clone(),
								key: format!(
									"live-{}-{}-{}",
									work.id, message.turn_id, message.item_id
								),
							})
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
			Some(AgentHistoryResult::Unavailable) =>
				panel = panel.child(muted("Messages could not be loaded. Retrying…")),
			None => panel = panel.child(ui_loading::conversation("Loading conversation")),
		}

		self.history_activity(panel, work)
	}

	fn history_activity(&self, mut panel: Div, work: &AgentWorkItemDto) -> Div {
		let active = matches!(
			work.dispatch_state,
			AgentDispatchStateDto::Running | AgentDispatchStateDto::Dispatching
		) || (self.native_agents.selected.is_none()
			&& self.selected.as_ref() == Some(&work.id)
			&& (self.sending || self.feedback == "Message saved · Waiting for agent…"));

		if self.native_agents.selected.is_none() && !self.native_history_active(work) {
			panel = panel.children(self.send_previews(&work.id));
		}

		panel = panel.child(Working {
			key: format!(
				"working-{}-{}",
				work.id,
				work.codex_thread_id.as_deref().unwrap_or_default()
			),
			turn: (active
				&& (self.native_agents.selected.is_some()
					|| self.composer_unavailable_reason().is_none()))
			.then(|| work.active_turn_id.clone().unwrap_or_else(|| work.id.clone())),
		});

		if active
			&& self.native_history_active(work)
			&& work.active_turn_id.is_some()
			&& self.timeline.native.safety_buffering_turn_id == work.active_turn_id
		{
			panel = panel.child(
				gpui::div()
					.id("native-safety-buffering")
					.role(Role::Status)
					.child(muted("Waiting for provider safety checks…")),
			);
		}

		panel.when(self.native_agents.selected.is_none(), |p| {
			p.children(self.live_chat_caption(&work.id))
		})
	}

	fn capacity_retry_control(
		&self,
		work_id: String,
		event_id: i64,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		self.workspace_action(
			format!("cancel-capacity-retry-{event_id}"),
			"Cancel automatic retry".into(),
			move |s, cx| {
				if let Ok(work_id) = EntityId::new(work_id.clone()) {
					s.execute(AgentActionDto::CancelCapacityRetry { work_id, event_id }, None, cx);
				}
			},
			cx,
		)
	}

	fn pending_panel(
		&self,
		snapshot: &AgentSnapshotDto,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let mut panel = gpui::div().flex().flex_col().gap_2();

		for event in snapshot
			.pending_events
			.iter()
			.filter(|event| event.work_item_id == work.id && event.event_kind.ends_with("_pending"))
		{
			let id = event.id;

			panel = panel.child(
				gpui::div()
					.id(SharedString::from(format!("review-request-{id}")))
					.role(Role::Button)
					.tab_index(0)
					.aria_label("Review request")
					.cursor_pointer()
					.text_color(gpui::rgb(BLUE))
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
		snapshot: &AgentSnapshotDto,
		id: &str,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let selected = id.to_owned();
		let keyboard_id = selected.clone();

		gpui::div()
			.id(SharedString::from(format!("agent-relation-{label}-{id}")))
			.role(Role::Button)
			.tab_index(28)
			.cursor_pointer()
			.text_color(gpui::rgb(BLUE))
			.on_click(cx.listener(move |surface, _, _, cx| {
				surface.open_page(&selected, cx);
				cx.notify();
			}))
			.on_key_down(cx.listener(move |surface, event: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					surface.open_page(&keyboard_id, cx);
					cx.notify();
				}
			}))
			.child(format!(
				"{label} → {}",
				snapshot
					.work_items
					.iter()
					.find(|w| w.id == id)
					.map(|w| self.work_label(w))
					.unwrap_or_else(|| id.into())
			))
	}

	fn cycle_sandbox(&mut self, cx: &mut Context<Self>) {
		self.sandbox = match self.sandbox {
			AgentSandboxDto::ReadOnly => AgentSandboxDto::WorkspaceWrite,
			AgentSandboxDto::WorkspaceWrite => AgentSandboxDto::FullAccess,
			AgentSandboxDto::FullAccess => AgentSandboxDto::ReadOnly,
		};

		cx.notify();
	}
}

impl AgentSurface {
	fn render_preferences(&self, cx: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.w_full()
			.flex()
			.flex_col()
			.gap(gpui::px(6.0))
			.px(gpui::px(0.0))
			.py(gpui::px(0.0))
			.child(
				gpui::div()
					.id("agent-advanced-preferences")
					.role(Role::Button)
					.aria_label("New agent defaults")
					.aria_expanded(self.workspace.setup_expanded)
					.tab_index(0)
					.h(gpui::px(32.0))
					.flex()
					.items_center()
					.cursor_pointer()
					.rounded(gpui::px(6.0))
					.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
					.on_click(cx.listener(|s, _, _, cx| {
						s.workspace.setup_expanded = !s.workspace.setup_expanded;

						cx.notify();
					}))
					.on_key_down(cx.listener(|s, event: &KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							s.workspace.setup_expanded = !s.workspace.setup_expanded;

							cx.notify();
						}
					}))
					.w_full()
					.justify_between()
					.child("New agent defaults")
					.child(workspace_symbols::icon(Symbol::ChevronDown))
					.smooth(),
			)
			.children(match self.current_model_catalog(cx) {
				Some(decodex_protocol::AgentCapabilitiesResult::Available {
					memory_enabled: Some(enabled),
					..
				}) => Some(
					gpui::div()
						.h(gpui::px(26.))
						.flex()
						.items_center()
						.justify_between()
						.child(muted("Codex memory"))
						.child(muted(if *enabled { "On" } else { "Off" })),
				),
				_ => None,
			})
			.child(ui_motion::disclosure(
				"agent-advanced-motion",
				self.workspace.setup_expanded,
				self.render_setup_controls(cx),
			))
			.when_some(self.selected.as_ref(), |panel, work| {
				panel
					.child(self.resources_panel(work, cx))
					.child(self.integrations_panel(work, cx))
					.child(self.voice_settings_panel(work, cx))
					.child(self.search_settings_panel(work, cx))
					.child(self.usage_estimate_panel(work, cx))
					.child(self.native_goal_panel(cx))
					.child(self.transcript_panel(cx))
					.children(
						self.snapshot
							.as_ref()
							.and_then(|s| s.work_items.iter().find(|w| &w.id == work))
							.map(|item| {
								gpui::div()
									.flex()
									.flex_col()
									.gap_2()
									.child(self.model_settings_panel(item, cx))
									.child(self.live_reviewer_panel(item, cx))
									.child(self.permission_profiles_panel(item, cx))
									.child(self.task_models_panel(item, cx))
									.child(self.hook_settings_panel(item, cx))
							}),
					)
			})
	}

	fn render_setup_controls(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let account = self
			.accounts
			.iter()
			.find(|(id, _)| id == self.account.read(cx).content())
			.map_or("Automatic routing", |(_, label)| label.as_str());

		gpui::div()
			.flex()
			.flex_col()
			.gap_2()
			.child(muted("Applies when starting a new agent."))
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap_2()
					.child(gpui::div().w(gpui::px(72.)).child(muted("Directory")))
					.child(gpui::div().flex_1().min_w_0().child(self.cwd.clone())),
			)
			.child(self.workspace_action(
				"agent-default-account".into(),
				format!("Account · {account}"),
				|s, cx| s.cycle_account(cx),
				cx,
			))
			.child(self.workspace_action(
				"agent-default-access".into(),
				format!("Access · {:?}", self.sandbox),
				|s, cx| s.cycle_sandbox(cx),
				cx,
			))
	}
}

impl Render for AgentSurface {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.observe_recap_focus(window, cx);

		self.render_workspace(window, cx)
	}
}

struct WorkspaceView {
	new_conversation: Option<String>,
	browsing: bool,
	workspace_filter: Option<String>,
	new_conversation_workspace: Option<String>,
	folder_error: Option<String>,
	workspace_picker: Option<bool>,
	workspace_search: Option<Entity<ComposerInput>>,
	opening_work: Option<String>,
	composer_overlay_height: f32,
	pages: Vec<String>,
	preview_page: Option<String>,
	closing_pages: HashSet<String>,
	graph_visible: bool,
	graph_expanded: bool,
	page_views: std::collections::BTreeMap<String, PageView>,
	graph_scope: Option<String>,
	graph_selected: Option<String>,
	graph_zoom: f32,
	graph_display_zoom: f32,
	graph_pan: (f32, f32),
	graph_inset: (f32, f32),
	graph_drag: Option<Point<Pixels>>,
	agent_tree_visible: bool,
	agent_tree_collapsed: std::collections::BTreeSet<String>,
	sidebar_visible: bool,
	sidebar_peek: bool,
	sidebar_leave: Option<Task<()>>,
	sidebar_width: f32,
	sidebar_motion: std::cell::RefCell<Option<crate::ui_motion::Tween>>,
	agent_panel_width: f32,
	graph_panel_height: f32,
	focused_panel: Option<workspace_size::Panel>,
	sidebar_drag: Option<(f32, f32)>,
	connection_details_expanded: bool,
	details_visible: bool,
	setup_expanded: bool,
}
impl Default for WorkspaceView {
	fn default() -> Self {
		Self {
			new_conversation: None,
			browsing: false,
			workspace_filter: None,
			new_conversation_workspace: None,
			folder_error: None,
			workspace_picker: None,
			workspace_search: None,
			opening_work: None,
			composer_overlay_height: 0.,
			pages: vec![],
			preview_page: None,
			closing_pages: Default::default(),
			graph_visible: true,
			graph_expanded: false,
			page_views: Default::default(),
			graph_scope: None,
			graph_selected: None,
			graph_zoom: 0.85,
			graph_display_zoom: 0.85,
			graph_pan: (0.0, 0.0),
			graph_inset: (0.0, 0.0),
			graph_drag: None,
			agent_tree_visible: true,
			agent_tree_collapsed: Default::default(),
			sidebar_visible: true,
			sidebar_peek: false,
			sidebar_leave: None,
			sidebar_width: PanelDefaults::configured().sidebar.into(),
			sidebar_motion: Default::default(),
			agent_panel_width: PanelDefaults::configured().sidebar.into(),
			graph_panel_height: PanelDefaults::configured().dock.into(),
			focused_panel: None,
			sidebar_drag: None,
			connection_details_expanded: false,
			details_visible: false,
			setup_expanded: false,
		}
	}
}

#[derive(Default)]
struct TimelineView {
	native: Timeline,
	marks: std::collections::BTreeMap<HistoryKey, HistoryMark>,
	marks_work: Option<(String, bool)>,
	marks_revision: Option<(u64, u64)>,
	selected: Option<HistoryKey>,
	latest_follow_work: Option<String>,
	hover: Option<usize>,
	navigation: Option<HistoryNavigation>,
	wheel_scroll: Option<WheelScroll>,
	read_at: Option<std::time::Instant>,
	cache: std::collections::BTreeMap<String, AgentHistoryResult>,
	scroll: std::collections::BTreeMap<String, ScrollHandle>,
	follow_paused: std::collections::BTreeSet<String>,
	expanded_progress: std::collections::BTreeSet<String>,
	expanded_records: std::collections::BTreeSet<String>,
	transcript_busy: bool,
	transcript_failed: bool,
	older_history: std::collections::BTreeMap<String, (Vec<AgentHistoryEntryDto>, Option<i64>)>,
	loading_older: bool,
	older_retry_after: Option<std::time::Instant>,
	older_scroll_anchor: Option<activity::HistoryScrollAnchor>,
}
impl TimelineView {
	fn scroll_for(&self, selected: Option<&str>) -> ScrollHandle {
		self.scroll.get(selected.unwrap_or_default()).cloned().unwrap_or_default()
	}
}

struct AgentInputs {
	model: Entity<ComposerInput>,
	cwd: Entity<ComposerInput>,
	composer: Entity<ComposerInput>,
}

#[derive(Clone)]
struct QueuedCommand {
	profile: ClientProfile,
	action: AgentActionDto,
	key: IdempotencyKey,
	pending: PendingCommand,
}

#[derive(Clone)]
struct PendingCommand {
	recovery: Option<DesktopRecoveredDraft>,
	key: Option<IdempotencyKey>,
	steer: Option<decodex_protocol::AgentSteerIdentity>,
	epoch: u64,
	execution_intent: Option<(String, u64)>,
	draft: Option<String>,
	owner: Option<String>,
	attachments: Option<Vec<AgentAttachmentDto>>,
	references: Option<Vec<AgentTaskReferenceDto>>,
}

#[derive(Clone)]
struct RequestReadSource {
	profile_epoch: u64,
	runtime_source: Option<EntityId>,
	event: AgentPendingEventDto,
}

pub(crate) fn compact_tokens(value: u64) -> String {
	let (divisor, suffix) = if value >= 999_950_000 {
		(1_000_000_000.0, "B")
	} else if value >= 999_950 {
		(1_000_000.0, "M")
	} else if value >= 1_000 {
		(1_000.0, "K")
	} else {
		return value.to_string();
	};
	let text = format!("{:.1}", value as f64 / divisor);

	format!("{}{suffix}", text.trim_end_matches(".0"))
}

fn should_poll_snapshot(has_profile: bool, state: &LoadState) -> bool {
	has_profile && *state != LoadState::Loading
}

// This startup advisory describes the Codex environment, not a failed turn.
// Keep the original stored record, but present it once in the notification center.
fn startup_feature_warning(entry: &AgentHistoryEntryDto) -> bool {
	entry.kind == "execution_notice"
		&& entry.text.starts_with("Codex warning: Under-development features enabled:")
}

fn unique_command() -> String {
	static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

	let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();

	format!(
		"gpui-agent-{}-{time}-{}",
		process::id(),
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
	let now = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|time| time.as_micros() as i64)
		.unwrap_or(0);
	let seconds = due.saturating_sub(now) / 1_000_000;

	if seconds <= 0 {
		"Due now".into()
	} else if seconds < 60 {
		format!("In {seconds} seconds")
	} else if seconds < 3_600 {
		format!("In {} minutes", seconds / 60)
	} else if seconds < 86_400 {
		format!("In {} hours", seconds / 3_600)
	} else {
		format!("In {} days", seconds / 86_400)
	}
}

fn muted(text: impl Into<SharedString>) -> impl IntoElement {
	gpui::div()
		.text_size(gpui::px(CAPTION_SIZE))
		.text_color(gpui::rgb(TEXT_MUTED))
		.child(text.into())
}

fn auth_recovery_entry(entry: &AgentHistoryEntryDto) -> Div {
	let id = entry.id;

	gpui::div()
		.w_full()
		.flex()
		.flex_col()
		.gap_1()
		.debug_selector(move || format!("auth-recovery-receipt-{id}"))
		.child(muted("Provider sign-in · Recorded event"))
		.child(entry.text.clone())
}

fn history_entry(entry: &AgentHistoryEntryDto) -> Div {
	if entry.kind == "auth_recovery" {
		return auth_recovery_entry(entry);
	}

	history_entry_with_key(entry, &entry.id.to_string())
}

fn history_entry_with_key(entry: &AgentHistoryEntryDto, identity: &str) -> Div {
	history_entry_with_metrics(entry, identity, None)
}

fn history_entry_with_metrics(
	entry: &AgentHistoryEntryDto,
	identity: &str,
	metrics: Option<AnyElement>,
) -> Div {
	history_entry_presented(entry, identity, metrics, None)
}

fn history_entry_presented(
	entry: &AgentHistoryEntryDto,
	identity: &str,
	metrics: Option<AnyElement>,
	streamed_body: Option<AnyElement>,
) -> Div {
	if entry.kind == "checklist" {
		let id = entry.id;

		return gpui::div()
			.w_full()
			.py_2()
			.debug_selector(move || format!("checklist-receipt-{id}"))
			.child(muted("Recorded checklist"))
			.child(markdown::render(&entry.text, &format!("checklist-{id}")));
	}
	if matches!(entry.kind.as_str(), "partial_plan" | "partial_answer") {
		return gpui::div()
			.w_full()
			.py_2()
			.child(muted(if entry.kind == "partial_plan" {
				"Proposed plan · Unfinished"
			} else {
				"Assistant · Unfinished"
			}))
			.child(markdown::render(&entry.text, &format!("partial-{}", entry.id)))
			.child(markdown::response_copy_button(
				&format!("copy-partial-{}", entry.id),
				"Copy unfinished output",
				entry.text.clone(),
			));
	}

	if entry.kind == "user"
		&& let Some(text) = voice::history::handoff(&entry.text)
	{
		return gpui::div().w_full().child(voice::history::VoiceBlock {
			key: identity.into(),
			title: "Voice conversation".into(),
			expanded: false,
			text,
		});
	}
	let user = matches!(entry.kind.as_str(), "user" | "instruction");
	let visible_text = if entry.kind == "assistant" {
		markdown::response_text(&entry.text)
	} else {
		entry.text.clone()
	};

	if matches!(entry.kind.as_str(), "execution_notice" | "capacity_retry_pending" | "stopped") {
		return gpui::div()
			.w_full()
			.py_2()
			.text_size(gpui::px(11.))
			.text_color(gpui::rgb(if entry.kind == "stopped" { TEXT_MUTED } else { AMBER }))
			.child(SelectableText {
				key: format!("notice-{identity}"),
				text: entry.text.clone(),
				highlights: vec![],
				links: vec![],
			});
	}

	gpui::div()
		.w_full()
		.min_w_0()
		.flex_none()
		.flex()
		.when(user, |row| row.child(gpui::div().flex_1().min_w_0()))
		.child(
			gpui::div()
				.min_w_0()
				.when(user, |bubble| {
					bubble
						.flex_none()
						.max_w(gpui::relative(0.78))
						.px_4()
						.py(gpui::px(9.))
						.rounded(gpui::px(18.0))
						.bg(gpui::rgba(0xffffff0e))
				})
				.when(!user, |body| body.w_full().py(gpui::px(2.)))
				.when(entry.kind == "instruction", |body| body.child(muted("Task input")))
				.child(streamed_body.unwrap_or_else(|| {
					markdown::render(&visible_text, &format!("message-{identity}"))
				}))
				.children(
					entry
						.weather
						.iter()
						.enumerate()
						.map(|(i, forecast)| weather::render(forecast, &format!("{identity}-{i}"))),
				)
				.when(!user, |body| {
					body.child(
						gpui::div()
							.mt(gpui::px(METADATA_GAP))
							.flex()
							.items_center()
							.gap(gpui::px(2.))
							.child(
								metrics.unwrap_or_else(|| reply_metrics(entry).into_any_element()),
							)
							.when(entry.kind == "assistant", |row| {
								row.child(markdown::response_copy_button(
									&format!("copy-response-{identity}"),
									"Copy response",
									if entry.weather.is_empty() {
										visible_text.clone()
									} else {
										format!(
											"{}\n\n{}",
											visible_text,
											entry
												.weather
												.iter()
												.map(|f| f.markdown())
												.collect::<Vec<_>>()
												.join("\n\n")
										)
									},
								))
							}),
					)
				}),
		)
}

fn reply_metrics(entry: &AgentHistoryEntryDto) -> impl IntoElement {
	ResponseMetrics {
		key: format!("local-{}", entry.id),
		duration_ms: entry.duration_ms,
		status: None,
		usage: entry.usage.clone(),
	}
}

fn resource_field(
	placeholder: &'static str,
	label: &'static str,
	cx: &mut Context<AgentSurface>,
) -> Entity<ComposerInput> {
	cx.new(|cx| ComposerInput::with_placeholder(40, placeholder, label, cx))
}

#[cfg(test)]
#[path = "agent_request_source_tests.rs"]
mod request_source_tests;
#[cfg(test)]
#[path = "agent_wire_test_support.rs"]
mod wire_test_support;
#[cfg(test)]
mod tests {
	use std::{
		fs,
		os::unix::fs::{MetadataExt as _, PermissionsExt as _},
	};

	use gpui::{AppContext as _, Focusable};

	use crate::shell::agent_surface::{
		self, AgentActionDto, AgentCommandResponse, AgentDispatchStateDto, AgentHistoryResult,
		AgentRequestResult, AgentSnapshotDto, AgentSnapshotResult, AgentSurface, AgentWorkItemDto,
		AgentWorkStatusDto, ClientProfile, ConversationWorkingDirectory, EntityId, HistoryText,
		LoadState, PendingCommand, WireText,
	};
	use decodex_protocol::AgentWorkKindDto;

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

			super::history_entry(&decodex_protocol::AgentHistoryEntryDto {
				native_source: None,
				turn_id: None,
				weather: Vec::new(),
				receipt: None,
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
	fn startup_warnings_are_deduplicated_across_loaded_conversations(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let (_, history) = s.history.as_mut().unwrap();
			let AgentHistoryResult::Available { entries, .. } = history else {
				panic!("fixture");
			};
			let mut notice = entries[0].clone();

			notice.kind = "execution_notice".into();
			notice.text = "Codex warning: Under-development features enabled: chronicle.".into();

			assert!(agent_surface::startup_feature_warning(&notice));

			entries.extend([notice.clone(), notice]);
			s.timeline.cache.insert("other-agent".into(), history.clone());

			assert_eq!(
				s.operation_notices()
					.iter()
					.filter(|(title, _)| *title == "Experimental Codex features")
					.count(),
				1
			);

			s.history = None;

			assert_eq!(
				s.operation_notices()
					.iter()
					.filter(|(title, _)| *title == "Experimental Codex features")
					.count(),
				1
			);
		});
	}

	#[gpui::test]
	fn user_bubbles_stay_right_aligned_at_multiple_widths(cx: &mut gpui::TestAppContext) {
		for width in [640.0, 1_248.0] {
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
		assert_eq!(super::compact_tokens(1_000), "1K");
		assert_eq!(super::compact_tokens(24_860), "24.9K");
		assert_eq!(super::compact_tokens(999_950), "1M");
		assert_eq!(super::compact_tokens(1_280_000), "1.3M");
		assert_eq!(super::compact_tokens(999_950_000), "1B");
		assert_eq!(super::compact_tokens(2_450_000_000), "2.5B");
	}

	#[gpui::test]
	fn context_is_hidden_without_reported_usage(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(super::AgentSurface::new);

		surface.update(cx, |s, cx| {
			assert!(s.usage_line(cx).is_none());

			s.visual_workspace_fixture(cx);

			assert!(s.usage_line(cx).is_none());

			s.visual_workspace_page("markdown", cx);

			assert!(s.usage_line(cx).is_some());
		});
	}

	#[gpui::test]
	fn context_detail_uses_space_above_the_composer(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| super::AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_000.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_workspace_page("markdown", cx);

			s.context_tip_visible = true;
			s.composer_menu = None;

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let detail =
			visual.debug_bounds("composer-context-detail").expect("context shown by parent");

		assert!(detail.top() >= gpui::px(0.));

		surface.update(visual, |s, cx| {
			s.context_tip_visible = false;

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("composer-context-detail").is_none());
	}

	#[test]
	fn snapshot_poll_requires_a_profile_and_no_in_flight_read() {
		assert!(agent_surface::should_poll_snapshot(true, &LoadState::Unavailable));
		assert!(agent_surface::should_poll_snapshot(true, &LoadState::Stale));
		assert!(agent_surface::should_poll_snapshot(true, &LoadState::Idle));
		assert!(!agent_surface::should_poll_snapshot(false, &LoadState::Unavailable));
		assert!(!agent_surface::should_poll_snapshot(true, &LoadState::Loading));
		assert!(agent_surface::should_poll_snapshot(true, &LoadState::Ready));
	}

	#[gpui::test]
	fn command_enter_submits_agent_composer_and_keeps_unaccepted_draft(
		cx: &mut gpui::TestAppContext,
	) {
		cx.update(crate::composer_input::bind_keys);

		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		let input = surface.update(visual, |surface, cx| {
			surface.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
				connection_initializing: false,
				runtime_source: None,
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
			assert_eq!(
				surface.feedback,
				"Refresh account defaults for this directory before sending."
			);
			assert_eq!(surface.composer.read(cx).content(), "Please coordinate this goal");
			assert!(!surface.sending);

			surface.mark_model_intent(cx);
			surface.mark_effort_intent(cx);
			surface.mark_tier_intent();
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
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

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
			assert!(!surface.workspace.details_visible);
		});
	}

	#[test]
	fn approval_buttons_use_only_the_exact_command_request_choices() {
		assert_eq!(
			agent_surface::offered_decisions(
				"item/commandExecution/requestApproval",
				r#"{"availableDecisions":["decline","accept","acceptForSession",{"acceptWithExecpolicyAmendment":{}}]}"#
			),
			vec!["decline", "accept", "acceptForSession"]
		);
		assert_eq!(
			agent_surface::offered_decisions("item/commandExecution/requestApproval", "{}"),
			vec!["accept", "decline"]
		);
		assert!(
			agent_surface::offered_decisions(
				"item/permissions/requestApproval",
				r#"{"availableDecisions":["accept"]}"#
			)
			.is_empty()
		);
	}

	#[gpui::test]
	fn history_prefetch_requires_reading_near_top_and_a_remaining_cursor(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(320.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			if let Some((_, AgentHistoryResult::Available { next_before, .. })) = &mut s.history {
				*next_before = Some(1);
			}
		});

		visual.update(|window, cx| window.draw(cx).clear());

		surface.update(visual, |s, _| {
			s.timeline.scroll["agent"].set_offset(gpui::point(gpui::px(0.), gpui::px(-20.)));

			assert!(
				!s.history_prefetch_needed(),
				"startup and bottom-follow must not fetch all history"
			);

			s.timeline.follow_paused.insert("agent".into());

			assert!(s.history_prefetch_needed(), "prefetch before reaching the edge");

			s.timeline.loading_older = true;

			assert!(!s.history_prefetch_needed(), "only one request may be in flight");

			s.timeline.loading_older = false;

			s.timeline.older_history.insert("agent".into(), (vec![], None));

			assert!(!s.history_prefetch_needed(), "stop when history is exhausted");
		});
	}

	#[gpui::test]
	fn completed_dispatch_clears_waiting_feedback_without_observing_running(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			let mut snapshot = s.snapshot.clone().unwrap();

			snapshot.pending_events.clear();

			for work in &mut snapshot.work_items {
				work.dispatch_state = AgentDispatchStateDto::Idle;
				work.active_turn_id = None;
			}

			s.feedback = "Message saved · Waiting for agent…".into();

			s.apply_result(Ok(AgentSnapshotResult::Available(snapshot)));

			assert!(
				s.feedback.is_empty(),
				"a completed or failed fast turn must not leave a phantom queue"
			);
		});
	}

	#[gpui::test]
	fn pending_capacity_retry_has_an_actionable_cancel_button(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |surface, _| {
			surface.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
				connection_initializing: false,
				runtime_source: None,
				workspaces: vec![],
				work_items: vec![AgentWorkItemDto {
					id: "root".into(),
					parent_goal_id: None,
					kind: AgentWorkKindDto::Goal,
					title: "Agent".into(),
					codex_thread_id: Some("thread".into()),
					active_turn_id: None,
					dispatch_state: AgentDispatchStateDto::Idle,
					status: AgentWorkStatusDto::Open,
					next_check_at_micros: None,
					created_at_micros: 1,
					updated_at_micros: 1,
				}],
				dependencies: vec![],
				pending_events: vec![],
			})));

			surface.history = Some((
				"root".into(),
				AgentHistoryResult::Available {
					questions: vec![],
					questions_truncated: false,
					questions_recovering: false,
					misalignment: None,
					live: vec![],
					next_before: None,
					usage: None,
					entries: vec![decodex_protocol::AgentHistoryEntryDto {
						native_source: None,
						turn_id: None,
						weather: Vec::new(),
						receipt: None,
						activity: None,
						duration_ms: None,
						usage: None,
						id: 7,
						kind: "capacity_retry_pending".into(),
						text: "Model capacity retry 1/3 is pending.".into(),
						created_at_micros: 1,
					}],
					has_more: false,
				},
			));
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(1_200.0)));
			window.draw(cx).clear();
		});

		let bounds =
			visual.debug_bounds("capacity-retry-cancel").expect("visible cancellation control");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |surface, _| {
			assert_eq!(surface.feedback, "No service profile is configured.");
			assert!(!surface.sending);
		});
	}

	#[gpui::test]
	fn selected_work_history_and_pending_request_render_together(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |surface, _| {
			surface.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
				connection_initializing: false,
				runtime_source: None,
				workspaces: vec![],
				work_items: vec![AgentWorkItemDto {
					id: "root".into(),
					parent_goal_id: None,
					kind: AgentWorkKindDto::Goal,
					title: "Agent".into(),
					codex_thread_id: Some("thread-real".into()),
					active_turn_id: Some("turn-real".into()),
					dispatch_state: AgentDispatchStateDto::Running,
					status: AgentWorkStatusDto::UserDecision,
					next_check_at_micros: None,
					created_at_micros: 1,
					updated_at_micros: 1,
				}],
				dependencies: vec![],
				pending_events: vec![decodex_protocol::AgentPendingEventDto {
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
				AgentHistoryResult::Available {
					questions: vec![],
					questions_truncated: false,
					questions_recovering: false,
					misalignment: None,
					usage: None,
					entries: vec![decodex_protocol::AgentHistoryEntryDto {
						native_source: None,
						turn_id: None,
						weather: Vec::new(),
						receipt: None,
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
			surface.request = Some(AgentRequestResult::Available {
				work_id: "root".into(),
				event_id: 1,
				method: "item/commandExecution/requestApproval".into(),
				request_json: decodex_protocol::AgentRequestText::new(
					r#"{"command":"pwd","availableDecisions":["accept","decline"]}"#,
				)
				.unwrap(),
			});
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(720.0)));
			window.draw(cx).clear();
		});
	}

	#[gpui::test]
	fn offline_commands_preserve_editable_draft_and_attachments(cx: &mut gpui::TestAppContext) {
		let root = tempfile::tempdir_in("/tmp").unwrap();
		let path = root.path().canonicalize().unwrap();

		fs::create_dir(path.join("server")).unwrap();
		fs::set_permissions(path.join("server"), std::fs::Permissions::from_mode(0o700)).unwrap();

		let uid = fs::metadata(&path).unwrap().uid();
		let config = path.join("config.toml");

		fs::write(&config, format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"018f0f9e-7b6e-4a31-8f4c-1d2e3f405162\"\n")).unwrap();
		fs::set_permissions(config, std::fs::Permissions::from_mode(0o600)).unwrap();

		let profile = ClientProfile::load(&path, None).unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.profile = Some(profile);

			s.composer.update(cx, |input, cx| input.set_content("draft", cx));
			s.attachments.push(decodex_protocol::AgentAttachmentDto {
				path: ConversationWorkingDirectory::new("/tmp/draft.png").unwrap(),
				image: true,
				skill_name: None,
			});
			s.task_references.push(decodex_protocol::AgentTaskReferenceDto {
				work_id: EntityId::new("evidence").unwrap(),
				thread_id: WireText::new("thread").unwrap(),
				title: WireText::new("Evidence").unwrap(),
			});

			for state in [
				LoadState::Stale,
				LoadState::Unavailable,
				LoadState::Idle,
				LoadState::Capacity { work: 1, edges: 0, events: 0 },
			] {
				s.state = state;

				s.execute(
					AgentActionDto::Send {
						root_id: EntityId::new("root").unwrap(),
						text: HistoryText::new("draft").unwrap(),
					},
					Some("draft".into()),
					cx,
				);

				assert!(s.submission.command.is_none());
				assert!(!s.sending);
				assert!(s.feedback.contains("Connection unavailable"));
				assert_eq!(s.attachments.len(), 1);
				assert_eq!(s.task_references.len(), 1);
			}

			s.composer.update(cx, |input, cx| input.set_content("edited offline", cx));

			assert_eq!(s.composer.read(cx).content(), "edited offline");

			s.state = LoadState::Loading;
			s.status_before_refresh = Some(LoadState::Stale);

			assert!(!s.command_connection_ready());

			s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
				connection_initializing: false,
				runtime_source: None,
				workspaces: vec![],
				work_items: vec![],
				dependencies: vec![],
				pending_events: vec![],
			})));

			assert!(s.command_connection_ready());
			assert!(s.submission.command.is_none(), "fresh readback must not replay the draft");

			s.state = LoadState::Loading;
			s.status_before_refresh = None;

			assert!(s.command_connection_ready(), "normal polling must not disable sending");
		});
	}

	#[gpui::test]
	fn old_service_acceptance_cannot_clear_identical_new_draft(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			let file = decodex_protocol::AgentAttachmentDto {
				path: ConversationWorkingDirectory::new("/tmp/draft.png").unwrap(),
				image: true,
				skill_name: None,
			};
			let pending = PendingCommand {
				recovery: None,
				key: None,
				steer: None,
				execution_intent: None,
				epoch: s.command_epoch,
				draft: Some("same draft".into()),
				owner: Some("root".into()),
				attachments: Some(vec![file.clone()]),
				references: None,
			};

			s.sending = true;

			s.composer.update(cx, |input, cx| input.set_content("same draft", cx));
			s.bind_profile(None, cx);

			assert!(s.uncertain);
			assert!(!s.sending);
			assert_eq!(s.composer.read(cx).content(), "same draft");

			s.composer_manager = Some("root".into());
			s.attachments = vec![file];
			s.sending = true;

			let feedback = s.feedback.clone();

			s.finish_command(
				pending,
				Ok(AgentCommandResponse::Accepted { work_id: EntityId::new("root").unwrap() }),
				cx,
			);

			assert!(s.sending, "old completion must not mutate current command state");
			assert!(s.uncertain);
			assert_eq!(s.feedback, feedback);
			assert_eq!(s.composer.read(cx).content(), "same draft");
			assert_eq!(s.attachments.len(), 1);
		});
	}

	#[gpui::test]
	fn durable_acceptance_clears_only_the_submitted_draft(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |surface, cx| {
			surface.composer.update(cx, |input, cx| input.set_content("edited while sending", cx));
			surface.apply_command_result(
				Ok(AgentCommandResponse::Accepted { work_id: EntityId::new("root").unwrap() }),
				Some("original"),
				cx,
			);

			assert_eq!(surface.composer.read(cx).content(), "edited while sending");

			surface.apply_command_result(
				Ok(AgentCommandResponse::Accepted { work_id: EntityId::new("root").unwrap() }),
				Some("edited while sending"),
				cx,
			);

			assert!(surface.composer.read(cx).content().is_empty());
		});
	}

	#[gpui::test]
	fn unknown_acceptance_preserves_draft_and_blocks_another_send(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |surface, cx| {
			surface.composer.update(cx, |input, cx| input.set_content("do work", cx));
			surface.apply_command_result(
				Ok(AgentCommandResponse::PotentiallyDispatched {
					failure: decodex_protocol::ClientFailure::ProtocolTimeout,
				}),
				Some("do work"),
				cx,
			);

			assert!(surface.uncertain);
			assert_eq!(surface.composer.read(cx).content(), "do work");

			surface.submit(cx);

			assert!(surface.submission.command.is_none());
			assert!(surface.feedback.contains("Acceptance unknown"));
		});
	}

	#[gpui::test]
	fn failed_refresh_retains_stale_snapshot_but_capacity_never_shows_partial_graph(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |surface, _| {
			surface.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
				connection_initializing: false,
				runtime_source: None,
				workspaces: vec![],
				work_items: vec![],
				dependencies: vec![],
				pending_events: vec![],
			})));

			assert_eq!(surface.state, LoadState::Ready);

			surface.apply_result(Err(()));

			assert_eq!(surface.state, LoadState::Ready);
			assert!(surface.status_notice().is_none(), "one read failure is not a disconnect");

			surface.apply_result(Err(()));

			assert_eq!(surface.state, LoadState::Ready);

			surface.apply_result(Err(()));

			assert_eq!(surface.state, LoadState::Stale);
			assert_eq!(surface.status_notice().unwrap().0, "Updates paused");

			surface.status_before_refresh = Some(LoadState::Stale);
			surface.state = LoadState::Loading;

			let notice = surface.status_notice().unwrap();

			assert_eq!(notice.0, "Updates paused");
			assert!(!notice.2, "a running refresh must not offer another retry");

			surface.apply_result(Ok(AgentSnapshotResult::Available(
				surface.snapshot.clone().unwrap(),
			)));

			assert!(surface.status_notice().is_none(), "recovery clears the status");
			assert!(surface.snapshot.is_some());

			surface.apply_result(Ok(AgentSnapshotResult::CapacityExceeded {
				work_items: 101,
				dependencies: 0,
				pending_events: 0,
			}));

			assert!(surface.snapshot.is_none());
			assert!(surface.status_text().contains("No partial graph"));
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(720.0)));
			window.draw(cx).clear();
		});
	}

	#[gpui::test]
	fn saved_service_events_do_not_create_current_alerts_or_cross_conversations(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, _| {
			s.selected = Some("agent".into());
			s.history = Some((
				"agent".into(),
				AgentHistoryResult::Available {
					questions: vec![],
					questions_truncated: false,
					questions_recovering: false,
					misalignment: None,
					usage: None,
					entries: vec![decodex_protocol::AgentHistoryEntryDto {
						native_source: None,
						turn_id: None,
						weather: Vec::new(),
						receipt: None,
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
			s.snapshot = Some(AgentSnapshotDto {
				connection_initializing: false,
				runtime_source: None,
				workspaces: vec![],
				work_items: vec![],
				dependencies: vec![],
				pending_events: vec![],
			});

			assert!(s.status_notice().is_none());

			s.snapshot.as_mut().unwrap().pending_events.push(
				decodex_protocol::AgentPendingEventDto {
					id: 2,
					source_event_id: "failure".into(),
					work_item_id: "agent".into(),
					event_kind: "recovery_needs_attention".into(),
					created_at_micros: 2,
					delivery_claimed: false,
				},
			);

			assert_eq!(s.status_notice().unwrap().0, "Work needs attention");

			s.snapshot.as_mut().unwrap().pending_events[0].event_kind =
				"thread_in_use_needs_attention".into();

			assert_eq!(s.status_notice().unwrap().0, "In use by another app");
			assert!(s.thread_in_use("agent"));
			assert!(!s.thread_in_use("another-agent"));

			s.snapshot.as_mut().unwrap().pending_events.clear();

			assert!(!s.thread_in_use("agent"));
			assert!(s.status_notice().is_none());

			s.selected = Some("another-agent".into());
		});
	}

	#[gpui::test]
	fn unconfigured_refresh_has_explicit_unavailable_state(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |surface, cx| {
			surface.refresh(cx);

			assert_eq!(surface.state, LoadState::Unavailable);
			assert!(surface.snapshot.is_none());
			assert!(surface.task.is_none());
		});
	}
}
