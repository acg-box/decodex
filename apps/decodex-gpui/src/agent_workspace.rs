//! Conversation-first desktop presentation. All displayed work comes from the service.
use std::{mem, rc::Rc};

use gpui::{
	AnyElement, AppContext as _, Div, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
	PathBuilder, ScrollEvent, Stateful,
};
use graph::{Layout, Node};
use ui_theme::{
	AGENT_CHAT_OVERLAY, AGENT_SIDEBAR_MATERIAL, AMBER, BLUE, BODY_LINE_HEIGHT, BODY_SIZE,
	CHROME_CONTROL_SIZE, CONTROL_MARGIN, FONT_FAMILY, HOVER_FILL, SELECTED_HOVER_FILL, TEXT,
	TEXT_MUTED, TREE_ROW_HEIGHT,
};

// Pane controls can leave the dispatch tree after a layout change. Keep keyboard
// actions on the persistent shell instead of a detached control focus node.
pub(super) fn restore_shell_focus(window: &mut Window, cx: &mut gpui::App) {
	if let Some(shell) = window.root::<crate::shell::Shell>().flatten() {
		let focus = shell.read(cx).root_focus.clone();
		window.focus(&focus, cx);
	}
}

fn action_accessible_label(id: &str, label: &str) -> String {
	if id.starts_with("attention-") {
		format!("{label} pending decisions · open next")
	} else {
		match id {
			"new-project" => "New project",
			"graph-up" => "Parent work scope",
			"handoff-close" => "Collapse task details",
			"zoom-in" => "Zoom in",
			"zoom-out" => "Zoom out",
			_ => label,
		}
		.to_owned()
	}
}

#[cfg(test)] use crate::shell::agent_surface::ConversationWorkingDirectory;
#[cfg(any(test, feature = "visual-capture"))]
use crate::shell::agent_surface::{
	AgentRequestResult, AgentSnapshotDto, AgentSnapshotResult, ConversationReasoningEffort,
};
use crate::{
	shell::{
		WINDOW_CONTROLS_CLEARANCE,
		agent_surface::{
			AgentDispatchStateDto, AgentHistoryResult, AgentSurface, AgentWorkItemDto, Context,
			FluentBuilder, InteractiveElement, IntoElement, LoadState, ParentElement, Render, Role,
			SharedString, StatefulInteractiveElement, Styled, SubmitComposer, Window,
			activity::HistoryScrollAnchor, creation_setup, graph, prompts, ui_theme,
		},
		workspace_symbols,
	},
	ui_feedback::{self, Feedback, Tone},
	ui_loading,
	ui_motion::{self, SmoothControl, TabReveal},
};
#[cfg(any(test, feature = "visual-capture"))]
use decodex_protocol::{
	AgentDependencyDto,
	AgentDispatchStateDto::{Idle, Running},
	AgentWorkStatusDto::{Open, Resolved, UserDecision},
};
use decodex_protocol::{AgentWorkKindDto, DesktopRecoveredDraft};

const THREAD_LOCKED_MESSAGE: &str = "In use by another app";
const PARENT_MANAGED_MESSAGE: &str = "Continue in parent chat";

fn open_connection_settings(
	destination: crate::shell::Destination,
	window: &mut Window,
	cx: &mut gpui::App,
) {
	if let Some(shell) = window.root::<crate::shell::Shell>().flatten() {
		shell.update(cx, |shell, cx| shell.open_settings_window(destination, cx));
	}
	#[cfg(all(target_os = "macos", not(test)))]
	if let Some(parent) = super::native_composer::parent_window(window, cx) {
		cx.defer(move |cx| {
			let _ = parent
				.update(cx, |_, window, cx| open_connection_settings(destination, window, cx));
		});
	}
}

fn connection_recovery_notice(
	message: &'static str,
	destination: crate::shell::Destination,
) -> AnyElement {
	let (id, label) = if destination == crate::shell::Destination::Accounts {
		("connection-open-accounts", "Open Accounts")
	} else {
		("connection-open-diagnostics", "Open Diagnostics")
	};
	gpui::div()
		.id("conversation-unavailable")
		.debug_selector(|| "conversation-unavailable".into())
		.role(Role::Status)
		.aria_label(format!("Error: {message}"))
		.w_full()
		.min_w_0()
		.flex()
		.flex_col()
		.items_center()
		.gap(gpui::px(12.))
		.text_size(gpui::px(12.))
		.line_height(gpui::px(18.))
		.child(Tone::Error.icon())
		.child(
			gpui::div()
				.w_full()
				.text_center()
				.text_color(gpui::rgb(Tone::Error.color()))
				.child(message),
		)
		.child(
			gpui::div()
				.id(id)
				.debug_selector(move || id.into())
				.role(Role::Button)
				.aria_label(label)
				.tab_index(0)
				.cursor_pointer()
				.text_color(gpui::rgb(BLUE))
				.child(label)
				.on_click(move |_, window, cx| open_connection_settings(destination, window, cx))
				.on_key_down(move |event: &KeyDownEvent, window, cx| {
					if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
						open_connection_settings(destination, window, cx);
						cx.stop_propagation();
					}
				}),
		)
		.into_any_element()
}

fn account_connection_message(detail: &str) -> Option<&'static str> {
	let recovery = detail.strip_prefix("Agent account requires recovery: ");
	Some(match recovery {
		Some("RefreshQuota") =>
			"Account availability could not be confirmed. Check usage in Accounts. Your draft is saved.",
		Some("ConfigureFixedAccount") => "Select an account to continue. Your draft is saved.",
		Some("EnableAccount") => "Enable the selected account to continue. Your draft is saved.",
		Some("EnrollCredentials" | "RestoreProviderAgreement") =>
			"Sign in again in Accounts to continue. Your draft is saved.",
		Some("ResolveCredentialOperation" | "RepairCredentialStore") =>
			"Account setup needs attention. Open Accounts to continue. Your draft is saved.",
		Some("UpgradeCodex") => "This account requires a newer Codex runtime. Your draft is saved.",
		_ if detail.contains("account has reached its usage limit") =>
			"This account has reached its usage limit. Check Accounts for the reset time. Your draft is saved.",
		_ => return None,
	})
}

#[derive(Clone)]
pub(super) struct PageView {
	scope: Option<String>,
	selected: Option<String>,
	pan: (f32, f32),
	zoom: f32,
}

struct PanelTip(String);
impl Render for PanelTip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		crate::ui_motion::tooltip_surface(gpui::div())
			.px_2()
			.py_1()
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
		action_icon(["workspace-sidebar", "workspace-graph", "workspace-agents"][index])
			.expect("known panel glyph")
	}

	pub(crate) fn workspace_panels(&self) -> [(bool, bool); 3] {
		if self.workspace_connecting() {
			return [(false, false); 3];
		}
		[
			(self.workspace.sidebar_visible, true),
			(
				self.workspace.graph_visible && self.reserve_workspace_panels(),
				self.reserve_workspace_panels(),
			),
			(self.workspace.agent_tree_visible, self.reserve_workspace_panels()),
		]
	}

	pub(crate) fn toggle_workspace_sidebar(&mut self, cx: &mut Context<Self>) {
		if self.workspace_connecting() {
			return;
		}

		self.workspace.sidebar_visible = !self.workspace.sidebar_visible;
		self.workspace.sidebar_peek = false;
		self.workspace.sidebar_leave = None;

		cx.notify();
	}

	pub(crate) fn toggle_focused_content(&mut self, cx: &mut Context<Self>) {
		if self.workspace_connecting() {
			return;
		}

		if self.workspace.graph_expanded {
			self.toggle_graph_fullscreen(cx);
		} else if self.workspace.chat_expanded {
			self.toggle_chat_fullscreen(cx);
		} else if self.workspace.focused_panel == Some(super::workspace_size::Panel::Bottom)
			&& self.workspace.graph_visible
		{
			self.toggle_graph_fullscreen(cx);
		} else {
			self.toggle_chat_fullscreen(cx);
		}
	}

	pub(super) fn toggle_graph_fullscreen(&mut self, cx: &mut Context<Self>) {
		self.reveal_relations_after_resize();
		self.workspace.graph_expanded = !self.workspace.graph_expanded;
		self.workspace.chat_expanded = false;
		cx.notify();
	}

	pub(super) fn toggle_chat_fullscreen(&mut self, cx: &mut Context<Self>) {
		if self.workspace.graph_expanded {
			self.reveal_relations_after_resize();
		}
		self.workspace.focused_panel = None;
		self.workspace.chat_expanded = !self.workspace.chat_expanded;
		self.workspace.graph_expanded = false;
		cx.notify();
	}

	pub(crate) fn toggle_workspace_graph(&mut self, cx: &mut Context<Self>) {
		if self.workspace_connecting() {
			return;
		}

		if self.workspace.graph_expanded {
			self.reveal_relations_after_resize();
		}
		self.workspace.graph_visible = !self.workspace.graph_visible;
		self.workspace.graph_expanded = false;

		cx.notify();
	}

	// Unknown data must not collapse panels that the workspace intends to show.
	pub(super) fn reserve_workspace_panels(&self) -> bool {
		self.snapshot.as_ref().is_none_or(|snapshot| !snapshot.work_items.is_empty())
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
		if let Some((owner, thread)) = self.native_agents.pages.get(id).cloned() {
			self.open_native_agent(&owner, &thread, cx);
			return;
		}
		self.open_work_conversation(id, None, cx);
	}

	pub(super) fn open_work_conversation(
		&mut self,
		id: &str,
		thread: Option<&str>,
		cx: &mut Context<Self>,
	) {
		let Some(work) =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| w.id == id))
		else {
			return;
		};
		let thread = thread.map(str::to_owned).or_else(|| {
			(work.kind == AgentWorkKindDto::Task).then(|| work.codex_thread_id.clone()).flatten()
		});
		let editor_ready = thread.is_some()
			|| work.parent_goal_id.is_some() && work.kind != AgentWorkKindDto::Manager
			|| self.composer_manager.as_deref() == Some(id);
		// Explicit conversation navigation releases a previous graph inspection.
		// Keep the camera; the newly opened conversation supplies the card highlight.
		self.clear_relation_selection();
		self.keep_edited_preview(cx);
		self.workspace.browsing = false;
		self.workspace.closing_pages.remove(id);
		if self.selected.as_deref() == Some(id)
			&& self.native_agents.selected.as_ref().map(|(_, thread)| thread) == thread.as_ref()
			&& editor_ready
		{
			cx.notify();
			return;
		}
		self.close_native_agent(cx);
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

		self.handoffs.read_on_open = self
			.handoff_items()
			.into_iter()
			.find(|h| h.work == id && h.result)
			.map(|h| (h.work, h.key));
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
			});

			if !self.dock_open() {
				self.workspace.graph_scope = saved.scope;
				self.workspace.graph_selected = saved.selected;
				self.workspace.graph_pan = saved.pan;
				self.workspace.graph_zoom = saved.zoom;
			}
			// Dock visibility is global; conversation navigation only restores graph inspection.
			self.workspace.graph_expanded = false;
		}

		self.cancel_model_settings_read();
		self.reset_live_reviewer();
		self.reset_permission_profiles();
		self.reset_task_models();
		self.reset_hook_settings();
		self.reset_native_goal();
		self.clear_activity_detail();
		self.reset_recap();
		self.reset_sample();
		self.reset_prompt_edit();

		self.selected = Some(id.to_owned());
		self.workspace.connection_details_expanded = false;
		self.history = self.timeline.cache.get(id).cloned().map(|h| (id.to_owned(), h));
		self.workspace.details_visible = false;
		self.request = None;
		self.request_task = None;

		if let Some(thread) = thread {
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
					input.set_placeholder(prompts::session_quote(), cx);
				});
			}

			self.composer_manager = Some(id.into());
		}
	}

	fn close_page(&mut self, id: &str, cx: &mut Context<Self>) {
		if self.workspace.preview_page.as_deref() == Some(id) {
			self.workspace.preview_page = None;
		}
		self.workspace.closing_pages.insert(id.to_owned());

		if self.conversation_page().as_deref() == Some(id)
			&& let Some(root) = self.root_id()
		{
			self.open_page(&root, cx);
		}
		self.timeline.native_cache.retain(|(work, _), _| work != id);

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
		let accessible = action_accessible_label(&id, &label);
		let icon = action_icon(&id);
		let restores_focus = id.starts_with("graph-usage-");
		let icon_only = icon.is_some();
		let show_tip = is_tab || icon_only || id.starts_with("attention-");
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
			.on_click(cx.listener(move |s, _, window, cx| {
				if is_tree || is_tab {
					cx.stop_propagation();
				}
				action(s, cx);
				if restores_focus {
					restore_shell_focus(window, cx);
				}
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, window, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					keyboard(s, cx);
					if restores_focus {
						restore_shell_focus(window, cx);
					}

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
				crate::ui_motion::OverflowLabel {
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
			.gap_1();

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
				panel.child(self.sidebar_resize_handle(super::workspace_size::Panel::Left, cx))
			})
			.into_any_element()
	}

	pub(super) fn conversation_pages(&self) -> Vec<(String, String, bool)> {
		let mut pages = vec![(self.root_id().unwrap_or_default(), "Main".into(), false)];
		let open = &self.workspace.pages;
		if let Some(snapshot) = &self.snapshot {
			pages.extend(open.iter().filter(|id| Some(*id) != self.root_id().as_ref()).filter_map(
				|id| {
					self.native_page_label(id)
						.or_else(|| {
							snapshot
								.work_items
								.iter()
								.find(|w| &w.id == id)
								.map(|w| self.work_label(w))
						})
						.map(|label| (id.clone(), label, true))
				},
			));
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
				.when(self.workspace.preview_page.as_deref() == Some(&id), |tab| {
					tab.tooltip(preview_tip)
				})
				.tab_index(0)
				.w_full()
				.min_w_0()
				.h(gpui::px(ui_theme::CONVERSATION_TAB_SIZE))
				.flex_none()
				.overflow_hidden()
				.flex()
				.items_center()
				.rounded(gpui::px(7.))
				.cursor_pointer()
				.bg(gpui::rgba(if active { 0xffffff10 } else { 0xffffff00 }))
				.hover(move |s| s.bg(gpui::rgba(if active { 0xffffff18 } else { HOVER_FILL })))
				.on_click(cx.listener(move |s, event: &gpui::ClickEvent, _, cx| {
					if event.click_count() == 2
						&& s.workspace.preview_page.as_deref() == Some(&row_select)
					{
						s.workspace.preview_page = None;
					}
					s.open_page(&row_select, cx);
				}))
				.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
					if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str())
					{
						s.open_page(&keyboard_select, cx);
						cx.stop_propagation();
					}
				}))
				.child(crate::ui_motion::AgentRailStatus {
					id: id.clone(),
					state: self.conversation_status(&id),
					label: label.clone(),
				});
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
						.mr(gpui::px(6.))
						.flex_none()
						.flex()
						.items_center()
						.justify_center()
						.opacity(if active { 0.65 } else { 0. })
						.group_hover(group, |s| s.opacity(0.65))
						.focus(|s| s.opacity(1.))
						.hover(|s| s.opacity(1.))
						.active(|s| s.opacity(0.45))
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
				list = list.child(tab.mb(gpui::px(ui_theme::CONVERSATION_TAB_GAP)));
			}
		}
		list.into_any_element()
	}

	pub(crate) fn workspace_connecting(&self) -> bool {
		// Before the first snapshot, there is no workspace state to present as empty
		// or disconnected. Use the same loading surface throughout service startup.
		(self.snapshot.is_none()
			&& matches!(
				self.displayed_load_state(),
				LoadState::Idle | LoadState::Loading | LoadState::Unavailable
			))
			|| self.connection_initializing()
	}

	pub(super) fn connection_initializing(&self) -> bool {
		self.snapshot.as_ref().is_some_and(|s| s.connection_initializing)
	}

	pub(super) fn composer_unavailable_reason(&self) -> Option<&'static str> {
		if self.native_agents.selected.is_some() {
			return matches!(
				self.native_agents.connection,
				super::native_agents::NativeConnection::ParentManaged
			)
			.then_some(PARENT_MANAGED_MESSAGE);
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
		if !self.draft_owner_available() {
			return Some(
				"This draft's conversation is unavailable. Select a conversation before sending.",
			);
		}

		let selected = self.selected.as_deref()?;
		let snapshot = self.snapshot.as_ref()?;
		let root = self.root_id();

		for event in &snapshot.pending_events {
			if event.work_item_id != selected && Some(&event.work_item_id) != root.as_ref() {
				continue;
			}

			let reason = match event.event_kind.as_str() {
				"thread_in_use_needs_attention" if event.work_item_id == *selected =>
					THREAD_LOCKED_MESSAGE,
				"thread_in_use_needs_attention" => continue,
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

	pub(super) fn connection_notice(
		&self,
		reason: &'static str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		if reason == PARENT_MANAGED_MESSAGE {
			return Tone::Neutral
				.composer_row(
					"conversation-unavailable",
					PARENT_MANAGED_MESSAGE,
					workspace_symbols::icon_sized(workspace_symbols::Symbol::AccountRoute, 14.),
				)
				.child(self.workspace_action(
					"native-open-parent".into(),
					"Open parent conversation".into(),
					|s, cx| s.open_native_parent(cx),
					cx,
				))
				.into_any_element();
		}
		if reason == THREAD_LOCKED_MESSAGE {
			return Tone::Warning
				.composer_row(
					"conversation-unavailable",
					THREAD_LOCKED_MESSAGE,
					workspace_symbols::icon_sized(workspace_symbols::Symbol::Lock, 14.),
				)
				.into_any_element();
		}

		let detail = self.connection_failure_detail();
		if !self.uncertain
			&& let Some(message) = detail.and_then(account_connection_message)
		{
			return connection_recovery_notice(message, crate::shell::Destination::Accounts);
		}
		let title = if self.uncertain {
			"Delivery unconfirmed · Sending paused"
		} else {
			match detail {
				Some(text) if text.contains("ProcessUnavailable") =>
					"Codex unavailable · Retrying automatically",
				Some(text) if text.contains("RefreshQuota") || text.contains("usage limit") =>
					"Account unavailable · Check Settings → Accounts",
				Some(text) if text.contains("SelectWorkingDirectory") =>
					"Project folder unavailable · Restore folder access",
				_ if reason.contains("retry automatically") =>
					"Connection lost · Retrying automatically",
				_ => "Connection unavailable · Sending paused",
			}
		};

		Tone::Warning
			.row("conversation-unavailable", title)
			.debug_selector(|| "conversation-unavailable".into())
			.flex_wrap()
			.when(detail.is_some(), |d| {
				d.child(
					gpui::div().flex().items_center().child(
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
					.w_full()
					.pt(gpui::px(8.))
					.text_size(gpui::px(11.))
					.line_height(gpui::px(17.))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(format!("{reason}\n{}", detail.unwrap_or_default())),
			))
			.into_any_element()
	}

	pub(super) fn composer_footer(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
		if self.native_agents.selected.is_some() {
			let feedback = self.native_feedback();
			return gpui::div()
				.flex_none()
				.w_full()
				.child(ui_feedback::notice(
					"conversation-feedback",
					self.feedback_scope(),
					(!matches!(feedback.message.as_str(), "Sent" | "Sending…"))
						.then(|| feedback.clone()),
				))
				.child(self.render_composer(window, cx));
		}

		let saved_inputs = self
			.conversation_work()
			.filter(|work| self.saved_inputs_in_composer(work))
			.map(|work| self.saved_input_rows(&work, cx))
			.unwrap_or_default();
		let has_saved_inputs = !saved_inputs.is_empty();
		// The parent overlays this transparent footer above the scrolling conversation.
		gpui::div().debug_selector(|| "composer-footer".into()).flex_none().w_full().child(
			gpui::div()
				.w_full()
				.flex()
				.flex_col()
				.child(self.conversation_activity(cx))
				.child(self.recovered_draft_panel(cx))
				.when(has_saved_inputs, |footer| {
					footer.child(
						gpui::div()
							.debug_selector(|| "composer-saved-inputs".into())
							.w_full()
							.px(gpui::px(ui_theme::CONVERSATION_INSET))
							.pb(gpui::px(ui_theme::MESSAGE_GAP - ui_theme::COMPOSER_TOP_GAP))
							.flex()
							.justify_center()
							.child(
								gpui::div()
									.id("composer-saved-inputs-scroll")
									.w_full()
									.max_w(gpui::px(ui_theme::CONVERSATION_WIDTH))
									.max_h(window.viewport_size().height * 0.35)
									.overflow_y_scroll()
									.flex()
									.flex_col()
									.gap_2()
									.children(saved_inputs),
							),
					)
				})
				.child(
					gpui::div()
						.debug_selector(|| "composer-input-region".into())
						.child(self.render_composer(window, cx)),
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

	pub(super) fn render_transcript_view(
		&mut self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		self.prepare_transcript_history(window, cx);
		let selected = self.conversation_work();
		let is_agent = self.native_agents.selected.is_some() || self.selected_is_manager();
		self.workspace_transcript(selected.as_ref(), is_agent, window, cx)
	}

	fn conversation_owns_requests(&self, work: &AgentWorkItemDto) -> bool {
		self.native_agents.selected.as_ref().is_none_or(|(owner, thread)| {
			owner == &work.id
				&& self.snapshot.as_ref().is_some_and(|snapshot| {
					snapshot.work_items.iter().any(|canonical| {
						canonical.id == work.id
							&& canonical.codex_thread_id.as_ref() == Some(thread)
					})
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
				.justify_center()
				.pb(gpui::px(self.workspace.composer_overlay_height))
				.gap_2()
				.child(gpui::div().text_size(gpui::px(20.)).child("What would you like to do?"))
				.child(self.workspace_choices(true, cx))
				.into_any_element();
		}

		let scroll = self
			.timeline
			.scroll
			.entry(self.selected.clone().unwrap_or_default())
			.or_default()
			.clone();
		let voice_min_height = self.animate_voice_reading_space(&scroll, window, cx);
		let mut transcript = gpui::div()
			.debug_selector(|| "workspace-transcript".into())
			.id(SharedString::from(format!(
				"transcript-{}",
				self.selected.as_deref().unwrap_or("agent")
			)))
			// The viewport owns its size; history must not participate in the
			// surrounding flex containers' intrinsic-size measurements.
			.absolute()
			.inset_0()
			.min_w_0()
			.min_h_0()
			.overflow_hidden()
			.track_scroll(&scroll)
			.on_scroll(cx.listener(|s, event, _, cx| s.scroll_history(event, cx)));

		if let Some(work) = selected {
			if let Some(snapshot) = &self.snapshot {
				let content = if is_agent {
					gpui::div()
						.debug_selector(|| "conversation-content".into())
						.min_h(gpui::px(voice_min_height))
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
						.when(self.conversation_owns_requests(work), |content| {
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
								.child(self.sample_panel(&work.id, cx))
						})
						.into_any_element()
				} else {
					self.details(snapshot, work, cx).into_any_element()
				};

				transcript = transcript.child(self.anchored_native_content(content));
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

		let fill_owner = cx.entity().downgrade();
		let fill_key = self.selected.clone();
		// Measure after layout, including folded process rows and the composer inset.
		let fill_viewport =
			move |_: Vec<gpui::Bounds<gpui::Pixels>>, _: &mut Window, cx: &mut gpui::App| {
				let Some(entity) = fill_owner.upgrade() else {
					return;
				};
				let state = entity.read(cx);
				if state.selected != fill_key || !state.history_viewport_fill_ready() {
					return;
				}
				let owner = fill_owner.clone();
				let key = fill_key.clone();
				cx.defer(move |cx| {
					let _ = owner.update(cx, |s, cx| {
						if s.selected == key
							&& key.as_ref().is_some_and(|key| s.history_viewport_underfilled(key))
						{
							s.prefetch_older_history(cx);
						}
					});
				});
			};
		gpui::div()
			.on_children_prepainted(fill_viewport)
			.size_full()
			.flex_1()
			.min_h_0()
			.min_w_0()
			.relative()
			.flex()
			.child(self.anchored_native_viewport(transcript.into_any_element(), scroll.clone()))
			.when(is_agent, |viewport| viewport.children(self.native_pagination_feedback(cx)))
			.into_any_element()
	}

	pub(super) fn render_workspace(
		&mut self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		self.poll_native_agents(cx);
		let connecting = self.workspace_connecting();
		self.workspace.arrival.observe(!connecting);
		if self.workspace.arrival.progress(0.12) < 1. && !connecting {
			ui_motion::request_frame(window, cx);
		}
		if connecting {
			let unavailable =
				matches!(self.displayed_load_state(), LoadState::Unavailable | LoadState::Stale);
			return gpui::div()
				.debug_selector(|| "workspace-connecting".into())
				.size_full()
				.py(gpui::px(WINDOW_CONTROLS_CLEARANCE))
				.bg(gpui::rgba(AGENT_SIDEBAR_MATERIAL))
				.flex()
				.items_center()
				.justify_center()
				.child(if unavailable {
					gpui::div()
						.w_full()
						.min_w_0()
						.max_w(gpui::px(440.))
						.px(gpui::px(ui_theme::CONVERSATION_INSET))
						.child(connection_recovery_notice(
							"Could not connect to Codex. Your history and drafts are saved.",
							crate::shell::Destination::Health,
						))
						.into_any_element()
				} else {
					crate::ui_loading::workspace().into_any_element()
				})
				.into_any_element();
		}
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
			.opacity(self.workspace.arrival.progress(0.12))
			.flex_1()
			.min_w_0()
			.min_h_0()
			.overflow_hidden()
			.flex()
			.flex_col()
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
		chat = chat.child(
			gpui::div()
				.flex_1()
				.min_h_0()
				.flex()
				.relative()
				// Repaint the current transcript geometry. Whole-scene replay can
				// retain old pixels during scrolling and native panel transitions.
				// Markdown shaping and history preparation retain their own caches.
				.child(self.transcript_presentation.clone())
				.when(!self.is_new_conversation(), |row| {
					row.child(self.history_rail_slot(window, cx))
				})
				.child(self.latest_button(window, cx)),
		);

		if floating_composer && !self.workspace.browsing {
			chat = chat.child(self.floating_composer(window, cx));
		} else if !is_agent && let Some(work) = selected.as_ref() {
			chat = chat
				.child(self.recovered_draft_panel(cx))
				.child(self.conversation_activity(cx))
				.child(self.workspace_followup(work, cx));
		}

		chat = chat.child(
			gpui::div()
				.absolute()
				.top(gpui::px(6.))
				.right(gpui::px(8.))
				.occlude()
				.child(self.panel_controls(super::panel_controls::Panel::Conversation, cx)),
		);

		let chat = self.workspace_details_overlay(chat, selected.as_ref(), window, cx);
		self.workspace_frame(chat.into_any_element(), wide, window, cx)
	}

	fn floating_composer(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let owner = cx.entity();
		gpui::div()
			.debug_selector(|| "floating-composer".into())
			.absolute()
			.left_0()
			.right_0()
			.bottom_0()
			// Expanded maps place this footer outside the canvas. Continue the
			// same surface to the window edge behind the floating glass editor.
			.when(self.workspace.graph_expanded, |footer| footer.bg(gpui::rgba(AGENT_CHAT_OVERLAY)))
			.child(self.composer_footer(window, cx))
			.child(
				gpui::canvas(
					move |bounds, _, cx| {
						let height = f32::from(bounds.size.height);
						if (owner.read(cx).workspace.composer_overlay_height - height).abs() > 0.5 {
							let owner = owner.clone();
							cx.defer(move |cx| {
								owner.update(cx, |s, cx| {
									s.workspace.composer_overlay_height = height;
									cx.notify();
								})
							});
						}
					},
					|_, _, _, _| {},
				)
				.absolute()
				.inset_0(),
			)
	}

	fn workspace_frame(
		&mut self,
		chat: AnyElement,
		wide: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let (graph_width, graph_height) = self.workspace_graph_size(window, wide);
		let graph_height = if self.workspace.graph_expanded && self.graph_has_composer() {
			(graph_height - self.workspace.composer_overlay_height).max(120.)
		} else {
			graph_height
		};

		self.update_graph_inset(graph_width - 24., (graph_height - 54.).max(0.));
		let displayed_height = ui_motion::direct_value(
			"execution-surface-height",
			graph_height,
			self.workspace
				.panel_drag
				.is_some_and(|(panel, _, _)| panel == super::workspace_size::Panel::Bottom),
			window,
			cx,
		);
		// The top panel owns its space. Chat follows it in normal layout flow.
		let center = gpui::div()
			.relative()
			.flex_1()
			.min_w_0()
			.h_full()
			.flex()
			.flex_col()
			.child(
				gpui::div()
					.flex_none()
					.overflow_hidden()
					.w_full()
					.h(gpui::px(displayed_height))
					.opacity(self.workspace.arrival.progress(0.06))
					.when(displayed_height > 0.1, |panel| panel.child(self.handoff_canvas(cx))),
			)
			.when(!self.workspace.graph_expanded, |center| center.child(chat))
			.when(self.workspace.graph_expanded && self.graph_has_composer(), |center| {
				center.child(self.floating_composer(window, cx))
			});

		let right_width = ui_motion::direct_value(
			"agent-tree-panel",
			self.agent_tree_width(window),
			self.workspace
				.panel_drag
				.is_some_and(|(panel, _, _)| panel == super::workspace_size::Panel::Right),
			window,
			cx,
		);
		let body = gpui::div().flex_1().min_h_0().flex().overflow_hidden().child(center).child(
			gpui::div()
				.flex_none()
				.w(gpui::px(right_width))
				.h_full()
				.overflow_hidden()
				.opacity(self.workspace.arrival.progress(0.08))
				.when(right_width > 0.1, |panel| panel.child(self.agent_tree(right_width, cx))),
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
			.child(body);

		self.workspace_resize_root(cx)
			.size_full()
			.relative()
			.bg(gpui::rgba(AGENT_SIDEBAR_MATERIAL))
			.flex()
			.text_size(gpui::px(BODY_SIZE))
			.font_family(FONT_FAMILY)
			.text_color(gpui::rgb(TEXT))
			.on_action(cx.listener(|_, _: &SubmitComposer, _, cx| cx.stop_propagation()))
			.child(self.sidebar_slot(wide, window, cx))
			.child(main)
			.when(self.workspace.arrival.progress(0.) < 1., |root| {
				root.child(
					gpui::div()
						.absolute()
						.inset_0()
						.debug_selector(|| "workspace-arrival-overlay".into())
						.opacity(1. - self.workspace.arrival.progress(0.))
						.pt(gpui::px(WINDOW_CONTROLS_CLEARANCE))
						.flex()
						.items_center()
						.justify_center()
						.child(crate::ui_loading::loading("")),
				)
			})
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
						.left(gpui::px(12.))
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
		} else {
			self.prepare_history_marks();
		}
	}

	fn prepare_transcript_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.is_new_conversation() {
			return;
		}
		if self.timeline.older_scroll_anchor.is_none() {
			self.prefetch_older_history(cx);
		}

		self.restore_history_anchor(window, cx);
		self.prepare_history_layout(window);
		self.animate_history_scroll(window, cx);
		self.follow_voice_scroll(window, cx);

		if self.timeline.latest_follow_work == self.selected
			&& let Some(scroll) =
				self.selected.as_ref().and_then(|work| self.timeline.scroll.get(work))
		{
			// GPUI resolves this against the new content height before painting.
			// Chasing last frame's maximum makes each streamed line fall and rebound.
			// Explicit navigation keeps its own animation in animate_history_scroll.
			scroll.scroll_to_bottom();
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
			"Discuss this work →".into(),
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

		work.title.clone()
	}

	pub(super) fn workspace_dependency_graph(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut panel = gpui::div().w_full().h_full().min_h_0().flex().flex_col();
		let Some(snapshot) = &self.snapshot else {
			return panel.into_any_element();
		};
		let layout = self.workspace_graph_layout();
		let mut area = self.graph_canvas(&layout, cx);
		if layout.nodes.is_empty() {
			area = area.child(
				gpui::div()
					.p_3()
					.text_size(gpui::px(12.))
					.text_color(gpui::rgb(TEXT_MUTED))
					.child("No dependencies are recorded for this task."),
			);
		}

		for node in &layout.nodes {
			let Some(work) = snapshot.work_items.iter().find(|w| w.id == node.id) else {
				continue;
			};

			area = area.child(self.graph_node(node, work, cx));
		}

		panel = panel.child(area);

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
						"Fit".into(),
						|s, cx| {
							s.workspace.graph_zoom = s.workspace.graph_fit_zoom;
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

				let blocked = self.snapshot.as_ref().is_some_and(|snapshot| {
					snapshot.work_items.iter().find(|w| w.id == b.id).is_some_and(|work| {
						graph::blockers(snapshot, work).iter().any(|w| w.id == a.id)
					})
				});
				let (width, height) = self.graph_node_size(&a.id);
				let (_, target_height) = self.graph_node_size(&b.id);
				((a.x + width, a.y + height / 2.), (b.x, b.y + target_height / 2.), report, blocked)
			})
			.collect();

		gpui::div()
			.h_0()
			.id("work-graph-canvas")
			.debug_selector(|| "work-graph-canvas".into())
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
			.on_scroll(cx.listener(|s, event: &ScrollEvent, _, cx| {
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
					&& s.workspace.panel_drag.is_none()
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
						for (a, b, report, blocked) in edges {
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

							let path = graph_edge_path(start, end, report, blocked);

							if let Ok(path) = path.build() {
								window.paint_path(
									path,
									gpui::rgb(if blocked {
										AMBER
									} else if report {
										BLUE
									} else {
										TEXT_MUTED
									}),
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
		if self.handoffs.focus.as_deref() == Some(&node.id) {
			return self.expanded_graph_node(node, work, cx);
		}

		let zoom = self.workspace.graph_display_zoom;
		let pan = (
			self.workspace.graph_pan.0 + self.workspace.graph_inset.0,
			self.workspace.graph_pan.1 + self.workspace.graph_inset.1,
		);
		let id = work.id.clone();
		let key = id.clone();
		let state =
			super::dock::progress_state(self.snapshot.as_ref().expect("graph snapshot"), work);
		let status = self.graph_node_caption(work);
		let color = state.color;
		let tip = format!("{} · {}\n{}", self.work_label(work), status, state.reason);

		let element = gpui::div()
			.id(SharedString::from(format!("graph-node-{id}")))
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!("{}: {status}. Show task details.", self.work_label(work)))
			.debug_selector({
				let key = work.id.clone();
				move || format!("graph-node-{key}")
			})
			.tooltip(move |_, cx| cx.new(|_| PanelTip(tip.clone())).into())
			.absolute()
			.left(gpui::px(node.x * zoom + pan.0))
			.top(gpui::px(node.y * zoom + pan.1))
			.w(gpui::px(190.0 * zoom))
			.h(gpui::px((66.0 * zoom).max(42.)))
			.border_1()
			.border_color(
				gpui::rgb(
					if self.handoffs.focus.as_ref().or(self.workspace.dock_record.as_ref())
						== Some(&id)
					{
						TEXT
					} else {
						color
					},
				)
				.opacity(if state.group == 3 { 0.25 } else { 0.7 }),
			)
			.px_2()
			.py(gpui::px(3.))
			.rounded(gpui::px(9.0))
			.bg(
				if self.handoffs.focus.as_ref().or(self.workspace.dock_record.as_ref()) == Some(&id)
				{
					gpui::rgba(0x35353ce8)
				} else {
					gpui::rgba(0x242427db)
				},
			)
			.hover(|style| style.bg(gpui::rgba(0x3a3a40eb)))
			.cursor_pointer()
			.overflow_hidden()
			.text_size(gpui::px((12.0 * zoom).max(10.0)))
			.line_height(gpui::px(14.))
			.on_click(cx.listener(move |s, _, _, cx| s.activate_graph_node(&id, cx)))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if event.keystroke.key == "enter" || event.keystroke.key == "space" {
					s.activate_graph_node(&key, cx);
					cx.stop_propagation();
				}
			}))
			.flex()
			.flex_col()
			.justify_center()
			.gap(gpui::px(2.))
			.child(gpui::div().whitespace_nowrap().text_ellipsis().child(self.work_label(work)))
			.child(
				gpui::div()
					.text_size(gpui::px((11.0 * zoom).max(10.0)))
					.text_color(gpui::rgb(color))
					.whitespace_nowrap()
					.text_ellipsis()
					.child(status),
			);

		element.into_any_element()
	}
}

fn graph_edge_path(
	start: gpui::Point<gpui::Pixels>,
	end: gpui::Point<gpui::Pixels>,
	report: bool,
	blocked: bool,
) -> PathBuilder {
	let mut path = PathBuilder::stroke(gpui::px(if blocked { 2. } else { 1. }));
	path.move_to(start);
	path.cubic_bezier_to(
		end,
		gpui::point((start.x + end.x) * 0.5, start.y),
		gpui::point((start.x + end.x) * 0.5, end.y),
	);
	if !report {
		path.move_to(end + gpui::point(gpui::px(-5.), gpui::px(-3.)));
		path.line_to(end);
		path.line_to(end + gpui::point(gpui::px(-5.), gpui::px(3.)));
	}
	path
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	pub(crate) fn visual_workspace_page(&mut self, page: &str, cx: &mut Context<Self>) {
		if page == "deliveries" {
			self.visual_delivery_fixture(cx);
			return;
		}
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
			"workbench" | "workbench-agent" | "workbench-history" | "workbench-split" =>
				self.visual_workbench_page(page, cx),
			"factory" | "relations" | "relations-resource" => {
				self.work_board.graph = false;
				self.workspace.graph_expanded = true;
				if page.starts_with("relations") {
					self.visual_relation_evidence();
					if page == "relations-resource" {
						self.visual_relation_resource_focus();
					}
				}
			},
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
					archived_work_ids: Default::default(),
					agent_slots: Default::default(),
					context_references: vec![],
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
			"dock-execution" => self.visual_execution_dock(cx),
			"dock-execution-failed" => self.visual_execution_failure(cx),
			"dock-compact" | "dock-running" | "dock-result" | "dock-completed"
			| "dock-dependencies" | "dock-blocked" => self.visual_dock_page(page, cx),
			"expanded" => self.workspace.graph_expanded = true,
			"chat-expanded" => self.workspace.chat_expanded = true,
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
			"mermaid" => self.visual_mermaid_page(cx),
			#[cfg(feature = "visual-capture")]
			"mermaid-scroll" => self.visual_mermaid_scroll_page(cx),
			"task-references" => self.visual_task_references(),
			"conversation" => {
				self.workspace.graph_visible = false;
			},
			_ => {},
		}

		cx.notify();
	}

	fn visual_mermaid_page(&mut self, cx: &mut Context<Self>) {
		self.visual_workspace_page("markdown", cx);
		let source = std::env::var("DECODEX_VISUAL_MERMAID").unwrap_or_else(|_| {
			"flowchart LR\nA([Request]) --> B{Valid?}\nB -->|Yes| C[(Database)]\nB -->|No| D[Retry]\nD --> A".into()
		});
		if let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut self.history {
			for entry in entries.iter_mut().filter(|entry| entry.kind == "assistant") {
				entry.text = format!(
					"## Mermaid preview\n\n```mermaid\n{source}\n```\n\nThe diagram keeps its original source for copying."
				);
			}
		}
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
			self.composer_menu_content = Some("model");
		}
	}

	/// Explicit capture fixture. Never installed by the normal application path.
	pub(crate) fn visual_workspace_fixture(&mut self, cx: &mut Context<Self>) {
		self.work_board.graph = true;
		self.handoffs.fixture();
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
			archived_work_ids: Default::default(),
			agent_slots: Default::default(),
			context_references: vec![],
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
						.gap(gpui::px(2.))
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
			.py(gpui::px(3.))
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

	pub(super) fn feedback_scope(&self) -> SharedString {
		format!(
			"{:?}:{:?}:{:?}",
			self.selected, self.native_agents.selected, self.workspace.new_conversation
		)
		.into()
	}

	fn conversation_activity(&self, cx: &mut Context<Self>) -> AnyElement {
		ui_feedback::notice(
			"conversation-feedback",
			self.feedback_scope(),
			self.conversation_feedback(),
		)
		.action(self.can_keep_both_drafts().then(|| self.keep_both_draft_button(cx)))
		.into_any_element()
	}

	fn conversation_feedback(&self) -> Option<Feedback> {
		if self.connection_initializing()
			|| (self.composer_unavailable_reason().is_some()
				&& self.draft_storage_notice().is_none())
		{
			return None;
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
			return None;
		}

		if self.uncertain {
			Some(Feedback::warning(
				"Delivery unconfirmed · Draft kept. Sending is paused to avoid duplicates.",
			))
		} else if let Some(notice) = self.draft_storage_notice() {
			Some(Feedback::error(notice))
		} else if self.selected.as_deref().is_some_and(|id| self.thread_in_use(id)) {
			Some(Feedback::warning("In use by another app · Your draft is kept"))
		} else if pending.iter().any(|event| event.event_kind.ends_with("_failed")) {
			Some(Feedback::error("The current operation could not complete."))
		} else if pending.iter().any(|event| event.event_kind.ends_with("_needs_attention")) {
			Some(Feedback::warning("This conversation needs attention."))
		} else if matches!(self.displayed_load_state(), LoadState::Unavailable | LoadState::Stale) {
			Some(Feedback::warning("Connection unavailable · Reconnecting"))
		} else if compacting {
			Some(Feedback::loading("Compacting context"))
		} else if !self.feedback.is_empty()
			&& self.feedback.message != "Message saved · Waiting for agent…"
		{
			Some(self.feedback.clone())
		} else {
			selected.and_then(|work| match work.dispatch_state {
				AgentDispatchStateDto::Unknown =>
					Some(Feedback::warning("Connection interrupted · Checking delivery")),
				_ => None,
			})
		}
		.or_else(|| notice.as_ref().map(|(_, detail, _)| Feedback::error(detail)))
		.or_else(|| {
			(!self.archive.feedback.is_empty()).then(|| Feedback::neutral(&self.archive.feedback))
		})
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

			snapshot.workspaces.push(decodex_protocol::WorkspaceDto {
				id: "release-folder".into(),
				work_ids: vec!["release".into()],
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

pub(super) fn clock_label(micros: i64) -> String {
	let seconds = micros / 1_000_000;

	format!("{:02}:{:02}:{:02}", (seconds / 3_600) % 24, (seconds / 60) % 60, seconds % 60)
}

fn action_icon(id: &str) -> Option<AnyElement> {
	let symbol = match id {
		"workspace-sidebar" => crate::shell::workspace_symbols::Symbol::Sidebar,
		"workspace-graph" => crate::shell::workspace_symbols::Symbol::Graph,
		"workspace-agents" => crate::shell::workspace_symbols::Symbol::Agents,
		"tree-close" => crate::shell::workspace_symbols::Symbol::Close,
		"graph-up" | "graph-home" => crate::shell::workspace_symbols::Symbol::Back,
		"board-show-graph" => crate::shell::workspace_symbols::Symbol::Graph,
		"board-show-list" => crate::shell::workspace_symbols::Symbol::AllWork,
		"dock-open-source" | "native-open-parent" =>
			crate::shell::workspace_symbols::Symbol::Forward,
		"dock-evidence-toggle" => crate::shell::workspace_symbols::Symbol::Eye,
		"dock-subtasks" => crate::shell::workspace_symbols::Symbol::Agents,
		"zoom-in" => crate::shell::workspace_symbols::Symbol::Plus,
		"zoom-out" => crate::shell::workspace_symbols::Symbol::Minus,
		id if id.starts_with("close-") => crate::shell::workspace_symbols::Symbol::Close,
		_ => return None,
	};

	Some(workspace_symbols::icon(symbol))
}

fn preview_tip(_: &mut Window, cx: &mut gpui::App) -> gpui::AnyView {
	cx.new(|_| crate::shell::ControlTooltip("Preview · Double-click to keep open")).into()
}

#[cfg(test)]
mod tests {
	#[gpui::test]
	fn fullscreen_controls_keep_shift_escape_working_after_clicks(cx: &mut gpui::TestAppContext) {
		cx.update(crate::shell::bind_keys);
		let (shell, visual) = cx.add_window_view(|window, cx| {
			crate::shell::Shell::new(window, cx, crate::shell::ConnectionView::Stopped)
		});
		shell.update(visual, |s, cx| {
			s.agent.update(cx, |agent, cx| {
				agent.visual_workspace_fixture(cx);
				agent.visual_workspace_page("workbench-split", cx);
			})
		});
		visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1000.)));
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(std::time::Duration::from_millis(260));
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		let dock = visual.debug_bounds("graph-expand").expect("Dock panel control");
		let chat = visual.debug_bounds("chat-expand").expect("conversation panel control");
		assert_eq!(dock.size, chat.size, "pane controls share the same click target");
		let group = visual.debug_bounds("relations-pane-controls").unwrap();
		let close = visual.debug_bounds("graph-close").unwrap();
		assert!(dock.right() < close.left(), "pane actions have separate click targets");
		assert_eq!(group.right(), close.right(), "no extra outer frame gutter");
		for control in ["graph-expand", "chat-expand"] {
			let button = visual.debug_bounds(control).expect("fullscreen control");
			let agent = shell.read_with(visual, |s, _| s.agent.clone());
			visual.simulate_mouse_move(button.center(), None, Default::default());
			visual.simulate_click(button.center(), Default::default());
			std::thread::sleep(std::time::Duration::from_millis(260));
			visual.run_until_parked();
			visual.update(crate::ui_frame::draw_frame);
			assert_eq!(
				agent.read_with(visual, |a, _| (
					a.workspace.graph_expanded,
					a.workspace.chat_expanded
				)),
				(control == "graph-expand", control == "chat-expand"),
				"{control} expands the selected pane"
			);
			if control == "graph-expand" {
				for selector in ["relations-usage", "graph-usage-time", "relations-out"] {
					let bounds = visual.debug_bounds(selector).expect(selector);
					visual.simulate_mouse_move(bounds.center(), None, Default::default());
					visual.simulate_click(bounds.center(), Default::default());
					visual.run_until_parked();
					visual.update(crate::ui_frame::draw_frame);
				}
				assert!(visual.debug_bounds("graph-usage-time").is_none(), "menu closed");
				assert!(
					visual.update(|window, cx| shell.read(cx).root_focus.is_focused(window)),
					"canvas controls retain persistent keyboard focus"
				);
			}
			visual.simulate_keystrokes("shift-escape");
			std::thread::sleep(std::time::Duration::from_millis(260));
			visual.run_until_parked();
			visual.update(crate::ui_frame::draw_frame);
			assert_eq!(
				agent.read_with(visual, |a, _| (
					a.workspace.graph_expanded,
					a.workspace.chat_expanded
				)),
				(false, false),
				"{control} shortcut restores layout"
			);
			assert!(visual.debug_bounds("chat-expand").is_some(), "{control} restores chat");
		}
	}

	use std::thread;

	use gpui::{AppContext as _, Focusable as _};

	use crate::shell::agent_surface::workspace::{
		AgentHistoryResult, AgentSurface, Context, ConversationWorkingDirectory, IntoElement,
		LoadState, Render, Window, graph,
	};
	use decodex_protocol::AgentWorkKindDto;

	#[gpui::test]
	fn workspace_animation_does_not_publish_navigation_or_draft_changes(
		cx: &mut gpui::TestAppContext,
	) {
		let (model, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
		visual.update(|window, cx| {
			let notifications = notifications.clone();
			cx.observe(&model, move |_, _| notifications.set(notifications.get() + 1)).detach();
			window.draw(cx).clear(cx);
			assert!(window.simulate_next_frame(cx) > 0);
		});
		cx.run_until_parked();
		assert_eq!(notifications.get(), 0);
	}

	struct ActionView(gpui::Entity<AgentSurface>);
	impl Render for ActionView {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			self.0.update(cx, |s, cx| {
				s.workspace_action(
					"test-action".into(),
					"Action".into(),
					|s, _| s.feedback.message.push('x'),
					cx,
				)
			})
		}
	}

	#[test]
	fn account_connection_notice_does_not_confuse_unknown_usage_with_exhaustion() {
		let pending =
			super::account_connection_message("Agent account requires recovery: RefreshQuota")
				.unwrap();
		assert!(pending.contains("could not be confirmed"));
		assert!(!pending.contains("RefreshQuota"));
		assert!(!pending.contains("retry"));
		let exhausted =
			super::account_connection_message("This Agent's account has reached its usage limit.")
				.unwrap();
		assert!(exhausted.contains("reset time"));
		assert!(
			super::account_connection_message(
				"Agent process requires recovery: ProcessUnavailable"
			)
			.is_none()
		);
	}

	#[gpui::test]
	fn saved_voice_history_starts_collapsed_during_connection_failure(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			for work in &mut s.snapshot.as_mut().unwrap().work_items {
				work.codex_thread_id = None;
			}
			let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history else {
				panic!("fixture")
			};
			let base = entries[0].clone();
			*entries = (1..=4)
				.map(|id| {
					let mut entry = base.clone();
					entry.id = id;
					entry.kind = if id % 2 == 1 { "user" } else { "assistant" }.into();
					entry.text = "Saved speech".into();
					entry.receipt = Some(decodex_protocol::AgentHistoryReceiptDto {
						unsent_reason: None,
						voice_session_id: Some(if id < 3 { "first" } else { "second" }.into()),
						event_kind: format!("voice_{}", entry.kind),
						delivered_turn_id: None,
						disposed: true,
					});
					entry
				})
				.collect();
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		for (header, body) in [
			("voice-history-block-saved-agent-1", "voice-history-transcript-saved-agent-1"),
			("voice-history-block-saved-agent-3", "voice-history-transcript-saved-agent-3"),
		] {
			assert!(visual.debug_bounds(header).is_some());
			assert!(visual.debug_bounds(body).is_none());
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
			s.snapshot.as_mut().unwrap().pending_events[0].id = 99;
			let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history else {
				unreachable!()
			};
			entries.last_mut().unwrap().text =
				"Agent account requires recovery: RefreshQuota".into();
			s.workspace.connection_details_expanded = true;
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("connection-open-accounts").is_some());
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
		visual.update(|window, cx| window.draw(cx).clear(cx));
		let workspace = visual.debug_bounds("workspace-connecting").unwrap();
		let loading = visual.debug_bounds("loading-feedback-Opening workspace").unwrap();
		assert!((workspace.center().x - loading.center().x).abs() < gpui::px(1.));
		assert!((workspace.center().y - loading.center().y).abs() < gpui::px(1.));
		surface.update(visual, |s, cx| {
			assert_eq!(s.workspace_panels(), [(false, false); 3]);
			let panels = (
				s.workspace.sidebar_visible,
				s.workspace.graph_visible,
				s.workspace.agent_tree_visible,
			);
			s.toggle_workspace_sidebar(cx);
			s.toggle_workspace_graph(cx);
			s.toggle_agent_tree(cx);
			assert_eq!(
				panels,
				(
					s.workspace.sidebar_visible,
					s.workspace.graph_visible,
					s.workspace.agent_tree_visible
				)
			);
		});
		assert!(visual.debug_bounds("composer-send").is_none());
		assert!(visual.debug_bounds("composer-editor-area").is_none());

		assert!(visual.debug_bounds("agent-composer").is_none());
		assert!(visual.debug_bounds("conversation-unavailable").is_none());
		assert!(visual.debug_bounds("conversation-activity-status").is_none());
		surface.update(visual, |s, cx| {
			s.snapshot.as_mut().unwrap().connection_initializing = false;
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("conversation-unavailable").is_some());
		assert!(visual.debug_bounds("conversation-activity-status").is_none());
		surface.update(visual, |s, cx| {
			s.snapshot.as_mut().unwrap().pending_events.clear();
			assert_eq!(s.composer.read(cx).content(), "Keep my draft");
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("agent-composer").is_some());
		assert!(visual.debug_bounds("workspace-connecting").is_none());
		thread::sleep(std::time::Duration::from_millis(220));
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("conversation-unavailable").is_none());
	}

	#[gpui::test]
	fn failed_startup_keeps_unconfirmed_history_hidden_and_preserves_the_draft(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.snapshot.as_mut().unwrap().connection_initializing = true;
			s.composer.update(cx, |input, cx| input.set_content("Keep my draft", cx));
			s.apply_result(Ok(decodex_protocol::AgentSnapshotResult::Unavailable));
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("connection-open-diagnostics").is_some());
		assert!(visual.debug_bounds("loading-feedback-Opening workspace").is_none());
		assert!(visual.debug_bounds("work-dock").is_none());
		assert!(visual.debug_bounds("workspace-transcript").is_none());
		assert!(visual.debug_bounds("agent-composer").is_none());
		surface.update(visual, |s, cx| {
			// A background retry must retain actionable feedback until a new result arrives.
			s.status_before_refresh = Some(s.state.clone());
			s.state = LoadState::Loading;
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("connection-open-diagnostics").is_some());
		surface.update(visual, |s, cx| {
			let mut ready = s.snapshot.clone().unwrap();
			ready.connection_initializing = false;
			ready.pending_events.clear();
			s.apply_result(Ok(decodex_protocol::AgentSnapshotResult::Available(ready)));
			assert_eq!(s.composer.read(cx).content(), "Keep my draft");
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("workspace-connecting").is_none());
		assert!(visual.debug_bounds("connection-open-diagnostics").is_none());
		assert!(visual.debug_bounds("agent-composer").is_some());
	}

	#[gpui::test]
	fn a_main_writer_lock_does_not_disable_another_conversation(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.snapshot.as_mut().unwrap().pending_events =
				vec![decodex_protocol::AgentPendingEventDto {
					id: 99,
					source_event_id: "external-main-writer".into(),
					work_item_id: "agent".into(),
					event_kind: "thread_in_use_needs_attention".into(),
					created_at_micros: 1,
					delivery_claimed: false,
				}];
			assert_eq!(s.composer_unavailable_reason(), Some(super::THREAD_LOCKED_MESSAGE));
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("composer-blocked").is_some());
		assert!(visual.debug_bounds("composer-editor-area").is_none());
		surface.update(visual, |s, cx| {
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "release")
				.unwrap()
				.kind = decodex_protocol::AgentWorkKindDto::Manager;
			s.open_page("release", cx);
			assert!(s.composer_unavailable_reason().is_none());
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("composer-blocked").is_none());
		assert!(visual.debug_bounds("composer-editor-area").is_some());
		surface.update(visual, |s, cx| {
			s.snapshot.as_mut().unwrap().pending_events[0].work_item_id = "release".into();
			assert_eq!(s.composer_unavailable_reason(), Some(super::THREAD_LOCKED_MESSAGE));
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("composer-blocked").is_some());
		assert!(visual.debug_bounds("composer-editor-area").is_none());
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

		visual.update(|w, cx| w.draw(cx).clear(cx));

		let reserved = visual.update(|window, cx| {
			let s = surface.read(cx);

			(s.agent_tree_width(window), s.workspace_graph_size(window, true))
		});

		assert!(visual.debug_bounds("workspace-connecting").is_some());
		assert!(visual.debug_bounds("loading-feedback-Opening workspace").is_some());
		assert!(visual.debug_bounds("work-dock").is_none());

		surface.update(visual, |s, cx| {
			s.state = LoadState::Unavailable;

			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear(cx));

		assert!(visual.debug_bounds("loading-feedback-Opening workspace").is_none());
		assert!(visual.debug_bounds("connection-open-diagnostics").is_some());
		assert!(visual.debug_bounds("work-dock").is_none());
		assert!(
			visual.debug_bounds("workspace-welcome").is_none(),
			"a cold connection is not an empty conversation"
		);

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.state = LoadState::Loading;

			cx.notify();
		});

		visual.update(|w, cx| w.draw(cx).clear(cx));

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
		visual.update(|w, cx| w.draw(cx).clear(cx));

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

		visual.update(|window, cx| window.draw(cx).clear(cx));

		let bounds = visual.debug_bounds("test-action").expect("workspace action");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.read_with(visual, |s, _| {
			assert_eq!(s.feedback.message, "x", "click activates action")
		});

		for key in ["enter", "space"] {
			visual.simulate_event(gpui::KeyDownEvent {
				keystroke: gpui::Keystroke::parse(key).expect("activation key"),
				is_held: true,
				prefer_character_input: false,
			});
			surface.read_with(visual, |s, _| {
				assert_eq!(s.feedback.message, "x", "held key repeats action")
			});
		}

		visual.simulate_keystrokes("enter space");
		surface.read_with(visual, |s, _| {
			assert_eq!(s.feedback.message, "xxx", "fresh keys activate action")
		});
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
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("worker-stop").is_some(), "running worker has no stop control");

		surface.update(visual, |s, cx| {
			assert_eq!(s.running_turn().expect("running worker").0.as_str(), "verify");

			s.interrupt_current(cx);

			assert_eq!(s.feedback.message, "No service profile is configured.");
			assert_eq!(s.composer.read(cx).content(), "Retained manager draft");

			s.state = LoadState::Unavailable;

			cx.notify();
		});

		visual.update(|window, cx| window.draw(cx).clear(cx));

		assert!(visual.debug_bounds("worker-stop").is_none(), "disconnected worker cannot stop");
	}

	#[gpui::test]
	fn unavailable_composer_keeps_a_read_only_draft_and_resumes_after_recovery(
		cx: &mut gpui::TestAppContext,
	) {
		cx.update(crate::composer_input::bind_keys);

		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		let input = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

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
			window.draw(cx).clear(cx);
		});
		surface.update(visual, |s, cx| {
			s.state = LoadState::Stale;
			s.composer_menu = Some("attachments");
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		visual.run_until_parked();

		assert!(visual.debug_bounds("agent-composer").is_some());
		let composer = visual.debug_bounds("agent-composer").unwrap();
		let notice = visual.debug_bounds("conversation-unavailable").unwrap();
		assert!(notice.top() >= composer.top() && notice.bottom() <= composer.bottom());
		assert!(visual.debug_bounds("composer-retained-draft").is_some());
		assert!(visual.debug_bounds("composer-editor-area").is_none());
		assert!(visual.debug_bounds("composer-action-row").is_none());
		assert!(visual.debug_bounds("composer-menu-popover").is_none());

		visual.simulate_keystrokes("cmd-end space e d i t e d enter cmd-enter");

		surface.update(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Saved");
			assert_eq!(s.attachments.len(), 1);
			assert!(s.composer_menu.is_none());

			s.escape_interrupt(cx);
			s.escape_interrupt(cx);
			s.interrupt_current(cx);

			assert!(!s.sending);
			assert!(s.submission.command.is_none());
			assert!(s.interrupt_task.is_none());
			assert!(s.escape_stop.is_none());

			s.state = LoadState::Ready;

			assert_eq!(s.composer.read(cx).content(), "Saved");
			assert!(s.submission.command.is_none(), "reconnection cannot submit retained edits");
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});
		assert!(visual.debug_bounds("composer-blocked").is_some(), "retain the lock during exit");
		thread::sleep(std::time::Duration::from_millis(220));
		visual.update(|window, cx| {
			crate::ui_frame::draw_frame(window, cx);
			window.focus(&input.focus_handle(cx), cx);
		});
		assert!(visual.debug_bounds("composer-blocked").is_none());
		assert!(visual.debug_bounds("composer-editor-area").is_some());
		visual.simulate_keystrokes("cmd-end space e d i t e d");
		surface.update(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "Saved edited");
			assert_eq!(s.attachments.len(), 1);
			assert!(s.submission.command.is_none());
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
			window.draw(cx).clear(cx);
		});
	}
	#[gpui::test]
	fn reopening_a_closing_tab_keeps_it_and_close_returns_to_main(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.open_page("verify", cx);
		});
		visual.update(|w, cx| w.draw(cx).clear(cx));
		surface.update(visual, |s, cx| s.close_page("verify", cx));
		visual.update(|w, cx| w.draw(cx).clear(cx));
		surface.update(visual, |s, cx| s.open_page("verify", cx));
		visual.update(|w, cx| w.draw(cx).clear(cx));
		surface.update(visual, |s, _| {
			assert_eq!(s.workspace.pages, vec!["verify"]);
			assert_eq!(s.selected.as_deref(), Some("verify"));
			assert!(s.workspace.closing_pages.is_empty());
		});
		surface.update(visual, |s, cx| s.close_page("verify", cx));
		visual.update(|w, cx| w.draw(cx).clear(cx));

		// Wait only for deferred removal; visual smoothness is not a unit-test claim.
		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|w, cx| w.draw(cx).clear(cx));
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
