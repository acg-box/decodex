//! Navigate the personal Assistant, independent chats and projects in one sidebar.
#[path = "agent_navigation_actions.rs"] mod actions;
#[path = "agent_navigation_projects.rs"] mod project_actions;
use crate::{
	shell::{
		agent_surface::{AgentSurface, Context},
		workspace_symbols::{self},
	},
	ui_motion::{AgentStatusStyle, HoverHint as _, PanelReveal, SmoothControl as _},
	ui_theme::{self, NAVIGATION_DETAIL_SIZE, TEXT_MUTED},
};
#[cfg(any(test, feature = "visual-capture"))] use decodex_protocol::work::WorkState;
use gpui::{
	AnyElement, FontWeight, KeyDownEvent, Role, SharedString, Task,
	prelude::{
		FluentBuilder as _, InteractiveElement as _, IntoElement as _, ParentElement as _,
		StatefulInteractiveElement as _, Styled as _,
	},
};

#[path = "agent_command_palette.rs"] mod commands;
#[path = "agent_navigation_icons.rs"] mod icons;
#[path = "agent_navigation_model.rs"] pub(in crate::shell::agent_surface) mod model;
#[path = "agent_navigation_style.rs"] mod style;
use gpui::{FocusHandle, Window};
use icons::{Glyph, Icon};
use model::{Item, Section, Sort};
use std::{collections::HashSet, time::Instant};

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
	search: Option<gpui::Entity<crate::composer_input::ComposerInput>>,
	command_open: bool,
	command_index: usize,
	command_session: usize,
	command_scroll: gpui::ScrollHandle,
	choose_project: bool,
	clock: Option<Task<()>>,
	reveal: Option<(String, String, Instant)>,
}
impl Default for State {
	fn default() -> Self {
		Self {
			collapsed: HashSet::from([Section::Pin.key()]),
			projects: HashSet::new(),
			preferences: model::Preferences::load(),
			menu: None,
			confirm_delete: false,
			confirm_started: None,
			mutation: None,
			feedback: None,
			hovered: None,
			options: None,
			search: None,
			command_open: false,
			command_index: 0,
			command_session: 0,
			command_scroll: gpui::ScrollHandle::new(),
			choose_project: false,
			clock: None,
			reveal: None,
		}
	}
}
impl AgentSurface {
	pub(super) fn project_conversation_pages(&self) -> Vec<(String, String, bool)> {
		let mut pages = self.conversation_pages();
		if let Some(snapshot) = &self.snapshot {
			let mut seen: HashSet<String> = pages.iter().map(|(id, _, _)| id.clone()).collect();
			for work in
				snapshot.work_items.iter().filter(|work| Self::can_register_relationship(work))
			{
				if !snapshot.archived_work_ids.contains(&work.id) && seen.insert(work.id.clone()) {
					pages.push((work.id.clone(), self.conversation_label(work), false));
				}
			}
			for agent in snapshot.agents.iter() {
				if !snapshot.archived_work_ids.contains(&agent.id) && seen.insert(agent.id.clone())
				{
					let label = self.agent_conversation_label(agent);
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

	pub(super) fn reveal_navigation_conversation(&mut self, id: &str, cx: &Context<Self>) {
		if self.is_assistant_conversation(id) {
			self.prepare_navigation_reveal(Section::Assistant.key(), Section::Assistant.key());
			return;
		}
		let Some(item) = self.navigation_items(cx).into_iter().find(|item| item.id == id) else {
			return;
		};
		let section = if self.workspace.navigation.preferences.pinned.contains(id)
			&& !self.workspace.navigation.collapsed.contains(Section::Pin.key())
		{
			Section::Pin
		} else {
			item.section()
		};
		self.workspace.navigation.collapsed.remove(section.key());
		if section == Section::Projects
			&& let Some(project) = item.project
		{
			self.workspace.navigation.projects.insert(project);
		}
		self.prepare_navigation_reveal(id, section.key());
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
			for (id, title, _) in self.project_conversation_pages() {
				let work = snapshot.work_items.iter().find(|w| w.id == id);
				let project = snapshot.projects.iter().find(|p| p.agent_ids.contains(&id));
				items.push(Item {
					id: id.clone(),
					title,
					location: Item::location_label(false, project),
					project: project.map(|p| p.id.clone()),
					updated: work.map_or(0, |w| w.updated_at_micros),
					draft: work.is_some_and(|w| w.codex_thread_id.is_none()),
					assistant: false,
					archived: false,
					state: self.conversation_status(&id),
				});
			}
		}
		if let Some(id) = &self.workspace.new_conversation
			&& self.selected.as_ref() == Some(id)
			&& !items.iter().any(|i| &i.id == id)
			&& (!self.composer.read(cx).content().is_empty()
				|| !self.attachments.is_empty()
				|| !self.task_references.is_empty())
		{
			let project = self.workspace.new_conversation_workspace.clone();
			let directory_project = self
				.snapshot
				.as_ref()
				.and_then(|s| s.projects.iter().find(|p| Some(&p.id) == project.as_ref()));
			items.push(Item {
				id: id.clone(),
				title: "New chat".into(),
				location: Item::location_label(false, directory_project),
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
		let rail = ui_theme::CONVERSATION_TAB_SIZE + 2. * ui_theme::SIDEBAR_INSET;
		let width = self.workspace_sidebar_display_width(window);
		let content_width = self.workspace_sidebar_expanded_width(window);
		let reveal =
			PanelReveal::new(self.workspace_sidebar_expanded(window), width, rail, content_width);
		crate::frame_trace::sidebar_geometry(width, content_width);
		self.render_navigation_sidebar_content(reveal, window, cx)
	}

	pub(super) fn render_navigation_sidebar_content(
		&self,
		reveal: PanelReveal,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let items = self.navigation_items(cx);
		let directory = self.navigation_directory(&items, &reveal, window, cx);
		// Retain one complete content layout while the outer panel clips its width.
		// A target change must not remove labels or squeeze trailing controls mid-motion.
		let mut content = gpui::div()
			.debug_selector(|| "navigation-content-canvas".into())
			.w(gpui::px(reveal.content_width))
			.h_full()
			.flex_none()
			.flex()
			.flex_col()
			.px(gpui::px(ui_theme::SIDEBAR_INSET))
			.py(gpui::px(4.))
			.overflow_hidden()
			.child(self.navigation_utility(
				"navigation-command-entry",
				"Search and commands",
				Glyph::Search,
				&reveal,
				|s, w, cx| s.open_global_commands(w, cx),
				cx,
			))
			.child(navigation_divider());
		content = content
			.child(
				gpui::div()
					.id("workspace-navigation-scroll")
					.track_scroll(&navigation_scroll("directory", window, cx))
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
				"codex-conversations",
				"Codex conversations",
				Glyph::Conversations,
				&reveal,
				|s, _, cx| s.open_thread_registration(None, cx),
				cx,
			))
			.child(self.navigation_utility(
				"archive-browser",
				"Archived",
				Glyph::Archive,
				&reveal,
				|s, _, cx| s.open_archive_catalog(cx),
				cx,
			));
		let viewport = reveal.viewport.anchor();
		ui_theme::workspace_panel_content()
			.id("agent-sidebar")
			.size_full()
			.relative()
			.overflow_hidden()
			.child(viewport)
			.child(content)
			.into_any_element()
	}

	fn navigation_directory(
		&self,
		items: &[Item],
		reveal: &PanelReveal,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> gpui::Div {
		let mut directory = gpui::div().flex().flex_col();
		for section in Section::ALL {
			if section == Section::Projects {
				directory = directory.child(navigation_divider());
			}
			let open = !self.workspace.navigation.collapsed.contains(section.key());
			directory = directory.child(self.navigation_heading(section, open, reveal, window, cx));
			if section == Section::Assistant {
				continue;
			}
			let pref = self.workspace.navigation.preferences.section(section);
			let mut content = gpui::div().flex().flex_col();
			if section == Section::Projects {
				content = self.navigation_projects(items, reveal, window, cx);
			} else {
				let mut selected: Vec<_> = items
					.iter()
					.filter(|item| match section {
						Section::Pin =>
							self.workspace.navigation.preferences.pinned.contains(&item.id),
						_ => item.section() == section,
					})
					.cloned()
					.collect();
				model::sort(&mut selected, pref.sort);
				if selected.is_empty() {
					content = content.child(navigation_empty(
						reveal,
						if section == Section::Pin {
							"No pinned conversations."
						} else {
							"No conversations yet."
						},
					));
				}
				for item in selected {
					content = content.child(self.navigation_item(
						&item,
						section.key(),
						reveal,
						pref.auto_scroll,
						window,
						cx,
					));
				}
			}
			directory = directory.child(crate::ui_motion::disclosure(
				SharedString::from(format!("navigation-disclosure-{}", section.key())),
				open,
				content,
			));
		}
		directory
	}

	fn navigation_projects(
		&self,
		items: &[Item],
		reveal: &PanelReveal,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> gpui::Div {
		let pref = self.workspace.navigation.preferences.section(Section::Projects);
		let mut content = gpui::div().flex().flex_col();
		let mut projects: Vec<_> = self.snapshot.iter().flat_map(|s| &s.projects).collect();
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
		if projects.is_empty() {
			return content.child(navigation_empty(reveal, "No projects yet."));
		}
		for project in projects {
			let mut children: Vec<_> = items
				.iter()
				.filter(|i| !i.assistant && i.project.as_deref() == Some(&project.id))
				.cloned()
				.collect();
			model::sort(&mut children, pref.sort);
			content = content.child(self.navigation_project(
				project,
				&children,
				reveal,
				pref.auto_scroll,
				window,
				cx,
			));
		}
		content
	}

	fn navigation_project(
		&self,
		project: &decodex_protocol::ProjectDto,
		items: &[Item],
		reveal: &PanelReveal,
		auto_scroll: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let id = project.id.as_str();
		let open = self.workspace.navigation.projects.contains(id);
		let key = format!("project-{id}");
		let mut conversations = gpui::div().flex().flex_col();
		if items.is_empty() {
			conversations = conversations.child(navigation_empty(reveal, "No conversations yet."));
		}
		for item in items {
			conversations = conversations.child(self.navigation_item(
				item,
				&key,
				reveal,
				auto_scroll,
				window,
				cx,
			));
		}
		let debug = format!("navigation-project-group-{id}");
		gpui::div()
			.debug_selector(move || debug.clone())
			.flex()
			.flex_col()
			.pb(gpui::px(if open && reveal.expanded {
				ui_theme::NAVIGATION_PROJECT_GAP
			} else {
				0.
			}))
			.child(self.navigation_project_entry(project, reveal, window, cx))
			.child(crate::ui_motion::disclosure(
				SharedString::from(format!("navigation-project-disclosure-{id}")),
				open,
				conversations,
			))
			.into_any_element()
	}

	fn navigation_utility(
		&self,
		id: &'static str,
		label: &'static str,
		glyph: Glyph,
		reveal: &PanelReveal,
		action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let action = std::rc::Rc::new(action);
		let keyboard = action.clone();
		let catalog = matches!(id, "archive-browser" | "codex-conversations");
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
			.rounded(gpui::px(ui_theme::SMALL_CONTROL_RADIUS))
			.cursor_pointer()
			.hover(|s| s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.when(!reveal.expanded, |row| {
				row.hover_hint(if id == "navigation-command-entry" {
					"Search and commands (⌘K)"
				} else {
					label
				})
			})
			.child(
				navigation_leading_slot()
					.text_color(ui_theme::rgb(TEXT_MUTED))
					.child(Icon { glyph, size: ui_theme::NAVIGATION_ICON_SIZE }),
			)
			.when(reveal.expanded, |row| {
				row.child(
					gpui::div()
						.flex_1()
						.whitespace_nowrap()
						.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
						.debug_selector(move || format!("{id}-label"))
						.when(catalog, navigation_category_label)
						.when(id == "navigation-command-entry", |label| {
							label.text_color(ui_theme::rgb(TEXT_MUTED))
						})
						.child(navigation_text(reveal, label)),
				)
				.when(id == "navigation-command-entry", |row| {
					row.child(
						gpui::div()
							.pr_1()
							.text_size(gpui::px(NAVIGATION_DETAIL_SIZE))
							.text_color(ui_theme::rgb(TEXT_MUTED))
							.child(navigation_text(reveal, "⌘K")),
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
		open: bool,
		reveal: &PanelReveal,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let key = format!("heading-{}", section.key());
		let focus = navigation_focus(&key, window, cx);
		let glyph = match section {
			Section::Pin => Glyph::Pin,
			Section::Assistant => Glyph::Assistant,
			Section::Chats => Glyph::Chats,
			Section::Projects => Glyph::Projects,
		};
		let mut row = gpui::div()
			.id(SharedString::from(format!("navigation-section-{}", section.key())))
			.debug_selector(move || format!("navigation-section-{}", section.key()))
			.track_focus(&focus)
			.role(Role::Button)
			.tab_index(0)
			.aria_label(section.label())
			.relative()
			.h(gpui::px(ui_theme::CONTROL_SIZE))
			.flex_none()
			.flex()
			.items_center()
			.rounded(gpui::px(ui_theme::SMALL_CONTROL_RADIUS))
			.overflow_hidden()
			.cursor_pointer()
			.on_click(cx.listener(move |s, _, _, cx| s.activate_navigation_heading(section, cx)))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					s.activate_navigation_heading(section, cx);
					cx.stop_propagation();
				}
			}))
			.when(!reveal.expanded, |row| row.hover_hint(section.label()))
			.child(
				navigation_leading_slot()
					.debug_selector(move || format!("navigation-heading-icon-{}", section.key()))
					.text_color(style::section_color(section))
					.child(Icon { glyph, size: ui_theme::NAVIGATION_ICON_SIZE }),
			)
			.child(
				gpui::div()
					.debug_selector(move || format!("navigation-section-label-{}", section.key()))
					.flex_1()
					.min_w_0()
					.map(navigation_category_label)
					.child(navigation_text(reveal, section.label())),
			);
		if section == Section::Assistant {
			let selected = !self.workspace.browsing
				&& self.workspace.work_detail.is_none()
				&& self.selected.as_deref().is_some_and(|id| self.is_assistant_conversation(id));
			let anchor =
				self.navigation_anchor(section.key(), section.key(), section.key(), window, cx);
			return conversation_row_feedback(
				row.aria_selected(selected).anchor_scroll(Some(anchor)),
				selected,
			)
			.into_any_element();
		}
		let shown = reveal.expanded
			&& (self.workspace.navigation.hovered.as_ref() == Some(&key)
				|| self.workspace.navigation.options == Some(section)
				|| (window.last_input_was_keyboard() && focus.contains_focused(window, cx)));
		let opacity = crate::ui_motion::value(
			SharedString::from(format!("{key}-actions")),
			if shown { 1. } else { 0. },
			window,
			cx,
		);
		row = row
			.aria_expanded(open)
			.hover(|row| row.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.on_hover(
				cx.listener(move |s, hovered: &bool, _, cx| s.navigation_hover(&key, *hovered, cx)),
			);
		if reveal.expanded {
			let mut controls = gpui::div()
				.flex()
				.items_center()
				.child(self.navigation_options(section, 1., window, cx));
			match section {
				Section::Chats => {
					controls = controls.child(self.navigation_create(
						"New chat",
						"navigation-create-chat".into(),
						|s, cx| s.new_project_agent(None, cx),
						cx,
					));
				},
				Section::Projects => {
					controls = controls.child(self.navigation_create(
						"New project",
						"navigation-create-projects".into(),
						|s, cx| s.add_navigation_project(cx),
						cx,
					));
				},
				Section::Assistant | Section::Pin => {},
			}
			row = row
				.child(navigation_context_actions(
					SharedString::from(format!("navigation-heading-actions-{}", section.key())),
					opacity,
					if section == Section::Pin { 1 } else { 2 },
					controls,
				))
				.child(navigation_chevron(
					SharedString::from(format!("navigation-chevron-{}", section.key())),
					open,
				));
		}

		row.into_any_element()
	}

	fn activate_navigation_heading(&mut self, section: Section, cx: &mut Context<Self>) {
		if section == Section::Assistant {
			self.open_assistant(cx);
			return;
		}
		if !self.workspace.navigation.collapsed.remove(section.key()) {
			self.workspace.navigation.collapsed.insert(section.key());
		}
		self.workspace.navigation.menu = None;
		self.workspace.navigation.confirm_delete = false;
		self.workspace.navigation.options = None;
		cx.notify();
	}

	fn toggle_navigation_project(&mut self, project: &str, cx: &mut Context<Self>) {
		if !self.workspace.navigation.projects.remove(project) {
			self.workspace.navigation.projects.insert(project.into());
		}
		self.workspace.navigation.menu = None;
		self.workspace.navigation.confirm_delete = false;
		cx.notify();
	}

	fn navigation_create(
		&self,
		label: &str,
		id: String,
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
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(4.))
			.cursor_pointer()
			.text_color(ui_theme::rgb(TEXT_MUTED))
			.hover(|s| {
				s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)).text_color(ui_theme::rgb(ui_theme::BLUE))
			})
			.hover_hint(tip.clone())
			.child(Icon { glyph: Glyph::Plus, size: ui_theme::NAVIGATION_ACTION_SIZE })
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

	fn navigation_project_entry(
		&self,
		project: &decodex_protocol::ProjectDto,
		reveal: &PanelReveal,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let id = project.id.as_str();
		let name = project.name.as_str();
		let hint = if reveal.expanded { project.directory.clone() } else { name.to_owned() };
		let project = id.to_owned();
		let keyboard = project.clone();
		let debug = project.clone();
		let key = format!("project-heading-{id}");
		let enter = key.clone();
		let focus = navigation_focus(&key, window, cx);
		let open = self.workspace.navigation.projects.contains(id);
		let hovered = self.workspace.navigation.hovered.as_ref() == Some(&key);
		let keyboard_focus = window.last_input_was_keyboard() && focus.contains_focused(window, cx);
		let opacity = crate::ui_motion::value(
			SharedString::from(format!("{key}-actions")),
			if reveal.expanded
				&& (hovered
					|| keyboard_focus
					|| self.workspace.navigation.menu.as_deref() == Some(&key))
			{
				1.
			} else {
				0.
			},
			window,
			cx,
		);
		let anchor =
			self.navigation_anchor(&format!("project-{id}"), id, "project-header", window, cx);
		let mut row = gpui::div()
			.id(SharedString::from(format!("project-header-{id}")))
			.debug_selector(move || format!("project-header-{debug}"))
			.anchor_scroll(Some(anchor))
			.track_focus(&focus)
			.role(Role::Button)
			.tab_index(0)
			.aria_label(format!("{name} project"))
			.aria_expanded(open)
			.h(gpui::px(ui_theme::CONTROL_SIZE))
			.flex_none()
			.w_full()
			.flex()
			.items_center()
			.rounded(gpui::px(ui_theme::SMALL_CONTROL_RADIUS))
			.overflow_hidden()
			.cursor_pointer()
			.hover(|row| row.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.hover_hint(hint)
			.on_hover(
				cx.listener(move |s, hovered: &bool, _, cx| {
					s.navigation_hover(&enter, *hovered, cx)
				}),
			)
			.on_click(cx.listener(move |s, _, _, cx| {
				s.toggle_navigation_project(&project, cx);
				cx.stop_propagation();
			}))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
				if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					s.toggle_navigation_project(&keyboard, cx);
					cx.stop_propagation();
				}
			}))
			.child(navigation_leading_slot().text_color(ui_theme::rgb(TEXT_MUTED)).child(
				icons::ProjectIcon {
					id: id.to_owned(),
					label: name.to_owned(),
					compact: !reveal.expanded,
				},
			))
			.child(navigation_project_label(
				id,
				name,
				reveal,
				hovered
					|| keyboard_focus
					|| self.workspace.navigation.preferences.section(Section::Projects).auto_scroll,
			));
		row = self.navigation_cancel_on_outside(row, key, cx);
		if reveal.expanded {
			row = row
				.child(navigation_context_actions(
					SharedString::from(format!("navigation-project-actions-{id}")),
					opacity,
					2,
					self.navigation_project_controls(id, name, cx),
				))
				.child(navigation_chevron(
					SharedString::from(format!("project-chevron-{id}")),
					open,
				));
		}

		row.into_any_element()
	}

	fn navigation_item(
		&self,
		item: &Item,
		section: &str,
		reveal: &PanelReveal,
		auto_scroll: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let expanded = reveal.expanded;
		let key = format!("{section}-{}", item.id);
		let work = self.snapshot.as_ref().and_then(|snapshot| {
			snapshot.work_items.iter().find(|work| work.id == item.id).map(|work| (snapshot, work))
		});
		let handoff = work.and_then(|(snapshot, work)| self.handoffs.item_for(snapshot, work));
		let (status, state) = match handoff {
			Some(handoff) if handoff.result => (AgentStatusStyle::UnreadResult, handoff.label),
			Some(handoff) => (AgentStatusStyle::Attention, handoff.label),
			None => AgentStatusStyle::presentation(&item.state),
		};
		let group = if section.starts_with("project-") { "projects" } else { section };
		let anchor = self.navigation_anchor(&key, &item.id, group, window, cx);
		let debug = format!("navigation-{key}");
		let focus = navigation_focus(&key, window, cx);
		let hovered = self.workspace.navigation.hovered.as_ref() == Some(&key);
		let keyboard = window.last_input_was_keyboard() && focus.contains_focused(window, cx);
		let menu_open = self.workspace.navigation.menu.as_deref() == Some(&key);
		let manageable = work.is_some();
		let shown = expanded && manageable && (hovered || keyboard || menu_open);
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
		let enter = key.clone();
		let timestamp = crate::ui_time::timestamp(item.updated);
		let mut row =
			gpui::div()
				.id(SharedString::from(format!("navigation-row-{key}")))
				.anchor_scroll(Some(anchor))
				.debug_selector(move || debug.clone())
				.track_focus(&focus)
				.role(Role::Button)
				.tab_index(0)
				.aria_label(format!("{} ({}) {}", item.title, item.location, timestamp))
				.aria_selected(selected)
				.when(expanded && status == AgentStatusStyle::Completed, |row| {
					row.aria_description(state.to_owned())
				})
				.h(gpui::px(ui_theme::CONTROL_SIZE))
				.min_w(gpui::px(ui_theme::CONTROL_SIZE))
				.w_full()
				.flex_none()
				.relative()
				.flex()
				.items_center()
				.overflow_hidden()
				.rounded(gpui::px(ui_theme::SMALL_CONTROL_RADIUS))
				.cursor_pointer()
				.map(|row| conversation_row_feedback(row, selected))
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
				}));
		row = self.navigation_cancel_on_outside(row, key.clone(), cx);
		row = navigation_item_tooltip(
			row,
			item,
			expanded,
			(status != AgentStatusStyle::Quiet).then_some(state),
		);
		let base = if !expanded || item.draft { 0. } else { 44. };
		let inset = base + (76. - base) * actions;
		let label = navigation_item_label(
			item,
			&key,
			status,
			state,
			expanded && (auto_scroll || hovered || keyboard),
			reveal,
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
				row = row.child(navigation_item_time(item, actions, reveal));
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

	pub(super) fn new_project_agent(&mut self, project: Option<&str>, cx: &mut Context<Self>) {
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
		let section = if let Some(id) = project {
			self.workspace.navigation.projects.insert(id.to_owned());
			Section::Projects
		} else {
			Section::Chats
		};
		self.workspace.navigation.collapsed.remove(section.key());
		self.workspace.work_detail = None;
		self.new_work_conversation(cx);
		if let Some(id) = self.workspace.new_conversation.clone() {
			self.prepare_navigation_reveal(&id, section.key());
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
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	pub(super) fn visual_navigation_overview(&mut self, compact: bool, cx: &mut Context<Self>) {
		self.visual_navigation_hierarchy(compact, cx);
		let snapshot = self.snapshot.as_mut().expect("visual workspace");
		let seed = snapshot.work_items[0].clone();
		let now = crate::ui_time::now();
		for index in 0..40 {
			let mut chat = seed.clone();
			chat.kind = decodex_protocol::AgentWorkKindDto::Goal;
			chat.parent_goal_id = None;
			chat.id = format!("chat-{index}");
			chat.title = format!(
				"{} {}",
				["Weekly priorities", "Release planning", "Research notes", "Writing review"]
					[index % 4],
				index / 4 + 1
			);
			chat.codex_thread_id = Some(format!("chat-thread-{index}"));
			chat.updated_at_micros = now - index as i64 * 900_000_000;
			snapshot.work_items.push(chat);
		}
		self.workspace.navigation.collapsed.remove(Section::Chats.key());
		self.workspace.navigation.preferences.pinned = std::collections::BTreeSet::from([
			"chat-0".into(),
			"navigation-project-0-agent-0".into(),
		]);
		cx.notify();
	}

	pub(super) fn visual_navigation_hierarchy(&mut self, compact: bool, cx: &mut Context<Self>) {
		self.visual_workspace_fixture(cx);
		self.visual_personal_navigation(cx);
		let snapshot = self.snapshot.as_mut().expect("visual workspace");
		let seed = snapshot.work_items[0].clone();
		for (index, name) in [
			"Decodex",
			"Data tools",
			"HAST",
			"ProfitPilot",
			"Research",
			"Design system",
			"Notes",
			"Deployments",
			"Docs",
			"Automations",
			"Desktop",
			"Observability",
		]
		.into_iter()
		.enumerate()
		{
			let id = format!("navigation-project-{index}");
			let mut agents = Vec::new();
			for (child, title) in ["Review next steps", "Working notes"].into_iter().enumerate() {
				let agent_id = format!("{id}-agent-{child}");
				let mut work = seed.clone();
				work.id = agent_id.clone();
				work.title = title.into();
				work.codex_thread_id = Some(format!("fixture-{agent_id}"));
				snapshot.work_items.push(work);
				snapshot.agents.push(decodex_protocol::work::Agent {
					id: agent_id.clone(),
					name: title.into(),
					native_thread_id: None,
					assistant: false,
					managed_conversation_ids: Default::default(),
					scheduled_wakes: Default::default(),
				});
				agents.push(agent_id);
			}
			snapshot.projects.push(decodex_protocol::ProjectDto {
				id: id.clone(),
				name: name.into(),
				directory: format!("/workspace/{name}"),
				native_project_id: None,
				lead_agent_id: None,
				agent_ids: agents,
			});
		}
		self.workspace.navigation.collapsed.remove(Section::Projects.key());
		self.workspace.navigation.projects.insert("navigation-project-0".into());
		self.selected = Some("navigation-project-0-agent-0".into());
		self.workspace.sidebar_visible = !compact;
		self.workspace.sidebar_peek = false;
		self.workspace.work_detail = None;
		cx.notify();
	}

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
			managed_conversation_ids: Default::default(),
			scheduled_wakes: Default::default(),
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
			[("research-chat", "Research workflow"), ("preferences-chat", "Working preferences")]
		{
			let mut chat = seed.clone();
			chat.kind = decodex_protocol::AgentWorkKindDto::Goal;
			chat.parent_goal_id = None;
			chat.id = id.into();
			chat.title = title.into();
			chat.codex_thread_id = Some(format!("fixture-{id}"));
			snapshot.work_items.push(chat);
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
/// Keep the conversation lists on the palette's selection and hover colors.
pub(super) fn conversation_row_feedback(
	row: gpui::Stateful<gpui::Div>,
	selected: bool,
) -> gpui::Stateful<gpui::Div> {
	let feedback = if selected { ui_theme::SELECTED_HOVER_FILL } else { ui_theme::HOVER_FILL };
	row.when(selected, |row| row.bg(ui_theme::rgba(ui_theme::SELECTED_FILL)))
		.hover(move |row| row.bg(ui_theme::rgba(feedback)))
		.focus_visible(move |row| row.bg(ui_theme::rgba(feedback)))
		.active(move |row| {
			row.bg(ui_theme::rgba(if selected {
				ui_theme::SELECTED_HOVER_FILL
			} else {
				ui_theme::PRESSED_FILL
			}))
		})
}

#[cfg(test)]
pub(in crate::shell::agent_surface) fn check_catalog_selection_paint(
	visual: &mut gpui::VisualTestContext,
	row: &'static str,
) {
	use ui_theme::{ColorTheme, appearance::Appearance};
	let saved = (Appearance::current(), ColorTheme::configured());
	for (appearance, colors) in [
		(Appearance::Dark, ColorTheme::Decodex),
		(Appearance::Dark, ColorTheme::TokyoNight),
		(Appearance::Light, ColorTheme::Decodex),
		(Appearance::Light, ColorTheme::TokyoNight),
	] {
		visual.update(|_, cx| {
			appearance.apply(cx);
			colors.apply(cx);
		});
		visual.update(crate::ui_frame::draw_frame);
		let bounds = visual.debug_bounds(row).expect("visible selected conversation");
		for (pointer, token) in [
			(gpui::point(gpui::px(0.), gpui::px(0.)), ui_theme::SELECTED_FILL),
			(bounds.center(), ui_theme::SELECTED_HOVER_FILL),
		] {
			visual.simulate_mouse_move(pointer, None, Default::default());
			visual.update(|window, cx| {
				crate::ui_frame::draw_frame(window, cx);
				let expected = gpui::Background::from(ui_theme::rgba(token));
				let center = bounds.center().scale(window.scale_factor());
				assert!(
					window
						.painted_quads()
						.iter()
						.any(|quad| quad.background == expected && quad.bounds.contains(&center)),
					"selected {row} in {appearance:?}/{colors:?} uses {token:#010x}"
				);
			});
		}
	}
	visual.update(|_, cx| {
		saved.0.apply(cx);
		saved.1.apply(cx);
	});
}

fn navigation_category_label(label: gpui::Div) -> gpui::Div {
	label
		.text_size(gpui::px(ui_theme::NAVIGATION_SECTION_SIZE))
		.font_weight(FontWeight::MEDIUM)
		.text_color(ui_theme::ColorScheme::current().navigation_heading())
}

fn navigation_context_actions(
	id: SharedString,
	opacity: f32,
	buttons: usize,
	content: impl gpui::IntoElement,
) -> AnyElement {
	let width = ui_theme::CHROME_CONTROL_SIZE * buttons as f32;
	gpui::div()
		.debug_selector(move || id.to_string())
		.w(gpui::px(width * opacity))
		.h_full()
		.flex_none()
		.overflow_hidden()
		.opacity(opacity)
		.when(opacity == 0., |strip| strip.invisible())
		.child(gpui::div().w(gpui::px(width)).h_full().flex().items_center().child(content))
		.into_any_element()
}

fn navigation_chevron(id: SharedString, open: bool) -> AnyElement {
	gpui::div()
		.w(gpui::px(ui_theme::CHROME_CONTROL_SIZE))
		.h_full()
		.flex_none()
		.flex()
		.items_center()
		.justify_center()
		.text_color(ui_theme::rgb(TEXT_MUTED))
		.child(workspace_symbols::process_chevron(id, open))
		.into_any_element()
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
		.px(gpui::px(ui_theme::SIDEBAR_INSET))
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
	state: Option<&str>,
) -> gpui::Stateful<gpui::Div> {
	let bounds = std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::<gpui::Pixels>::default()));
	let measured = bounds.clone();
	let title = (!expanded).then(|| item.title.clone());
	let location = item.location.clone();
	let time = crate::ui_time::timestamp(item.updated);
	let state = state.map(str::to_owned);
	row.tooltip(move |_, cx| {
		let title = title.clone();
		let location = location.clone();
		let time = time.clone();
		let state = state.clone();
		crate::ui_motion::tooltip_at(cx, bounds.clone(), move |_, _| {
			gpui::div()
				.debug_selector(|| "navigation-metadata-tooltip".into())
				.flex()
				.flex_col()
				.gap(gpui::px(2.))
				.children(title.clone())
				.children(state.clone())
				.child(location.clone())
				.child(
					gpui::div()
						.text_color(ui_theme::rgb(TEXT_MUTED))
						.child(format!("Updated: {time}")),
				)
				.into_any_element()
		})
	})
	.child(
		gpui::canvas(move |bounds, _, _| measured.set(bounds), |_, _, _, _| {})
			.absolute()
			.inset_0(),
	)
}

impl AgentSurface {
	fn navigation_cancel_on_outside(
		&self,
		row: gpui::Stateful<gpui::Div>,
		key: String,
		cx: &mut Context<Self>,
	) -> gpui::Stateful<gpui::Div> {
		let confirming = self.workspace.navigation.confirm_delete
			&& self.workspace.navigation.menu.as_deref() == Some(&key);
		row.when(confirming, |row| {
			row.on_mouse_down_out(cx.listener(move |s, _, _, cx| {
				if s.workspace.navigation.menu.as_ref() == Some(&key) {
					s.dismiss_navigation_confirmation(cx);
				}
			}))
		})
	}

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
			self.workspace.navigation.confirm_started = None;
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
			if self.workspace.navigation.confirm_delete
				&& self.workspace.navigation.menu.as_deref() == Some(key)
			{
				self.dismiss_navigation_confirmation(cx);
			}
		}
		if before != self.workspace.navigation.hovered {
			self.presentations.sidebar.update(cx, |_, cx| cx.notify());
		}
	}
}

impl AgentSurface {
	pub(super) fn reveal_navigation_project(&mut self, id: &str, cx: &mut Context<Self>) {
		self.workspace.sidebar_visible = true;
		self.workspace.navigation.collapsed.remove(Section::Projects.key());
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
		let scroll = navigation_scroll("directory", window, cx);
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
					.update(cx, |s, cx| {
						s.presentations.sidebar.update(cx, |_, cx| cx.notify());
						// Conversation previews observe this owner and share the same clock.
						cx.notify();
					})
					.is_err()
				{
					break;
				}
			}
		}));
	}
}

fn navigation_leading_slot() -> gpui::Div {
	gpui::div()
		.w(gpui::px(ui_theme::NAVIGATION_LEADING_SIZE))
		.h(gpui::px(ui_theme::CONTROL_SIZE))
		.flex_none()
		.flex()
		.items_center()
		.justify_center()
}

fn navigation_project_label(
	id: &str,
	name: &str,
	reveal: &PanelReveal,
	active: bool,
) -> AnyElement {
	let debug = format!("navigation-project-label-{id}");
	gpui::div()
		.debug_selector(move || debug.clone())
		.flex_1()
		.min_w_0()
		.h_full()
		.flex()
		.items_center()
		.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
		.font_weight(FontWeight::NORMAL)
		.text_color(ui_theme::rgb(ui_theme::foreground::BODY))
		.child(reveal.viewport.clip(
			ui_theme::SIDEBAR_INSET,
			crate::ui_motion::HoverOverflowLabel {
				id: SharedString::from(format!("navigation-project-title-{id}")).into(),
				text: name.to_owned().into(),
				active: reveal.expanded && active,
			},
		))
		.into_any_element()
}

fn navigation_item_label(
	item: &Item,
	key: &str,
	status: AgentStatusStyle,
	state: &str,
	scroll: bool,
	reveal: &PanelReveal,
) -> AnyElement {
	let label_debug = format!("navigation-item-label-{key}");
	let leading_debug = format!("navigation-leading-{key}");
	let compact = !reveal.expanded;
	let show_mark =
		compact || !matches!(status, AgentStatusStyle::Quiet | AgentStatusStyle::Completed);
	gpui::div()
		.h_full()
		.flex_1()
		.min_w_0()
		.flex()
		.items_center()
		.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
		.font_weight(FontWeight::NORMAL)
		.text_color(ui_theme::rgb(ui_theme::foreground::BODY))
		.child(navigation_leading_slot().debug_selector(move || leading_debug.clone()).when(
			show_mark,
			|slot| {
				slot.child(icons::ConversationMark {
					id: format!("navigation-{key}"),
					state: state.to_owned(),
					style: status,
					label: item.title.clone(),
					compact,
				})
			},
		))
		.child(gpui::div().debug_selector(move || label_debug.clone()).flex_1().min_w_0().child(
			reveal.viewport.clip(
				ui_theme::SIDEBAR_INSET,
				crate::ui_motion::HoverOverflowLabel {
					id: SharedString::from(format!("navigation-title-{key}")).into(),
					text: item.title.clone().into(),
					active: scroll,
				},
			),
		))
		.into_any_element()
}

fn navigation_empty(reveal: &PanelReveal, message: impl Into<SharedString>) -> AnyElement {
	if !reveal.expanded {
		return gpui::div().into_any_element();
	}
	gpui::div()
		.pl(gpui::px(ui_theme::NAVIGATION_LEADING_SIZE))
		.h(gpui::px(ui_theme::CONTROL_SIZE))
		.flex_none()
		.flex()
		.items_center()
		.text_size(gpui::px(ui_theme::NAVIGATION_TITLE_SIZE))
		.text_color(ui_theme::rgb(TEXT_MUTED))
		.child(navigation_text(reveal, message.into()))
		.into_any_element()
}

fn navigation_item_time(item: &Item, actions: f32, reveal: &PanelReveal) -> impl gpui::IntoElement {
	ui_theme::metric_text()
		.absolute()
		.right(gpui::px(5.))
		.w(gpui::px(34.))
		.h_full()
		.flex()
		.items_center()
		.justify_end()
		.opacity(1. - actions)
		.text_color(ui_theme::metric_ink())
		.child(navigation_text(
			reveal,
			crate::ui_time::relative_time(item.updated, crate::ui_time::now()),
		))
}

fn navigation_text(reveal: &PanelReveal, child: impl gpui::IntoElement) -> AnyElement {
	reveal.viewport.clip(ui_theme::SIDEBAR_INSET, child).into_any_element()
}

#[cfg(test)]
#[path = "agent_navigation_sidebar_tests.rs"]
mod interaction_tests;
