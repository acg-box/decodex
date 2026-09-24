//! Saved task permission profiles; current-turn reviewer controls remain separate.
use super::{mcp_forms::mcp_button, *};
use decodex_protocol::{ChiefPermissionOutcome as Outcome, ChiefPermissionState as State};

#[derive(Default)]
pub(super) struct Panel {
	state: Option<State>,
	work: Option<String>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
	reviewed: bool,
}
impl ChiefSurface {
	pub(super) fn reset_permission_profiles(&mut self) {
		self.permission_profiles =
			Panel { epoch: self.permission_profiles.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_permission_profiles(&mut self, next: &ChiefSnapshotDto) {
		let Some(work) = &self.permission_profiles.work else {
			return;
		};
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);
		if !matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id && a.dispatch_state==b.dispatch_state && a.active_turn_id==b.active_turn_id)
			|| self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
		{
			self.reset_permission_profiles();
		}
	}

	fn permission_selection(
		&self,
		work_id: &EntityId,
		thread: &str,
		profile_id: WireText,
	) -> Option<ChiefActionDto> {
		let State::Available {
			work_id: reviewed_work,
			thread_id,
			review_token,
			profiles,
			can_update: true,
			..
		} = self.permission_profiles.state.as_ref()?
		else {
			return None;
		};
		if reviewed_work != work_id
			|| thread_id.as_str() != thread
			|| self.permission_profiles.work.as_deref() != Some(work_id.as_str())
			|| !profiles.iter().any(|p| p.id == profile_id && p.allowed && p.can_select)
		{
			return None;
		}
		Some(ChiefActionDto::SelectPermissions {
			work_id: work_id.clone(),
			thread_id: thread_id.clone(),
			review_token: review_token.clone(),
			profile_id,
		})
	}

	fn update_permission_profiles(
		&mut self,
		work: String,
		profile_id: Option<WireText>,
		cx: &mut Context<Self>,
	) {
		if self.permission_profiles.task.is_some()
			|| self.selected.as_ref() != Some(&work)
			|| !self.command_connection_ready()
			|| self.native_agents.selected.is_some()
			|| (profile_id.is_some() && !self.permission_profiles.reviewed)
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
		let action = if let Some(profile_id) = profile_id {
			let Some(action) = self.permission_selection(&work_id, &thread, profile_id) else {
				return;
			};
			Some(action)
		} else {
			None
		};
		let saving = action.is_some();
		let generation = self.generation;
		self.permission_profiles.epoch = self.permission_profiles.epoch.wrapping_add(1);
		let epoch = self.permission_profiles.epoch;
		self.permission_profiles.work = Some(work.clone());
		self.permission_profiles.state = None;
		self.permission_profiles.reviewed = false;
		self.permission_profiles.feedback = if saving {
			"Submitting permission selection…"
		} else {
			"Reading native permission profiles…"
		}
		.into();
		let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state =
				runtime.block_on(client.permission_profiles(work_id)).unwrap_or(State::Unavailable);
			Some((outcome, state))
		});
		self.permission_profiles.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation || s.permission_profiles.epoch != epoch {
					return;
				}
				s.permission_profiles.task = None;
				let current = s.command_connection_ready()
					&& s.native_agents.selected.is_none()
					&& s.selected.as_ref() == Some(&work)
					&& s.snapshot.as_ref().is_some_and(|snapshot| {
						snapshot.runtime_source.as_ref() == Some(&source)
							&& snapshot.work_items.iter().any(|w| {
								w.id == work && w.codex_thread_id.as_deref() == Some(&thread)
							})
					});
				if !current {
					s.reset_permission_profiles();
					cx.notify();
					return;
				}
				let (outcome, state) = result.unwrap_or((None, State::Unavailable));
				s.permission_profiles.reviewed = !saving;
				s.permission_profiles.feedback = match outcome {
					Some(Ok(ChiefCommandResponse::Accepted { .. })) =>
						"Selection queued. The native state below determines whether it took effect.",
					Some(Ok(ChiefCommandResponse::Rejected { .. })) =>
						"Selection was not accepted. Refresh the permissions before choosing again.",
					Some(_) => "Selection could not be confirmed. It was not retried.",
					None if saving => "Selection could not be confirmed. Refresh its saved state.",
					None => "Profiles apply to this task. Native policy controls availability.",
				}
				.into();
				s.permission_profiles.state = Some(match state {
					State::Available { ref work_id, ref thread_id, .. }
						if work_id.as_str() != work || thread_id.as_str() != thread =>
						State::Unavailable,
					other => other,
				});
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn permission_profiles_panel(
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
		let mut panel =
			div().flex().flex_col().gap_2().child("Task permissions").child(mcp_button(
				"permission-profiles-read".into(),
				"Review / refresh permissions".into(),
				false,
				cx,
				move |s, cx| s.update_permission_profiles(owner.clone(), None, cx),
			));
		if self.permission_profiles.work.as_ref() != Some(&work.id) {
			return panel.into_any_element();
		}
		panel = panel.child(self.permission_profiles.feedback.clone());
		match &self.permission_profiles.state {
			Some(State::Available {
				cwd, profile_id, profiles, can_update, last_outcome, ..
			}) => {
				panel = panel
					.child(format!(
						"Configured profile: {}",
						profile_id.as_ref().map_or("Unnamed native policy", |id| id.as_str())
					))
					.child(format!("Directory: {}", cwd.as_str()))
					.child("An active step may still use the permissions it started with.");
				if let Some(state) = last_outcome {
					panel = panel.child(format!("Last selection: {}", label(*state)));
				}
				if !can_update {
					panel =
						panel.child("Permissions cannot change while the task is changing state.");
				}
				for (index, p) in profiles.iter().enumerate() {
					let description = p
						.description
						.as_ref()
						.map(|d| format!(" — {}", d.as_str()))
						.unwrap_or_default();
					let text = format!("{}{description}", p.id.as_str());
					if !p.allowed {
						panel = panel.child(format!("{text} · Unavailable by policy"));
					} else if profile_id.as_ref() == Some(&p.id) {
						panel = panel.child(format!("{text} · Configured"));
					} else if !p.can_select {
						panel =
							panel.child(format!("{text} · Unavailable in the current task state"));
					} else if *can_update
						&& self.permission_profiles.task.is_none()
						&& self.permission_profiles.reviewed
					{
						let (owner, id) = (work.id.clone(), p.id.clone());
						panel = panel.child(mcp_button(
							format!("permission-profile-{index}"),
							text,
							false,
							cx,
							move |s, cx| {
								s.update_permission_profiles(owner.clone(), Some(id.clone()), cx)
							},
						));
					} else {
						panel = panel.child(text);
					}
				}
			},
			Some(State::Pending { profile_id, state }) => {
				panel = panel
					.child(format!("{}: {}", profile_id.as_str(), label(*state)))
					.child("Refresh to check the native state. The selection will not be resent.");
			},
			Some(State::Unsupported) => {
				panel = panel.child("This Codex version does not provide permission profiles.");
			},
			Some(State::Unavailable) => {
				panel = panel.child(
					"Current native permissions are unavailable. Refresh after the task reconnects.",
				);
			},
			None => {},
		}
		panel.into_any_element()
	}
}
fn label(state: Outcome) -> &'static str {
	match state {
		Outcome::Reserved => "Awaiting confirmation",
		Outcome::Queued => "Queued; awaiting native state",
		Outcome::Unknown => "Unconfirmed",
		Outcome::Rejected => "Rejected",
		Outcome::TargetObserved => "Target profile observed",
		Outcome::Superseded => "Replaced by current native permissions",
	}
}

#[cfg(test)]
#[path = "chief_permissions_wire_tests.rs"]
mod wire_tests;
