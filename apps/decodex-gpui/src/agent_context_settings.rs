//! Choose native context defaults without changing loaded conversations.
#[cfg(test)] use gpui::px;
use gpui::{
	AnyElement, Div, KeyDownEvent, Role,
	prelude::{FluentBuilder as _, InteractiveElement as _, StatefulInteractiveElement as _},
};
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::ClientProfile;
#[cfg(test)] use crate::shell::agent_surface::Entity;
#[cfg(test)] use crate::shell::agent_surface::Render;
#[cfg(test)] use crate::shell::agent_surface::Window;
use crate::{
	shell::agent_surface::{
		self, AgentActionDto, AgentClient, AgentCommandResponse, AgentSnapshotDto, AgentSurface,
		Context, EntityId, IdempotencyKey, IntoElement, ParentElement, Styled, Task, mcp_forms,
	},
	ui_feedback::{self, Feedback},
	ui_motion::{self, SmoothControl as _},
	ui_theme,
};
use decodex_protocol::{
	AgentContextFeature, AgentContextPreference, AgentContextSettingsResult, ClientFailure,
};

#[derive(Default)]
pub(super) struct Panel {
	work: Option<String>,
	state: Option<AgentContextSettingsResult>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: Feedback,
	catalog_expanded: bool,
}

impl AgentSurface {
	pub(super) fn invalidate_context_settings(&mut self, next: &AgentSnapshotDto) {
		let Some(work) = &self.context_settings.work else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);

		if self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
			|| !matches!((before, after), (Some(a), Some(b)) if a.codex_thread_id == b.codex_thread_id)
		{
			self.reset_context_settings();
		}
	}

	pub(super) fn reset_context_settings(&mut self) {
		self.context_settings =
			Panel { epoch: self.context_settings.epoch.wrapping_add(1), ..Default::default() };
	}

	fn update_context_settings(
		&mut self,
		work: &str,
		change: Option<(AgentContextFeature, bool)>,
		cx: &mut Context<Self>,
	) {
		if self.selected.as_deref() != Some(work)
			|| self.context_settings.task.is_some()
			|| self.native_agents.selected.is_some()
		{
			return;
		}

		let Some(profile) = self.profile.clone() else {
			self.context_settings.feedback = Feedback::error("No service connection is available.");

			cx.notify();

			return;
		};
		let Ok(owner) = EntityId::new(work) else { return };
		let action = if let Some((feature, enabled)) = &change {
			let Some(AgentContextSettingsResult::Available {
				work_id,
				review_token,
				preferences,
				..
			}) = &self.context_settings.state
			else {
				return;
			};

			if work_id != &owner || !preferences.iter().any(|p| &p.feature == feature) {
				return;
			}

			Some(AgentActionDto::SetContextPreference {
				work_id: owner.clone(),
				review_token: review_token.clone(),
				feature: feature.clone(),
				enabled: *enabled,
			})
		} else {
			None
		};

		if self.context_settings.work.as_deref() != Some(work) {
			self.context_settings.state = None;
		}
		self.context_settings.work = Some(work.into());
		self.context_settings.epoch = self.context_settings.epoch.wrapping_add(1);

		let epoch = self.context_settings.epoch;
		self.context_settings.feedback = if change.is_some() {
			Feedback::loading("Saving context…")
		} else {
			Feedback::default()
		};

		let key =
			IdempotencyKey::new(agent_surface::unique_command()).expect("bounded command identity");
		let query_owner = owner.clone();
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state = runtime
				.block_on(client.context_settings(query_owner))
				.unwrap_or(AgentContextSettingsResult::Unavailable);

			Some((outcome, state))
		});

		self.context_settings.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.context_settings.epoch != epoch {
					return;
				}

				s.context_settings.task = None;
				if s.selected.as_deref() != Some(owner.as_str()) {
					s.context_settings.feedback.clear();
					cx.notify();
					return;
				}

				let (outcome, state) =
					result.unwrap_or((None, AgentContextSettingsResult::Unavailable));

				s.context_settings.feedback = feedback(change.as_ref(), outcome.as_ref(), &state);
				s.context_settings.state = Some(state);

				cx.notify();
			});
		}));

		cx.notify();
	}

	pub(super) fn context_settings_panel(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		if self.native_agents.selected.is_some() {
			return gpui::div().into_any_element();
		}

		let owner = work.to_owned();
		let opened = self.context_settings.work.as_deref() == Some(work);
		let panel = gpui::div().flex().flex_col().gap_2().child(mcp_forms::mcp_button(
			"context-settings-toggle".into(),
			"Context defaults".into(),
			opened,
			cx,
			move |s, cx| {
				if s.context_settings.work.as_deref() == Some(&owner) {
					s.reset_context_settings();
					cx.notify();
				} else {
					s.update_context_settings(&owner, None, cx);
				}
			},
		));

		if !opened {
			return panel.into_any_element();
		}

		panel.child(self.context_settings_controls(work, cx)).into_any_element()
	}

	pub(crate) fn native_defaults_settings(&self, cx: &mut Context<Self>) -> AnyElement {
		let Some(work) = self.selected.as_deref().filter(|_| self.native_agents.selected.is_none())
		else {
			return gpui::div()
				.child("Select a connected Agent to read native defaults.")
				.into_any_element();
		};

		self.context_settings_controls(work, cx)
	}

	pub(crate) fn read_native_defaults(&mut self, cx: &mut Context<Self>) {
		// Settings calls this on entry. Rendering never starts a read.
		if let Some(work) = self.selected.clone() {
			self.update_context_settings(&work, None, cx);
		}
	}

	fn context_settings_controls(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		let busy = self.context_settings.task.is_some();
		let retry = (!busy
			&& matches!(
				self.context_settings.feedback.tone,
				ui_feedback::Tone::Warning | ui_feedback::Tone::Error
			))
		.then(|| {
			let owner = work.to_owned();
			self.workspace_button(
				"context-settings-retry".into(),
				"Retry".into(),
				move |s, cx| s.update_context_settings(&owner, None, cx),
				cx,
			)
		});
		let mut panel = ui_theme::settings_rows()
			.child(ui_theme::settings_description("Shared with Codex for new conversations."))
			.child(
				ui_feedback::notice(
					"context-settings-feedback",
					work.to_owned(),
					Some(self.context_settings.feedback.clone()),
				)
				.inline()
				.action(retry)
				.loading_delay(std::time::Duration::from_millis(350)),
			);

		if self.context_settings.work.as_deref() == Some(work)
			&& let Some(AgentContextSettingsResult::Available { preferences, .. }) =
				&self.context_settings.state
		{
			let mut rows: Vec<_> = preferences
				.iter()
				.filter(|p| !is_catalog(&p.feature))
				.map(|preference| {
					self.context_preference_row(work, preference, busy, cx).into_any_element()
				})
				.collect();
			if preferences.iter().any(|p| is_catalog(&p.feature)) {
				rows.push(
					self.mcp_catalog_settings(work, preferences, busy, cx).into_any_element(),
				);
			}
			panel = panel.child(ui_theme::settings_divided_rows(rows));
		}

		panel.into_any_element()
	}

	fn context_preference_row(
		&self,
		work: &str,
		preference: &AgentContextPreference,
		busy: bool,
		cx: &mut Context<Self>,
	) -> Div {
		let feature = preference.feature.clone();
		let owner = work.to_owned();
		let epoch = self.context_settings.epoch;
		let key = feature_key(&feature);
		let label = feature_label(&feature);
		let notice_id = format!("context-override-{key}");
		let information = ui_theme::settings_label(label.clone(), feature_detail(&feature))
			.when_some(override_notice(preference), |label, notice| {
				label.child(
					ui_theme::settings_description(notice)
						.debug_selector(move || notice_id.clone())
						.text_color(ui_theme::rgb(ui_theme::WARNING)),
				)
			});

		ui_theme::settings_row()
			.when(is_catalog(&feature), |row| row.min_h(gpui::px(36.)).py(gpui::px(6.)))
			.child(information)
			.child(
				crate::settings_surface::settings_switch(
					format!("context-choice-{key}"),
					format!("context-knob-{key}"),
					label,
					preference.preference.unwrap_or(preference.effective),
					!busy,
				)
				.on_change(cx.listener(move |s, enabled: &bool, _, cx| {
					if s.context_settings.epoch == epoch {
						s.update_context_settings(&owner, Some((feature.clone(), *enabled)), cx);
					}
				})),
			)
	}

	fn mcp_catalog_settings(
		&self,
		work: &str,
		preferences: &[AgentContextPreference],
		busy: bool,
		cx: &mut Context<Self>,
	) -> Div {
		let catalogs: Vec<_> = preferences.iter().filter(|p| is_catalog(&p.feature)).collect();
		if catalogs.is_empty() {
			return gpui::div();
		}
		let count = catalogs.len();
		let expanded = self.context_settings.catalog_expanded;
		let toggle = ui_theme::settings_row()
			.id("context-mcp-catalogs")
			.debug_selector(|| "context-mcp-catalogs".into())
			.role(Role::Button)
			.aria_label("Tool catalog cache")
			.aria_expanded(expanded)
			.tab_index(0)
			.cursor_pointer()
			.rounded(gpui::px(4.))
			.hover(|row| row.bg(ui_theme::rgba(ui_theme::HOVER_FILL)))
			.on_click(cx.listener(|s, _, _, cx| s.toggle_catalog_settings(cx)))
			.on_key_down(cx.listener(|s, event: &KeyDownEvent, _, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					s.toggle_catalog_settings(cx);
					cx.stop_propagation();
				}
			}))
			.child(ui_theme::settings_label("Tool catalog cache", ""))
			.child(ui_theme::settings_description(format!(
				"{count} {}",
				if count == 1 { "server" } else { "servers" }
			)))
			.child(crate::shell::workspace_symbols::disclosure_chevron(
				"context-mcp-chevron",
				expanded,
			))
			.smooth();
		let rows = ui_theme::settings_rows()
			.child(ui_theme::settings_description(
				"Use a valid cached tool list while servers connect. Tool calls still need a live connection.",
			))
			.child(ui_theme::settings_divided_rows(
				catalogs
					.into_iter()
					.map(|preference| {
						self.context_preference_row(work, preference, busy, cx).into_any_element()
					})
					.collect(),
			));
		ui_theme::settings_rows().child(toggle).child(ui_motion::disclosure(
			"context-mcp-body",
			expanded,
			ui_theme::settings_detail(rows),
		))
	}

	fn toggle_catalog_settings(&mut self, cx: &mut Context<Self>) {
		self.context_settings.catalog_expanded = !self.context_settings.catalog_expanded;
		cx.notify();
	}

	#[cfg(any(test, feature = "visual-capture"))]
	pub(crate) fn visual_context_settings(&mut self, expanded: bool, cx: &mut Context<Self>) {
		let work = self.selected.clone().expect("selected fixture Agent");
		let mut preferences: Vec<_> =
			[AgentContextFeature::AsyncCompaction, AgentContextFeature::RetainClientToolOutputs]
				.into_iter()
				.map(|feature| AgentContextPreference {
					feature,
					effective: false,
					preference: None,
				})
				.collect();
		preferences.extend(
			["xcode", "node_repl", "computer-use", "openwiki", "brave"].into_iter().map(|server| {
				AgentContextPreference {
					feature: AgentContextFeature::McpCatalogStartup {
						server: decodex_protocol::WireText::new(server).expect("fixture server"),
					},
					effective: server == "brave",
					preference: Some(false),
				}
			}),
		);
		self.context_settings = Panel {
			work: Some(work.clone()),
			state: Some(AgentContextSettingsResult::Available {
				work_id: EntityId::new(work).expect("fixture Agent"),
				review_token: decodex_protocol::WireText::new("visual-native-defaults")
					.expect("fixture review"),
				preferences,
			}),
			catalog_expanded: expanded,
			..Panel::default()
		};
		cx.notify();
	}
}

fn is_catalog(feature: &AgentContextFeature) -> bool {
	matches!(feature, AgentContextFeature::McpCatalogStartup { .. })
}

fn override_notice(preference: &AgentContextPreference) -> Option<String> {
	preference.preference.filter(|saved| *saved != preference.effective).map(|saved| {
		format!("Uses {} here (saved: {})", on_off(preference.effective), on_off(saved))
	})
}

fn feedback(
	change: Option<&(AgentContextFeature, bool)>,
	outcome: Option<&Result<AgentCommandResponse, ClientFailure>>,
	state: &AgentContextSettingsResult,
) -> Feedback {
	if let Some((feature, enabled)) = change {
		if matches!(outcome, Some(Ok(AgentCommandResponse::Accepted { .. })))
			&& let AgentContextSettingsResult::Available { preferences, .. } = state
			&& let Some(AgentContextPreference { effective, preference: Some(saved), .. }) =
				preferences.iter().find(|p| &p.feature == feature)
			&& saved == enabled
		{
			return if effective == enabled {
				Feedback::success("Saved for new conversations.")
			} else {
				Feedback::info("Saved. Overridden by project or managed settings.")
			};
		}

		return Feedback::warning("Could not confirm the saved value.");
	}

	if matches!(state, AgentContextSettingsResult::Available { .. }) {
		Feedback::default()
	} else {
		Feedback::warning("Could not read native defaults.")
	}
}

fn on_off(enabled: bool) -> &'static str {
	if enabled { "On" } else { "Off" }
}
fn feature_key(feature: &AgentContextFeature) -> String {
	match feature {
		AgentContextFeature::AsyncCompaction => "async".into(),
		AgentContextFeature::RetainClientToolOutputs => "retain".into(),
		AgentContextFeature::McpCatalogStartup { server } => format!("mcp-{}", server.as_str()),
	}
}
fn feature_label(feature: &AgentContextFeature) -> String {
	match feature {
		AgentContextFeature::AsyncCompaction => "Background compaction".into(),
		AgentContextFeature::RetainClientToolOutputs => "Retain marked tool outputs".into(),
		AgentContextFeature::McpCatalogStartup { server } => server.as_str().into(),
	}
}
fn feature_detail(feature: &AgentContextFeature) -> &'static str {
	match feature {
		AgentContextFeature::AsyncCompaction =>
			"Experimental. Prepare a summary before context fills. May use extra quota.",
		AgentContextFeature::RetainClientToolOutputs =>
			"Experimental. Keep client-marked outputs after compaction, within Codex limits.",
		AgentContextFeature::McpCatalogStartup { .. } => "",
	}
}

#[cfg(test)]
#[path = "agent_context_settings_wire_tests.rs"]
mod tests;
