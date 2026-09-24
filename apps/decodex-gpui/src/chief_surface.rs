//! Agent conversation and work overview. The service owns records and execution.

#[path = "chief_activity.rs"] mod activity;
#[path = "chief_tree.rs"] mod agent_tree;
#[path = "chief_archive.rs"] mod archive;
#[path = "chief_async_questions.rs"] mod async_questions;
#[path = "chief_capabilities.rs"] mod capabilities;
#[path = "chief_composer.rs"] mod composer;
#[path = "chief_detail.rs"] mod detail;
#[path = "chief_dictation.rs"] mod dictation;
#[path = "chief_drafts.rs"] mod drafts;
#[path = "chief_execution_intent.rs"] mod execution_intent;
#[path = "chief_graph.rs"] mod graph;
#[path = "chief_guardian.rs"] mod guardian;
#[path = "chief_inspection.rs"] mod inspection;
#[path = "chief_install.rs"] mod install;
#[path = "chief_integrations.rs"] mod integrations;
#[path = "chief_live_settings.rs"] mod live_settings;
#[path = "chief_markdown.rs"] mod markdown;
#[path = "chief_mcp_forms.rs"] mod mcp_forms;
#[path = "chief_misalignment.rs"] mod misalignment;
#[path = "chief_model_settings.rs"] mod model_settings;
#[path = "chief_native_agents.rs"] mod native_agents;
#[path = "chief_timeline.rs"] mod native_timeline;
#[path = "chief_output_stream.rs"] mod output_stream;
#[path = "chief_progress.rs"] mod progress;
#[path = "chief_prompts.rs"] mod prompts;
#[path = "chief_question_notices.rs"] mod question_notices;
#[path = "chief_requests.rs"] mod requests;
#[path = "chief_resources.rs"] mod resources;
#[path = "chief_selectable_text.rs"] mod selectable_text;
#[path = "chief_steer_receipts.rs"] mod steer_receipts;
#[path = "chief_text_reveal.rs"] mod text_reveal;
#[path = "chief_usage_estimates.rs"] mod usage_estimates;
#[path = "chief_voice.rs"] mod voice;
#[path = "chief_weather.rs"] mod weather;
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
	AnimationExt, ClipboardItem, Context, Entity, FocusHandle, FontWeight, Render, Role,
	SharedString, Task, Window, div, prelude::*, px, rgb, rgba,
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

#[cfg(all(target_os = "macos", not(test)))]
#[path = "chief_native_composer.rs"]
mod native_composer;

pub(crate) struct ChiefSurface {
	#[cfg(all(target_os = "macos", not(test)))]
	native_composer: native_composer::NativeComposer,
	voice: Option<voice::VoiceUi>,
	voice_task: Option<Task<()>>,
	audio_inputs: Vec<String>,
	audio_input: String,
	dictation: Option<dictation::DictationUi>,
	dictation_task: Option<Task<()>>,
	activity_detail: detail::ActivityDetailState,
	resources: Option<(String, Option<decodex_protocol::ChiefResourcesResult>)>,
	resources_task: Option<Task<()>>,
	usage_estimate: Option<(String, Option<decodex_protocol::ChiefUsageEstimateResult>)>,
	usage_estimate_task: Option<Task<()>>,
	native_history: native_timeline::Timeline,
	integrations: Option<(String, Option<decodex_protocol::ChiefIntegrationsResult>)>,
	integrations_task: Option<Task<()>>,
	integration_refresh_task: Option<Task<()>>,
	integration_feedback: String,
	mcp_login: Option<(String, String, decodex_protocol::McpLoginStatus)>,
	mcp_login_task: Option<Task<()>>,
	resource_mutation_task: Option<Task<()>>,
	resource_feedback: String,
	resource_title: Entity<ComposerInput>,
	resource_url: Entity<ComposerInput>,
	capabilities: Option<decodex_protocol::ChiefCapabilitiesResult>,
	capabilities_context: Option<capabilities::CatalogContext>,
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
	history_marks: std::collections::BTreeMap<activity::HistoryKey, activity::HistoryMark>,
	history_marks_work: Option<(String, bool)>,
	history_selected: Option<activity::HistoryKey>,
	latest_follow_work: Option<String>,
	connection_details_expanded: bool,
	history_hover: Option<usize>,
	history_navigation: Option<activity::HistoryNavigation>,
	native_agents: native_agents::NativeAgents,
	output_stream: output_stream::OutputStream,
	history_read_at: Option<std::time::Instant>,
	agent_tree_visible: bool,
	agent_tree_collapsed: std::collections::BTreeSet<String>,
	sidebar_visible: bool,
	sidebar_width: f32,
	agent_panel_width: f32,
	graph_panel_height: f32,
	focused_panel: Option<workspace_size::Panel>,
	sidebar_drag: Option<(f32, f32)>,
	history_cache: std::collections::BTreeMap<String, ChiefHistoryResult>,
	transcript_scroll: std::collections::BTreeMap<String, gpui::ScrollHandle>,
	history_follow_paused: std::collections::BTreeSet<String>,
	profile: Option<ClientProfile>,
	snapshot: Option<ChiefSnapshotDto>,
	state: LoadState,
	status_before_refresh: Option<LoadState>,
	selected: Option<String>,
	task: Option<Task<()>>,

	generation: u64,
	refresh_failures: u8,
	composer: Entity<ComposerInput>,
	composer_footer_height: f32,
	fast: bool,
	service_tier: Option<decodex_protocol::ServiceTier>,
	steer: bool,
	media_spare: Option<voice::Media>,
	media_warm_attempted: bool,
	effort_focus: gpui::FocusHandle,
	effort_drag: Option<(f32, f32)>,
	effort_pointer: Option<f32>,
	effort_track_bounds: Option<gpui::Bounds<gpui::Pixels>>,
	menu_trigger_bounds: std::collections::BTreeMap<&'static str, gpui::Bounds<gpui::Pixels>>,
	composer_menu: Option<&'static str>,
	escape_stop: Option<(String, String, std::time::Instant)>,
	interrupting: Option<(String, String)>,
	interrupt_task: Option<Task<()>>,
	composer_menu_content: Option<&'static str>,
	attachments: Vec<decodex_protocol::ChiefAttachmentDto>,
	task_references: Vec<decodex_protocol::ChiefTaskReferenceDto>,
	task_reference_search: Entity<ComposerInput>,
	draft_profiles: drafts::Profiles,
	composer_manager: Option<String>,
	model_settings: model_settings::Panel,
	live_reviewer: live_settings::Panel,
	model: Entity<ComposerInput>,
	cwd: Entity<ComposerInput>,
	account: Entity<ComposerInput>,
	effort: ConversationReasoningEffort,
	sandbox: ChiefSandboxDto,
	submission: drafts::SubmissionState,
	command_epoch: u64,
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
	older_retry_after: Option<std::time::Instant>,
	older_scroll_anchor: Option<activity::HistoryScrollAnchor>,
	poll_task: Option<Task<()>>,
	request: Option<ChiefRequestResult>,
	request_task: Option<Task<()>>,
	misalignment_reviewed: Option<(String, String)>,
	guardian: guardian::Panel,
	archive: archive::Panel,
	mcp_form_event: Option<i64>,
	installation: install::Panel,
	mcp_url_opened: Option<(i64, String)>,
	mcp_inputs: std::collections::BTreeMap<String, Entity<ComposerInput>>,
	mcp_answers: std::collections::BTreeMap<String, serde_json::Value>,
	question_timers: std::collections::BTreeMap<i64, requests::QuestionTimer>,
	question_inputs: std::collections::BTreeMap<String, Entity<ComposerInput>>,
	question_notices: question_notices::QuestionNotices,
	restored_question_drafts: Vec<decodex_protocol::DesktopQuestionDraft>,
	collapsed_async_questions: std::collections::BTreeSet<String>,
	async_question_threads: std::collections::BTreeMap<String, String>,
	async_question_choices:
		std::collections::BTreeMap<(String, String), async_questions::ChoiceDraft>,
	async_question_inputs: std::collections::BTreeMap<(String, String), Entity<ComposerInput>>,
	details_visible: bool,
	accounts: Vec<(String, String)>,
	setup_expanded: bool,
}

struct ChiefInputs {
	model: Entity<ComposerInput>,
	cwd: Entity<ComposerInput>,
	composer: Entity<ComposerInput>,
}

#[derive(Clone)]
struct QueuedCommand {
	profile: ClientProfile,
	action: ChiefActionDto,
	key: IdempotencyKey,
	pending: PendingCommand,
}

#[derive(Clone)]
struct PendingCommand {
	recovery: Option<decodex_protocol::DesktopRecoveredDraft>,
	key: Option<IdempotencyKey>,
	steer: Option<decodex_protocol::ChiefSteerIdentity>,
	epoch: u64,
	execution_intent: Option<(String, u64)>,
	draft: Option<String>,
	owner: Option<String>,
	attachments: Option<Vec<decodex_protocol::ChiefAttachmentDto>>,
	references: Option<Vec<decodex_protocol::ChiefTaskReferenceDto>>,
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
		let inputs = Self::new_inputs(cx);
		let mut surface = Self::with_inputs(inputs, cx);
		surface.restore_unbound_draft(cx);
		surface
	}

	// Keep the initial values for this view's owned state together.
	#[allow(clippy::too_many_lines)]
	fn with_inputs(inputs: ChiefInputs, cx: &mut Context<Self>) -> Self {
		let ChiefInputs { model, cwd, composer } = inputs;
		Self {
			voice: None,
			voice_task: None,
			audio_inputs: Vec::new(),
			audio_input: String::new(),
			dictation: None,
			dictation_task: None,
			activity_detail: Default::default(),
			resources: None,
			resources_task: None,
			usage_estimate: None,
			usage_estimate_task: None,
			native_history: Default::default(),
			integrations: None,
			integrations_task: None,
			integration_refresh_task: None,
			integration_feedback: String::new(),
			mcp_login: None,
			mcp_login_task: None,
			resource_mutation_task: None,
			resource_feedback: String::new(),
			resource_title: resource_field("Link title", "Resource title", cx),
			resource_url: resource_field("https://…", "Resource URL", cx),
			capabilities: None,
			capabilities_context: None,
			capabilities_checked: None,
			capability_task: None,
			fast: false,
			service_tier: None,
			steer: true,
			media_spare: None,
			media_warm_attempted: false,
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
			attachments: vec![],
			task_references: vec![],
			task_reference_search: Self::new_task_reference_search(cx),
			draft_profiles: Default::default(),
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
			history_selected: None,
			latest_follow_work: None,
			connection_details_expanded: false,
			history_hover: None,
			history_navigation: None,
			native_agents: Default::default(),
			output_stream: Default::default(),
			history_read_at: None,
			agent_tree_visible: true,
			agent_tree_collapsed: Default::default(),
			sidebar_visible: true,
			sidebar_width: crate::panel_preferences::PanelDefaults::configured().sidebar.into(),
			agent_panel_width: crate::panel_preferences::PanelDefaults::configured().sidebar.into(),
			graph_panel_height: crate::panel_preferences::PanelDefaults::configured().dock.into(),
			focused_panel: None,
			sidebar_drag: None,
			history_cache: Default::default(),
			expanded_progress: Default::default(),
			transcript_scroll: Default::default(),
			history_follow_paused: Default::default(),
			details_visible: false,
			accounts: vec![],
			setup_expanded: false,
			composer,
			composer_footer_height: 74.,
			model,
			model_settings: Default::default(),
			live_reviewer: Default::default(),
			cwd,
			account: Self::account_input(cx),
			effort: ConversationReasoningEffort::High,
			sandbox: ChiefSandboxDto::ReadOnly,
			submission: Default::default(),
			command_epoch: 0,
			sending: false,
			uncertain: false,
			feedback: String::new(),
			history: None,
			history_task: None,
			history_requested_for: None,
			older_history: Default::default(),
			older_task: None,
			loading_older: false,
			older_retry_after: None,
			older_scroll_anchor: None,
			poll_task: None,
			request: None,
			request_task: None,
			misalignment_reviewed: None,
			guardian: Default::default(),
			archive: Default::default(),
			mcp_form_event: None,
			installation: Default::default(),
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

	fn new_inputs(cx: &mut Context<Self>) -> ChiefInputs {
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
		ChiefInputs { model, cwd, composer }
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
		self.history_read_at = Some(std::time::Instant::now());
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
					if matches!(history, ChiefHistoryResult::Available { .. })
						|| !surface.history.as_ref().is_some_and(|(current, saved)| {
							current == &id && matches!(saved, ChiefHistoryResult::Available { .. })
						}) {
						if let Some(scroll) = surface.transcript_scroll.get(&id)
							&& surface.voice.is_none()
							&& !surface.history_follow_paused.contains(&id)
							&& (scroll.offset().y + scroll.max_offset().y).abs() < px(24.0)
						{
							surface.latest_follow_work = Some(id.clone());
						}
						if surface.feedback == "Message saved · Waiting for agent…" {
							let last_reply = |history: &ChiefHistoryResult| match history {
								ChiefHistoryResult::Available { entries, .. } => entries
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
						surface.prepare_async_question_inputs(&id, &history, cx);
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

	#[cfg(test)]
	fn cycle_model(&mut self, cx: &mut Context<Self>) {
		if let Some(decodex_protocol::ChiefCapabilitiesResult::Available { models, .. }) =
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
						&& !selected
							.as_ref()
							.is_some_and(|id| surface.history_follow_paused.contains(id))
						&& (scroll.offset().y + scroll.max_offset().y).abs() < px(24.0)
					{
						scroll.scroll_to_bottom();
					}
					surface.prepare_question_inputs(&result, cx);
					surface.request = Some(result);
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
		let build = || -> Result<ChiefActionDto, String> {
			let prompt = HistoryText::new(if text.trim().is_empty() {
				"Please inspect the selected tasks and attached files.".into()
			} else {
				text.clone()
			})
			.map_err(|_| "Message is too long")?;
			if !self.draft_owner_available() {
				return Err("This draft's conversation is unavailable. Select a conversation before sending.".into());
			}
			if let Some(root) = self.snapshot.as_ref().and_then(|snapshot| {
				snapshot
					.work_items
					.iter()
					.find(|work| {
						Some(&work.id) == self.composer_manager.as_ref().or(self.selected.as_ref())
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
				effort: self.effort.clone(),
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
					reasoning_effort: self.effort.clone(),
					fast: self.fast,
					service_tier: self.service_tier.clone(),
				};
				let attachments = self.attachments.clone();
				let action = match action {
					ChiefActionDto::Start(start) => ChiefActionDto::StartConfigured {
						start,
						execution,
						attachments,
						task_references: self.task_references.clone(),
					},
					ChiefActionDto::Send { root_id, text } =>
						self.configured_send(root_id, text, attachments),
					action => action,
				};
				self.follow_latest_after_send(cx);
				self.execute(action, Some(text), cx);
			},
			Err(message) => {
				self.feedback = message;
				cx.notify();
			},
		}
	}

	fn command_connection_ready(&self) -> bool {
		self.state == LoadState::Ready
			|| (self.state == LoadState::Loading
				&& self.status_before_refresh.is_none()
				&& self.snapshot.is_some())
	}

	pub(super) fn steer_identity(
		&self,
		action: &ChiefActionDto,
		key: &IdempotencyKey,
	) -> Option<decodex_protocol::ChiefSteerIdentity> {
		let ChiefActionDto::Steer { work_id, turn_id, .. } = action else { return None };
		let work =
			self.snapshot.as_ref()?.work_items.iter().find(|work| work.id == work_id.as_str())?;
		Some(decodex_protocol::ChiefSteerIdentity {
			work_id: work_id.clone(),
			thread_id: WireText::new(work.codex_thread_id.clone()?).ok()?,
			turn_id: turn_id.clone(),
			submission_id: key.clone(),
		})
	}

	fn execute(&mut self, action: ChiefActionDto, draft: Option<String>, cx: &mut Context<Self>) {
		if self.draft_quit_in_progress() {
			return;
		}
		if let ChiefActionDto::Interrupt { work_id, turn_id } = action {
			self.request_interrupt(work_id, turn_id, cx);
			return;
		}
		if self.sending || (self.uncertain && !matches!(&action, ChiefActionDto::Interrupt { .. }))
		{
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
		self.sending = true;
		self.feedback = "Waiting for durable acceptance…".into();
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
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "Cannot create client runtime".to_string())?;
			runtime
				.block_on(ChiefClient::new(profile).execute(action, key))
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
		result: Result<ChiefCommandResponse, String>,
		cx: &mut Context<Self>,
	) {
		if self.command_epoch != pending.epoch {
			return;
		}
		self.sending = false;
		self.remove_command_draft_fence(&pending);
		if !matches!(&result, Ok(ChiefCommandResponse::Accepted { .. })) {
			self.retain_failed_command_draft(
				&pending,
				matches!(&result, Ok(ChiefCommandResponse::PotentiallyDispatched { .. })),
				cx,
			);
		}
		if !matches!(&result, Ok(ChiefCommandResponse::PotentiallyDispatched { .. })) {
			self.submission.unconfirmed.retain(|key| Some(key) != pending.key.as_ref());
		}
		// A different command must not erase an unresolved steering receipt.
		if !matches!(&result, Ok(ChiefCommandResponse::PotentiallyDispatched { .. }))
			&& pending.steer.is_some()
			&& self.submission.pending.as_ref().and_then(|saved| saved.steer.as_ref())
				== pending.steer.as_ref()
		{
			self.submission.pending = None;
		}
		let current_owner = self.composer_manager.clone().or_else(|| self.root_id());
		let same_owner = current_owner == pending.owner
			|| (pending.owner.is_none()
				&& matches!(&result, Ok(ChiefCommandResponse::Accepted { work_id }) if current_owner.as_deref()==Some(work_id.as_str())));
		if !same_owner
			&& matches!(&result, Ok(ChiefCommandResponse::Accepted { .. }))
			&& let Some(owner) = &pending.owner
			&& self.draft_profiles.texts.get(owner).map(String::as_str) == pending.draft.as_deref()
		{
			self.draft_profiles.texts.remove(owner);
		}
		if matches!(&result, Ok(ChiefCommandResponse::Accepted { .. }))
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
		if matches!(&result, Ok(ChiefCommandResponse::Accepted { .. }))
			&& let Some(sent) = &pending.references
		{
			self.clear_sent_task_references(sent, same_owner, pending.owner.as_deref());
		}
		if matches!(&result, Ok(ChiefCommandResponse::Accepted { .. })) {
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
		result: Result<ChiefCommandResponse, String>,
		draft: Option<&str>,
		cx: &mut Context<Self>,
	) {
		let surface = self;
		match result {
			Ok(ChiefCommandResponse::Accepted { work_id }) => {
				surface.feedback = if draft.is_some() {
					"Message saved · Waiting for agent…".into()
				} else {
					String::new()
				};
				if draft.is_some() && surface.selected.as_deref() == Some(work_id.as_str()) {
					surface.follow_latest_after_send(cx);
				}
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

	pub(crate) fn bind_profile(&mut self, profile: Option<ClientProfile>, cx: &mut Context<Self>) {
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
		let epoch = self.native_history.epoch + 1;
		self.native_history = Default::default();
		self.native_history.epoch = epoch;
		self.generation += 1;
		self.refresh_failures = 0;
		self.task = None;
		self.profile = profile;
		self.output_stream = Default::default();
		self.interrupting = None;
		self.interrupt_task = None;
		self.history_read_at = None;
		self.clear_activity_detail();
		self.resources = None;
		self.resources_task = None;
		self.usage_estimate = None;
		self.usage_estimate_task = None;
		self.integrations = None;
		self.integrations_task = None;
		self.integration_refresh_task = None;
		self.integration_feedback.clear();
		self.mcp_login = None;
		self.mcp_login_task = None;
		self.resource_mutation_task = None;
		self.resource_feedback.clear();
		self.resource_title.update(cx, |input, cx| input.clear(cx));
		self.resource_url.update(cx, |input, cx| input.clear(cx));
		self.capability_task = None;
		self.capabilities = None;
		self.capabilities_context = None;
		self.fast = false;
		self.service_tier = None;
		self.capabilities_checked = None;
		self.reset_model_settings();
		self.reset_live_reviewer();
		self.snapshot = None;
		self.pages.clear();
		self.page_views.clear();
		self.graph_expanded = false;
		self.history_cache.clear();
		self.history_marks.clear();
		self.history_marks_work = None;
		self.older_history.clear();
		self.older_task = None;
		self.loading_older = false;
		self.older_retry_after = None;
		self.transcript_scroll.clear();
		self.history_follow_paused.clear();
		self.graph_scope = None;
		self.graph_selected = None;
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
				cx.background_executor().timer(std::time::Duration::from_millis(500)).await;
				if surface
					.update(cx, |surface, cx| {
						surface.save_draft_document(cx);
						if should_poll_snapshot(surface.profile.is_some(), &surface.state, false) {
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

	pub(crate) fn mark_stale(&mut self, cx: &mut Context<Self>) {
		self.question_notices = Default::default();
		self.clear_activity_detail();
		self.output_stream = Default::default();
		self.generation += 1;
		self.guardian_disconnected();
		self.archive_disconnected();
		self.installation_disconnected();
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
				let changed = match &result {
					Ok(ChiefSnapshotResult::Available(snapshot)) =>
						surface.snapshot.as_ref() != Some(snapshot),
					_ => true,
				};
				surface.apply_result(result);
				if surface.current_model_catalog(cx).is_none()
					|| surface.capabilities_checked.is_none_or(|at| at.elapsed().as_secs() >= 60)
				{
					surface.load_capabilities(cx);
				}
				if changed || surface.history_read_at.is_none_or(|at| at.elapsed().as_secs() >= 2) {
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

	fn apply_result(&mut self, result: Result<ChiefSnapshotResult, ()>) {
		if !matches!(&result, Ok(ChiefSnapshotResult::Available(_))) {
			self.reset_live_reviewer();
			self.question_notices = Default::default();
			self.clear_activity_detail();
		}
		match result {
			Ok(ChiefSnapshotResult::Available(snapshot)) => {
				self.invalidate_model_settings(&snapshot);
				self.invalidate_live_reviewer_for_snapshot(&snapshot);
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
							&& (work.dispatch_state != ChiefDispatchStateDto::Idle
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
					self.native_history.reset();
				}
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
			Ok(ChiefSnapshotResult::Unavailable) => {
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
		[
			("Review", &self.guardian.feedback),
			("Installation", &self.installation.feedback),
			("Tools and plugins", &self.integration_feedback),
			("Task resources", &self.resource_feedback),
		]
		.into_iter()
		.filter(|(_, detail)| !detail.is_empty())
		.map(|(title, detail)| (title, detail.clone()))
		.collect()
	}

	pub(crate) fn status_notice(&self) -> Option<(&'static str, String, bool)> {
		if !self.sending
			&& !self.feedback.is_empty()
			&& self.feedback != "Message saved · Waiting for agent…"
		{
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
				"Reconnecting to Chief. Work may still be running. Connection details are in Settings → Diagnostics."
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
		snapshot: &ChiefSnapshotDto,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let target = cx.entity();
		let status = graph::state_in(snapshot, work).0;
		div()
			.flex_none()
			.px_4()
			.py_1()
			.flex()
			.flex_col()
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.child(
						div()
							.text_size(px(12.))
							.text_color(rgb(ui_theme::TEXT_MUTED))
							.child(format!("{} · {status}", self.work_label(work))),
					)
					.child(
						div()
							.child(self.workspace_action(
								"inspect-work".into(),
								"Details".into(),
								|s, cx| {
									s.details_visible = !s.details_visible;
									if s.details_visible
										&& let Some(work) = s.selected.clone()
										&& s.resources
											.as_ref()
											.is_none_or(|(owner, _)| owner != &work)
									{
										s.toggle_resources(&work, cx);
									}
									cx.notify();
								},
								cx,
							))
							.relative()
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
							),
					),
			)
			.into_any_element()
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
		if self.loading_older || self.older_scroll_anchor.is_some() {
			return false;
		}
		let Some(id) = self.selected.as_ref().filter(|id| self.history_follow_paused.contains(*id))
		else {
			return false;
		};
		let Some((owner, ChiefHistoryResult::Available { next_before, .. })) =
			self.history.as_ref()
		else {
			return false;
		};
		if owner != id
			|| self.older_history.get(id).map_or(*next_before, |(_, cursor)| *cursor).is_none()
		{
			return false;
		}
		self.transcript_scroll.get(id).is_some_and(|scroll| {
			let height = f32::from(scroll.bounds().size.height);
			height > 0. && -f32::from(scroll.offset().y) <= (height * 0.6).clamp(240., 600.)
		})
	}

	fn load_older_history(&mut self, cx: &mut Context<Self>) {
		if self.loading_older
			|| self.older_retry_after.is_some_and(|at| at > std::time::Instant::now())
		{
			return;
		}
		let (Some(profile), Some((id, ChiefHistoryResult::Available { next_before, .. }))) =
			(self.profile.clone(), self.history.as_ref())
		else {
			return;
		};
		if self.selected.as_ref() != Some(id) {
			return;
		}
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
				if let Some(ChiefHistoryResult::Available { entries, next_before, .. }) = result
					&& next_before.is_none_or(|next| next < before)
				{
					s.older_retry_after = None;
					if s.selected.as_ref() == Some(&id)
						&& let Some(scroll) = s.transcript_scroll.get(&id)
					{
						s.older_scroll_anchor = Some(activity::HistoryScrollAnchor {
							work: id.clone(),
							offset: f32::from(scroll.offset().y),
							maximum: f32::from(scroll.max_offset().y),
							message: s
								.history_marks
								.iter()
								.next()
								.map(|(id, mark)| (id.clone(), mark.position.get())),
						});
					}
					let page = s.older_history.entry(id).or_default();
					page.0.extend(entries);
					page.0.sort_by_key(|entry| entry.id);
					page.0.dedup_by_key(|entry| entry.id);
					page.1 = next_before;
				} else {
					s.older_retry_after =
						Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn history_panel(&self, work: &ChiefWorkItemDto, cx: &mut Context<Self>) -> impl IntoElement {
		let panel = div()
			.w_full()
			.min_w_0()
			.flex_none()
			.flex()
			.flex_col()
			.gap(px(ui_theme::MESSAGE_GAP))
			.child(self.native_timeline_panel(work, cx));
		if self.native_history_active(work) {
			return self.history_activity(panel.child(self.native_receipts_panel(work, cx)), work);
		}
		let mut panel = panel.debug_selector(|| "saved-local-history".into());
		match self.history.as_ref().filter(|(id, _)| id == &work.id).map(|(_, history)| history) {
			Some(ChiefHistoryResult::Available {
				entries, has_more, next_before, live, ..
			}) => {
				let older = self.older_history.get(&work.id);
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
						div()
							.w_full()
							.py(px(2.))
							.child(text_reveal::StreamingText {
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
			Some(ChiefHistoryResult::Unavailable) =>
				panel = panel.child(muted("Messages could not be loaded. Retrying…")),
			None => panel = panel.child(muted("Loading messages…")),
		}
		self.history_activity(panel, work)
	}

	fn history_activity(&self, mut panel: gpui::Div, work: &ChiefWorkItemDto) -> gpui::Div {
		let active = matches!(
			work.dispatch_state,
			ChiefDispatchStateDto::Running | ChiefDispatchStateDto::Dispatching
		) || (self.selected.as_ref() == Some(&work.id)
			&& (self.sending || self.feedback == "Message saved · Waiting for agent…"));
		if active && self.composer_unavailable_reason().is_none() {
			panel = panel.child(
				div()
					.id("reply-activity")
					.role(Role::Status)
					.aria_label("Agent is working")
					.h(px(22.))
					.flex()
					.items_center()
					.gap(px(4.))
					.children((0..3).map(|index| {
						div()
							.size(px(4.))
							.rounded_full()
							.bg(rgb(ui_theme::TEXT_MUTED))
							.with_animation(
								format!("reply-working-{index}"),
								gpui::Animation::new(std::time::Duration::from_millis(1100))
									.repeat(),
								move |dot, phase| {
									dot.opacity(
										0.35 + 0.65
											* ((phase * std::f32::consts::TAU
												- index as f32 * 0.7)
												.sin() * 0.5 + 0.5),
									)
								},
							)
					})),
			);
		}
		panel.children(self.live_chat_caption())
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
					s.execute(ChiefActionDto::CancelCapacityRetry { work_id, event_id }, None, cx);
				}
			},
			cx,
		)
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
			ChiefSandboxDto::ReadOnly => ChiefSandboxDto::WorkspaceWrite,
			ChiefSandboxDto::WorkspaceWrite => ChiefSandboxDto::FullAccess,
			ChiefSandboxDto::FullAccess => ChiefSandboxDto::ReadOnly,
		};
		cx.notify();
	}
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
fn muted(text: impl Into<SharedString>) -> impl IntoElement {
	div()
		.text_size(px(ui_theme::CAPTION_SIZE))
		.text_color(rgb(ui_theme::TEXT_MUTED))
		.child(text.into())
}
fn history_entry(entry: &decodex_protocol::ChiefHistoryEntryDto) -> gpui::Div {
	history_entry_with_key(entry, &entry.id.to_string())
}

fn history_entry_with_key(
	entry: &decodex_protocol::ChiefHistoryEntryDto,
	identity: &str,
) -> gpui::Div {
	let user = entry.kind == "user";
	let visible_text = if entry.kind == "assistant" {
		markdown::response_text(&entry.text)
	} else {
		entry.text.clone()
	};
	if matches!(entry.kind.as_str(), "execution_notice" | "capacity_retry_pending" | "stopped") {
		return div()
			.w_full()
			.py_2()
			.text_size(px(11.))
			.text_color(rgb(if entry.kind == "stopped" {
				ui_theme::TEXT_MUTED
			} else {
				ui_theme::AMBER
			}))
			.child(selectable_text::SelectableText {
				key: format!("notice-{identity}"),
				text: entry.text.clone(),
				highlights: vec![],
				links: vec![],
			});
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
						.py(px(9.))
						.rounded(px(18.0))
						.bg(rgba(0xffffff0e))
				})
				.when(!user, |body| body.w_full().py(px(2.)))
				.when(entry.kind == "instruction", |body| {
					body.pl_3()
						.border_l_2()
						.border_color(rgb(ui_theme::BLUE))
						.child(muted("Manager instruction"))
				})
				.child(markdown::render(&visible_text, &format!("message-{identity}")))
				.children(entry.weather.iter().enumerate().map(|(i, forecast)| {
					let date = time::OffsetDateTime::from_unix_timestamp(
						entry.created_at_micros / 1_000_000,
					)
					.ok()
					.map(|d| format!("{} · Saved forecast", d.date()))
					.unwrap_or_else(|| "Saved forecast".into());
					weather::render(forecast, &date, &format!("{identity}-{i}"))
				}))
				.when(!user, |body| {
					body.child(
						div()
							.mt(px(ui_theme::METADATA_GAP))
							.flex()
							.items_center()
							.gap(px(8.))
							.child(reply_metrics(entry))
							.when(entry.kind == "assistant", |row| {
								row.child(markdown::copy_button(
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

pub(crate) fn compact_tokens(value: u64) -> String {
	let (divisor, suffix) = if value >= 999_950_000 {
		(1_000_000_000.0, "B")
	} else if value >= 999_950 {
		(1_000_000.0, "M")
	} else if value >= 1000 {
		(1000.0, "K")
	} else {
		return value.to_string();
	};
	let text = format!("{:.1}", value as f64 / divisor);
	format!("{}{suffix}", text.trim_end_matches(".0"))
}

struct ReplyMetricsTip(String);
impl Render for ReplyMetricsTip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
			.px_3()
			.py_2()
			.rounded(px(8.))
			.bg(rgb(0x242429))
			.text_size(px(ui_theme::CAPTION_SIZE))
			.text_color(rgb(ui_theme::TEXT))
			.child(self.0.clone())
	}
}
fn reply_metrics(entry: &decodex_protocol::ChiefHistoryEntryDto) -> impl IntoElement {
	let label = entry
		.duration_ms
		.map(|duration| {
			if duration >= 60_000 {
				format!("Worked for {}m {}s", duration / 60_000, duration % 60_000 / 1000)
			} else {
				format!("Worked for {:.1}s", duration as f64 / 1000.0)
			}
		})
		.unwrap_or_else(|| "Details".into());
	div()
		.id(SharedString::from(format!("reply-details-{}", entry.id)))
		.h(px(24.))
		.flex()
		.items_center()
		.gap(px(4.))
		.text_size(px(ui_theme::CAPTION_SIZE))
		.text_color(rgb(ui_theme::TEXT_MUTED))
		.when(entry.duration_ms.is_some() || entry.usage.is_some(), |row| row.child(label))
		.when_some(entry.usage.as_ref(), |row, usage| {
			let detail = format!(
				"In {} · Out {} tokens",
				compact_tokens(usage.input_tokens),
				compact_tokens(usage.output_tokens)
			);
			row.child("›")
				.hover(|s| s.text_color(rgb(ui_theme::TEXT)))
				.tooltip(move |_, cx| cx.new(|_| ReplyMetricsTip(detail.clone())).into())
		})
}

impl ChiefSurface {}

impl Render for ChiefSurface {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.prepare_voice_media(window);
		self.render_workspace(window, cx)
	}
}

impl ChiefSurface {
	fn render_preferences(&self, cx: &mut Context<Self>) -> impl IntoElement {
		div()
			.w_full()
			.flex()
			.flex_col()
			.gap(px(6.0))
			.px(px(0.0))
			.py(px(0.0))
			.child(
				div()
					.id("chief-advanced-preferences")
					.role(Role::Button)
					.aria_label("New agent defaults")
					.aria_expanded(self.setup_expanded)
					.tab_index(0)
					.h(px(32.0))
					.flex()
					.items_center()
					.cursor_pointer()
					.rounded(px(6.0))
					.hover(|s| s.bg(rgba(0xffffff09)))
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
					.child("New agent defaults")
					.child(super::workspace_symbols::icon(
						super::workspace_symbols::Symbol::ChevronDown,
					))
					.smooth(),
			)
			.children(match self.current_model_catalog(cx) {
				Some(decodex_protocol::ChiefCapabilitiesResult::Available {
					memory_enabled: Some(enabled),
					..
				}) => Some(
					div()
						.h(px(26.))
						.flex()
						.items_center()
						.justify_between()
						.child(muted("Codex memory"))
						.child(muted(if *enabled { "On" } else { "Off" })),
				),
				_ => None,
			})
			.child(disclosure(
				"chief-advanced-motion",
				self.setup_expanded,
				self.render_setup_controls(cx),
			))
			.when_some(self.selected.as_ref(), |panel, work| {
				panel
					.child(self.resources_panel(work, cx))
					.child(self.integrations_panel(work, cx))
					.child(self.usage_estimate_panel(work, cx))
					.children(
						self.snapshot
							.as_ref()
							.and_then(|s| s.work_items.iter().find(|w| &w.id == work))
							.map(|item| {
								div()
									.flex()
									.flex_col()
									.gap_2()
									.child(self.model_settings_panel(item, cx))
									.child(self.live_reviewer_panel(item, cx))
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
		div()
			.flex()
			.flex_col()
			.gap_2()
			.child(muted("Applies when starting a new agent."))
			.child(
				div()
					.flex()
					.items_center()
					.gap_2()
					.child(div().w(px(72.)).child(muted("Directory")))
					.child(div().flex_1().min_w_0().child(self.cwd.clone())),
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

fn resource_field(
	placeholder: &'static str,
	label: &'static str,
	cx: &mut Context<ChiefSurface>,
) -> Entity<ComposerInput> {
	cx.new(|cx| ComposerInput::with_placeholder(40, placeholder, label, cx))
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
		assert_eq!(super::compact_tokens(999950000), "1B");
		assert_eq!(super::compact_tokens(2450000000), "2.5B");
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
	fn history_prefetch_requires_reading_near_top_and_a_remaining_cursor(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.simulate_resize(gpui::size(px(1400.), px(320.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.graph_visible = false;
			if let Some((_, ChiefHistoryResult::Available { next_before, .. })) = &mut s.history {
				*next_before = Some(1);
			}
		});
		visual.update(|window, cx| window.draw(cx).clear());
		surface.update(visual, |s, _| {
			s.transcript_scroll["chief"].set_offset(gpui::point(px(0.), px(-20.)));
			assert!(
				!s.history_prefetch_needed(),
				"startup and bottom-follow must not fetch all history"
			);
			s.history_follow_paused.insert("chief".into());
			assert!(s.history_prefetch_needed(), "prefetch before reaching the edge");
			s.loading_older = true;
			assert!(!s.history_prefetch_needed(), "only one request may be in flight");
			s.loading_older = false;
			s.older_history.insert("chief".into(), (vec![], None));
			assert!(!s.history_prefetch_needed(), "stop when history is exhausted");
		});
	}

	#[gpui::test]
	fn completed_dispatch_clears_waiting_feedback_without_observing_running(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			let mut snapshot = s.snapshot.clone().unwrap();
			snapshot.pending_events.clear();
			for work in &mut snapshot.work_items {
				work.dispatch_state = ChiefDispatchStateDto::Idle;
				work.active_turn_id = None;
			}
			s.feedback = "Message saved · Waiting for agent…".into();
			s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot)));
			assert!(
				s.feedback.is_empty(),
				"a completed or failed fast turn must not leave a phantom queue"
			);
		});
	}

	#[gpui::test]
	fn pending_capacity_retry_has_an_actionable_cancel_button(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |surface, _| {
			surface.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				runtime_source: None,
				workspaces: vec![],
				work_items: vec![ChiefWorkItemDto {
					id: "root".into(),
					parent_goal_id: None,
					kind: ChiefWorkKindDto::Goal,
					title: "Chief".into(),
					codex_thread_id: Some("thread".into()),
					active_turn_id: None,
					dispatch_state: ChiefDispatchStateDto::Idle,
					status: ChiefWorkStatusDto::Open,
					next_check_at_micros: None,
					created_at_micros: 1,
					updated_at_micros: 1,
				}],
				dependencies: vec![],
				pending_events: vec![],
			})));
			surface.history = Some((
				"root".into(),
				ChiefHistoryResult::Available {
					questions: vec![],
					questions_truncated: false,
					questions_recovering: false,
					misalignment: None,
					live: vec![],
					next_before: None,
					usage: None,
					entries: vec![decodex_protocol::ChiefHistoryEntryDto {
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
			window.resize(gpui::size(px(1180.0), px(1200.0)));
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
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |surface, _| {
			surface.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				runtime_source: None,
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
					questions: vec![],
					questions_truncated: false,
					questions_recovering: false,
					misalignment: None,
					usage: None,
					entries: vec![decodex_protocol::ChiefHistoryEntryDto {
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
	fn offline_commands_preserve_editable_draft_and_attachments(cx: &mut gpui::TestAppContext) {
		use std::os::unix::fs::{MetadataExt, PermissionsExt};
		let root = tempfile::tempdir_in("/tmp").unwrap();
		let path = root.path().canonicalize().unwrap();
		std::fs::create_dir(path.join("server")).unwrap();
		std::fs::set_permissions(path.join("server"), std::fs::Permissions::from_mode(0o700))
			.unwrap();
		let uid = std::fs::metadata(&path).unwrap().uid();
		let config = path.join("config.toml");
		std::fs::write(&config, format!("version = 1\nactive_profile = \"local\"\ncache = {{}}\n[profiles.local]\nkind = \"local\"\npolicy = \"same_uid\"\nservice_owner_uid = {uid}\nexpected_server_identity = \"018f0f9e-7b6e-4a31-8f4c-1d2e3f405162\"\n")).unwrap();
		std::fs::set_permissions(config, std::fs::Permissions::from_mode(0o600)).unwrap();
		let profile = ClientProfile::load(&path, None).unwrap();
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.profile = Some(profile);
			s.composer.update(cx, |input, cx| input.set_content("draft", cx));
			s.attachments.push(decodex_protocol::ChiefAttachmentDto {
				path: ConversationWorkingDirectory::new("/tmp/draft.png").unwrap(),
				image: true,
			});
			s.task_references.push(decodex_protocol::ChiefTaskReferenceDto {
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
					ChiefActionDto::Send {
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
			s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
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
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			let file = decodex_protocol::ChiefAttachmentDto {
				path: ConversationWorkingDirectory::new("/tmp/draft.png").unwrap(),
				image: true,
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
				Ok(ChiefCommandResponse::Accepted { work_id: EntityId::new("root").unwrap() }),
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
			assert!(surface.submission.command.is_none());
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
					questions: vec![],
					questions_truncated: false,
					questions_recovering: false,
					misalignment: None,
					usage: None,
					entries: vec![decodex_protocol::ChiefHistoryEntryDto {
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
			s.snapshot = Some(ChiefSnapshotDto {
				runtime_source: None,
				workspaces: vec![],
				work_items: vec![],
				dependencies: vec![],
				pending_events: vec![],
			});
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
			assert_eq!(s.status_notice().unwrap().0, "In use by another app");
			assert!(s.thread_in_use("chief"));
			assert!(!s.thread_in_use("another-chief"));
			s.snapshot.as_mut().unwrap().pending_events.clear();
			assert!(!s.thread_in_use("chief"));
			assert!(s.status_notice().is_none());
			s.selected = Some("another-chief".into());
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
