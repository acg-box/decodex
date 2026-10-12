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
		self.history = None;
		self.timeline.cache.clear();
		self.timeline.read_at = None;
		self.timeline.navigation = None;
		self.timeline.latest_follow_work = None;
		self.timeline.older_scroll_anchor = None;
		self.timeline.loading_older = false;
		self.timeline.older_history.clear();
		self.older_task = None;
		let epoch = self.timeline.native.epoch.wrapping_add(1);
		self.timeline.native = Default::default();
		self.timeline.native.epoch = epoch;
		self.timeline.scroll.clear();
		self.timeline.follow_paused.clear();
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
			assert!(s.debug_reload(cx));
			assert_eq!(s.snapshot, snapshot, "reload must retain the workspace frame");
			assert_eq!(s.workspace_panels(), panels);
			assert!(!s.workspace_connecting());
			assert!(s.history.is_none());
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
