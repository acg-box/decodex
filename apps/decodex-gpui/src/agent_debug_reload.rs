//! Reload presentation reads without replacing drafts or restarting runtime work.
use super::{AgentSurface, Context, LoadState};

impl AgentSurface {
	pub(crate) fn debug_reload(&mut self, cx: &mut Context<Self>) -> bool {
		if self.voice.is_some()
			|| self.voice_task.is_some()
			|| self.dictation.is_some()
			|| self.dictation_task.is_some()
			|| self.sending
			|| self.submission.command.is_some()
		{
			self.feedback =
				"Finish voice input or the pending send before reloading the workspace.".into();
			cx.notify();
			return false;
		}
		self.generation += 1;
		self.task = None;
		self.state = LoadState::Idle;
		// Keep the workspace frame and native composer mounted while data reloads.
		// Clearing this snapshot briefly replaces the entire page with a loading screen.
		self.history_task = None;
		self.timeline.cache.clear();
		self.timeline.read_at = None;
		self.timeline.loading_older = false;
		self.older_task = None;
		self.timeline.older_scroll_anchor = None;
		self.cancel_native_scroll_anchor();
		self.timeline.native.invalidate_read();
		self.debug_reload_graph();
		self.clear_activity_detail();
		self.reset_resources();
		self.reset_integrations();
		cx.notify();
		true
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::AppContext as _;

	#[gpui::test]
	fn reload_preserves_draft_selection_and_panel_preferences(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let selected = s.selected.clone();
			let composer = s.composer.clone();
			composer.update(cx, |input, cx| input.set_content("Unsent draft", cx));
			let graph = s.workspace.graph_visible;
			let snapshot = s.snapshot.clone();
			let panels = s.workspace_panels();
			let history = s.history.clone();
			let selected_id = selected.clone().unwrap();
			let scroll = gpui::ScrollHandle::new();
			scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-240.)));
			s.timeline.scroll.insert(selected_id.clone(), scroll.clone());
			s.timeline.follow_paused.insert(selected_id.clone());
			assert!(s.debug_reload(cx));
			assert_eq!(s.snapshot, snapshot, "reload must retain the workspace frame");
			assert_eq!(s.workspace_panels(), panels);
			assert!(!s.workspace_connecting());
			assert_eq!(s.history, history);
			assert_eq!(s.timeline.scroll[&selected_id].offset(), scroll.offset());
			assert!(s.timeline.follow_paused.contains(&selected_id));
			assert!(s.timeline.read_at.is_none());
			assert_eq!(s.selected, selected);
			assert_eq!(s.composer, composer);
			assert_eq!(s.composer.read(cx).content(), "Unsent draft");
			assert_eq!(s.workspace.graph_visible, graph);
			s.sending = true;
			assert!(!s.debug_reload(cx));
			assert!(s.sending);
		});
	}
}

#[cfg(test)]
mod dispatch_tests {
	use crate::{
		client_lifecycle::ConnectionView,
		shell::{self, Shell},
	};
	#[gpui::test]
	fn debug_reload_action_reaches_the_dispatching_window(cx: &mut gpui::TestAppContext) {
		cx.update(shell::bind_keys);
		let (shell, visual) =
			cx.add_window_view(|window, cx| Shell::new(window, cx, ConnectionView::Stopped));
		let generation = shell.update(visual, |s, cx| {
			s.agent.update(cx, |a, cx| {
				a.visual_workspace_fixture(cx);
				a.generation
			})
		});
		visual.dispatch_action(shell::ReloadWorkspace);
		shell.update(visual, |s, cx| assert!(s.agent.read(cx).generation > generation));
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("loading-feedback-Opening workspace").is_none());
		assert!(visual.debug_bounds("loading-feedback-Loading conversation").is_none());
	}
}
