//! Local navigation history. Traversal restores a view; it never replays a command.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
struct Location {
	destination: Destination,
	work: Option<String>,
}

pub(super) struct NavigationHistory {
	entries: Vec<Location>,
	cursor: usize,
}

impl NavigationHistory {
	pub(super) fn new() -> Self {
		Self { entries: vec![Location { destination: Destination::Chief, work: None }], cursor: 0 }
	}

	fn record(&mut self, location: Location) {
		if self.entries[self.cursor] == location {
			return;
		}
		self.entries.truncate(self.cursor + 1);
		self.entries.push(location);
		if self.entries.len() > 100 {
			self.entries.remove(0);
		}
		self.cursor = self.entries.len() - 1;
	}
}

impl Shell {
	pub(super) fn record_navigation(&mut self, cx: &Context<Self>) {
		self.navigation.record(Location {
			destination: self.selected,
			work: if self.selected == Destination::Chief {
				self.chief.read(cx).navigation_work()
			} else {
				None
			},
		});
	}

	fn navigation_neighbor(&self, forward: bool, cx: &Context<Self>) -> Option<usize> {
		let mut index = self.navigation.cursor;
		loop {
			index = if forward { index.checked_add(1)? } else { index.checked_sub(1)? };
			let location = self.navigation.entries.get(index)?;
			if location.destination != Destination::Chief
				|| self.chief.read(cx).can_restore_work(location.work.as_deref())
			{
				return Some(index);
			}
		}
	}

	pub(super) fn navigate_history(&mut self, forward: bool, cx: &mut Context<Self>) {
		self.record_navigation(cx);
		let Some(index) = self.navigation_neighbor(forward, cx) else {
			return;
		};
		let location = self.navigation.entries[index].clone();
		self.navigation.cursor = index;
		if location.destination == Destination::Chief {
			self.chief.update(cx, |chief, cx| chief.restore_work(location.work.as_deref(), cx));
		}
		self.select_destination(location.destination, cx);
		cx.notify();
	}

	pub(super) fn navigation_control(&self, forward: bool, cx: &Context<Self>) -> AnyElement {
		let enabled = self.navigation_neighbor(forward, cx).is_some();
		let label = if forward { "Forward · Command-]" } else { "Back · Command-[" };
		div()
			.id(if forward { "navigate-forward" } else { "navigate-back" })
			.role(Role::Button)
			.aria_label(label)
			.when(!enabled, |el| {
				el.aria_label(if forward { "Forward unavailable" } else { "Back unavailable" })
			})
			.when(enabled, |el| el.tab_index(0))
			.tooltip(move |_, cx| cx.new(|_| ControlTooltip(label)).into())
			.size(px(ui_theme::CHROME_CONTROL_SIZE))
			.flex_none()
			.flex()
			.items_center()
			.justify_center()
			.rounded(px(5.0))
			.occlude()
			.when(!enabled, |el| el.opacity(0.25))
			.when(enabled, |el| el.cursor_pointer().hover(|el| el.bg(rgba(0xffffff0c))))
			.on_mouse_down(MouseButton::Left, |_, window, cx| {
				window.prevent_default();
				cx.stop_propagation();
			})
			.on_click(cx.listener(move |s, _, _, cx| {
				if enabled {
					s.navigate_history(forward, cx);
				}
				cx.stop_propagation();
			}))
			.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
				if enabled && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.navigate_history(forward, cx);
					cx.stop_propagation();
				}
			}))
			.child(workspace_symbols::icon(if forward {
				workspace_symbols::Symbol::Forward
			} else {
				workspace_symbols::Symbol::Back
			}))
			.smooth()
			.enabled(enabled)
			.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn new_navigation_replaces_the_forward_branch_and_deduplicates_refreshes() {
		let mut history = NavigationHistory::new();
		let worker = Location { destination: Destination::Chief, work: Some("worker".into()) };
		history.record(worker.clone());
		history.record(worker);
		history.record(Location { destination: Destination::Settings, work: None });
		assert_eq!(history.entries.len(), 3);
		history.cursor = 1;
		history.record(Location { destination: Destination::Health, work: None });
		assert_eq!(history.entries.len(), 3);
		assert_eq!(history.entries[2].destination, Destination::Health);
		assert_eq!(history.cursor, 2);
	}
}
