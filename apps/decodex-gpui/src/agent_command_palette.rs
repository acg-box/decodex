//! Global discovery keeps navigation and draft ownership in their existing consumers.
#[cfg(test)]
#[path = "agent_command_palette_tests.rs"]
mod tests;
#[path = "agent_command_palette_view.rs"] mod view;

use super::{AgentSurface, Context, Item, Section, model};
use crate::composer_input::{ComposerEvent, ComposerInput};
use gpui::{App, AppContext as _, Focusable as _, KeyBinding, Window};

gpui::actions!(
	decodex_command_palette,
	[PreviousResult, NextResult, AcceptResult, ExitPalette, EraseSearch]
);

#[derive(Clone, Copy)]
enum CommandKey {
	Previous,
	Next,
	Accept,
	Exit,
	Erase,
}

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

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
enum Group {
	Conversations,
	Projects,
	Work,
	Commands,
}

struct Command {
	label: String,
	detail: String,
	action: Action,
}

impl Command {
	fn identity(&self) -> String {
		match &self.action {
			Action::Conversation(item) => format!("conversation-{}", item.id),
			Action::Project(id) => format!("project-{id}"),
			Action::NewAgent(id) => format!("new-agent-{id}"),
			Action::Work(id, _) => format!("work-{id}"),
			_ => self.label.clone(),
		}
	}

	fn group(&self) -> Group {
		match &self.action {
			Action::Conversation(_) => Group::Conversations,
			Action::Project(_) | Action::NewAgent(_) => Group::Projects,
			Action::Work(_, _) => Group::Work,
			_ => Group::Commands,
		}
	}

	fn verb(&self) -> &'static str {
		match self.action {
			Action::NewAssistant | Action::NewProject => "Create",
			Action::ChooseProject => "Choose project",
			Action::NewAgent(_) => "Start Agent",
			_ => "Open",
		}
	}
}

/// Prefer title matches to incidental owner or folder matches, keeping each group together.
fn ranked(commands: Vec<Command>, query: &str) -> Vec<Command> {
	let mut items: Vec<_> = commands
		.into_iter()
		.filter_map(|command| match_score(&command, query).map(|score| (score, command)))
		.collect();
	let mut groups = [u8::MAX; 4];
	for (score, command) in &items {
		let rank = &mut groups[command.group() as usize];
		*rank = (*rank).min(*score);
	}
	items.sort_by_key(|(score, command)| {
		(groups[command.group() as usize], command.group(), *score)
	});
	items.into_iter().map(|(_, command)| command).collect()
}

fn match_score(command: &Command, query: &str) -> Option<u8> {
	let title = command.label.to_lowercase();
	if query.is_empty() || title == query {
		Some(0)
	} else if title.starts_with(query) {
		Some(1)
	} else if title.contains(query) {
		Some(2)
	} else if model::matches(query, &title) {
		Some(3)
	} else {
		model::matches(query, &format!("{title} {}", command.detail)).then_some(4)
	}
}

/// Scroll indices include group labels; command indices never include them.
fn display_index(commands: &[Command], selected: usize) -> usize {
	let mut previous = None;
	let mut headers = 0;
	for command in commands.iter().take(selected + 1) {
		if previous != Some(command.group()) {
			headers += 1;
			previous = Some(command.group());
		}
	}
	selected + headers
}

impl AgentSurface {
	/// Native text input resolves these bindings before ordinary editor navigation.
	pub(crate) fn bind_command_keys(cx: &mut App) {
		let context = Some("GlobalCommands > ComposerInput");
		cx.bind_keys([
			KeyBinding::new("up", PreviousResult, context),
			KeyBinding::new("down", NextResult, context),
			KeyBinding::new("enter", AcceptResult, context),
			KeyBinding::new("cmd-enter", AcceptResult, context),
			KeyBinding::new("escape", ExitPalette, context),
			KeyBinding::new("backspace", EraseSearch, context),
		]);
	}

	fn command_key(&mut self, key: CommandKey, window: &mut Window, cx: &mut Context<Self>) {
		cx.stop_propagation();
		if self
			.workspace
			.navigation
			.search
			.as_ref()
			.is_some_and(|input| input.read(cx).is_composing())
		{
			return;
		}
		let commands = self.global_commands(cx);
		let length = commands.len();
		match key {
			CommandKey::Previous if length > 0 => self.select_global_command(
				(self.workspace.navigation.command_index + length - 1) % length,
				&commands,
			),
			CommandKey::Next if length > 0 => self.select_global_command(
				(self.workspace.navigation.command_index + 1) % length,
				&commands,
			),
			CommandKey::Accept =>
				self.run_global_command(self.workspace.navigation.command_index, window, cx),
			CommandKey::Exit if self.workspace.navigation.choose_project =>
				self.reset_command_search(false, cx),
			CommandKey::Exit => self.workspace.navigation.command_open = false,
			CommandKey::Erase
				if self.workspace.navigation.choose_project
					&& self.command_query(cx).is_empty() =>
				self.reset_command_search(false, cx),
			CommandKey::Erase =>
				window.dispatch_action(Box::new(crate::composer_input::Backspace), cx),
			_ => {},
		}
		// The owner lives in the main window; repaint the native popup immediately.
		window.refresh();
		cx.notify();
	}

	pub(crate) fn open_global_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.workspace.navigation.command_open {
			self.workspace.navigation.command_open = false;
			cx.notify();
			return;
		}
		self.initialize_command_search(cx);
		self.reset_command_search(false, cx);
		let input = self.workspace.navigation.search.as_ref().expect("initialized search");
		window.focus(&input.focus_handle(cx), cx);
		self.workspace.navigation.command_open = true;
		self.workspace.navigation.command_session += 1;
		self.workspace.navigation.options = None;
		self.workspace.navigation.confirm_delete = false;
		self.load_navigation_archives(cx);
		cx.notify();
	}

	fn initialize_command_search(&mut self, cx: &mut Context<Self>) {
		if self.workspace.navigation.search.is_none() {
			let input = cx.new(|cx| {
				ComposerInput::search(
					"Search or jump to…",
					"Search topics, projects and commands",
					cx,
				)
			});
			cx.subscribe(&input, |s, _, event: &ComposerEvent, cx| {
				if !matches!(event, ComposerEvent::Changed) {
					return;
				}
				s.workspace.navigation.command_index = 0;
				s.workspace
					.navigation
					.command_scroll
					.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
				cx.notify();
			})
			.detach();
			self.workspace.navigation.search = Some(input);
		}
	}

	fn reset_command_search(&mut self, choose_project: bool, cx: &mut Context<Self>) {
		self.workspace.navigation.choose_project = choose_project;
		self.workspace.navigation.command_index = 0;
		self.workspace
			.navigation
			.command_scroll
			.set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
		if let Some(input) = &self.workspace.navigation.search {
			input.update(cx, |input, cx| {
				input.clear(cx);
				input.set_placeholder(
					if choose_project { "Search projects…" } else { "Search or jump to…" },
					cx,
				);
			});
		}
	}

	fn command_query(&self, cx: &Context<Self>) -> String {
		self.workspace
			.navigation
			.search
			.as_ref()
			.map(|input| input.read(cx).content().trim().to_lowercase())
			.unwrap_or_default()
	}

	fn global_commands(&self, cx: &Context<Self>) -> Vec<Command> {
		let query = self.command_query(cx);
		let mut commands = Vec::new();
		if self.workspace.navigation.choose_project {
			self.project_commands(true, &mut commands);
			commands.push(Command {
				label: "New project".into(),
				detail: "Choose a project folder".into(),
				action: Action::NewProject,
			});
		} else {
			let mut items = self.navigation_items(cx);
			if !query.is_empty() {
				items.extend(self.navigation_archived_items());
			}
			model::sort(&mut items, super::Sort::Updated);
			for item in items.into_iter().take(if query.is_empty() { 6 } else { usize::MAX }) {
				commands.push(Command {
					label: item.title.clone(),
					detail: format!(
						"{}{}",
						item.owner,
						if item.archived { " (Archived)" } else { "" }
					),
					action: Action::Conversation(item),
				});
			}
			for (label, detail, action) in [
				("New Assistant topic", "Assistant", Action::NewAssistant),
				("New project Agent", "Choose a project", Action::ChooseProject),
				("New project", "Choose a project folder", Action::NewProject),
				("Archived conversations", "Browse and restore", Action::Archives),
				("Settings", "App preferences", Action::Settings),
			] {
				commands.push(Command { label: label.into(), detail: detail.into(), action });
			}
			if !query.is_empty() {
				self.project_commands(false, &mut commands);
				if let Some(snapshot) = &self.snapshot {
					for work in &snapshot.portfolio.works {
						commands.push(Command {
							label: work.title.clone(),
							detail: "Work".into(),
							action: Action::Work(work.id.clone(), work.owner_agent_id.clone()),
						});
					}
				}
			}
		}
		ranked(commands, &query)
	}

	fn project_commands(&self, create_agent: bool, commands: &mut Vec<Command>) {
		if let Some(snapshot) = &self.snapshot {
			for project in &snapshot.projects {
				commands.push(Command {
					label: project.name.clone(),
					detail: project.directory.clone(),
					action: if create_agent {
						Action::NewAgent(project.id.clone())
					} else {
						Action::Project(project.id.clone())
					},
				});
			}
		}
	}

	fn select_global_command(&mut self, next: usize, commands: &[Command]) {
		self.workspace.navigation.command_index = next;
		self.workspace.navigation.command_scroll.scroll_to_item(display_index(commands, next));
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
					self.reveal_navigation_conversation(&item.id, cx);
				} else {
					self.workspace.navigation.archives_open = true;
					self.workspace.sidebar_visible = true;
					self.prepare_navigation_reveal(&item.id, "archive");
				}
				self.activate_navigation_item(&item, cx);
			},
			Action::Project(id) => self.reveal_navigation_project(&id, cx),
			Action::NewAssistant => {
				self.workspace.navigation.collapsed.remove(Section::Assistant.key());
				self.new_assistant_conversation(cx);
			},
			Action::NewProject => self.add_navigation_project(cx),
			Action::ChooseProject => {
				self.workspace.navigation.command_open = true;
				self.reset_command_search(true, cx);
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
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	pub(crate) fn visual_command_palette(&mut self, page: &str, cx: &mut Context<Self>) {
		self.visual_navigation_hierarchy(false, cx);
		self.initialize_command_search(cx);
		self.reset_command_search(page == "commands-projects", cx);
		self.workspace.navigation.command_open = true;
		self.workspace.navigation.command_session += 1;
		let query = match page {
			"commands-search" => "Decodex",
			"commands-empty" => "qzx-no-match",
			_ => "",
		};
		self.workspace
			.navigation
			.search
			.as_ref()
			.expect("initialized visual search")
			.update(cx, |input, cx| input.set_content(query, cx));
		cx.notify();
	}
}
