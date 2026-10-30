//! Restore visited views through their existing read and draft owners.

use crate::shell::agent_surface::{AgentSurface, Context};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum AgentLocation {
	#[default]
	Root,
	Conversation {
		work: Option<String>,
		thread: Option<String>,
	},
	Agents {
		project: Option<String>,
	},
	Work {
		project: Option<String>,
		detail: Option<String>,
	},
	Guardian,
	Archive,
}

impl AgentSurface {
	pub(crate) fn navigation_location(&self) -> AgentLocation {
		if self.workspace.browsing {
			return if self.workspace.archives.open {
				AgentLocation::Archive
			} else if self.workspace.guardian_open {
				AgentLocation::Guardian
			} else if self.workspace.portfolio_mode {
				AgentLocation::Work {
					project: self.workspace.workspace_filter.clone(),
					detail: self.workspace.work_detail.clone(),
				}
			} else {
				AgentLocation::Agents { project: self.workspace.workspace_filter.clone() }
			};
		}

		let work = self.selected.clone().filter(|id| Some(id) != self.root_id().as_ref());
		// A default native binding can arrive after the page opens. Only an explicit
		// alternate thread changes its identity; loading must not add a history step.
		let thread = self.native_agents.selected.as_ref().and_then(|(owner, thread)| {
			let default = self
				.snapshot
				.as_ref()?
				.work_items
				.iter()
				.find(|work| &work.id == owner)?
				.codex_thread_id
				.as_ref();

			(default != Some(thread)).then(|| thread.clone())
		});

		if work.is_none() && thread.is_none() {
			AgentLocation::Root
		} else {
			AgentLocation::Conversation { work, thread }
		}
	}

	pub(crate) fn can_restore_location(&self, location: &AgentLocation) -> bool {
		match location {
			AgentLocation::Conversation { work, thread: Some(thread) } => work
				.clone()
				.or_else(|| self.root_id())
				.is_some_and(|owner| self.can_open_native_agent(&owner, thread)),
			AgentLocation::Conversation { work: Some(id), .. } =>
				self.workspace.new_conversation.as_ref() == Some(id)
					&& !self.sending
					&& !self.uncertain
					|| self.conversation_record(id).is_some()
					|| self.workspace.archives.archived_work(id).is_some(),
			_ => true,
		}
	}

	pub(crate) fn restore_location(
		&mut self,
		location: &AgentLocation,
		cx: &mut Context<Self>,
	) -> bool {
		if !self.can_restore_location(location) {
			return false;
		}

		match location {
			AgentLocation::Root | AgentLocation::Conversation { .. } => {
				let (work, thread) = match location {
					AgentLocation::Conversation { work, thread } =>
						(work.clone(), thread.as_deref()),
					_ => (None, None),
				};
				let Some(work) = work.or_else(|| self.root_id()) else {
					self.workspace.browsing = false;
					cx.notify();

					return true;
				};

				if let Some(thread) = thread {
					self.open_native_agent(&work, thread, cx);
				} else if self.workspace.new_conversation.as_ref() == Some(&work) {
					self.new_work_conversation(cx);
				} else if !self
					.snapshot
					.as_ref()
					.is_some_and(|snapshot| snapshot.work_items.iter().any(|item| item.id == work))
					&& self.workspace.archives.archived_work(&work).is_some()
				{
					self.open_archived_conversation(&work, cx);
				} else {
					self.open_work_conversation(&work, thread, cx);
				}
			},
			AgentLocation::Archive => self.open_archive_catalog(cx),
			AgentLocation::Agents { project } | AgentLocation::Work { project, .. } => {
				self.cancel_dictation(cx);
				self.workspace.browsing = true;
				self.workspace.archives.open = false;
				self.workspace.guardian_open = false;
				self.workspace.portfolio_mode = matches!(location, AgentLocation::Work { .. });
				self.workspace.workspace_filter = project.clone();

				if let AgentLocation::Work { detail, .. } = location {
					self.workspace.work_detail = detail.clone();

					if let Some(detail) = detail {
						self.load_work_detail(detail.clone(), cx);
					}
				}
			},
			AgentLocation::Guardian => {
				self.workspace.browsing = true;
				self.workspace.archives.open = false;
				self.workspace.guardian_open = true;
			},
		}

		cx.notify();

		true
	}
}

#[cfg(test)]
mod tests {
	use gpui::{AppContext as _, Focusable as _, TestAppContext};

	use super::{AgentLocation, AgentSurface};

	#[gpui::test]
	fn opening_a_conversation_releases_detached_list_focus_for_navigation(cx: &mut TestAppContext) {
		let (shell, visual) = crate::shell::tests::open_shell(cx);

		shell.update(visual, |s, cx| {
			s.agent.update(cx, |a, cx| {
				a.visual_workspace_fixture(cx);
				a.restore_location(&AgentLocation::Agents { project: None }, cx);
			});
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let search =
			shell.read_with(visual, |s, cx| s.agent.read(cx).work_search.read(cx).focus_handle(cx));

		visual.update(|window, cx| window.focus(&search, cx));
		shell.update(visual, |s, cx| s.agent.update(cx, |a, cx| a.open_page("verify", cx)));
		visual.update(crate::ui_frame::draw_current_frame);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		visual.simulate_keystrokes("cmd-[");
		shell.read_with(visual, |s, cx| {
			assert_eq!(
				s.agent.read(cx).navigation_location(),
				AgentLocation::Agents { project: None }
			);
		});

		visual.update(crate::ui_frame::draw_current_frame);
		visual.update(|window, cx| window.focus(&search, cx));
		shell.update(visual, |s, cx| {
			s.agent.update(cx, |a, cx| {
				a.restore_location(&AgentLocation::Agents { project: Some("work".into()) }, cx);
			});
		});
		visual.update(crate::ui_frame::draw_current_frame);
		visual.update(crate::ui_frame::draw_frame);
		visual.update(|window, _| {
			assert!(search.is_focused(window), "a mounted search field must keep its focus");
		});
	}

	#[gpui::test]
	fn navigation_keeps_thread_identity_independent_of_native_page_aliases(
		cx: &mut TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let root = s.root_id().unwrap();
			s.native_agents.selected = Some((root.clone(), "child-thread".into()));
			let location = s.navigation_location();

			assert_eq!(
				location,
				AgentLocation::Conversation { work: None, thread: Some("child-thread".into()) }
			);
			s.native_agents.pages.clear();
			assert!(
				!s.can_restore_location(&location),
				"an unbound thread must not claim a restorable destination"
			);

			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| work.id == root)
				.unwrap()
				.codex_thread_id = Some("child-thread".into());
			assert_eq!(
				s.navigation_location(),
				AgentLocation::Root,
				"default binding is not another visit"
			);
		});
	}

	#[gpui::test]
	fn navigation_restores_an_unsent_new_agent_without_recreating_it(cx: &mut TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.new_work_conversation(cx);
			s.composer.update(cx, |input, cx| input.set_content("Unsent new agent draft", cx));
			let draft = s.navigation_location();
			let identity = s.workspace.new_conversation.clone();

			assert!(s.restore_location(&AgentLocation::Root, cx));
			assert!(s.restore_location(&draft, cx));
			assert_eq!(s.workspace.new_conversation, identity);
			assert_eq!(s.navigation_location(), draft);
			assert_eq!(s.composer.read(cx).content(), "Unsent new agent draft");
			assert!(!s.sending && !s.uncertain);
		});
	}
}
