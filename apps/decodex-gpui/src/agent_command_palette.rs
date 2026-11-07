//! Global discovery keeps navigation and draft ownership in their existing consumers.
use super::{AgentSurface, Context, Item, Section, model};
use crate::{composer_input::ComposerInput, ui_motion, ui_theme};
use gpui::{
	AnyElement, AppContext as _, Focusable as _, KeyDownEvent, Role, SharedString, Window,
	prelude::*,
};

#[derive(Clone)]
enum Action {
	Conversation(Item),
	Project(String),
	NewAssistant,
	NewProject,
	ChooseProject,
	NewAgent(String),
	Archives,
	Settings,
	Work(String, String),
}
struct Command {
	label: String,
	detail: String,
	action: Action,
}
impl AgentSurface {
	pub(crate) fn open_global_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.workspace.navigation.command_open {
			self.workspace.navigation.command_open = false;
			cx.notify();
			return;
		}
		if self.workspace.navigation.search.is_none() {
			let input = cx.new(|cx| {
				ComposerInput::with_placeholder(
					0,
					"Search topics, projects and commands…",
					"Search topics, projects and commands",
					cx,
				)
			});
			cx.observe(&input, |s, _, cx| {
				s.workspace.navigation.command_index = 0;
				cx.notify();
			})
			.detach();
			self.workspace.navigation.search = Some(input);
		}
		let input = self
			.workspace
			.navigation
			.search
			.as_ref()
			.expect("initialize command search before focusing it");
		input.update(cx, |input, cx| {
			input.clear(cx);
			input.set_placeholder("Search topics, projects and commands…", cx);
		});
		window.focus(&input.focus_handle(cx), cx);
		self.workspace.navigation.command_open = true;
		self.workspace
			.navigation
			.command_scroll
			.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
		self.workspace.navigation.choose_project = false;
		self.workspace.navigation.options = None;
		self.workspace.navigation.confirm_delete = false;
		self.load_navigation_archives(cx);
		cx.notify();
	}

	fn global_commands(&self, cx: &Context<Self>) -> Vec<Command> {
		let query = self
			.workspace
			.navigation
			.search
			.as_ref()
			.map(|s| s.read(cx).content().trim().to_lowercase())
			.unwrap_or_default();
		let mut out = Vec::new();
		if self.workspace.navigation.choose_project {
			if let Some(snapshot) = &self.snapshot {
				for project in &snapshot.projects {
					if model::matches(&query, &format!("{} {}", project.name, project.directory)) {
						out.push(Command {
							label: project.name.clone(),
							detail: project.directory.clone(),
							action: Action::NewAgent(project.id.clone()),
						});
					}
				}
			}
			out.push(Command {
				label: "New project".into(),
				detail: "Choose a project folder".into(),
				action: Action::NewProject,
			});
			return out;
		}
		for (label, detail, action) in [
			("New Assistant topic", "Assistant", Action::NewAssistant),
			("New project", "Choose a project folder", Action::NewProject),
			("New project Agent", "Choose a project", Action::ChooseProject),
			("Archived conversations", "Browse and restore", Action::Archives),
			("Settings", "App preferences", Action::Settings),
		] {
			if model::matches(&query, &format!("{label} {detail}")) {
				out.push(Command { label: label.into(), detail: detail.into(), action });
			}
		}
		let mut items = self.navigation_items(cx);
		if !query.is_empty() {
			items.extend(self.navigation_archived_items());
		}
		model::sort(&mut items, super::Sort::Updated);
		for item in items
			.into_iter()
			.filter(|i| model::matches(&query, &format!("{} {}", i.title, i.owner)))
			.enumerate()
			.filter_map(|(index, item)| (!query.is_empty() || index < 12).then_some(item))
		{
			out.push(Command {
				label: item.title.clone(),
				detail: format!("{}{}", item.owner, if item.archived { " (Archived)" } else { "" }),
				action: Action::Conversation(item),
			});
		}
		if let Some(snapshot) = &self.snapshot {
			for project in &snapshot.projects {
				if model::matches(&query, &format!("{} {}", project.name, project.directory)) {
					out.push(Command {
						label: project.name.clone(),
						detail: project.directory.clone(),
						action: Action::Project(project.id.clone()),
					});
				}
			}
			if !query.is_empty() {
				for work in &snapshot.portfolio.works {
					if model::matches(&query, &work.title) {
						out.push(Command {
							label: work.title.clone(),
							detail: "Work".into(),
							action: Action::Work(work.id.clone(), work.owner_agent_id.clone()),
						});
					}
				}
			}
		}
		out
	}

	fn run_global_command(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
		let Some(action) = self.global_commands(cx).get(index).map(|item| item.action.clone())
		else {
			return;
		};
		self.workspace.navigation.command_open = false;
		match action {
			Action::Conversation(item) => {
				if !item.archived {
					let section =
						if item.assistant { Section::Assistant } else { Section::Projects };
					self.workspace.navigation.collapsed.remove(section.key());
					if !item.assistant {
						self.workspace
							.navigation
							.projects
							.insert(item.project.clone().unwrap_or_default());
					}
					self.prepare_navigation_reveal(&item.id, section.key());
				}
				if item.archived {
					self.workspace.navigation.archives_open = true;
					self.workspace.sidebar_visible = true;
					self.prepare_navigation_reveal(&item.id, "archive");
				}
				self.activate_navigation_item(&item, cx);
			},
			Action::Project(id) => self.reveal_navigation_project(&id, cx),
			Action::NewAssistant => {
				self.workspace.navigation.collapsed.remove("assistant");
				self.new_assistant_conversation(cx);
			},
			Action::NewProject => self.add_navigation_project(cx),
			Action::ChooseProject => {
				self.workspace.navigation.command_open = true;
				self.workspace.navigation.choose_project = true;
				if let Some(input) = &self.workspace.navigation.search {
					input.update(cx, |input, cx| {
						input.clear(cx);
						input.set_placeholder("Choose a project…", cx);
					});
				}
			},
			Action::NewAgent(project) => self.new_project_agent(Some(&project), cx),
			Action::Archives => {
				self.workspace.sidebar_visible = true;
				self.workspace.navigation.archives_open = true;
				self.load_navigation_archives(cx);
			},
			Action::Settings =>
				window.dispatch_action(Box::new(crate::shell::ActivateSettings), cx),
			Action::Work(id, owner) => {
				self.open_navigation_conversation(&owner, cx);
				self.workspace.work_detail = Some(id.clone());
				self.load_work_detail(id, cx);
			},
		}
		cx.notify();
	}

	pub(in crate::shell::agent_surface) fn render_global_commands(
		&self,
		_window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let owner = cx.entity().downgrade();
		let dismiss = owner.clone();
		let focus = self.workspace.navigation.search.as_ref().map(|input| input.focus_handle(cx));
		gpui::div()
			.absolute()
			.top(gpui::px(64.))
			.left_0()
			.w_full()
			.flex()
			.justify_center()
			.child(
				gpui::div().w(gpui::px(540.)).max_w_full().child(
					ui_motion::popover_lazy(
						"global-command-palette",
						self.workspace.navigation.command_open,
						move |window, cx| {
							owner
								.update(cx, |s, cx| s.command_palette_content(window, cx))
								.unwrap_or_else(|_| gpui::div().into_any_element())
						},
					)
					.focus(focus)
					.on_dismiss(move |cx| {
						let _ = dismiss.update(cx, |s, cx| {
							s.workspace.navigation.command_open = false;
							cx.notify();
						});
					}),
				),
			)
			.into_any_element()
	}

	fn command_palette_content(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		let Some(search) = &self.workspace.navigation.search else {
			return gpui::div().into_any_element();
		};
		let commands = self.global_commands(cx);
		let selected =
			self.workspace.navigation.command_index.min(commands.len().saturating_sub(1));
		let scroll = self.workspace.navigation.command_scroll.clone();
		scroll.scroll_to_item(selected);
		let mut rows = gpui::div()
			.id("global-command-results")
			.track_scroll(&scroll)
			.max_h(gpui::px(420.))
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.p(gpui::px(4.));
		if commands.is_empty() {
			rows = rows.child(
				gpui::div()
					.p_2()
					.text_size(gpui::px(12.))
					.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
					.child("No matching conversations or commands."),
			);
		}
		for (index, command) in commands.into_iter().enumerate() {
			rows = rows.child(
				gpui::div()
					.id(SharedString::from(format!("global-command-result-{index}")))
					.debug_selector(move || format!("global-command-result-{index}"))
					.role(Role::Button)
					.aria_label(format!("{} ({})", command.label, command.detail))
					.aria_selected(index == selected)
					.min_h(gpui::px(40.))
					.px(gpui::px(10.))
					.py(gpui::px(5.))
					.rounded(gpui::px(5.))
					.bg(ui_theme::rgba(if index == selected {
						ui_theme::SELECTED_FILL
					} else {
						0xffffff00
					}))
					.flex()
					.flex_col()
					.cursor_pointer()
					.child(gpui::div().text_size(gpui::px(12.)).child(command.label))
					.child(
						gpui::div()
							.text_size(gpui::px(10.5))
							.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
							.child(command.detail),
					)
					.on_hover(cx.listener(move |s, h: &bool, _, cx| {
						if *h && s.workspace.navigation.command_index != index {
							s.workspace.navigation.command_index = index;
							cx.notify();
						}
					}))
					.on_click(cx.listener(move |s, _, window, cx| {
						s.run_global_command(index, window, cx);
						cx.stop_propagation();
					})),
			);
		}
		ui_theme::menu_outline(gpui::div())
			.id("global-command-dialog")
			.role(Role::Dialog)
			.aria_label("Search and commands")
			.w_full()
			.flex()
			.flex_col()
			.capture_key_down(cx.listener(|s, event: &KeyDownEvent, window, cx| {
				let length = s.global_commands(cx).len();
				match event.keystroke.key.as_str() {
					"escape" => s.workspace.navigation.command_open = false,
					"down" if length > 0 =>
						s.workspace.navigation.command_index =
							(s.workspace.navigation.command_index + 1) % length,
					"up" if length > 0 =>
						s.workspace.navigation.command_index =
							(s.workspace.navigation.command_index + length - 1) % length,
					"enter" if !event.is_held =>
						s.run_global_command(s.workspace.navigation.command_index, window, cx),
					_ => return,
				}
				cx.stop_propagation();
				cx.notify();
			}))
			.on_action(cx.listener(|s, _: &crate::composer_input::SubmitComposer, window, cx| {
				s.run_global_command(s.workspace.navigation.command_index, window, cx);
				cx.stop_propagation();
			}))
			.child(
				gpui::div()
					.h(gpui::px(40.))
					.px(gpui::px(10.))
					.flex()
					.items_center()
					.gap(gpui::px(8.))
					.border_b_1()
					.border_color(ui_theme::rgba(ui_theme::PANEL_DIVIDER))
					.child(super::Icon { glyph: super::Glyph::Search, size: 14. })
					.child(gpui::div().flex_1().min_w_0().child(search.clone())),
			)
			.child(rows)
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn command_search_reaches_work_and_keeps_the_original_input(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_personal_navigation(cx);
			s.composer.update(cx, |i, cx| i.set_content("Do not send this draft", cx));
			cx.notify();
		});
		visual.update(|window, cx| surface.update(cx, |s, cx| s.open_global_commands(window, cx)));
		surface.update(visual, |s, cx| {
			s.workspace
				.navigation
				.search
				.as_ref()
				.unwrap()
				.update(cx, |i, cx| i.set_content("Choose a research stack", cx));
			assert_eq!(s.global_commands(cx).len(), 1);
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let result = visual.debug_bounds("global-command-result-0").unwrap();
		visual.simulate_click(result.center(), Default::default());
		surface.read_with(visual, |s, cx| {
			assert_eq!(s.workspace.work_detail.as_deref(), Some("research"));
			assert_eq!(s.composer.read(cx).content(), "Do not send this draft");
			assert!(!s.workspace.navigation.command_open);
			assert!(!s.sending);
		});
	}
	#[gpui::test]
	fn command_keyboard_selects_a_topic_without_submitting_chat(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1440.), gpui::px(1000.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_personal_navigation(cx);
			s.composer.update(cx, |i, cx| i.set_content("Keep original input", cx));
		});
		visual.update(|window, cx| surface.update(cx, |s, cx| s.open_global_commands(window, cx)));
		surface.update(visual, |s, cx| {
			s.workspace
				.navigation
				.search
				.as_ref()
				.unwrap()
				.update(cx, |i, cx| i.set_content("Working preferences", cx))
		});
		visual.update(crate::ui_frame::draw_current_frame);
		visual.simulate_keystrokes("enter");
		surface.update(visual, |s, cx| {
			assert_eq!(s.selected.as_deref(), Some("topic-preferences"));
			assert!(!s.sending);
			assert!(!s.workspace.navigation.command_open);
			s.open_navigation_conversation("agent", cx);
			assert_eq!(s.composer.read(cx).content(), "Keep original input");
		});
	}
}
