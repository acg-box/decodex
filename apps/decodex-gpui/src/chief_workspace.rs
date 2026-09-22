//! Conversation-first desktop presentation. All displayed work comes from the service.
use super::*;
use crate::ui_motion::{SmoothControl, reveal};
use gpui::{AnyElement, MouseButton, PathBuilder, canvas, point};

#[derive(Clone)]
pub(super) struct PageView {
	scope: Option<String>,
	selected: Option<String>,
	pan: (f32, f32),
	zoom: f32,
	graph_visible: bool,
	timeline_visible: bool,
}

impl ChiefSurface {
	pub(crate) fn navigation_work(&self) -> Option<String> {
		self.selected.clone().filter(|id| Some(id) != self.root_id().as_ref())
	}

	pub(crate) fn can_restore_work(&self, work: Option<&str>) -> bool {
		work.is_none_or(|id| {
			self.snapshot.as_ref().is_some_and(|s| s.work_items.iter().any(|w| w.id == id))
		})
	}

	pub(crate) fn restore_work(&mut self, work: Option<&str>, cx: &mut Context<Self>) {
		if let Some(id) = work.map(str::to_owned).or_else(|| self.root_id()) {
			self.open_page(&id, cx);
		}
	}

	pub(crate) fn panel_glyph(index: usize) -> AnyElement {
		panel_icon(
			["workspace-sidebar", "workspace-graph", "workspace-timeline", "workspace-agents"]
				[index],
		)
		.expect("known panel glyph")
	}

	pub(crate) fn workspace_panels(&self) -> [(bool, bool); 4] {
		[
			(self.sidebar_visible, true),
			(self.graph_visible && self.has_work(), self.has_work()),
			(
				self.timeline_visible && selected_history_available(self),
				selected_history_available(self),
			),
			(self.agent_tree_visible, self.has_work()),
		]
	}

	pub(crate) fn toggle_workspace_timeline(&mut self, cx: &mut Context<Self>) {
		self.timeline_visible = !self.timeline_visible;

		cx.notify();
	}

	pub(crate) fn toggle_workspace_sidebar(&mut self, cx: &mut Context<Self>) {
		self.sidebar_visible = !self.sidebar_visible;
		cx.notify();
	}

	pub(crate) fn toggle_workspace_graph(&mut self, cx: &mut Context<Self>) {
		self.graph_visible = !self.graph_visible;
		self.graph_expanded = false;
		cx.notify();
	}

	pub(super) fn has_work(&self) -> bool {
		self.snapshot
			.as_ref()
			.is_some_and(|s| s.work_items.iter().any(|w| w.parent_goal_id.is_some()))
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
		self.native_agents.selected = None;
		if !self.snapshot.as_ref().is_some_and(|s| s.work_items.iter().any(|w| w.id == id)) {
			return;
		}
		if self.selected.as_deref() != Some(id) {
			self.resources = None;
			self.resources_task = None;
			self.usage_estimate = None;
			self.usage_estimate_task = None;
			self.integrations = None;
			self.integrations_task = None;
			self.integration_refresh_task = None;
			self.integration_feedback.clear();
			self.resource_mutation_task = None;
			self.resource_feedback.clear();
		}
		let is_manager = self.snapshot.as_ref().is_some_and(|snapshot| {
			snapshot.work_items.iter().any(|work| {
				work.id == id
					&& (work.parent_goal_id.is_none()
						|| work.kind == decodex_protocol::ChiefWorkKindDto::Manager)
			})
		});
		if is_manager {
			let previous = self.composer_manager.clone().or_else(|| self.root_id());
			if previous.as_deref() != Some(id) {
				if let Some(previous) = previous {
					self.task_reference_drafts
						.insert(previous.clone(), std::mem::take(&mut self.task_references));
					self.attachment_drafts
						.insert(previous.clone(), std::mem::take(&mut self.attachments));
					self.manager_drafts.insert(previous, self.composer.read(cx).content().into());
				}
				self.attachments = self.attachment_drafts.remove(id).unwrap_or_default();
				self.task_references = self.task_reference_drafts.remove(id).unwrap_or_default();
				self.composer_menu = None;
				let draft = self.manager_drafts.get(id).cloned().unwrap_or_default();
				self.composer.update(cx, |input, cx| {
					input.set_content(&draft, cx);
					input.set_placeholder(prompts::next(), cx);
				});
				Self::refresh_prompt(cx);
			}
			self.composer_manager = Some(id.into());
		}
		if let Some((old, history)) = &self.history {
			self.history_cache.insert(old.clone(), history.clone());
		}
		if self.root_id().as_deref() != Some(id) && !self.pages.iter().any(|p| p == id) {
			self.pages.push(id.to_owned());
		}
		if self.selected.as_deref() != Some(id) {
			self.stop_voice(cx);
			if let Some(old) = &self.selected {
				self.page_views.insert(
					old.clone(),
					PageView {
						scope: self.graph_scope.clone(),
						selected: self.graph_selected.clone(),
						pan: self.graph_pan,
						zoom: self.graph_zoom,
						graph_visible: self.graph_visible,
						timeline_visible: self.timeline_visible,
					},
				);
			}
			let saved = self.page_views.get(id).cloned().unwrap_or_else(|| PageView {
				scope: self
					.snapshot
					.as_ref()
					.and_then(|snap| snap.work_items.iter().find(|w| w.id == id))
					.and_then(|w| {
						if w.kind == decodex_protocol::ChiefWorkKindDto::Manager {
							Some(w.id.clone())
						} else {
							w.parent_goal_id.clone()
						}
					}),
				selected: Some(id.to_owned()),
				pan: (0.0, 0.0),
				zoom: 0.85,
				graph_visible: self.graph_visible,
				timeline_visible: self.timeline_visible,
			});
			self.graph_scope = saved.scope;
			self.graph_selected = saved.selected;
			self.graph_pan = saved.pan;
			self.graph_zoom = saved.zoom;
			self.graph_visible = saved.graph_visible;
			self.timeline_visible = saved.timeline_visible;
			self.graph_expanded = false;
		}

		self.selected = Some(id.to_owned());
		self.connection_details_expanded = false;
		self.history = self.history_cache.get(id).cloned().map(|h| (id.to_owned(), h));
		self.details_visible = false;
		self.request = None;
		self.request_task = None;
		self.load_history(cx);

		self.sync_request(cx);
		cx.notify();
	}

	fn close_page(&mut self, id: &str, cx: &mut Context<Self>) {
		self.pages.retain(|p| p != id);
		if self.selected.as_deref() == Some(id)
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
		let is_event = id.starts_with("event-");
		let active = if is_tab {
			self.selected.as_deref() == id.strip_prefix("page-")
		} else if let Some(work) = id.strip_prefix("sidebar-") {
			self.selected.as_deref() == Some(work)
		} else if id == "chief-home" {
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
				_ => label.as_str(),
			}
			.to_owned()
		};
		let icon = panel_icon(&id);
		let icon_only = icon.is_some();
		let show_tip = icon_only || is_tab || id.starts_with("attention-");
		let tip = accessible.clone();
		let action = std::rc::Rc::new(action);
		let keyboard = action.clone();
		div()
			.id(SharedString::from(id))
			.role(if is_tab { Role::Tab } else { Role::Button })
			.aria_selected(active)
			.when(active && !is_tab, |row| row.bg(rgba(0xffffff0d)))
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
				button.size(px(ui_theme::CHROME_CONTROL_SIZE)).p_0().justify_center()
			})
			.rounded(px(5.0))
			.cursor_pointer()
			.hover(|s| s.bg(rgba(ui_theme::SURFACE_RAISED_MATERIAL)))
			.on_click(cx.listener(move |s, _, _, cx| action(s, cx)))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					keyboard(s, cx);
					cx.stop_propagation();
				}
			}))
			.when(is_event, |button| button.w_full().min_w_0())
			.child(if let Some(icon) = icon {
				icon
			} else {
				div()
					.min_w_0()
					.when(!is_event, |text| text.whitespace_nowrap().text_ellipsis())
					.child(label)
					.into_any_element()
			})
			.smooth()
			.into_any_element()
	}

	pub(super) fn workspace_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut panel = div()
			.id("chief-sidebar")
			.w_full()
			.min_w_0()
			.relative()
			.h_full()
			.flex()
			.flex_col()
			.p(px(ui_theme::CONTROL_MARGIN))
			.pt(px(super::super::WINDOW_CONTROLS_CLEARANCE))
			.gap_1()
			.bg(rgba(ui_theme::CHIEF_SIDEBAR_MATERIAL))
			.pr(px(4.));
		panel = panel.child(self.workspace_action(
			"chief-home".into(),
			"Main".into(),
			|s, cx| {
				if let Some(id) = s.root_id() {
					s.open_page(&id, cx);
				}
			},
			cx,
		));
		panel = panel.child(
			div()
				.mt_5()
				.px_2()
				.text_size(px(11.0))
				.text_color(rgb(ui_theme::TEXT_MUTED))
				.flex()
				.items_center()
				.justify_between()
				.child("Projects")
				.child(self.workspace_action(
					"new-project".into(),
					"+".into(),
					|s, cx| {
						if let Some(id) = s.root_id() {
							s.open_page(&id, cx);
						}
						if s.composer.read(cx).content().trim().is_empty() {
							s.composer.update(cx, |input, cx| {
								input.set_content("Create a project workspace for ", cx)
							});
						} else {
							s.feedback="Your draft is kept. Send or clear it before starting a new project.".into();
						}
						cx.notify();
					},
					cx,
				)),
		);
		let mut list = div().id("chief-sidebar-work").flex_1().min_h_0().overflow_y_scroll();
		if let Some(snapshot) = &self.snapshot {
			for work in snapshot.work_items.iter().filter(|w| {
				snapshot.workspaces.iter().any(|p| p.chief_id == w.id)
					|| (w.parent_goal_id == self.root_id()
						&& w.kind == decodex_protocol::ChiefWorkKindDto::Manager)
			}) {
				let id = work.id.clone();
				let label = snapshot
					.workspaces
					.iter()
					.find(|p| p.chief_id == id)
					.map_or_else(|| work.title.clone(), |p| p.name.clone());
				let waiting = snapshot.work_items.iter().find(|w| {
					within_project(snapshot, &id, &w.id)
						&& (w.status == ChiefWorkStatusDto::UserDecision
							|| snapshot.pending_events.iter().any(|e| {
								e.work_item_id == w.id && e.event_kind.ends_with("_pending")
							}))
				});
				let count = snapshot
					.work_items
					.iter()
					.filter(|w| {
						within_project(snapshot, &id, &w.id)
							&& (w.status == ChiefWorkStatusDto::UserDecision
								|| snapshot.pending_events.iter().any(|e| {
									e.work_item_id == w.id && e.event_kind.ends_with("_pending")
								}))
					})
					.count();
				let mut row = div().flex().items_center().child(div().flex_1().min_w_0().child(
					self.workspace_action(
						format!("sidebar-{id}"),
						label,
						move |s, cx| s.open_page(&id, cx),
						cx,
					),
				));
				if let Some(waiting) = waiting {
					let target = waiting.id.clone();
					row = row.child(self.workspace_action(
						format!("attention-{}", work.id),
						format!("{count}"),
						move |s, cx| {
							s.open_page(&target, cx);
							if let Some(scroll) = s.transcript_scroll.get(&target) {
								scroll.scroll_to_bottom();
							}
						},
						cx,
					));
				}
				list = list.child(row);
			}
		}
		panel = panel.child(list);
		panel.child(self.sidebar_resize_handle(cx)).into_any_element()
	}

	fn workspace_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut row = div()
			.id("chief-pages")
			.role(Role::TabList)
			.aria_label("Open conversations")
			.h(px(35.0))
			.min_h(px(35.0))
			.flex()
			.items_center()
			.gap_1()
			.px_2()
			.pb(px(4.));
		let root = self.root_id();
		let mut pages = vec![(root.clone().unwrap_or_default(), "Main".to_owned(), false)];
		if let Some(snapshot) = &self.snapshot {
			pages.extend(self.pages.iter().filter_map(|id| {
				snapshot
					.work_items
					.iter()
					.find(|w| &w.id == id)
					.map(|w| (id.clone(), self.work_label(w), true))
			}));
		}
		for (id, label, closable) in pages {
			let active =
				self.selected.as_ref() == Some(&id) || (!closable && self.selected.is_none());
			let select = id.clone();
			let mut tab = div()
				.flex()
				.items_center()
				.h(px(28.))
				.rounded(px(8.))
				.when(active, |tab| tab.bg(rgba(0xffffff0d)))
				.child(self.workspace_action(
					format!("page-{id}"),
					label,
					move |s, cx| s.open_page(&select, cx),
					cx,
				));
			if closable {
				let close = id.clone();
				tab = tab.child(self.workspace_action(
					format!("close-{id}"),
					"×".into(),
					move |s, cx| s.close_page(&close, cx),
					cx,
				));
			}
			row = row.child(tab);
		}
		row.into_any_element()
	}

	pub(super) fn composer_unavailable_reason(&self) -> Option<&'static str> {
		if self.uncertain {
			return Some(
				"Delivery is unconfirmed. Your draft is kept; sending is paused to avoid duplicates.",
			);
		}
		let selected = self.selected.as_deref()?;
		if matches!(self.displayed_load_state(), LoadState::Unavailable | LoadState::Stale) {
			return Some("The service connection is unavailable. Your history and draft are kept.");
		}
		let snapshot = self.snapshot.as_ref()?;
		let root = self.root_id();
		for event in &snapshot.pending_events {
			if event.work_item_id != selected && Some(&event.work_item_id) != root.as_ref() {
				continue;
			}
			let reason = match event.event_kind.as_str() {
				"thread_in_use_needs_attention" =>
					"This conversation is in use in another app. Sending is unavailable here; your history remains readable.",
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
			.filter(|w| w.dispatch_state == ChiefDispatchStateDto::Unknown)
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
		let (owner, ChiefHistoryResult::Available { entries, .. }) = self.history.as_ref()? else {
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
		let detail = self.connection_failure_detail();
		let (title, description) = match detail {
			Some(text) if text.contains("ProcessUnavailable") => (
				"Codex couldn't start",
				"The local Codex connection could not be started or initialized. Your messages are saved. Decodex will retry automatically; you do not need to resend them.",
			),
			Some(text) if text.contains("RefreshQuota") || text.contains("usage limit") => (
				"Account availability needs checking",
				"Codex could not confirm an account with available usage. Your messages are saved. Review account availability in Settings → Accounts.",
			),
			Some(text) if text.contains("SelectWorkingDirectory") => (
				"Project folder is unavailable",
				"Restore access to the project folder so this conversation can resume. Your messages and draft are kept.",
			),
			_ => ("Can't continue this conversation", reason),
		};
		div()
			.id("conversation-unavailable")
			.role(Role::Status)
			.aria_label(format!("{title}. {description}"))
			.m_4()
			.p(px(16.))
			.rounded(px(12.))
			.bg(rgb(0x26262b))
			.text_color(rgb(ui_theme::TEXT))
			.flex()
			.flex_col()
			.child(
				div()
					.mb(px(8.))
					.text_size(px(12.))
					.line_height(px(17.))
					.font_weight(FontWeight::MEDIUM)
					.child(title),
			)
			.child(
				div()
					.text_size(px(11.))
					.line_height(px(17.))
					.text_color(rgb(ui_theme::TEXT_MUTED))
					.child(description),
			)
			.when(detail.is_some(), |d| {
				d.child(
					div().mt(px(8.)).flex().justify_end().items_center().child(
						div()
							.id("connection-details")
							.role(Role::Button)
							.aria_label("Technical details")
							.aria_expanded(self.connection_details_expanded)
							.tab_index(0)
							.cursor_pointer()
							.flex()
							.items_center()
							.gap(px(5.))
							.h(px(20.))
							.text_size(px(11.))
							.text_color(rgb(ui_theme::TEXT_MUTED))
							.hover(|s| s.text_color(rgb(ui_theme::TEXT)))
							.child("Details")
							.child(super::super::workspace_symbols::disclosure_chevron(
								"connection-details-chevron",
								self.connection_details_expanded,
							))
							.on_click(cx.listener(|s, _, _, cx| {
								s.toggle_connection_details(cx);
							}))
							.on_key_down(cx.listener(|s, event: &gpui::KeyDownEvent, _, cx| {
								if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
									s.toggle_connection_details(cx);
									cx.stop_propagation();
								}
							}))
							.smooth(),
					),
				)
			})
			.child(crate::ui_motion::disclosure(
				"connection-diagnostic",
				self.connection_details_expanded && detail.is_some(),
				div()
					.pt(px(8.))
					.text_size(px(11.))
					.line_height(px(17.))
					.text_color(rgb(ui_theme::TEXT_MUTED))
					.child(detail.unwrap_or_default().to_owned()),
			))
			.into_any_element()
	}

	fn floating_composer(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
		// Reserve a footer outside the scroll viewport so history cannot leak
		// below the floating glass capsule or receive clicks through its margins.
		div().w_full().flex_shrink_0().child(
			div()
				.w_full()
				.flex()
				.flex_col()
				.when_some(self.composer_unavailable_reason(), |d, reason| {
					d.child(self.unavailable_composer(reason, cx))
				})
				.when(self.composer_unavailable_reason().is_none(), |d| {
					d.child(self.conversation_activity(cx)).child(self.render_composer(window, cx))
				}),
		)
	}

	pub(super) fn selected_is_manager(&self) -> bool {
		self.selected.is_none()
			|| self.selected == self.root_id()
			|| self.snapshot.as_ref().is_some_and(|snapshot| {
				snapshot.work_items.iter().any(|work| {
					Some(&work.id) == self.selected.as_ref()
						&& work.kind == decodex_protocol::ChiefWorkKindDto::Manager
				})
			})
	}

	pub(super) fn render_workspace(
		&mut self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		self.poll_native_agents(cx);
		self.prepare_workspace_history(window, cx);
		let is_chief = self.selected_is_manager();
		let selected = self
			.snapshot
			.as_ref()
			.and_then(|s| s.work_items.iter().find(|w| Some(&w.id) == self.selected.as_ref()))
			.cloned();
		let wide = f32::from(window.viewport_size().width) > 1000.0;
		let mut chat = div()
			.id("conversation-panel-focus")
			.capture_any_mouse_down(cx.listener(|s, _, _, _| s.focused_panel = None))
			.relative()
			.flex_1()
			.min_w_0()
			.min_h_0()
			.overflow_hidden()
			.flex()
			.flex_col()
			.rounded(px(10.))
			.bg(rgba(ui_theme::CHIEF_CHAT_OVERLAY));
		chat = chat
			.when_some(selected.as_ref(), |chat, work| chat.child(self.archive_panel(work, cx)));
		if let (Some(snapshot), Some(work)) = (&self.snapshot, &selected) {
			chat = chat.child(self.work_context(snapshot, work, cx));
		}

		let scroll = self
			.transcript_scroll
			.entry(self.selected.clone().unwrap_or_default())
			.or_default()
			.clone();
		let mut transcript = div()
			.id(SharedString::from(format!(
				"transcript-{}",
				self.selected.as_deref().unwrap_or("chief")
			)))
			.flex_1()
			.min_w_0()
			.min_h_0()
			.overflow_hidden()
			.track_scroll(&scroll)
			.on_scroll_wheel(cx.listener(|s, event, _, cx| s.scroll_history(event, cx)));
		if let Some(work) = &selected {
			if let Some(snapshot) = &self.snapshot {
				let content = if is_chief {
					div()
						.p_4()
						.w_full()
						.mx_auto()
						.line_height(px(ui_theme::BODY_LINE_HEIGHT))
						.child(self.history_panel(work, cx))
						.when(
							snapshot.pending_events.iter().any(|e| {
								e.work_item_id == work.id && e.event_kind.ends_with("_pending")
							}) && self.request.is_none(),
							|row| row.child(self.pending_panel(snapshot, work, cx)),
						)
						.child(self.misalignment_panel(work, cx))
						.child(self.guardian_panel(work, cx))
						.child(self.request_panel(snapshot, work, cx))
						.child(self.async_question_panel(work, cx))
						.into_any_element()
				} else {
					self.details(snapshot, work, cx).into_any_element()
				};
				transcript = transcript.child(content);
			}
		} else {
			transcript = transcript.child(self.workspace_welcome(window, cx));
		}

		chat = chat.child(
			div()
				.flex_1()
				.min_h_0()
				.flex()
				.child(self.history_rail_slot(window, cx))
				.relative()
				.child(transcript)
				.child(self.latest_button(window, cx)),
		);
		if self.native_agents.selected.is_none()
			&& is_chief
			&& selected.is_some()
			&& !self.selected_is_archived()
		{
			chat = chat.child(self.floating_composer(window, cx));
		} else if !is_chief && let Some(work) = selected {
			chat = chat
				.child(self.conversation_activity(cx))
				.child(self.workspace_followup(&work, cx));
		}

		let chat = if self.native_agents.selected.is_some() {
			self.native_agent_view(cx)
		} else {
			chat.into_any_element()
		};
		let (graph_width, graph_height) = self.workspace_graph_size(window, wide);
		self.update_graph_inset(graph_width, graph_height);
		let center = div().flex_1().min_w_0().h_full().flex().flex_col().child(chat).child(reveal(
			"chief-graph-dock",
			graph_height,
			false,
			self.workspace_graph(cx),
		));
		let body = div().flex_1().min_h_0().flex().overflow_hidden().child(center).child(reveal(
			"agent-tree-panel",
			self.agent_tree_width(window),
			true,
			self.agent_tree(cx),
		));
		// Share one glass plane with the left sidebar; only the conversation adds a light tint.
		let main = div()
			.flex_1()
			.min_w_0()
			.h_full()
			.pt(px(super::super::WINDOW_CONTROLS_CLEARANCE))
			.relative()
			.flex()
			.flex_col()
			.bg(rgba(ui_theme::CHIEF_SIDEBAR_MATERIAL))
			.when(!self.pages.is_empty(), |main| main.child(self.workspace_tabs(cx)))
			.child(body);

		self.workspace_resize_root(cx)
			.size_full()
			.flex()
			.text_size(px(ui_theme::BODY_SIZE))
			.font_family(ui_theme::FONT_FAMILY)
			.text_color(rgb(ui_theme::TEXT))
			.on_action(cx.listener(|_, _: &SubmitComposer, _, cx| cx.stop_propagation()))
			.child(self.sidebar_slot(wide, window, cx))
			.child(main)
			.into_any_element()
	}

	fn prepare_workspace_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		self.graph_display_zoom =
			crate::ui_motion::value("chief-graph-zoom", self.graph_zoom, window, cx);
		self.restore_history_anchor(window, cx);
		self.prepare_history_marks();
		self.animate_history_scroll(window, cx);
		self.follow_voice_scroll(window, cx);
		if self.latest_follow_work == self.selected
			&& let Some(scroll) =
				self.selected.as_ref().and_then(|work| self.transcript_scroll.get(work))
		{
			scroll.scroll_to_bottom();
		}
	}

	fn restore_history_anchor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some((id, offset, maximum)) = self.older_scroll_anchor.clone() else {
			return;
		};
		if self.selected.as_ref() != Some(&id) {
			return;
		}
		self.older_scroll_anchor = None;
		if let Some(scroll) = self.transcript_scroll.get(&id).cloned() {
			let entity = cx.entity();
			window.on_next_frame(move |_, cx| {
				let delta = f32::from(scroll.max_offset().y) - maximum;
				scroll.set_offset(point(scroll.offset().x, px(offset - delta)));
				entity.update(cx, |_, cx| cx.notify());
			});
		}
	}

	fn workspace_welcome(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		div()
			.size_full()
			.flex()
			.flex_col()
			.justify_center()
			.items_center()
			.pb(px(90.0))
			.child(div().w_full().max_w(px(672.0)).child(self.render_composer(window, cx)))
			.into_any_element()
	}

	fn workspace_followup(&self, work: &ChiefWorkItemDto, cx: &mut Context<Self>) -> AnyElement {
		let title = work.title.clone();
		let footer = div().p_3().child(self.workspace_action(
			"discuss-with-chief".into(),
			"Discuss this work with Chief →".into(),
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
		footer.into_any_element()
	}

	pub(super) fn work_label(&self, work: &ChiefWorkItemDto) -> String {
		if Some(&work.id) == self.root_id().as_ref()
			&& ["Chief", "Main"].contains(&work.title.as_str())
		{
			return "Main".into();
		}
		if let Some(snapshot) = &self.snapshot {
			if let Some(project) = snapshot.workspaces.iter().find(|p| p.chief_id == work.id) {
				return project.name.clone();
			}
			if work.title == work.id && work.kind == decodex_protocol::ChiefWorkKindDto::Task {
				let position = snapshot
					.work_items
					.iter()
					.filter(|w| w.parent_goal_id == work.parent_goal_id && w.kind == work.kind)
					.position(|w| w.id == work.id)
					.unwrap_or(0) + 1;
				return format!("Agent {position}");
			}
		}
		work.title.clone()
	}

	fn workspace_graph(&self, cx: &mut Context<Self>) -> AnyElement {
		let scope = self.graph_scope.clone().or_else(|| self.root_id());
		let title = self
			.snapshot
			.as_ref()
			.and_then(|s| s.work_items.iter().find(|w| Some(&w.id) == scope.as_ref()))
			.map(|w| self.work_label(w))
			.unwrap_or_else(|| "Work".into());
		let mut panel = self.graph_frame(title, cx).id("graph-panel-focus").capture_any_mouse_down(
			cx.listener(|s, _, _, _| s.focused_panel = Some(workspace_size::Panel::Bottom)),
		);
		let Some(snapshot) = &self.snapshot else {
			return panel.child(div().p_4().child("Work graph unavailable")).into_any_element();
		};
		let layout = self.workspace_graph_layout();
		let zoom = self.graph_zoom;
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
				div()
					.px_2()
					.text_size(px(10.0))
					.text_color(rgb(ui_theme::TEXT_MUTED))
					.child("Blue: reporting · arrows: prerequisites"),
			);
		}
		if layout.cyclic {
			panel = panel.child(
				div()
					.px_2()
					.text_size(px(11.0))
					.text_color(rgb(ui_theme::AMBER))
					.child("Dependency cycle · review work relations"),
			);
		}

		panel
			.child(
				div()
					.flex()
					.justify_start()
					.gap_1()
					.p(px(ui_theme::CONTROL_MARGIN))
					.child(self.workspace_action(
						"zoom-out".into(),
						"−".into(),
						|s, cx| {
							s.graph_zoom = (s.graph_zoom / 1.2).max(0.35);
							cx.notify();
						},
						cx,
					))
					.child(self.workspace_action(
						"zoom-reset".into(),
						format!("{}%", (zoom * 100.0).round()),
						|s, cx| {
							s.graph_zoom = 0.85;
							s.graph_pan = (0.0, 0.0);
							cx.notify();
						},
						cx,
					))
					.child(self.workspace_action(
						"zoom-in".into(),
						"+".into(),
						|s, cx| {
							s.graph_zoom = (s.graph_zoom * 1.2).min(1.8);
							cx.notify();
						},
						cx,
					)),
			)
			.into_any_element()
	}

	fn graph_frame(&self, title: String, cx: &mut Context<Self>) -> gpui::Div {
		let mut panel = div().w_full().min_w_0().h_full().flex().flex_col().pt(px(8.));
		panel = panel.child(
			div()
				.h(px(ui_theme::PANEL_HEADER_HEIGHT))
				.min_h(px(ui_theme::PANEL_HEADER_HEIGHT))
				.flex()
				.items_center()
				.px_2()
				.child(self.workspace_action(
					"graph-up".into(),
					"←".into(),
					|s, cx| {
						s.graph_scope = s
							.snapshot
							.as_ref()
							.and_then(|snap| {
								snap.work_items
									.iter()
									.find(|w| Some(&w.id) == s.graph_scope.as_ref())
							})
							.and_then(|w| w.parent_goal_id.clone());
						s.graph_pan = (0.0, 0.0);
						cx.notify();
					},
					cx,
				))
				.child(
					div()
						.flex_1()
						.overflow_hidden()
						.whitespace_nowrap()
						.text_ellipsis()
						.child(title),
				)
				.child(
					self.workspace_action(
						"graph-expand".into(),
						if self.graph_expanded { "Restore conversation" } else { "Expand graph" }
							.into(),
						|s, cx| {
							s.graph_expanded = !s.graph_expanded;
							cx.notify();
						},
						cx,
					),
				)
				.child(self.workspace_action(
					"graph-close".into(),
					"×".into(),
					|s, cx| {
						s.graph_visible = false;
						s.graph_expanded = false;
						cx.notify();
					},
					cx,
				)),
		);
		panel
	}

	fn graph_canvas(
		&self,
		layout: &graph::Layout,
		cx: &mut Context<Self>,
	) -> gpui::Stateful<gpui::Div> {
		let zoom = self.graph_display_zoom;
		let pan = (self.graph_pan.0 + self.graph_inset.0, self.graph_pan.1 + self.graph_inset.1);
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
		div()
			.id("work-graph-canvas")
			.tab_index(0)
			.role(Role::Group)
			.aria_label("Work graph. Drag or scroll to pan. Control-scroll to zoom.")
			.on_key_down(cx.listener(|s, event: &gpui::KeyDownEvent, _, cx| {
				match event.keystroke.key.as_str() {
					"left" => s.graph_pan.0 += 32.0,
					"right" => s.graph_pan.0 -= 32.0,
					"up" => s.graph_pan.1 += 32.0,
					"down" => s.graph_pan.1 -= 32.0,
					"+" | "=" => s.graph_zoom = (s.graph_zoom * 1.2).min(1.8),
					"-" => s.graph_zoom = (s.graph_zoom / 1.2).max(0.35),
					"escape" => s.graph_expanded = false,
					_ => return,
				};
				cx.stop_propagation();
				cx.notify();
			}))
			.flex_1()
			.min_h_0()
			.relative()
			.overflow_hidden()
			.on_scroll_wheel(cx.listener(|s, event: &gpui::ScrollWheelEvent, _, cx| {
				let delta = event.delta.pixel_delta(px(20.0));
				if event.modifiers.control || event.modifiers.platform {
					s.graph_zoom = (s.graph_zoom + f32::from(delta.y) * 0.002).clamp(0.35, 1.8);
				} else {
					s.graph_pan.0 += f32::from(delta.x);
					s.graph_pan.1 += f32::from(delta.y);
				}
				cx.stop_propagation();
				cx.notify();
			}))
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(|s, event: &gpui::MouseDownEvent, _, _| {
					s.graph_drag = Some(event.position);
				}),
			)
			.on_mouse_up(
				MouseButton::Left,
				cx.listener(|s, _, _, _| {
					s.graph_drag = None;
				}),
			)
			.on_mouse_move(cx.listener(|s, event: &gpui::MouseMoveEvent, _, cx| {
				if event.pressed_button == Some(MouseButton::Left) && s.sidebar_drag.is_none() {
					if let Some(previous) = s.graph_drag {
						s.graph_pan.0 += f32::from(event.position.x - previous.x);
						s.graph_pan.1 += f32::from(event.position.y - previous.y);
						s.graph_drag = Some(event.position);
						cx.notify();
					}
				} else {
					s.graph_drag = None;
				}
			}))
			.child(
				canvas(
					|_, _, _| (),
					move |bounds, _, window, _| {
						for (a, b, report) in edges {
							let start = bounds.origin
								+ point(px(a.0 * zoom + pan.0), px(a.1 * zoom + pan.1));
							let end = bounds.origin
								+ point(px(b.0 * zoom + pan.0), px(b.1 * zoom + pan.1));
							let mut path = PathBuilder::stroke(px(1.0));
							path.move_to(start);
							path.cubic_bezier_to(
								end,
								point((start.x + end.x) * 0.5, start.y),
								point((start.x + end.x) * 0.5, end.y),
							);
							if !report {
								path.move_to(end + point(px(-5.0), px(-3.0)));
								path.line_to(end);
								path.line_to(end + point(px(-5.0), px(3.0)));
							}
							if let Ok(path) = path.build() {
								window.paint_path(
									path,
									rgb(if report { ui_theme::BLUE } else { ui_theme::TEXT_MUTED }),
								);
							}
						}
					},
				)
				.absolute()
				.size_full(),
			)
	}

	fn graph_node(
		&self,
		node: &graph::Node,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let zoom = self.graph_display_zoom;
		let pan = (self.graph_pan.0 + self.graph_inset.0, self.graph_pan.1 + self.graph_inset.1);

		let id = work.id.clone();
		let key = id.clone();
		let (status, color) = self
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
		let element = div()
			.id(SharedString::from(format!("graph-node-{id}")))
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!("{}: {status}. Open conversation.", self.work_label(work)))
			.tooltip(move |_, cx| cx.new(|_| PanelTip(tip.clone())).into())
			.absolute()
			.left(px(node.x * zoom + pan.0))
			.top(px(node.y * zoom + pan.1))
			.w(px(160.0 * zoom))
			.h(px(52.0 * zoom))
			.px_2()
			.py_1()
			.rounded(px(9.0))
			.bg(if self.graph_selected.as_ref() == Some(&id) {
				rgba(0x35353ce8)
			} else {
				rgba(0x242427db)
			})
			.hover(|style| style.bg(rgba(0x3a3a40eb)))
			.cursor_pointer()
			.overflow_hidden()
			.text_size(px((12.0 * zoom).max(10.0)))
			.on_click(cx.listener(move |s, _, _, cx| {
				s.open_page(&id, cx);
				s.graph_visible = true;
				s.graph_selected = Some(id.clone());
				cx.notify();
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
				if event.keystroke.key == "enter" {
					s.open_page(&key, cx);
				}
			}))
			.child(div().whitespace_nowrap().text_ellipsis().child(self.work_label(work)))
			.child(div().text_size(px(10.0)).text_color(rgb(color)).child(status));
		element.into_any_element()
	}
}

pub(super) fn within_project(snapshot: &ChiefSnapshotDto, project: &str, work: &str) -> bool {
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
	format!("{:02}:{:02}:{:02}", (seconds / 3600) % 24, (seconds / 60) % 60, seconds % 60)
}

#[cfg(any(test, feature = "visual-capture"))]
impl ChiefSurface {
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
			self.visual_workspace_page("markdown", cx);
			self.composer.update(cx, |input,cx|input.set_content("Review the interface and simplify the controls.\nKeep the glass material and check keyboard navigation.",cx));
			self.capabilities = Some(decodex_protocol::ChiefCapabilitiesResult::Available {
				models: ["gpt-6-astra", "gpt-5.6-sol"]
					.into_iter()
					.map(|name| decodex_protocol::ChiefModelDto {
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
						supports_images: true,
						availability: None,
						upgrade: None,
					})
					.collect(),
				memory_enabled: None,
			});
			self.fast = true;
			if page == "composer-menu" {
				self.composer_menu = Some("model");
			}
			if page == "composer-effort" {
				self.composer_menu = Some("model");
			}
			return;
		}
		if ["questions", "approval", "live", "hierarchy"].contains(&page) {
			self.visual_functional_page(page, cx);
			return;
		}
		match page {
			"worker" => self.open_page("verify", cx),
			"empty" | "empty-draft" => {
				self.sidebar_visible = false;
				if page == "empty-draft" {
					self.composer.update(cx, |input, cx| {
						input.set_content("Review the release plan with me.", cx)
					});
				}
				self.snapshot = Some(ChiefSnapshotDto {
					workspaces: vec![],
					work_items: vec![],
					dependencies: vec![],
					pending_events: vec![],
				});
				self.selected = None;
				self.history = None;
				self.pages.clear();
			},
			"expanded" => self.graph_expanded = true,
			"in-use" => {
				self.graph_visible = false;
				self.timeline_visible = false;
				self.snapshot.as_mut().expect("fixture").pending_events.push(
					decodex_protocol::ChiefPendingEventDto {
						id: 999,
						source_event_id: "fixture-in-use".into(),
						work_item_id: "chief".into(),
						event_kind: "thread_in_use_needs_attention".into(),
						created_at_micros: 1,
						delivery_claimed: false,
					},
				);
			},
			"compact-graph" => {
				self.graph_scope = Some("chief".into());
				self.sidebar_width = 280.0;
				self.timeline_visible = false;
			},
			"markdown" => {
				self.graph_visible = false;
				self.timeline_visible = false;
				if let Some((_, ChiefHistoryResult::Available { entries, usage, .. })) =
					&mut self.history
				{
					*usage = Some(decodex_protocol::ChiefUsageDto {
						input_tokens: 24860,
						output_tokens: 1820,
						context_tokens: 26700,
						context_window: Some(128000),
					});
					entries.clear();
					for (i,(kind,text)) in [("user","请整理检查结果，并说明下一步安排。"),("assistant","## 检查完成\n\n两位下属已提交报告，**现有会话保持可用**。\n\n- 登录流程：保留原会话\n- 启动流程：继续验证性能\n\n| 工作 | 结果 | 下一步 |\n|---|---|---|\n| 登录检查 | 已验收 | 合并检查结果 |\n| 启动检查 | 待验证 | 补充冷启动数据 |\n\n### 验证命令\n```rust\nlet status = review.result();\nassert!(status.is_verified());\n```\n\n查看 [源码](/Users/x/code/acg-box/decodex/apps/decodex-gpui/src/chief_surface.rs:1)，再确认 `review` 的结果。")].into_iter().enumerate() {
                        entries.push(decodex_protocol::ChiefHistoryEntryDto{ activity: None,usage: (kind == "assistant").then_some(decodex_protocol::ChiefTurnUsageDto {input_tokens:24860,output_tokens:1820}),duration_ms: (kind == "assistant").then_some(18400),id:i as i64+1,kind:kind.into(),text:text.into(),created_at_micros:1789480440000000});
                    }
				}
			},
			"task-references" => self.visual_task_references(),
			"conversation" => {
				self.graph_visible = false;
				self.timeline_visible = false;
			},
			_ => {},
		}
		cx.notify();
	}

	/// Explicit capture fixture. Never installed by the normal application path.
	pub(crate) fn visual_workspace_fixture(&mut self, cx: &mut Context<Self>) {
		self.sidebar_visible = true;
		use decodex_protocol::{ChiefDependencyDto, ChiefHistoryEntryDto, ChiefWorkKindDto};
		let make =
			|id: &str, parent: Option<&str>, title: &str, status, dispatch| ChiefWorkItemDto {
				id: id.into(),
				parent_goal_id: parent.map(str::to_owned),
				kind: if id == "chief" || id == "release" {
					ChiefWorkKindDto::Goal
				} else {
					ChiefWorkKindDto::Task
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
		use ChiefDispatchStateDto::{Idle, Running};
		use ChiefWorkStatusDto::{Open, Resolved, UserDecision};
		self.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
			workspaces: vec![],
			work_items: vec![
				make("chief", None, "Chief", Open, Idle),
				make("release", Some("chief"), "September release", UserDecision, Idle),
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
			.map(|(a, b)| ChiefDependencyDto { work_item_id: b.into(), depends_on_id: a.into() })
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
		let history = ChiefHistoryResult::Available {
			questions: vec![],
			questions_truncated: false,
			questions_recovering: false,
			misalignment: None,
			usage: None,
			entries: messages
				.into_iter()
				.enumerate()
				.map(|(i, (kind, text))| ChiefHistoryEntryDto {
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
		self.history_cache.insert("chief".into(), history.clone());
		self.history = Some(("chief".into(), history));
		self.selected = Some("chief".into());
		self.pages = vec!["verify".into()];
		self.graph_scope = Some("release".into());
		self.graph_selected = Some("verify".into());
		self.timeline_visible = true;
		self.history_cache.insert("verify".into(),ChiefHistoryResult::Available{questions:vec![],questions_truncated:false,questions_recovering:false,misalignment:None,usage: None,entries:vec![ChiefHistoryEntryDto{ activity: None,usage: None,duration_ms: None,id:100,kind:"assistant".into(),text:"Checking that existing sessions reopen without another sign-in. Fresh-install verification is still running.".into(),created_at_micros:1_789_481_040_000_000}],has_more:false,next_before:None,live:vec![]});
		cx.notify();
	}
}

pub(super) fn selected_history_available(surface: &ChiefSurface) -> bool {
	surface.has_work() || surface.history.as_ref().is_some_and(
		|(_, history)| matches!(history,ChiefHistoryResult::Available{entries,..} if !entries.is_empty()),
	)
}

struct PanelTip(String);
impl Render for PanelTip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
			.px_2()
			.py_1()
			.rounded(px(5.0))
			.bg(rgb(ui_theme::CANVAS))
			.text_color(rgb(ui_theme::TEXT))
			.text_size(px(11.0))
			.child(self.0.clone())
	}
}

fn panel_icon(id: &str) -> Option<AnyElement> {
	use super::super::workspace_symbols::{self, Symbol};
	let symbol = match id {
		"workspace-sidebar" => Symbol::Sidebar,
		"workspace-graph" => Symbol::Graph,
		"workspace-timeline" => Symbol::Timeline,
		"workspace-agents" => Symbol::Agents,
		"graph-expand" => Symbol::Expand,
		"graph-close" | "timeline-close" | "tree-close" => Symbol::Close,
		"graph-up" => Symbol::Back,
		"zoom-in" => Symbol::Plus,
		"zoom-out" => Symbol::Minus,
		id if id.starts_with("close-") => Symbol::Close,
		_ => return None,
	};
	Some(workspace_symbols::icon(symbol))
}

impl ChiefSurface {
	fn conversation_activity(&self, cx: &mut Context<Self>) -> AnyElement {
		let selected = self.snapshot.as_ref().and_then(|snapshot| {
			snapshot.work_items.iter().find(|work| Some(&work.id) == self.selected.as_ref())
		});
		let notice = self.status_notice();
		let current = selected.and_then(|work| self.current_activity_label(work));
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
		let label = if self.sending {
			Some("Sending…")
		} else if self.uncertain {
			Some("Delivery unconfirmed · Draft kept. Sending is paused to avoid duplicates.")
		} else if self.selected.as_deref().is_some_and(|id| self.thread_in_use(id)) {
			Some("In use in another app · Your message is saved and waiting")
		} else if pending.iter().any(|event| {
			event.event_kind.ends_with("_needs_attention") || event.event_kind.ends_with("_failed")
		}) {
			Some("This conversation needs attention. Its current operation could not complete.")
		} else if matches!(self.displayed_load_state(), LoadState::Unavailable | LoadState::Stale) {
			Some("Connection unavailable · Reconnecting")
		} else if !self.feedback.is_empty() && self.feedback != "Message saved · Waiting for agent…"
		{
			Some(self.feedback.as_str())
		} else {
			selected.and_then(|work| match work.dispatch_state {
				ChiefDispatchStateDto::Dispatching => Some("Starting…"),
				ChiefDispatchStateDto::Running => Some(current.as_deref().unwrap_or("Working…")),
				ChiefDispatchStateDto::Unknown =>
					Some("Connection interrupted · Checking delivery"),
				ChiefDispatchStateDto::Idle
					if pending.iter().any(|event| event.event_kind == "user_message") =>
					Some("Message saved · Waiting for agent…"),
				ChiefDispatchStateDto::Idle
					if self.feedback == "Message saved · Waiting for agent…" =>
					Some(self.feedback.as_str()),
				ChiefDispatchStateDto::Idle => None,
			})
		}
		.or_else(|| notice.as_ref().map(|(_, detail, _)| detail.as_str()))
		.or_else(|| (!self.archive.feedback.is_empty()).then_some(self.archive.feedback.as_str()));
		let Some(label) = label else {
			return div().into_any_element();
		};
		let stop = selected
			.filter(|work| work.dispatch_state == ChiefDispatchStateDto::Running)
			.and_then(|work| {
				Some((
					EntityId::new(work.id.clone()).ok()?,
					WireText::new(work.active_turn_id.clone()?).ok()?,
				))
			});
		div()
			.id("conversation-activity-status")
			.role(Role::Status)
			.aria_label(label.to_owned())
			.w_full()
			.px_4()
			.py_1()
			.flex()
			.items_center()
			.justify_center()
			.gap_3()
			.text_size(px(11.0))
			.text_color(rgb(ui_theme::TEXT_MUTED))
			.child(label.to_owned())
			.when_some(stop, |row, (work_id, turn_id)| {
				row.child(
					div()
						.id("conversation-stop")
						.role(Role::Button)
						.tab_index(0)
						.aria_label("Stop response")
						.h(px(26.0))
						.px_2()
						.flex()
						.items_center()
						.rounded(px(5.0))
						.cursor_pointer()
						.hover(|style| style.bg(rgba(0xffffff10)))
						.on_click(cx.listener(move |s, _, _, cx| {
							s.execute(
								ChiefActionDto::Interrupt {
									work_id: work_id.clone(),
									turn_id: turn_id.clone(),
								},
								None,
								cx,
							)
						}))
						.child("Stop")
						.smooth(),
				)
			})
			.into_any_element()
	}
}

#[cfg(any(test, feature = "visual-capture"))]
impl ChiefSurface {
	fn visual_functional_page(&mut self, page: &str, cx: &mut Context<Self>) {
		self.graph_visible = false;
		self.timeline_visible = false;
		self.sidebar_visible = page == "hierarchy";
		if page == "hierarchy" {
			let snapshot = self.snapshot.as_mut().expect("fixture");
			let project =
				snapshot.work_items.iter_mut().find(|work| work.id == "release").expect("fixture");
			project.kind = decodex_protocol::ChiefWorkKindDto::Manager;
			snapshot.workspaces.push(decodex_protocol::ChiefWorkspaceDto {
				chief_id: "release".into(),
				name: "September release".into(),
				directory: "/Users/demo/projects/release".into(),
			});
			self.open_page("release", cx);
			return;
		}
		if page == "live" {
			if let Some((_, ChiefHistoryResult::Available { live, .. })) = &mut self.history {
				live.push(decodex_protocol::ChiefLiveMessageDto {turn_id:"live-turn".into(),item_id:"live-item".into(),text:"The compatibility check is progressing. I’m reviewing the existing session behavior and…".into(),truncated:false});
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
			decodex_protocol::ChiefPendingEventDto {
				id: 987,
				source_event_id: "fixture-request".into(),
				work_item_id: "chief".into(),
				event_kind: "user_input_pending".into(),
				created_at_micros: 1,
				delivery_claimed: false,
			},
		);
		let request = ChiefRequestResult::Available {
			work_id: "chief".into(),
			event_id: 987,
			method: method.into(),
			request_json: HistoryText::new(value.to_string()).expect("bounded fixture"),
		};
		self.prepare_question_inputs(&request, cx);
		self.request = Some(request);
		self.transcript_scroll.entry("chief".into()).or_default().scroll_to_bottom();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn connection_details_match_the_current_failure_not_an_old_log(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.snapshot.as_mut().unwrap().pending_events =
				vec![decodex_protocol::ChiefPendingEventDto {
					id: 99,
					source_event_id: "failure".into(),
					work_item_id: "chief".into(),
					event_kind: "reconnection_needs_attention".into(),
					created_at_micros: 1,
					delivery_claimed: false,
				}];
			let Some((_, ChiefHistoryResult::Available { entries, .. })) = &mut s.history else {
				panic!("fixture");
			};
			let mut entry = entries[0].clone();
			entry.id = 99;
			entry.kind = "system".into();
			entry.text = "Chief process requires recovery: ProcessUnavailable".into();
			entries.push(entry);
			assert!(s.connection_failure_detail().unwrap().contains("ProcessUnavailable"));
			s.snapshot.as_mut().unwrap().pending_events[0].id = 100;
			assert!(s.connection_failure_detail().is_none(), "never reuse an obsolete error");
		});
	}

	#[gpui::test]
	fn unavailable_thread_blocks_submission_without_losing_draft(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.snapshot.as_mut().unwrap().pending_events.clear();
			assert!(s.composer_unavailable_reason().is_none());
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
			s.snapshot.as_mut().unwrap().pending_events.push(
				decodex_protocol::ChiefPendingEventDto {
					id: 99,
					source_event_id: "offline".into(),
					work_item_id: "chief".into(),
					event_kind: "reconnection_needs_attention".into(),
					created_at_micros: 1,
					delivery_claimed: false,
				},
			);
			assert!(s.composer_unavailable_reason().unwrap().contains("could not reconnect"));
			s.submit(cx);
			assert!(!s.sending);
			assert!(s.command_task.is_none());
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
			s.snapshot.as_mut().unwrap().pending_events.clear();
			assert!(s.composer_unavailable_reason().is_none());
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		});
	}

	#[gpui::test]
	fn pages_reuse_identity_preserve_draft_and_return_to_chief(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep my draft", cx));
			s.graph_pan = (15.0, 25.0);
			s.graph_zoom = 1.2;
			s.open_page("verify", cx);
			s.open_page("verify", cx);
			assert_eq!(s.pages, vec!["verify"]);
			assert_eq!(s.selected.as_deref(), Some("verify"));
			assert!(s.history.as_ref().is_some_and(|(id, _)| id == "verify"));
			s.close_page("verify", cx);
			assert_eq!(s.selected.as_deref(), Some("chief"));
			assert!(s.pages.is_empty());
			assert_eq!(s.graph_pan, (15.0, 25.0));
			assert_eq!(s.graph_zoom, 1.2);
			assert_eq!(s.graph_scope.as_deref(), Some("release"));
			assert_eq!(s.composer.read(cx).content(), "Keep my draft");
			s.open_page("missing", cx);
			assert_eq!(s.selected.as_deref(), Some("chief"));
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
	}
	#[gpui::test]
	fn manager_switches_keep_drafts_with_their_recipient(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| work.id == "release")
				.unwrap()
				.kind = decodex_protocol::ChiefWorkKindDto::Manager;
			s.composer.update(cx, |input, cx| input.set_content("Main Chief draft", cx));
			s.open_page("release", cx);
			assert_eq!(s.composer.read(cx).content(), "");
			s.composer.update(cx, |input, cx| input.set_content("Project Chief draft", cx));
			s.open_page("chief", cx);
			assert_eq!(s.composer.read(cx).content(), "Main Chief draft");
			s.open_page("release", cx);
			assert_eq!(s.composer.read(cx).content(), "Project Chief draft");
		});
	}

	#[gpui::test]
	fn graph_layers_dependencies_and_flags_cycles(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
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
			snapshot.dependencies.push(decodex_protocol::ChiefDependencyDto {
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
