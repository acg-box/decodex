//! Conversation-first desktop presentation. All displayed work comes from the service.
use std::{mem, rc::Rc};

use gpui::{
	AnyElement, AppContext as _, Div, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
	PathBuilder, ScrollWheelEvent, Stateful,
};
use graph::{Layout, Node};
use ui_theme::{
	AGENT_CHAT_OVERLAY, AGENT_SIDEBAR_MATERIAL, AMBER, BLUE, BODY_LINE_HEIGHT, BODY_SIZE, CANVAS,
	CHROME_CONTROL_SIZE, CONTROL_MARGIN, FONT_FAMILY, HOVER_FILL, PANEL_HEADER_HEIGHT,
	SELECTED_HOVER_FILL, TEXT, TEXT_MUTED, TREE_ROW_HEIGHT,
};

#[cfg(test)] use crate::shell::agent_surface::ConversationWorkingDirectory;
use crate::{
	shell::{
		WINDOW_CONTROLS_CLEARANCE,
		agent_surface::{
			AgentDispatchStateDto, AgentHistoryResult, AgentRequestResult, AgentSnapshotDto,
			AgentSnapshotResult, AgentSurface, AgentWorkItemDto, AgentWorkStatusDto, Context,
			ConversationReasoningEffort, FluentBuilder, FontWeight, InteractiveElement,
			IntoElement, LoadState, ParentElement, Render, Role, SharedString,
			StatefulInteractiveElement, Styled, SubmitComposer, Window,
			activity::HistoryScrollAnchor, creation_setup, graph, prompts, ui_theme,
			workspace_size::Panel,
		},
		workspace_symbols,
	},
	ui_loading,
	ui_motion::{self, SmoothControl, TabReveal},
	ui_scroll::SmoothScrollArea,
};
#[cfg(any(test, feature = "visual-capture"))]
use decodex_protocol::{
	AgentDependencyDto,
	AgentDispatchStateDto::{Idle, Running},
	AgentWorkStatusDto::{Open, Resolved, UserDecision},
};
use decodex_protocol::{AgentWorkKindDto, DesktopRecoveredDraft};

const THREAD_LOCKED_MESSAGE: &str = "In use by another app";

#[derive(Clone)]
pub(super) struct PageView {
	scope: Option<String>,
	selected: Option<String>,
	pan: (f32, f32),
	zoom: f32,
	graph_visible: bool,
}

struct PanelTip(String);
impl Render for PanelTip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.px_2()
			.py_1()
			.rounded(gpui::px(5.0))
			.bg(gpui::rgb(CANVAS))
			.text_color(gpui::rgb(TEXT))
			.text_size(gpui::px(11.0))
			.child(self.0.clone())
	}
}

impl AgentSurface {
	pub(crate) fn navigation_work(&self) -> Option<String> {
		self.conversation_page().filter(|id| Some(id) != self.root_id().as_ref())
	}

	pub(crate) fn can_restore_work(&self, work: Option<&str>) -> bool {
		work.is_none_or(|id| {
			let id = self.native_agents.pages.get(id).map_or(id, |(owner, _)| owner.as_str());
			self.snapshot.as_ref().is_some_and(|s| s.work_items.iter().any(|w| w.id == id))
		})
	}

	pub(crate) fn restore_work(&mut self, work: Option<&str>, cx: &mut Context<Self>) {
		if let Some(id) = work.map(str::to_owned).or_else(|| self.root_id()) {
			self.open_page(&id, cx);
		}
	}

	pub(crate) fn panel_glyph(index: usize) -> AnyElement {
		panel_icon(["workspace-sidebar", "workspace-graph", "workspace-agents"][index])
			.expect("known panel glyph")
	}

	pub(crate) fn workspace_panels(&self) -> [(bool, bool); 3] {
		[
			(self.workspace.sidebar_visible, true),
			(self.workspace.graph_visible && self.reserve_workspace_panels(), self.has_work()),
			(self.workspace.agent_tree_visible, self.has_work()),
		]
	}

	pub(crate) fn toggle_workspace_sidebar(&mut self, cx: &mut Context<Self>) {
		self.workspace.sidebar_visible = !self.workspace.sidebar_visible;
		self.workspace.sidebar_peek = false;
		self.workspace.sidebar_leave = None;

		cx.notify();
	}

	pub(crate) fn toggle_workspace_graph(&mut self, cx: &mut Context<Self>) {
		self.workspace.graph_visible = !self.workspace.graph_visible;
		self.workspace.graph_expanded = false;

		cx.notify();
	}

	// Unknown data must not collapse panels that the workspace intends to show.
	pub(super) fn reserve_workspace_panels(&self) -> bool {
		self.snapshot.is_none() || self.has_work()
	}

	pub(super) fn has_work(&self) -> bool {
		self.snapshot
			.as_ref()
			.is_some_and(|s| s.work_items.iter().any(|w| w.parent_goal_id.is_some()))
			|| self.native_agents.lists.values().any(|agents| !agents.is_empty())
	}

	pub(super) fn root_id(&self) -> Option<String> {
		self.snapshot
			.as_ref()?
			.work_items
			.iter()
			.find(|w| w.parent_goal_id.is_none())
			.map(|w| w.id.clone())
	}

	pub(super) fn open_page(&mut self, id: &str, cx: &mut Context<Self>) {
		self.workspace.browsing = false;
		self.workspace.closing_pages.remove(id);
		if let Some((owner, thread)) = self.native_agents.pages.get(id).cloned() {
			self.open_native_agent(&owner, &thread, cx);
			return;
		}
		self.close_native_agent(cx);

		if !self.snapshot.as_ref().is_some_and(|s| s.work_items.iter().any(|w| w.id == id)) {
			return;
		}
		if self.selected.as_deref() != Some(id) {
			// Cancel against the outgoing editor before parking or replacing its draft.
			self.stop_voice(cx);
			self.reset_automatic_recap();
			self.reset_voice_settings();
			self.reset_resources();
			self.clear_usage_estimate();
			self.reset_integrations();
			self.resource_feedback.clear();
		}

		self.restore_manager_composer(id, cx);

		if let Some((old, history)) = &self.history {
			self.timeline.cache.insert(old.clone(), history.clone());
		}

		if self.root_id().as_deref() != Some(id) && !self.workspace.pages.iter().any(|p| p == id) {
			self.workspace.pages.push(id.to_owned());
		}
		if self.selected.as_deref() != Some(id) {
			if let Some(old) = &self.selected {
				self.workspace.page_views.insert(
					old.clone(),
					PageView {
						scope: self.workspace.graph_scope.clone(),
						selected: self.workspace.graph_selected.clone(),
						pan: self.workspace.graph_pan,
						zoom: self.workspace.graph_zoom,
						graph_visible: self.workspace.graph_visible,
					},
				);
			}

			let saved = self.workspace.page_views.get(id).cloned().unwrap_or_else(|| PageView {
				scope: self
					.snapshot
					.as_ref()
					.and_then(|snap| snap.work_items.iter().find(|w| w.id == id))
					.and_then(|w| {
						if w.kind == AgentWorkKindDto::Manager {
							Some(w.id.clone())
						} else {
							w.parent_goal_id.clone()
						}
					}),
				selected: Some(id.to_owned()),
				pan: (0.0, 0.0),
				zoom: 0.85,
				graph_visible: self.workspace.graph_visible,
			});

			self.workspace.graph_scope = saved.scope;
			self.workspace.graph_selected = saved.selected;
			self.workspace.graph_pan = saved.pan;
			self.workspace.graph_zoom = saved.zoom;
			self.workspace.graph_visible = saved.graph_visible;
			self.workspace.graph_expanded = false;
		}

		self.reset_model_settings();
		self.reset_live_reviewer();
		self.reset_permission_profiles();
		self.reset_task_models();
		self.reset_hook_settings();
		self.reset_native_goal();
		self.clear_activity_detail();
		self.reset_recap();
		self.reset_prompt_edit();

		self.selected = Some(id.to_owned());
		self.workspace.connection_details_expanded = false;
		self.history = self.timeline.cache.get(id).cloned().map(|h| (id.to_owned(), h));
		self.workspace.details_visible = false;
		self.request = None;
		self.request_task = None;

		let task_thread = self
			.snapshot
			.as_ref()
			.and_then(|s| s.work_items.iter().find(|w| w.id == id))
			.filter(|w| w.kind == AgentWorkKindDto::Task)
			.and_then(|w| w.codex_thread_id.clone());
		if let Some(thread) = task_thread {
			self.enter_native_conversation(id, &thread, cx);
		} else {
			self.load_history(cx);
			self.sync_request(cx);
		}
		cx.notify();
	}

	pub(super) fn restore_manager_composer(&mut self, id: &str, cx: &mut Context<Self>) {
		let is_manager = self.workspace.new_conversation.as_deref() == Some(id)
			|| self.snapshot.as_ref().is_some_and(|snapshot| {
				snapshot.work_items.iter().any(|work| {
					work.id == id
						&& (work.parent_goal_id.is_none() || work.kind == AgentWorkKindDto::Manager)
				})
			});

		if is_manager {
			let previous = self.composer_manager.clone().or_else(|| self.root_id());

			if previous.as_deref() != Some(id) {
				if let Some(previous) = previous {
					self.draft_profiles
						.tasks
						.insert(previous.clone(), mem::take(&mut self.task_references));
					self.draft_profiles
						.files
						.insert(previous.clone(), mem::take(&mut self.attachments));
					self.draft_profiles
						.texts
						.insert(previous, self.composer.read(cx).content().into());
				}

				self.attachments = self.draft_profiles.files.remove(id).unwrap_or_default();
				self.task_references = self.draft_profiles.tasks.remove(id).unwrap_or_default();
				self.composer_menu = None;

				let draft = self.draft_profiles.texts.get(id).cloned().unwrap_or_default();

				self.composer.update(cx, |input, cx| {
					input.set_content(&draft, cx);
					input.set_placeholder(prompts::next(), cx);
				});

				Self::refresh_prompt(cx);
			}

			self.composer_manager = Some(id.into());
		}
	}

	fn close_page(&mut self, id: &str, cx: &mut Context<Self>) {
		self.workspace.closing_pages.insert(id.to_owned());

		if self.conversation_page().as_deref() == Some(id)
			&& let Some(root) = self.root_id()
		{
			self.open_page(&root, cx);
		}

		cx.notify();
	}

	pub(super) fn workspace_action(
		&self,
		id: String,
		label: String,
		action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let is_tab = id.starts_with("page-");
		let is_tree = id.starts_with("agent-open-") || id.starts_with("native-agent-open-");
		let is_event = id.starts_with("event-");
		let is_prompt = id.starts_with("prompt-")
			|| id.starts_with("saved-prompt-")
			|| id.starts_with("discard-prompt-");
		let active = if is_tab {
			self.conversation_page().as_deref() == id.strip_prefix("page-")
		} else if let Some(work) = id.strip_prefix("sidebar-") {
			self.selected.as_deref() == Some(work)
		} else if id == "agent-home" {
			self.selected == self.root_id()
		} else {
			false
		};
		let accessible = if id.starts_with("attention-") {
			format!("{label} pending decisions · open next")
		} else {
			match id.as_str() {
				"new-project" => "New project",
				"graph-up" => "Parent work scope",
				"graph-close" => "Close graph",
				"zoom-in" => "Zoom in",
				"zoom-out" => "Zoom out",
				"inspect-work" => "Work details",
				_ => label.as_str(),
			}
			.to_owned()
		};
		let icon = panel_icon(&id);
		let icon_only = icon.is_some();
		let show_tip = is_tab || icon_only || id.starts_with("attention-") || id == "inspect-work";
		let tip = accessible.clone();
		let action = Rc::new(action);
		let keyboard = action.clone();
		let debug_id = id.clone();
		let label_id = format!("overflow-{id}");

		gpui::div()
			.debug_selector(move || debug_id)
			.id(SharedString::from(id))
			.role(if is_tab { Role::Tab } else { Role::Button })
			.aria_selected(active)
			.when(active && !is_tab, |row| row.bg(gpui::rgba(0xffffff0d)))
			.tab_index(0)
			.aria_label(accessible)
			.when(show_tip, |button| {
				button.tooltip(move |_, cx| cx.new(|_| PanelTip(tip.clone())).into())
			})
			.px_2()
			.py_1()
			.flex()
			.items_center()
			.when(icon_only, |button| {
				button.size(gpui::px(CHROME_CONTROL_SIZE)).p_0().justify_center()
			})
			.rounded(gpui::px(5.0))
			.cursor_pointer()
			.when(!is_tree && !is_tab, |button| {
				button.hover(move |s| {
					s.bg(gpui::rgba(if active { SELECTED_HOVER_FILL } else { HOVER_FILL }))
				})
			})
			.when(is_tree, |button| button.hover(|s| s.text_color(gpui::rgb(TEXT))))
			.on_click(cx.listener(move |s, _, _, cx| {
				if is_tree || is_tab {
					cx.stop_propagation();
				}
				action(s, cx);
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					keyboard(s, cx);

					cx.stop_propagation();
				}
			}))
			.when(is_tree, |button| {
				button.px_0().py_0().h(gpui::px(TREE_ROW_HEIGHT)).w_full().min_w_0()
			})
			.when(is_tab, |button| {
				button
					.h(gpui::px(26.))
					.py_0()
					.px(gpui::px(10.))
					.w_full()
					.min_w_0()
					.text_size(gpui::px(12.))
					.line_height(gpui::px(18.))
					.text_color(gpui::rgb(if active { TEXT } else { TEXT_MUTED }))
					.hover(|style| style.text_color(gpui::rgb(TEXT)))
			})
			.when(is_event || is_prompt, |button| button.w_full().min_w_0())
			.child(if let Some(icon) = icon {
				icon
			} else if is_tree {
				crate::ui_motion::AgentLabel {
					id: SharedString::from(label_id).into(),
					text: label.into(),
				}
				.into_any_element()
			} else {
				gpui::div()
					.min_w_0()
					.when(is_prompt, |text| text.flex_1())
					.when(!is_event && !is_prompt, |text| text.whitespace_nowrap().text_ellipsis())
					.child(label)
					.into_any_element()
			})
			.smooth()
			.enabled(!is_tree)
			.into_any_element()
	}

	pub(super) fn workspace_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut panel = gpui::div()
			.id("agent-sidebar")
			.w_full()
			.min_w_0()
			.relative()
			.h_full()
			.flex()
			.flex_col()
			.px(gpui::px(6.))
			.pb(gpui::px(CONTROL_MARGIN))
			.pt(gpui::px(WINDOW_CONTROLS_CLEARANCE))
			.gap_1()
			.bg(gpui::rgba(AGENT_SIDEBAR_MATERIAL));

		panel = panel
			.child(self.work_navigation(
				"new-conversation",
				"New conversation",
				workspace_symbols::Symbol::Plus,
				|s, cx| s.new_work_conversation(cx),
				cx,
			))
			.child(self.workspace_tabs(cx))
			.child(gpui::div().flex_1())
			.child(self.work_navigation(
				"all-work",
				"All work",
				workspace_symbols::Symbol::AllWork,
				|s, cx| {
					s.stop_voice(cx);
					s.workspace.browsing = true;
					cx.notify();
				},
				cx,
			));
		panel
			.when(self.workspace.sidebar_visible, |panel| {
				panel.child(self.sidebar_resize_handle(cx))
			})
			.into_any_element()
	}

	pub(super) fn conversation_pages(&self) -> Vec<(String, String, bool)> {
		let mut pages = vec![(self.root_id().unwrap_or_default(), "Main".into(), false)];
		let open = &self.workspace.pages;
		if let Some(snapshot) = &self.snapshot {
			pages.extend(open.iter().filter_map(|id| {
				self.native_page_label(id)
					.or_else(|| {
						snapshot.work_items.iter().find(|w| &w.id == id).map(|w| self.work_label(w))
					})
					.map(|label| (id.clone(), label, true))
			}));
		}
		pages
	}

	pub(super) fn conversation_status(&self, id: &str) -> String {
		if let Some((owner, thread)) = self.native_agents.pages.get(id) {
			return self
				.native_agents
				.lists
				.get(owner)
				.into_iter()
				.flatten()
				.find(|a| &a.thread_id == thread)
				.map(|a| a.status.clone())
				.unwrap_or_default();
		}
		self.snapshot
			.as_ref()
			.and_then(|snapshot| {
				snapshot
					.work_items
					.iter()
					.find(|w| w.id == id)
					.map(|w| super::graph::state_in(snapshot, w).0.to_string())
			})
			.unwrap_or_default()
	}

	pub(super) fn workspace_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut list = gpui::div()
			.id("agent-pages")
			.role(Role::TabList)
			.aria_label("Open conversations")
			.w_full()
			.min_w_0()
			.max_h(gpui::px(360.))
			.overflow_y_scroll()
			.flex()
			.flex_col();
		for (id, label, closable) in self.conversation_pages() {
			let active = self.conversation_page().as_ref() == Some(&id)
				|| (!closable && self.selected.is_none());
			let keyboard_select = id.clone();
			let row_select = id.clone();
			let group = SharedString::from(format!("conversation-tab-{id}"));
			let mut tab = gpui::div()
				.id(group.clone())
				.group(group.clone())
				.debug_selector({
					let id = id.clone();
					move || format!("page-{id}")
				})
				.role(Role::Tab)
				.aria_label(label.clone())
				.aria_selected(active)
				.tab_index(0)
				.w_full()
				.min_w_0()
				.h(gpui::px(34.))
				.flex_none()
				.overflow_hidden()
				.flex()
				.items_center()
				.gap(gpui::px(6.))
				.rounded(gpui::px(7.))
				.cursor_pointer()
				.bg(gpui::rgba(if active { 0xffffff10 } else { 0xffffff00 }))
				.hover(move |s| s.bg(gpui::rgba(if active { 0xffffff18 } else { HOVER_FILL })))
				.on_click(cx.listener(move |s, _, _, cx| s.open_page(&row_select, cx)))
				.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
					if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str())
					{
						s.open_page(&keyboard_select, cx);
						cx.stop_propagation();
					}
				}))
				.child(
					gpui::div()
						.debug_selector({
							let id = id.clone();
							move || format!("conversation-mark-{id}")
						})
						.w(gpui::px(40.))
						.h_full()
						.flex_none()
						.flex()
						.items_center()
						.justify_center()
						.child(crate::ui_motion::AgentRailStatus {
							state: self.conversation_status(&id),
							label: label.clone(),
						}),
				)
				.child(
					gpui::div()
						.flex_1()
						.min_w(gpui::px(80.))
						.overflow_hidden()
						.whitespace_nowrap()
						.text_ellipsis()
						.child(label.clone()),
				);
			if closable {
				let close = id.clone();
				let keyboard = id.clone();
				tab = tab.child(
					gpui::div()
						.id(SharedString::from(format!("close-{id}")))
						.role(Role::Button)
						.tab_index(0)
						.aria_label(format!("Close {label}"))
						.size(gpui::px(20.))
						.flex_none()
						.rounded(gpui::px(5.))
						.flex()
						.items_center()
						.justify_center()
						.opacity(if active { 0.65 } else { 0. })
						.group_hover(group, |s| s.opacity(1.))
						.focus(|s| s.opacity(1.))
						.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
						.child(workspace_symbols::icon_sized(
							super::super::workspace_symbols::Symbol::Close,
							12.,
						))
						.on_click(cx.listener(move |s, _, _, cx| {
							cx.stop_propagation();
							s.close_page(&close, cx);
						}))
						.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
							if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str())
							{
								cx.stop_propagation();
								s.close_page(&keyboard, cx);
							}
						})),
				);
				let visible = !self.workspace.closing_pages.contains(&id);
				let surface = cx.entity().downgrade();
				list = list.child(TabReveal {
					id: SharedString::from(format!("tab-reveal-{id}")).into(),
					visible,
					child: tab.into_any_element(),
					closed: Box::new(move |cx| {
						let _ = surface.update(cx, |s, cx| {
							if s.workspace.closing_pages.remove(&id) {
								s.workspace.pages.retain(|p| p != &id);
								cx.notify();
							}
						});
					}),
				});
			} else {
				list = list.child(tab.mb(gpui::px(4.)));
			}
		}
		list.into_any_element()
	}

	pub(super) fn connection_initializing(&self) -> bool {
		self.snapshot.as_ref().is_some_and(|s| s.connection_initializing)
	}

	pub(super) fn composer_unavailable_reason(&self) -> Option<&'static str> {
		if self.native_agents.selected.is_some() {
			// Native connection feedback stays inside the same composer surface.
			return None;
		}

		if self.uncertain {
			return Some(
				"Delivery is unconfirmed. Your draft is kept; sending is paused to avoid duplicates.",
			);
		}
		if matches!(self.displayed_load_state(), LoadState::Unavailable | LoadState::Stale) {
			return Some("The service connection is unavailable. Your history and draft are kept.");
		}

		if self.connection_initializing() {
			return Some("Connecting to Codex…");
		}

		let selected = self.selected.as_deref()?;
		let snapshot = self.snapshot.as_ref()?;
		let root = self.root_id();

		for event in &snapshot.pending_events {
			if event.work_item_id != selected && Some(&event.work_item_id) != root.as_ref() {
				continue;
			}

			let reason = match event.event_kind.as_str() {
				"thread_in_use_needs_attention" => THREAD_LOCKED_MESSAGE,
				"reconnection_needs_attention" =>
					"The agent could not reconnect. Messages are saved and sending is paused. Decodex will retry automatically.",
				"configuration_needs_attention" =>
					"The agent configuration is unavailable. Your history and draft are kept.",
				"recovery_needs_attention" | "wake_failed" =>
					"The agent could not resume this conversation. Your history and draft are kept.",
				_ => continue,
			};

			return Some(reason);
		}

		snapshot
			.work_items
			.iter()
			.find(|w| w.id == selected)
			.filter(|w| w.dispatch_state == AgentDispatchStateDto::Unknown)
			.map(
				|_| "The agent connection is interrupted. Checking the current conversation state.",
			)
	}

	fn connection_failure_detail(&self) -> Option<&str> {
		let selected = self.selected.as_ref()?;
		let event = self.snapshot.as_ref()?.pending_events.iter().rev().find(|e| {
			&e.work_item_id == selected
				&& matches!(
					e.event_kind.as_str(),
					"reconnection_needs_attention" | "recovery_needs_attention" | "wake_failed"
				)
		})?;
		let (owner, AgentHistoryResult::Available { entries, .. }) = self.history.as_ref()? else {
			return None;
		};

		if owner != selected {
			return None;
		}

		entries
			.iter()
			.find(|entry| entry.id == event.id && entry.kind == "system")
			.map(|entry| entry.text.as_str())
	}

	fn unavailable_composer(&self, reason: &'static str, cx: &mut Context<Self>) -> AnyElement {
		if reason == THREAD_LOCKED_MESSAGE {
			return gpui::div()
				.id("conversation-unavailable")
				.debug_selector(|| "conversation-unavailable".into())
				.role(Role::Status)
				.aria_label(THREAD_LOCKED_MESSAGE)
				.h(gpui::px(40.))
				.flex()
				.items_center()
				.justify_center()
				.gap(gpui::px(8.))
				.text_size(gpui::px(12.))
				.text_color(gpui::rgb(TEXT_MUTED))
				.child(workspace_symbols::icon(super::super::workspace_symbols::Symbol::Lock))
				.child(THREAD_LOCKED_MESSAGE)
				.into_any_element();
		}

		let detail = self.connection_failure_detail();
		let (title, description) = match detail {
			Some(text) if text.contains("ProcessUnavailable") => (
				"Codex couldn't start",
				"The local connection could not start. Decodex will retry automatically.",
			),
			Some(text) if text.contains("RefreshQuota") || text.contains("usage limit") =>
				("Account unavailable", "Check account usage or sign-in in Settings → Accounts."),
			Some(text) if text.contains("SelectWorkingDirectory") => (
				"Project folder is unavailable",
				"Restore access to the project folder so this conversation can resume. Your messages and draft are kept.",
			),
			_ => ("Can't continue this conversation", reason),
		};

		gpui::div()
			.id("conversation-unavailable")
			.debug_selector(|| "conversation-unavailable".into())
			.role(Role::Status)
			.aria_label(format!("{title}. {description}"))
			.text_color(gpui::rgb(TEXT))
			.flex()
			.flex_col()
			.child(
				gpui::div()
					.mb(gpui::px(8.))
					.text_size(gpui::px(12.))
					.line_height(gpui::px(17.))
					.font_weight(FontWeight::MEDIUM)
					.child(title),
			)
			.child(
				gpui::div()
					.text_size(gpui::px(11.))
					.line_height(gpui::px(17.))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(description),
			)
			.when(detail.is_some(), |d| {
				d.child(
					gpui::div().mt(gpui::px(8.)).flex().justify_end().items_center().child(
						gpui::div()
							.id("connection-details")
							.role(Role::Button)
							.aria_label("Technical details")
							.aria_expanded(self.workspace.connection_details_expanded)
							.tab_index(0)
							.cursor_pointer()
							.flex()
							.items_center()
							.gap(gpui::px(5.))
							.h(gpui::px(20.))
							.text_size(gpui::px(11.))
							.text_color(gpui::rgb(TEXT_MUTED))
							.hover(|s| s.text_color(gpui::rgb(TEXT)))
							.child("Details")
							.child(workspace_symbols::disclosure_chevron(
								"connection-details-chevron",
								self.workspace.connection_details_expanded,
							))
							.on_click(cx.listener(|s, _, _, cx| {
								s.toggle_connection_details(cx);
							}))
							.on_key_down(cx.listener(|s, event: &KeyDownEvent, _, cx| {
								if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
									s.toggle_connection_details(cx);
									cx.stop_propagation();
								}
							}))
							.smooth(),
					),
				)
			})
			.child(ui_motion::disclosure(
				"connection-diagnostic",
				self.workspace.connection_details_expanded && detail.is_some(),
				gpui::div()
					.pt(gpui::px(8.))
					.text_size(gpui::px(11.))
					.line_height(gpui::px(17.))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(detail.unwrap_or_default().to_owned()),
			))
			.into_any_element()
	}

	fn composer_footer(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
		if self.native_agents.selected.is_some() {
			let feedback = self.native_feedback();
			return gpui::div()
				.flex_none()
				.w_full()
				.when(!feedback.is_empty() && !matches!(feedback, "Sent" | "Sending…"), |d| {
					d.child(
						gpui::div()
							.px(gpui::px(ui_theme::CONVERSATION_INSET))
							.py(gpui::px(6.))
							.text_size(gpui::px(ui_theme::CAPTION_SIZE))
							.text_color(gpui::rgb(TEXT_MUTED))
							.child(feedback.to_owned()),
					)
				})
				.child(self.render_composer(window, cx));
		}

		// The parent overlays this transparent footer above the scrolling conversation.
		gpui::div().debug_selector(|| "composer-footer".into()).flex_none().w_full().child(
			gpui::div()
				.w_full()
				.flex()
				.flex_col()
				.child(self.conversation_activity(cx))
				.child(self.recovered_draft_panel(cx))
				.when_some(
					self.composer_unavailable_reason().filter(|_| !self.connection_initializing()),
					|d, reason| {
						d.child(
							gpui::div()
								.w_full()
								.px(gpui::px(ui_theme::CONVERSATION_INSET))
								.py(gpui::px(12.))
								.flex()
								.justify_center()
								.child(
									gpui::div()
										.w_full()
										.max_w(gpui::px(ui_theme::CONVERSATION_WIDTH))
										.min_w_0()
										.p(gpui::px(14.))
										.rounded(gpui::px(ui_theme::COMPOSER_RADIUS))
										.bg(gpui::rgb(0x27272b))
										.flex()
										.flex_col()
										.gap(gpui::px(12.))
										.child(self.unavailable_composer(reason, cx))
										.child(self.recovery_composer(cx)),
								),
						)
					},
				)
				.when(
					self.connection_initializing() || self.composer_unavailable_reason().is_none(),
					|d| d.child(self.render_composer(window, cx)),
				),
		)
	}

	pub(super) fn selected_is_manager(&self) -> bool {
		self.is_new_conversation()
			|| self.selected.is_none()
			|| self.selected == self.root_id()
			|| self.snapshot.as_ref().is_some_and(|snapshot| {
				snapshot.work_items.iter().any(|work| {
					Some(&work.id) == self.selected.as_ref()
						&& work.kind == AgentWorkKindDto::Manager
				})
			})
	}

	fn workspace_transcript(
		&mut self,
		selected: Option<&AgentWorkItemDto>,
		is_agent: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		if self.is_new_conversation() {
			return gpui::div()
				.flex_1()
				.h_full()
				.flex()
				.flex_col()
				.items_center()
				.pt(gpui::px(96.))
				.gap_3()
				.child(
					gpui::div().text_size(gpui::px(24.)).child("What would you like to work on?"),
				)
				.into_any_element();
		}

		let scroll = self
			.timeline
			.scroll
			.entry(self.selected.clone().unwrap_or_default())
			.or_default()
			.clone();
		let mut transcript = gpui::div()
			.debug_selector(|| "workspace-transcript".into())
			.id(SharedString::from(format!(
				"transcript-{}",
				self.selected.as_deref().unwrap_or("agent")
			)))
			.flex_1()
			.min_w_0()
			.min_h_0()
			.overflow_hidden()
			.track_scroll(&scroll)
			.on_scroll_wheel(cx.listener(|s, event, _, cx| s.scroll_history(event, cx)));

		if let Some(work) = selected {
			if let Some(snapshot) = &self.snapshot {
				let content = if is_agent {
					gpui::div()
						.debug_selector(|| "conversation-content".into())
						.pt(gpui::px(16.))
						.pb(gpui::px(self.workspace.composer_overlay_height))
						.px(gpui::px(ui_theme::CONVERSATION_INSET))
						.w_full()
						.max_w(gpui::px(
							ui_theme::CONVERSATION_WIDTH + 2. * ui_theme::CONVERSATION_INSET,
						))
						.min_w_0()
						.mx_auto()
						.line_height(gpui::px(BODY_LINE_HEIGHT))
						.child(self.history_panel(work, cx))
						.when(self.native_agents.selected.is_none(), |content| {
							content
								.when(
									snapshot.pending_events.iter().any(|e| {
										e.work_item_id == work.id
											&& e.event_kind.ends_with("_pending")
									}) && self.request.is_none(),
									|row| row.child(self.pending_panel(snapshot, work, cx)),
								)
								.child(self.misalignment_panel(work, cx))
								.child(self.guardian_panel(work, cx))
								.child(self.request_panel(snapshot, work, cx))
								.child(self.async_question_panel(work, cx))
						})
						.into_any_element()
				} else {
					self.details(snapshot, work, cx).into_any_element()
				};

				transcript = transcript.child(content);
			}
		} else if self.snapshot.is_none() {
			transcript = transcript.child(gpui::div().size_full().flex().items_center().child(
				ui_loading::conversation(
					if matches!(self.state, LoadState::Unavailable | LoadState::Stale) {
						"Connecting to workspace"
					} else {
						"Loading workspace"
					},
				),
			));
		} else {
			transcript = transcript.child(self.workspace_welcome(window, cx));
		}

		let owner = cx.entity();
		let key = self.selected.clone().unwrap_or_default();
		gpui::div()
			.flex_1()
			.min_h_0()
			.min_w_0()
			.relative()
			.flex()
			.child(self.anchored_native_viewport(transcript.into_any_element(), scroll.clone()))
			.child(crate::ui_scroll::Scrollbar {
				id: SharedString::from(format!("history-scrollbar-{key}")).into(),
				scroll,
				changed: Rc::new(move |offset, _, cx| {
					owner.update(cx, |s, cx| s.drag_history_scrollbar(offset, cx));
				}),
			})
			.into_any_element()
	}

	pub(super) fn render_workspace(
		&mut self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		self.poll_native_agents(cx);
		self.observe_visible_output(cx);
		self.prepare_workspace_history(window, cx);

		let is_agent = self.native_agents.selected.is_some() || self.selected_is_manager();
		let selected = self.conversation_work();
		let wide = f32::from(window.viewport_size().width) > 1_000.0;
		if self.workspace.browsing {
			let browser = self.render_work_browser(cx);
			return self.workspace_frame(browser, wide, window, cx);
		}
		let mut chat = gpui::div()
			.id("conversation-panel-focus")
			.capture_any_mouse_down(cx.listener(|s, _, _, _| s.workspace.focused_panel = None))
			.relative()
			.flex_1()
			.min_w_0()
			.min_h_0()
			.overflow_hidden()
			.flex()
			.flex_col()
			.rounded(gpui::px(10.))
			.bg(gpui::rgba(AGENT_CHAT_OVERLAY));

		chat = chat.when(self.native_agents.selected.is_none(), |chat| {
			chat.when_some(selected.as_ref(), |chat, work| chat.child(self.archive_panel(work, cx)))
		});

		let floating_composer = is_agent
			&& (selected.is_some() || self.is_new_conversation())
			&& !self.selected_is_archived();
		if !floating_composer {
			self.workspace.composer_overlay_height = 0.;
		}
		let transcript = self.workspace_transcript(selected.as_ref(), is_agent, window, cx);

		chat = chat.child(
			gpui::div()
				.flex_1()
				.min_h_0()
				.flex()
				.child(self.history_rail_slot(window, cx))
				.relative()
				.child(transcript)
				.child(self.latest_button(window, cx)),
		);

		if floating_composer && !self.workspace.browsing {
			let owner = cx.entity();
			chat = chat.child(
				gpui::div()
					.debug_selector(|| "floating-composer".into())
					.absolute()
					.left_0()
					.right_0()
					.bottom_0()
					.child(self.composer_footer(window, cx))
					.child(
						gpui::canvas(
							move |bounds, _, cx| {
								owner.update(cx, |s, cx| {
									let height = f32::from(bounds.size.height);
									if (s.workspace.composer_overlay_height - height).abs() > 0.5 {
										s.workspace.composer_overlay_height = height;
										cx.notify();
									}
								});
							},
							|_, _, _, _| {},
						)
						.absolute()
						.inset_0(),
					),
			);
		} else if !is_agent && let Some(work) = selected.as_ref() {
			chat = chat
				.child(self.recovered_draft_panel(cx))
				.child(self.conversation_activity(cx))
				.child(self.workspace_followup(work, cx));
		}

		let chat = self.workspace_details_overlay(chat, selected.as_ref(), window, cx);
		self.workspace_frame(chat.into_any_element(), wide, window, cx)
	}

	fn workspace_frame(
		&mut self,
		chat: AnyElement,
		wide: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let (graph_width, graph_height) = self.workspace_graph_size(window, wide);

		self.update_graph_inset(graph_width, graph_height);

		let center = gpui::div().flex_1().min_w_0().h_full().flex().flex_col().child(chat).child(
			ui_motion::reveal("agent-graph-dock", graph_height, false, self.workspace_graph(cx)),
		);
		let body = gpui::div().flex_1().min_h_0().flex().overflow_hidden().child(center).child(
			ui_motion::reveal(
				"agent-tree-panel",
				self.agent_tree_width(window),
				true,
				self.agent_tree(cx),
			),
		);
		// Share one glass plane with the left sidebar; only the conversation adds a light tint.
		let main = gpui::div()
			.flex_1()
			.min_w_0()
			.h_full()
			.pt(gpui::px(WINDOW_CONTROLS_CLEARANCE))
			.relative()
			.flex()
			.flex_col()
			.bg(gpui::rgba(AGENT_SIDEBAR_MATERIAL))
			.child(body);

		self.workspace_resize_root(cx)
			.size_full()
			.flex()
			.text_size(gpui::px(BODY_SIZE))
			.font_family(FONT_FAMILY)
			.text_color(gpui::rgb(TEXT))
			.on_action(cx.listener(|_, _: &SubmitComposer, _, cx| cx.stop_propagation()))
			.child(self.sidebar_slot(wide, window, cx))
			.child(main)
			.into_any_element()
	}

	fn workspace_details_overlay(
		&self,
		mut chat: Stateful<Div>,
		selected: Option<&AgentWorkItemDto>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let presence = ui_motion::value(
			"work-details-presence",
			if self.workspace.details_visible { 1. } else { 0. },
			window,
			cx,
		);

		if presence > 0.001
			&& let (Some(snapshot), Some(work)) = (&self.snapshot, selected)
		{
			chat = chat.child(
				gpui::deferred(
					gpui::div()
						.absolute()
						.top(gpui::px(8. + (1. - presence) * 5.))
						.left(gpui::px(
							self.menu_trigger_bounds
								.get("inspect-work")
								.map(|bounds| {
									f32::from(bounds.origin.x + bounds.size.width)
										- self.workspace_sidebar_width(window)
										- 320.
								})
								.unwrap_or(12.)
								.max(12.),
						))
						.w(gpui::px(320.))
						.max_w_full()
						.opacity(presence)
						.child(self.inspection_card(snapshot, work, cx)),
				)
				.with_priority(2),
			);
		}

		chat
	}

	fn prepare_workspace_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(id) = self.workspace.opening_work.clone()
			&& self.snapshot.as_ref().is_some_and(|s| s.work_items.iter().any(|w| w.id == id))
		{
			self.workspace.opening_work = None;
			self.workspace.new_conversation = None;
			self.open_page(&id, cx);
		}
		if let Some(current) = self.conversation_page()
			&& !self.is_new_conversation()
			&& Some(&current) != self.root_id().as_ref()
			&& !self.workspace.pages.contains(&current)
			&& !self.workspace.closing_pages.contains(&current)
		{
			self.workspace.pages.push(current);
		}

		self.workspace.graph_display_zoom =
			ui_motion::value("agent-graph-zoom", self.workspace.graph_zoom, window, cx);

		if self.is_new_conversation() {
			self.timeline.marks.clear();
			return;
		}
		if self.timeline.older_scroll_anchor.is_none() {
			self.prefetch_older_history(cx);
		}

		self.restore_history_anchor(window, cx);
		self.prepare_history_marks();
		self.prepare_history_layout(window);
		self.animate_history_scroll(window, cx);
		self.follow_voice_scroll(window, cx);

		if self.timeline.latest_follow_work == self.selected
			&& let Some(scroll) =
				self.selected.as_ref().and_then(|work| self.timeline.scroll.get(work))
		{
			let current = f32::from(scroll.offset().y);
			let target = -f32::from(scroll.max_offset().y);

			if (target - current).abs() > 0.5 {
				scroll.set_offset(gpui::point(
					gpui::px(0.),
					gpui::px(current + (target - current) * 0.22),
				));

				ui_motion::request_frame(window, cx);

				cx.notify();
			} else {
				scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(target)));
			}
		}
	}

	fn restore_history_anchor(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
		let Some(HistoryScrollAnchor { work: id, offset, maximum, message: anchor }) =
			self.timeline.older_scroll_anchor.clone()
		else {
			return;
		};

		if self.selected.as_ref() != Some(&id) {
			self.timeline.older_scroll_anchor = None;

			return;
		}

		self.timeline.wheel_scroll = None;

		if let Some(scroll) = self.timeline.scroll.get(&id).cloned() {
			let entity = cx.entity();

			cx.defer(move |cx| {
				entity.update(cx, |s, cx| {
					if s.selected.as_ref() != Some(&id) {
						return;
					}

					let delta = anchor
						.and_then(|(id, position)| {
							s.timeline.marks.get(&id).map(|mark| mark.position.get() - position)
						})
						.unwrap_or_else(|| f32::from(scroll.max_offset().y) - maximum);
					let target = (offset - delta).clamp(-f32::from(scroll.max_offset().y), 0.);

					if (f32::from(scroll.offset().y) - target).abs() < 0.5 {
						s.timeline.older_scroll_anchor = None;
					} else {
						scroll.set_offset(gpui::point(scroll.offset().x, gpui::px(target)));
					}

					cx.notify();
				})
			});
		}
	}

	fn workspace_welcome(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		gpui::div()
			.debug_selector(|| "workspace-welcome".into())
			.size_full()
			.flex()
			.flex_col()
			.justify_center()
			.items_center()
			.pb(gpui::px(90.0))
			.child(self.conversation_activity(cx))
			.child(self.recovered_draft_panel(cx))
			.child(gpui::div().w_full().child(self.render_composer(window, cx)))
			.into_any_element()
	}

	fn workspace_followup(&self, work: &AgentWorkItemDto, cx: &mut Context<Self>) -> AnyElement {
		let title = work.title.clone();
		let footer = gpui::div().p_3().child(self.workspace_action(
			"discuss-with-agent".into(),
			"Discuss this work with Agent →".into(),
			move |s, cx| {
				if let Some(root) = s.root_id() {
					s.open_page(&root, cx);

					if s.composer.read(cx).content().trim().is_empty() {
						s.composer.update(cx, |input, cx| {
							input.set_content(&format!("About {title}: "), cx)
						});
					}
				}
			},
			cx,
		));

		footer
			.when(
				self.command_connection_ready()
					&& self.running_turn().is_some_and(|(id, _)| id.as_str() == work.id),
				|footer| {
					footer.child(
						self.workspace_action(
							"worker-stop".into(),
							if self.interrupting.is_some() {
								"Stopping response…"
							} else {
								"Stop response"
							}
							.into(),
							|s, cx| s.interrupt_current(cx),
							cx,
						),
					)
				},
			)
			.into_any_element()
	}

	pub(super) fn work_label(&self, work: &AgentWorkItemDto) -> String {
		if Some(&work.id) == self.root_id().as_ref()
			&& ["Agent", "Main"].contains(&work.title.as_str())
		{
			return "Main".into();
		}

		if let Some(snapshot) = &self.snapshot {
			if let Some(project) = snapshot.workspaces.iter().find(|p| p.agent_id == work.id) {
				return project.name.clone();
			}

			if work.title == work.id && work.kind == AgentWorkKindDto::Task {
				// Older records used readable work slugs as titles. Preserve their meaning
				// instead of replacing it with a position-dependent Agent 1/2 label.
				let title = work
					.parent_goal_id
					.as_ref()
					.and_then(|parent| work.title.strip_prefix(&format!("{parent}-")))
					.unwrap_or(&work.title);
				return title
					.split(['-', '_'])
					.filter(|word| !word.is_empty())
					.enumerate()
					.map(|(index, word)| match word {
						"gpui" => "GPUI".into(),
						"ui" => "UI".into(),
						"chief" => "Agent".into(),
						_ if index == 0 => {
							let mut chars = word.chars();
							chars
								.next()
								.map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
								.unwrap_or_default()
						},
						_ => word.to_owned(),
					})
					.collect::<Vec<_>>()
					.join(" ");
			}
		}

		work.title.clone()
	}

	fn workspace_graph(&self, cx: &mut Context<Self>) -> AnyElement {
		let scope = self.workspace.graph_scope.clone().or_else(|| self.root_id());
		let title = self
			.snapshot
			.as_ref()
			.and_then(|s| s.work_items.iter().find(|w| Some(&w.id) == scope.as_ref()))
			.map(|w| self.work_label(w))
			.unwrap_or_else(|| "Work".into());
		let mut panel = self.graph_frame(title, cx).id("graph-panel-focus").capture_any_mouse_down(
			cx.listener(|s, _, _, _| s.workspace.focused_panel = Some(Panel::Bottom)),
		);
		let Some(snapshot) = &self.snapshot else {
			return panel.into_any_element();
		};
		let layout = self.workspace_graph_layout();
		let zoom = self.workspace.graph_zoom;
		let mut area = self.graph_canvas(&layout, cx);

		for node in &layout.nodes {
			let Some(work) = snapshot.work_items.iter().find(|w| w.id == node.id) else {
				continue;
			};

			area = area.child(self.graph_node(node, work, cx));
		}

		panel = panel.child(area);

		if !layout.edges.is_empty() {
			panel = panel.child(
				gpui::div()
					.px_2()
					.text_size(gpui::px(10.0))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child("Blue: reporting · arrows: prerequisites"),
			);
		}
		if layout.cyclic {
			panel = panel.child(
				gpui::div()
					.px_2()
					.text_size(gpui::px(11.0))
					.text_color(gpui::rgb(AMBER))
					.child("Dependency cycle · review work relations"),
			);
		}

		panel
			.child(
				gpui::div()
					.flex()
					.justify_start()
					.gap_1()
					.p(gpui::px(CONTROL_MARGIN))
					.child(self.workspace_action(
						"zoom-out".into(),
						"−".into(),
						|s, cx| {
							s.workspace.graph_zoom = (s.workspace.graph_zoom / 1.2).max(0.35);

							cx.notify();
						},
						cx,
					))
					.child(self.workspace_action(
						"zoom-reset".into(),
						format!("{}%", (zoom * 100.0).round()),
						|s, cx| {
							s.workspace.graph_zoom = 0.85;
							s.workspace.graph_pan = (0.0, 0.0);

							cx.notify();
						},
						cx,
					))
					.child(self.workspace_action(
						"zoom-in".into(),
						"+".into(),
						|s, cx| {
							s.workspace.graph_zoom = (s.workspace.graph_zoom * 1.2).min(1.8);

							cx.notify();
						},
						cx,
					)),
			)
			.into_any_element()
	}

	fn graph_frame(&self, title: String, cx: &mut Context<Self>) -> Div {
		let mut panel = gpui::div().w_full().min_w_0().h_full().flex().flex_col().pt(gpui::px(8.));

		panel = panel.child(
			gpui::div()
				.h(gpui::px(PANEL_HEADER_HEIGHT))
				.min_h(gpui::px(PANEL_HEADER_HEIGHT))
				.flex()
				.items_center()
				.px_2()
				.child(self.workspace_action(
					"graph-up".into(),
					"←".into(),
					|s, cx| {
						s.workspace.graph_scope = s
							.snapshot
							.as_ref()
							.and_then(|snap| {
								snap.work_items
									.iter()
									.find(|w| Some(&w.id) == s.workspace.graph_scope.as_ref())
							})
							.and_then(|w| w.parent_goal_id.clone());
						s.workspace.graph_pan = (0.0, 0.0);

						cx.notify();
					},
					cx,
				))
				.child(
					gpui::div()
						.flex_1()
						.overflow_hidden()
						.whitespace_nowrap()
						.text_ellipsis()
						.child(title),
				)
				.child(
					self.workspace_action(
						"graph-expand".into(),
						if self.workspace.graph_expanded {
							"Restore conversation"
						} else {
							"Expand graph"
						}
						.into(),
						|s, cx| {
							s.workspace.graph_expanded = !s.workspace.graph_expanded;

							cx.notify();
						},
						cx,
					),
				)
				.child(self.workspace_action(
					"graph-close".into(),
					"×".into(),
					|s, cx| {
						s.workspace.graph_visible = false;
						s.workspace.graph_expanded = false;

						cx.notify();
					},
					cx,
				)),
		);

		panel
	}

	fn graph_canvas(&self, layout: &Layout, cx: &mut Context<Self>) -> Stateful<Div> {
		let zoom = self.workspace.graph_display_zoom;
		let pan = (
			self.workspace.graph_pan.0 + self.workspace.graph_inset.0,
			self.workspace.graph_pan.1 + self.workspace.graph_inset.1,
		);
		let edges: Vec<_> = layout
			.edges
			.iter()
			.map(|edge| (edge, false))
			.chain(layout.reports.iter().map(|edge| (edge, true)))
			.map(|((a, b), report)| {
				let a = &layout.nodes[*a];
				let b = &layout.nodes[*b];

				((a.x + 160.0, a.y + 26.0), (b.x, b.y + 26.0), report)
			})
			.collect();

		gpui::div()
			.id("work-graph-canvas")
			.tab_index(0)
			.role(Role::Group)
			.aria_label("Work graph. Drag or scroll to pan. Control-scroll to zoom.")
			.on_key_down(
				cx.listener(|s, event: &KeyDownEvent, _, cx| s.handle_graph_key(event, cx)),
			)
			.flex_1()
			.min_h_0()
			.relative()
			.overflow_hidden()
			.on_scroll_wheel(cx.listener(|s, event: &ScrollWheelEvent, _, cx| {
				let delta = event.delta.pixel_delta(gpui::px(20.0));

				if event.modifiers.control || event.modifiers.platform {
					s.workspace.graph_zoom =
						(s.workspace.graph_zoom + f32::from(delta.y) * 0.002).clamp(0.35, 1.8);
				} else {
					s.workspace.graph_pan.0 += f32::from(delta.x);
					s.workspace.graph_pan.1 += f32::from(delta.y);
				}

				cx.stop_propagation();
				cx.notify();
			}))
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(|s, event: &MouseDownEvent, _, _| {
					s.workspace.graph_drag = Some(event.position);
				}),
			)
			.on_mouse_up(
				MouseButton::Left,
				cx.listener(|s, _, _, _| {
					s.workspace.graph_drag = None;
				}),
			)
			.on_mouse_move(cx.listener(|s, event: &MouseMoveEvent, _, cx| {
				if event.pressed_button == Some(MouseButton::Left)
					&& s.workspace.sidebar_drag.is_none()
				{
					if let Some(previous) = s.workspace.graph_drag {
						s.workspace.graph_pan.0 += f32::from(event.position.x - previous.x);
						s.workspace.graph_pan.1 += f32::from(event.position.y - previous.y);
						s.workspace.graph_drag = Some(event.position);

						cx.notify();
					}
				} else {
					s.workspace.graph_drag = None;
				}
			}))
			.child(
				gpui::canvas(
					|_, _, _| (),
					move |bounds, _, window, _| {
						for (a, b, report) in edges {
							let start = bounds.origin
								+ gpui::point(
									gpui::px(a.0 * zoom + pan.0),
									gpui::px(a.1 * zoom + pan.1),
								);
							let end = bounds.origin
								+ gpui::point(
									gpui::px(b.0 * zoom + pan.0),
									gpui::px(b.1 * zoom + pan.1),
								);
							let mut path = PathBuilder::stroke(gpui::px(1.0));

							path.move_to(start);
							path.cubic_bezier_to(
								end,
								gpui::point((start.x + end.x) * 0.5, start.y),
								gpui::point((start.x + end.x) * 0.5, end.y),
							);

							if !report {
								path.move_to(end + gpui::point(gpui::px(-5.0), gpui::px(-3.0)));
								path.line_to(end);
								path.line_to(end + gpui::point(gpui::px(-5.0), gpui::px(3.0)));
							}

							if let Ok(path) = path.build() {
								window.paint_path(
									path,
									gpui::rgb(if report { BLUE } else { TEXT_MUTED }),
								);
							}
						}
					},
				)
				.absolute()
				.size_full(),
			)
	}

	fn handle_graph_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
		match event.keystroke.key.as_str() {
			"left" => self.workspace.graph_pan.0 += 32.0,
			"right" => self.workspace.graph_pan.0 -= 32.0,
			"up" => self.workspace.graph_pan.1 += 32.0,
			"down" => self.workspace.graph_pan.1 -= 32.0,
			"+" | "=" => self.workspace.graph_zoom = (self.workspace.graph_zoom * 1.2).min(1.8),
			"-" => self.workspace.graph_zoom = (self.workspace.graph_zoom / 1.2).max(0.35),
			"escape" => self.workspace.graph_expanded = false,
			_ => return,
		};

		cx.stop_propagation();
		cx.notify();
	}

	fn graph_node(
		&self,
		node: &Node,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let zoom = self.workspace.graph_display_zoom;
		let pan = (
			self.workspace.graph_pan.0 + self.workspace.graph_inset.0,
			self.workspace.graph_pan.1 + self.workspace.graph_inset.1,
		);
		let id = work.id.clone();
		let key = id.clone();
		let (status, _) = self
			.snapshot
			.as_ref()
			.map_or_else(|| graph::state(work), |snapshot| graph::state_in(snapshot, work));
		let blocked_by = self
			.snapshot
			.as_ref()
			.map(|snapshot| {
				graph::blockers(snapshot, work)
					.into_iter()
					.map(|item| self.work_label(item))
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();
		let tip = if blocked_by.is_empty() {
			format!("{} · {status}", self.work_label(work))
		} else {
			format!("Waiting for: {}", blocked_by.join(", "))
		};
		let element = gpui::div()
			.id(SharedString::from(format!("graph-node-{id}")))
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!("{}: {status}. Open conversation.", self.work_label(work)))
			.when(!blocked_by.is_empty(), |node| {
				node.tooltip(move |_, cx| cx.new(|_| PanelTip(tip.clone())).into())
			})
			.absolute()
			.left(gpui::px(node.x * zoom + pan.0))
			.top(gpui::px(node.y * zoom + pan.1))
			.w(gpui::px(160.0 * zoom))
			.h(gpui::px(52.0 * zoom))
			.px_2()
			.py_1()
			.rounded(gpui::px(9.0))
			.bg(if self.workspace.graph_selected.as_ref() == Some(&id) {
				gpui::rgba(0x35353ce8)
			} else {
				gpui::rgba(0x242427db)
			})
			.hover(|style| style.bg(gpui::rgba(0x3a3a40eb)))
			.cursor_pointer()
			.overflow_hidden()
			.text_size(gpui::px((12.0 * zoom).max(10.0)))
			.on_click(cx.listener(move |s, _, _, cx| {
				s.open_page(&id, cx);

				s.workspace.graph_visible = true;
				s.workspace.graph_selected = Some(id.clone());

				cx.notify();
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if event.keystroke.key == "enter" {
					s.open_page(&key, cx);
				}
			}))
			.flex()
			.items_center()
			.gap(gpui::px(4.))
			.child(crate::ui_motion::AgentSignal {
				id: SharedString::from(format!("graph-signal-{}", work.id)).into(),
				state: status.into(),
			})
			.child(
				gpui::div()
					.min_w_0()
					.whitespace_nowrap()
					.text_ellipsis()
					.child(self.work_label(work)),
			);

		element.into_any_element()
	}
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	pub(crate) fn visual_workspace_page(&mut self, page: &str, cx: &mut Context<Self>) {
		if matches!(page, "attachments" | "microphone") {
			self.visual_workspace_page("markdown", cx);

			self.composer_menu =
				Some(if page == "microphone" { "microphone" } else { "attachments" });
			self.audio_inputs =
				vec!["MacBook Pro Microphone".into(), "Studio Display Microphone".into()];

			return;
		}
		if page == "activity" || page == "activity-collapsed" {
			self.visual_progress_fixture(page == "activity", cx);

			return;
		}
		if ["composer", "composer-menu", "composer-effort"].contains(&page) {
			self.visual_composer_page(page, cx);

			return;
		}
		if ["questions", "approval", "live", "hierarchy"].contains(&page) {
			self.visual_functional_page(page, cx);

			return;
		}

		match page {
			#[cfg(feature = "visual-capture")]
			"prompt-editor" | "prompt-remove" => self.visual_prompt_editor(page == "prompt-remove", cx),
			"recap" => self.visual_recap(),
			"worker" => self.open_page("verify", cx),
			"empty" | "empty-draft" => {
				self.workspace.sidebar_visible = false;

				if page == "empty-draft" {
					self.composer.update(cx, |input, cx| {
						input.set_content("Review the release plan with me.", cx)
					});
				}

				self.snapshot = Some(AgentSnapshotDto {
					connection_initializing: false,
					runtime_source: None,
					workspaces: vec![],
					work_items: vec![],
					dependencies: vec![],
					pending_events: vec![],
				});
				self.selected = None;
				self.history = None;

				self.workspace.pages.clear();
				self.native_agents.pages.clear();
				self.workspace.closing_pages.clear();
			},
			"expanded" => self.workspace.graph_expanded = true,
			"in-use" => {
				self.workspace.graph_visible = false;

				self.snapshot.as_mut().expect("fixture").pending_events.push(
					decodex_protocol::AgentPendingEventDto {
						id: 999,
						source_event_id: "fixture-in-use".into(),
						work_item_id: "agent".into(),
						event_kind: "thread_in_use_needs_attention".into(),
						created_at_micros: 1,
						delivery_claimed: false,
					},
				);
			},
			"compact-graph" => {
				self.workspace.graph_scope = Some("agent".into());
				self.workspace.sidebar_width = 280.0;
			},
			"markdown" => {
				self.workspace.graph_visible = false;

				if let Some((_, AgentHistoryResult::Available { entries, usage, .. })) =
					&mut self.history
				{
					*usage = Some(decodex_protocol::AgentUsageDto {
						input_tokens: 24_860,
						output_tokens: 1_820,
						context_tokens: 26_700,
						context_window: Some(128_000),
					});

					entries.clear();

					for (i,(kind,text)) in [("user","请整理检查结果，并说明下一步安排。"),("assistant","## 检查完成\n\n两位下属已提交报告，**现有会话保持可用**。\n\n- 登录流程：保留原会话\n- 启动流程：继续验证性能\n\n| 工作 | 结果 | 下一步 |\n|---|---|---|\n| 登录检查 | 已验收 | 合并检查结果 |\n| 启动检查 | 待验证 | 补充冷启动数据 |\n\n### 验证命令\n```rust\nlet status = review.result();\nassert!(status.is_verified());\n```\n\n查看 [源码](/Users/x/code/acg-box/decodex/apps/decodex-gpui/src/agent_surface.rs:1)，再确认 `review` 的结果。")].into_iter().enumerate() {
                        entries.push(decodex_protocol::AgentHistoryEntryDto{native_source:None,receipt: None, turn_id: None, weather:Vec::new(), activity: None,usage: (kind == "assistant").then_some(decodex_protocol::AgentTurnUsageDto {details:None,input_tokens:24_860,output_tokens:1_820}),duration_ms: (kind == "assistant").then_some(18_400),id:i as i64+1,kind:kind.into(),text:text.into(),created_at_micros:1_789_480_440_000_000});
                    }
				}
			},
			"task-references" => self.visual_task_references(),
			"conversation" => {
				self.workspace.graph_visible = false;
			},
			_ => {},
		}

		cx.notify();
	}

	fn visual_composer_page(&mut self, page: &str, cx: &mut Context<Self>) {
		self.visual_workspace_page("markdown", cx);
		self.composer.update(cx, |input,cx|input.set_content("Review the interface and simplify the controls.\nKeep the glass material and check keyboard navigation.",cx));

		self.capabilities = Some(decodex_protocol::AgentCapabilitiesResult::Available {
			models: ["gpt-6-astra", "gpt-5.6-sol"]
				.into_iter()
				.map(|name| decodex_protocol::AgentModelDto {
					model: decodex_protocol::ConversationModel::new(name)
						.expect("valid fixture model"),
					name: name.into(),
					efforts: vec![
						ConversationReasoningEffort::Low,
						ConversationReasoningEffort::Medium,
						ConversationReasoningEffort::High,
					],
					default_effort: Some(ConversationReasoningEffort::Medium),
					supports_fast: true,
					service_tiers: vec![],
					default_service_tier: None,
					available_cyber_programs: None,
					specialty: None,
					supports_images: true,
					availability: None,
					upgrade: None,
				})
				.collect(),
			memory_enabled: None,
		});
		self.fast = true;

		if matches!(page, "composer-menu" | "composer-effort") {
			self.composer_menu = Some("model");
		}
	}

	/// Explicit capture fixture. Never installed by the normal application path.
	pub(crate) fn visual_workspace_fixture(&mut self, cx: &mut Context<Self>) {
		self.workspace.sidebar_visible = true;

		let make =
			|id: &str, parent: Option<&str>, title: &str, status, dispatch| AgentWorkItemDto {
				id: id.into(),
				parent_goal_id: parent.map(str::to_owned),
				kind: if id == "agent" || id == "release" {
					AgentWorkKindDto::Goal
				} else {
					AgentWorkKindDto::Task
				},
				title: title.into(),
				codex_thread_id: None,
				active_turn_id: None,
				dispatch_state: dispatch,
				status,
				next_check_at_micros: None,
				created_at_micros: 1_789_480_440_000_000,
				updated_at_micros: 1_789_481_040_000_000,
			};

		self.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
			connection_initializing: false,
			runtime_source: None,
			workspaces: vec![],
			work_items: vec![
				make("agent", None, "Agent", Open, Idle),
				make("release", Some("agent"), "September release", UserDecision, Idle),
				make("flow", Some("release"), "Simplify sign-in", Resolved, Idle),
				make("verify", Some("release"), "Verify compatibility", Open, Running),
				make("trace", Some("release"), "Investigate startup", Resolved, Idle),
				make("improve", Some("release"), "Improve launch time", Open, Running),
				make("impact", Some("release"), "Check launch impact", Open, Idle),
				make("ready", Some("release"), "Release recommendation", Open, Idle),
			],
			dependencies: vec![
				("flow", "verify"),
				("trace", "improve"),
				("improve", "impact"),
				("verify", "ready"),
				("impact", "ready"),
			]
			.into_iter()
			.map(|(a, b)| AgentDependencyDto { work_item_id: b.into(), depends_on_id: a.into() })
			.collect(),
			pending_events: vec![],
		})));

		let messages = [
			("user", "Get September ready to ship. Simplify sign-in and improve startup."),
			(
				"assistant",
				"I’ve arranged two workstreams and will review their results before recommending a release.",
			),
			("user", "Keep existing sessions working. No forced sign-in after the update."),
			(
				"assistant",
				"That is a release requirement. The sign-in work includes compatibility verification.",
			),
			(
				"assistant",
				"The sign-in changes are ready. Compatibility verification is still running.\n\nStartup improvements are progressing independently. If they take longer, the sign-in changes could ship separately once verified.\n\nShould sign-in ship first, or should both changes stay in one release?",
			),
		];
		let history = AgentHistoryResult::Available {
			questions: vec![],
			questions_truncated: false,
			questions_recovering: false,
			misalignment: None,
			usage: None,
			entries: messages
				.into_iter()
				.enumerate()
				.map(|(i, (kind, text))| crate::shell::agent_surface::AgentHistoryEntryDto {
					native_source: None,
					turn_id: None,
					weather: Vec::new(),
					receipt: None,
					activity: None,
					usage: None,
					duration_ms: None,
					id: i as i64 + 1,
					kind: kind.into(),
					text: text.into(),
					created_at_micros: 1_789_480_440_000_000 + i as i64 * 120_000_000,
				})
				.collect(),
			has_more: false,
			next_before: None,
			live: vec![],
		};

		self.timeline.cache.insert("agent".into(), history.clone());

		self.history = Some(("agent".into(), history));
		self.selected = Some("agent".into());
		self.workspace.pages = vec!["verify".into()];
		self.workspace.graph_scope = Some("release".into());
		self.workspace.graph_selected = Some("verify".into());

		self.timeline.cache.insert("verify".into(),AgentHistoryResult::Available{questions:vec![],questions_truncated:false,questions_recovering:false,misalignment:None,usage: None,entries:vec![crate::shell::agent_surface::AgentHistoryEntryDto{native_source:None,receipt: None, turn_id: None, weather:Vec::new(), activity: None,usage: None,duration_ms: None,id:100,kind:"assistant".into(),text:"Checking that existing sessions reopen without another sign-in. Fresh-install verification is still running.".into(),created_at_micros:1_789_481_040_000_000}],has_more:false,next_before:None,live:vec![]});
		cx.notify();
	}
}

impl AgentSurface {
	fn draft_copy_controls(
		&self,
		index: usize,
		copy: DesktopRecoveredDraft,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let export = copy.clone();
		let mut row = gpui::div().flex().flex_wrap().gap_2();

		row = row.child(self.workspace_action(
			format!("draft-copy-export-{index}"),
			"Export full draft".into(),
			move |s, cx| s.export_draft_copy(export.clone(), cx),
			cx,
		));

		if self.draft_copy_matches_service(&copy) {
			let restore = copy.clone();

			row = row.child(self.workspace_action(
				format!("draft-copy-restore-{index}"),
				"Restore this copy".into(),
				move |s, cx| s.restore_draft_copy(restore.clone(), cx),
				cx,
			));
		} else {
			row = row.child("Unassigned copy · export to retain all data");
		}
		if copy.draft.has_unconfirmed_delivery() {
			row = row.child("Confirm delivery before removing this copy");
		} else if self.removing_draft_copy(&copy) {
			row = row
				.child("Remove this saved copy? Current input stays.")
				.child(self.workspace_action(
					format!("draft-copy-remove-confirm-{index}"),
					"Confirm removal".into(),
					move |s, cx| s.confirm_draft_copy_removal(copy.clone(), cx),
					cx,
				))
				.child(self.workspace_action(
					format!("draft-copy-remove-cancel-{index}"),
					"Cancel".into(),
					|s, cx| s.cancel_draft_copy_removal(cx),
					cx,
				));
		} else {
			row = row.child(self.workspace_action(
				format!("draft-copy-remove-{index}"),
				"Remove copy…".into(),
				move |s, cx| s.request_draft_copy_removal(copy.clone(), cx),
				cx,
			));
		}

		row.into_any_element()
	}

	fn recovered_draft_panel(&self, cx: &mut Context<Self>) -> AnyElement {
		let count = self.recovered_draft_count();

		if count == 0 {
			return gpui::div().into_any_element();
		}

		let mut panel = gpui::div().w_full().flex().flex_col().gap_2().px_4().py_2();

		panel = panel.child(self.workspace_action(
			"draft-copies-toggle".into(),
			format!("Saved draft copies ({count})"),
			|s, cx| s.toggle_recovered_drafts(cx),
			cx,
		));

		if self.show_recovered_drafts() {
			let copies = self.recovered_drafts();
			let mut list = gpui::div()
				.id("draft-copy-list")
				.max_h(gpui::px(220.0))
				.overflow_y_scroll()
				.flex()
				.flex_col()
				.gap_2();

			for (index, copy) in copies.into_iter().enumerate() {
				let preview: String = copy.draft.composer.text.chars().take(180).collect();
				let ordinary_preview = copy.draft.ordinary.values().next().map(|draft| {
					format!(
						"Ordinary draft · {} · {}",
						draft.working_directory.as_str(),
						draft.composer.text.chars().take(180).collect::<String>()
					)
				});
				let summary = format!(
					"Copy {} · {} files · {} task references · {} question drafts · {} ordinary directories",
					index + 1,
					copy.draft.composer.attachments.len(),
					copy.draft.composer.references.len(),
					copy.draft.questions.len(),
					copy.draft.ordinary.len()
				);

				list = list.child(
					gpui::div()
						.flex()
						.flex_col()
						.gap_1()
						.child(summary)
						.child(format!("Preview: {preview}"))
						.when_some(ordinary_preview, |element, preview| element.child(preview))
						.when_some(copy.draft.composer.creation.as_ref(), |element, setup| {
							element.child(creation_setup::summary(setup))
						})
						.child(self.draft_copy_controls(index, copy, cx)),
				);
			}

			panel = panel.child(list);
		}

		panel.into_any_element()
	}

	fn keep_both_draft_button(&self, cx: &mut Context<Self>) -> AnyElement {
		gpui::div()
			.id("draft-keep-both")
			.debug_selector(|| "draft-keep-both".into())
			.role(Role::Button)
			.tab_index(0)
			.aria_label("Keep both draft copies")
			.px_2()
			.py_1()
			.cursor_pointer()
			.on_click(cx.listener(|s, _, _, cx| s.keep_both_drafts(cx)))
			.on_key_down(cx.listener(|s, event: &KeyDownEvent, _, cx| {
				if !event.is_held && matches!(event.keystroke.key.as_str(), "enter" | "space") {
					cx.stop_propagation();
					s.keep_both_drafts(cx);
				}
			}))
			.child("Keep both drafts")
			.into_any_element()
	}

	fn conversation_activity(&self, cx: &mut Context<Self>) -> AnyElement {
		if self.connection_initializing()
			|| (self.composer_unavailable_reason().is_some()
				&& self.draft_storage_notice().is_none())
		{
			return gpui::div().into_any_element();
		}
		let selected = self.snapshot.as_ref().and_then(|snapshot| {
			snapshot.work_items.iter().find(|work| Some(&work.id) == self.selected.as_ref())
		});
		let notice = self.status_notice();
		let pending = self
			.snapshot
			.as_ref()
			.map(|snapshot| {
				snapshot
					.pending_events
					.iter()
					.filter(|event| Some(&event.work_item_id) == self.selected.as_ref())
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();
		let compacting = selected.is_some_and(|work| self.has_active_compaction(work));

		if self.sending && !compacting {
			return gpui::div().into_any_element();
		}

		let label = if self.uncertain {
			Some("Delivery unconfirmed · Draft kept. Sending is paused to avoid duplicates.")
		} else if let Some(notice) = self.draft_storage_notice() {
			Some(notice)
		} else if self.selected.as_deref().is_some_and(|id| self.thread_in_use(id)) {
			Some("In use in another app · Your message is saved and waiting")
		} else if pending.iter().any(|event| {
			event.event_kind.ends_with("_needs_attention") || event.event_kind.ends_with("_failed")
		}) {
			Some("This conversation needs attention. Its current operation could not complete.")
		} else if matches!(self.displayed_load_state(), LoadState::Unavailable | LoadState::Stale) {
			Some("Connection unavailable · Reconnecting")
		} else if compacting {
			Some("Compacting context")
		} else if !self.feedback.is_empty() && self.feedback != "Message saved · Waiting for agent…"
		{
			Some(self.feedback.as_str())
		} else {
			selected.and_then(|work| match work.dispatch_state {
				AgentDispatchStateDto::Dispatching | AgentDispatchStateDto::Running => None,
				AgentDispatchStateDto::Unknown =>
					Some("Connection interrupted · Checking delivery"),
				AgentDispatchStateDto::Idle => None,
			})
		}
		.or_else(|| notice.as_ref().map(|(_, detail, _)| detail.as_str()))
		.or_else(|| (!self.archive.feedback.is_empty()).then_some(self.archive.feedback.as_str()));
		let Some(label) = label else {
			return gpui::div().into_any_element();
		};

		gpui::div()
			.id("conversation-activity-status")
			.debug_selector(|| "conversation-activity-status".into())
			.role(Role::Status)
			.aria_label(label.to_owned())
			.w_full()
			.px_4()
			.py_1()
			.flex()
			.items_center()
			.justify_center()
			.gap_3()
			.text_size(gpui::px(11.0))
			.text_color(gpui::rgb(TEXT_MUTED))
			.child(label.to_owned())
			.when(self.can_keep_both_drafts(), |row| row.child(self.keep_both_draft_button(cx)))
			.into_any_element()
	}
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	fn visual_functional_page(&mut self, page: &str, cx: &mut Context<Self>) {
		self.workspace.graph_visible = false;
		self.workspace.sidebar_visible = page == "hierarchy";

		if page == "hierarchy" {
			let snapshot = self.snapshot.as_mut().expect("fixture");
			let project =
				snapshot.work_items.iter_mut().find(|work| work.id == "release").expect("fixture");

			project.kind = AgentWorkKindDto::Manager;

			snapshot.workspaces.push(decodex_protocol::AgentWorkspaceDto {
				agent_id: "release".into(),
				name: "September release".into(),
				directory: "/Users/demo/projects/release".into(),
			});
			self.open_page("release", cx);

			return;
		}
		if page == "live" {
			if let Some((_, AgentHistoryResult::Available { live, .. })) = &mut self.history {
				live.push(decodex_protocol::AgentLiveMessageDto { kind: Default::default(),turn_id:"live-turn".into(),item_id:"live-item".into(),text:"The compatibility check is progressing. I’m reviewing the existing session behavior and…".into(),truncated:false});
			}

			return;
		}

		let (method, value) = if page == "questions" {
			(
				"item/tool/requestUserInput",
				serde_json::json!({"questions":[{"id":"release_order","question":"Which release should I prepare first?","options":[{"label":"Sign-in","description":"Ship the compatibility fixes first."},{"label":"Both together","description":"Wait for startup verification."}]},{"id":"notes","question":"Anything else I should account for?","options":null}]}),
			)
		} else {
			(
				"item/commandExecution/requestApproval",
				serde_json::json!({"command":"cargo test -p app --lib","cwd":"/Users/demo/projects/release","reason":"Verify the changed sign-in behavior.","availableDecisions":["accept","decline"]}),
			)
		};

		self.snapshot.as_mut().expect("fixture").pending_events.push(
			decodex_protocol::AgentPendingEventDto {
				id: 987,
				source_event_id: "fixture-request".into(),
				work_item_id: "agent".into(),
				event_kind: "user_input_pending".into(),
				created_at_micros: 1,
				delivery_claimed: false,
			},
		);

		let request = AgentRequestResult::Available {
			work_id: "agent".into(),
			event_id: 987,
			method: method.into(),
			request_json: decodex_protocol::AgentRequestText::new(value.to_string())
				.expect("bounded fixture"),
		};

		self.prepare_question_inputs(&request, cx);

		self.request = Some(request);

		self.timeline.scroll.entry("agent".into()).or_default().scroll_to_bottom();
	}
}

pub(super) fn within_project(snapshot: &AgentSnapshotDto, project: &str, work: &str) -> bool {
	let mut current = Some(work);

	for _ in 0..=snapshot.work_items.len() {
		let Some(id) = current else {
			return false;
		};

		if id == project {
			return true;
		}

		current = snapshot
			.work_items
			.iter()
			.find(|w| w.id == id)
			.and_then(|w| w.parent_goal_id.as_deref());
	}

	false
}

pub(super) fn clock_label(micros: i64) -> String {
	let seconds = micros / 1_000_000;

	format!("{:02}:{:02}:{:02}", (seconds / 3_600) % 24, (seconds / 60) % 60, seconds % 60)
}

fn panel_icon(id: &str) -> Option<AnyElement> {
	let symbol = match id {
		"workspace-sidebar" => crate::shell::workspace_symbols::Symbol::Sidebar,
		"workspace-graph" => crate::shell::workspace_symbols::Symbol::Graph,
		"workspace-agents" => crate::shell::workspace_symbols::Symbol::Agents,
		"graph-expand" => crate::shell::workspace_symbols::Symbol::Expand,
		"graph-close" | "tree-close" => crate::shell::workspace_symbols::Symbol::Close,
		"graph-up" => crate::shell::workspace_symbols::Symbol::Back,
		"zoom-in" => crate::shell::workspace_symbols::Symbol::Plus,
		"zoom-out" => crate::shell::workspace_symbols::Symbol::Minus,
		id if id.starts_with("close-") => crate::shell::workspace_symbols::Symbol::Close,
		_ => return None,
	};

	Some(workspace_symbols::icon(symbol))
}

#[cfg(test)]
mod tests {
	use std::thread;

	use gpui::{AppContext as _, Focusable as _, ParentElement as _, Styled as _};

	use crate::shell::agent_surface::workspace::{
		AgentHistoryResult, AgentSurface, Context, ConversationWorkingDirectory, IntoElement,
		LoadState, Render, Window, graph,
	};
	use decodex_protocol::AgentWorkKindDto;

	struct ActionView(gpui::Entity<AgentSurface>);
	impl Render for ActionView {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			self.0.update(cx, |s, cx| {
				s.workspace_action(
					"test-action".into(),
					"Action".into(),
					|s, _| s.feedback.push('x'),
					cx,
				)
			})
		}
	}

	#[gpui::test]
	fn connection_details_match_the_current_failure_not_an_old_log(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.snapshot.as_mut().unwrap().pending_events =
				vec![decodex_protocol::AgentPendingEventDto {
					id: 99,
					source_event_id: "failure".into(),
					work_item_id: "agent".into(),
					event_kind: "reconnection_needs_attention".into(),
					created_at_micros: 1,
					delivery_claimed: false,
				}];

			let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history else {
				panic!("fixture");
			};
			let mut entry = entries[0].clone();

			entry.id = 99;
			entry.kind = "system".into();
			entry.text = "Agent process requires recovery: ProcessUnavailable".into();

			entries.push(entry);

			assert!(s.connection_failure_detail().unwrap().contains("ProcessUnavailable"));

			s.snapshot.as_mut().unwrap().pending_events[0].id = 100;

			assert!(s.connection_failure_detail().is_none(), "never reuse an obsolete error");
		});
	}

	#[gpui::test]
	fn startup_rechecks_saved_failures_before_showing_one_blocker(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.snapshot.as_mut().unwrap().connection_initializing = true;
			s.snapshot.as_mut().unwrap().pending_events =
				vec![decodex_protocol::AgentPendingEventDto {
					id: 99,
					source_event_id: "old-failure".into(),
					work_item_id: "agent".into(),
					event_kind: "reconnection_needs_attention".into(),
					created_at_micros: 1,
					delivery_claimed: false,
				}];
			s.composer.update(cx, |input, cx| input.set_content("Keep my draft", cx));
			s.submit(cx);
			assert!(!s.sending);
			assert!(s.submission.command.is_none());
			assert!(s.status_notice().is_none());
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear());
		assert!(visual.debug_bounds("agent-composer").is_some());
		assert!(visual.debug_bounds("conversation-unavailable").is_none());
		assert!(visual.debug_bounds("conversation-activity-status").is_none());
		surface.update(visual, |s, cx| {
			s.snapshot.as_mut().unwrap().connection_initializing = false;
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear());
		assert!(visual.debug_bounds("conversation-unavailable").is_some());
		assert!(visual.debug_bounds("conversation-activity-status").is_none());
		surface.update(visual, |s, cx| {
			s.snapshot.as_mut().unwrap().pending_events.clear();
			assert_eq!(s.composer.read(cx).content(), "Keep my draft");
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear());
		assert!(visual.debug_bounds("agent-composer").is_some());
		assert!(visual.debug_bounds("conversation-unavailable").is_none());
	}

	#[gpui::test]
	fn unavailable_thread_blocks_submission_without_losing_draft(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.snapshot.as_mut().unwrap().pending_events.clear();

			assert!(s.composer_unavailable_reason().is_none());

			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
			s.snapshot.as_mut().unwrap().pending_events.push(
				decodex_protocol::AgentPendingEventDto {
					id: 99,
					source_event_id: "offline".into(),
					work_item_id: "agent".into(),
					event_kind: "reconnection_needs_attention".into(),
					created_at_micros: 1,
					delivery_claimed: false,
				},
			);

			assert!(s.composer_unavailable_reason().unwrap().contains("could not reconnect"));

			s.submit(cx);

			assert!(!s.sending);
			assert!(s.submission.command.is_none());
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");

			s.snapshot.as_mut().unwrap().pending_events.clear();

			assert!(s.composer_unavailable_reason().is_none());
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		});
	}

	#[gpui::test]
	fn first_snapshot_has_feedback_without_replacing_retained_history(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.state = LoadState::Loading;

			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear());

		let reserved = visual.update(|window, cx| {
			let s = surface.read(cx);

			(s.agent_tree_width(window), s.workspace_graph_size(window, true))
		});

		assert!(visual.debug_bounds("loading-feedback-Loading workspace").is_some());

		surface.update(visual, |s, cx| {
			s.state = LoadState::Unavailable;

			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear());

		assert!(visual.debug_bounds("loading-feedback-Connecting to workspace").is_some());
		assert!(
			visual.debug_bounds("workspace-welcome").is_none(),
			"a cold connection is not an empty conversation"
		);

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.state = LoadState::Loading;

			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear());

		assert!(visual.debug_bounds("loading-feedback-Loading workspace").is_none());

		let loaded = visual.update(|window, cx| {
			let s = surface.read(cx);

			(s.agent_tree_width(window), s.workspace_graph_size(window, true))
		});

		assert_eq!(reserved, loaded, "the first snapshot fills existing panel slots");
		let transcript = visual.debug_bounds("workspace-transcript").unwrap();
		surface.update(visual, |s, cx| {
			s.workspace.pages.push("release".into());
			cx.notify();
		});
		visual.update(|w, cx| w.draw(cx).clear());

		assert_eq!(
			transcript,
			visual.debug_bounds("workspace-transcript").unwrap(),
			"opening the first tab must not add another layout row"
		);
	}

	#[gpui::test]
	fn held_keys_do_not_repeat_workspace_actions(cx: &mut gpui::TestAppContext) {
		let (view, visual) = cx.add_window_view(|_, cx| ActionView(cx.new(AgentSurface::new)));
		let surface = view.read_with(visual, |v, _| v.0.clone());

		visual.update(|window, cx| window.draw(cx).clear());

		let bounds = visual.debug_bounds("test-action").expect("workspace action");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.read_with(visual, |s, _| assert_eq!(s.feedback, "x", "click activates action"));

		for key in ["enter", "space"] {
			visual.simulate_event(gpui::KeyDownEvent {
				keystroke: gpui::Keystroke::parse(key).expect("activation key"),
				is_held: true,
				prefer_character_input: false,
			});
			surface
				.read_with(visual, |s, _| assert_eq!(s.feedback, "x", "held key repeats action"));
		}

		visual.simulate_keystrokes("enter space");
		surface
			.read_with(visual, |s, _| assert_eq!(s.feedback, "xxx", "fresh keys activate action"));
	}

	#[gpui::test]
	fn running_worker_keeps_an_explicit_stop_control(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.open_page("verify", cx);

			s.snapshot
				.as_mut()
				.expect("snapshot")
				.work_items
				.iter_mut()
				.find(|work| work.id == "verify")
				.expect("worker")
				.active_turn_id = Some("worker-turn".into());

			s.composer.update(cx, |input, cx| input.set_content("Retained manager draft", cx));
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.), gpui::px(1_200.)));
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("worker-stop").is_some(), "running worker has no stop control");

		surface.update(visual, |s, cx| {
			assert_eq!(s.running_turn().expect("running worker").0.as_str(), "verify");

			s.interrupt_current(cx);

			assert_eq!(s.feedback, "No service profile is configured.");
			assert_eq!(s.composer.read(cx).content(), "Retained manager draft");

			s.state = LoadState::Unavailable;

			cx.notify();
		});

		visual.update(|window, cx| window.draw(cx).clear());

		assert!(visual.debug_bounds("worker-stop").is_none(), "disconnected worker cannot stop");
	}

	#[gpui::test]
	fn unavailable_draft_remains_visible_editable_and_never_dispatches(
		cx: &mut gpui::TestAppContext,
	) {
		cx.update(crate::composer_input::bind_keys);

		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		let input = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.state = LoadState::Stale;

			s.snapshot.as_mut().unwrap().pending_events.clear();
			s.composer.update(cx, |input, cx| input.set_content("Saved", cx));

			s.attachments = vec![decodex_protocol::AgentAttachmentDto {
				path: ConversationWorkingDirectory::new("/tmp/retained.txt").unwrap(),
				image: false,
				skill_name: None,
			}];

			s.composer.clone()
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.), gpui::px(1_200.)));
			window.focus(&input.focus_handle(cx), cx);
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("recovery-draft-editor").is_some());

		visual.simulate_keystrokes("cmd-end space e d i t e d enter cmd-enter");

		surface.update(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Saved edited");
			assert_eq!(s.attachments.len(), 1);

			s.escape_interrupt(cx);
			s.escape_interrupt(cx);
			s.interrupt_current(cx);

			assert!(!s.sending);
			assert!(s.submission.command.is_none());
			assert!(s.interrupt_task.is_none());
			assert!(s.escape_stop.is_none());

			s.state = LoadState::Ready;

			assert_eq!(s.composer.read(cx).content(), "Saved edited");
			assert!(s.submission.command.is_none(), "reconnection cannot submit retained edits");
		});
	}

	#[gpui::test]
	fn disconnected_empty_workspace_and_uncertain_delivery_keep_sending_blocked(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.state = LoadState::Unavailable;

			s.composer.update(cx, |input, cx| input.set_content("Keep before connection", cx));

			assert!(s.composer_unavailable_reason().is_some());

			s.submit(cx);

			assert!(s.submission.command.is_none());

			s.state = LoadState::Ready;
			s.uncertain = true;

			s.composer.update(cx, |input, cx| input.set_content("Edited uncertain draft", cx));
			s.submit(cx);

			assert!(s.submission.command.is_none());
			assert_eq!(s.composer.read(cx).content(), "Edited uncertain draft");
		});
	}

	#[gpui::test]
	fn pages_reuse_identity_preserve_draft_and_return_to_agent(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep my draft", cx));

			s.workspace.graph_pan = (15.0, 25.0);
			s.workspace.graph_zoom = 1.2;

			s.open_page("verify", cx);
			s.open_page("verify", cx);

			assert_eq!(s.workspace.pages, vec!["verify"]);
			assert_eq!(s.selected.as_deref(), Some("verify"));
			assert!(s.history.as_ref().is_some_and(|(id, _)| id == "verify"));

			s.close_page("verify", cx);

			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert!(s.workspace.closing_pages.contains("verify"));
			assert_eq!(s.workspace.graph_pan, (15.0, 25.0));
			assert_eq!(s.workspace.graph_zoom, 1.2);
			assert_eq!(s.workspace.graph_scope.as_deref(), Some("release"));
			assert_eq!(s.composer.read(cx).content(), "Keep my draft");

			s.open_page("missing", cx);

			assert_eq!(s.selected.as_deref(), Some("agent"));
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
	}
	struct WorkspaceWithTabs(gpui::Entity<AgentSurface>);

	impl gpui::Render for WorkspaceWithTabs {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			gpui::div()
				.size_full()
				.flex()
				.flex_col()
				.child(self.0.update(cx, |surface, cx| surface.work_context(cx)))
				.child(self.0.clone())
		}
	}

	#[gpui::test]
	fn reopening_a_closing_tab_keeps_it_and_close_returns_to_main(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		let (_, visual) = cx.add_window_view(|_, _| WorkspaceWithTabs(surface.clone()));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.open_page("verify", cx);
		});
		visual.update(|w, cx| w.draw(cx).clear());
		surface.update(visual, |s, cx| s.close_page("verify", cx));
		visual.update(|w, cx| w.draw(cx).clear());
		surface.update(visual, |s, cx| s.open_page("verify", cx));
		visual.update(|w, cx| w.draw(cx).clear());
		surface.update(visual, |s, _| {
			assert_eq!(s.workspace.pages, vec!["verify"]);
			assert_eq!(s.selected.as_deref(), Some("verify"));
			assert!(s.workspace.closing_pages.is_empty());
		});
		surface.update(visual, |s, cx| s.close_page("verify", cx));
		visual.update(|w, cx| w.draw(cx).clear());

		// Wait only for deferred removal; visual smoothness is not a unit-test claim.
		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|w, cx| w.draw(cx).clear());
		visual.run_until_parked();
		surface.update(visual, |s, _| {
			assert!(s.workspace.pages.is_empty());
			assert_eq!(s.selected.as_deref(), Some("agent"));
		});
	}

	#[gpui::test]
	fn manager_switches_keep_drafts_with_their_recipient(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| work.id == "release")
				.unwrap()
				.kind = AgentWorkKindDto::Manager;

			s.composer.update(cx, |input, cx| input.set_content("Main Agent draft", cx));
			s.open_page("release", cx);

			assert_eq!(s.composer.read(cx).content(), "");

			s.composer.update(cx, |input, cx| input.set_content("Project Agent draft", cx));
			s.open_page("agent", cx);

			assert_eq!(s.composer.read(cx).content(), "Main Agent draft");

			s.open_page("release", cx);

			assert_eq!(s.composer.read(cx).content(), "Project Agent draft");
		});
	}

	#[gpui::test]
	fn graph_layers_dependencies_and_flags_cycles(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			let mut snapshot = s.snapshot.clone().expect("fixture has snapshot");
			let layout = graph::Layout::new(&snapshot, Some("release"));

			assert_eq!(layout.nodes.len(), 7);
			assert_eq!(layout.reports.len(), 6);
			assert_eq!(layout.edges.len(), 5);
			assert!(!layout.cyclic);

			let location =
				|id: &str| layout.nodes.iter().find(|n| n.id == id).expect("fixture node");

			assert_eq!(location("impact").x, location("improve").x);
			assert_ne!(location("impact").x, location("verify").x);

			for (a, b) in &layout.edges {
				assert!(layout.nodes[*a].y < layout.nodes[*b].y);
			}

			snapshot.dependencies.push(decodex_protocol::AgentDependencyDto {
				work_item_id: "flow".into(),
				depends_on_id: "ready".into(),
			});

			let cycle = graph::Layout::new(&snapshot, Some("release"));

			assert!(cycle.cyclic);
			assert_eq!(cycle.nodes.len(), 7);
			assert!(graph::Layout::new(&snapshot, Some("missing")).nodes.is_empty());
		});
	}
}
