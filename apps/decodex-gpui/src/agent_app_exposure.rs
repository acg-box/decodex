//! Connector-level visibility preferences, separate from account approval settings.
#[cfg(test)] use gpui::px;
use gpui::{AnyElement, Div, Stateful};
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::AgentDispatchStateDto;
#[cfg(test)] use crate::shell::agent_surface::AgentSnapshotResult;
#[cfg(test)] use crate::shell::agent_surface::AgentWorkItemDto;
#[cfg(test)] use crate::shell::agent_surface::AgentWorkStatusDto;
#[cfg(test)] use crate::shell::agent_surface::ClientProfile;
#[cfg(test)] use crate::shell::agent_surface::Entity;
#[cfg(test)] use crate::shell::agent_surface::Render;
#[cfg(test)] use crate::shell::agent_surface::Window;
use crate::shell::agent_surface::{
	self, AgentActionDto, AgentClient, AgentCommandResponse, AgentSnapshotDto, AgentSurface,
	Context, EntityId, IdempotencyKey, InteractiveElement, IntoElement, ParentElement, Styled,
	Task, WireText, mcp_forms,
};
use decodex_protocol::{AgentAppExposureResult, AgentToolExposureSurface};

#[derive(Default)]
pub(super) struct Panel {
	owner: Option<(String, String)>,
	state: Option<AgentAppExposureResult>,
	draft: Option<Vec<AgentToolExposureSurface>>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
}

impl AgentSurface {
	pub(super) fn reset_app_exposure(&mut self) {
		self.app_exposure =
			Panel { epoch: self.app_exposure.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_app_exposure(&mut self, next: &AgentSnapshotDto) {
		let Some((work, _)) = &self.app_exposure.owner else {
			return;
		};
		let old = self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let new = next.work_items.iter().find(|w| &w.id == work);

		if !matches!((old,new),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id)
			|| self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
		{
			self.reset_app_exposure();
		}
	}

	pub(super) fn update_app_exposure(
		&mut self,
		work: &str,
		connector: &str,
		save: bool,
		cx: &mut Context<Self>,
	) {
		if self.selected.as_deref() != Some(work) || self.app_exposure.task.is_some() {
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
		let (Ok(work_id), Ok(connector_id)) = (EntityId::new(work), WireText::new(connector))
		else {
			return;
		};
		let action = if save {
			let Some(action) = self.app_exposure_edit(&work_id, &connector_id) else {
				return;
			};

			Some(action)
		} else {
			None
		};

		self.app_exposure.epoch = self.app_exposure.epoch.wrapping_add(1);

		let epoch = self.app_exposure.epoch;

		self.app_exposure.owner = Some((work.into(), connector.into()));
		self.app_exposure.state = None;
		self.app_exposure.feedback =
			if save { "Saving app settings…" } else { "Reading app settings…" }.into();

		let key =
			IdempotencyKey::new(agent_surface::unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			let client = AgentClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state = runtime
				.block_on(client.app_tool_exposure(work_id, connector_id))
				.unwrap_or(AgentAppExposureResult::Unavailable);

			Some((outcome, state))
		});
		let work = work.to_owned();

		self.app_exposure.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.app_exposure.epoch != epoch {
					return;
				}

				s.app_exposure.task = None;

				if s.selected.as_ref() != Some(&work)
					|| s.snapshot.as_ref().is_none_or(|v| {
						v.runtime_source.as_ref() != Some(&source)
							|| !v.work_items.iter().any(|w| {
								w.id == work && w.codex_thread_id.as_ref() == Some(&thread)
							})
					}) {
					s.reset_app_exposure();
					cx.notify();

					return;
				}

				let (outcome, state) =
					result.unwrap_or((None, AgentAppExposureResult::Unavailable));

				s.app_exposure.feedback = match outcome {
					Some(Ok(AgentCommandResponse::Accepted { .. })) =>
						"Saved. Running tools keep their existing settings.",
					Some(Ok(AgentCommandResponse::Rejected { .. })) =>
						"Change rejected. Refresh and try again.",
					Some(_) => "Save unconfirmed. Check current settings before retrying.",
					None if save => "Save unconfirmed. Refresh before retrying.",
					None => "Shared by conversations using this config.",
				}
				.into();

				if let AgentAppExposureResult::Available { preference, .. } = &state {
					s.app_exposure.draft = known(preference).flatten();
				}

				s.app_exposure.state = Some(state);

				cx.notify();
			});
		}));

		cx.notify();
	}

	pub(super) fn app_exposure_panel(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		let Some((owner, connector)) = &self.app_exposure.owner else {
			return gpui::div().into_any_element();
		};

		if owner != work {
			return gpui::div().into_any_element();
		}

		let (owner, app) = (owner.clone(), connector.clone());
		let mut panel = self.app_exposure_header(owner, app, connector, cx);
		let Some(AgentAppExposureResult::Available {
			effective,
			preference,
			can_update,
			last_outcome,
			..
		}) = &self.app_exposure.state
		else {
			if self.app_exposure.task.is_some() {
				return panel.into_any_element();
			}

			return panel.child("Settings unavailable. Refresh to retry.").into_any_element();
		};

		panel = panel
			.child(format!("Saved: {}", description(preference)))
			.child(format!("Effective: {}", description(effective)));

		if effective != preference && preference.is_some() {
			panel = panel.child("Overridden by another configuration layer.");
		}

		if let Some(outcome) = last_outcome {
			panel = panel.child(format!(
				"Last save: {}",
				match outcome.as_str() {
					"saved" => "Saved",
					"overridden" => "Saved; overridden by another configuration layer",
					"target_observed" => "Saved value confirmed",
					"superseded" => "Settings changed after saving",
					"reserved" | "unknown" => "Unconfirmed; no automatic retry",
					"rejected" => "Not sent",
					_ => "Unknown outcome",
				}
			));
		}

		if !can_update || self.app_exposure.task.is_some() {
			return panel.child("Refresh settings to continue.").into_any_element();
		}
		if known(preference).is_none() || known(effective).is_none() {
			return panel
				.child("These restrictions cannot be edited in this Decodex version.")
				.into_any_element();
		}

		panel = self.app_exposure_reset_controls(panel, cx);

		for (index, surface, label) in [
			(0, AgentToolExposureSurface::Direct, "Hide from initial tools"),
			(1, AgentToolExposureSurface::Deferred, "Hide from tool search"),
			(2, AgentToolExposureSurface::CodeMode, "Hide from Code Mode"),
		] {
			let inherited = known(effective).flatten().unwrap_or_default();
			let selected =
				self.app_exposure.draft.as_ref().unwrap_or(&inherited).contains(&surface);

			panel = panel.child(mcp_forms::mcp_button(
				format!("app-exposure-surface-{index}"),
				label.into(),
				selected,
				cx,
				move |s, cx| {
					let values = s.app_exposure.draft.get_or_insert_with(|| inherited.clone());

					if values.contains(&surface) {
						values.retain(|v| v != &surface);
					} else {
						values.push(surface);
					}

					cx.notify();
				},
			));
		}

		let changed = known(preference) != Some(self.app_exposure.draft.clone());

		if changed {
			let (owner, app) = (work.to_owned(), connector.clone());

			panel = panel.child(mcp_forms::mcp_button(
				"app-exposure-save".into(),
				"Save settings".into(),
				false,
				cx,
				move |s, cx| s.update_app_exposure(&owner, &app, true, cx),
			));
		}

		panel
			.child(
				"Visibility only. App connections, approvals and server restrictions stay unchanged.",
			)
			.into_any_element()
	}
}

impl AgentSurface {
	fn app_exposure_edit(
		&self,
		work_id: &EntityId,
		connector_id: &WireText,
	) -> Option<AgentActionDto> {
		let Some(AgentAppExposureResult::Available {
			work_id: actual,
			connector_id: app,
			review_token,
			can_update: true,
			preference,
			..
		}) = &self.app_exposure.state
		else {
			return None;
		};

		if actual != work_id
			|| app != connector_id
			|| known(preference).is_none()
			|| known(preference) == Some(self.app_exposure.draft.clone())
		{
			return None;
		}

		Some(AgentActionDto::SetAppToolExposure {
			work_id: work_id.clone(),
			connector_id: connector_id.clone(),
			review_token: review_token.clone(),
			omit: self.app_exposure.draft.clone(),
		})
	}

	fn app_exposure_header(
		&self,
		owner: String,
		app: String,
		connector: &str,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let panel = gpui::div()
			.id("app-exposure-panel")
			.flex()
			.flex_col()
			.gap_2()
			.child(format!("Tool visibility · {connector}"))
			.child(self.app_exposure.feedback.clone());

		panel.child(mcp_forms::mcp_button(
			"app-exposure-refresh".into(),
			"Refresh settings".into(),
			false,
			cx,
			move |s, cx| s.update_app_exposure(&owner, &app, false, cx),
		))
	}

	fn app_exposure_reset_controls(
		&self,
		mut panel: Stateful<Div>,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		panel = panel.child(mcp_forms::mcp_button(
			"app-exposure-inherit".into(),
			"Use inherited settings".into(),
			self.app_exposure.draft.is_none(),
			cx,
			|s, cx| {
				s.app_exposure.draft = None;

				cx.notify();
			},
		));
		panel = panel.child(mcp_forms::mcp_button(
			"app-exposure-clear".into(),
			"Show in all tool surfaces".into(),
			self.app_exposure.draft == Some(vec![]),
			cx,
			|s, cx| {
				s.app_exposure.draft = Some(vec![]);

				cx.notify();
			},
		));

		panel
	}
}

fn known(values: &Option<Vec<String>>) -> Option<Option<Vec<AgentToolExposureSurface>>> {
	match values {
		None => Some(None),
		Some(values) => values
			.iter()
			.map(|v| match v.as_str() {
				"code_mode" => Some(AgentToolExposureSurface::CodeMode),
				"deferred" => Some(AgentToolExposureSurface::Deferred),
				"direct" => Some(AgentToolExposureSurface::Direct),
				_ => None,
			})
			.collect::<Option<Vec<_>>>()
			.map(Some),
	}
}

fn description(values: &Option<Vec<String>>) -> String {
	match values {
		None => "Inherited".into(),
		Some(v) if v.is_empty() => "Visible in all tool surfaces".into(),
		Some(v) => v.join(", "),
	}
}

#[cfg(test)]
#[path = "agent_app_exposure_tests.rs"]
mod tests;
#[cfg(test)]
#[path = "agent_app_exposure_wire_tests.rs"]
mod wire_tests;
