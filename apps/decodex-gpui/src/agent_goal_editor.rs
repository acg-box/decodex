//! Explicit edits to the reviewed native goal, without a second goal store.
use std::{fs::File, io::Read as _};

use gpui::{AnyElement, AppContext as _, IntoElement, ParentElement, PathPromptOptions, Styled};
use tokio::runtime::Builder;

use crate::shell::agent_surface::native_goal::{
	self, AgentActionDto, AgentClient, AgentCommandResponse, AgentSurface, ComposerInput, Context,
	Entity, EntityId, IdempotencyKey, Result, WireText,
};
use decodex_protocol::{AgentGoalBudgetEdit, AgentGoalEdit};

pub(super) struct Editor {
	target: (String, String),
	review: WireText,
	objective: Entity<ComposerInput>,
	budget: Entity<ComposerInput>,
	original_objective: String,
	original_budget: String,
	new_goal: bool,
}
impl AgentSurface {
	fn begin_goal_edit(&mut self, cx: &mut Context<Self>) {
		let Some(target) = self.native_goal_target() else { return };
		let Some(Result::Available { review_token: Some(review), goal, .. }) =
			&self.native_goal.result
		else {
			return;
		};
		let objective = goal
			.as_ref()
			.filter(|g| !g.objective_truncated)
			.map_or(String::new(), |g| g.objective.clone());
		let budget =
			goal.as_ref().and_then(|g| g.token_budget).map_or(String::new(), |n| n.to_string());
		let input = cx.new(|cx| ComposerInput::new(0, cx));

		input.update(cx, |i, cx| i.set_content(&objective, cx));

		let tokens = cx.new(|cx| ComposerInput::new(0, cx));

		tokens.update(cx, |i, cx| i.set_content(&budget, cx));

		self.native_goal.editor = Some(Editor {
			target,
			review: review.clone(),
			objective: input,
			budget: tokens,
			original_objective: objective,
			original_budget: budget,
			new_goal: goal.is_none(),
		});

		cx.notify();
	}

	pub(super) fn goal_edit_controls(&self, cx: &mut Context<Self>) -> AnyElement {
		if self.native_goal.task.is_some() {
			return native_goal::div().into_any_element();
		}

		let Some(Result::Available { review_token: Some(_), goal, .. }) = &self.native_goal.result
		else {
			return native_goal::div().into_any_element();
		};

		if self.native_goal.target != self.native_goal_target() {
			return native_goal::div().into_any_element();
		}

		let mut panel = native_goal::div().flex().flex_col().gap_2();

		if let Some(editor) = &self.native_goal.editor {
			panel = panel
				.child("Objective (leave unchanged to keep the current objective)")
				.child(editor.objective.clone())
				.child("Token budget (blank uses native policy)")
				.child(editor.budget.clone())
				.child(self.workspace_action(
					"goal-import".into(),
					"Import objective text…".into(),
					|s, cx| s.import_goal_objective(cx),
					cx,
				))
				.child(self.workspace_action(
					"goal-save".into(),
					if editor.new_goal { "Create paused goal" } else { "Save goal changes" }.into(),
					|s, cx| s.save_goal_editor(false, cx),
					cx,
				))
				.child(self.workspace_action(
					"goal-start".into(),
					"Save and start goal".into(),
					|s, cx| s.save_goal_editor(true, cx),
					cx,
				))
				.child(self.workspace_action(
					"goal-cancel-edit".into(),
					"Cancel edit".into(),
					|s, cx| {
						s.native_goal.editor = None;

						cx.notify();
					},
					cx,
				));
		} else {
			panel = panel.child(self.workspace_action(
				"goal-edit".into(),
				if goal.is_some() { "Edit goal" } else { "Create goal" }.into(),
				|s, cx| s.begin_goal_edit(cx),
				cx,
			));

			if let Some(goal) = goal {
				let (label, status) =
					if goal.status == decodex_protocol::AgentNativeGoalStatus::Active {
						("Pause goal", decodex_protocol::AgentNativeGoalStatus::Paused)
					} else {
						("Resume goal", decodex_protocol::AgentNativeGoalStatus::Active)
					};

				panel = panel.child(self.workspace_action(
					"goal-status".into(),
					label.into(),
					move |s, cx| s.change_goal_status(status.clone(), cx),
					cx,
				));
			}
		}

		panel.into_any_element()
	}

	fn import_goal_objective(&mut self, cx: &mut Context<Self>) {
		let Some(editor) = &self.native_goal.editor else { return };
		let target = editor.target.clone();
		let input = editor.objective.clone();
		let selected = cx.prompt_for_paths(PathPromptOptions {
			files: true,
			directories: false,
			multiple: false,
			prompt: Some("Import goal objective".into()),
		});

		cx.spawn(async move |surface, cx| {
			let Ok(Ok(Some(paths))) = selected.await else { return };
			let Some(path) = paths.into_iter().next() else { return };
			let text = cx
				.background_executor()
				.spawn(async move {
					let mut file = File::open(path).map_err(|_| ())?;
					let mut bytes = Vec::new();

					file.by_ref().take(64 * 1_024 + 1).read_to_end(&mut bytes).map_err(|_| ())?;

					if bytes.len() > 64 * 1_024 {
						return Err(());
					}

					String::from_utf8(bytes).map_err(|_| ())
				})
				.await;
			let _ = surface.update(cx, |s, cx| {
				if !s
					.native_goal
					.editor
					.as_ref()
					.is_some_and(|e| e.target == target && e.objective == input)
				{
					return;
				}

				match text {
					Ok(text) => {
						input.update(cx, |i, cx| i.set_content(&text, cx));
						s.native_goal.feedback.clear();
					},
					Err(()) =>
						s.native_goal.feedback =
							"Choose a UTF-8 text file no larger than 64 KiB.".into(),
				};

				cx.notify();
			});
		})
		.detach();
	}

	fn save_goal_editor(&mut self, start: bool, cx: &mut Context<Self>) {
		let Some(editor) = &self.native_goal.editor else { return };
		let text = editor.objective.read(cx).content().to_owned();
		let budget = editor.budget.read(cx).content().trim().to_owned();
		let objective = (text != editor.original_objective || editor.new_goal).then_some(text);

		if objective.as_ref().is_some_and(|text| text.trim().is_empty() || text.len() > 64 * 1_024)
		{
			self.native_goal.feedback = "Enter an objective no larger than 64 KiB.".into();

			cx.notify();

			return;
		}

		let budget = if budget == editor.original_budget {
			AgentGoalBudgetEdit::Keep
		} else if budget.is_empty() {
			AgentGoalBudgetEdit::Reset
		} else {
			let Some(tokens) = budget.parse::<i64>().ok().filter(|tokens| *tokens > 0) else {
				self.native_goal.feedback = "Enter a positive whole-number token budget.".into();

				cx.notify();

				return;
			};

			AgentGoalBudgetEdit::Set(tokens)
		};
		let status = if start {
			Some(decodex_protocol::AgentNativeGoalStatus::Active)
		} else if editor.new_goal {
			Some(decodex_protocol::AgentNativeGoalStatus::Paused)
		} else {
			None
		};

		self.submit_goal_edit(
			editor.target.clone(),
			editor.review.clone(),
			AgentGoalEdit { objective, status, budget },
			cx,
		);
	}

	fn change_goal_status(
		&mut self,
		status: decodex_protocol::AgentNativeGoalStatus,
		cx: &mut Context<Self>,
	) {
		let Some(Result::Available { review_token: Some(review), .. }) = &self.native_goal.result
		else {
			return;
		};
		let Some(target) = self.native_goal_target() else { return };

		self.submit_goal_edit(
			target,
			review.clone(),
			AgentGoalEdit {
				objective: None,
				status: Some(status),
				budget: AgentGoalBudgetEdit::Keep,
			},
			cx,
		);
	}

	fn submit_goal_edit(
		&mut self,
		target: (String, String),
		review: WireText,
		edit: AgentGoalEdit,
		cx: &mut Context<Self>,
	) {
		if self.native_goal.task.is_some() || self.native_goal_target() != Some(target.clone()) {
			return;
		}

		let Some(profile) = self.profile.clone() else { return };
		let (Ok(work), Ok(thread)) =
			(EntityId::new(target.0.clone()), EntityId::new(target.1.clone()))
		else {
			return;
		};
		let action = AgentActionDto::EditNativeGoal {
			work_id: work.clone(),
			thread_id: thread.clone(),
			review_token: review,
			edit,
		};
		let key = IdempotencyKey::new(native_goal::unique_command()).expect("command identity");
		let epoch = self.native_goal.epoch;

		self.native_goal.feedback = "Saving native goal…".into();

		let task = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let outcome = runtime.block_on(client.execute(action, key));
			let read =
				runtime.block_on(client.native_goal(work, thread)).unwrap_or(Result::Unavailable);

			Some((outcome, read))
		});

		self.native_goal.task = Some(cx.spawn(async move |surface, cx| {
			let result = task.await;
			let _ = surface.update(cx, |s, cx| {
				if s.native_goal.epoch != epoch {
					return;
				}

				s.native_goal.task = None;

				if s.native_goal_target() != Some(target) {
					return;
				}

				if let Some((outcome, read)) = result {
					let applied = matches!(outcome, Ok(AgentCommandResponse::Accepted { .. }));

					s.native_goal.result = Some(read);

					if applied {
						s.native_goal.editor = None;
					}

					s.native_goal.feedback = if applied {
						"Native goal saved."
					} else {
						"The edit was rejected or could not be confirmed. Read the native goal before retrying."
					}
					.into();
				} else {
					s.native_goal.feedback =
						"The edit could not be confirmed. Read the native goal before retrying."
							.into();
				}

				cx.notify();
			});
		}));

		cx.notify();
	}
}
