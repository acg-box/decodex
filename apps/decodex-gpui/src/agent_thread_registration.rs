//! Register native conversations and edit their responsible Agent from the same view.
use std::{collections::HashSet, rc::Rc};

use crate::{
	shell::{
		Destination,
		workspace_symbols::{self, Symbol},
	},
	ui_motion::HoverHint as _,
	ui_theme,
};
use decodex_protocol::{
	AccountClient, AccountsResult, AgentActionDto, AgentCommandResponse, ClientFailure,
	ClientProfile, EntityId, IdempotencyKey, NativeThreadCandidate, NativeThreadPage,
	work::{Agent, ThreadRegistration},
};
use gpui::{
	AnyElement, Context, Entity, Focusable as _, IntoElement as _, KeyDownEvent, Role, Task,
	prelude::*,
};
use tokio::runtime::Builder;

use super::{
	AgentClient, AgentSurface, AgentWorkItemDto, AgentWorkKindDto, Catalog, CatalogEvent,
	ComposerInput,
};

pub(super) enum RegistrationContext<'a> {
	Agent,
	Conversation { owner: Option<&'a Agent> },
}

enum Source {
	Codex,
	Agent(String),
	Conversation,
}

pub(super) struct Panel {
	source: Source,
	search: Entity<ComposerInput>,
	threads: Vec<NativeThreadCandidate>,
	cursor: Option<String>,
	read: Option<Task<()>>,
	read_error: Option<CatalogProblem>,
	selected: Option<NativeThreadCandidate>,
	choosing_agent: bool,
	mutation: Mutation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CatalogProblem {
	NoAccounts,
	Unavailable,
	ClientStart,
	Service(ClientFailure),
}

impl CatalogProblem {
	fn message(self) -> &'static str {
		match self {
			Self::NoAccounts =>
				"Add a subscription account in Settings > Accounts to see its conversations.",
			Self::Unavailable => "Codex could not load the conversation list. Try again.",
			Self::ClientStart => "Could not start the conversation client. Try again.",
			Self::Service(ClientFailure::ProtocolDisconnected) =>
				"Could not reach the Decodex service. Check Diagnostics and try again.",
			Self::Service(ClientFailure::ProtocolTimeout) =>
				"The service took too long to respond. Try again.",
			Self::Service(ClientFailure::ServiceVersionMismatch) =>
				"The app and service use different protocol versions. Update Decodex.",
			Self::Service(ClientFailure::RemoteMutationUnsupported) =>
				"Connect to a local Decodex service to view conversations.",
			Self::Service(failure) => super::super::startup_failure(failure),
		}
	}

	fn destination(self) -> Option<Destination> {
		match self {
			Self::NoAccounts => Some(Destination::Accounts),
			Self::Service(_) => Some(Destination::Health),
			Self::Unavailable | Self::ClientStart => None,
		}
	}
}

async fn load_catalog(
	profile: ClientProfile,
	cursor: Option<String>,
) -> Result<NativeThreadPage, CatalogProblem> {
	if let Some(page) = AgentClient::new(profile.clone())
		.native_threads(cursor)
		.await
		.map_err(CatalogProblem::Service)?
	{
		return Ok(page);
	}
	if matches!(AccountClient::new(profile).list().await, Ok(AccountsResult::Available { accounts, .. }) if accounts.is_empty())
	{
		Err(CatalogProblem::NoAccounts)
	} else {
		Err(CatalogProblem::Unavailable)
	}
}

enum Mutation {
	Idle,
	Saving(IdempotencyKey),
	Saved(String),
	Failed(String),
}

impl AgentSurface {
	pub(super) fn open_thread_registration(
		&mut self,
		context: Option<&str>,
		cx: &mut Context<Self>,
	) {
		let (source, selected) = match context {
			None => (Source::Codex, None),
			Some(id) => match self.thread_registration_context(id) {
				Some(RegistrationContext::Agent) => (Source::Agent(id.to_owned()), None),
				Some(RegistrationContext::Conversation { .. }) => {
					let Some(work) = self.conversation_record(id) else { return };
					(Source::Conversation, self.bound_thread_candidate(work))
				},
				None => return,
			},
		};
		self.cancel_dictation(cx);
		self.clear_archive_selection();
		self.start_registration(source, selected, cx);
		cx.emit(CatalogEvent::Open(Catalog::Codex));
	}

	fn start_registration(
		&mut self,
		source: Source,
		selected: Option<NativeThreadCandidate>,
		cx: &mut Context<Self>,
	) {
		let browse = !matches!(source, Source::Conversation);
		let search = super::resource_field("Search conversations…", "Search conversations", cx);
		cx.observe(&search, |_, _, cx| cx.notify()).detach();
		self.workspace.registration = Some(Panel {
			source,
			search,
			selected,
			choosing_agent: false,
			threads: Vec::new(),
			cursor: None,
			read: None,
			read_error: None,
			mutation: Mutation::Idle,
		});
		if browse {
			self.read_registration_page(cx);
		}
		cx.notify();
	}

	fn select_registration(
		&mut self,
		candidate: Option<NativeThreadCandidate>,
		cx: &mut Context<Self>,
	) {
		if let Some(panel) = self.workspace.registration.as_mut()
			&& !matches!(panel.mutation, Mutation::Saving(_) | Mutation::Saved(_))
		{
			panel.selected = candidate;
			panel.choosing_agent = false;
			panel.mutation = Mutation::Idle;
			cx.notify();
		}
	}

	fn bound_thread_candidate(&self, work: &AgentWorkItemDto) -> Option<NativeThreadCandidate> {
		Some(NativeThreadCandidate {
			thread_id: work.codex_thread_id.clone()?,
			title: self.conversation_label(work),
			preview: None,
		})
	}

	fn read_registration_page(&mut self, cx: &mut Context<Self>) {
		let Some(panel) = self.workspace.registration.as_mut() else { return };
		if panel.read.is_some() {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			panel.read_error = Some(CatalogProblem::Service(ClientFailure::ProtocolDisconnected));
			cx.notify();
			return;
		};
		panel.read_error = None;
		let cursor = panel.cursor.clone();
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| CatalogProblem::ClientStart)?;
			runtime.block_on(load_catalog(profile, cursor))
		});
		panel.read = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				let Some(panel) = s.workspace.registration.as_mut() else { return };
				panel.read = None;
				match result {
					Ok(page) => {
						panel.threads.extend(page.threads);
						panel.cursor = page.next_cursor;
					},
					Err(message) => panel.read_error = Some(message),
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn finish_thread_registration(
		&mut self,
		key: &IdempotencyKey,
		result: &Result<AgentCommandResponse, String>,
	) {
		let Some(panel) = self.workspace.registration.as_mut() else { return };
		if !matches!(&panel.mutation, Mutation::Saving(pending) if pending == key) {
			return;
		}
		panel.mutation = match result {
			Ok(AgentCommandResponse::Accepted { work_id }) =>
				Mutation::Saved(work_id.as_str().to_owned()),
			Ok(AgentCommandResponse::Rejected { error }) =>
				Mutation::Failed(super::command_rejection_message(error)),
			Ok(AgentCommandResponse::PotentiallyDispatched { .. }) => Mutation::Failed(
				"The change could not be confirmed. Check the current relationship.".into(),
			),
			Err(message) => Mutation::Failed(message.clone()),
		};
	}

	pub(super) fn open_registered_thread(&mut self, cx: &mut Context<Self>) {
		let Some(panel) = self.workspace.registration.as_ref() else { return };
		let Mutation::Saved(thread) = &panel.mutation else { return };
		let open = matches!(panel.source, Source::Codex);
		let Some(work) = self.snapshot.as_ref().and_then(|s| {
			s.work_items.iter().find(|w| w.codex_thread_id.as_deref() == Some(thread))
		}) else {
			return;
		};
		let id = work.id.clone();
		self.workspace.registration = None;
		cx.emit(CatalogEvent::Close);
		if open {
			self.workspace.graph_expanded = false;
			self.open_navigation_conversation(&id, cx);
		}
		cx.notify();
	}

	fn submit_registration(
		&mut self,
		thread: &str,
		relation: ThreadRegistration,
		cx: &mut Context<Self>,
	) {
		let profile = self.profile.clone().filter(|_| self.command_connection_ready());
		let Some(panel) = self.workspace.registration.as_mut() else { return };
		if matches!(panel.mutation, Mutation::Saving(_) | Mutation::Saved(_))
			|| panel.selected.as_ref().is_none_or(|c| c.thread_id != thread)
		{
			return;
		}
		let Ok(thread_id) = EntityId::new(thread) else {
			panel.mutation = Mutation::Failed("Invalid Codex conversation ID.".into());
			cx.notify();
			return;
		};
		let Some(profile) = profile else {
			panel.mutation = Mutation::Failed("Connect to the service and try again.".into());
			cx.notify();
			return;
		};
		let action = AgentActionDto::RegisterNativeThread { thread_id, relation };
		let key = IdempotencyKey::new(super::unique_command()).expect("generated command identity");
		panel.mutation = Mutation::Saving(key.clone());
		let request_profile = profile.clone();
		let request_key = key.clone();
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "Could not start the client.".to_owned())?;
			runtime
				.block_on(AgentClient::new(request_profile).execute(action, request_key))
				.map_err(|error| CatalogProblem::Service(error).message().to_owned())
		});
		cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.profile.as_ref() == Some(&profile) {
					s.finish_thread_registration(&key, &result);
					s.refresh(cx);
					cx.notify();
				}
			});
		})
		.detach();
		cx.notify();
	}

	pub(super) fn adopt_conversation_entry(
		&self,
		selector: String,
		id: &str,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		if !matches!(self.thread_registration_context(id), Some(RegistrationContext::Agent)) {
			return None;
		}
		let work = id.to_owned();
		Some(self.conversation_action_button(
			selector,
			"Adopt conversation…",
			"Adopt conversation",
			cx,
			move |s, _, cx| {
				s.open_thread_registration(Some(&work), cx);
			},
		))
	}

	pub(super) fn can_register_relationship(work: &AgentWorkItemDto) -> bool {
		work.kind == AgentWorkKindDto::Goal
			&& work.parent_goal_id.is_none()
			&& work.codex_thread_id.is_some()
	}

	pub(super) fn thread_registration_context(&self, id: &str) -> Option<RegistrationContext<'_>> {
		let work = self.snapshot.as_ref()?.work_items.iter().find(|work| work.id == id)?;
		self.work_registration_context(work)
	}

	fn work_registration_context(
		&self,
		work: &AgentWorkItemDto,
	) -> Option<RegistrationContext<'_>> {
		let snapshot = self.snapshot.as_ref()?;
		if !self.registration_id_available(&work.id) {
			return None;
		}
		if snapshot.agents.iter().any(|agent| agent.id == work.id) {
			return Some(RegistrationContext::Agent);
		}
		Self::can_register_relationship(work).then(|| RegistrationContext::Conversation {
			owner: snapshot
				.agents
				.iter()
				.find(|agent| agent.managed_conversation_ids.contains(&work.id)),
		})
	}

	fn registration_id_available(&self, id: &str) -> bool {
		self.snapshot.as_ref().is_some_and(|s| {
			!s.archived_work_ids.contains(id)
				&& (s.assistant.agent_id.as_deref() == Some(id)
					|| !self.is_assistant_conversation(id))
		})
	}

	pub(super) fn render_thread_registration(
		&mut self,
		window: &gpui::Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		if self.workspace.registration.is_none() {
			self.start_registration(Source::Codex, None, cx);
		}
		let panel = self.workspace.registration.as_ref().expect("initialized catalog");
		let selected = panel.selected.as_ref();
		let browsing = !matches!(panel.source, Source::Conversation);
		let wide = browsing && f32::from(window.viewport_size().width) >= 850.;
		let mut body = gpui::div()
			.id("thread-registration")
			.debug_selector(|| "thread-registration".into())
			.size_full()
			.min_w_0()
			.min_h_0()
			.flex()
			.gap_3()
			.p_3();
		if browsing && (wide || selected.is_none()) {
			let list = gpui::div()
				.h_full()
				.min_w_0()
				.min_h_0()
				.flex()
				.flex_col()
				.when(wide && selected.is_some(), |list| list.w(gpui::relative(0.42)))
				.when(selected.is_none(), |list| list.flex_1())
				.child(self.registration_choices(selected.is_none(), cx));
			body = body.child(list);
		}
		if let Some(selected) = selected {
			let detail = gpui::div()
				.h_full()
				.min_w_0()
				.min_h_0()
				.flex_1()
				.flex()
				.flex_col()
				.gap_2()
				.when(browsing && !wide, |detail| {
					detail.child(gpui::div().pr(gpui::px(36.)).flex().child(
						self.conversation_action_button(
							"registration-back".into(),
							"Conversations",
							"Back to conversations",
							cx,
							|s, window, cx| {
								let Some(panel) = s.workspace.registration.as_ref() else { return };
								let focus = panel.search.read(cx).focus_handle(cx);
								s.select_registration(None, cx);
								window.on_next_frame(move |window, cx| window.focus(&focus, cx));
							},
						),
					))
				})
				.child(self.registration_actions(selected, wide, cx));
			body = body.child(detail);
		}
		body.into_any_element()
	}

	fn registration_choices(&self, full_width: bool, cx: &mut Context<Self>) -> AnyElement {
		let panel = self.workspace.registration.as_ref().expect("open registration");
		let query = panel.search.read(cx).content().trim().to_lowercase();
		let mut rows = gpui::div()
			.id("registration-candidates")
			.debug_selector(|| "registration-candidates".into())
			.min_h_0()
			.flex_1()
			.overflow_y_scroll()
			.min_w_0()
			.flex()
			.flex_col();
		let mut seen = HashSet::new();
		let mut count = 0;
		let snapshot = self.snapshot.as_ref();
		// Suppress every bound thread in the upstream list, including ineligible roles.
		let local = snapshot
			.into_iter()
			.flat_map(|s| &s.work_items)
			.filter_map(|work| {
				let thread = work.codex_thread_id.as_ref()?;
				seen.insert(thread.clone());
				(matches!(&panel.source, Source::Agent(_))
					&& matches!(
						self.work_registration_context(work),
						Some(RegistrationContext::Conversation { owner: None })
					))
				.then(|| self.bound_thread_candidate(work))
				.flatten()
			})
			.collect::<Vec<_>>();
		let candidates = local
			.into_iter()
			.chain(panel.threads.iter().filter(|c| seen.insert(c.thread_id.clone())).cloned());
		for candidate in candidates.filter(|c| {
			query.is_empty()
				|| c.title.to_lowercase().contains(&query)
				|| c.thread_id.to_lowercase().contains(&query)
				|| c.preview.as_ref().is_some_and(|p| p.to_lowercase().contains(&query))
		}) {
			count += 1;
			let row_id = format!("choose-thread-{}", candidate.thread_id);
			let title = candidate.title.clone();
			let selected =
				panel.selected.as_ref().is_some_and(|c| c.thread_id == candidate.thread_id);
			let keyboard = candidate.clone();
			rows = rows.child(
				gpui::div()
					.id(gpui::SharedString::from(row_id.clone()))
					.debug_selector(move || row_id)
					.role(Role::Button)
					.tab_index(0)
					.w_full()
					.min_w_0()
					.rounded(gpui::px(ui_theme::SMALL_CONTROL_RADIUS))
					.text_color(ui_theme::rgb(ui_theme::TEXT))
					.cursor_pointer()
					.aria_label(title.clone())
					.aria_selected(selected)
					.flex_none()
					.h(gpui::px(ui_theme::CONTROL_SIZE))
					.px_0()
					.py_0()
					.flex()
					.items_center()
					.map(|row| super::navigation_sidebar::conversation_row_feedback(row, selected))
					.child(
						gpui::div()
							.w(gpui::px(ui_theme::NAVIGATION_LEADING_SIZE))
							.flex_none()
							.flex()
							.items_center()
							.justify_center()
							.child(workspace_symbols::icon_sized(
								Symbol::Document,
								ui_theme::NAVIGATION_ICON_SIZE,
							)),
					)
					.child(
						gpui::div()
							.min_w_0()
							.flex_1()
							.truncate()
							.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
							.child(title),
					)
					.on_click(cx.listener(move |s, _, _, cx| {
						s.select_registration(Some(candidate.clone()), cx)
					}))
					.on_key_down(cx.listener(move |s, key: &KeyDownEvent, _, cx| {
						if !key.is_held && ["enter", "space"].contains(&key.keystroke.key.as_str())
						{
							cx.stop_propagation();
							s.select_registration(Some(keyboard.clone()), cx);
						}
					})),
			);
		}
		if count == 0 && panel.read.is_none() && panel.read_error.is_none() {
			rows = rows.child(if query.is_empty() {
				"No conversations available."
			} else {
				"No conversations match this search."
			});
		}
		let mut view = gpui::div().size_full().min_w_0().min_h_0().flex().flex_col().gap_2().child(
			gpui::div()
				.h(gpui::px(ui_theme::CONTROL_SIZE))
				.flex_none()
				.when(full_width, |search| search.pr(gpui::px(36.)))
				.child(panel.search.clone()),
		);
		if let Source::Agent(id) = &panel.source
			&& let Some(agent) = snapshot.and_then(|s| s.agents.iter().find(|a| &a.id == id))
		{
			let label = self.agent_conversation_label(agent);
			view = view
				.child(gpui::div().truncate().child(format!("Choose a conversation for {label}.")));
		}
		view = view.child(rows);
		if let Some(error) = panel.read_error {
			view = view.child(error.message());
			if let Some(destination) = error.destination() {
				let (id, label) = if destination == Destination::Accounts {
					("registration-open-accounts", "Open Accounts")
				} else {
					("registration-open-diagnostics", "Open Diagnostics")
				};
				view = view.child(self.conversation_action_button(
					id.into(),
					label,
					label,
					cx,
					move |_, _, cx| cx.emit(CatalogEvent::OpenSettings(destination)),
				));
			}
		}
		if panel.read.is_some() {
			view = view.child("Loading Codex conversations…");
		} else if panel.read_error.is_some() || panel.cursor.is_some() {
			let label = if panel.read_error.is_some() { "Retry" } else { "Load more" };
			view = view.child(self.conversation_action_button(
				"registration-load".into(),
				label,
				label,
				cx,
				|s, _, cx| s.read_registration_page(cx),
			));
		}
		view.into_any_element()
	}

	fn registration_actions(
		&self,
		candidate: &NativeThreadCandidate,
		wide: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let panel = self.workspace.registration.as_ref().expect("open registration");
		let title = if candidate.title.trim().is_empty() {
			"Untitled conversation"
		} else {
			candidate.title.trim()
		};
		let mut view = gpui::div()
			.id("registration-actions")
			.debug_selector(|| "registration-actions".into())
			.min_h_0()
			.min_w_0()
			.flex_1()
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap_3()
			.when(wide, |detail| {
				detail.border_l_1().border_color(ui_theme::rgba(ui_theme::PANEL_DIVIDER)).pl_3()
			})
			.child(
				gpui::div()
					.w_full()
					.min_w_0()
					.min_h(gpui::px(ui_theme::CONTROL_SIZE))
					.flex_none()
					.flex()
					.items_center()
					.gap_2()
					.when(wide || matches!(panel.source, Source::Conversation), |header| {
						header.pr(gpui::px(36.))
					})
					.child(
						gpui::div()
							.id("conversation-title")
							.debug_selector(|| "registration-title".into())
							.role(Role::Heading)
							.aria_label(title.to_owned())
							.flex_1()
							.min_w_0()
							.line_clamp(2)
							.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
							.font_weight(gpui::FontWeight::SEMIBOLD)
							.child(title.to_owned()),
					)
					.child(super::markdown::copy_button(
						"registration-copy-id",
						"Copy thread ID",
						candidate.thread_id.clone(),
					)),
			)
			.children(candidate.preview.as_ref().filter(|text| text.trim() != title).map(
				|preview| {
					gpui::div()
						.id("registration-preview")
						.debug_selector(|| "registration-preview".into())
						.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
						.line_clamp(3)
						.hover_hint(preview.clone())
						.child(preview.clone())
				},
			));
		match &panel.mutation {
			Mutation::Idle => {},
			Mutation::Failed(message) => view = view.child(message.clone()),
			Mutation::Saving(_) | Mutation::Saved(_) =>
				return view.child("Updating conversation…").into_any_element(),
		}
		let Some(snapshot) = self.snapshot.as_ref() else {
			return view
				.child("Connect to the service to edit this conversation.")
				.into_any_element();
		};
		let local = snapshot
			.work_items
			.iter()
			.find(|work| work.codex_thread_id.as_deref() == Some(&candidate.thread_id));
		let owner = match (&panel.source, local) {
			(Source::Codex, Some(_)) =>
				return view.child("This conversation is already in Decodex.").into_any_element(),
			(Source::Conversation, None) =>
				return view.child("This conversation is no longer available.").into_any_element(),
			(_, Some(work)) => match self.work_registration_context(work) {
				Some(RegistrationContext::Conversation { owner }) => owner,
				_ =>
					return view
						.child("This conversation is no longer available.")
						.into_any_element(),
			},
			(_, None) => None,
		};
		let expected_owner = owner.map(|a| a.id.clone());
		let action = |id: String,
		              label: String,
		              primary: bool,
		              relation: ThreadRegistration,
		              cx: &mut Context<Self>| {
			let thread = candidate.thread_id.clone();
			registration_button(format!("registration-{id}"), label, primary, cx, move |s, cx| {
				s.submit_registration(&thread, relation.clone(), cx)
			})
		};
		let assign = |agent: &Agent, primary: bool, cx: &mut Context<Self>| {
			let name = self.agent_conversation_label(agent);
			action(
				format!("assign-{}", agent.id),
				if owner.is_some() {
					format!("Transfer to {name}")
				} else {
					format!("Assign to {name}")
				},
				primary,
				ThreadRegistration::Assign {
					agent_id: agent.id.clone(),
					expected_owner: expected_owner.clone(),
				},
				cx,
			)
		};
		if let Source::Agent(id) = &panel.source {
			if owner.is_some() {
				return view
					.child("Another Agent has adopted this conversation.")
					.into_any_element();
			}
			return match snapshot
				.agents
				.iter()
				.find(|a| &a.id == id && self.registration_id_available(id))
			{
				Some(agent) => view.child(assign(agent, true, cx)),
				None => view.child("This Agent is no longer available."),
			}
			.into_any_element();
		}

		let mut actions =
			gpui::div().min_w_0().flex_none().flex().flex_wrap().items_center().gap_2();
		if local.is_none() {
			actions = actions.child(action(
				"register".into(),
				"Add and open".into(),
				true,
				ThreadRegistration::Register,
				cx,
			));
		}
		if let Some(owner) = owner {
			view = view.child(format!("Managed by {}", self.agent_conversation_label(owner)));
		}
		let mut targets = snapshot
			.agents
			.iter()
			.filter(|agent| {
				Some(&agent.id) != expected_owner.as_ref()
					&& self.registration_id_available(&agent.id)
			})
			.peekable();
		let has_targets = targets.peek().is_some();
		if has_targets {
			let label = if owner.is_some() { "Transfer to Agent…" } else { "Assign to Agent…" };
			actions = actions.child(registration_button(
				"registration-assignment".into(),
				label.into(),
				false,
				cx,
				|s, cx| {
					if let Some(panel) = s.workspace.registration.as_mut() {
						panel.choosing_agent = !panel.choosing_agent;
						cx.notify();
					}
				},
			));
		}
		actions = actions.child(action(
			"promote".into(),
			"Make an independent Agent".into(),
			false,
			ThreadRegistration::Promote { expected_owner: expected_owner.clone() },
			cx,
		));
		if let Some(owner) = owner {
			actions = actions.child(action(
				"release".into(),
				format!("Unassign from {}", self.agent_conversation_label(owner)),
				false,
				ThreadRegistration::Release { expected_owner: owner.id.clone() },
				cx,
			));
		}
		view = view.child(actions);
		if has_targets && panel.choosing_agent {
			view = view.child(
				gpui::div()
					.flex_none()
					.flex()
					.flex_wrap()
					.gap_2()
					.children(targets.map(|a| assign(a, false, cx))),
			);
		}

		view.into_any_element()
	}
}

fn registration_button(
	id: String,
	label: String,
	primary: bool,
	cx: &mut Context<AgentSurface>,
	action: impl Fn(&mut AgentSurface, &mut Context<AgentSurface>) + 'static,
) -> AnyElement {
	let button = if primary {
		ui_theme::settings_primary_button(id, label.clone())
	} else {
		ui_theme::settings_button(id, label.clone())
	};
	let action = Rc::new(action);
	let click = action.clone();
	button
		.max_w_full()
		.min_w_0()
		.justify_start()
		.truncate()
		.aria_label(label.clone())
		.hover_hint(label)
		.on_click(cx.listener(move |s, _, _, cx| {
			cx.stop_propagation();
			click(s, cx);
		}))
		.on_key_down(cx.listener(move |s, key: &KeyDownEvent, _, cx| {
			if !key.is_held && ["enter", "space"].contains(&key.keystroke.key.as_str()) {
				cx.stop_propagation();
				action(s, cx);
			}
		}))
		.into_any_element()
}

#[cfg(test)]
mod tests {
	use super::{AgentSurface, Mutation};
	use decodex_protocol::{AgentCommandResponse, AgentSnapshotResult, EntityId, IdempotencyKey};
	use gpui::{AppContext as _, Focusable as _};

	fn candidate(id: &str) -> super::NativeThreadCandidate {
		super::NativeThreadCandidate { thread_id: id.into(), title: id.into(), preview: None }
	}

	#[tokio::test]
	async fn thread_registration_catalog_checks_accounts_only_after_an_unavailable_reply() {
		use super::{CatalogProblem, load_catalog};
		use crate::shell::agent_surface::wire_test_support;
		use decodex_protocol::{
			AccountDto, AccountLifecycleReadinessDto, AccountObservedStateDto,
			AccountQuotaStateDto, AccountQuotaWindowDto, AccountsResult, ClientFailure,
			ClientMessage, EntityRevision, NativeThreadPage, QueryPayload, QueryResultPayload,
			WireText,
		};
		use futures_util::StreamExt as _;
		use tokio_tungstenite::tungstenite::Message;

		let empty = NativeThreadPage { threads: vec![], next_cursor: None };
		let quota = |duration_minutes| AccountQuotaWindowDto {
			duration_minutes,
			observed_at_unix_micros: None,
			result: AccountQuotaStateDto::Unknown,
		};
		let account = AccountDto {
			account_id: EntityId::new("10000000-0000-4000-8000-000000000001").unwrap(),
			alias: WireText::new("Primary").unwrap(),
			enabled: true,
			account_revision: EntityRevision(1),
			observed_state: AccountObservedStateDto::Unknown,
			lifecycle_readiness: AccountLifecycleReadinessDto::CredentialAbsent,
			credential_binding: None,
			unsettled_operation: None,
			five_hour_quota: quota(300),
			seven_day_quota: quota(10_080),
		};
		for (page, accounts, expected) in [
			(Some(empty.clone()), None, Ok(empty)),
			(
				None,
				Some(AccountsResult::Available { accounts: vec![], routing: None }),
				Err(CatalogProblem::NoAccounts),
			),
			(
				None,
				Some(AccountsResult::Available { accounts: vec![account], routing: None }),
				Err(CatalogProblem::Unavailable),
			),
			(None, Some(AccountsResult::Unavailable), Err(CatalogProblem::Unavailable)),
		] {
			let (_home, profile, server) = wire_test_support::fixture(|listener| async move {
				let mut replies = vec![(
					QueryPayload::ListNativeThreads { cursor: None },
					QueryResultPayload::NativeThreads(page),
				)];
				if let Some(accounts) = accounts {
					replies
						.push((QueryPayload::ListAccounts, QueryResultPayload::Accounts(accounts)));
				}
				for (payload, reply) in replies {
					let mut socket = wire_test_support::accept(&listener).await;
					let Message::Text(raw) = socket.next().await.unwrap().unwrap() else {
						panic!("text query")
					};
					let ClientMessage::Query(query) = serde_json::from_str(&raw).unwrap() else {
						panic!("catalog query")
					};
					assert_eq!(query.payload, payload);
					wire_test_support::answer_query(&mut socket, query.query_id, reply).await;
				}
			});
			assert_eq!(load_catalog(profile, None).await, expected);
			server.join().unwrap();
		}
		let (_home, profile, stopped) = wire_test_support::fixture(|_| async {});
		stopped.join().unwrap();
		let error = load_catalog(profile, None).await.unwrap_err();
		assert_eq!(error, CatalogProblem::Service(ClientFailure::ProtocolDisconnected));
		assert_eq!(error.destination(), Some(super::Destination::Health));
	}

	#[gpui::test]
	fn thread_registration_recovery_actions_open_the_relevant_settings(
		cx: &mut gpui::TestAppContext,
	) {
		let (shell, visual) = crate::shell::tests::open_shell(cx);
		visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(920.)));
		for (error, selector, destination) in [
			(
				super::CatalogProblem::NoAccounts,
				"registration-open-accounts",
				super::Destination::Accounts,
			),
			(
				super::CatalogProblem::Service(super::ClientFailure::ProtocolDisconnected),
				"registration-open-diagnostics",
				super::Destination::Health,
			),
		] {
			shell.update(visual, |shell, cx| {
				shell.agent.update(cx, |s, cx| {
					s.visual_workspace_fixture(cx);
					s.open_thread_registration(None, cx);
					s.workspace.registration.as_mut().unwrap().read_error = Some(error);
					cx.notify();
				})
			});
			visual.update(crate::ui_frame::draw_current_frame);
			visual.executor().advance_clock(std::time::Duration::from_millis(250));
			visual.update(crate::ui_frame::draw_frame);
			let action = visual.debug_bounds(selector).expect("recovery action");
			visual.simulate_click(action.center(), Default::default());
			shell.read_with(visual, |shell, _| {
				assert_eq!(shell.settings_selected, destination);
				assert!(shell.settings_open());
			});
		}
	}

	#[gpui::test]
	fn thread_registration_empty_workspace_keeps_archive_and_current_conversation(
		cx: &mut gpui::TestAppContext,
	) {
		let (shell, visual) = crate::shell::tests::open_shell(cx);
		visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(920.)));
		shell.update(visual, |shell, cx| {
			shell.agent.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.visual_workspace_page("empty", cx);
				cx.notify();
			})
		});
		visual.update(crate::ui_frame::draw_frame);
		let catalog = visual.debug_bounds("codex-conversations").unwrap();
		let archive = visual.debug_bounds("archive-browser").unwrap();
		assert!(catalog.bottom() <= archive.top());
		visual.simulate_click(
			gpui::point(catalog.left() + gpui::px(12.), catalog.center().y),
			Default::default(),
		);
		visual.update(crate::ui_frame::draw_frame);
		visual.executor().advance_clock(std::time::Duration::from_millis(250));
		visual.update(crate::ui_frame::draw_frame);
		let back = visual.debug_bounds("workspace-overlay-close").unwrap();
		let search = visual.debug_bounds("field-input-text").expect("catalog search field");
		assert!(!back.intersects(&search), "the close control must leave the search field usable");
		assert!(visual.debug_bounds("registration-candidates").is_some());
		shell.read_with(visual, |shell, cx| {
			assert_eq!(
				shell.agent.read(cx).navigation_location(),
				crate::shell::agent_surface::AgentLocation::Root
			)
		});
		visual.simulate_click(
			gpui::point(back.left() + gpui::px(12.), back.center().y),
			Default::default(),
		);
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("workspace-overlay-close").is_none());
		assert!(visual.debug_bounds("archive-browser").is_some());
	}

	#[gpui::test]
	fn thread_registration_selection_and_dismissal_preserve_conversation_history(
		cx: &mut gpui::TestAppContext,
	) {
		let (shell, visual) = crate::shell::tests::open_shell(cx);
		visual.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(920.)));
		shell.update(visual, |shell, cx| {
			shell.agent.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.workspace.graph_visible = false;
				s.snapshot.as_mut().unwrap().work_items[0].codex_thread_id =
					Some("already-in-decodex".into());
				cx.notify();
			})
		});
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		let catalog = visual.debug_bounds("codex-conversations").unwrap();
		visual.simulate_click(catalog.center(), Default::default());
		shell.update(visual, |shell, cx| {
			shell.agent.update(cx, |s, cx| {
				let panel = s.workspace.registration.as_mut().unwrap();
				panel.threads = ["already-in-decodex", "release-handoff"].map(candidate).to_vec();
				panel.read_error = None;
				cx.notify();
			})
		});
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("choose-thread-already-in-decodex").is_none());
		let conversation = visual
			.debug_bounds("choose-thread-release-handoff")
			.expect("conversation in the catalog");
		visual.simulate_click(conversation.center(), Default::default());
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		assert!(
			visual.debug_bounds("registration-register").is_some(),
			"expected detail at {:?}",
			shell.read_with(visual, |shell, cx| shell.agent.read(cx).navigation_location())
		);
		assert!(visual.debug_bounds("choose-thread-release-handoff").is_some());
		super::super::navigation_sidebar::check_catalog_selection_paint(
			visual,
			"choose-thread-release-handoff",
		);
		visual.simulate_resize(gpui::size(gpui::px(700.), gpui::px(820.)));
		visual.update(crate::ui_frame::draw_frame);
		let back = visual
			.debug_bounds("registration-back")
			.expect("compact detail can return to the list");
		assert!(visual.debug_bounds("choose-thread-release-handoff").is_none());
		let close = visual.debug_bounds("workspace-overlay-close").unwrap();
		assert!(
			!back.intersects(&close),
			"the compact back and close controls have separate hit areas"
		);
		visual.simulate_click(back.center(), Default::default());
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("choose-thread-release-handoff").is_some());
		assert!(visual.debug_bounds("registration-register").is_none());
		visual.update(|window, cx| {
			let agent = shell.read(cx).agent.read(cx);
			let panel = agent.workspace.registration.as_ref().unwrap();
			assert!(
				panel.search.read(cx).focus_handle(cx).is_focused(window),
				"return focus to search"
			);
		});
		visual.simulate_keystrokes("escape");
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		assert!(
			visual.debug_bounds("thread-registration").is_none(),
			"Escape must dismiss the catalog. Active overlay: {:?}",
			shell.read_with(visual, |shell, _| shell.overlay)
		);
		shell.read_with(visual, |shell, cx| {
			assert_eq!(
				shell.agent.read(cx).navigation_location(),
				crate::shell::agent_surface::AgentLocation::Root
			)
		});
	}

	#[gpui::test]
	fn thread_registration_agent_only_adopts_unassigned_conversations(
		cx: &mut gpui::TestAppContext,
	) {
		let (shell, visual) = crate::shell::tests::open_shell(cx);
		visual.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(920.)));
		shell.update(visual, |shell, cx| {
			shell.agent.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				let snapshot = s.snapshot.as_mut().unwrap();
				let template = snapshot.work_items[0].clone();
				for id in ["ordinary", "managed", "archived"] {
					let mut work = template.clone();
					work.id = id.into();
					work.codex_thread_id = Some(id.into());
					snapshot.work_items.push(work);
				}
				snapshot.work_items[0].codex_thread_id = Some("agent".into());
				snapshot.agents.push(super::Agent {
					id: "agent".into(),
					name: "Coordinator".into(),
					native_thread_id: Some("agent".into()),
					assistant: true,
					managed_conversation_ids: vec!["managed".into()],
					scheduled_wakes: Default::default(),
				});
				snapshot.assistant.agent_id = Some("agent".into());
				snapshot.archived_work_ids.insert("archived".into());
				s.open_thread_registration(Some("agent"), cx);
				let panel = s.workspace.registration.as_mut().unwrap();
				panel.read_error = None;
				panel.threads = ["external", "ordinary", "managed", "archived", "agent"]
					.map(candidate)
					.to_vec();
				cx.notify();
			})
		});
		visual.update(crate::ui_frame::draw_current_frame);
		for selector in ["choose-thread-managed", "choose-thread-archived", "choose-thread-agent"] {
			assert!(visual.debug_bounds(selector).is_none(), "{selector}");
		}
		for selector in ["choose-thread-external", "choose-thread-ordinary"] {
			let row = visual.debug_bounds(selector).expect(selector);
			visual.simulate_click(row.center(), Default::default());
			visual.update(crate::ui_frame::draw_current_frame);
			assert!(visual.debug_bounds("registration-assign-agent").is_some());
			for hidden in [
				"registration-register",
				"registration-promote",
				"registration-assignment",
				"registration-release",
			] {
				assert!(visual.debug_bounds(hidden).is_none(), "{hidden}");
			}
		}
		shell.update(visual, |shell, cx| {
			shell.agent.update(cx, |s, cx| {
				s.workspace.graph_expanded = true;
				s.composer.update(cx, |input, cx| input.set_content("Keep drafting", cx));
				let location = s.navigation_location();
				for source in [super::Source::Agent("agent".into()), super::Source::Conversation] {
					s.start_registration(source, Some(candidate("ordinary")), cx);
					s.workspace.registration.as_mut().unwrap().mutation =
						Mutation::Saved("ordinary".into());
					s.open_registered_thread(cx);
					assert!(s.workspace.registration.is_none() && s.workspace.graph_expanded);
					assert_eq!(s.navigation_location(), location);
					assert_eq!(s.composer.read(cx).content(), "Keep drafting");
				}
			})
		});
	}

	#[gpui::test]
	fn thread_registration_agent_picker_scrolls_to_the_last_choice(cx: &mut gpui::TestAppContext) {
		let (shell, visual) = crate::shell::tests::open_shell(cx);
		visual.simulate_resize(gpui::size(gpui::px(700.), gpui::px(560.)));
		shell.update(visual, |shell, cx| {
			shell.agent.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				let snapshot = s.snapshot.as_mut().unwrap();
				for index in 0..60 {
					snapshot.agents.push(super::Agent {
						id: format!("agent-{index}"),
						name: format!("Work coordinator {index}"),
						assistant: false,
						native_thread_id: None,
						managed_conversation_ids: vec![],
						scheduled_wakes: Default::default(),
					});
				}
				s.open_thread_registration(None, cx);
				let panel = s.workspace.registration.as_mut().unwrap();
				panel.threads = ["first", "second"].map(candidate).to_vec();
				panel.read_error = None;
				s.select_registration(Some(candidate("first")), cx);
			})
		});
		visual.update(crate::ui_frame::draw_frame);
		visual.executor().advance_clock(std::time::Duration::from_millis(250));
		visual.update(crate::ui_frame::draw_frame);
		let expand = visual.debug_bounds("registration-assignment").unwrap();
		visual.simulate_click(expand.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		let viewport = visual.debug_bounds("registration-actions").unwrap();
		let last = "registration-assign-agent-59";
		assert!(visual.debug_bounds(last).is_none_or(|b| !viewport.contains(&b.center())));
		visual.simulate_event(gpui::ScrollEvent {
			position: viewport.center(),
			delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-6000.))),
			..Default::default()
		});
		visual.update(crate::ui_frame::draw_frame);
		let choice = visual.debug_bounds(last).expect("last Agent reachable by scrolling");
		assert!(viewport.contains(&choice.center()));
		visual.simulate_click(choice.center(), Default::default());
		shell.update(visual, |shell, cx| {
			shell.agent.update(cx, |s, cx| {
				assert!(
					matches!(
						s.workspace.registration.as_ref().unwrap().mutation,
						Mutation::Failed(_)
					),
					"the visible Agent received the click"
				);
				s.select_registration(Some(candidate("second")), cx);
				s.submit_registration("first", super::ThreadRegistration::Register, cx);
				let panel = s.workspace.registration.as_ref().unwrap();
				assert!(
					!panel.choosing_agent && matches!(panel.mutation, Mutation::Idle),
					"changing conversations discards the previous picker and its actions"
				);
			})
		});
	}

	#[gpui::test]
	fn thread_registration_opens_the_existing_local_identity_after_receipt(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.open_page("agent", cx);
			s.start_registration(
				super::Source::Codex,
				Some(decodex_protocol::NativeThreadCandidate {
					thread_id: "native-existing".into(),
					title: "Existing conversation".into(),
					preview: None,
				}),
				cx,
			);
			let key = IdempotencyKey::new("adopt").unwrap();
			s.workspace.registration.as_mut().unwrap().mutation = Mutation::Saving(key.clone());
			s.finish_thread_registration(
				&key,
				&Ok(AgentCommandResponse::Accepted {
					work_id: EntityId::new("native-existing").unwrap(),
				}),
			);
			s.open_registered_thread(cx);
			assert_eq!(s.selected.as_deref(), Some("agent"));
			let mut snapshot = s.snapshot.clone().unwrap();
			let mut local = snapshot.work_items[0].clone();
			local.id = "conversation-existing".into();
			local.codex_thread_id = Some("native-existing".into());
			snapshot.work_items.push(local);
			s.apply_refreshed_snapshot(Ok(AgentSnapshotResult::Available(snapshot)), cx);
			assert!(s.workspace.registration.is_none());
			assert_eq!(s.selected.as_deref(), Some("conversation-existing"));
			assert_eq!(s.composer_manager.as_deref(), Some("conversation-existing"));
		});
	}
}
