//! Navigate personal topics and durable outcomes without duplicating collaboration.
use gpui::{
	AnyElement, Div, FontWeight, KeyDownEvent, Role,
	prelude::{
		InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

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

struct NavigationRow {
	id: String,
	title: String,
	detail: Option<String>,
	active: bool,
	symbol: Symbol,
}

impl AgentSurface {
	pub(super) fn project_conversation_pages(&self) -> Vec<(String, String, bool)> {
		self.conversation_pages()
			.into_iter()
			.filter(|(id, _, _)| {
				!self.is_assistant_conversation(id)
					&& !self.snapshot.as_ref().is_some_and(|snapshot| {
						self.workspace
							.attached
							.family_root(id, snapshot)
							.is_some_and(|root| root != id)
					})
			})
			.collect()
	}

	pub(super) fn open_navigation_conversation(&mut self, id: &str, cx: &mut Context<Self>) {
		self.workspace.work_detail = None;
		self.open_page(id, cx);
	}

	pub(super) fn navigation_work_context(&self) -> Option<AnyElement> {
		let id = self.workspace.work_detail.as_ref()?;
		let work =
			self.snapshot.as_ref()?.portfolio.works.iter().find(|work| {
				&work.id == id && Some(&work.owner_agent_id) == self.selected.as_ref()
			})?;
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
		let mut content = gpui::div()
			.id("workspace-navigation-scroll")
			.flex_1()
			.min_h_0()
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap(gpui::px(NAVIGATION_SECTION_GAP))
			.child(self.personal_navigation(cx))
			.child(self.outcome_navigation(cx));
		let project_chats =
			self.conversation_pages().iter().any(|(id, _, _)| !self.is_assistant_conversation(id));
		if project_chats {
			content = content.child(
				gpui::div()
					.flex()
					.flex_col()
					.gap_1()
					.child(section_label("Project chats"))
					.child(self.workspace_tabs(cx)),
			);
		}
		gpui::div()
			.id("agent-sidebar")
			.size_full()
			.flex()
			.flex_col()
			.gap_2()
			.px(gpui::px(6.))
			.py(gpui::px(10.))
			.child(content)
			.child(gpui::div().h(gpui::px(1.)).bg(ui_theme::rgba(ui_theme::PANEL_DIVIDER)))
			.child(self.work_navigation(
				"archive-browser",
				"Archived conversations",
				Symbol::Archive,
				|s, cx| s.open_archive_catalog(cx),
				cx,
			))
			.child(self.work_navigation(
				"new-conversation",
				"New project Agent",
				Symbol::Plus,
				|s, cx| s.new_work_conversation(cx),
				cx,
			))
			.child(
				gpui::div()
					.id("sidebar-settings")
					.role(Role::Button)
					.tab_index(0)
					.aria_label("Settings")
					.h(gpui::px(30.))
					.px_2()
					.flex()
					.items_center()
					.gap_2()
					.rounded(gpui::px(NAVIGATION_ROW_RADIUS))
					.text_size(gpui::px(NAVIGATION_TITLE_SIZE))
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.cursor_pointer()
					.hover(|el| el.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
					.on_click(|_, window, cx| {
						window.dispatch_action(Box::new(crate::shell::ActivateSettings), cx)
					})
					.on_key_down(|e: &KeyDownEvent, window, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							window.dispatch_action(Box::new(crate::shell::ActivateSettings), cx);
							cx.stop_propagation();
						}
					})
					.child(workspace_symbols::icon_sized(Symbol::Settings, 15.))
					.child("Settings")
					.smooth(),
			)
			.into_any_element()
	}

	fn personal_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut group = gpui::div()
			.flex()
			.flex_col()
			.gap_1()
			.child(self.work_navigation(
				"personal-assistant",
				"My Assistant",
				Symbol::TeamMembers,
				|s, cx| s.open_assistant(cx),
				cx,
			))
			.child(self.work_navigation(
				"new-assistant-conversation",
				"New conversation",
				Symbol::Plus,
				|s, cx| s.new_assistant_conversation(cx),
				cx,
			));
		if let Some(snapshot) = &self.snapshot {
			for id in snapshot.assistant.topic_ids.iter().chain(snapshot.assistant.agent_id.iter())
			{
				if snapshot.archived_work_ids.contains(id) {
					continue;
				}
				let Some(work) = snapshot.work_items.iter().find(|w| &w.id == id) else { continue };
				let title = self.assistant_conversation_label(work);
				let target = id.clone();
				group = group.child(self.navigation_row(
					NavigationRow {
						id: format!("assistant-topic-{id}"),
						title,
						detail: None,
						active: !self.workspace.browsing
							&& self.selected.as_ref() == Some(id)
							&& self.workspace.work_detail.is_none(),
						symbol: Symbol::Document,
					},
					move |s, cx| {
						s.workspace.work_detail = None;
						s.open_page(&target, cx);
					},
					cx,
				));
			}
		}
		group.into_any_element()
	}

	fn outcome_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut group = gpui::div()
			.flex()
			.flex_col()
			.gap_1()
			.pt_2()
			.border_t_1()
			.border_color(ui_theme::rgba(ui_theme::PANEL_DIVIDER))
			.child(section_label("Work"))
			.child(self.work_navigation(
				"new-personal-work",
				"Discuss new work",
				Symbol::Plus,
				|s, cx| s.new_assistant_conversation(cx),
				cx,
			));
		if let Some(snapshot) = &self.snapshot {
			for work in snapshot
				.portfolio
				.works
				.iter()
				.filter(|w| !matches!(w.state, WorkState::Completed | WorkState::Cancelled))
				.take(8)
			{
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
					},
					move |s, cx| {
						s.open_page(&owner, cx);
						s.workspace.work_detail = Some(id.clone());
						s.load_work_detail(id.clone(), cx);
					},
					cx,
				));
			}
		}
		group = group
			.child(self.work_navigation(
				"project-work",
				"View all work",
				Symbol::AllWork,
				|s, cx| {
					s.cancel_dictation(cx);
					s.workspace.browsing = true;
					s.workspace.portfolio_mode = true;
					s.workspace.guardian_open = false;
					s.workspace.archives.open = false;
					s.workspace.work_detail = None;
					cx.notify();
				},
				cx,
			))
			.child(self.work_navigation(
				"all-work",
				"Find project chats",
				Symbol::ActivitySearch,
				|s, cx| {
					s.cancel_dictation(cx);
					s.workspace.browsing = true;
					s.workspace.portfolio_mode = false;
					s.workspace.guardian_open = false;
					s.workspace.archives.open = false;
					s.workspace.work_detail = None;
					cx.notify();
				},
				cx,
			));
		group.into_any_element()
	}

	fn navigation_row(
		&self,
		row: NavigationRow,
		action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let action = std::rc::Rc::new(action);
		let keyboard = action.clone();
		let debug = row.id.clone();
		gpui::div()
			.id(gpui::SharedString::from(row.id))
			.debug_selector(move || debug.clone())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(row.title.clone())
			.aria_selected(row.active)
			.w_full()
			.min_w_0()
			.flex_none()
			.px_0()
			.py(gpui::px(5.))
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
					.flex_none()
					.w(gpui::px(28.))
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
					.flex_col()
					.gap(gpui::px(3.))
					.child(
						gpui::div()
							.truncate()
							.line_height(gpui::px(18.))
							.text_size(gpui::px(NAVIGATION_TITLE_SIZE))
							.text_color(ui_theme::rgb(TEXT))
							.child(row.title),
					)
					.children(row.detail.map(|detail| {
						gpui::div()
							.truncate()
							.text_size(gpui::px(NAVIGATION_DETAIL_SIZE))
							.text_color(ui_theme::rgb(TEXT_MUTED))
							.child(detail)
					})),
			)
			.on_click(cx.listener(move |s, _, _, cx| action(s, cx)))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					keyboard(s, cx);
					cx.stop_propagation();
				}
			}))
			.smooth()
			.into_any_element()
	}
}

fn section_label(title: &'static str) -> Div {
	gpui::div()
		.h(gpui::px(24.))
		.flex_none()
		.whitespace_nowrap()
		.overflow_hidden()
		.px_2()
		.py_1()
		.text_size(gpui::px(NAVIGATION_DETAIL_SIZE))
		.font_weight(FontWeight::MEDIUM)
		.text_color(ui_theme::rgb(TEXT_MUTED))
		.child(title)
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
