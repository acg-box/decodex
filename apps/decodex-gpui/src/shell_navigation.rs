//! Local navigation history. Traversal restores a view; it never replays a command.
use gpui::{
	AppContext as _, InteractiveElement as _, IntoElement as _, KeyDownEvent, ParentElement as _,
	StatefulInteractiveElement as _, Styled as _, prelude::FluentBuilder as _,
};

use decodex_protocol::EntityId;

use crate::{
	settings_surface::SettingsCategory,
	shell::{
		AnyElement, Context, ControlTooltip, Destination, MouseButton, Role, SettingsWindow, Shell,
		agent_surface::AgentLocation, ui_theme::CHROME_CONTROL_SIZE, workspace_symbols,
		workspace_symbols::Symbol,
	},
	ui_motion::SmoothControl as _,
	ui_theme::{self, HOVER_FILL},
};

pub(super) fn mouse_direction(button: MouseButton) -> Option<bool> {
	match button {
		MouseButton::Navigate(gpui::NavigationDirection::Back) => Some(false),
		MouseButton::Navigate(gpui::NavigationDirection::Forward) => Some(true),
		_ => None,
	}
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn forward_mouse_navigation(
	parent: gpui::AnyWindowHandle,
	event: &gpui::MouseDownEvent,
	window: &mut gpui::Window,
	cx: &mut gpui::App,
) -> bool {
	let Some(forward) = mouse_direction(event.button) else { return false };

	window.prevent_default();

	if forward {
		super::forward_shell_action(parent, &super::NavigateForward, cx);
	} else {
		super::forward_shell_action(parent, &super::NavigateBack, cx);
	}

	true
}

pub(super) struct NavigationHistory {
	entries: Vec<Location>,
	cursor: usize,
	restoring: bool,
	focus_pending: bool,
}
impl NavigationHistory {
	pub(super) fn new() -> Self {
		Self::at(Location::Agent(AgentLocation::Root))
	}

	pub(super) fn for_settings(owner: &Shell, cx: &gpui::App) -> Self {
		Self::at(owner.settings_location(cx))
	}

	fn at(location: Location) -> Self {
		Self { entries: vec![location], cursor: 0, restoring: false, focus_pending: false }
	}

	fn record(&mut self, location: Location) {
		if self.restoring || self.entries[self.cursor] == location {
			return;
		}

		self.entries.truncate(self.cursor + 1);
		self.entries.push(location);

		if self.entries.len() > 100 {
			self.entries.remove(0);
		}

		self.cursor = self.entries.len() - 1;
		self.focus_pending = true;
	}

	pub(super) fn repair_focus(&mut self, root: &gpui::FocusHandle, window: &gpui::Window) {
		if !std::mem::take(&mut self.focus_pending) {
			return;
		}

		let root = root.clone();

		// The new frame determines whether the outgoing control is still mounted.
		window.on_next_frame(move |window, cx| {
			if !root.contains_focused(window, cx) {
				window.focus(&root, cx);
			}
		});
	}

	fn neighbor(&self, forward: bool, available: impl Fn(&Location) -> bool) -> Option<usize> {
		let mut index = self.cursor;

		loop {
			index = if forward { index.checked_add(1)? } else { index.checked_sub(1)? };
			let location = self.entries.get(index)?;

			if location != &self.entries[self.cursor] && available(location) {
				return Some(index);
			}
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Location {
	Agent(AgentLocation),
	Conversation(Option<EntityId>),
	Settings(SettingsCategory),
	Destination(Destination),
}

impl Shell {
	pub(super) fn record_navigation(&mut self, cx: &Context<Self>) {
		let location = match self.selected {
			Destination::Agent => Location::Agent(self.agent.read(cx).navigation_location()),
			Destination::Conversations => Location::Conversation(self.quick.selected.clone()),
			Destination::Settings => Location::Settings(self.settings.read(cx).category),
			destination => Location::Destination(destination),
		};

		self.navigation.record(location);
	}

	fn navigation_neighbor(&self, forward: bool, cx: &Context<Self>) -> Option<usize> {
		self.navigation.neighbor(forward, |location| match location {
			Location::Agent(location) => self.agent.read(cx).can_restore_location(location),
			Location::Conversation(Some(id)) =>
				self.quick.tasks.iter().any(|task| &task.conversation_id == id),
			_ => true,
		})
	}

	pub(super) fn navigate_history(
		&mut self,
		forward: bool,
		window: &mut gpui::Window,
		cx: &mut Context<Self>,
	) {
		if self.selected == Destination::Agent && self.agent.read(cx).workspace_connecting() {
			return;
		}

		self.record_navigation(cx);

		let Some(index) = self.navigation_neighbor(forward, cx) else { return };
		let location = self.navigation.entries[index].clone();

		// Reads and observer notifications during restoration cannot create a visit.
		self.navigation.restoring = true;
		let restored = self.restore_navigation(location, cx);
		self.navigation.restoring = false;

		if restored {
			self.navigation.cursor = index;
			// The outgoing surface can detach the control that owned keyboard focus.
			window.focus(&self.root_focus, cx);
		}

		cx.notify();
	}

	fn restore_navigation(&mut self, location: Location, cx: &mut Context<Self>) -> bool {
		match location {
			Location::Agent(location) => {
				if !self.agent.update(cx, |agent, cx| agent.restore_location(&location, cx)) {
					return false;
				}
				self.select_destination(Destination::Agent, cx);
			},
			Location::Conversation(id) => {
				self.creating_new = id.is_none();
				self.deferred_provider_refresh = None;
				self.input_status = None;

				if let Some(id) = id {
					if !self.conversations.select(id) {
						return false;
					}
				} else {
					self.conversations.begin_new();
				}
				self.synchronize_conversations(cx);
				self.select_destination(Destination::Conversations, cx);
			},
			Location::Settings(category) => {
				self.settings.update(cx, |settings, cx| {
					settings.category = category;
					cx.notify();
				});
				self.select_destination(Destination::Settings, cx);
			},
			Location::Destination(destination) => self.select_destination(destination, cx),
		}

		true
	}

	fn settings_location(&self, cx: &gpui::App) -> Location {
		if self.settings_selected == Destination::Settings {
			Location::Settings(self.settings.read(cx).category)
		} else {
			Location::Destination(self.settings_selected)
		}
	}
}

impl SettingsWindow {
	pub(super) fn record_navigation(&mut self, cx: &Context<Self>) {
		self.navigation.record(self.owner.read(cx).settings_location(cx));
	}

	pub(super) fn navigate_history(
		&mut self,
		forward: bool,
		window: &mut gpui::Window,
		cx: &mut Context<Self>,
	) {
		self.record_navigation(cx);
		let Some(index) = self.navigation.neighbor(forward, |_| true) else { return };
		let location = self.navigation.entries[index].clone();

		self.navigation.cursor = index;
		self.owner.update(cx, |shell, cx| {
			let destination = match location {
				Location::Settings(category) => {
					shell.settings.update(cx, |settings, cx| {
						settings.category = category;
						cx.notify();
					});
					Destination::Settings
				},
				Location::Destination(destination) => destination,
				_ => return,
			};
			shell.select_settings_section(destination, cx);
		});

		window.focus(&self.focus, cx);
		cx.notify();
	}
}

impl Shell {
	pub(super) fn navigation_control(&self, forward: bool, cx: &Context<Self>) -> AnyElement {
		let enabled = !(self.selected == Destination::Agent
			&& self.agent.read(cx).workspace_connecting())
			&& self.navigation_neighbor(forward, cx).is_some();
		let label = if forward { "Forward · Command-]" } else { "Back · Command-[" };

		gpui::div()
			.id(if forward { "navigate-forward" } else { "navigate-back" })
			.debug_selector(move || {
				if forward { "navigate-forward" } else { "navigate-back" }.into()
			})
			.role(Role::Button)
			.aria_label(label)
			.when(!enabled, |el| {
				el.aria_label(if forward { "Forward unavailable" } else { "Back unavailable" })
			})
			.when(enabled, |el| el.tab_index(0))
			.tooltip(move |_, cx| cx.new(|_| ControlTooltip(label)).into())
			.size(gpui::px(CHROME_CONTROL_SIZE))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(5.0))
			.occlude()
			.when(!enabled, |el| el.opacity(0.25))
			.when(enabled, |el| el.cursor_pointer().hover(|el| el.bg(ui_theme::rgba(HOVER_FILL))))
			.on_mouse_down(MouseButton::Left, |_, window, cx| {
				window.prevent_default();
				cx.stop_propagation();
			})
			.on_click(cx.listener(move |s, _, window, cx| {
				if enabled {
					s.navigate_history(forward, window, cx);
				}

				cx.stop_propagation();
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, window, cx| {
				if enabled && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.navigate_history(forward, window, cx);
					cx.stop_propagation();
				}
			}))
			.relative()
			.child(self.titlebar.control())
			.child(workspace_symbols::icon(if forward { Symbol::Forward } else { Symbol::Back }))
			.smooth()
			.enabled(enabled)
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use gpui::{
		AnyWindowHandle, Context, FocusHandle, InteractiveElement as _, MouseButton,
		NavigationDirection, Render, Styled as _, TestAppContext, VisualTestContext, Window,
	};

	use crate::shell::{
		Destination,
		navigation::{AgentLocation, Location, NavigationHistory, SettingsCategory},
		tests::open_shell,
	};

	struct NavigationRelay {
		parent: AnyWindowHandle,
		focus: FocusHandle,
	}
	impl Render for NavigationRelay {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl gpui::IntoElement {
			let parent = self.parent;

			gpui::div()
				.size_full()
				.track_focus(&self.focus)
				.capture_any_mouse_down(move |event, window, cx| {
					super::forward_mouse_navigation(parent, event, window, cx);
				})
				.on_action(move |action: &crate::shell::NavigateForward, _, cx| {
					crate::shell::forward_shell_action(parent, action, cx);
				})
		}
	}

	#[test]
	fn new_navigation_replaces_the_forward_branch_and_deduplicates_refreshes() {
		let worker = Location::Agent(super::AgentLocation::Conversation {
			work: Some("worker".into()),
			thread: None,
		});
		let mut history = NavigationHistory::new();

		history.record(worker.clone());
		history.record(worker);
		history.record(Location::Settings(super::SettingsCategory::General));

		assert_eq!(history.entries.len(), 3);

		history.restoring = true;
		history.record(Location::Destination(Destination::Accounts));
		history.restoring = false;

		assert_eq!(history.entries.len(), 3, "restoration cannot create a visit");

		history.cursor = 1;

		history.record(Location::Destination(Destination::Health));

		assert_eq!(history.entries.len(), 3);
		assert_eq!(history.entries[2], Location::Destination(Destination::Health));
		assert_eq!(history.cursor, 2);
		assert_eq!(
			history.neighbor(false, |location| *location == Location::Agent(AgentLocation::Root)),
			Some(0),
			"unavailable visits are skipped without consuming a step"
		);
		assert_eq!(history.cursor, 2);
	}

	#[gpui::test]
	fn navigation_visits_workspace_lists_in_order_without_duplicate_history(
		cx: &mut TestAppContext,
	) {
		let (shell, visual) = open_shell(cx);

		shell.update(visual, |s, cx| {
			s.agent.update(cx, |agent, cx| {
				agent.visual_workspace_fixture(cx);
			});
		});
		let visits = [
			("project-work", AgentLocation::Work { project: None, detail: None }),
			("global-guardian", AgentLocation::Guardian),
			("all-work", AgentLocation::Agents { project: None }),
			("archive-browser", AgentLocation::Archive),
		];

		for (button, location) in &visits {
			visual.update(crate::ui_frame::draw_current_frame);
			let bounds = visual.debug_bounds(button).expect("sidebar navigation");

			visual.simulate_click(bounds.center(), Default::default());
			shell.read_with(visual, |s, cx| {
				assert_eq!(&s.agent.read(cx).navigation_location(), location);
			});
		}
		let count = shell.read_with(visual, |s, _| s.navigation.entries.len());

		assert_eq!(count, 5);

		for location in
			visits[..3].iter().rev().map(|(_, location)| location).chain([&AgentLocation::Root])
		{
			visual.update(crate::ui_frame::draw_current_frame);
			let back = visual.debug_bounds("navigate-back").unwrap();

			visual.simulate_click(back.center(), Default::default());
			shell.read_with(visual, |s, cx| {
				assert_eq!(&s.agent.read(cx).navigation_location(), location);
			});
		}
		for (_, location) in &visits {
			visual.simulate_keystrokes("cmd-]");
			shell.read_with(visual, |s, cx| {
				assert_eq!(&s.agent.read(cx).navigation_location(), location);
				assert_eq!(s.navigation.entries.len(), count);
			});
		}
	}

	#[gpui::test]
	fn settings_navigation_restores_categories_without_changing_workspace_history(
		cx: &mut TestAppContext,
	) {
		let (shell, visual) = open_shell(cx);

		shell.update(visual, |s, cx| s.open_settings_window(Destination::Settings, cx));
		let window = shell.read_with(visual, |s, _| s.settings_window.unwrap());
		let settings = &mut VisualTestContext::from_window(window.into(), visual);

		for section in [
			"settings-section-Appearance",
			"settings-section-Components",
			"settings-section-Accounts",
			"settings-section-Diagnostics",
		] {
			settings.update(crate::ui_frame::draw_current_frame);
			let bounds = settings.debug_bounds(section).unwrap();

			settings.simulate_click(bounds.center(), Default::default());
		}
		for (step, location) in [
			Location::Destination(Destination::Accounts),
			Location::Settings(SettingsCategory::Components),
			Location::Settings(SettingsCategory::Appearance),
			Location::Settings(SettingsCategory::General),
		]
		.into_iter()
		.enumerate()
		{
			if step % 2 == 0 {
				let position = gpui::point(gpui::px(250.), gpui::px(150.));
				let button = MouseButton::Navigate(NavigationDirection::Back);

				settings.simulate_mouse_down(position, button, Default::default());
				settings.simulate_mouse_up(position, button, Default::default());
			} else {
				settings.simulate_keystrokes("cmd-[");
			}
			shell.read_with(settings, |s, cx| {
				assert_eq!(s.settings_location(cx), location);
				assert_eq!(s.selected, Destination::Agent);
				assert_eq!(s.navigation.entries.len(), 1);
			});
		}
		settings.simulate_keystrokes("cmd-]");
		shell.read_with(settings, |s, cx| {
			assert_eq!(s.settings_location(cx), Location::Settings(SettingsCategory::Appearance));
		});
	}

	#[gpui::test]
	fn navigation_restores_a_specific_history_conversation_and_the_new_editor(
		cx: &mut TestAppContext,
	) {
		let (shell, visual) = open_shell(cx);
		let conversations = crate::conversations::tests::native_settings_fixture();
		let selected = conversations.snapshot().selected;

		shell.update(visual, |s, cx| {
			s.ordinary_syncing = true; // Controller fixture; do not bind a production draft store.
			s.conversations = conversations.clone();
			s.quick = conversations.snapshot();
			s.select_destination(Destination::Conversations, cx);
		});
		visual.update(|window, cx| shell.update(cx, |s, cx| s.start_new_conversation(window, cx)));
		visual.simulate_keystrokes("cmd-[");
		shell.read_with(visual, |s, _| {
			assert_eq!(s.quick.selected, selected);
			assert!(!s.creating_new);
		});
		visual.simulate_keystrokes("cmd-]");
		shell.read_with(visual, |s, _| {
			assert!(s.quick.selected.is_none());
			assert!(s.creating_new);
		});
	}

	#[gpui::test]
	fn side_buttons_move_once_per_press_without_activating_the_hovered_control(
		cx: &mut TestAppContext,
	) {
		let (shell, visual) = open_shell(cx);
		shell.update(visual, |s, cx| s.agent.update(cx, |a, cx| a.visual_workspace_fixture(cx)));
		for location in
			[AgentLocation::Work { project: None, detail: None }, AgentLocation::Guardian]
		{
			shell.update(visual, |s, cx| {
				assert!(s.agent.update(cx, |a, cx| a.restore_location(&location, cx)));
			});
		}
		visual.update(crate::ui_frame::draw_current_frame);
		let pointer = visual.debug_bounds("project-work").unwrap().center();

		for _ in 0..3 {
			for (direction, expected) in [
				(NavigationDirection::Back, AgentLocation::Work { project: None, detail: None }),
				(NavigationDirection::Forward, AgentLocation::Guardian),
			] {
				let button = MouseButton::Navigate(direction);

				visual.simulate_mouse_down(pointer, button, Default::default());
				visual.simulate_mouse_up(pointer, button, Default::default());
				shell.read_with(visual, |s, cx| {
					assert_eq!(s.agent.read(cx).navigation_location(), expected);
					assert_eq!(s.navigation.entries.len(), 3);
				});
			}
		}
	}

	#[gpui::test]
	fn attached_controls_forward_side_buttons_and_keyboard_to_the_workspace(
		cx: &mut TestAppContext,
	) {
		let (shell, visual) = open_shell(cx);
		shell.update(visual, |s, cx| s.agent.update(cx, |a, cx| a.visual_workspace_fixture(cx)));
		shell.update(visual, |s, cx| {
			assert!(s.agent.update(cx, |a, cx| a.restore_location(&AgentLocation::Guardian, cx)));
		});
		let parent = visual.update(|window, _| window.window_handle());
		let (_, child) = visual.add_window_view(|window, cx| {
			let focus = cx.focus_handle();

			window.focus(&focus, cx);
			NavigationRelay { parent, focus }
		});
		child.update(crate::ui_frame::draw_current_frame);
		let pointer = gpui::point(gpui::px(100.), gpui::px(100.));
		let button = MouseButton::Navigate(NavigationDirection::Back);

		child.simulate_mouse_down(pointer, button, Default::default());
		child.simulate_mouse_up(pointer, button, Default::default());
		shell.read_with(child, |s, cx| {
			assert_eq!(s.agent.read(cx).navigation_location(), AgentLocation::Root)
		});
		child.simulate_keystrokes("cmd-]");
		shell.read_with(child, |s, cx| {
			assert_eq!(s.agent.read(cx).navigation_location(), AgentLocation::Guardian);
			assert_eq!(s.navigation.entries.len(), 2);
		});
	}
}
