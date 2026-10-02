//! Select the next call's voice without changing a live audio session.
use gpui::{AnyElement, AppContext as _};
use tokio::runtime::Builder;

use crate::shell::agent_surface::{
	self, AgentActionDto, AgentClient, AgentCommandResponse, AgentSnapshotDto, AgentSurface,
	ComposerInput, Context, Entity, EntityId, IdempotencyKey, IntoElement, ParentElement, Styled,
	Task, WireText, mcp_forms,
};
#[cfg(test)] use crate::shell::agent_surface::{ClientProfile, Render, Window};
use decodex_protocol::{AgentVoiceSettingsResult as State, ClientFailure, HistoryText};

#[derive(Default)]
pub(super) struct Panel {
	next: Option<NextCall>,
	work: Option<String>,
	state: Option<State>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
}

struct NextCall {
	target: (String, String, Option<EntityId>),
	model: Entity<ComposerInput>,
	start: Entity<ComposerInput>,
	end: Entity<ComposerInput>,
}

impl AgentSurface {
	fn voice_option_target(&self, work: &str) -> Option<(String, String, Option<EntityId>)> {
		if self.composer_manager.clone().or_else(|| self.root_id()).as_deref() != Some(work) {
			return None;
		}

		let snapshot = self.snapshot.as_ref()?;
		let item = snapshot.work_items.iter().find(|item| item.id == work)?;

		Some((work.into(), item.codex_thread_id.clone()?, snapshot.runtime_source.clone()))
	}

	pub(super) fn voice_call_options(
		&self,
		work: &str,
		cx: &Context<Self>,
	) -> Result<decodex_protocol::AgentVoiceOptions, &'static str> {
		let Some(next) = self
			.voice_settings
			.next
			.as_ref()
			.filter(|next| Some(&next.target) == self.voice_option_target(work).as_ref())
		else {
			return Ok(Default::default());
		};
		let value = |input: &Entity<ComposerInput>| {
			let value = input.read(cx).content().trim().to_owned();

			(!value.is_empty()).then_some(value)
		};

		Ok(decodex_protocol::AgentVoiceOptions {
			model: value(&next.model)
				.map(WireText::new)
				.transpose()
				.map_err(|_| "The realtime model name is too long.")?,
			start_instructions: value(&next.start)
				.map(HistoryText::new)
				.transpose()
				.map_err(|_| "The voice start instructions are too long.")?,
			end_instructions: value(&next.end)
				.map(HistoryText::new)
				.transpose()
				.map_err(|_| "The voice end instructions are too long.")?,
		})
	}

	fn advanced_voice_options(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		if self.voice_option_target(work).is_none() {
			return gpui::div().into_any_element();
		}

		let owner = work.to_owned();
		let active = self
			.voice_settings
			.next
			.as_ref()
			.is_some_and(|next| Some(&next.target) == self.voice_option_target(work).as_ref());
		let mut panel = gpui::div().flex().flex_col().gap_2().child(mcp_forms::mcp_button(
			"voice-call-options".into(),
			if active { "Clear call overrides" } else { "Advanced call options" }.into(),
			active,
			cx,
			move |s, cx| {
				if active {
					s.voice_settings.next = None;
				} else if let Some(target) = s.voice_option_target(&owner) {
					s.voice_settings.next = Some(NextCall {
						target,
						model: cx.new(|cx| {
							ComposerInput::with_placeholder(
								0,
								"Configured voice model",
								"Realtime model override",
								cx,
							)
						}),
						start: cx.new(|cx| {
							ComposerInput::with_placeholder(
								0,
								"Default start behavior",
								"Voice start instructions",
								cx,
							)
						}),
						end: cx.new(|cx| {
							ComposerInput::with_placeholder(
								0,
								"Default end behavior",
								"Voice end instructions",
								cx,
							)
						}),
					});
				}

				cx.notify();
			},
		));

		if active && let Some(next) = &self.voice_settings.next {
			panel=panel.child("Applies when you start a call. Blank fields use configured defaults. Changes do not affect an active call.")
                .child("Realtime model").child(next.model.clone())
                .child("Instructions for the Agent when voice starts").child(gpui::div().h(gpui::px(90.)).child(next.start.clone()))
                .child("Instructions for the Agent when voice ends").child(gpui::div().h(gpui::px(90.)).child(next.end.clone()));
		}

		panel.into_any_element()
	}

	pub(super) fn invalidate_voice_settings(&mut self, next: &AgentSnapshotDto) {
		let Some(work) = &self.voice_settings.work else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);

		if self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
			|| !matches!((before, after), (Some(a), Some(b)) if a.codex_thread_id == b.codex_thread_id)
		{
			self.reset_voice_settings();
		}
	}

	pub(super) fn reset_voice_settings(&mut self) {
		self.voice_settings =
			Panel { epoch: self.voice_settings.epoch.wrapping_add(1), ..Default::default() };
	}

	fn update_voice_settings(
		&mut self,
		work: &str,
		voice: Option<WireText>,
		cx: &mut Context<Self>,
	) {
		if self.selected.as_deref() != Some(work)
			|| self.voice_settings.task.is_some()
			|| self.native_agents.selected.is_some()
		{
			return;
		}

		let Some(profile) = self.profile.clone() else {
			self.voice_settings.feedback = "No service connection is available.".into();

			cx.notify();

			return;
		};
		let Ok(owner) = EntityId::new(work) else { return };
		let action = if let Some(voice) = voice.clone() {
			let Some(State::Available { work_id, review_token, voices, .. }) =
				&self.voice_settings.state
			else {
				return;
			};

			if work_id != &owner || !voices.contains(&voice) {
				return;
			}

			Some(AgentActionDto::SetVoicePreference {
				work_id: owner.clone(),
				review_token: review_token.clone(),
				voice,
			})
		} else {
			None
		};

		self.voice_settings.work = Some(work.into());
		self.voice_settings.epoch = self.voice_settings.epoch.wrapping_add(1);

		let epoch = self.voice_settings.epoch;
		let generation = self.generation;

		self.voice_settings.state = None;
		self.voice_settings.feedback =
			if voice.is_some() { "Saving voice…" } else { "Reading voice settings…" }.into();

		let key =
			IdempotencyKey::new(agent_surface::unique_command()).expect("bounded command identity");
		let query_owner = owner.clone();
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state =
				runtime.block_on(client.voice_settings(query_owner)).unwrap_or(State::Unavailable);

			Some((outcome, state))
		});

		self.voice_settings.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation
					|| s.voice_settings.epoch != epoch
					|| s.selected.as_deref() != Some(owner.as_str())
				{
					return;
				}

				s.voice_settings.task = None;

				let (outcome, state) = result.unwrap_or((None, State::Unavailable));

				s.voice_settings.feedback =
					feedback(voice.as_ref(), outcome.as_ref(), &state).into();
				s.voice_settings.state = Some(state);

				cx.notify();
			});
		}));

		cx.notify();
	}

	pub(super) fn voice_settings_panel(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		if self.native_agents.selected.is_some() {
			return gpui::div().into_any_element();
		}

		let owner = work.to_owned();
		let opened = self.voice_settings.work.as_deref() == Some(work);
		let mut panel = gpui::div().flex().flex_col().gap_2().child(mcp_forms::mcp_button(
			"voice-settings-toggle".into(),
			"Voice settings".into(),
			opened,
			cx,
			move |s, cx| {
				if s.voice_settings.work.as_deref() == Some(&owner) {
					let next = s.voice_settings.next.take();

					s.reset_voice_settings();

					s.voice_settings.next = next;

					cx.notify();
				} else {
					s.update_voice_settings(&owner, None, cx);
				}
			},
		));

		if !opened {
			return panel.into_any_element();
		}

		panel = panel
			.child("Applies to your next voice conversation.")
			.child(self.voice_settings.feedback.clone());

		if self.voice_settings.task.is_some() {
			return panel.into_any_element();
		}

		let owner = work.to_owned();

		panel = panel.child(mcp_forms::mcp_button(
			"voice-settings-refresh".into(),
			"Refresh voices".into(),
			false,
			cx,
			move |s, cx| s.update_voice_settings(&owner, None, cx),
		));

		if let Some(State::Available { voices, effective, preference, .. }) =
			&self.voice_settings.state
		{
			if preference != effective {
				panel = panel.child(format!(
					"Saved preference: {} · Effective here: {}",
					preference.as_ref().map_or("Server default", WireText::as_str),
					effective.as_ref().map_or("Server default", WireText::as_str)
				));
			}

			for (index, voice) in voices.iter().enumerate() {
				let owner = work.to_owned();
				let selected = Some(voice) == effective.as_ref();
				let label =
					format!("{}{}", voice.as_str(), if selected { " (current)" } else { "" });
				let voice = voice.clone();
				let epoch = self.voice_settings.epoch;

				panel = panel.child(mcp_forms::mcp_button(
					format!("voice-choice-{index}"),
					label,
					selected,
					cx,
					move |s, cx| {
						if s.voice_settings.epoch == epoch {
							s.update_voice_settings(&owner, Some(voice.clone()), cx);
						}
					},
				));
			}
		} else {
			panel = panel.child("Voice settings are unavailable. Refresh to try again.");
		}

		panel.child(self.advanced_voice_options(work, cx)).into_any_element()
	}
}

fn feedback(
	voice: Option<&WireText>,
	outcome: Option<&Result<AgentCommandResponse, ClientFailure>>,
	state: &State,
) -> &'static str {
	match (voice, outcome, state) {
		(
			Some(voice),
			Some(Ok(AgentCommandResponse::Accepted { .. })),
			State::Available { effective, preference, .. },
		) if preference.as_ref() == Some(voice) =>
			if effective.as_ref() == Some(voice) {
				"Voice saved for your next conversation."
			} else {
				"Voice saved. This project's settings select a different voice."
			},
		(Some(_), Some(Ok(AgentCommandResponse::Rejected { .. })), _) =>
			"The change was not accepted. Review the refreshed settings.",
		(Some(_), _, _) =>
			"The save could not be confirmed. Review the refreshed settings before trying again.",
		(None, _, _) => "Choose a voice for future conversations.",
	}
}

#[cfg(test)]
#[path = "agent_voice_settings_wire_tests.rs"]
mod tests;
