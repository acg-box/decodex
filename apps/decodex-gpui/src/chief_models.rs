//! Explicit task model selection with source-bound review and durable result readback.
use super::{mcp_forms::mcp_button, *};
use decodex_protocol::{
	ChiefModelOutcome as Outcome, ChiefModelSelectionState as State, ConversationModel,
	ConversationReasoningEffort,
};

#[derive(Default)]
pub(super) struct Panel {
	state: Option<State>,
	work: Option<String>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
	reviewed: bool,
	selected_model: Option<ConversationModel>,
}
impl ChiefSurface {
	pub(super) fn reset_task_models(&mut self) {
		self.task_models =
			Panel { epoch: self.task_models.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_task_models(&mut self, next: &ChiefSnapshotDto) {
		let Some(work) = &self.task_models.work else {
			return;
		};
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);
		if !matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id && a.status==b.status && a.dispatch_state==b.dispatch_state && a.active_turn_id==b.active_turn_id)
			|| self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
		{
			self.reset_task_models();
		}
	}

	fn model_selection_action(
		&self,
		work: &EntityId,
		thread: &str,
		model: ConversationModel,
		effort: Option<ConversationReasoningEffort>,
	) -> Option<ChiefActionDto> {
		let State::Available {
			work_id,
			thread_id,
			review_token,
			model: current_model,
			effort: current_effort,
			models,
			can_update: true,
			..
		} = self.task_models.state.as_ref()?
		else {
			return None;
		};
		if work_id != work
			|| thread_id.as_str() != thread
			|| self.task_models.work.as_deref() != Some(work.as_str())
			|| (current_model == &model
				&& effort.as_ref().is_none_or(|e| current_effort.as_ref() == Some(e)))
			|| !models
				.iter()
				.any(|m| m.model == model && effort.as_ref().is_none_or(|e| m.efforts.contains(e)))
		{
			return None;
		}
		Some(ChiefActionDto::SetTaskModel {
			work_id: work.clone(),
			thread_id: thread_id.clone(),
			review_token: review_token.clone(),
			model,
			effort,
		})
	}

	fn choose_task_model(&mut self, model: ConversationModel, cx: &mut Context<Self>) {
		if !self.task_models.reviewed || self.task_models.task.is_some() {
			return;
		}
		if matches!(&self.task_models.state, Some(State::Available { models, can_update:true, .. }) if models.iter().any(|m|m.model==model))
		{
			self.task_models.selected_model = Some(model);
			cx.notify();
		}
	}

	fn update_task_models(
		&mut self,
		work: String,
		selection: Option<(ConversationModel, Option<ConversationReasoningEffort>)>,
		cx: &mut Context<Self>,
	) {
		if self.task_models.task.is_some()
			|| self.selected.as_ref() != Some(&work)
			|| !self.command_connection_ready()
			|| self.native_agents.selected.is_some()
			|| (selection.is_some() && !self.task_models.reviewed)
		{
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Some(snapshot) = &self.snapshot else {
			return;
		};
		let Some(source) = snapshot.runtime_source.clone() else {
			return;
		};
		let Some(thread) = snapshot
			.work_items
			.iter()
			.find(|w| w.id == work)
			.and_then(|w| w.codex_thread_id.clone())
		else {
			return;
		};
		let Ok(work_id) = EntityId::new(work.clone()) else {
			return;
		};
		let action = if let Some((model, effort)) = selection {
			let Some(action) = self.model_selection_action(&work_id, &thread, model, effort) else {
				return;
			};
			Some(action)
		} else {
			None
		};
		let saving = action.is_some();
		let generation = self.generation;
		self.task_models.epoch = self.task_models.epoch.wrapping_add(1);
		let epoch = self.task_models.epoch;
		self.task_models.work = Some(work.clone());
		self.task_models.state = None;
		self.task_models.selected_model = None;
		self.task_models.reviewed = false;
		self.task_models.feedback =
			if saving { "Saving model selection…" } else { "Reading model settings…" }.into();
		let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state =
				runtime.block_on(client.model_selection(work_id)).unwrap_or(State::Unavailable);
			Some((outcome, state))
		});
		self.task_models.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				s.finish_task_models(
					(generation, epoch),
					(&work, &thread, &source),
					saving,
					result,
					cx,
				);
			});
		}));
		cx.notify();
	}

	fn finish_task_models(
		&mut self,
		versions: (u64, u64),
		binding: (&str, &str, &EntityId),
		saving: bool,
		result: Option<(
			Option<Result<ChiefCommandResponse, decodex_protocol::ClientFailure>>,
			State,
		)>,
		cx: &mut Context<Self>,
	) {
		let (generation, epoch) = versions;
		let (work, thread, source) = binding;
		if self.generation != generation || self.task_models.epoch != epoch {
			return;
		}
		self.task_models.task = None;
		let current = self.command_connection_ready()
			&& self.native_agents.selected.is_none()
			&& self.selected.as_deref() == Some(work)
			&& self.snapshot.as_ref().is_some_and(|snapshot| {
				snapshot.runtime_source.as_ref() == Some(source)
					&& snapshot
						.work_items
						.iter()
						.any(|w| w.id == work && w.codex_thread_id.as_deref() == Some(thread))
			});
		if !current {
			self.reset_task_models();
			cx.notify();
			return;
		}
		let (outcome, state) = result.unwrap_or((None, State::Unavailable));
		self.task_models.reviewed = !saving;
		self.task_models.feedback = match outcome {
			Some(Ok(ChiefCommandResponse::Accepted { .. })) =>
				"Change submitted. Saved settings appear below.",
			Some(Ok(ChiefCommandResponse::Rejected { .. })) =>
				"Change was not accepted. Refresh before choosing again.",
			Some(_) => "Response was not confirmed. Saved settings appear below.",
			None if saving => "Change could not be confirmed. Refresh to check its saved state.",
			None => "Changes apply to subsequent turns.",
		}
		.into();
		self.task_models.state = Some(match state {
			State::Available { ref work_id, ref thread_id, .. }
				if work_id.as_str() != work || thread_id.as_str() != thread =>
				State::Unavailable,
			other => other,
		});
		cx.notify();
	}

	pub(super) fn task_models_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		if work.codex_thread_id.is_none()
			|| self.native_agents.selected.is_some()
			|| !self.command_connection_ready()
		{
			return div().into_any_element();
		}
		let owner = work.id.clone();
		let mut panel = div().flex().flex_col().gap_2().child("Model").child(mcp_button(
			"task-models-read".into(),
			"Review / change model".into(),
			false,
			cx,
			move |s, cx| s.update_task_models(owner.clone(), None, cx),
		));
		if self.task_models.work.as_ref() != Some(&work.id) {
			return panel.into_any_element();
		}
		panel = panel.child(self.task_models.feedback.clone());
		match &self.task_models.state {
			Some(State::Available {
				model,
				model_provider,
				effort: current_effort,
				models,
				can_update,
				last_outcome,
				..
			}) => {
				panel = panel.child(format!(
					"{} · {} · effort {}",
					model.as_str(),
					model_provider.as_str(),
					current_effort.as_ref().map_or("default", |e| e.as_str())
				));
				if let Some(state) = last_outcome {
					panel = panel.child(label(*state));
				}
				if *can_update
					&& self.task_models.reviewed
					&& self.task_models.task.is_none()
					&& work.status != ChiefWorkStatusDto::Resolved
				{
					panel = panel.child(self.task_model_choices(
						work,
						model,
						current_effort.as_ref(),
						models,
						cx,
					));
				}
			},
			Some(State::Pending { model, state, .. }) => {
				panel = panel
					.child(format!("{} · {}", model.as_str(), label(*state)))
					.child("Refresh to check confirmation. The change will not be resent.");
			},
			Some(State::Unavailable) => {
				panel = panel
					.child("Model settings are unavailable. Refresh when the task is connected.");
			},
			None => {},
		}
		panel.into_any_element()
	}

	fn task_model_choices(
		&self,
		work: &ChiefWorkItemDto,
		model: &ConversationModel,
		current_effort: Option<&ConversationReasoningEffort>,
		models: &[decodex_protocol::ChiefModelDto],
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let mut panel = div().flex().flex_col().gap_2();
		let mut choices = div()
			.id("task-model-choices")
			.flex()
			.flex_col()
			.gap_1()
			.max_h(px(240.))
			.overflow_y_scroll();
		for (index, model) in models.iter().enumerate() {
			let selected = model.model.clone();
			choices = choices.child(mcp_button(
				format!("task-model-{index}"),
				model.name.clone(),
				false,
				cx,
				move |s, cx| s.choose_task_model(selected.clone(), cx),
			));
		}
		panel = panel.child(choices);
		if let Some(selected) = self
			.task_models
			.selected_model
			.as_ref()
			.and_then(|selected| models.iter().find(|m| &m.model == selected))
		{
			panel = panel.child(format!("Selected: {}", selected.name));
			if &selected.model != model {
				let owner = work.id.clone();
				let model = selected.model.clone();
				panel = panel.child(mcp_button(
					"task-model-preserve".into(),
					"Use model · keep current effort".into(),
					false,
					cx,
					move |s, cx| {
						s.update_task_models(owner.clone(), Some((model.clone(), None)), cx)
					},
				));
			}
			for (index, effort) in selected.efforts.iter().enumerate() {
				if &selected.model == model && Some(effort) == current_effort {
					continue;
				}
				let owner = work.id.clone();
				let model = selected.model.clone();
				let effort = effort.clone();
				panel = panel.child(mcp_button(
					format!("task-model-effort-{index}"),
					format!("Use model · {} effort", effort.as_str()),
					false,
					cx,
					move |s, cx| {
						s.update_task_models(
							owner.clone(),
							Some((model.clone(), Some(effort.clone()))),
							cx,
						)
					},
				));
			}
		}
		panel.into_any_element()
	}
}

fn label(state: Outcome) -> &'static str {
	match state {
		Outcome::Reserved => "Awaiting confirmation",
		Outcome::Queued => "Waiting for confirmation",
		Outcome::Unknown => "Unconfirmed",
		Outcome::Rejected => "Rejected",
		Outcome::TargetObserved => "Saved for subsequent turns",
		Outcome::Superseded => "Replaced by current settings",
	}
}

#[cfg(test)]
#[path = "chief_models_wire_tests.rs"]
mod wire_tests;
