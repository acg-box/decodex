//! One suggestion for a newly completed visible turn, using existing history refreshes.
use super::{AgentSurface, Context, EntityId};
use crate::shell::agent_surface::native_timeline::Binding;
use decodex_protocol::AgentTimelineContent;

#[derive(Default)]
pub(super) struct Automatic {
	owner: Option<(Option<EntityId>, Binding)>,
	turn: Option<(String, bool)>,
}

impl Automatic {
	fn observe(
		&mut self,
		owner: (Option<EntityId>, Binding),
		turn: Option<(String, bool)>,
		enabled: bool,
	) -> bool {
		// A cleared active-turn snapshot can arrive before that turn's history page.
		// Keep waiting for its boundary instead of treating an older answer as new.
		if self.owner.as_ref() == Some(&owner)
			&& matches!((&self.turn, &turn), (Some((pending, false)), Some((finished, true))) if pending != finished)
		{
			return false;
		}
		let completed = self.owner.as_ref() == Some(&owner)
			&& turn.as_ref().is_some_and(|(_, complete)| *complete)
			&& self.turn != turn;
		self.owner = Some(owner);
		self.turn = turn;
		completed && enabled
	}
}

impl AgentSurface {
	pub(in super::super) fn observe_prompt_completion(&mut self, cx: &mut Context<Self>) {
		if !self.command_connection_ready() || !self.can_open_sample() {
			return;
		}
		let Some(work) = self.conversation_work().filter(|w| self.native_history_active(w)) else {
			return;
		};
		let Some(binding) = self.timeline.native.binding.clone() else { return };
		let boundary = self.timeline.native.entries.iter().rev().find_map(|entry| {
			if let AgentTimelineContent::TurnBoundary { turn_id, completed, status, .. } =
				&entry.content
			{
				Some((turn_id.clone(), *completed, status.as_deref() == Some("completed")))
			} else {
				None
			}
		});
		let turn = match (&work.active_turn_id, &boundary) {
			(Some(active), _) => Some((active.clone(), false)),
			(None, _) => boundary.as_ref().map(|(id, complete, _)| (id.clone(), *complete)),
		};
		let owner = (self.snapshot.as_ref().and_then(|s| s.runtime_source.clone()), binding);
		if !self.sample.automatic.observe(
			owner,
			turn,
			crate::ui_preferences::automatic_prompt_suggestions(None),
		) {
			return;
		}
		let Some((turn, true, true)) = boundary else { return };
		let has_answer = self.timeline.native.entries.iter().any(|entry| {
			matches!(&entry.content, AgentTimelineContent::Item { turn_id, kind, text, phase, .. }
				if turn_id == &turn && kind == "agentMessage" && !text.trim().is_empty()
					&& phase.as_deref().is_none_or(|phase| phase == "final_answer"))
		});
		if has_answer
			&& self.sample.work.is_none()
			&& self.composer_manager.as_ref().is_none_or(|owner| owner == &work.id)
			&& self.draft_owner_available()
			&& !self.sending
			&& !self.uncertain
			&& self.composer.read(cx).content().is_empty()
			&& !self.composer.read(cx).is_composing()
			&& self.attachments.is_empty()
			&& self.task_references.is_empty()
			&& self.voice.is_none()
			&& self.dictation.is_none()
		{
			self.open_sample_panel(true, false, cx);
		}
	}
}

#[cfg(test)]
mod tests {
	#[test]
	fn disabling_suggestions_consumes_turn_boundaries_without_replaying_them_on_enable() {
		let owner = (
			None,
			super::Binding {
				work: "work".into(),
				thread: "thread".into(),
				account: "account".into(),
			},
		);
		let mut automatic = super::Automatic::default();

		assert!(!automatic.observe(owner.clone(), Some(("first".into(), false)), false));
		assert!(!automatic.observe(owner.clone(), Some(("first".into(), true)), false));
		assert!(!automatic.observe(owner.clone(), Some(("first".into(), true)), true));
		assert!(automatic.observe(owner, Some(("second".into(), true)), true));
	}
}
