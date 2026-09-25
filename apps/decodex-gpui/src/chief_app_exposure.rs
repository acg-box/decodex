//! Connector-level visibility preferences, separate from account approval settings.
use super::{mcp_forms::mcp_button, *};
use decodex_protocol::{ChiefAppExposureResult as State, ChiefToolExposureSurface as Surface};

#[derive(Default)]
pub(super) struct Panel {
	owner: Option<(String, String)>,
	state: Option<State>,
	draft: Option<Vec<Surface>>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
}
fn known(values: &Option<Vec<String>>) -> Option<Option<Vec<Surface>>> {
	match values {
		None => Some(None),
		Some(values) => values
			.iter()
			.map(|v| match v.as_str() {
				"code_mode" => Some(Surface::CodeMode),
				"deferred" => Some(Surface::Deferred),
				"direct" => Some(Surface::Direct),
				_ => None,
			})
			.collect::<Option<Vec<_>>>()
			.map(Some),
	}
}
fn description(values: &Option<Vec<String>>) -> String {
	match values {
		None => "Inherited".into(),
		Some(v) if v.is_empty() => "No App-specific omissions".into(),
		Some(v) => v.join(", "),
	}
}
impl ChiefSurface {
	pub(super) fn reset_app_exposure(&mut self) {
		self.app_exposure =
			Panel { epoch: self.app_exposure.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_app_exposure(&mut self, next: &ChiefSnapshotDto) {
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
			let Some(State::Available {
				work_id: actual,
				connector_id: app,
				review_token,
				can_update: true,
				preference,
				..
			}) = &self.app_exposure.state
			else {
				return;
			};
			if actual != &work_id
				|| app != &connector_id
				|| known(preference).is_none()
				|| known(preference) == Some(self.app_exposure.draft.clone())
			{
				return;
			}
			Some(ChiefActionDto::SetAppToolExposure {
				work_id: work_id.clone(),
				connector_id: connector_id.clone(),
				review_token: review_token.clone(),
				omit: self.app_exposure.draft.clone(),
			})
		} else {
			None
		};
		self.app_exposure.epoch = self.app_exposure.epoch.wrapping_add(1);
		let epoch = self.app_exposure.epoch;
		let generation = self.generation;
		self.app_exposure.owner = Some((work.into(), connector.into()));
		self.app_exposure.state = None;
		self.app_exposure.feedback =
			if save { "Saving App settings…" } else { "Reading App settings…" }.into();
		let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state = runtime
				.block_on(client.app_tool_exposure(work_id, connector_id))
				.unwrap_or(State::Unavailable);
			Some((outcome, state))
		});
		let work = work.to_owned();
		self.app_exposure.task=Some(cx.spawn(async move |surface,cx| {
   let result=future.await;
   let _=surface.update(cx,|s,cx| {
    if s.generation!=generation || s.app_exposure.epoch!=epoch {return;}
    s.app_exposure.task=None;
    if s.selected.as_ref()!=Some(&work) || s.snapshot.as_ref().is_none_or(|v|v.runtime_source.as_ref()!=Some(&source) || !v.work_items.iter().any(|w|w.id==work && w.codex_thread_id.as_ref()==Some(&thread))) {s.reset_app_exposure();cx.notify();return;}
    let (outcome,state)=result.unwrap_or((None,State::Unavailable));
    s.app_exposure.feedback=match outcome {
     Some(Ok(ChiefCommandResponse::Accepted {..}))=>"Preference saved. Current settings are shown below; running tools may retain their previous configuration.",
     Some(Ok(ChiefCommandResponse::Rejected {..}))=>"The edit was not accepted. Refresh the settings before trying again.",
     Some(_)=>"The write is unconfirmed. It was not retried. Check task diagnostics and the current preference below.",
     None if save=>"The write is unconfirmed. Refresh before further action.",
     None=>"This setting affects the App across tasks using this native user configuration.",
    }.into();
    if let State::Available {preference,..}=&state {s.app_exposure.draft=known(preference).flatten();}
    s.app_exposure.state=Some(state);cx.notify();
   });
  }));
		cx.notify();
	}

	pub(super) fn app_exposure_panel(
		&self,
		work: &str,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let Some((owner, connector)) = &self.app_exposure.owner else {
			return div().into_any_element();
		};
		if owner != work {
			return div().into_any_element();
		}
		let mut panel = div()
			.id("app-exposure-panel")
			.flex()
			.flex_col()
			.gap_2()
			.child(format!("Tool visibility · {connector}"))
			.child(self.app_exposure.feedback.clone());
		let (owner, app) = (owner.clone(), connector.clone());
		panel = panel.child(mcp_button(
			"app-exposure-refresh".into(),
			"Refresh App settings".into(),
			false,
			cx,
			move |s, cx| s.update_app_exposure(&owner, &app, false, cx),
		));
		let Some(State::Available { effective, preference, can_update, last_outcome, .. }) =
			&self.app_exposure.state
		else {
			return panel.child("Settings are unavailable or still loading.").into_any_element();
		};
		panel = panel
			.child(format!("Stored preference: {}", description(preference)))
			.child(format!("Effective connector setting: {}", description(effective)));
		if effective != preference && preference.is_some() {
			panel = panel.child("Another configuration layer overrides this preference.");
		}
		if let Some(outcome) = last_outcome {
			panel = panel.child(format!(
				"Last shared configuration write: {}",
				match outcome.as_str() {
					"saved" => "Configuration read back",
					"overridden" => "Saved; another configuration layer takes precedence",
					"target_observed" => "Saved value confirmed after recovery",
					"superseded" => "Configuration changed after the original write",
					"reserved" | "unknown" => "Unconfirmed; no automatic retry",
					"rejected" => "Not sent",
					_ => "Unknown outcome",
				}
			));
		}
		if !can_update || self.app_exposure.task.is_some() {
			return panel.child("Refresh to obtain a current settings review.").into_any_element();
		}
		if known(preference).is_none() || known(effective).is_none() {
			return panel
				.child("This Codex version reports restrictions that this editor does not support.")
				.into_any_element();
		}
		panel = panel.child(mcp_button(
			"app-exposure-inherit".into(),
			"Use inherited settings".into(),
			self.app_exposure.draft.is_none(),
			cx,
			|s, cx| {
				s.app_exposure.draft = None;
				cx.notify();
			},
		));
		panel = panel.child(mcp_button(
			"app-exposure-clear".into(),
			"Clear App-specific omissions".into(),
			self.app_exposure.draft == Some(vec![]),
			cx,
			|s, cx| {
				s.app_exposure.draft = Some(vec![]);
				cx.notify();
			},
		));
		for (index, surface, label) in [
			(0, Surface::Direct, "Hide from initial tools"),
			(1, Surface::Deferred, "Hide from tool search"),
			(2, Surface::CodeMode, "Hide from Code Mode"),
		] {
			let inherited = known(effective).flatten().unwrap_or_default();
			let selected =
				self.app_exposure.draft.as_ref().unwrap_or(&inherited).contains(&surface);
			panel = panel.child(mcp_button(
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
			panel = panel.child(mcp_button(
				"app-exposure-save".into(),
				"Save App settings".into(),
				false,
				cx,
				move |s, cx| s.update_app_exposure(&owner, &app, true, cx),
			));
		}
		panel
			.child(
				"Server restrictions still apply. This does not change tool approvals or disconnect the App.",
			)
			.into_any_element()
	}
}

#[cfg(test)]
#[path = "chief_app_exposure_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "chief_app_exposure_wire_tests.rs"]
mod wire_tests;
