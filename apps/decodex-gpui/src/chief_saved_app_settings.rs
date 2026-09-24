//! Explicit management of saved native connection overrides, independent of tool approvals.
use super::{mcp_forms::mcp_button, *};
use decodex_protocol::{ChiefAppSettingEdit as Edit, ChiefSavedAppSettingsResult as State};
#[derive(Default)]
pub(super) struct Panel {
	work: Option<String>,
	state: Option<State>,
	task: Option<Task<()>>,
	epoch: u64,
	reviewed: bool,
	expanded: Option<(String, String)>,
	feedback: String,
}
impl ChiefSurface {
	pub(super) fn reset_saved_app_settings(&mut self) {
		self.saved_app_settings =
			Panel { epoch: self.saved_app_settings.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_saved_app_settings(&mut self, next: &ChiefSnapshotDto) {
		let Some(work) = &self.saved_app_settings.work else { return };
		let before =
			self.snapshot.as_ref().and_then(|s| s.work_items.iter().find(|w| &w.id == work));
		let after = next.work_items.iter().find(|w| &w.id == work);
		if self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
			|| !matches!((before,after),(Some(a),Some(b)) if a.codex_thread_id==b.codex_thread_id && a.active_turn_id==b.active_turn_id && a.dispatch_state==b.dispatch_state && a.status==b.status)
		{
			self.reset_saved_app_settings();
		}
	}

	fn saved_app_action(
		&self,
		work: &str,
		thread: &str,
		selection: (String, String, Edit),
	) -> Option<ChiefActionDto> {
		let State::Available { work_id, thread_id, connections, can_update: true, .. } =
			self.saved_app_settings.state.as_ref()?
		else {
			return None;
		};
		if work_id != work
			|| thread_id != thread
			|| self.saved_app_settings.work.as_deref() != Some(work)
			|| !self.saved_app_settings.reviewed
		{
			return None;
		}
		let (connector, link, edit) = selection;
		let row = connections.iter().find(|r| r.connector_id == connector && r.link_id == link)?;
		let (field, value) = edit.native_value();
		if (if field == "approvals_reviewer" {
			row.user_reviewer.as_deref()
		} else {
			row.user_mode.as_deref()
		}) == value
		{
			return None;
		}
		Some(ChiefActionDto::SetSavedAppSetting {
			work_id: EntityId::new(work).ok()?,
			thread_id: EntityId::new(thread).ok()?,
			connector_id: WireText::new(connector).ok()?,
			link_id: WireText::new(link).ok()?,
			review_token: WireText::new(row.review_token.clone()).ok()?,
			edit,
		})
	}

	fn update_saved_app_settings(
		&mut self,
		work: String,
		selection: Option<(String, String, Edit)>,
		cx: &mut Context<Self>,
	) {
		if self.saved_app_settings.task.is_some()
			|| self.selected.as_ref() != Some(&work)
			|| !self.command_connection_ready()
			|| self.native_agents.selected.is_some()
		{
			return;
		}
		let Some(profile) = self.profile.clone() else { return };
		let Some(snapshot) = &self.snapshot else { return };
		let Some(source) = snapshot.runtime_source.clone() else { return };
		let Some(thread) = snapshot
			.work_items
			.iter()
			.find(|w| w.id == work)
			.and_then(|w| w.codex_thread_id.clone())
		else {
			return;
		};
		let action = match selection {
			Some(s) => {
				let Some(action) = self.saved_app_action(&work, &thread, s) else { return };
				Some(action)
			},
			None => None,
		};
		let Ok(work_id) = EntityId::new(work.clone()) else { return };
		let saving = action.is_some();
		let generation = self.generation;
		self.saved_app_settings.epoch = self.saved_app_settings.epoch.wrapping_add(1);
		let epoch = self.saved_app_settings.epoch;
		self.saved_app_settings.work = Some(work.clone());
		self.saved_app_settings.state = None;
		self.saved_app_settings.reviewed = false;
		self.saved_app_settings.feedback = if saving {
			"Saving connection override…"
		} else {
			"Reading saved connection overrides…"
		}
		.into();
		let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state =
				runtime.block_on(client.saved_app_settings(work_id)).unwrap_or(State::Unavailable);
			Some((state, outcome))
		});
		self.saved_app_settings.task=Some(cx.spawn(async move |surface,cx| {
            let result=future.await;
            let _=surface.update(cx,|s,cx| {
                if s.generation!=generation || s.saved_app_settings.epoch!=epoch {return}
                s.saved_app_settings.task=None;
                if s.selected.as_ref()!=Some(&work) || s.native_agents.selected.is_some()
                    || s.snapshot.as_ref().is_none_or(|v|v.runtime_source.as_ref()!=Some(&source) || !v.work_items.iter().any(|w|w.id==work && w.codex_thread_id.as_ref()==Some(&thread))) {
                    s.reset_saved_app_settings();cx.notify();return
                }
                let (state,outcome)=result.unwrap_or((State::Unavailable,None));
                s.saved_app_settings.reviewed = !saving;
                s.saved_app_settings.feedback=match outcome {
                    Some(Ok(ChiefCommandResponse::Accepted{..}))=>"Native save acknowledged. Read settings to review another edit.",
                    Some(Ok(ChiefCommandResponse::Rejected{..}))=>"The edit was rejected. Read and review the current settings.",
                    Some(_)=>"The result is unconfirmed. Inspect the native configuration and receipt below; the edit will not be resent.",
                    None if saving=>"The result could not be confirmed. Read current settings before further action.",
                    None=>"Saved overrides are shared by tasks using this native configuration.",
                }.into();
                s.saved_app_settings.state=Some(state);cx.notify();
            });
        }));
		cx.notify();
	}

	pub(super) fn saved_app_settings_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		if work.codex_thread_id.is_none() || self.native_agents.selected.is_some() {
			return div().into_any_element();
		}
		let owner = work.id.clone();
		let mut panel = div()
			.id("saved-app-settings")
			.flex()
			.flex_col()
			.gap_2()
			.child("Saved connection overrides")
			.child(mcp_button(
				"saved-app-settings-read".into(),
				"Review saved connection settings".into(),
				self.saved_app_settings.task.is_some(),
				cx,
				move |s, cx| s.update_saved_app_settings(owner.clone(), None, cx),
			));
		if self.saved_app_settings.work.as_ref() != Some(&work.id) {
			return panel.into_any_element();
		}
		panel = panel.child(self.saved_app_settings.feedback.clone());
		match &self.saved_app_settings.state {
			Some(State::Available { config_file, connections, can_update, last_edit, .. }) => {
				panel = panel.child(format!("Shared configuration: {config_file}"));
				if let Some(r) = last_edit {
					panel = panel.child(format!(
						"{}: {}. Original task: {}; Codex account: {}.",
						r.target, r.outcome, r.work_id, r.account_id
					));
				}
				if connections.is_empty() {
					panel = panel.child("No connection approval or reviewer overrides are saved.");
				}
				for (index, row) in connections.iter().enumerate() {
					let identity = (row.connector_id.clone(), row.link_id.clone());
					let expanded = self.saved_app_settings.expanded.as_ref() == Some(&identity);
					panel = panel.child(mcp_button(
						format!("saved-app-edit-{index}"),
						format!("{} · {}", row.connector_id, row.link_id),
						false,
						cx,
						move |s, cx| {
							s.saved_app_settings.expanded = Some(identity.clone());
							cx.notify();
						},
					));
					if !expanded {
						continue;
					}
					panel = panel
						.child(format!(
							"Saved mode: {}; reviewer: {}",
							app_settings::display(&row.user_mode),
							app_settings::display(&row.user_reviewer)
						))
						.child(format!(
							"Merged mode: {}; reviewer: {}",
							app_settings::display(&row.effective_mode),
							app_settings::display(&row.effective_reviewer)
						))
						.child(
							"Tool and managed policy take precedence. Saving does not answer pending tool requests.",
						);
					if !can_update
						|| !self.saved_app_settings.reviewed
						|| self.saved_app_settings.task.is_some()
					{
						continue;
					}
					for (id, label, edit) in app_settings::choices() {
						let (field, value) = edit.native_value();
						let current = if field == "approvals_reviewer" {
							&row.user_reviewer
						} else {
							&row.user_mode
						};
						let (app, link, owner) =
							(row.connector_id.clone(), row.link_id.clone(), work.id.clone());
						panel = panel.child(mcp_button(
							format!("saved-app-{index}-{id}"),
							label.into(),
							current.as_deref() == value,
							cx,
							move |s, cx| {
								s.update_saved_app_settings(
									owner.clone(),
									Some((app.clone(), link.clone(), edit.clone())),
									cx,
								)
							},
						));
					}
				}
			},
			Some(State::Unavailable) =>
				panel = panel.child("Saved native connection settings are unavailable."),
			None => {},
		}
		panel.into_any_element()
	}
}
