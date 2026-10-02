//! Explicit current-turn settings, separate from future defaults and pending approvals.
use gpui::AnyElement;
use tokio::runtime::Builder;

use crate::shell::agent_surface::{
	self, AgentActionDto, AgentClient, AgentCommandResponse, AgentDispatchStateDto,
	AgentSnapshotDto, AgentSurface, AgentWorkItemDto, Context, EntityId, IdempotencyKey,
	InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled, Task,
	WireText, mcp_forms, model_settings, px,
};
#[cfg(test)]
use crate::shell::agent_surface::{
	AgentSnapshotResult, AgentWorkStatusDto, ClientProfile, Entity, Render, Window,
};
use decodex_protocol::{
	AgentLiveReviewerOutcome, AgentLiveReviewerOutcome as O, AgentLiveReviewerState as State,
	AgentModelDto, AgentReviewer as Reviewer,
};

#[derive(Default)]
pub(super) struct Panel {
	state: Option<State>,
	work: Option<String>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
	reviewed: bool,
	model_draft: Option<decodex_protocol::AgentLiveModelSelection>,
	choosing_model: bool,
}

enum Edit {
	Reviewer(Reviewer),
	Model(decodex_protocol::AgentLiveModelSelection),
}
impl Edit {
	fn action(
		self,
		work: &EntityId,
		turn: &EntityId,
		review: &WireText,
		models: Option<&[AgentModelDto]>,
	) -> Option<AgentActionDto> {
		Some(match self {
			Self::Reviewer(reviewer) => AgentActionDto::SetLiveReviewer {
				work_id: work.clone(),
				turn_id: turn.clone(),
				review_token: review.clone(),
				reviewer,
			},
			Self::Model(selection) => {
				if !models.is_some_and(|models| {
					models.iter().any(|m| {
						m.model == selection.model && m.efforts.contains(&selection.effort)
					})
				}) {
					return None;
				}

				AgentActionDto::SetLiveModel {
					work_id: work.clone(),
					turn_id: turn.clone(),
					review_token: review.clone(),
					model: selection.model,
					effort: selection.effort,
				}
			},
		})
	}
}

impl AgentSurface {
	pub(super) fn invalidate_live_reviewer_for_snapshot(&mut self, next: &AgentSnapshotDto) {
		let Some(work) = self.live_reviewer.work.as_ref() else {
			return;
		};
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);
		let same_source = self.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref())
			== next.runtime_source.as_ref();
		let unchanged = same_source
			&& matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id && a.active_turn_id==b.active_turn_id && b.dispatch_state==AgentDispatchStateDto::Running);

		if !unchanged {
			self.reset_live_reviewer();
		}
	}

	pub(super) fn reset_live_reviewer(&mut self) {
		self.live_reviewer =
			Panel { epoch: self.live_reviewer.epoch.wrapping_add(1), ..Default::default() };
	}

	fn update_live_settings(
		&mut self,
		work: String,
		turn: String,
		edit: Option<Edit>,
		cx: &mut Context<Self>,
	) {
		if self.live_reviewer.task.is_some()
			|| self.selected.as_ref() != Some(&work)
			|| !self.command_connection_ready()
			|| self.native_agents.selected.is_some()
			|| (edit.is_some() && !self.live_reviewer.reviewed)
		{
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
						&& w.dispatch_state == AgentDispatchStateDto::Running
				})
			})
			.and_then(|w| w.codex_thread_id.clone())
		else {
			return;
		};
		let Ok(work_id) = EntityId::new(work.clone()) else {
			return;
		};
		let action = if let Some(edit) = edit {
			let Some(State::Available {
				thread_id,
				turn_id,
				review_token,
				can_update: true,
				model_choices,
				..
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

			let Some(action) =
				edit.action(&work_id, turn_id, review_token, model_choices.as_deref())
			else {
				return;
			};

			Some(action)
		} else {
			None
		};
		let saving = action.is_some();

		self.live_reviewer.epoch = self.live_reviewer.epoch.wrapping_add(1);

		let epoch = self.live_reviewer.epoch;

		self.live_reviewer.work = Some(work.clone());
		self.live_reviewer.state = None;
		self.live_reviewer.reviewed = false;
		self.live_reviewer.feedback = if saving {
			"Updating current-turn settings…"
		} else {
			"Reading current-turn operation state…"
		}
		.into();

		let key =
			IdempotencyKey::new(agent_surface::unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state =
				runtime.block_on(client.live_settings(work_id, true)).unwrap_or(State::Unavailable);

			Some((outcome, state))
		});

		self.live_reviewer.task=Some(cx.spawn(async move |surface,cx| {
			let result=future.await;
			let _=surface.update(cx,|s,cx| {
				if s.live_reviewer.epoch!=epoch {return;}

				s.live_reviewer.task=None;

				let current=s.snapshot.as_ref().is_some_and(|snapshot| snapshot.runtime_source.as_ref()==Some(&source)
					&& snapshot.work_items.iter().any(|w| w.id==work && w.codex_thread_id.as_deref()==Some(&thread) && w.active_turn_id.as_deref()==Some(&turn) && w.dispatch_state==AgentDispatchStateDto::Running));

				if !current||s.selected.as_ref()!=Some(&work) {s.reset_live_reviewer();cx.notify();return;}

				let (outcome,state)=result.unwrap_or((None,State::Unavailable));

				s.live_reviewer.reviewed = !saving;

				s.live_reviewer.feedback=match outcome {
					Some(Ok(AgentCommandResponse::Accepted {..}))=>"Published for subsequent steps of this turn. This does not confirm a later inference used the selection.",
					Some(Ok(AgentCommandResponse::Rejected {..}))=>"The edit was not accepted. Refresh and review the current turn.",
					Some(_)=>"Publication could not be confirmed. No automatic retry was made.",
					None if saving=>"The operation could not be confirmed. Refresh its receipt before another edit.",
					None if matches!(state,State::Unavailable)=>"No editable active turn is available. Refresh the task.",
					None=>"Changes apply to subsequent steps of this turn. Saved task defaults stay unchanged.",
				}.into();
				s.live_reviewer.state=Some(match state {State::Available {ref thread_id,ref turn_id,..} if thread_id.as_str()!=thread || turn_id.as_str()!=turn=>State::Unavailable,other=>other});
				cx.notify();
			});
		}));

		cx.notify();
	}

	pub(super) fn live_reviewer_panel(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		if self.native_agents.selected.is_some() || !self.command_connection_ready() {
			return agent_surface::div().into_any_element();
		}

		let Some(turn) = work
			.active_turn_id
			.as_ref()
			.filter(|_| work.dispatch_state == AgentDispatchStateDto::Running)
		else {
			return agent_surface::div().into_any_element();
		};
		let (owner, target) = (work.id.clone(), turn.clone());
		let mut panel =
			agent_surface::div().flex().flex_col().gap_2().child("Current-turn settings").child(
				mcp_forms::mcp_button(
					"live-reviewer-read".into(),
					"Review current-turn settings".into(),
					false,
					cx,
					move |s, cx| s.update_live_settings(owner.clone(), target.clone(), None, cx),
				),
			);

		if self.live_reviewer.work.as_ref() == Some(&work.id) {
			panel = panel.child(self.live_reviewer.feedback.clone());

			if let Some(State::Available {
				turn_id,
				can_update,
				last_reviewer,
				last_model,
				last_outcome,
				model_choices,
				..
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
				if let (Some(selection), Some(outcome)) = (last_model, last_outcome) {
					panel = panel.child(format!(
						"Last model request: {} / {} — {}",
						selection.model.as_str(),
						selection.effort.as_str(),
						outcome_label(*outcome)
					));
				}

				if *can_update
					&& self.live_reviewer.reviewed
					&& let Some(models) = model_choices
				{
					panel = panel.child(self.live_model_controls(work, models, cx));
				}
				if *can_update && self.live_reviewer.reviewed {
					for (id, label, reviewer) in [
						("live-reviewer-user", "Ask me", Reviewer::User),
						("live-reviewer-auto", "Automatic review", Reviewer::AutoReview),
					] {
						let (owner, target) = (work.id.clone(), turn.clone());

						panel = panel.child(mcp_forms::mcp_button(
							id.into(),
							label.into(),
							false,
							cx,
							move |s, cx| {
								s.update_live_settings(
									owner.clone(),
									target.clone(),
									Some(Edit::Reviewer(reviewer)),
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

	fn live_model_controls(
		&self,
		work: &AgentWorkItemDto,
		models: &[AgentModelDto],
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut panel =
			agent_surface::div().flex().flex_col().gap_2().child(mcp_forms::mcp_button(
				"live-model-open".into(),
				"Change model and effort…".into(),
				false,
				cx,
				|s, cx| {
					s.live_reviewer.choosing_model = !s.live_reviewer.choosing_model;

					cx.notify();
				},
			));

		if !self.live_reviewer.choosing_model {
			return panel.into_any_element();
		}

		let mut choices = agent_surface::div()
			.id("live-model-choices")
			.flex()
			.flex_col()
			.gap_1()
			.max_h(px(180.))
			.overflow_y_scroll();

		for (index, model) in models.iter().enumerate() {
			let Some(effort) =
				model.default_effort.clone().or_else(|| model.efforts.first().cloned())
			else {
				continue;
			};
			let selection =
				decodex_protocol::AgentLiveModelSelection { model: model.model.clone(), effort };

			choices = choices.child(mcp_forms::mcp_button(
				format!("live-model-choice-{index}"),
				model_settings::model_choice_label(model),
				false,
				cx,
				move |s, cx| {
					s.live_reviewer.model_draft = Some(selection.clone());

					cx.notify();
				},
			));
		}

		panel = panel.child(choices);

		if let Some(selection) = &self.live_reviewer.model_draft
			&& let Some(model) = models.iter().find(|m| m.model == selection.model)
		{
			panel = panel.child(format!("{} / {}", model.name, selection.effort.as_str()));

			let mut efforts = agent_surface::div().flex().flex_wrap().gap_1();

			for (index, effort) in model.efforts.iter().enumerate() {
				let effort = effort.clone();

				efforts = efforts.child(mcp_forms::mcp_button(
					format!("live-model-effort-{index}"),
					effort.as_str().into(),
					false,
					cx,
					move |s, cx| {
						if let Some(draft) = &mut s.live_reviewer.model_draft {
							draft.effort = effort.clone();
						}

						cx.notify();
					},
				));
			}

			let (owner, turn, selection) = (
				work.id.clone(),
				work.active_turn_id.clone().unwrap_or_default(),
				selection.clone(),
			);

			panel = panel.child(efforts).child(mcp_forms::mcp_button(
				"live-model-apply".into(),
				"Apply to this turn".into(),
				false,
				cx,
				move |s, cx| {
					s.update_live_settings(
						owner.clone(),
						turn.clone(),
						Some(Edit::Model(selection.clone())),
						cx,
					)
				},
			));
		}

		panel.into_any_element()
	}
}

fn outcome_label(outcome: AgentLiveReviewerOutcome) -> &'static str {
	match outcome {
		O::Reserved => "awaiting confirmation",
		O::Applied => "published",
		O::TargetUnavailable => "turn no longer active",
		O::Rejected => "rejected",
		O::Unknown => "unconfirmed",
	}
}

#[cfg(test)]
#[path = "agent_live_settings_wire_tests.rs"]
pub(super) mod wire_tests;
