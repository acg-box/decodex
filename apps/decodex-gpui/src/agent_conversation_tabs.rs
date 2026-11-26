//! Conversation-local tabs reuse retained native attachments and ordinary page navigation.
use crate::ui_motion::HoverHint as _;
use gpui::{AnyElement, ClickEvent, KeyDownEvent, Role, SharedString, prelude::*};

use super::{Panel, PromptForkPhase};
use crate::{
	shell::{
		agent_surface::{AgentSnapshotDto, AgentSurface, Context},
		workspace_symbols,
	},
	ui_motion, ui_theme,
};
use decodex_protocol::{AttachedBranchesResult, PromptForkBoundary, PromptForkStatus};

const MAX_TAB_WIDTH: f32 = 156.;

struct Tab {
	id: String,
	label: String,
	closable: bool,
	temporary: bool,
}

impl Panel {
	fn branches<'a>(&'a self, snapshot: &'a AgentSnapshotDto) -> Vec<&'a PromptForkStatus> {
		let Some((owner, runtime)) = &self.owner else { return Vec::new() };
		if snapshot.runtime_source.as_ref() != Some(runtime) {
			return Vec::new();
		}
		let Some(source) = snapshot.work_items.iter().find(|work| &work.id == owner) else {
			return Vec::new();
		};
		let Some(AttachedBranchesResult::Available { branches, .. }) = &self.result else {
			return Vec::new();
		};
		branches
			.iter()
			.filter(|branch| {
				branch.work_id.as_str() == owner
					&& source.codex_thread_id.as_deref() == Some(branch.thread_id.as_str())
					&& branch.boundary.is_side_chat()
					&& branch.phase == PromptForkPhase::Forked
					&& !snapshot.archived_work_ids.contains(branch.target_work_id.as_str())
					&& branch.target_thread_id.as_ref().is_some_and(|thread| {
						snapshot.work_items.iter().any(|work| {
							work.id == branch.target_work_id.as_str()
								&& work.codex_thread_id.as_deref() == Some(thread.as_str())
						})
					})
			})
			.collect()
	}

	fn looked_up(&self, work: &decodex_protocol::AgentWorkItemDto) -> bool {
		self.lookup_for
			.as_ref()
			.is_some_and(|(id, thread)| id == &work.id && thread == &work.codex_thread_id)
	}

	fn lookup_root<'a>(
		&'a self,
		work: &'a decodex_protocol::AgentWorkItemDto,
		snapshot: &'a AgentSnapshotDto,
	) -> &'a str {
		if let Some(parent) = snapshot.side_chat_sources.get(&work.id) {
			return parent;
		}
		if let Some(root) = self.family_root(&work.id, snapshot) {
			return root;
		}
		let Some(parent) = work.parent_goal_id.as_deref() else { return &work.id };
		let checked_parent = self.looked_up(work)
			&& self.result.is_some()
			&& self.owner.as_ref().is_some_and(|(root, _)| root == parent);
		if checked_parent { &work.id } else { parent }
	}

	pub(in crate::shell::agent_surface) fn family_root<'a>(
		&'a self,
		work: &str,
		snapshot: &AgentSnapshotDto,
	) -> Option<&'a str> {
		let (root, runtime) = self.owner.as_ref()?;
		(snapshot.runtime_source.as_ref() == Some(runtime)
			&& (root == work
				|| self
					.branches(snapshot)
					.iter()
					.any(|branch| branch.target_work_id.as_str() == work)))
		.then_some(root.as_str())
	}
}

impl AgentSurface {
	pub(in crate::shell::agent_surface) fn sync_side_chat_family(
		&mut self,
		cx: &mut Context<Self>,
	) {
		if self.native_agents.selected.is_some()
			|| !self.selected_has_conversation_composer()
			|| self.selected_is_archived()
		{
			return;
		}
		let Some(snapshot) = &self.snapshot else { return };
		let Some(work) = self
			.selected
			.as_ref()
			.and_then(|id| snapshot.work_items.iter().find(|work| &work.id == id))
		else {
			return;
		};
		// A local parent is only a lookup candidate. A fresh native receipt proves attachment.
		let state = &self.workspace.attached;
		let root = state.lookup_root(work, snapshot).to_owned();
		let known = state.family_root(&work.id, snapshot).is_some();
		if state.owner.as_ref().is_some_and(|(owner, runtime)| {
			owner == &root && snapshot.runtime_source.as_ref() == Some(runtime)
		}) && (state.task.is_some()
			|| state.result.is_some() && (known || state.looked_up(work)))
		{
			return;
		}
		self.load_attached_branches(&root, cx);
	}

	fn family_tabs(&self) -> Vec<Tab> {
		let Some(selected) = self.conversation_page() else { return Vec::new() };
		let Some(snapshot) = &self.snapshot else { return Vec::new() };
		let root = snapshot
			.side_chat_sources
			.get(&selected)
			.map(String::as_str)
			.or_else(|| self.workspace.attached.family_root(&selected, snapshot))
			.unwrap_or(&selected);
		let label = if self.is_assistant_conversation(root) { "Assistant" } else { "Main" }.into();
		let mut tabs = vec![Tab { id: root.into(), label, closable: false, temporary: false }];
		if self.workspace.attached.family_root(&selected, snapshot).is_none() {
			return tabs;
		}
		for (index, branch) in
			self.workspace.attached.branches(snapshot).into_iter().rev().enumerate()
		{
			let id = branch.target_work_id.as_str();
			let label = snapshot
				.work_items
				.iter()
				.find(|work| work.id == id)
				.and_then(|work| work.codex_thread_id.as_ref())
				.and_then(|thread| snapshot.conversation_titles.get(thread))
				.cloned()
				.unwrap_or_else(|| format!("Side chat {}", index + 1));
			tabs.push(Tab {
				id: id.into(),
				label,
				closable: true,
				temporary: branch.boundary == PromptForkBoundary::TemporarySideChat,
			});
		}
		tabs
	}

	pub(in crate::shell::agent_surface) fn temporary_tab(
		&self,
		id: &str,
	) -> Option<(String, String)> {
		let branch =
			self.workspace.attached.branches(self.snapshot.as_ref()?).into_iter().find(|b| {
				b.target_work_id.as_str() == id
					&& b.boundary == PromptForkBoundary::TemporarySideChat
			})?;
		Some((branch.work_id.as_str().into(), branch.target_thread_id.as_ref()?.as_str().into()))
	}

	fn conversation_tabs(&self) -> Vec<Tab> {
		let selected = self.conversation_page();
		self.family_tabs()
			.into_iter()
			.filter(|t| {
				!t.closable
					|| t.temporary
					|| selected.as_ref() == Some(&t.id)
					|| self.workspace.pages.contains(&t.id)
			})
			.collect()
	}

	pub(in crate::shell::agent_surface) fn can_start_side_chat(&self) -> bool {
		self.command_connection_ready()
			&& self.native_agents.selected.is_none()
			&& self.selected_has_conversation_composer()
			&& !self.selected_is_archived()
			&& self
				.selected
				.as_ref()
				.and_then(|id| self.conversation_record(id))
				.is_some_and(|w| w.codex_thread_id.is_some())
			&& !self.voice_on_current_conversation()
			&& self.prompt_edit.task.is_none()
			&& !self.prompt_edit.list_branch_pending
	}

	pub(crate) fn open_side_chat_from_keyboard(
		&mut self,
		window: &gpui::Window,
		cx: &mut Context<Self>,
	) {
		let input = self.composer.read(cx);
		let passage = gpui::Focusable::focus_handle(input, cx)
			.is_focused(window)
			.then(|| input.selected_passage())
			.flatten();
		self.start_side_chat(passage, cx);
	}

	pub(in crate::shell::agent_surface) fn start_side_chat(
		&mut self,
		reference: Option<String>,
		cx: &mut Context<Self>,
	) {
		if !self.can_start_side_chat() {
			return;
		}
		let Some(selected) = self.selected.clone() else { return };
		let root = self
			.snapshot
			.as_ref()
			.and_then(|s| {
				s.side_chat_sources
					.get(&selected)
					.map(String::as_str)
					.or_else(|| self.workspace.attached.family_root(&selected, s))
			})
			.unwrap_or(&selected)
			.to_owned();
		self.request_live_side_chat(&selected, &root, reference, cx);
	}

	pub(in crate::shell::agent_surface) fn install_side_family(
		&mut self,
		root: String,
		branches: AttachedBranchesResult,
	) {
		if let Some(runtime) = self.snapshot.as_ref().and_then(|s| s.runtime_source.clone()) {
			self.reset_attached_branches();
			self.workspace.attached.owner = Some((root, runtime));
			self.workspace.attached.result = Some(branches);
		}
	}

	pub(in crate::shell::agent_surface) fn conversation_tab_bar(
		&self,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let selected = self.conversation_page();
		let tabs = self.conversation_tabs();

		let more_tabs = self.family_tabs().len() > tabs.len()
			|| self.workspace.attached.scroll.max_offset().x > gpui::px(1.);
		let mut strip = gpui::div()
			.id("conversation-tabs")
			.debug_selector(|| "conversation-tabs".into())
			.role(Role::TabList)
			.aria_label("Conversation and side chats")
			.min_w_0()
			.flex_initial()
			.h_full()
			.overflow_x_scroll()
			.track_scroll(&self.workspace.attached.scroll)
			.flex()
			.items_center()
			.gap(gpui::px(ui_theme::CONVERSATION_TAB_GAP));
		if self.workspace.attached.visible_tab.borrow().as_ref() != selected.as_ref() {
			if let Some(index) = tabs.iter().position(|tab| Some(&tab.id) == selected.as_ref()) {
				self.workspace.attached.scroll.scroll_to_item(index);
			}
			*self.workspace.attached.visible_tab.borrow_mut() = selected.clone();
		}
		for tab in tabs {
			let active = selected.as_deref() == Some(&tab.id);
			let id = tab.id.clone();
			let mut element = self.conversation_header_tab(&tab, active, cx);
			if tab.closable {
				let owner = cx.entity().downgrade();
				element = ui_motion::TabReveal {
					id: SharedString::from(format!("conversation-reveal-{id}")).into(),
					visible: !self.workspace.closing_pages.contains(&id),
					max_width: MAX_TAB_WIDTH,
					child: element,
					closed: Box::new(move |cx| {
						let _ = owner.update(cx, |s, cx| {
							if s.workspace.closing_pages.remove(&id) {
								s.workspace.pages.retain(|page| page != &id);
								cx.notify();
							}
						});
					}),
				}
				.into_any_element();
			}
			strip = strip.child(element);
		}
		gpui::div()
			.min_w_0()
			.flex_1()
			.h_full()
			.flex()
			.items_center()
			.gap(gpui::px(5.))
			.child(strip)
			.child(self.new_side_chat_button(cx))
			.when(more_tabs, |bar| bar.child(self.tab_menu(cx)))
			.child(gpui::div().flex_grow(1.))
			.into_any_element()
	}

	fn tab_menu(&self, cx: &mut Context<Self>) -> AnyElement {
		gpui::div()
			.id("all-conversation-tabs")
			.debug_selector(|| "all-conversation-tabs".into())
			.role(Role::Button)
			.aria_label("All conversation tabs")
			.hover_hint("All conversation tabs")
			.tab_index(0)
			.size(gpui::px(24.))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.cursor_pointer()
			.rounded(gpui::px(5.))
			.hover(|b| b.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.on_click(cx.listener(|s, _, w, cx| s.show_tab_menu(w, cx)))
			.on_key_down(cx.listener(|s, e: &KeyDownEvent, w, cx| {
				if !e.is_held && matches!(e.keystroke.key.as_str(), "enter" | "space") {
					cx.stop_propagation();
					s.show_tab_menu(w, cx);
				}
			}))
			.child(workspace_symbols::icon_sized(workspace_symbols::Symbol::ChevronDown, 12.))
			.into_any_element()
	}

	fn show_tab_menu(&self, _window: &mut gpui::Window, _cx: &mut Context<Self>) {
		#[cfg(all(target_os = "macos", not(test)))]
		{
			use crate::ui_native_menu::{self, Item};
			let owner = _cx.entity().downgrade();
			let rows = self
				.family_tabs()
				.into_iter()
				.map(|tab| Item::command(tab.label, "", true, tab.id))
				.collect();
			ui_native_menu::show(_window, _cx, rows, move |id, cx| {
				let _ = owner.update(cx, |s, cx| s.open_page(&id, cx));
			});
		}
	}

	fn new_side_chat_button(&self, cx: &mut Context<Self>) -> AnyElement {
		let ready = self.can_start_side_chat();
		let opening =
			self.prompt_edit.live_side_target.is_some() && self.prompt_edit.task.is_some();
		if !ready && !opening {
			return gpui::div().into_any_element();
		}
		let add = gpui::div()
			.id("new-side-chat")
			.debug_selector(|| "new-side-chat".into())
			.role(Role::Button)
			.aria_label("New side chat")
			.hover_hint(crate::shell::shortcuts::hint(
				"New side chat",
				&crate::shell::OpenSideChat,
				cx,
			))
			.tab_index(if ready { 0 } else { -1 })
			.h(gpui::px(25.))
			.px(gpui::px(6.))
			.gap(gpui::px(3.))
			.text_size(gpui::px(11.5))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(6.))
			.opacity(if ready { 1. } else { 0.35 })
			.cursor_pointer()
			.hover(|style| style.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.on_click(cx.listener(move |s, _, _, cx| {
				if ready {
					s.start_side_chat(None, cx);
				}
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if ready
					&& !event.is_held
					&& matches!(event.keystroke.key.as_str(), "enter" | "space")
				{
					cx.stop_propagation();
					s.start_side_chat(None, cx);
				}
			}))
			.child(workspace_symbols::icon_sized(workspace_symbols::Symbol::Plus, 12.))
			.when(opening || self.conversation_tabs().len() <= 1, |b| {
				b.child(if opening { "Opening…" } else { "Side chat" })
			});
		add.into_any_element()
	}

	fn conversation_header_tab(
		&self,
		tab: &Tab,
		active: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let id = tab.id.clone();
		let keyboard = id.clone();
		let mut element = gpui::div()
			.id(SharedString::from(format!("conversation-tab-{}", tab.id)))
			.debug_selector({
				let id = id.clone();
				move || format!("conversation-tab-{id}")
			})
			.role(Role::Tab)
			.aria_label(tab.label.clone())
			.aria_selected(active)
			.hover_hint(tab.label.clone())
			.tab_index(0)
			.max_w(gpui::px(MAX_TAB_WIDTH))
			.h(gpui::px(27.))
			.px(gpui::px(8.))
			.flex_none()
			.flex()
			.items_center()
			.gap(gpui::px(7.))
			.rounded(gpui::px(7.))
			.cursor_pointer()
			.bg(ui_theme::rgba(if active { ui_theme::CONTROL_SELECTED_FILL } else { 0xffffff00 }))
			.text_color(ui_theme::rgb(if active { ui_theme::TEXT } else { ui_theme::TEXT_MUTED }))
			.hover(|style| style.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.on_click(cx.listener(move |s, _: &ClickEvent, _, cx| s.open_page(&id, cx)))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if !event.is_held && matches!(event.keystroke.key.as_str(), "enter" | "space") {
					cx.stop_propagation();
					s.open_page(&keyboard, cx);
				}
			}))
			.child(ui_motion::ConversationTabLabel {
				id: format!("header-{}", tab.id),
				state: self.conversation_status(&tab.id),
				title: tab.label.clone(),
			});
		if tab.closable {
			let close = tab.id.clone();
			let keyboard = close.clone();
			element = element.child(
				gpui::div()
					.id(SharedString::from(format!("conversation-close-{}", tab.id)))
					.debug_selector({
						let id = tab.id.clone();
						move || format!("conversation-close-{id}")
					})
					.role(Role::Button)
					.aria_label(format!("Close {}", tab.label))
					.tab_index(0)
					.size(gpui::px(16.))
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.rounded(gpui::px(4.))
					.opacity(if active { 0.7 } else { 0.4 })
					.hover(|style| style.opacity(1.).bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.on_click(cx.listener(move |s, _, _, cx| {
						cx.stop_propagation();
						s.close_page(&close, cx);
					}))
					.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
						if !event.is_held
							&& matches!(event.keystroke.key.as_str(), "enter" | "space")
						{
							cx.stop_propagation();
							s.close_page(&keyboard, cx);
						}
					}))
					.child(workspace_symbols::icon_sized(workspace_symbols::Symbol::Close, 10.)),
			);
		}
		element.into_any_element()
	}
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	pub(in crate::shell::agent_surface) fn visual_side_chat_tabs(
		&mut self,
		root: &str,
		cx: &mut Context<Self>,
	) {
		use decodex_protocol::{AgentWorkKindDto, EntityId, WireText};
		self.visual_workspace_fixture(cx);
		self.work_board.graph = false;
		self.workspace.graph_expanded = false;
		self.workspace.browsing = false;
		let snapshot = self.snapshot.as_mut().expect("fixture snapshot");
		let source =
			snapshot.work_items.iter_mut().find(|work| work.id == root).expect("fixture root");
		source.kind = AgentWorkKindDto::Manager;
		source.codex_thread_id = Some(format!("source-{root}"));
		let source = source.clone();
		let runtime = EntityId::new("side-chat-fixture").expect("fixture runtime");
		snapshot.runtime_source = Some(runtime.clone());
		let mut branches = Vec::new();
		for (id, title) in [("side-one", "Architecture"), ("side-two", "Tradeoffs")] {
			let mut work = source.clone();
			work.id = id.into();
			work.title = title.into();
			work.parent_goal_id = Some(root.into());
			work.codex_thread_id = Some(format!("native-{id}"));
			work.active_turn_id = None;
			snapshot.side_chat_sources.insert(id.into(), root.into());
			snapshot.work_items.push(work);
			branches.insert(
				0,
				PromptForkStatus {
					model_provider: None,
					work_id: EntityId::new(root).expect("fixture source"),
					thread_id: WireText::new(format!("source-{root}")).expect("fixture thread"),
					review_token: WireText::new(format!("review-{id}")).expect("fixture review"),
					target_work_id: EntityId::new(id).expect("fixture work"),
					target_thread_id: Some(
						WireText::new(format!("native-{id}")).expect("fixture target"),
					),
					boundary: PromptForkBoundary::AttachedAfterTurn,
					phase: PromptForkPhase::Forked,
					edit_receipt_id: None,
				},
			);
		}
		self.workspace.pages.extend(["side-one".into(), "side-two".into()]);
		self.open_page(root, cx);
		self.workspace.attached = Panel {
			owner: Some((root.into(), runtime)),
			result: Some(AttachedBranchesResult::Available { branches, truncated: false }),
			..Default::default()
		};
		self.workspace.graph_visible = false;
		self.workspace.agent_tree_visible = false;
	}
}

#[cfg(test)]
#[path = "agent_conversation_tab_tests.rs"]
mod tests;
