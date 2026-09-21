//! Explicit account approval overrides, separate from answering the pending request.
use super::{mcp_forms::mcp_button, *};
use decodex_protocol::{
	ChiefAppApprovalMode as Mode, ChiefAppReviewer as Reviewer, ChiefAppSettingEdit as Edit,
	ChiefAppSettingsResult as State,
};

#[derive(Default)]
pub(super) struct Panel {
	event: Option<i64>,
	state: Option<State>,
	task: Option<Task<()>>,
	epoch: u64,
	feedback: String,
}

impl ChiefSurface {
	pub(super) fn app_settings_disconnected(&mut self) {
		self.app_settings =
			Panel { epoch: self.app_settings.epoch.wrapping_add(1), ..Default::default() };
	}

	fn update_account_settings(&mut self, event: i64, edit: Option<Edit>, cx: &mut Context<Self>) {
		let Some(ChiefRequestResult::Available { event_id, work_id, method, .. }) = &self.request
		else {
			return;
		};
		if *event_id != event
			|| method != "mcpServer/elicitation/request"
			|| self.selected.as_ref() != Some(work_id)
			|| self.app_settings.task.is_some()
		{
			return;
		}
		let Some(profile) = self.profile.clone() else { return };
		let Ok(work) = EntityId::new(work_id.clone()) else { return };
		let action = match edit {
			Some(edit) => {
				if self.app_settings.event != Some(event) {
					return;
				}
				let Some(State::Available { review_token, .. }) = &self.app_settings.state else {
					return;
				};
				let Ok(review_token) = WireText::new(review_token.clone()) else { return };
				Some(ChiefActionDto::SetAppSetting {
					work_id: work.clone(),
					event_id: event,
					review_token,
					edit,
				})
			},
			None => None,
		};
		let saving = action.is_some();
		let generation = self.generation;
		let owner = work_id.clone();
		self.app_settings.epoch = self.app_settings.epoch.wrapping_add(1);
		let epoch = self.app_settings.epoch;
		self.app_settings.event = Some(event);
		self.app_settings.state = None;
		self.app_settings.feedback =
			if saving { "Saving account setting…" } else { "Reading account settings…" }.into();
		let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state =
				runtime.block_on(client.app_settings(work, event)).unwrap_or(State::Unavailable);
			Some((state, outcome))
		});
		self.app_settings.task=Some(cx.spawn(async move |surface,cx| {
			let result=future.await;
			let _=surface.update(cx,|s,cx| {
				if s.generation!=generation || s.app_settings.epoch!=epoch || s.selected.as_ref()!=Some(&owner)
					|| !matches!(&s.request,Some(ChiefRequestResult::Available{event_id,..}) if *event_id==event) {return;}
				s.app_settings.task=None;
				let (state,outcome)=result.unwrap_or((State::Unavailable,None));
				s.app_settings.feedback=match outcome {
					Some(Ok(ChiefCommandResponse::Accepted {..})) => "Saved and read back. Native tool and managed policies can still override this account setting.",
					Some(Ok(ChiefCommandResponse::Rejected {..})) => "The edit was not accepted. Review the current settings before changing them.",
					Some(_) => "The edit may have been saved. Readback is shown below; it will not be retried automatically.",
					None if saving => "The edit could not be confirmed. Refresh settings before further action.",
					None => "These overrides are shared by tasks using this connected account.",
				}.into();
				s.app_settings.state=Some(state);cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn account_settings_panel(
		&self,
		event: i64,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let mut panel = div().id("account-approval-settings").flex().flex_col().gap_2();
		let current = self.app_settings.event == Some(event);
		let busy = current && self.app_settings.task.is_some();
		if current {
			panel = panel.child(self.app_settings.feedback.clone());
		}
		if !busy {
			panel = panel.child(mcp_button(
				"account-settings-read".into(),
				"Review account approval settings".into(),
				false,
				cx,
				move |s, cx| s.update_account_settings(event, None, cx),
			));
		}
		if current && !busy {
			if let Some(State::Available {
				connector_id,
				link_id,
				effective_mode,
				effective_reviewer,
				user_mode,
				user_reviewer,
				..
			}) = &self.app_settings.state
			{
				panel = panel
					.child(format!("App: {connector_id} · Account: {link_id}"))
					.child(format!(
						"User overrides — mode: {}; reviewer: {}",
						display(user_mode),
						display(user_reviewer)
					))
					.child(format!(
						"Merged configuration — mode: {}; reviewer: {}",
						display(effective_mode),
						display(effective_reviewer)
					))
					.child(
						"Select an override to save it. This does not answer the pending approval.",
					);
				for (id, label, edit) in choices() {
					let (field, value) = edit.native_value();
					let selected =
						if field == "approvals_reviewer" { user_reviewer } else { user_mode };
					panel = panel.child(mcp_button(
						id.into(),
						label.into(),
						selected.as_deref() == value,
						cx,
						move |s, cx| s.update_account_settings(event, Some(edit.clone()), cx),
					));
				}
			} else if self.app_settings.state.is_some() {
				panel = panel.child("Account settings are unavailable for this request.");
			}
		}
		panel.into_any_element()
	}
}

fn display(value: &Option<String>) -> &str {
	value.as_deref().unwrap_or("inherit")
}
fn choices() -> [(&'static str, &'static str, Edit); 8] {
	[
		("account-mode-inherit", "Inherit approval mode", Edit::ApprovalMode(None)),
		("account-mode-auto", "Use automatic approval rules", Edit::ApprovalMode(Some(Mode::Auto))),
		("account-mode-prompt", "Ask for every tool call", Edit::ApprovalMode(Some(Mode::Prompt))),
		(
			"account-mode-writes",
			"Ask for calls not marked read-only",
			Edit::ApprovalMode(Some(Mode::Writes)),
		),
		(
			"account-mode-approve",
			"Skip tool approval prompts",
			Edit::ApprovalMode(Some(Mode::Approve)),
		),
		("account-reviewer-inherit", "Inherit reviewer", Edit::Reviewer(None)),
		("account-reviewer-user", "Send reviews to me", Edit::Reviewer(Some(Reviewer::User))),
		(
			"account-reviewer-auto",
			"Use automatic reviewer",
			Edit::Reviewer(Some(Reviewer::AutoReview)),
		),
	]
}

#[cfg(test)]
#[path = "chief_app_settings_wire_tests.rs"]
mod wire_tests;

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;
	#[gpui::test]
	fn account_settings_require_review_and_clear_on_disconnect(cx: &mut gpui::TestAppContext) {
		use decodex_protocol::{ChiefPendingEventDto, ChiefWorkKindDto};
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual,|s,_| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				runtime_source:None,workspaces:vec![],dependencies:vec![],
				work_items:vec![ChiefWorkItemDto{id:"root".into(),parent_goal_id:None,kind:ChiefWorkKindDto::Goal,title:"Chief".into(),codex_thread_id:Some("thread".into()),active_turn_id:None,dispatch_state:ChiefDispatchStateDto::Idle,status:ChiefWorkStatusDto::Open,next_check_at_micros:None,created_at_micros:1,updated_at_micros:1}],
				pending_events:vec![ChiefPendingEventDto{id:7,source_event_id:"approval".into(),work_item_id:"root".into(),event_kind:"server_request_pending".into(),created_at_micros:1,delivery_claimed:false}],
			})));
			s.request=Some(ChiefRequestResult::Available {event_id:7,work_id:"root".into(),method:"mcpServer/elicitation/request".into(),request_json:HistoryText::new(json!({"serverName":"codex_apps","mode":"form","message":"Review action","requestedSchema":{"type":"object","properties":{}},"_meta":{"connector_id":"calendar","link_id":"work","codex_approval_kind":"tool_call"}}).to_string()).unwrap()});
		});
		visual.update(|w, cx| {
			w.resize(gpui::size(px(1180.0), px(1800.0)));
			w.draw(cx).clear();
		});
		assert!(visual.debug_bounds("account-settings-read").is_some());
		assert!(visual.debug_bounds("account-mode-auto").is_none());
		surface.update(visual, |s, _| {
			s.app_settings.event = Some(7);
			s.app_settings.state = Some(State::Available {
				connector_id: "calendar".into(),
				link_id: "work".into(),
				review_token: "a".repeat(64),
				effective_mode: Some("prompt".into()),
				effective_reviewer: None,
				user_mode: None,
				user_reviewer: None,
			});
		});
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});
		for (id, _, _) in choices() {
			assert!(visual.debug_bounds(id).is_some(), "{id}");
		}
		surface.update(visual, |s, cx| {
			let epoch = s.app_settings.epoch;
			let mut snapshot = s.snapshot.clone().unwrap();
			snapshot.runtime_source = Some(EntityId::new("changed-native-source").unwrap());
			s.apply_result(Ok(ChiefSnapshotResult::Available(snapshot)));
			assert!(s.app_settings.epoch != epoch);
			assert!(s.app_settings.state.is_none());
			let epoch = s.app_settings.epoch;
			s.mark_stale(cx);
			assert!(s.app_settings.epoch != epoch);
			assert!(s.app_settings.state.is_none());
		});
		visual.update(|w, cx| {
			w.draw(cx).clear();
		});
		assert!(visual.debug_bounds("account-mode-auto").is_none());
	}
}
