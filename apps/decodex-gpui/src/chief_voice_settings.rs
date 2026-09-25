//! Select the next call's voice without changing a live audio session.
use super::{mcp_forms::mcp_button, *};
use decodex_protocol::ChiefVoiceSettingsResult as State;

#[derive(Default)]
pub(super) struct Panel {
	work: Option<String>,
	state: Option<State>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
}

impl ChiefSurface {
	pub(super) fn invalidate_voice_settings(&mut self, next: &ChiefSnapshotDto) {
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
			Some(ChiefActionDto::SetVoicePreference {
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
		let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let query_owner = owner.clone();
		let future = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
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

	pub(super) fn voice_settings_panel(
		&self,
		work: &str,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		if self.native_agents.selected.is_some() {
			return div().into_any_element();
		}
		let owner = work.to_owned();
		let opened = self.voice_settings.work.as_deref() == Some(work);
		let mut panel = div().flex().flex_col().gap_2().child(mcp_button(
			"voice-settings-toggle".into(),
			"Voice settings".into(),
			opened,
			cx,
			move |s, cx| {
				if s.voice_settings.work.as_deref() == Some(&owner) {
					s.reset_voice_settings();
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
		panel = panel.child(mcp_button(
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
				panel = panel.child(mcp_button(
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
		panel.into_any_element()
	}
}

fn feedback(
	voice: Option<&WireText>,
	outcome: Option<&Result<ChiefCommandResponse, decodex_protocol::ClientFailure>>,
	state: &State,
) -> &'static str {
	match (voice, outcome, state) {
		(
			Some(voice),
			Some(Ok(ChiefCommandResponse::Accepted { .. })),
			State::Available { effective, preference, .. },
		) if preference.as_ref() == Some(voice) =>
			if effective.as_ref() == Some(voice) {
				"Voice saved for your next conversation."
			} else {
				"Voice saved. This project's settings select a different voice."
			},
		(Some(_), Some(Ok(ChiefCommandResponse::Rejected { .. })), _) =>
			"The change was not accepted. Review the refreshed settings.",
		(Some(_), _, _) =>
			"The save could not be confirmed. Review the refreshed settings before trying again.",
		(None, _, _) => "Choose a voice for future conversations.",
	}
}

#[cfg(test)]
#[path = "chief_voice_settings_wire_tests.rs"]
mod tests;
