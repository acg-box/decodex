//! Choose native search defaults without changing loaded conversations.
use gpui::AnyElement;
use tokio::runtime::Builder;

use crate::shell::agent_surface::{
	self, AgentActionDto, AgentClient, AgentCommandResponse, AgentSnapshotDto, AgentSurface,
	Context, EntityId, IdempotencyKey, IntoElement, ParentElement, Styled, Task, WireText,
	mcp_forms,
};
#[cfg(test)] use crate::shell::agent_surface::{ClientProfile, Entity, Render, Window, px};
use decodex_protocol::{AgentSearchSettingsResult as State, ClientFailure};

#[derive(Default)]
pub(super) struct Panel {
	work: Option<String>,
	state: Option<State>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
}

impl AgentSurface {
	pub(super) fn invalidate_search_settings(&mut self, next: &AgentSnapshotDto) {
		let Some(work) = &self.search_settings.work else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);

		if self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
			|| !matches!((before, after), (Some(a), Some(b)) if a.codex_thread_id == b.codex_thread_id)
		{
			self.reset_search_settings();
		}
	}

	pub(super) fn reset_search_settings(&mut self) {
		self.search_settings =
			Panel { epoch: self.search_settings.epoch.wrapping_add(1), ..Default::default() };
	}

	fn update_search_settings(
		&mut self,
		work: &str,
		mode: Option<WireText>,
		cx: &mut Context<Self>,
	) {
		if self.selected.as_deref() != Some(work)
			|| self.search_settings.task.is_some()
			|| self.native_agents.selected.is_some()
		{
			return;
		}

		let Some(profile) = self.profile.clone() else {
			self.search_settings.feedback = "No service connection is available.".into();

			cx.notify();

			return;
		};
		let Ok(owner) = EntityId::new(work) else { return };
		let action = if let Some(mode) = mode.clone() {
			let Some(State::Available { work_id, review_token, modes, .. }) =
				&self.search_settings.state
			else {
				return;
			};

			if work_id != &owner || !modes.contains(&mode) {
				return;
			}

			Some(AgentActionDto::SetSearchPreference {
				work_id: owner.clone(),
				review_token: review_token.clone(),
				mode,
			})
		} else {
			None
		};

		self.search_settings.work = Some(work.into());
		self.search_settings.epoch = self.search_settings.epoch.wrapping_add(1);

		let epoch = self.search_settings.epoch;
		let generation = self.generation;

		self.search_settings.state = None;
		self.search_settings.feedback =
			if mode.is_some() { "Saving search…" } else { "Reading search settings…" }.into();

		let key =
			IdempotencyKey::new(agent_surface::unique_command()).expect("bounded command identity");
		let query_owner = owner.clone();
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state =
				runtime.block_on(client.search_settings(query_owner)).unwrap_or(State::Unavailable);

			Some((outcome, state))
		});

		self.search_settings.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation
					|| s.search_settings.epoch != epoch
					|| s.selected.as_deref() != Some(owner.as_str())
				{
					return;
				}

				s.search_settings.task = None;

				let (outcome, state) = result.unwrap_or((None, State::Unavailable));

				s.search_settings.feedback =
					feedback(mode.as_ref(), outcome.as_ref(), &state).into();
				s.search_settings.state = Some(state);

				cx.notify();
			});
		}));

		cx.notify();
	}

	pub(super) fn search_settings_panel(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		if self.native_agents.selected.is_some() {
			return agent_surface::div().into_any_element();
		}

		let owner = work.to_owned();
		let opened = self.search_settings.work.as_deref() == Some(work);
		let mut panel =
			agent_surface::div().flex().flex_col().gap_2().child(mcp_forms::mcp_button(
				"search-settings-toggle".into(),
				"Search defaults".into(),
				opened,
				cx,
				move |s, cx| {
					if s.search_settings.work.as_deref() == Some(&owner) {
						s.reset_search_settings();
						cx.notify();
					} else {
						s.update_search_settings(&owner, None, cx);
					}
				},
			));

		if !opened {
			return panel.into_any_element();
		}

		panel = panel
			.child("Shared across accounts in Codex user settings. Applies to new conversations. Project settings can override this default; loaded conversations keep their current mode.")
			.child(self.search_settings.feedback.clone());

		if self.search_settings.task.is_some() {
			return panel.into_any_element();
		}

		let owner = work.to_owned();

		panel = panel.child(mcp_forms::mcp_button(
			"search-settings-refresh".into(),
			"Refresh search settings".into(),
			false,
			cx,
			move |s, cx| s.update_search_settings(&owner, None, cx),
		));

		if let Some(State::Available { modes, effective, preference, .. }) =
			&self.search_settings.state
		{
			panel = panel.child(format!(
				"Project default for new conversations: {}",
				effective.as_ref().map_or("Native default", |mode| mode_label(mode.as_str()))
			));

			if modes.is_empty() {
				panel = panel.child(
					"No selectable modes are available under the current Codex requirements.",
				);
			}
			if preference != effective {
				panel = panel.child(format!(
					"Saved user default: {}",
					preference.as_ref().map_or("Native default", |mode| mode_label(mode.as_str()))
				));
			}

			for (index, mode) in modes.iter().enumerate() {
				let owner = work.to_owned();
				let selected = Some(mode) == effective.as_ref();
				let label = format!(
					"{}{}",
					mode_label(mode.as_str()),
					if selected { " (project default)" } else { "" }
				);
				let mode = mode.clone();
				let epoch = self.search_settings.epoch;

				panel = panel.child(mcp_forms::mcp_button(
					format!("search-choice-{index}"),
					label,
					selected,
					cx,
					move |s, cx| {
						if s.search_settings.epoch == epoch {
							s.update_search_settings(&owner, Some(mode.clone()), cx);
						}
					},
				));
			}
		} else {
			panel = panel.child("Search settings are unavailable. Refresh to try again.");
		}

		panel.into_any_element()
	}
}

fn feedback(
	mode: Option<&WireText>,
	outcome: Option<&Result<AgentCommandResponse, ClientFailure>>,
	state: &State,
) -> &'static str {
	match (mode, outcome, state) {
		(
			Some(mode),
			Some(Ok(AgentCommandResponse::Accepted { .. })),
			State::Available { effective, preference, .. },
		) if preference.as_ref() == Some(mode) =>
			if effective.as_ref() == Some(mode) {
				"Search default saved for new conversations."
			} else {
				"Search saved. This project's settings select a different search mode."
			},
		(Some(_), Some(Ok(AgentCommandResponse::Rejected { .. })), _) =>
			"The change was not accepted. Review the refreshed settings.",
		(Some(_), _, _) =>
			"The save could not be confirmed. Review the refreshed settings before trying again.",
		(None, _, _) => "Choose a search mode for new conversations.",
	}
}

fn mode_label(mode: &str) -> &str {
	match mode {
		"disabled" => "Off",
		"cached" => "Cached results",
		"indexed" => "Indexed web",
		"live" => "Live web",
		other => other,
	}
}

#[cfg(test)]
#[path = "agent_search_settings_wire_tests.rs"]
mod tests;
