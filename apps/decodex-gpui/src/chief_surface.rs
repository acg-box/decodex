//! Chief conversation and work overview. The service owns records and execution.

use decodex_protocol::{
	ChiefActionDto, ChiefClient, ChiefCommandResponse, ChiefDispatchStateDto, ChiefHistoryResult,
	ChiefRequestResult, ChiefSandboxDto, ChiefSnapshotDto, ChiefSnapshotResult, ChiefStartDto,
	ChiefWorkItemDto, ChiefWorkKindDto, ChiefWorkStatusDto, ClientProfile, ConversationModel,
	ConversationReasoningEffort, ConversationWorkingDirectory, EntityId, HistoryText,
	IdempotencyKey, WireText,
};
use gpui::{
	ClipboardItem, Context, Entity, FocusHandle, FontWeight, Render, Role, SharedString, Task,
	Window, div, prelude::*, px, rgb, rgba,
};

use crate::{
	composer_input::{ComposerInput, SubmitComposer},
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

pub(crate) struct ChiefSurface {
	profile: Option<ClientProfile>,
	snapshot: Option<ChiefSnapshotDto>,
	state: LoadState,
	selected: Option<String>,
	task: Option<Task<()>>,
	focus: FocusHandle,
	refresh_focus: FocusHandle,
	copy_focus: FocusHandle,
	generation: u64,
	composer: Entity<ComposerInput>,
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
	poll_task: Option<Task<()>>,
	request: Option<ChiefRequestResult>,
	request_task: Option<Task<()>>,
	response: Entity<ComposerInput>,
	details_visible: bool,
	accounts: Vec<(String, String)>,
	earlier_history_visible: bool,
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
		Self {
			details_visible: false,
			accounts: vec![],
			earlier_history_visible: false,
			composer: cx.new(|cx| {
				ComposerInput::with_placeholder(35, "Message your Chief…", "Chief message", cx)
			}),
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
			poll_task: None,
			request: None,
			request_task: None,
			response: cx.new(|cx| {
				ComposerInput::with_placeholder(
					40,
					"Exact response JSON",
					"Provider request response JSON",
					cx,
				)
			}),
			profile: None,
			snapshot: None,
			state: LoadState::Idle,
			selected: None,
			task: None,
			focus: cx.focus_handle().tab_index(21).tab_stop(true),
			refresh_focus: cx.focus_handle().tab_index(20).tab_stop(true),
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
				if surface.selected.as_ref() == Some(&id) {
					surface.history = Some((id, history));
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
		let models = crate::conversations::CONVERSATION_MODELS;
		let next = models
			.iter()
			.position(|model| *model == self.model.read(cx).content())
			.map_or(0, |index| (index + 1) % models.len());
		self.model.update(cx, |input, cx| input.set_content(models[next], cx));
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
		self.feedback = "Reading the exact pending request…".into();
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
				if surface.selected == selected {
					surface.request = Some(result);
					surface.feedback.clear();
					surface.response.update(cx, |input, cx| input.clear(cx));
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
		if self.sending || self.uncertain {
			return;
		}
		let text = self.composer.read(cx).content().to_owned();
		if text.trim().is_empty() {
			return;
		}
		let build = || -> Result<ChiefActionDto, String> {
			let prompt = HistoryText::new(text.clone()).map_err(|_| "Message is too long")?;
			if let Some(root) = self.snapshot.as_ref().and_then(|snapshot| {
				snapshot.work_items.iter().find(|work| work.parent_goal_id.is_none())
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
			Ok(action) => self.execute(action, Some(text), cx),
			Err(message) => {
				self.feedback = message;
				cx.notify();
			},
		}
	}

	fn execute(&mut self, action: ChiefActionDto, draft: Option<String>, cx: &mut Context<Self>) {
		if self.sending || self.uncertain {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			self.feedback = "No service profile is configured.".into();
			cx.notify();
			return;
		};
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
				surface.apply_command_result(result, draft.as_deref(), cx);
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
				surface.feedback = format!("Accepted by service · {}", work_id.as_str());
				if draft == Some(surface.composer.read(cx).content()) {
					surface.composer.update(cx, |input, cx| input.clear(cx));
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
		self.snapshot = None;
		self.history = None;
		self.history_task = None;
		self.request = None;
		self.request_task = None;
		self.selected = None;
		self.state = LoadState::Idle;
		self.poll_task = Some(cx.spawn(async move |surface, cx| {
			loop {
				cx.background_executor().timer(std::time::Duration::from_secs(3)).await;
				if surface
					.update(cx, |surface, cx| {
						let active = surface.snapshot.as_ref().is_some_and(|snapshot| {
							snapshot.work_items.iter().any(|work| {
								work.dispatch_state != ChiefDispatchStateDto::Idle
									|| work.next_check_at_micros.is_some()
							}) || !snapshot.pending_events.is_empty()
						});
						if active {
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
				surface.load_history(cx);
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
					self.selected = snapshot.work_items.first().map(|work| work.id.clone());
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

	fn select_relative(&mut self, delta: isize, cx: &mut Context<Self>) {
		let Some(snapshot) = &self.snapshot else {
			return;
		};
		if snapshot.work_items.is_empty() {
			return;
		}
		let index = snapshot
			.work_items
			.iter()
			.position(|work| Some(&work.id) == self.selected.as_ref())
			.unwrap_or(0);
		let next =
			(index as isize + delta).clamp(0, snapshot.work_items.len() as isize - 1) as usize;
		self.selected = Some(snapshot.work_items[next].id.clone());
		self.load_history(cx);
		cx.notify();
	}

	fn status_text(&self) -> String {
		match &self.state {
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
				"Chief work is unavailable. Check the service connection in Health, then refresh."
			}
			.into(),
			LoadState::Stale =>
				"Stale snapshot · the service could not confirm current work. Refresh to retry."
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
			.child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(work.title.clone()))
			.child(muted(format!("{} · {}", judgment(work.status), execution(work.dispatch_state))))
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
					}),
			)
			.when(self.details_visible, |panel| {
				panel
					.child(self.work_metadata(snapshot, work, cx))
					.child(self.work_graph(snapshot, work, cx))
			})
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
			.child(div().text_xl().font_weight(FontWeight::SEMIBOLD).child(work.title.clone()))
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
					.text_sm()
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
					.child("Copy thread ID"),
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
						.child("Interrupt this acknowledged turn"),
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
		let mut panel = div()
			.flex()
			.flex_col()
			.gap_3()
			.child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Work graph"));
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

	fn history_panel(&self, work: &ChiefWorkItemDto, cx: &mut Context<Self>) -> impl IntoElement {
		let mut panel = div().flex().flex_col().gap_3().child(
			div().mt_3().font_weight(FontWeight::SEMIBOLD).child("Conversation and results"),
		);
		match self.history.as_ref().filter(|(id, _)| id == &work.id).map(|(_, history)| history) {
			Some(ChiefHistoryResult::Available { entries, has_more }) => {
				if entries.len() > 4 {
					panel = panel.child(
						div()
							.id("chief-earlier-history")
							.role(Role::Button)
							.tab_index(26)
							.cursor_pointer()
							.text_color(rgb(ui_theme::BLUE))
							.on_key_down(cx.listener(
								|surface, event: &gpui::KeyDownEvent, _, cx| {
									if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
										surface.earlier_history_visible =
											!surface.earlier_history_visible;
										cx.notify();
									}
								},
							))
							.on_click(cx.listener(|surface, _, _, cx| {
								surface.earlier_history_visible = !surface.earlier_history_visible;
								cx.notify();
							}))
							.child(if self.earlier_history_visible {
								"Show recent messages"
							} else {
								"Show earlier saved messages"
							}),
					);
				}
				if *has_more {
					panel = panel.child(muted(
						"Bounded saved history · older entries or part of the content are omitted.",
					));
				}
				if entries.is_empty() {
					panel = panel.child(muted(
						"No saved entries. This does not prove that external work did not run.",
					));
				}
				for entry in entries.iter().skip(if self.earlier_history_visible {
					0
				} else {
					entries.len().saturating_sub(4)
				}) {
					panel = panel.child(
						div()
							.p_3()
							.rounded_md()
							.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
							.child(muted(entry.kind.clone()))
							.child(div().text_sm().child(entry.text.clone())),
					);
				}
			},
			Some(ChiefHistoryResult::Unavailable) =>
				panel = panel
					.child(muted("History unavailable. No inference about execution can be made.")),
			None => panel = panel.child(muted("Loading saved history…")),
		}
		panel
	}

	fn pending_panel(
		&self,
		snapshot: &ChiefSnapshotDto,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let mut panel = div().flex().flex_col().gap_3().child(
			div().mt_3().text_sm().font_weight(FontWeight::SEMIBOLD).child("Pending events"),
		);
		let pending: Vec<_> =
			snapshot.pending_events.iter().filter(|event| event.work_item_id == work.id).collect();
		if pending.is_empty() {
			panel = panel.child(muted("No pending events"));
		}
		for event in pending {
			let event_id = event.id;
			panel = panel.child(
				div()
					.p_3()
					.rounded_md()
					.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL))
					.flex()
					.flex_col()
					.gap_1()
					.child(event.event_kind.clone())
					.child(muted(format!(
						"Receipt {} · {}",
						event.id,
						if event.delivery_claimed {
							"delivery claimed; not disposed"
						} else {
							"awaiting delivery"
						}
					)))
					.child(muted(event.source_event_id.clone())),
				// Only the service can project a supported unresolved request.
			);
			if ["permission_pending", "user_input_pending", "server_request_pending"]
				.contains(&event.event_kind.as_str())
			{
				panel =
					panel.child(
						div()
							.id(SharedString::from(format!("chief-request-{event_id}")))
							.role(Role::Button)
							.tab_index(39)
							.cursor_pointer()
							.text_color(rgb(ui_theme::BLUE))
							.on_click(cx.listener(move |surface, _, _, cx| {
								surface.load_request(event_id, cx)
							}))
							.on_key_down(cx.listener(
								move |surface, event: &gpui::KeyDownEvent, _, cx| {
									if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
										surface.load_request(event_id, cx);
									}
								},
							))
							.child("Inspect decision request"),
					);
			}
		}
		panel
	}

	fn request_panel(
		&self,
		snapshot: &ChiefSnapshotDto,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let mut panel = div().flex().flex_col().gap_3();
		match &self.request {
			Some(ChiefRequestResult::Available { work_id, event_id, method, request_json })
				if work_id == &work.id
					&& snapshot.pending_events.iter().any(|event| event.id == *event_id) =>
			{
				panel = panel
					.child(detail("Decision request", method))
					.child(div().text_sm().child(request_json.as_str().to_owned()))
					.child(muted(
						"Advanced response · enter the exact JSON object required by this provider request",
					))
					.child(div().h(px(64.0)).child(self.response.clone()))
					.child(
						div()
							.id("chief-respond")
							.role(Role::Button)
							.tab_index(41)
							.cursor_pointer()
							.text_color(rgb(ui_theme::BLUE))
							.on_key_down(cx.listener(
								|surface, event: &gpui::KeyDownEvent, _, cx| {
									if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
										let json = surface.response.read(cx).content().to_owned();
										surface.respond(json, cx);
									}
								},
							))
							.on_click(cx.listener(|surface, _, _, cx| {
								let json = surface.response.read(cx).content().to_owned();
								surface.respond(json, cx);
							}))
							.child("Submit this response"),
					);
				for decision in offered_decisions(method, request_json.as_str()) {
					let response = serde_json::json!({ "decision": decision }).to_string();
					let keyboard_response = response.clone();
					panel = panel.child(
						div()
							.id(SharedString::from(format!("chief-decision-{decision}")))
							.role(Role::Button)
							.tab_index(42)
							.cursor_pointer()
							.text_color(rgb(ui_theme::BLUE))
							.on_click(cx.listener(move |surface, _, _, cx| {
								surface.respond(response.clone(), cx)
							}))
							.on_key_down(cx.listener(
								move |surface, event: &gpui::KeyDownEvent, _, cx| {
									if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
										surface.respond(keyboard_response.clone(), cx);
									}
								},
							))
							.child(format!("Respond: {decision}")),
					);
				}
			},
			Some(ChiefRequestResult::Unavailable) =>
				panel =
					panel.child(muted("Request is unavailable, already resolved, or unsupported.")),
			_ => {},
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
				surface.selected = Some(selected.clone());
				surface.load_history(cx);
				cx.notify();
			}))
			.on_key_down(cx.listener(move |surface, event: &gpui::KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					surface.selected = Some(keyboard_id.clone());
					surface.load_history(cx);
					cx.notify();
				}
			}))
			.child(format!("{label} → {}", title(snapshot, id)))
	}

	fn cycle_effort(&mut self, cx: &mut Context<Self>) {
		self.effort = match self.effort {
			ConversationReasoningEffort::Low => ConversationReasoningEffort::Medium,
			ConversationReasoningEffort::Medium => ConversationReasoningEffort::High,
			ConversationReasoningEffort::High => ConversationReasoningEffort::XHigh,
			ConversationReasoningEffort::XHigh => ConversationReasoningEffort::Max,
			ConversationReasoningEffort::Max => ConversationReasoningEffort::Ultra,
			ConversationReasoningEffort::Ultra => ConversationReasoningEffort::Low,
		};
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
	value
		.get("availableDecisions")
		.and_then(|value| value.as_array())
		.into_iter()
		.flatten()
		.filter_map(|value| value.as_str())
		.filter(|value| ["accept", "decline", "cancel"].contains(value))
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
	div().text_sm().text_color(rgb(ui_theme::TEXT_MUTED)).child(text.into())
}
fn detail(label: &str, value: &str) -> impl IntoElement {
	div()
		.flex()
		.flex_col()
		.gap_1()
		.child(muted(label.to_owned()))
		.child(div().text_sm().child(value.to_owned()))
}

impl ChiefSurface {
	fn work_row(
		&self,
		snapshot: &ChiefSnapshotDto,
		work: &ChiefWorkItemDto,
		index: usize,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let selected = self.selected.as_ref() == Some(&work.id);
		let id = work.id.clone();
		let keyboard_id = work.id.clone();
		let pending =
			snapshot.pending_events.iter().filter(|event| event.work_item_id == work.id).count();
		div()
			.id(SharedString::from(format!("chief-work-{}", work.id)))
			.role(Role::Button)
			.tab_index(23 + index as isize)
			.aria_selected(selected)
			.aria_label(format!(
				"{}: {}, {}",
				work.title,
				judgment(work.status),
				execution(work.dispatch_state)
			))
			.p_3()
			.rounded_md()
			.border_1()
			.border_color(rgb(if selected { ui_theme::BLUE } else { ui_theme::LINE }))
			.bg(rgba(if selected {
				ui_theme::SURFACE_OVERLAY_MATERIAL
			} else {
				ui_theme::SURFACE_MATERIAL
			}))
			.cursor_pointer()
			.flex()
			.flex_col()
			.gap_1()
			.on_click(cx.listener(move |surface, _, window, cx| {
				surface.selected = Some(id.clone());
				surface.load_history(cx);
				window.focus(&surface.focus, cx);
				cx.notify();
			}))
			.on_key_down(cx.listener(move |surface, event: &gpui::KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					surface.selected = Some(keyboard_id.clone());
					surface.load_history(cx);
					cx.notify();
				}
			}))
			.child(muted(if work.parent_goal_id.is_none() {
				"PERSONAL CHIEF"
			} else if work.kind == ChiefWorkKindDto::Goal {
				"GOAL"
			} else {
				"WORKER"
			}))
			.child(div().font_weight(FontWeight::SEMIBOLD).child(work.title.clone()))
			.child(muted(judgment(work.status)))
			.child(muted(execution(work.dispatch_state)))
			.when(pending > 0, |row| row.child(muted(format!("{pending} pending events"))))
	}

	fn render_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let mut body = div().flex_1().min_h_0().flex().overflow_hidden();
		if let Some(snapshot) = &self.snapshot {
			if snapshot.work_items.is_empty() {
				body = body.child(
					div()
						.p_8()
						.flex()
						.flex_col()
						.gap_3()
						.child(div().text_xl().child("No Chief work yet"))
						.child(muted(
							"The service has no saved goals, workers, or events to show.",
						)),
				);
			} else {
				let mut list = div()
					.id("chief-work-list")
					.w(px(360.0))
					.flex_shrink_0()
					.h_full()
					.overflow_y_scroll()
					.p_3()
					.flex()
					.flex_col()
					.gap_2()
					.bg(rgba(ui_theme::SIDEBAR_MATERIAL))
					.track_focus(&self.focus)
					.key_context("ChiefWork")
					.on_key_down(cx.listener(|surface, event: &gpui::KeyDownEvent, _, cx| {
						match event.keystroke.key.as_str() {
							"down" => surface.select_relative(1, cx),
							"up" => surface.select_relative(-1, cx),
							"r" => surface.refresh(cx),
							_ => {},
						}
					}));
				for (index, work) in snapshot.work_items.iter().enumerate() {
					list = list.child(self.work_row(snapshot, work, index, cx));
				}
				body = body.child(list);
				if let Some(work) =
					snapshot.work_items.iter().find(|work| Some(&work.id) == self.selected.as_ref())
				{
					body = body.child(
						div()
							.id("chief-detail-scroll")
							.flex_1()
							.min_w_0()
							.h_full()
							.overflow_y_scroll()
							.child(self.details(snapshot, work, cx)),
					);
				}
			}
		} else {
			body = body.child(
				div()
					.p_8()
					.flex()
					.flex_col()
					.gap_3()
					.child(div().text_xl().child("No verified work to show"))
					.child(muted("This overview requires a complete service snapshot.")),
			);
		}
		body
	}
}

impl Render for ChiefSurface {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let refreshing = self.state == LoadState::Loading;
		div()
			.id("chief-overview")
			.on_action(cx.listener(|_, _: &SubmitComposer, _, cx| cx.stop_propagation()))
			.role(Role::Main)
			.aria_label("Chief conversation")
			.size_full()
			.min_w_0()
			.min_h_0()
			.flex()
			.flex_col()
			.bg(rgba(ui_theme::CONTENT_MATERIAL))
			.text_color(rgb(ui_theme::TEXT))
			.child(
				div()
					.p_5()
					.border_b_1()
					.border_color(rgb(ui_theme::LINE))
					.flex()
					.items_center()
					.justify_between()
					.gap_4()
					.child(
						div()
							.flex()
							.flex_col()
							.gap_1()
							.child(
								div()
									.text_2xl()
									.font_weight(FontWeight::SEMIBOLD)
									.child("Your Chief"),
							)
							.child(muted(self.status_text())),
					)
					.child(
						div()
							.id("chief-refresh")
							.role(Role::Button)
							.aria_label("Refresh Chief work")
							.track_focus(&self.refresh_focus)
							.px_4()
							.py_2()
							.rounded_md()
							.border_1()
							.border_color(rgb(ui_theme::LINE_STRONG))
							.cursor_pointer()
							.opacity(if refreshing { 0.5 } else { 1.0 })
							.on_click(cx.listener(|surface, _, _, cx| surface.refresh(cx)))
							.on_key_down(cx.listener(
								|surface, event: &gpui::KeyDownEvent, _, cx| {
									if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
										surface.refresh(cx);
									}
								},
							))
							.child(if refreshing { "Refreshing…" } else { "Refresh" }),
					),
			)
			.child(self.render_body(cx))
			.child(self.render_composer(cx))
	}
}

impl ChiefSurface {
	fn render_composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let has_root = self.snapshot.as_ref().is_some_and(|snapshot| {
			snapshot.work_items.iter().any(|work| work.parent_goal_id.is_none())
		});
		div()
			.p_4()
			.border_t_1()
			.border_color(rgb(ui_theme::LINE))
			.flex()
			.flex_col()
			.gap_2()
			.when(!has_root, |panel| {
				panel
					.child(self.context_choices(cx))
					.child(muted(
						"Start your personal Chief · workers use the selected model with medium effort",
					))
					.child(
						div()
							.flex()
							.gap_3()
							.h(px(38.0))
							.child(div().flex_1().min_w_0().child(self.model.clone()))
							.child(div().flex_1().min_w_0().child(self.cwd.clone()))
							.child(div().flex_1().min_w_0().child(self.account.clone())),
					)
					.child(
						div()
							.flex()
							.gap_4()
							.child(
								div()
									.id("chief-effort")
									.role(Role::Button)
									.tab_index(34)
									.cursor_pointer()
									.on_key_down(cx.listener(
										|surface, event: &gpui::KeyDownEvent, _, cx| {
											if ["enter", "space"]
												.contains(&event.keystroke.key.as_str())
											{
												surface.cycle_effort(cx);
											}
										},
									))
									.on_click(cx.listener(|surface, _, _, cx| {
										surface.cycle_effort(cx);
									}))
									.child(format!("Chief effort: {} ▸", self.effort.as_str())),
							)
							.child(
								div()
									.id("chief-sandbox")
									.role(Role::Button)
									.tab_index(35)
									.cursor_pointer()
									.on_key_down(cx.listener(
										|surface, event: &gpui::KeyDownEvent, _, cx| {
											if ["enter", "space"]
												.contains(&event.keystroke.key.as_str())
											{
												surface.cycle_sandbox(cx);
											}
										},
									))
									.on_click(cx.listener(|surface, _, _, cx| {
										surface.cycle_sandbox(cx);
									}))
									.child(format!("Sandbox: {:?} ▸", self.sandbox)),
							),
					)
			})
			.child(
				div()
					.h(px(64.0))
					.on_action(cx.listener(|surface, _: &SubmitComposer, _, cx| {
						surface.submit(cx);
						cx.stop_propagation();
					}))
					.child(self.composer.clone()),
			)
			.child(
				div()
					.id("chief-send")
					.role(Role::Button)
					.tab_index(36)
					.cursor_pointer()
					.text_color(rgb(ui_theme::BLUE))
					.on_click(cx.listener(|surface, _, _, cx| surface.submit(cx)))
					.on_key_down(cx.listener(|surface, event: &gpui::KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							surface.submit(cx);
						}
					}))
					.child(if self.sending {
						"Sending…"
					} else if self.uncertain {
						"Acceptance unknown · sending blocked"
					} else if has_root {
						"Send to Chief"
					} else {
						"Start Chief"
					}),
			)
			.child(muted(self.feedback.clone()))
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
					.child("Choose model from app catalog ▸"),
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
					.child(format!("Account: {account} ▸")),
			)
			.child(muted("Model support is checked at launch"))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::Focusable;
	#[gpui::test]
	fn enter_submits_chief_composer_and_keeps_unaccepted_draft(cx: &mut gpui::TestAppContext) {
		cx.update(crate::composer_input::bind_keys);
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		let input = surface.update(visual, |surface, cx| {
			surface.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
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
		visual.simulate_keystrokes("enter");
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
			vec!["decline", "accept"]
		);
		assert!(offered_decisions("item/commandExecution/requestApproval", "{}").is_empty());
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
					entries: vec![decodex_protocol::ChiefHistoryEntryDto {
						id: 1,
						kind: "assistant".into(),
						text: "Waiting for your decision".into(),
						created_at_micros: 1,
					}],
					has_more: false,
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
				work_items: vec![],
				dependencies: vec![],
				pending_events: vec![],
			})));
			assert_eq!(surface.state, LoadState::Ready);
			surface.apply_result(Err(()));
			assert_eq!(surface.state, LoadState::Stale);
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
