//! Work discovery is separate from the list of open conversations.
use super::{
	AgentSurface, ComposerInput, Context, Entity, EntityId, InteractiveElement, IntoElement,
	ParentElement, Role, SharedString, StatefulInteractiveElement, Styled,
};
use crate::{
	shell::workspace_symbols::{self, Symbol},
	ui_theme::{HOVER_FILL, TEXT_MUTED},
};
use gpui::{AnyElement, AppContext as _, KeyDownEvent};

struct WorkRow {
	id: String,
	title: String,
	project: String,
	status: String,
	color: u32,
	native: Option<(String, String)>,
}

impl AgentSurface {
	pub(super) fn new_work_search(cx: &mut Context<Self>) -> Entity<ComposerInput> {
		let input =
			cx.new(|cx| ComposerInput::with_placeholder(46, "Search work…", "Search all work", cx));
		cx.observe(&input, |_, _, cx| cx.notify()).detach();
		input
	}

	pub(super) fn new_work_conversation(&mut self, cx: &mut Context<Self>) {
		if self.sending || self.uncertain {
			return;
		}
		self.workspace.browsing = false;
		if self.root_id().is_none() {
			cx.notify();
			return;
		}
		let id = format!("conversation-{}", super::unique_command());
		self.workspace.opening_work = Some(id.clone());
		self.execute(
			super::AgentActionDto::NewConversation {
				work_id: EntityId::new(id).expect("generated identity"),
			},
			None,
			cx,
		);
		if !self.sending {
			self.workspace.opening_work = None;
		}
	}

	pub(super) fn work_navigation(
		&self,
		id: &'static str,
		label: &'static str,
		symbol: Symbol,
		action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let action = std::rc::Rc::new(action);
		let keyboard = action.clone();
		gpui::div()
			.id(id)
			.role(Role::Button)
			.aria_label(label)
			.tab_index(0)
			.h(gpui::px(34.))
			.mb(gpui::px(4.))
			.w_full()
			.flex_none()
			.overflow_hidden()
			.flex()
			.items_center()
			.gap(gpui::px(6.))
			.rounded(gpui::px(7.))
			.cursor_pointer()
			.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
			.child(
				gpui::div()
					.w(gpui::px(40.))
					.h_full()
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.child(workspace_symbols::icon_sized(symbol, 15.)),
			)
			.child(
				gpui::div()
					.min_w(gpui::px(120.))
					.whitespace_nowrap()
					.text_color(gpui::rgb(TEXT_MUTED))
					.child(label),
			)
			.on_click(cx.listener(move |s, _, _, cx| action(s, cx)))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					keyboard(s, cx);
					cx.stop_propagation();
				}
			}))
			.into_any_element()
	}

	fn browsable_work(&self, query: &str, selected_project: Option<&str>) -> Vec<WorkRow> {
		let Some(snapshot) = &self.snapshot else {
			return Vec::new();
		};
		let mut rows = Vec::new();
		let mut seen = std::collections::BTreeSet::new();
		let mut work = snapshot.work_items.iter().collect::<Vec<_>>();
		work.sort_by_key(|w| std::cmp::Reverse(w.updated_at_micros));
		let project_for = |id: &str| {
			snapshot
				.workspaces
				.iter()
				.find(|p| super::workspace::within_project(snapshot, &p.agent_id, id))
		};
		let matches = |title: &str, project: Option<&decodex_protocol::AgentWorkspaceDto>| {
			!selected_project.is_some_and(|id| project.is_none_or(|p| p.agent_id != id))
				&& (query.is_empty()
					|| format!("{title} {}", project.map(|p| p.name.as_str()).unwrap_or(""))
						.to_lowercase()
						.contains(query))
		};
		for item in work {
			if let Some(thread) = &item.codex_thread_id {
				seen.insert(thread.clone());
			}
			let project = project_for(&item.id);
			let title = self.work_label(item);
			if !matches(&title, project) {
				continue;
			}
			let (status, color) = super::graph::state_in(snapshot, item);
			rows.push(WorkRow {
				id: item.id.clone(),
				title,
				project: project.map(|p| p.name.clone()).unwrap_or_default(),
				status: status.to_owned(),
				color,
				native: None,
			});
		}
		for (owner, agents) in &self.native_agents.lists {
			for agent in agents {
				if !seen.insert(agent.thread_id.clone()) {
					continue;
				}
				let project = project_for(owner);
				if !matches(&agent.title, project) {
					continue;
				}
				rows.push(WorkRow {
					id: format!("native:{owner}:{}", agent.thread_id),
					title: agent.title.clone(),
					project: project.map(|p| p.name.clone()).unwrap_or_default(),
					status: agent.status.clone(),
					color: TEXT_MUTED,
					native: Some((owner.clone(), agent.thread_id.clone())),
				});
			}
		}
		rows
	}

	pub(super) fn render_work_browser(&self, cx: &mut Context<Self>) -> AnyElement {
		let query = self.work_search.read(cx).content().trim().to_lowercase();
		let selected_project = self.workspace.project_filter.as_deref();
		let mut filters = gpui::div().flex().flex_wrap().gap_2();
		let mut projects = vec![(None, "All".to_owned())];
		if let Some(snapshot) = &self.snapshot {
			projects.extend(
				snapshot.workspaces.iter().map(|p| (Some(p.agent_id.clone()), p.name.clone())),
			);
		}
		for (id, name) in projects {
			let active = id.as_deref() == selected_project;
			filters = filters.child(
				gpui::div()
					.id(SharedString::from(format!(
						"work-filter-{}",
						id.as_deref().unwrap_or("all")
					)))
					.role(Role::Button)
					.aria_label(format!("Filter work: {name}"))
					.tab_index(0)
					.px_3()
					.py_1()
					.rounded(gpui::px(6.))
					.cursor_pointer()
					.bg(gpui::rgba(if active { 0xffffff14 } else { 0xffffff00 }))
					.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
					.child(name)
					.on_click(cx.listener(move |s, _, _, cx| {
						s.workspace.project_filter = id.clone();
						cx.notify();
					})),
			);
		}
		let mut list = gpui::div()
			.id("all-work-list")
			.flex_1()
			.min_h_0()
			.overflow_y_scroll()
			.flex()
			.flex_col();
		let mut count = 0;
		for WorkRow { id, title, project: project_name, status, color, native } in
			self.browsable_work(&query, selected_project)
		{
			let keyboard = id.clone();
			let native_keyboard = native.clone();
			count += 1;
			list = list.child(
				gpui::div()
					.id(SharedString::from(format!("browse-{id}")))
					.role(Role::Button)
					.aria_label(format!("Open {title}"))
					.tab_index(0)
					.w_full()
					.min_w_0()
					.h(gpui::px(48.))
					.flex_none()
					.px_3()
					.flex()
					.items_center()
					.gap_3()
					.rounded(gpui::px(7.))
					.cursor_pointer()
					.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
					.child(
						gpui::div()
							.flex_1()
							.min_w_0()
							.overflow_hidden()
							.text_ellipsis()
							.child(title),
					)
					.child(
						gpui::div()
							.w(gpui::px(120.))
							.overflow_hidden()
							.text_ellipsis()
							.text_size(gpui::px(11.))
							.text_color(gpui::rgb(TEXT_MUTED))
							.child(project_name.to_owned()),
					)
					.child(
						gpui::div()
							.w(gpui::px(110.))
							.text_size(gpui::px(11.))
							.text_color(gpui::rgb(color))
							.child(status),
					)
					.on_click(cx.listener(move |s, _, _, cx| {
						if let Some((owner, thread)) = &native {
							s.open_native_agent(owner, thread, cx);
						} else {
							s.open_page(&id, cx);
						}
					}))
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							if let Some((owner, thread)) = &native_keyboard {
								s.open_native_agent(owner, thread, cx);
							} else {
								s.open_page(&keyboard, cx);
							}
							cx.stop_propagation();
						}
					})),
			);
		}
		if count == 0 {
			list = list.child(
				gpui::div().p_4().text_color(gpui::rgb(TEXT_MUTED)).child("No matching work."),
			);
		}
		gpui::div()
			.id("all-work-browser")
			.debug_selector(|| "all-work-browser".into())
			.flex_1()
			.min_h_0()
			.min_w_0()
			.flex()
			.flex_col()
			.p_6()
			.gap_4()
			.child(gpui::div().h(gpui::px(38.)).flex_none().child(self.work_search.clone()))
			.child(filters)
			.child(list)
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn browser_finds_closed_work_and_opening_it_preserves_other_drafts(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.open_page("agent", cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
			s.workspace.pages.clear();
			s.workspace.browsing = true;
			let child = decodex_protocol::NativeAgentDto {
				thread_id: "child-thread".into(),
				parent_thread_id: "parent".into(),
				title: "Native review".into(),
				status: "completed".into(),
			};
			s.native_agents.lists.insert("agent".into(), vec![child.clone()]);
			s.native_agents.lists.insert("release".into(), vec![child]);
			assert_eq!(s.browsable_work("native review", None).len(), 1);
			let rows = s.browsable_work("", None);
			assert!(rows.iter().any(|r| r.id == "verify"));
			assert!(s.browsable_work("unlikely-no-matching-title", None).is_empty());
			let title = rows.iter().find(|r| r.id == "verify").unwrap().title.to_lowercase();
			assert!(s.browsable_work(&title, None).iter().any(|r| r.id == "verify"));
			s.open_page("verify", cx);
			assert!(!s.workspace.browsing);
			assert!(s.workspace.pages.iter().any(|id| id == "verify"));
			s.open_page("agent", cx);
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
		});
	}
}
