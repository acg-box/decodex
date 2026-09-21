//! Explicit current-turn reviewer changes, separate from future defaults and pending approvals.
use super::{mcp_forms::mcp_button, *};
use decodex_protocol::{ChiefAppReviewer as Reviewer, ChiefLiveReviewerState as State};

#[derive(Default)]
pub(super) struct Panel {
	state: Option<State>,
	work: Option<String>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
}

impl ChiefSurface {
	pub(super) fn invalidate_live_reviewer_for_snapshot(&mut self, next: &ChiefSnapshotDto) {
		let Some(work) = self.live_reviewer.work.as_ref() else {
			return;
		};
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);
		let unchanged = matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id && a.active_turn_id==b.active_turn_id && b.dispatch_state==ChiefDispatchStateDto::Running);
		if !unchanged {
			self.reset_live_reviewer();
		}
	}

	pub(super) fn reset_live_reviewer(&mut self) {
		self.live_reviewer =
			Panel { epoch: self.live_reviewer.epoch.wrapping_add(1), ..Default::default() };
	}

	fn update_live_reviewer(
		&mut self,
		work: String,
		turn: String,
		reviewer: Option<Reviewer>,
		cx: &mut Context<Self>,
	) {
		if self.live_reviewer.task.is_some() || self.selected.as_ref() != Some(&work) {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Some(source) = self.snapshot.as_ref().and_then(|s| s.runtime_source.clone()) else {
			return;
		};
		let Some(thread) = self
			.snapshot
			.as_ref()
			.and_then(|s| {
				s.work_items.iter().find(|w| {
					w.id == work
						&& w.active_turn_id.as_deref() == Some(&turn)
						&& w.dispatch_state == ChiefDispatchStateDto::Running
				})
			})
			.and_then(|w| w.codex_thread_id.clone())
		else {
			return;
		};
		let Ok(work_id) = EntityId::new(work.clone()) else {
			return;
		};
		let action = if let Some(reviewer) = reviewer {
			let Some(State::Available {
				thread_id, turn_id, review_token, can_update: true, ..
			}) = &self.live_reviewer.state
			else {
				return;
			};
			if self.live_reviewer.work.as_ref() != Some(&work)
				|| turn_id.as_str() != turn
				|| thread_id.as_str() != thread
			{
				return;
			}
			Some(ChiefActionDto::SetLiveReviewer {
				work_id: work_id.clone(),
				turn_id: turn_id.clone(),
				review_token: review_token.clone(),
				reviewer,
			})
		} else {
			None
		};
		let saving = action.is_some();
		let generation = self.generation;
		self.live_reviewer.epoch = self.live_reviewer.epoch.wrapping_add(1);
		let epoch = self.live_reviewer.epoch;
		self.live_reviewer.work = Some(work.clone());
		self.live_reviewer.state = None;
		self.live_reviewer.feedback = if saving {
			"Updating current-turn reviewer…"
		} else {
			"Reading current-turn operation state…"
		}
		.into();
		let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state =
				runtime.block_on(client.live_reviewer(work_id)).unwrap_or(State::Unavailable);
			Some((outcome, state))
		});
		self.live_reviewer.task=Some(cx.spawn(async move |surface,cx| {
			let result=future.await;
			let _=surface.update(cx,|s,cx| {
				if s.generation!=generation||s.live_reviewer.epoch!=epoch {return;}
				s.live_reviewer.task=None;
				let current=s.snapshot.as_ref().is_some_and(|snapshot| snapshot.runtime_source.as_ref()==Some(&source)
					&& snapshot.work_items.iter().any(|w| w.id==work && w.codex_thread_id.as_deref()==Some(&thread) && w.active_turn_id.as_deref()==Some(&turn) && w.dispatch_state==ChiefDispatchStateDto::Running));
				if !current||s.selected.as_ref()!=Some(&work) {s.reset_live_reviewer();cx.notify();return;}
				let (outcome,state)=result.unwrap_or((None,State::Unavailable));
				s.live_reviewer.feedback=match outcome {
					Some(Ok(ChiefCommandResponse::Accepted {..}))=>"Published for subsequent approval requests in this turn. Pending requests and future defaults are unchanged.",
					Some(Ok(ChiefCommandResponse::Rejected {..}))=>"The edit was not accepted. Refresh and review the current turn.",
					Some(_)=>"Publication could not be confirmed. No automatic retry was made.",
					None if saving=>"The operation could not be confirmed. Refresh its receipt before another edit.",
					None if matches!(state,State::Unavailable)=>"No editable active turn is available. Refresh the task.",
					None=>"Choose how new approval requests in this turn are reviewed. Account and managed policies still apply.",
				}.into();
				s.live_reviewer.state=Some(match state {State::Available {ref thread_id,ref turn_id,..} if thread_id.as_str()!=thread || turn_id.as_str()!=turn=>State::Unavailable,other=>other});
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn live_reviewer_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let Some(turn) = work
			.active_turn_id
			.as_ref()
			.filter(|_| work.dispatch_state == ChiefDispatchStateDto::Running)
		else {
			return div().into_any_element();
		};
		let (owner, target) = (work.id.clone(), turn.clone());
		let mut panel =
			div().flex().flex_col().gap_2().child("Current-turn approval reviewer").child(
				mcp_button(
					"live-reviewer-read".into(),
					"Review current-turn settings".into(),
					false,
					cx,
					move |s, cx| s.update_live_reviewer(owner.clone(), target.clone(), None, cx),
				),
			);
		if self.live_reviewer.work.as_ref() == Some(&work.id) {
			panel = panel.child(self.live_reviewer.feedback.clone());
			if let Some(State::Available {
				turn_id, can_update, last_reviewer, last_outcome, ..
			}) = &self.live_reviewer.state
				&& turn_id.as_str() == turn
			{
				if let (Some(reviewer), Some(outcome)) = (last_reviewer, last_outcome) {
					panel = panel.child(format!(
						"Last local request: {} — {}",
						match reviewer {
							Reviewer::User => "User review",
							Reviewer::AutoReview => "Automatic review",
						},
						outcome_label(*outcome)
					));
				}
				if *can_update {
					for (id, label, reviewer) in [
						("live-reviewer-user", "Ask me", Reviewer::User),
						("live-reviewer-auto", "Automatic review", Reviewer::AutoReview),
					] {
						let (owner, target) = (work.id.clone(), turn.clone());
						panel = panel.child(mcp_button(
							id.into(),
							label.into(),
							false,
							cx,
							move |s, cx| {
								s.update_live_reviewer(
									owner.clone(),
									target.clone(),
									Some(reviewer),
									cx,
								)
							},
						));
					}
				}
			}
		}
		panel.into_any_element()
	}
}

fn outcome_label(outcome: decodex_protocol::ChiefLiveReviewerOutcome) -> &'static str {
	use decodex_protocol::ChiefLiveReviewerOutcome as O;
	match outcome {
		O::Reserved => "awaiting confirmation",
		O::Applied => "published",
		O::TargetUnavailable => "turn no longer active",
		O::Rejected => "rejected",
		O::Unknown => "unconfirmed",
	}
}

#[cfg(test)]
#[path = "chief_live_settings_wire_tests.rs"]
mod wire_tests;
