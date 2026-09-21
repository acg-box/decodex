//! Read-only native goals; native persistence remains the sole goal owner.
use super::*;
use decodex_protocol::{ChiefGoalResult, ChiefNativeGoal};

type GoalContext = (String, String, EntityId);
#[derive(Default)]
pub(super) struct Panel {
	owner: Option<GoalContext>,
	epoch: u64,
	result: Option<ChiefGoalResult>,
	last_read: Option<std::time::Instant>,
	task: Option<Task<()>>,
}
impl ChiefSurface {
	fn goal_context(&self) -> Option<GoalContext> {
		if matches!(self.state, LoadState::Stale | LoadState::Unavailable) {
			return None;
		}
		let snapshot = self.snapshot.as_ref()?;
		let work =
			snapshot.work_items.iter().find(|work| Some(&work.id) == self.selected.as_ref())?;
		Some((work.id.clone(), work.codex_thread_id.clone()?, snapshot.runtime_source.clone()?))
	}

	pub(super) fn goal_disconnected(&mut self) {
		self.native_goal =
			Panel { epoch: self.native_goal.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn load_native_goal(&mut self, cx: &mut Context<Self>) {
		let context = self.goal_context();
		if self.native_goal.owner != context {
			self.goal_disconnected();
			self.native_goal.owner = context.clone();
		}
		let (Some(owner), Some(profile)) = (context, self.profile.clone()) else {
			return;
		};
		if self.native_goal.task.is_some()
			|| self
				.native_goal
				.last_read
				.is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(5))
		{
			return;
		}
		let Ok(work) = EntityId::new(owner.0.clone()) else {
			return;
		};
		let epoch = self.native_goal.epoch;
		self.native_goal.last_read = Some(std::time::Instant::now());
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).goal_state(work)).ok()
		});
		self.native_goal.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(ChiefGoalResult::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.native_goal.epoch != epoch {
					return;
				}
				s.native_goal.task = None;
				if s.goal_context().as_ref() != Some(&owner) {
					return;
				}
				s.native_goal.result = Some(bind_result(result, &owner));
				cx.notify();
			});
		}));
	}

	pub(super) fn native_goal_panel(&self) -> impl IntoElement {
		let mut panel = div().id("chief-native-goal").flex().flex_col().gap_1();
		let Some(owner) = self.goal_context() else {
			return panel;
		};
		if self.native_goal.owner.as_ref() != Some(&owner) {
			return panel;
		}
		match self.native_goal.result.as_ref() {
			Some(ChiefGoalResult::Available { goal: Some(goal), .. }) => {
				panel = panel
					.p_3()
					.text_sm()
					.child(format!("Goal · {}", status_label(&goal.status)))
					.child(
						div()
							.id("chief-native-goal-objective")
							.debug_selector(|| "chief-native-goal-objective".into())
							.child(goal.objective.clone()),
					)
					.child(goal_progress(goal));
			},
			Some(ChiefGoalResult::Unavailable) => {
				panel = panel.text_sm().child("Goal status unavailable");
			},
			_ => {},
		}
		panel
	}
}
fn bind_result(result: ChiefGoalResult, owner: &GoalContext) -> ChiefGoalResult {
	match &result {
		ChiefGoalResult::Available { source, thread_id, .. }
			if source != &owner.2 || thread_id != &owner.1 || !result.is_valid() =>
			ChiefGoalResult::Unavailable,
		_ => result,
	}
}
fn status_label(status: &str) -> &str {
	match status {
		"active" => "Active",
		"paused" => "Paused",
		"blocked" => "Blocked",
		"usageLimited" => "Usage limit reached",
		"budgetLimited" => "Goal budget reached",
		"complete" => "Complete",
		_ => "Unknown status",
	}
}
fn goal_progress(goal: &ChiefNativeGoal) -> String {
	let tokens = goal.token_budget.map_or_else(
		|| format!("{} tokens used", goal.tokens_used),
		|budget| format!("{} / {budget} tokens", goal.tokens_used),
	);
	format!("{tokens} · {}s execution time", goal.time_used_seconds)
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn goal_readback_rejects_foreign_sources_and_does_not_infer_empty() {
		let source = EntityId::new("source").unwrap();
		let owner = ("work".into(), "thread".into(), source.clone());
		let empty = ChiefGoalResult::Available { source, thread_id: "thread".into(), goal: None };
		assert_eq!(bind_result(empty.clone(), &owner), empty);
		let foreign = ("work".into(), "thread".into(), EntityId::new("other").unwrap());
		assert_eq!(bind_result(empty, &foreign), ChiefGoalResult::Unavailable);
		assert_eq!(bind_result(ChiefGoalResult::Unavailable, &owner), ChiefGoalResult::Unavailable);
	}
	#[test]
	fn goal_progress_distinguishes_budget_from_usage_and_preserves_zero() {
		let mut goal = ChiefNativeGoal {
			thread_id: "thread".into(),
			objective: "Objective".into(),
			status: "active".into(),
			token_budget: None,
			tokens_used: 0,
			time_used_seconds: 0,
			created_at: 0,
			updated_at: 0,
		};
		assert_eq!(goal_progress(&goal), "0 tokens used · 0s execution time");
		goal.token_budget = Some(100);
		assert_eq!(goal_progress(&goal), "0 / 100 tokens · 0s execution time");
		assert_ne!(status_label("usageLimited"), status_label("budgetLimited"));
		assert_eq!(status_label("future"), "Unknown status");
	}
	#[gpui::test]
	fn native_goal_panel_clears_on_source_replacement_and_disconnect(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, _| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				runtime_source: Some(EntityId::new("source").unwrap()),
				workspaces: vec![],
				dependencies: vec![],
				pending_events: vec![],
				work_items: vec![ChiefWorkItemDto {
					id: "root".into(),
					parent_goal_id: None,
					kind: decodex_protocol::ChiefWorkKindDto::Goal,
					title: "Chief".into(),
					codex_thread_id: Some("thread".into()),
					active_turn_id: None,
					dispatch_state: ChiefDispatchStateDto::Idle,
					status: ChiefWorkStatusDto::Open,
					next_check_at_micros: None,
					created_at_micros: 1,
					updated_at_micros: 1,
				}],
			})));
			s.native_goal.owner = s.goal_context();
			s.native_goal.result = Some(ChiefGoalResult::Available {
				source: EntityId::new("source").unwrap(),
				thread_id: "thread".into(),
				goal: Some(ChiefNativeGoal {
					thread_id: "thread".into(),
					objective: "Continue the native goal".into(),
					status: "paused".into(),
					token_budget: None,
					tokens_used: 12,
					time_used_seconds: 3,
					created_at: 1,
					updated_at: 2,
				}),
			});
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.), px(1200.)));
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("chief-native-goal-objective").is_some());
		surface.update(visual, |s, cx| {
			s.snapshot.as_mut().unwrap().runtime_source =
				Some(EntityId::new("replacement").unwrap());
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("chief-native-goal-objective").is_none());
		surface.update(visual, |s, cx| {
			let old_epoch = s.native_goal.epoch;
			s.mark_stale(cx);
			assert!(s.native_goal.result.is_none());
			assert_ne!(s.native_goal.epoch, old_epoch);
		});
	}
}
