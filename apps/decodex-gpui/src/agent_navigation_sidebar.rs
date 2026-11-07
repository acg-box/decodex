//! Search and navigate personal topics, Work and project Agents in one sidebar.
#[path = "agent_navigation_actions.rs"] mod actions;
use crate::{
	shell::{
		agent_surface::{AgentSurface, Context},
		workspace_symbols::{self, Symbol},
	},
	ui_motion::SmoothControl as _,
	ui_theme::{
		self, NAVIGATION_DETAIL_SIZE, NAVIGATION_ROW_RADIUS, NAVIGATION_SECTION_GAP,
		NAVIGATION_TITLE_SIZE, TEXT, TEXT_MUTED,
	},
};
use decodex_protocol::work::WorkState;
use gpui::{
	AnyElement, AppContext as _, FontWeight, KeyDownEvent, Role, SharedString, Task,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct State {
	collapsed: HashSet<&'static str>,
	menu: Option<String>,
	confirm_delete: bool,
	mutation: Option<Task<()>>,
	feedback: Option<String>,
}
struct NavigationRow {
	id: String,
	title: String,
	detail: Option<String>,
	active: bool,
	symbol: Symbol,
	conversation: Option<String>,
}
impl AgentSurface {
	pub(super) fn project_conversation_pages(&self) -> Vec<(String, String, bool)> {
		let mut pages = self.conversation_pages();
		if let Some(snapshot) = &self.snapshot {
			for agent in snapshot.agents.iter() {
				if !pages.iter().any(|(id, _, _)| id == &agent.id)
					&& !snapshot.archived_work_ids.contains(&agent.id)
				{
					let label = snapshot
						.work_items
						.iter()
						.find(|w| w.id == agent.id)
						.map(|w| self.conversation_label(w))
						.unwrap_or_else(|| agent.name.clone());
					pages.push((agent.id.clone(), label, false));
				}
			}
		}
		pages
			.into_iter()
			.filter(|(id, _, _)| {
				!self.is_assistant_conversation(id)
					&& !self
						.snapshot
						.as_ref()
						.and_then(|snapshot| self.workspace.attached.family_root(id, snapshot))
						.is_some_and(|root| root != id)
			})
			.collect()
	}

	pub(super) fn open_navigation_conversation(&mut self, id: &str, cx: &mut Context<Self>) {
		self.workspace.work_detail = None;
		self.workspace.navigation.menu = None;
		self.open_page(id, cx);
	}

	pub(super) fn navigation_work_context(&self) -> Option<AnyElement> {
		let id = self.workspace.work_detail.as_ref()?;
		let work = self
			.snapshot
			.as_ref()?
			.portfolio
			.works
			.iter()
			.find(|w| &w.id == id && Some(&w.owner_agent_id) == self.selected.as_ref())?;
		Some(
			gpui::div()
				.min_w_0()
				.max_w(gpui::px(180.))
				.truncate()
				.text_size(gpui::px(NAVIGATION_DETAIL_SIZE))
				.text_color(ui_theme::rgb(TEXT_MUTED))
				.child(format!("About: {}", work.title))
				.into_any_element(),
		)
	}

	pub(super) fn render_navigation_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
		let query = self.work_search.read(cx).content().trim().to_lowercase();
		gpui::div()
			.id("agent-sidebar")
			.size_full()
			.flex()
			.flex_col()
			.gap_2()
			.px(gpui::px(6.))
			.py(gpui::px(10.))
			.child(
				gpui::div()
					.h(gpui::px(28.))
					.flex_none()
					.overflow_hidden()
					.child(self.work_search.clone()),
			)
			.child(
				gpui::div()
					.id("workspace-navigation-scroll")
					.flex_1()
					.min_h_0()
					.overflow_y_scroll()
					.flex()
					.flex_col()
					.gap(gpui::px(NAVIGATION_SECTION_GAP))
					.child(self.personal_navigation(&query, cx))
					.child(self.outcome_navigation(&query, cx))
					.child(self.project_navigation(&query, cx)),
			)
			.children(self.workspace.navigation.feedback.as_ref().map(|message| {
				gpui::div()
					.px_2()
					.text_size(gpui::px(NAVIGATION_DETAIL_SIZE))
					.text_color(ui_theme::rgb(ui_theme::ERROR))
					.child(message.clone())
			}))
			.child(gpui::div().h(gpui::px(1.)).bg(ui_theme::rgba(ui_theme::PANEL_DIVIDER)))
			.child(self.work_navigation(
				"archive-browser",
				"Archived conversations",
				Symbol::Archive,
				|s, cx| s.open_archive_catalog(cx),
				cx,
			))
			.child(self.sidebar_settings(cx))
			.into_any_element()
	}

	fn sidebar_settings(&self, cx: &mut Context<Self>) -> AnyElement {
		self.navigation_row(
			NavigationRow {
				id: "sidebar-settings".into(),
				title: "Settings".into(),
				detail: None,
				active: false,
				symbol: Symbol::Settings,
				conversation: None,
			},
			|_, window, cx| window.dispatch_action(Box::new(crate::shell::ActivateSettings), cx),
			cx,
		)
	}

	fn navigation_section(
		&self,
		id: &'static str,
		label: &'static str,
		create: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let open = !self.workspace.navigation.collapsed.contains(id)
			|| !self.work_search.read(cx).content().trim().is_empty();
		let name = if id == "assistant" { "New Assistant topic" } else { "New project Agent" };
		let mut heading = gpui::div()
			.id(SharedString::from(format!("navigation-section-{id}")))
			.debug_selector(move || format!("navigation-section-{id}"))
			.h(gpui::px(28.))
			.flex_none()
			.whitespace_nowrap()
			.overflow_hidden()
			.flex()
			.items_center()
			.gap_1()
			.text_color(ui_theme::rgb(TEXT_MUTED))
			.child(
				gpui::div()
					.id(SharedString::from(format!("navigation-toggle-{id}")))
					.debug_selector(move || format!("navigation-toggle-{id}"))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(label)
					.aria_expanded(open)
					.flex_1()
					.min_w_0()
					.h_full()
					.px_1()
					.flex()
					.items_center()
					.gap_1()
					.cursor_pointer()
					.on_click(cx.listener(move |s, _, _, cx| {
						if !s.workspace.navigation.collapsed.remove(id) {
							s.workspace.navigation.collapsed.insert(id);
						}
						cx.notify();
					}))
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							if !s.workspace.navigation.collapsed.remove(id) {
								s.workspace.navigation.collapsed.insert(id);
							}
							cx.stop_propagation();
							cx.notify();
						}
					}))
					.child(workspace_symbols::disclosure_chevron(
						SharedString::from(format!("navigation-chevron-{id}")),
						open,
					))
					.child(
						gpui::div()
							.truncate()
							.font_weight(FontWeight::MEDIUM)
							.text_size(gpui::px(NAVIGATION_TITLE_SIZE))
							.child(label),
					),
			);
		if create {
			heading = heading.child(
				gpui::div()
					.id(SharedString::from(format!("navigation-create-{id}")))
					.debug_selector(move || format!("navigation-create-{id}"))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(name)
					.tooltip(move |_, cx| cx.new(|_| crate::shell::ControlTooltip(name)).into())
					.size(gpui::px(24.))
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.rounded(gpui::px(5.))
					.cursor_pointer()
					.hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.on_click(cx.listener(move |s, _, _, cx| {
						s.workspace.navigation.collapsed.remove(id);
						if id == "assistant" {
							s.new_assistant_conversation(cx);
						} else {
							s.new_work_conversation(cx);
						}
					}))
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.workspace.navigation.collapsed.remove(id);
							if id == "assistant" {
								s.new_assistant_conversation(cx);
							} else {
								s.new_work_conversation(cx);
							}
							cx.stop_propagation();
						}
					}))
					.child(workspace_symbols::icon_sized(Symbol::Plus, 14.)),
			);
		}
		heading.into_any_element()
	}

	fn personal_navigation(&self, query: &str, cx: &mut Context<Self>) -> AnyElement {
		let mut group = gpui::div().flex().flex_col().gap_1().child(self.navigation_section(
			"assistant",
			"My Assistant",
			true,
			cx,
		));
		if query.is_empty() && self.workspace.navigation.collapsed.contains("assistant") {
			return group.into_any_element();
		}
		if let Some(snapshot) = &self.snapshot {
			for id in snapshot.assistant.topic_ids.iter().chain(snapshot.assistant.agent_id.iter())
			{
				if snapshot.archived_work_ids.contains(id) {
					continue;
				}
				let Some(work) = snapshot.work_items.iter().find(|w| &w.id == id) else { continue };
				if work.title == "Assistant"
					&& work.codex_thread_id.is_none()
					&& !self.assistant_home_has_draft(id, cx)
				{
					continue;
				}
				let title = self.assistant_conversation_label(work);
				if !query_matches(query, &title) {
					continue;
				}
				let target = id.clone();
				group = group.child(self.navigation_row(
					NavigationRow {
						id: format!("assistant-topic-{id}"),
						title,
						detail: work.codex_thread_id.is_none().then(|| "Draft".into()),
						active: !self.workspace.browsing
							&& self.selected.as_ref() == Some(id)
							&& self.workspace.work_detail.is_none(),
						symbol: Symbol::Document,
						conversation: Some(id.clone()),
					},
					move |s, _, cx| s.open_navigation_conversation(&target, cx),
					cx,
				));
			}
		} else {
			group = group.child(self.work_navigation(
				"personal-assistant",
				"Open My Assistant",
				Symbol::TeamMembers,
				|s, cx| s.open_assistant(cx),
				cx,
			));
		}
		group.into_any_element()
	}

	fn assistant_home_has_draft(&self, id: &str, cx: &Context<Self>) -> bool {
		if self.composer_manager.as_deref() == Some(id) {
			!self.composer.read(cx).content().is_empty()
				|| !self.attachments.is_empty()
				|| !self.task_references.is_empty()
		} else {
			self.draft_profiles.texts.get(id).is_some_and(|text| !text.is_empty())
				|| self.draft_profiles.files.get(id).is_some_and(|files| !files.is_empty())
				|| self.draft_profiles.tasks.get(id).is_some_and(|tasks| !tasks.is_empty())
		}
	}

	fn outcome_navigation(&self, query: &str, cx: &mut Context<Self>) -> AnyElement {
		let mut group = gpui::div()
			.flex()
			.flex_col()
			.gap_1()
			.child(self.navigation_section("work", "Work", false, cx));
		if query.is_empty() && self.workspace.navigation.collapsed.contains("work") {
			return group.into_any_element();
		}
		if let Some(snapshot) = &self.snapshot {
			for work in &snapshot.portfolio.works {
				let project = work
					.project_id
					.as_ref()
					.and_then(|id| snapshot.projects.iter().find(|p| &p.id == id))
					.map(|p| p.name.as_str())
					.unwrap_or("");
				if !query_matches(query, &format!("{} {project}", work.title)) {
					continue;
				}
				let id = work.id.clone();
				let owner = work.owner_agent_id.clone();
				let status = if snapshot.portfolio.awaiting_assessment.contains(&work.id) {
					"Awaiting assessment"
				} else {
					match work.state {
						WorkState::Open => "In progress",
						WorkState::Blocked => "Blocked",
						WorkState::Completed => "Completed",
						WorkState::Cancelled => "Cancelled",
					}
				};
				group = group.child(self.navigation_row(
					NavigationRow {
						id: format!("navigation-work-{}", work.id),
						title: work.title.clone(),
						detail: Some(status.into()),
						active: self.workspace.work_detail.as_ref() == Some(&work.id),
						symbol: Symbol::ActivityGoal,
						conversation: None,
					},
					move |s, _, cx| {
						s.open_navigation_conversation(&owner, cx);
						s.workspace.work_detail = Some(id.clone());
						s.load_work_detail(id.clone(), cx);
					},
					cx,
				));
			}
		}
		group.into_any_element()
	}

	fn project_navigation(&self, query: &str, cx: &mut Context<Self>) -> AnyElement {
		let mut group = gpui::div().flex().flex_col().gap_1().child(self.navigation_section(
			"projects",
			"Project Agents",
			true,
			cx,
		));
		if query.is_empty() && self.workspace.navigation.collapsed.contains("projects") {
			return group.into_any_element();
		}
		for (id, title, _) in self.project_conversation_pages() {
			let project = self
				.snapshot
				.as_ref()
				.and_then(|s| s.projects.iter().find(|p| p.agent_ids.contains(&id)))
				.map(|p| p.name.clone());
			if !query_matches(query, &format!("{title} {}", project.as_deref().unwrap_or(""))) {
				continue;
			}
			let target = id.clone();
			group = group.child(self.navigation_row(
				NavigationRow {
					id: format!("page-{id}"),
					title,
					detail: project,
					active: !self.workspace.browsing
						&& self.selected.as_ref() == Some(&id)
						&& self.workspace.work_detail.is_none(),
					symbol: Symbol::Agents,
					conversation: Some(id),
				},
				move |s, _, cx| s.open_navigation_conversation(&target, cx),
				cx,
			));
		}
		group.into_any_element()
	}

	fn navigation_row(
		&self,
		row: NavigationRow,
		action: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let action = std::rc::Rc::new(action);
		let keyboard = action.clone();
		let debug = row.id.clone();
		let icon_debug = format!("navigation-icon-{}", row.id);
		let conversation = row.conversation.is_some();
		let group = SharedString::from(format!("group-{}", row.id));
		let menu = row
			.conversation
			.as_deref()
			.map(|id| self.navigation_conversation_actions(id, &group, cx));
		gpui::div()
			.id(SharedString::from(row.id))
			.debug_selector(move || debug.clone())
			.group(group)
			.relative()
			.role(Role::Button)
			.tab_index(0)
			.aria_label(row.title.clone())
			.aria_selected(row.active)
			.w_full()
			.min_w_0()
			.flex_none()
			.py(gpui::px(5.))
			.when(conversation, |row| row.h(gpui::px(28.)).py_0())
			.flex()
			.items_center()
			.gap(gpui::px(6.))
			.rounded(gpui::px(NAVIGATION_ROW_RADIUS))
			.cursor_pointer()
			.bg(ui_theme::rgba(if row.active { ui_theme::SELECTED_FILL } else { 0xffffff00 }))
			.hover(move |el| {
				el.bg(ui_theme::rgba(if row.active {
					ui_theme::SELECTED_HOVER_FILL
				} else {
					ui_theme::HOVER_FILL
				}))
			})
			.focus_visible(|el| el.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.child(
				gpui::div()
					.debug_selector(move || icon_debug.clone())
					.flex_none()
					.size(gpui::px(28.))
					.flex()
					.items_center()
					.justify_center()
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.child(workspace_symbols::icon_sized(row.symbol, 14.)),
			)
			.child(
				gpui::div()
					.min_w_0()
					.flex_1()
					.flex()
					.when(!conversation, |text| text.flex_col())
					.gap(gpui::px(3.))
					.child(
						gpui::div()
							.when(conversation, |text| text.flex_1().min_w_0())
							.truncate()
							.line_height(gpui::px(18.))
							.text_size(gpui::px(NAVIGATION_TITLE_SIZE))
							.text_color(ui_theme::rgb(TEXT))
							.child(row.title),
					)
					.children(row.detail.map(|detail| {
						gpui::div()
							.when(conversation, |text| {
								text.max_w(gpui::px(80.)).whitespace_nowrap()
							})
							.truncate()
							.text_size(gpui::px(NAVIGATION_DETAIL_SIZE))
							.text_color(ui_theme::rgb(TEXT_MUTED))
							.child(detail)
					})),
			)
			.children(menu)
			.on_click(cx.listener(move |s, _, window, cx| action(s, window, cx)))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, window, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					keyboard(s, window, cx);
					cx.stop_propagation();
				}
			}))
			.smooth()
			.into_any_element()
	}
}
fn query_matches(query: &str, text: &str) -> bool {
	let text = text.to_lowercase();
	query.split_whitespace().all(|word| text.contains(word))
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	pub(super) fn visual_personal_navigation(&mut self, cx: &mut Context<Self>) {
		use decodex_protocol::work::{Agent, Work, WorkRequirements};
		let history = self.history.clone();
		self.visual_workbench_page("assistant-workspace", cx);
		self.history = history;
		self.timeline.native.reset();
		self.workspace.sidebar_visible = true;
		self.workspace.graph_visible = false;
		self.workspace.graph_expanded = false;
		self.workspace.agent_panel_width = 320.;
		self.workspace.sidebar_width = 252.;
		self.workspace.work_detail = Some("launch".into());
		let snapshot = self.snapshot.as_mut().expect("visual workspace");
		snapshot.assistant.agent_id = Some("agent".into());
		snapshot.agents.push(Agent {
			id: "agent".into(),
			name: "My Assistant".into(),
			native_thread_id: None,
			assistant: true,
		});
		let personal =
			snapshot.work_items.iter_mut().find(|w| w.id == "agent").expect("visual Assistant");
		personal.title = "Release priorities".into();
		personal.codex_thread_id = None;
		personal.active_turn_id = None;
		personal.dispatch_state = decodex_protocol::AgentDispatchStateDto::Idle;
		personal.status = decodex_protocol::AgentWorkStatusDto::Wait;
		let seed = personal.clone();
		for (id, title) in
			[("topic-research", "Research workflow"), ("topic-preferences", "Working preferences")]
		{
			let mut topic = seed.clone();
			topic.id = id.into();
			topic.title = title.into();
			snapshot.work_items.push(topic);
			snapshot.assistant.topic_ids.push(id.into());
		}
		for (id, title) in [("launch", "Launch Decodex"), ("research", "Choose a research stack")] {
			snapshot.portfolio.works.push(Work {
				id: id.into(),
				project_id: None,
				owner_agent_id: "agent".into(),
				title: title.into(),
				requirements: WorkRequirements {
					id: format!("{id}-requirements"),
					goal: title.into(),
					criteria: "Review the original deliverable".into(),
					source: "visual-fixture-user-input".into(),
				},
				state: WorkState::Open,
				reason: None,
				resume_condition: None,
				user_hold: false,
				assessment: None,
			});
		}
		snapshot.portfolio.awaiting_assessment.push("launch".into());
		for (id, owner, kind) in
			[(1001, "mira", "user_input_pending"), (1002, "sage", "permission_pending")]
		{
			snapshot.pending_events.push(decodex_protocol::AgentPendingEventDto {
				id,
				source_event_id: format!("visual-request-{id}"),
				work_item_id: owner.into(),
				event_kind: kind.into(),
				created_at_micros: 1,
				delivery_claimed: false,
			});
		}
		cx.notify();
	}
}

#[cfg(test)]
#[path = "agent_navigation_sidebar_tests.rs"]
mod interaction_tests;
#[cfg(test)]
mod tests {
	use super::AgentSurface;
	use gpui::TestAppContext;

	#[gpui::test]
	fn personal_topics_are_separate_navigation_and_switches_keep_each_draft(
		cx: &mut TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(960.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			let snapshot = s.snapshot.as_mut().unwrap();
			snapshot.assistant.agent_id = Some("agent".into());
			snapshot.assistant.topic_ids = vec!["release".into()];
			snapshot
				.work_items
				.iter_mut()
				.find(|work| work.id == "release")
				.unwrap()
				.parent_goal_id = None;
			snapshot.agents.push(decodex_protocol::work::Agent {
				id: "agent".into(),
				name: "My Assistant".into(),
				native_thread_id: None,
				assistant: true,
			});
			s.workspace.work_detail = None;
			s.composer.update(cx, |input, cx| input.set_content("General draft", cx));
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("assistant-topic-agent").is_some());
		assert!(visual.debug_bounds("assistant-topic-release").is_some());
		assert!(visual.debug_bounds("page-agent").is_none(), "Assistant must not appear twice");
		let topic = visual.debug_bounds("assistant-topic-release").unwrap();
		visual.simulate_click(topic.center(), Default::default());
		surface.update(visual, |s, cx| {
			assert_eq!(s.selected.as_deref(), Some("release"));
			s.composer.update(cx, |input, cx| input.set_content("Release draft", cx));
			s.open_page("agent", cx);
			assert_eq!(s.composer.read(cx).content(), "General draft");
			s.open_page("release", cx);
			assert_eq!(s.composer.read(cx).content(), "Release draft");
		});
	}
}
