//! The list shortcut reuses source-bound review and native branch confirmation.
use decodex_protocol::{AgentTimelineContent, AgentTimelineEntry};

use super::{AgentSurface, Context};

impl AgentSurface {
	pub(in crate::shell::agent_surface) fn review_provider_branch(
		&mut self,
		cx: &mut Context<Self>,
	) {
		let Some(providers) = self.selected_provider_mismatch() else { return };
		let Some(work) = self.selected.clone() else { return };
		self.review_list_branch(&work, cx);
		if self.prompt_edit.work == work {
			self.prompt_edit.branch_provider = Some(providers);
			cx.notify();
		}
	}

	pub(in crate::shell::agent_surface) fn review_list_branch(
		&mut self,
		work: &str,
		cx: &mut Context<Self>,
	) {
		let Some(thread) = self.snapshot.as_ref().and_then(|snapshot| {
			snapshot.work_items.iter().find(|item| item.id == work)?.codex_thread_id.clone()
		}) else {
			return;
		};
		if !self.command_connection_ready() {
			return;
		}
		let Some(profile) = self.profile.clone() else { return };

		self.open_page(work, cx);
		if self.selected.as_deref() != Some(work) {
			return;
		}
		self.begin_prompt_review(&profile, work, &thread);
		self.prompt_edit.branch_choice = Some(decodex_protocol::PromptForkBoundary::AfterTurn);
		self.prompt_edit.list_branch_pending = true;
		self.prompt_edit.feedback = "Reading the latest completed turn…".into();
		self.refresh_open_native_history(cx);
		if self.timeline.native.task.is_none() {
			self.resume_list_branch_review(true, cx);
		}
		cx.notify();
	}

	pub(in crate::shell::agent_surface) fn resume_list_branch_review(
		&mut self,
		page_available: bool,
		cx: &mut Context<Self>,
	) {
		if !self.prompt_edit.list_branch_pending {
			return;
		}
		if !self.prompt_editor_source_current() {
			self.prompt_edit.list_branch_pending = false;
			return;
		}
		let work = self.prompt_edit.work.clone();
		let thread = self.prompt_edit.thread.clone();
		let native = page_available
			&& self.conversation_work().is_some_and(|work| self.native_history_active(&work));
		if native {
			if let Some((turn, item)) = latest_completed_input(&self.timeline.native.entries) {
				let (turn, item) = (turn.to_owned(), item.to_owned());
				self.prompt_edit.list_branch_pending = false;
				self.request_prompt_review(
					(&work, &thread, &turn, &item),
					None,
					self.prompt_edit.branch_choice,
					cx,
				);
				return;
			}
			if self.timeline.native.older_cursor.is_some() {
				self.load_native_timeline(&work, &thread, true, cx);
				return;
			}
		}
		self.prompt_edit.list_branch_pending = false;
		self.prompt_edit.feedback =
			"A completed turn with full native history is required. Refresh history and try again."
				.into();
		cx.notify();
	}
}

fn latest_completed_input(entries: &[AgentTimelineEntry]) -> Option<(&str, &str)> {
	let turn = entries.iter().rev().find_map(|entry| match &entry.content {
		AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } => Some(turn_id),
		_ => None,
	})?;
	entries.iter().rev().find_map(|entry| match &entry.content {
		AgentTimelineContent::Item { turn_id, item_id, kind, .. }
			if turn_id == turn && kind == "userMessage" =>
			Some((turn_id.as_str(), item_id.as_str())),
		_ => None,
	})
}

#[cfg(test)]
mod tests {
	use super::{AgentSurface, AgentTimelineContent, AgentTimelineEntry, latest_completed_input};
	use crate::shell::agent_surface::{
		drafts::tests::profiles, prompt_edit::tests::install_fixture_editor,
	};
	use gpui::AppContext as _;

	fn boundary(turn: &str, completed: bool) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position: 1,
			content: AgentTimelineContent::TurnBoundary {
				turn_id: turn.into(),
				completed,
				status: None,
				duration_ms: None,
				usage: None,
				timing: None,
				usage_summary: None,
				error: None,
			},
		}
	}
	fn input(turn: &str, item: &str) -> AgentTimelineEntry {
		AgentTimelineEntry {
			position: 1,
			content: AgentTimelineContent::Item {
				turn_id: turn.into(),
				item_id: item.into(),
				kind: "userMessage".into(),
				phase: None,
				text: "Original input".into(),
				truncated: false,
				activity: None,
				app_ui: false,
				attachments: vec![],
				collaboration: None,
			},
		}
	}

	#[test]
	fn list_branch_selects_the_latest_completed_turn_not_live_input() {
		let entries = vec![
			input("old", "one"),
			boundary("old", true),
			input("complete", "two"),
			boundary("complete", true),
			boundary("live", false),
			input("live", "three"),
		];
		assert_eq!(latest_completed_input(&entries), Some(("complete", "two")));
	}

	#[test]
	fn list_branch_requires_the_completed_turns_own_input() {
		assert_eq!(latest_completed_input(&[input("old", "one"), boundary("new", true)]), None);
		assert_eq!(latest_completed_input(&[input("live", "one"), boundary("live", false)]), None);
	}

	#[gpui::test]
	fn list_branch_entry_preserves_drafts_and_cancellation(cx: &mut gpui::TestAppContext) {
		let (_directory, profile, _) = profiles();
		let surface = cx.new(AgentSurface::new);
		install_fixture_editor(&surface, profile, cx);
		surface.update(cx, |s, cx| {
			let original = s.selected.clone().unwrap();
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "verify")
				.unwrap()
				.codex_thread_id = Some("verify-thread".into());
			s.review_list_branch("verify", cx);
			assert_eq!(s.selected.as_deref(), Some("verify"));
			assert_eq!(s.prompt_edit.work, "verify");
			assert_eq!(s.prompt_edit.thread, "verify-thread");
			assert!(s.prompt_edit.branch_choice.is_some());
			assert!(s.prompt_edit.draft.is_none());
			s.close_prompt_editor(cx);
			assert!(s.prompt_edit.work.is_empty());
			s.open_page(&original, cx);
			assert_eq!(s.composer.read(cx).content(), "Unrelated unsent input");
		});
	}

	#[gpui::test]
	fn list_branch_rejects_changed_source_and_unmanaged_rows(cx: &mut gpui::TestAppContext) {
		let (_directory, profile, _) = profiles();
		let surface = cx.new(AgentSurface::new);
		install_fixture_editor(&surface, profile.clone(), cx);
		surface.update(cx, |s, cx| {
			let work = s.selected.clone().unwrap();
			s.review_list_branch("unmanaged-thread", cx);
			assert_eq!(s.selected.as_deref(), Some(work.as_str()));
			s.begin_prompt_review(&profile, &work, "thread");
			s.prompt_edit.list_branch_pending = true;
			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == work)
				.unwrap()
				.codex_thread_id = Some("replacement-thread".into());
			s.resume_list_branch_review(true, cx);
			assert!(!s.prompt_edit.list_branch_pending);
			assert!(s.prompt_edit.draft.is_none());
		});
	}
}
