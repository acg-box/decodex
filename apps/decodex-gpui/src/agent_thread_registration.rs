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
	work::ThreadRegistration,
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

pub(super) struct Panel {
	search: Entity<ComposerInput>,
	threads: Vec<NativeThreadCandidate>,
	cursor: Option<String>,
	read: Option<Task<()>>,
	read_error: Option<CatalogProblem>,
	selected: Option<NativeThreadCandidate>,
	preferred_agent: Option<String>,
	loaded: bool,
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
		let preferred_agent = context
			.and_then(|id| self.snapshot.as_ref()?.agents.iter().find(|a| a.id == id))
			.map(|a| a.id.clone());
		let selected = context.and_then(|id| {
			let work = self.conversation_record(id)?;
			(preferred_agent.is_none() && Self::can_register_relationship(work)).then_some(
				NativeThreadCandidate {
					thread_id: work.codex_thread_id.clone()?,
					title: self.conversation_label(work),
					preview: None,
				},
			)
		});
		self.cancel_dictation(cx);
		self.show_thread_registration(selected, preferred_agent, cx);
		cx.emit(CatalogEvent::Open(Catalog::Codex));
	}

	pub(super) fn show_thread_registration(
		&mut self,
		selected: Option<NativeThreadCandidate>,
		preferred_agent: Option<String>,
		cx: &mut Context<Self>,
	) {
		let panel = self.workspace.registration.get_or_insert_with(|| {
			let search = super::resource_field(
				"Search Codex conversations…",
				"Search Codex conversations",
				cx,
			);
			cx.observe(&search, |_, _, cx| cx.notify()).detach();
			Panel {
				search,
				threads: Vec::new(),
				cursor: None,
				read: None,
				read_error: None,
				selected: None,
				preferred_agent: None,
				loaded: false,
				mutation: Mutation::Idle,
			}
		});
		if panel.selected != selected || panel.preferred_agent != preferred_agent {
			panel.mutation = Mutation::Idle;
		}
		panel.selected = selected;
		panel.preferred_agent = preferred_agent;
		if !panel.loaded && panel.read_error.is_none() {
			self.read_registration_page(cx);
		}
		cx.notify();
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
						panel.loaded = true;
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
		let Some(work) = self.snapshot.as_ref().and_then(|s| {
			s.work_items.iter().find(|w| w.codex_thread_id.as_deref() == Some(thread))
		}) else {
			return;
		};
		let id = work.id.clone();
		self.workspace.graph_expanded = false;
		self.workspace.registration = None;
		cx.emit(CatalogEvent::Close);
		self.open_navigation_conversation(&id, cx);
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
		if matches!(panel.mutation, Mutation::Saving(_) | Mutation::Saved(_)) {
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

	pub(super) fn thread_registration_entry(
		&self,
		id: String,
		label: &'static str,
		work: Option<String>,
		cx: &mut Context<Self>,
	) -> AnyElement {
		self.conversation_action_button(id, label, label, cx, move |s, _, cx| {
			s.open_thread_registration(work.as_deref(), cx);
		})
	}

	pub(super) fn can_register_relationship(work: &AgentWorkItemDto) -> bool {
		work.kind == AgentWorkKindDto::Goal
			&& work.parent_goal_id.is_none()
			&& work.codex_thread_id.is_some()
	}

	pub(super) fn thread_registration_context(&self, id: &str) -> Option<&'static str> {
		let snapshot = self.snapshot.as_ref()?;
		if snapshot.archived_work_ids.contains(id) {
			return None;
		}
		if snapshot.agents.iter().any(|agent| agent.id == id) {
			return Some("Adopt conversation");
		}
		self.conversation_record(id)
			.filter(|w| Self::can_register_relationship(w))
			.map(|_| "Manage relationship")
	}

	pub(super) fn render_thread_registration(
		&mut self,
		window: &gpui::Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		if self.workspace.registration.is_none() {
			self.show_thread_registration(None, None, cx);
		}
		let panel = self.workspace.registration.as_ref().expect("initialized catalog");
		let selected = panel.selected.as_ref();
		let wide = f32::from(window.viewport_size().width) >= 850.;
		let mut body = gpui::div()
			.id("thread-registration")
			.debug_selector(|| "thread-registration".into())
			.size_full()
			.min_w_0()
			.min_h_0()
			.flex()
			.gap_3()
			.p_3();
		if wide || selected.is_none() {
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
				.when(!wide, |detail| {
					detail.child(gpui::div().pr(gpui::px(36.)).flex().child(
						self.conversation_action_button(
							"registration-back".into(),
							"Conversations",
							"Back to conversations",
							cx,
							|s, window, cx| {
								let Some(panel) = s.workspace.registration.as_ref() else { return };
								let focus = panel.search.read(cx).focus_handle(cx);
								s.show_thread_registration(None, panel.preferred_agent.clone(), cx);
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
		// Bound conversations remain manageable even if this page contains only new candidates.
		let bound = snapshot
			.into_iter()
			.flat_map(|s| &s.work_items)
			.filter_map(|w| {
				let thread_id = w.codex_thread_id.clone()?;
				seen.insert(thread_id.clone());
				(Self::can_register_relationship(w)
					&& snapshot.is_some_and(|s| {
						!s.archived_work_ids.contains(&w.id)
							&& !s.agents.iter().any(|a| a.id == w.id)
					}))
				.then(|| NativeThreadCandidate {
					thread_id,
					title: self.conversation_label(w),
					preview: None,
				})
			})
			.collect::<Vec<_>>();
		let candidates = bound
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
			let preferred = panel.preferred_agent.clone();
			let key_preferred = preferred.clone();
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
						s.show_thread_registration(Some(candidate.clone()), preferred.clone(), cx)
					}))
					.on_key_down(cx.listener(move |s, key: &KeyDownEvent, _, cx| {
						if !key.is_held && ["enter", "space"].contains(&key.keystroke.key.as_str())
						{
							cx.stop_propagation();
							s.show_thread_registration(
								Some(keyboard.clone()),
								key_preferred.clone(),
								cx,
							);
						}
					})),
			);
		}
		if count == 0 && panel.read.is_none() && panel.read_error.is_none() {
			rows = rows.child(if query.is_empty() {
				"No Codex conversations found."
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
		if let Some(agent) = panel
			.preferred_agent
			.as_ref()
			.and_then(|id| snapshot?.agents.iter().find(|a| &a.id == id))
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
					.when(wide, |header| header.pr(gpui::px(36.)))
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
		let agents = &snapshot.agents;
		let owner = local.and_then(|work| {
			agents.iter().find(|agent| agent.managed_conversation_ids.contains(&work.id))
		});
		let expected_owner = owner.map(|a| a.id.clone());
		let action = |id: String,
		              label: String,
		              description: String,
		              primary: bool,
		              relation: ThreadRegistration,
		              cx: &mut Context<Self>| {
			let thread = candidate.thread_id.clone();
			registration_button(
				format!("registration-{id}"),
				label,
				description,
				primary,
				cx,
				move |s, cx| s.submit_registration(&thread, relation.clone(), cx),
			)
		};
		let mut actions =
			gpui::div().min_w_0().flex_none().flex().flex_wrap().items_center().gap_2();
		if let Some(work) = local {
			let id = work.id.clone();
			actions = actions.child(registration_button(
				"registration-open".into(),
				"Open conversation".into(),
				"Open conversation".into(),
				true,
				cx,
				move |s, cx| {
					s.workspace.graph_expanded = false;
					cx.emit(CatalogEvent::Close);
					s.open_navigation_conversation(&id, cx);
				},
			));
			if agents.iter().any(|agent| agent.id == work.id) {
				return view.child(actions).into_any_element();
			}
		} else {
			actions = actions.child(action(
				"register".into(),
				"Add as conversation".into(),
				"Add as conversation".into(),
				true,
				ThreadRegistration::Register,
				cx,
			));
		}
		view = view.child(actions.child(action(
			"promote".into(),
			"Make an independent Agent".into(),
			"Make an independent Agent".into(),
			false,
			ThreadRegistration::Promote { expected_owner: expected_owner.clone() },
			cx,
		)));
		if let Some(owner) = owner {
			let name = self.agent_conversation_label(owner);
			view = view.child(
				gpui::div()
					.min_w_0()
					.flex_none()
					.flex()
					.items_center()
					.gap_2()
					.child(gpui::div().min_w_0().truncate().child(format!("Managed by {name}")))
					.child(action(
						"release".into(),
						"Release".into(),
						format!("Release from {name}"),
						false,
						ThreadRegistration::Release { expected_owner: owner.id.clone() },
						cx,
					)),
			);
		}
		let mut targets = agents
			.iter()
			.filter(|agent| {
				Some(&agent.id) != expected_owner.as_ref()
					&& !snapshot.archived_work_ids.contains(&agent.id)
			})
			.collect::<Vec<_>>();
		targets.sort_by_key(|agent| panel.preferred_agent.as_ref() != Some(&agent.id));
		if !targets.is_empty() {
			let mut choices = gpui::div().min_w_0().flex().flex_wrap().items_center().gap_2();
			for agent in targets {
				let name = self.agent_conversation_label(agent);
				choices = choices.child(action(
					format!("assign-{}", agent.id),
					name.clone(),
					format!("Assign to {name}"),
					false,
					ThreadRegistration::Assign {
						agent_id: agent.id.clone(),
						expected_owner: expected_owner.clone(),
					},
					cx,
				));
			}
			view = view.child(
				gpui::div()
					.min_w_0()
					.flex_none()
					.flex()
					.flex_col()
					.gap_2()
					.child(
						gpui::div()
							.id("registration-assignment")
							.debug_selector(|| "registration-assignment".into())
							.role(Role::Heading)
							.aria_label("Assign to an Agent")
							.text_size(gpui::px(ui_theme::CONTROL_LABEL_SIZE))
							.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
							.child("Assign to an Agent"),
					)
					.child(choices),
			);
		}
		view.into_any_element()
	}
}

fn registration_button(
	id: String,
	label: String,
	description: String,
	primary: bool,
	cx: &mut Context<AgentSurface>,
	action: impl Fn(&mut AgentSurface, &mut Context<AgentSurface>) + 'static,
) -> AnyElement {
	let button = if primary {
		ui_theme::settings_primary_button(id, label)
	} else {
		ui_theme::settings_button(id, label)
	};
	let action = Rc::new(action);
	let click = action.clone();
	button
		.max_w_full()
		.min_w_0()
		.justify_start()
		.truncate()
		.aria_label(description.clone())
		.hover_hint(description)
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
					Some("release-handoff".into());
				cx.notify();
			})
		});
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		let catalog = visual.debug_bounds("codex-conversations").unwrap();
		visual.simulate_click(catalog.center(), Default::default());
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);

		let conversation = visual
			.debug_bounds("choose-thread-release-handoff")
			.expect("conversation in the catalog");
		visual.simulate_click(conversation.center(), Default::default());
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		assert!(
			visual.debug_bounds("registration-open").is_some(),
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
		assert!(visual.debug_bounds("registration-open").is_none());
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
	fn thread_registration_opens_the_existing_local_identity_after_receipt(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.open_page("agent", cx);
			s.show_thread_registration(
				Some(decodex_protocol::NativeThreadCandidate {
					thread_id: "native-existing".into(),
					title: "Existing conversation".into(),
					preview: None,
				}),
				None,
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
