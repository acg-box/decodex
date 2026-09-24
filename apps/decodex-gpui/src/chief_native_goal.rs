//! Native goal observations do not change Chief coordination state.
use super::*;
use decodex_protocol::ChiefNativeGoalResult as Result;
use std::time::Instant;

#[derive(Default)]
pub(super) struct Panel {
	target: Option<(String, String)>,
	result: Option<Result>,
	task: Option<Task<()>>,
	epoch: u64,
	read_at: Option<Instant>,
}
impl ChiefSurface {
	pub(super) fn reset_native_goal(&mut self) {
		self.native_goal =
			Panel { epoch: self.native_goal.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_native_goal(&mut self, next: &ChiefSnapshotDto) {
		let Some((work, _)) = &self.native_goal.target else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);
		if self.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref())
			!= next.runtime_source.as_ref()
			|| !matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id)
		{
			self.reset_native_goal();
		}
	}

	fn native_goal_target(&self) -> Option<(String, String)> {
		let work = self.selected.as_ref()?;
		if let Some((owner, thread)) = &self.native_agents.selected {
			return (owner == work).then(|| (owner.clone(), thread.clone()));
		}
		let thread = self
			.snapshot
			.as_ref()?
			.work_items
			.iter()
			.find(|w| &w.id == work)?
			.codex_thread_id
			.clone()?;
		Some((work.clone(), thread))
	}

	pub(super) fn refresh_native_goal(&mut self, cx: &mut Context<Self>) {
		if self.native_goal.target.is_some()
			&& self.native_goal.read_at.is_none_or(|at| at.elapsed().as_secs() >= 5)
		{
			self.load_native_goal(cx);
		}
	}

	fn load_native_goal(&mut self, cx: &mut Context<Self>) {
		if self.native_goal.task.is_some() || !self.command_connection_ready() {
			return;
		}
		let (Some(target), Some(profile), Some(source)) = (
			self.native_goal_target(),
			self.profile.clone(),
			self.snapshot.as_ref().and_then(|s| s.runtime_source.clone()),
		) else {
			return;
		};
		let (Ok(work_id), Ok(thread_id)) =
			(EntityId::new(target.0.clone()), EntityId::new(target.1.clone()))
		else {
			return;
		};
		self.native_goal.target = Some(target.clone());
		self.native_goal.epoch = self.native_goal.epoch.wrapping_add(1);
		let epoch = self.native_goal.epoch;
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).native_goal(work_id, thread_id)).ok()
		});
		self.native_goal.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await.unwrap_or(Result::Unavailable);
			let _ = surface.update(cx, |s, cx| {
				if s.native_goal.epoch != epoch {
					return;
				}
				s.native_goal.task = None;
				if s.native_goal_target() != Some(target)
					|| s.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref()) != Some(&source)
				{
					s.reset_native_goal();
					cx.notify();
					return;
				}
				s.native_goal.result = Some(result);
				s.native_goal.read_at = Some(Instant::now());
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn native_goal_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		if !self.command_connection_ready() || self.native_goal_target().is_none() {
			return div().into_any_element();
		}
		let mut panel = div().flex().flex_col().gap_2().child(self.workspace_action(
			"native-goal-read".into(),
			"Read native goal".into(),
			|s, cx| s.load_native_goal(cx),
			cx,
		));
		if self.native_goal.target == self.native_goal_target() {
			panel = panel.child(
				self.native_goal
					.result
					.as_ref()
					.map(goal_text)
					.unwrap_or_else(|| "Reading native goal…".into()),
			);
		}
		panel.into_any_element()
	}
}
fn goal_text(result: &Result) -> String {
	match result {
		Result::Available { goal: None, .. } => "This conversation has no native goal.".into(),
		Result::Available { goal: Some(goal), observed_at_micros, .. } => {
			use decodex_protocol::ChiefNativeGoalStatus as S;
			let status = match goal.status {
				S::Active => "Active",
				S::Paused => "Paused",
				S::Blocked => "Blocked",
				S::UsageLimited => "Usage limited",
				S::BudgetLimited => "Budget limited",
				S::Complete => "Complete",
			};
			let budget = goal
				.token_budget
				.map_or_else(|| "No token budget".into(), |v| format!("Token budget: {v}"));
			let observed =
				time::OffsetDateTime::from_unix_timestamp(*observed_at_micros / 1_000_000)
					.map(|v| format!("{:02}:{:02}:{:02} UTC", v.hour(), v.minute(), v.second()))
					.unwrap_or_else(|_| "unknown time".into());
			format!(
				"Native goal · {status}\n{}{}\n{budget}\nGoal tokens used: {}\nGoal elapsed: {} seconds\nRead at {observed}. Refreshes while this task is selected.",
				goal.objective,
				if goal.objective_truncated { "\nSome objective content was omitted." } else { "" },
				goal.tokens_used,
				goal.time_used_seconds,
			)
		},
		Result::Disabled => "Native goals are disabled for this account process.".into(),
		Result::Unsupported => "This Codex version does not expose native goals.".into(),
		Result::Unavailable => "The native goal could not be read. Refresh to try again.".into(),
	}
}

#[cfg(test)]
#[path = "chief_native_goal_tests.rs"]
mod tests;
