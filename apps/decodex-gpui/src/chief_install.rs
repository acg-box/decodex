//! Explicit install, authorize and verify flow for native tool suggestions.
use super::{mcp_forms::mcp_button, *};
use decodex_protocol::{ChiefInstallState as State, McpInstallSuggestion};
use serde_json::{Value, json};

#[derive(Default)]
pub(super) struct Panel {
	event: Option<i64>,
	state: Option<State>,
	task: Option<Task<()>>,
	pub(super) feedback: String,
	epoch: u64,
}

impl ChiefSurface {
	pub(super) fn installation_disconnected(&mut self) {
		self.installation =
			Panel { epoch: self.installation.epoch.wrapping_add(1), ..Default::default() };
	}

	fn inspect_installation(&mut self, event: i64, install: bool, cx: &mut Context<Self>) {
		let Some(ChiefRequestResult::Available { event_id, work_id, method, .. }) = &self.request
		else {
			return;
		};
		if *event_id != event
			|| method != "mcpServer/elicitation/request"
			|| self.selected.as_ref() != Some(work_id)
		{
			return;
		}
		if self.installation.event == Some(event) && self.installation.task.is_some() {
			return;
		}
		let Some(profile) = self.profile.clone() else {
			return;
		};
		let Ok(work) = EntityId::new(work_id.clone()) else {
			return;
		};
		let action = if install {
			let Some(State::Available { event_id, can_install: true, review_token, .. }) =
				&self.installation.state
			else {
				return;
			};
			if *event_id != event {
				return;
			}
			let Ok(review_token) = WireText::new(review_token.clone()) else {
				return;
			};
			Some(ChiefActionDto::InstallSuggestedPlugin {
				work_id: work.clone(),
				event_id: event,
				review_token,
			})
		} else {
			None
		};
		let generation = self.generation;
		let work_name = work_id.clone();
		self.installation.epoch = self.installation.epoch.wrapping_add(1);
		let epoch = self.installation.epoch;
		self.installation.event = Some(event);
		self.installation.state = None;
		self.installation.feedback =
			if install { "Installing…" } else { "Checking installation and access…" }.into();
		let key = IdempotencyKey::new(unique_command()).expect("bounded command identity");
		let future = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			let client = ChiefClient::new(profile);
			let outcome = action.map(|action| runtime.block_on(client.execute(action, key)));
			let state =
				runtime.block_on(client.install_state(work, event)).unwrap_or(State::Unavailable);
			Some((state, outcome))
		});
		self.installation.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation
					|| s.selected.as_ref() != Some(&work_name)
					|| s.installation.epoch != epoch
					|| !matches!(&s.request,Some(ChiefRequestResult::Available{event_id,..}) if *event_id==event)
				{
					return;
				}
				s.installation.task = None;
				let (state, outcome) = result.unwrap_or((State::Unavailable, None));
				s.installation.feedback = match (&state, outcome) {
					(State::Available { can_continue: true, .. }, _) =>
						"Installation and connector access are verified. You can continue.",
					(State::Available { authorization_requirements_known:false,.. }, _) =>
						"Installation requirements are unconfirmed. Check status again; this request will not be installed twice.",
					(State::Available { installed: Some(true), .. }, _) =>
						"Plugin installed. Complete any required connection steps below.",
					(State::Available { attempted: true, .. }, _) =>
						"An installation was attempted. Its completion is unconfirmed; check status again.",
					(_, Some(Ok(ChiefCommandResponse::Rejected { .. }))) =>
						"Installation was not accepted. Review the current details.",
					(State::Available { .. }, _) =>
						"Review the integration and connection requirements below.",
					_ =>
						"Current installation state is unavailable. Check again before continuing.",
				}
				.into();
				s.installation.state = Some(state);
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn installation_panel(
		&self,
		event: i64,
		value: &Value,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let mut panel = div().id("installation-suggestion").flex().flex_col().gap_2();
		let parsed = McpInstallSuggestion::from_request(value);
		let valid = matches!(&parsed, Ok(Some(_)));
		if let Ok(Some(suggestion)) = parsed {
			panel = panel.child(format!("Set up {}", suggestion.tool_name));
			panel = panel.child(value["message"].as_str().unwrap_or("").to_owned());
		} else {
			panel = panel
				.child("This installation request cannot be read. You can decline or cancel it.");
		}
		let current = self.installation.event == Some(event);
		let busy = current && self.installation.task.is_some();
		if valid && !busy {
			panel = panel.child(mcp_button(
				"install-inspect".into(),
				"Check installation status".into(),
				false,
				cx,
				move |s, cx| s.inspect_installation(event, false, cx),
			));
		}
		if current
			&& let Some(State::Available {
				event_id,
				can_install,
				can_continue,
				review_details,
				apps,
				..
			}) = &self.installation.state
			&& *event_id == event
		{
			panel = panel.child(review_details.clone());
			for app in apps {
				panel = panel.child(format!(
					"{}: {}",
					app.name,
					if !app.enabled {
						"Disabled in settings"
					} else if app.accessible {
						"Connected"
					} else {
						"Connection required"
					}
				));
				if !app.accessible
					&& let Some(url) = &app.install_url
				{
					let url = url.as_str().to_owned();
					panel = panel.child(mcp_button(
						format!("install-connect-{}", app.id),
						format!("Connect {}", app.name),
						false,
						cx,
						move |s, cx| {
							if s.installation.event == Some(event)
								&& matches!(&s.request,Some(ChiefRequestResult::Available{event_id,..}) if *event_id==event)
							{
								cx.open_url(&url);
							}
						},
					));
				}
			}
			if *can_install && !busy {
				panel = panel.child(mcp_button(
					"install-confirm".into(),
					"Install this plugin".into(),
					false,
					cx,
					move |s, cx| s.inspect_installation(event, true, cx),
				));
			}
			if *can_continue && !busy {
				panel = panel.child(mcp_button(
					"install-continue".into(),
					"Continue with this integration".into(),
					false,
					cx,
					move |s, cx| {
						if s.installation.event == Some(event)
							&& matches!(&s.request,Some(ChiefRequestResult::Available{event_id,..}) if *event_id==event)
						{
							s.respond(
								json!({"action":"accept","content":{},"_meta":null}).to_string(),
								cx,
							);
						}
					},
				));
			}
		}
		for (action, label) in [("decline", "Decline"), ("cancel", "Cancel")] {
			panel = panel.child(mcp_button(
				format!("install-{action}"),
				label.into(),
				false,
				cx,
				move |s, cx| {
					if matches!(&s.request,Some(ChiefRequestResult::Available{event_id,..}) if *event_id==event)
					{
						s.respond(
							json!({"action":action,"content":null,"_meta":null}).to_string(),
							cx,
						);
					}
				},
			));
		}
		panel.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn installation_suggestion_has_separate_install_and_verified_continue_actions(
		cx: &mut gpui::TestAppContext,
	) {
		use decodex_protocol::{ChiefPendingEventDto, ChiefWorkKindDto};
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual,|s,_| {
			s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
				runtime_source: None,
				workspaces:vec![],dependencies:vec![],work_items:vec![ChiefWorkItemDto{id:"root".into(),parent_goal_id:None,kind:ChiefWorkKindDto::Goal,title:"Chief".into(),codex_thread_id:Some("thread".into()),active_turn_id:None,dispatch_state:ChiefDispatchStateDto::Idle,status:ChiefWorkStatusDto::Open,next_check_at_micros:None,created_at_micros:1,updated_at_micros:1}],
				pending_events:vec![ChiefPendingEventDto{id:7,source_event_id:"suggestion".into(),work_item_id:"root".into(),event_kind:"server_request_pending".into(),created_at_micros:1,delivery_claimed:false}],
			})));
			s.request=Some(ChiefRequestResult::Available{event_id:7,work_id:"root".into(),method:"mcpServer/elicitation/request".into(),request_json:decodex_protocol::ChiefRequestText::new(json!({"serverName":"codex_apps","mode":"form","requestedSchema":{"type":"object","properties":{}},"_meta":{"codex_approval_kind":"tool_suggestion","suggest_type":"install","tool_type":"plugin","tool_id":"sample@market","tool_name":"Sample"}}).to_string()).unwrap()});
			s.installation.event=Some(7);
			s.installation.state=Some(State::Available{event_id:7,tool_id:"sample@market".into(),tool_name:"Sample".into(),installed:Some(false),attempted:false,authorization_requirements_known:true,can_install:true,can_continue:false,review_token:"review".into(),review_details:"Sample from the selected marketplace".into(),apps:vec![]});
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1400.0)));
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("install-confirm").is_some());
		assert!(visual.debug_bounds("install-continue").is_none());
		assert!(
			visual.debug_bounds("mcp-submit").is_none(),
			"generic accept must not bypass installation"
		);
		surface.update(visual, |s, _| {
			if let Some(State::Available {
				installed, attempted, can_install, can_continue, ..
			}) = &mut s.installation.state
			{
				*installed = Some(true);
				*attempted = true;
				*can_install = false;
				*can_continue = true;
			}
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("install-confirm").is_none());
		let continue_button =
			visual.debug_bounds("install-continue").expect("verified continuation");
		visual.simulate_click(continue_button.center(), gpui::Modifiers::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.feedback, "No service profile is configured.");
			assert!(s.submission.command.is_none());
		});
		surface.update(visual, |s, _| {
			s.installation_disconnected();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(
			visual.debug_bounds("install-continue").is_none(),
			"disconnect invalidates verified state"
		);
		assert!(visual.debug_bounds("install-decline").is_some());
	}
}
