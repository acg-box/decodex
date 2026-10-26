//! Find retained attached conversations through the existing branch and navigation owners.
use gpui::AnyElement;
use tokio::runtime::Builder;

use crate::shell::agent_surface::{
	AgentClient, AgentSnapshotDto, AgentSurface, Context, EntityId, IntoElement, ParentElement,
	Styled, Task,
};
use decodex_protocol::{AttachedBranchesResult, PromptForkPhase, PromptForkStatus};

#[derive(Default)]
pub(super) struct Panel {
	owner: Option<(String, EntityId)>,
	result: Option<AttachedBranchesResult>,
	task: Option<Task<()>>,
	epoch: u64,
}

impl AgentSurface {
	pub(super) fn invalidate_attached_branches(&mut self, next: &AgentSnapshotDto) {
		if self.prompt_edit.attached.owner.as_ref().is_some_and(|(work, source)| {
			next.runtime_source.as_ref() != Some(source)
				|| !next.work_items.iter().any(|w| {
					&w.id == work
						&& self
							.snapshot
							.as_ref()
							.and_then(|s| s.work_items.iter().find(|old| &old.id == work))
							.is_some_and(|old| old.codex_thread_id == w.codex_thread_id)
				})
		}) {
			self.reset_attached_branches();
		}
	}

	pub(super) fn reset_attached_branches(&mut self) {
		self.prompt_edit.attached =
			Panel { epoch: self.prompt_edit.attached.epoch.wrapping_add(1), ..Default::default() };
	}

	fn load_attached_branches(&mut self, work: &str, cx: &mut Context<Self>) {
		let (Some(profile), Some(source)) =
			(self.profile.clone(), self.snapshot.as_ref().and_then(|s| s.runtime_source.clone()))
		else {
			return;
		};

		if self.selected.as_deref() != Some(work)
			|| self.native_agents.selected.is_some()
			|| (self.prompt_edit.attached.task.is_some()
				&& self.prompt_edit.attached.owner.as_ref().is_some_and(|(owner, _)| owner == work))
		{
			return;
		}

		let Ok(owner) = EntityId::new(work) else {
			return;
		};

		self.reset_attached_branches();

		let key = (work.to_owned(), source);

		self.prompt_edit.attached.owner = Some(key.clone());

		let epoch = self.prompt_edit.attached.epoch;
		let read = cx.background_executor().spawn(async move {
			let Ok(runtime) = Builder::new_current_thread().enable_all().build() else {
				return AttachedBranchesResult::Unavailable;
			};

			runtime
				.block_on(AgentClient::new(profile).attached_branches(owner))
				.unwrap_or(AttachedBranchesResult::Unavailable)
		});

		self.prompt_edit.attached.task = Some(cx.spawn(async move |surface, cx| {
			let result = read.await;
			let _ = surface.update(cx, |s, cx| {
				if s.prompt_edit.attached.epoch == epoch
					&& s.prompt_edit.attached.owner.as_ref() == Some(&key)
					&& s.snapshot.as_ref().and_then(|v| v.runtime_source.as_ref()) == Some(&key.1)
				{
					s.prompt_edit.attached.task = None;
					s.prompt_edit.attached.result = Some(result);

					cx.notify();
				}
			});
		}));

		cx.notify();
	}

	pub(super) fn attached_branches_panel(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		if self.native_agents.selected.is_some() {
			return gpui::div().into_any_element();
		}

		let opened = self.prompt_edit.attached.owner.as_ref().is_some_and(|(owner, source)| {
			owner == work
				&& self.snapshot.as_ref().and_then(|s| s.runtime_source.as_ref()) == Some(source)
		});
		let owner = work.to_owned();
		let mut panel = gpui::div().flex().flex_col().gap_2().child(self.workspace_action(
			"attached-branches-toggle".into(),
			"Attached branches".into(),
			move |s, cx| {
				if opened {
					s.reset_attached_branches();
					cx.notify();
				} else {
					s.load_attached_branches(&owner, cx);
				}
			},
			cx,
		));

		if !opened {
			return panel.into_any_element();
		}

		panel=panel.child("Choose ‘Attached branch after this turn’ when reviewing an earlier message. Open a branch here to continue it.");

		match &self.prompt_edit.attached.result {
			Some(AttachedBranchesResult::Available { branches, truncated }) => {
				if branches.is_empty() {
					panel = panel.child("No attached branches are recorded for this conversation.");
				}

				for branch in branches {
					panel = panel.child(self.attached_branch_row(branch, cx));
				}

				if *truncated {
					panel = panel.child("Showing the newest 100 attached branches.");
				}
			},
			Some(AttachedBranchesResult::Unavailable) =>
				panel = panel.child("Attached branches could not be read. Refresh to try again."),
			None => panel = panel.child("Reading attached branches…"),
		}

		if self.prompt_edit.attached.task.is_none() {
			let owner = work.to_owned();

			panel = panel.child(self.workspace_action(
				"attached-branches-refresh".into(),
				"Refresh attached branches".into(),
				move |s, cx| s.load_attached_branches(&owner, cx),
				cx,
			));
		}

		panel.into_any_element()
	}

	fn attached_branch_row(&self, branch: &PromptForkStatus, cx: &mut Context<Self>) -> AnyElement {
		let target = branch.target_thread_id.as_ref().and_then(|thread| {
			self.snapshot.as_ref().and_then(|s| {
				s.work_items.iter().find(|w| {
					w.id == branch.target_work_id.as_str()
						&& w.codex_thread_id.as_deref() == Some(thread.as_str())
				})
			})
		});
		let mut row = gpui::div()
			.flex()
			.flex_col()
			.gap_1()
			.child(target.map_or_else(|| "Attached branch".to_owned(), |w| w.title.clone()));

		if branch.phase == PromptForkPhase::Forked && target.is_some() {
			let expected = branch.clone();
			let owner = self.prompt_edit.attached.owner.clone();

			row = row.child(self.workspace_action(
				format!("attached-branch-open-{}", branch.target_work_id.as_str()),
				"Open branch".into(),
				move |s, cx| {
					if s.prompt_edit.attached.owner == owner
						&& owner.as_ref().is_some_and(|(work, source)| {
							s.selected.as_ref() == Some(work)
								&& s.snapshot.as_ref().and_then(|v| v.runtime_source.as_ref())
									== Some(source)
						})
						&& s.snapshot.as_ref().is_some_and(|v| {
							v.work_items.iter().any(|w| {
								w.id == expected.target_work_id.as_str()
									&& w.codex_thread_id.as_deref()
										== expected.target_thread_id.as_ref().map(|t| t.as_str())
							})
						}) {
						s.open_page(expected.target_work_id.as_str(), cx);
					}
				},
				cx,
			));
		} else {
			row = row.child(match branch.phase {
				PromptForkPhase::Uncertain =>
					"Creation is unconfirmed. Keep the saved choice; do not create it again.",
				PromptForkPhase::Acknowledged =>
					"Native identity is saved. Recover the saved branch choice to finish loading history.",
				PromptForkPhase::Rejected =>
					"Creation was rejected. The original conversation is retained.",
				PromptForkPhase::Forked =>
					"Not in the active view. If archived, restore it in Codex, then refresh.",
			});
		}

		row.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use gpui::{AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, Window};

	use crate::shell::agent_surface::{AgentSurface, attached_branches::Panel};
	use decodex_protocol::{
		AttachedBranchesResult, EntityId, PromptForkBoundary, PromptForkPhase, PromptForkStatus,
		WireText,
	};

	struct View {
		surface: Entity<AgentSurface>,
	}
	impl Render for View {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			self.surface.update(cx, |s, cx| s.attached_branches_panel("agent", cx))
		}
	}

	#[gpui::test]
	fn attached_branch_navigation_keeps_the_source_draft_and_requires_current_identity(
		cx: &mut TestAppContext,
	) {
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);

			cx.observe(&surface, |_, _, cx| cx.notify()).detach();

			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.visual_workbench_page("workbench", cx);

				s.selected = Some("agent".into());

				let snapshot = s.snapshot.as_mut().unwrap();
				let source = snapshot.work_items.iter().find(|w| w.id == "agent").unwrap().clone();
				let mut target = source.clone();

				target.id = "branch".into();
				target.parent_goal_id = Some(source.id.clone());
				target.codex_thread_id = Some("branch-native".into());

				snapshot.work_items.push(target);

				let runtime = EntityId::new("runtime-source").unwrap();

				snapshot.runtime_source = Some(runtime.clone());
				s.prompt_edit.attached = Panel {
					owner: Some((source.id.clone(), runtime)),
					result: Some(AttachedBranchesResult::Available {
						branches: vec![PromptForkStatus {
							work_id: EntityId::new(source.id).unwrap(),
							thread_id: WireText::new(source.codex_thread_id.unwrap()).unwrap(),
							review_token: WireText::new("a".repeat(64)).unwrap(),
							target_work_id: EntityId::new("branch").unwrap(),
							target_thread_id: Some(WireText::new("branch-native").unwrap()),
							boundary: PromptForkBoundary::AttachedAfterTurn,
							phase: PromptForkPhase::Forked,
							edit_receipt_id: None,
						}],
						truncated: false,
					}),
					..Default::default()
				};

				s.composer.update(cx, |input, cx| input.set_content("Keep the source draft", cx));
			});

			View { surface }
		});

		visual.simulate_resize(gpui::size(gpui::px(700.), gpui::px(500.)));
		visual.update(crate::ui_frame::draw_frame);

		let open =
			visual.debug_bounds("attached-branch-open-branch").expect("open retained branch");
		let surface = view.read_with(visual, |v, _| v.surface.clone());
		let (owner, result) = surface.read_with(visual, |s, _| {
			(s.prompt_edit.attached.owner.clone(), s.prompt_edit.attached.result.clone())
		});

		visual.simulate_click(open.center(), Default::default());

		surface.update(visual, |s, cx| {
			assert_eq!(s.selected.as_deref(), Some("branch"));
			assert!(s.submission.waiting.is_none());

			s.open_page("agent", cx);

			assert_eq!(s.composer.read(cx).content(), "Keep the source draft");

			s.prompt_edit.attached = Panel { owner, result, ..Default::default() };

			set_phase(&mut s.prompt_edit.attached, PromptForkPhase::Uncertain);

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("attached-branch-open-branch").is_none());

		surface.update(visual, |s, cx| {
			set_phase(&mut s.prompt_edit.attached, PromptForkPhase::Forked);

			s.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "branch")
				.unwrap()
				.codex_thread_id = Some("replacement-native".into());

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("attached-branch-open-branch").is_none());

		surface.update(visual, |s, cx| {
			let mut next = s.snapshot.clone().unwrap();

			next.runtime_source = Some(EntityId::new("replacement-runtime").unwrap());

			s.invalidate_attached_branches(&next);

			assert!(s.prompt_edit.attached.result.is_none());
			assert_eq!(s.composer.read(cx).content(), "Keep the source draft");

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("attached-branch-open-branch").is_none());
	}

	fn set_phase(panel: &mut Panel, phase: PromptForkPhase) {
		let Some(AttachedBranchesResult::Available { branches, .. }) = &mut panel.result else {
			panic!("retained branches");
		};

		branches[0].phase = phase;
	}
}
