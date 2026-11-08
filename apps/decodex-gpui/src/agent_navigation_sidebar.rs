//! Search and navigate personal topics, Work and project Agents in one sidebar.
#[path = "agent_navigation_actions.rs"] mod actions;
use crate::{
	shell::{
		agent_surface::{AgentSurface, Context},
		workspace_symbols::{self},
	},
	ui_motion::SmoothControl as _,
	ui_theme::{self, NAVIGATION_DETAIL_SIZE, TEXT, TEXT_MUTED},
};
#[cfg(any(test, feature = "visual-capture"))] use decodex_protocol::work::WorkState;
use gpui::{
	AnyElement, AppContext as _, FontWeight, KeyDownEvent, Role, SharedString, Task,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};
use std::collections::HashSet;

#[path = "agent_command_palette.rs"] mod commands;
#[path = "agent_navigation_icons.rs"] mod icons;
#[path = "agent_navigation_model.rs"] pub(in crate::shell::agent_surface) mod model;
use gpui::{FocusHandle, Window};
use icons::{Glyph, Icon};
use model::{Item, Section, Sort};
use std::time::Instant;

pub(super) struct State {
	collapsed: HashSet<&'static str>,
	projects: HashSet<String>,
	preferences: model::Preferences,
	menu: Option<String>,
	confirm_delete: bool,
	confirm_started: Option<Instant>,
	mutation: Option<Task<()>>,
	feedback: Option<String>,
	hovered: Option<String>,
	options: Option<Section>,
	archives_open: bool,
	search: Option<gpui::Entity<crate::composer_input::ComposerInput>>,
	command_open: bool,
	command_index: usize,
	command_scroll: gpui::ScrollHandle,
	choose_project: bool,
	clock: Option<Task<()>>,
	reveal: Option<(String, String, Instant)>,
}
impl Default for State {
	fn default() -> Self {
		Self {
			collapsed: HashSet::from(["assistant"]),
			projects: HashSet::new(),
			preferences: model::Preferences::load(),
			menu: None,
			confirm_delete: false,
			confirm_started: None,
			mutation: None,
			feedback: None,
			hovered: None,
			options: None,
			archives_open: false,
			search: None,
			command_open: false,
			command_index: 0,
			command_scroll: gpui::ScrollHandle::new(),
			choose_project: false,
			clock: None,
			reveal: None,
		}
	}
}
struct ProjectBranch<'a> {
	id: &'a str,
	name: &'a str,
	directory: Option<&'a str>,
	items: &'a [Item],
	expanded: bool,
	auto_scroll: bool,
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

	fn navigation_items(&self, cx: &Context<Self>) -> Vec<Item> {
		let mut items = Vec::new();
		if let Some(snapshot) = &self.snapshot {
			let projects = self.project_conversation_pages();
			let mut ids: Vec<String> = snapshot
				.assistant
				.topic_ids
				.iter()
				.chain(snapshot.assistant.agent_id.iter())
				.cloned()
				.collect();
			ids.extend(projects.iter().map(|(id, _, _)| id.clone()));
			ids.sort();
			ids.dedup();
			for id in ids {
				if snapshot.archived_work_ids.contains(&id) {
					continue;
				}
				let work = snapshot.work_items.iter().find(|w| w.id == id);
				let assistant = self.is_assistant_conversation(&id);
				if assistant
					&& work.is_some_and(|w| w.title == "Assistant" && w.codex_thread_id.is_none())
					&& !self.assistant_home_has_draft(&id, cx)
					&& self.selected.as_ref() != Some(&id)
				{
					continue;
				}
				let project = snapshot.projects.iter().find(|p| p.agent_ids.contains(&id));
				let title = if assistant {
					work.map(|w| self.assistant_conversation_label(w))
						.unwrap_or_else(|| "New topic".into())
				} else {
					projects
						.iter()
						.find(|(other, _, _)| other == &id)
						.map(|(_, title, _)| title.clone())
						.unwrap_or_else(|| "New Agent".into())
				};
				items.push(Item {
					id: id.clone(),
					title,
					owner: if assistant {
						"Assistant".into()
					} else {
						project.map_or_else(|| "No project".into(), |p| p.name.clone())
					},
					project: project.map(|p| p.id.clone()),
					updated: work.map_or(0, |w| w.updated_at_micros),
					draft: work.is_some_and(|w| w.codex_thread_id.is_none()),
					assistant,
					archived: false,
					state: self.conversation_status(&id),
				});
			}
		}
		if let Some(id) = &self.workspace.new_conversation
			&& self.selected.as_ref() == Some(id)
			&& !items.iter().any(|i| &i.id == id)
		{
			let project = self.workspace.new_conversation_workspace.clone();
			let owner = self
				.snapshot
				.as_ref()
				.and_then(|s| s.projects.iter().find(|p| Some(&p.id) == project.as_ref()))
				.map_or_else(|| "No project".into(), |p| p.name.clone());
			items.push(Item {
				id: id.clone(),
				title: "New Agent".into(),
				owner,
				project,
				updated: 0,
				draft: true,
				assistant: false,
				archived: false,
				state: "Idle".into(),
			});
		}
		items
	}

	pub(super) fn render_navigation_sidebar(
		&self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let expanded = self.workspace_sidebar_expanded(window);
		let items = self.navigation_items(cx);
		let directory = self.navigation_directory(&items, expanded, window, cx);
		gpui::div()
			.id("agent-sidebar")
			.size_full()
			.flex()
			.flex_col()
			.px(gpui::px(6.))
			.py(gpui::px(4.))
			.overflow_hidden()
			.child(self.navigation_utility(
				"navigation-command-entry",
				"Search and commands",
				Glyph::Search,
				expanded,
				|s, w, cx| s.open_global_commands(w, cx),
				cx,
			))
			.child(navigation_divider())
			.child(
				gpui::div()
					.id("workspace-navigation-scroll")
					.track_scroll(&navigation_scroll("main", window, cx))
					.flex_1()
					.min_h_0()
					.overflow_y_scroll()
					.child(directory),
			)
			.children(self.workspace.navigation.feedback.as_ref().map(|message| {
				gpui::div()
					.px_1()
					.text_size(gpui::px(10.5))
					.text_color(ui_theme::rgb(ui_theme::ERROR))
					.child(message.clone())
			}))
			.child(navigation_divider())
			.child(self.navigation_utility(
				"archive-browser",
				"Archived",
				Glyph::Archive,
				expanded,
				|s, _, cx| {
					s.workspace.navigation.archives_open = !s.workspace.navigation.archives_open;
					s.workspace.sidebar_visible = true;
					if s.workspace.navigation.archives_open {
						s.load_navigation_archives(cx);
					}
					cx.notify();
				},
				cx,
			))
			.into_any_element()
	}

	fn navigation_directory(
		&self,
		items: &[Item],
		expanded: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> gpui::Div {
		let mut directory = gpui::div().flex().flex_col();
		if self.workspace.navigation.archives_open && expanded {
			directory = directory.child(self.navigation_utility(
				"archive-back",
				"Archived",
				Glyph::Back,
				expanded,
				|s, _, cx| {
					s.workspace.navigation.archives_open = false;
					cx.notify();
				},
				cx,
			));
			for item in self.navigation_archived_items() {
				directory = directory
					.child(self.navigation_item(&item, "archive", expanded, false, window, cx));
			}
			if let Some(message) = self.navigation_archive_status() {
				directory = directory.child(
					gpui::div()
						.px_2()
						.py_1()
						.text_size(gpui::px(ui_theme::NAVIGATION_SECTION_SIZE))
						.text_color(ui_theme::rgb(TEXT_MUTED))
						.child(message),
				);
			}
		} else {
			for section in [Section::Pin, Section::Recent, Section::Assistant, Section::Projects] {
				let mut content = gpui::div().flex().flex_col();
				let pref = self.workspace.navigation.preferences.section(section);
				let open = !self.workspace.navigation.collapsed.contains(section.key());
				if section == Section::Projects {
					content = self.navigation_projects(items, pref, expanded, window, cx);
				} else {
					let mut selected: Vec<_> = items
						.iter()
						.filter(|item| match section {
							Section::Pin =>
								self.workspace.navigation.preferences.pinned.contains(&item.id),
							Section::Recent =>
								!self.workspace.navigation.preferences.pinned.contains(&item.id),
							Section::Assistant => item.assistant,
							Section::Projects => false,
						})
						.cloned()
						.collect();
					model::sort(&mut selected, pref.sort);
					for item in selected {
						content = content.child(self.navigation_item(
							&item,
							section.key(),
							expanded,
							pref.auto_scroll,
							window,
							cx,
						));
					}
				}
				let content = if section == Section::Projects {
					content.into_any_element()
				} else {
					gpui::div()
						.id(SharedString::from(format!("navigation-list-{}", section.key())))
						.track_scroll(&navigation_scroll(section.key(), window, cx))
						.max_h(gpui::px(
							ui_theme::CONTROL_SIZE * if section == Section::Pin { 4. } else { 7. },
						))
						.overflow_y_scroll()
						.child(content)
						.into_any_element()
				};
				directory = directory
					.child(self.navigation_heading(section, expanded, open, window, cx))
					.child(crate::ui_motion::disclosure(
						SharedString::from(format!("navigation-disclosure-{}", section.key())),
						open && (section != Section::Projects || expanded),
						content,
					));
			}
		}
		directory
	}

	fn navigation_projects(
		&self,
		items: &[Item],
		pref: model::Preference,
		expanded: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> gpui::Div {
		let mut content = gpui::div().flex().flex_col();
		let mut projects = self.snapshot.as_ref().map_or_else(Vec::new, |s| s.projects.clone());
		projects.sort_by(|a, b| {
			if pref.sort == Sort::Alphabetical {
				a.name.to_lowercase().cmp(&b.name.to_lowercase())
			} else {
				let updated = |id: &str| {
					items
						.iter()
						.filter(|i| i.project.as_deref() == Some(id))
						.map(|i| i.updated)
						.max()
						.unwrap_or(0)
				};
				updated(&b.id)
					.cmp(&updated(&a.id))
					.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
			}
		});
		for project in projects {
			let mut children: Vec<_> =
				items.iter().filter(|i| i.project.as_ref() == Some(&project.id)).cloned().collect();
			model::sort(&mut children, pref.sort);
			content = content.child(self.navigation_project(
				ProjectBranch {
					id: &project.id,
					name: &project.name,
					directory: Some(&project.directory),
					items: &children,
					expanded,
					auto_scroll: pref.auto_scroll,
				},
				window,
				cx,
			));
		}
		let unassigned: Vec<_> =
			items.iter().filter(|i| !i.assistant && i.project.is_none()).cloned().collect();
		if !unassigned.is_empty() {
			content = content.child(self.navigation_project(
				ProjectBranch {
					id: "",
					name: "No project",
					directory: None,
					items: &unassigned,
					expanded,
					auto_scroll: pref.auto_scroll,
				},
				window,
				cx,
			));
		}
		content
	}

	fn navigation_utility(
		&self,
		id: &'static str,
		label: &'static str,
		glyph: Glyph,
		expanded: bool,
		action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let action = std::rc::Rc::new(action);
		let keyboard = action.clone();
		gpui::div()
			.id(id)
			.debug_selector(move || id.into())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(if id == "navigation-command-entry" {
				"Search and commands (⌘K)"
			} else {
				label
			})
			.h(gpui::px(ui_theme::CONTROL_SIZE))
			.w_full()
			.min_w(gpui::px(ui_theme::CONTROL_SIZE))
			.flex_none()
			.overflow_hidden()
			.flex()
			.items_center()
			.rounded(gpui::px(5.))
			.cursor_pointer()
			.hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.tooltip(move |_, cx| cx.new(|_| crate::shell::ControlTooltip(label)).into())
			.child(
				gpui::div()
					.size(gpui::px(ui_theme::CONTROL_SIZE))
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.child(Icon { glyph, size: ui_theme::NAVIGATION_ICON_SIZE }),
			)
			.when(expanded, |row| {
				row.child(
					gpui::div()
						.flex_1()
						.whitespace_nowrap()
						.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
						.child(label),
				)
				.when(id == "navigation-command-entry", |row| {
					row.child(
						gpui::div()
							.pr_1()
							.text_size(gpui::px(10.))
							.text_color(ui_theme::rgb(TEXT_MUTED))
							.child("⌘K"),
					)
				})
			})
			.on_click(cx.listener(move |s, _, w, cx| action(s, w, cx)))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, w, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					keyboard(s, w, cx);
					cx.stop_propagation();
				}
			}))
			.smooth()
			.into_any_element()
	}

	fn navigation_heading(
		&self,
		section: Section,
		expanded: bool,
		open: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let key = format!("heading-{}", section.key());
		let focus = navigation_focus(&key, window, cx);
		let hovered = self.workspace.navigation.hovered.as_ref() == Some(&key);
		let shown = expanded
			&& (hovered
				|| (window.last_input_was_keyboard() && focus.contains_focused(window, cx)));
		let opacity = crate::ui_motion::value(
			SharedString::from(format!("{key}-actions")),
			if shown { 1. } else { 0. },
			window,
			cx,
		);
		let glyph = match section {
			Section::Pin => Glyph::Pin,
			Section::Recent => Glyph::Recent,
			Section::Assistant => Glyph::Assistant,
			Section::Projects => Glyph::Folder,
		};
		let enter = key.clone();
		let mut row =
			gpui::div()
				.id(SharedString::from(format!("navigation-section-{}", section.key())))
				.debug_selector(move || format!("navigation-section-{}", section.key()))
				.track_focus(&focus)
				.role(Role::Button)
				.tab_index(0)
				.aria_label(section.label())
				.aria_expanded(open)
				.relative()
				.h(gpui::px(ui_theme::CONTROL_SIZE))
				.flex_none()
				.flex()
				.items_center()
				.rounded(gpui::px(5.))
				.overflow_hidden()
				.cursor_pointer()
				.hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
				.on_hover(cx.listener(move |s, hovered: &bool, _, cx| {
					s.navigation_hover(&enter, *hovered, cx)
				}))
				.on_click(cx.listener(move |s, _, _, cx| s.toggle_navigation_section(section, cx)))
				.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
					if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
						s.toggle_navigation_section(section, cx);
						cx.stop_propagation();
					}
				}))
				.child(
					gpui::div()
						.size(gpui::px(ui_theme::CONTROL_SIZE))
						.flex_none()
						.flex()
						.items_center()
						.justify_center()
						.text_color(ui_theme::rgb(TEXT_MUTED))
						.child(Icon { glyph, size: ui_theme::NAVIGATION_ICON_SIZE }),
				);
		if expanded {
			row = row.child(
				gpui::div()
					.flex_1()
					.min_w_0()
					.text_size(gpui::px(ui_theme::NAVIGATION_SECTION_SIZE))
					.font_weight(FontWeight::MEDIUM)
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.child(section.label()),
			);
			row = row.child(self.navigation_options(section, opacity, window, cx));
			if matches!(section, Section::Assistant | Section::Projects) {
				row = row.child(self.navigation_create(
					if section == Section::Assistant {
						"New Assistant topic"
					} else {
						"New project"
					},
					format!("navigation-create-{}", section.key()),
					opacity,
					move |s, cx| {
						if section == Section::Assistant {
							s.workspace.navigation.collapsed.remove(section.key());
							s.new_assistant_conversation(cx);
						} else {
							s.add_navigation_project(cx);
						}
					},
					cx,
				));
			}
			row = row.child(
				gpui::div()
					.w(gpui::px(20.))
					.h_full()
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.child(workspace_symbols::disclosure_chevron(
						SharedString::from(format!("navigation-chevron-{}", section.key())),
						open,
					)),
			);
		}
		row.into_any_element()
	}

	fn toggle_navigation_section(&mut self, section: Section, cx: &mut Context<Self>) {
		if section == Section::Projects
			&& !(self.workspace.sidebar_visible || self.workspace.sidebar_peek)
		{
			self.workspace.sidebar_visible = true;
			self.workspace.navigation.collapsed.remove(section.key());
		} else if !self.workspace.navigation.collapsed.remove(section.key()) {
			self.workspace.navigation.collapsed.insert(section.key());
		}
		cx.notify();
	}

	fn navigation_create(
		&self,
		label: &str,
		id: String,
		opacity: f32,
		action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let label = label.to_owned();
		let tip = label.clone();
		let action = std::rc::Rc::new(action);
		let key = action.clone();
		let debug = id.clone();
		gpui::div()
			.id(SharedString::from(id))
			.debug_selector(move || debug.clone())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label)
			.size(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
			.flex_none()
			.opacity(opacity)
			.focus_visible(|s| s.opacity(1.))
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(4.))
			.cursor_pointer()
			.text_color(ui_theme::rgb(TEXT_MUTED))
			.hover(|s| {
				s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)).text_color(ui_theme::rgb(ui_theme::BLUE))
			})
			.tooltip(move |_, cx| cx.new(|_| crate::shell::ControlTooltip(tip.clone())).into())
			.child(Icon { glyph: Glyph::Plus, size: ui_theme::NAVIGATION_DISCLOSURE_SIZE })
			.on_click(cx.listener(move |s, _, _, cx| {
				cx.stop_propagation();
				action(s, cx);
			}))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					key(s, cx);
					cx.stop_propagation();
				}
			}))
			.into_any_element()
	}

	fn navigation_project_heading(
		&self,
		branch: &ProjectBranch<'_>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let ProjectBranch { id, name, directory, expanded, auto_scroll, .. } = *branch;
		let open = self.workspace.navigation.projects.contains(id);
		let key = format!("project-{id}");
		let focus = navigation_focus(&key, window, cx);
		let active = expanded
			&& (self.workspace.navigation.hovered.as_ref() == Some(&key)
				|| (window.last_input_was_keyboard() && focus.contains_focused(window, cx)));
		let opacity = crate::ui_motion::value(
			SharedString::from(format!("{key}-actions")),
			if active { 1. } else { 0. },
			window,
			cx,
		);
		let anchor = self.navigation_anchor(&key, id, "project-header", window, cx);
		let project = id.to_owned();
		let keyboard = project.clone();
		let draft = project.clone();
		let debug = project.clone();
		let enter = key.clone();
		let folder = directory.map(str::to_owned);
		let header = gpui::div()
			.id(SharedString::from(format!("project-header-{id}")))
			.when_some(folder, |row, folder| {
				row.tooltip(move |_, cx| {
					cx.new(|_| crate::shell::ControlTooltip(format!("Folder: {folder}"))).into()
				})
			})
			.anchor_scroll(Some(anchor))
			.debug_selector(move || format!("project-header-{debug}"))
			.track_focus(&focus)
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!("{name} project"))
			.aria_expanded(open)
			.h(gpui::px(ui_theme::CONTROL_SIZE))
			.flex_none()
			.relative()
			.flex()
			.items_center()
			.pl(gpui::px(9.))
			.rounded(gpui::px(5.))
			.bg(ui_theme::rgba(if open { ui_theme::HOVER_FILL } else { 0xffffff00 }))
			.cursor_pointer()
			.on_hover(
				cx.listener(move |s, hovered: &bool, _, cx| {
					s.navigation_hover(&enter, *hovered, cx)
				}),
			)
			.on_click(cx.listener(move |s, _, _, cx| {
				if !s.workspace.navigation.projects.remove(&project) {
					s.workspace.navigation.projects.insert(project.clone());
				}
				cx.notify();
			}))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					if !s.workspace.navigation.projects.remove(&keyboard) {
						s.workspace.navigation.projects.insert(keyboard.clone());
					}
					cx.stop_propagation();
					cx.notify();
				}
			}))
			.when(open, |h| {
				h.child(
					gpui::div()
						.absolute()
						.left_0()
						.top(gpui::px(8.))
						.w(gpui::px(2.))
						.h(gpui::px(12.))
						.rounded(gpui::px(1.))
						.bg(ui_theme::rgb(ui_theme::BLUE)),
				)
			})
			.child(
				gpui::div()
					.flex_1()
					.min_w_0()
					.overflow_hidden()
					.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
					.font_weight(FontWeight::MEDIUM)
					.text_color(ui_theme::rgb(TEXT))
					.child(crate::ui_motion::HoverOverflowLabel {
						id: SharedString::from(format!("project-title-{id}")).into(),
						text: name.to_owned().into(),
						active: expanded && (auto_scroll || active),
					}),
			)
			.child(self.navigation_create(
				&format!("New Agent in {name}"),
				format!("navigation-new-agent-{id}"),
				opacity,
				move |s, cx| s.new_project_agent((!draft.is_empty()).then_some(draft.as_str()), cx),
				cx,
			))
			.child(
				gpui::div()
					.w(gpui::px(20.))
					.h_full()
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.child(workspace_symbols::disclosure_chevron(
						SharedString::from(format!("project-chevron-{id}")),
						open,
					)),
			);
		header.into_any_element()
	}

	fn navigation_project(
		&self,
		branch: ProjectBranch<'_>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let header = self.navigation_project_heading(&branch, window, cx);
		let ProjectBranch { id, items, expanded, auto_scroll, .. } = branch;
		let open = self.workspace.navigation.projects.contains(id);
		let mut children = gpui::div().relative().flex().flex_col();
		for item in items {
			children = children.child(self.navigation_item(
				item,
				&format!("project-{id}"),
				expanded,
				auto_scroll,
				window,
				cx,
			));
		}
		if !items.is_empty() {
			children = children.child(
				gpui::div()
					.absolute()
					.left(gpui::px(9.))
					.right(gpui::px(9.))
					.bottom_0()
					.h(gpui::px(1.))
					.bg(ui_theme::rgba(ui_theme::PANEL_DIVIDER)),
			);
		}
		gpui::div()
			.flex()
			.flex_col()
			.child(header)
			.child(crate::ui_motion::disclosure(
				SharedString::from(format!("project-children-{id}")),
				open,
				children,
			))
			.into_any_element()
	}

	fn navigation_item(
		&self,
		item: &Item,
		section: &str,
		expanded: bool,
		auto_scroll: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let key = format!("{section}-{}", item.id);
		let group = if section.starts_with("project-") { "projects" } else { section };
		let anchor = self.navigation_anchor(&key, &item.id, group, window, cx);
		let debug = if section == "assistant" {
			format!("assistant-topic-{}", item.id)
		} else {
			format!("navigation-{key}")
		};
		let focus = navigation_focus(&key, window, cx);
		let hovered = self.workspace.navigation.hovered.as_ref() == Some(&key);
		let keyboard = window.last_input_was_keyboard() && focus.contains_focused(window, cx);
		let confirming = self.workspace.navigation.menu.as_deref() == Some(&key)
			&& self.workspace.navigation.confirm_delete;
		let manageable = item.archived
			|| self.snapshot.as_ref().is_some_and(|s| s.work_items.iter().any(|w| w.id == item.id));
		let shown = expanded && manageable && (hovered || keyboard || confirming);
		let actions = crate::ui_motion::value(
			SharedString::from(format!("row-actions-{key}")),
			if shown { 1. } else { 0. },
			window,
			cx,
		);
		let selected = !self.workspace.browsing
			&& self.selected.as_deref() == Some(&item.id)
			&& self.workspace.work_detail.is_none();
		let target = item.clone();
		let key_target = target.clone();
		let outside_key = key.clone();
		let enter = key.clone();
		let timestamp = Self::navigation_timestamp(item.updated);
		let mut row =
			gpui::div()
				.id(SharedString::from(format!("navigation-row-{key}")))
				.anchor_scroll(Some(anchor))
				.debug_selector(move || debug.clone())
				.track_focus(&focus)
				.role(Role::Button)
				.tab_index(0)
				.aria_label(format!("{} ({}) {}", item.title, item.owner, timestamp))
				.aria_selected(selected)
				.h(gpui::px(ui_theme::CONTROL_SIZE))
				.min_w(gpui::px(ui_theme::CONTROL_SIZE))
				.w_full()
				.flex_none()
				.relative()
				.flex()
				.items_center()
				.overflow_hidden()
				.rounded(gpui::px(5.))
				.cursor_pointer()
				.bg(ui_theme::rgba(if selected { ui_theme::SELECTED_FILL } else { 0xffffff00 }))
				.hover(move |s| {
					s.bg(ui_theme::rgba(if selected {
						ui_theme::SELECTED_HOVER_FILL
					} else {
						ui_theme::HOVER_FILL
					}))
				})
				.on_hover(cx.listener(move |s, hovered: &bool, _, cx| {
					s.navigation_hover(&enter, *hovered, cx)
				}))
				.on_click(cx.listener(move |s, _, _, cx| s.activate_navigation_item(&target, cx)))
				.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
					if e.keystroke.key == "escape" {
						s.workspace.navigation.confirm_delete = false;
						s.workspace.navigation.menu = None;
						cx.notify();
						cx.stop_propagation();
					} else if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
						s.activate_navigation_item(&key_target, cx);
						cx.stop_propagation();
					}
				}))
				.when(confirming, |row| {
					row.on_mouse_down_out(cx.listener(move |s, _, _, cx| {
						if s.workspace.navigation.menu.as_ref() == Some(&outside_key) {
							s.workspace.navigation.menu = None;
							s.workspace.navigation.confirm_delete = false;
							cx.notify();
						}
					}))
				});
		row = navigation_item_tooltip(row, item, expanded);
		let base = if !expanded || item.draft { 0. } else { 44. };
		let inset = base + (76. - base) * actions;
		let label = navigation_item_label(
			item,
			section,
			&key,
			expanded && (auto_scroll || hovered || keyboard),
		);
		row = row.child(
			gpui::div()
				.flex_1()
				.min_w(gpui::px(ui_theme::CONTROL_SIZE))
				.h_full()
				.mr(gpui::px(inset))
				.overflow_hidden()
				.flex()
				.items_center()
				.child(label),
		);
		if expanded {
			if !item.draft {
				row = row.child(navigation_item_time(item, actions));
			}
			if manageable {
				row = row.child(self.navigation_row_actions(item, &key, actions, cx));
			}
		}
		row.into_any_element()
	}

	fn activate_navigation_item(&mut self, item: &Item, cx: &mut Context<Self>) {
		self.workspace.navigation.confirm_delete = false;
		self.workspace.navigation.menu = None;
		if item.archived {
			self.open_archived_conversation(&item.id, cx);
		} else if self.workspace.new_conversation.as_ref() == Some(&item.id) {
			self.new_project_agent(item.project.as_deref(), cx);
		} else {
			self.open_navigation_conversation(&item.id, cx);
		}
	}

	fn new_project_agent(&mut self, project: Option<&str>, cx: &mut Context<Self>) {
		if self.sending || self.uncertain {
			return;
		}
		let scope = self.profile.as_ref().map_or_else(|| "unbound".into(), |p| p.draft_scope_key());
		let saved = self.workspace.navigation.preferences.project_drafts.entry(scope).or_default();
		if let Some(id) = &self.workspace.new_conversation {
			saved.insert(
				self.workspace.new_conversation_workspace.clone().unwrap_or_default(),
				id.clone(),
			);
		}
		let draft = saved.get(project.unwrap_or_default()).cloned().filter(|id| {
			self.snapshot.as_ref().is_none_or(|s| !s.work_items.iter().any(|w| &w.id == id))
		});
		self.workspace.new_conversation = draft;
		self.workspace.workspace_filter = project.map(str::to_owned);
		self.workspace.new_conversation_workspace = project.map(str::to_owned);
		self.workspace.navigation.collapsed.remove("projects");
		self.workspace.navigation.projects.insert(project.unwrap_or_default().to_owned());
		self.workspace.work_detail = None;
		self.new_work_conversation(cx);
		if let Some(id) = self.workspace.new_conversation.clone() {
			self.prepare_navigation_reveal(&id, "projects");
		}
		if let Some(id) = &self.workspace.new_conversation {
			self.workspace
				.navigation
				.preferences
				.project_drafts
				.entry(
					self.profile.as_ref().map_or_else(|| "unbound".into(), |p| p.draft_scope_key()),
				)
				.or_default()
				.insert(project.unwrap_or_default().to_owned(), id.clone());
			self.workspace.navigation.preferences.save();
		}
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

fn navigation_focus(id: &str, window: &mut Window, cx: &mut gpui::App) -> FocusHandle {
	window
		.use_keyed_state(SharedString::from(format!("navigation-focus-{id}")), cx, |_, cx| {
			cx.focus_handle()
		})
		.read(cx)
		.clone()
}
fn navigation_divider() -> AnyElement {
	let mut line = ui_theme::rgb(TEXT_MUTED);
	line.a = 0.18;
	let mut clear = line;
	clear.a = 0.;
	gpui::div()
		.h(gpui::px(ui_theme::NAVIGATION_DIVIDER_HEIGHT))
		.w_full()
		.flex_none()
		.px(gpui::px(6.))
		.flex()
		.items_center()
		.child(gpui::div().h(gpui::px(1.)).flex_1().bg(gpui::linear_gradient(
			90.,
			gpui::linear_color_stop(clear, 0.),
			gpui::linear_color_stop(line, 1.),
		)))
		.child(gpui::div().h(gpui::px(1.)).flex_1().bg(gpui::linear_gradient(
			90.,
			gpui::linear_color_stop(line, 0.),
			gpui::linear_color_stop(clear, 1.),
		)))
		.into_any_element()
}

fn navigation_item_tooltip(
	row: gpui::Stateful<gpui::Div>,
	item: &Item,
	expanded: bool,
) -> gpui::Stateful<gpui::Div> {
	let bounds = std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::<gpui::Pixels>::default()));
	let measured = bounds.clone();
	let title = (!expanded).then(|| item.title.clone());
	let owner = item.owner.clone();
	let time = AgentSurface::navigation_timestamp(item.updated);
	row.tooltip(move |_, cx| {
		cx.new(|_| NavigationTip {
			title: title.clone(),
			owner: owner.clone(),
			time: time.clone(),
			anchor: bounds.clone(),
		})
		.into()
	})
	.child(
		gpui::canvas(move |bounds, _, _| measured.set(bounds), |_, _, _, _| {})
			.absolute()
			.inset_0(),
	)
}

struct NavigationTip {
	anchor: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
	title: Option<String>,
	owner: String,
	time: String,
}
impl gpui::Render for NavigationTip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
		let content = crate::ui_motion::tooltip_surface(gpui::div())
			.debug_selector(|| "navigation-metadata-tooltip".into())
			.flex()
			.flex_col()
			.gap(gpui::px(2.))
			.text_size(gpui::px(ui_theme::NAVIGATION_SECTION_SIZE))
			.children(self.title.clone())
			.child(self.owner.clone())
			.child(
				gpui::div()
					.text_size(gpui::px(10.))
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.child(self.time.clone()),
			);
		gpui::anchored()
			.position(
				self.anchor.get().top_right()
					+ gpui::point(gpui::px(ui_theme::POPOVER_GAP), gpui::px(0.)),
			)
			.child(content)
	}
}

impl AgentSurface {
	pub(super) fn forget_navigation_conversation(&mut self, id: &str) {
		self.workspace.navigation.preferences.pinned.remove(id);
		for drafts in self.workspace.navigation.preferences.project_drafts.values_mut() {
			drafts.retain(|_, value| value != id);
		}
		self.workspace.navigation.preferences.save();
	}

	pub(crate) fn dismiss_navigation_confirmation(&mut self, cx: &mut Context<Self>) {
		if self.workspace.navigation.confirm_delete {
			self.workspace.navigation.confirm_delete = false;
			self.workspace.navigation.menu = None;
			cx.notify();
		}
	}
}

impl AgentSurface {
	fn navigation_hover(&mut self, key: &str, hovered: bool, cx: &mut Context<Self>) {
		let before = self.workspace.navigation.hovered.clone();
		if hovered {
			self.workspace.navigation.hovered = Some(key.into());
		} else if before.as_deref() == Some(key) {
			self.workspace.navigation.hovered = None;
		}
		if before != self.workspace.navigation.hovered {
			self.presentations.sidebar.update(cx, |_, cx| cx.notify());
		}
	}
}

impl AgentSurface {
	pub(super) fn reveal_navigation_project(&mut self, id: &str, cx: &mut Context<Self>) {
		self.workspace.sidebar_visible = true;
		self.workspace.navigation.archives_open = false;
		self.workspace.navigation.collapsed.remove("projects");
		self.workspace.navigation.projects.insert(id.into());
		self.prepare_navigation_reveal(id, "project-header");
		cx.notify();
	}

	pub(super) fn navigation_project_feedback(
		&mut self,
		message: Option<String>,
		cx: &mut Context<Self>,
	) {
		self.workspace.navigation.feedback = message;
		cx.notify();
	}
}

fn navigation_scroll(group: &str, window: &mut Window, cx: &mut gpui::App) -> gpui::ScrollHandle {
	window
		.use_keyed_state(SharedString::from(format!("navigation-scroll-{group}")), cx, |_, _| {
			gpui::ScrollHandle::new()
		})
		.read(cx)
		.clone()
}
impl AgentSurface {
	fn prepare_navigation_reveal(&mut self, id: &str, group: &str) {
		self.workspace.navigation.reveal = Some((id.into(), group.into(), Instant::now()));
	}

	fn navigation_anchor(
		&self,
		key: &str,
		id: &str,
		group: &str,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> gpui::ScrollAnchor {
		let scroll = navigation_scroll(
			if ["pin", "recent", "assistant"].contains(&group) { group } else { "main" },
			window,
			cx,
		);
		let anchor = window
			.use_keyed_state(SharedString::from(format!("navigation-anchor-{key}")), cx, |_, _| {
				gpui::ScrollAnchor::for_handle(scroll)
			})
			.read(cx)
			.clone();
		if let Some((target, scope, started)) = &self.workspace.navigation.reveal
			&& target == id
			&& scope == group
		{
			let receipt = window.use_keyed_state(
				SharedString::from(format!("navigation-reveal-{key}")),
				cx,
				|_, _| None::<Instant>,
			);
			if receipt.read(cx).as_ref() != Some(started) {
				if started.elapsed() >= std::time::Duration::from_millis(200)
					|| crate::ui_motion::reduced()
				{
					if ["pin", "recent", "assistant"].contains(&group) {
						navigation_scroll("main", window, cx)
							.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
					}
					anchor.scroll_to(window, cx);
					receipt.update(cx, |value, _| *value = Some(*started));
				} else {
					crate::ui_motion::request_frame(window, cx);
				}
			}
		}
		anchor
	}

	pub(super) fn start_navigation_clock(&mut self, cx: &mut Context<Self>) {
		if self.workspace.navigation.clock.is_some() {
			return;
		}
		self.workspace.navigation.clock = Some(cx.spawn(async |owner, cx| {
			loop {
				cx.background_executor().timer(std::time::Duration::from_secs(60)).await;
				if owner
					.update(cx, |s, cx| s.presentations.sidebar.update(cx, |_, cx| cx.notify()))
					.is_err()
				{
					break;
				}
			}
		}));
	}
}

fn navigation_item_label(item: &Item, section: &str, key: &str, scroll: bool) -> AnyElement {
	if section.starts_with("project-") || section == "archive" {
		gpui::div()
			.pl(gpui::px(9.))
			.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
			.child(crate::ui_motion::HoverOverflowLabel {
				id: SharedString::from(format!("navigation-title-{key}")).into(),
				text: item.title.clone().into(),
				active: scroll,
			})
			.into_any_element()
	} else {
		crate::ui_motion::AgentRailStatus {
			compact: false,
			id: format!("navigation-{key}"),
			label: item.title.clone(),
			state: item.state.clone(),
			scroll,
		}
		.into_any_element()
	}
}

fn navigation_item_time(item: &Item, actions: f32) -> impl gpui::IntoElement {
	gpui::div()
		.absolute()
		.right(gpui::px(5.))
		.w(gpui::px(34.))
		.h_full()
		.flex()
		.items_center()
		.justify_end()
		.opacity(1. - actions)
		.text_size(gpui::px(10.))
		.text_color(ui_theme::rgb(TEXT_MUTED))
		.child(model::relative_time(item.updated, model::now()))
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
			s.workspace.navigation.collapsed.remove("assistant");
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
