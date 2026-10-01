//! Route integration setup to Codex without installing or authorizing in Decodex.
use gpui::AnyElement;
use serde_json::{Value, json};

use crate::shell::agent_surface::{
	self, AgentRequestResult, AgentSurface, Context, InteractiveElement, IntoElement,
	ParentElement, Styled, mcp_forms,
};
#[cfg(test)]
use crate::shell::agent_surface::{
	AgentDispatchStateDto, AgentSnapshotDto, AgentSnapshotResult, AgentWorkItemDto,
	AgentWorkStatusDto, px,
};

impl AgentSurface {
	pub(super) fn installation_panel(
		&self,
		event: i64,
		_value: &Value,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut panel =
			agent_surface::div().id("installation-suggestion").flex().flex_col().gap_2().child(
				"Configure this integration in Codex for this account, then retry the task.",
			);

		for (action, label) in [("decline", "Decline"), ("cancel", "Cancel")] {
			panel = panel.child(mcp_forms::mcp_button(
				format!("install-{action}"), label.into(), false, cx,
				move |s, cx| {
					if matches!(&s.request, Some(AgentRequestResult::Available { event_id, .. }) if *event_id == event) {
						s.respond(json!({"action":action,"content":null,"_meta":null}).to_string(), cx);
					}
				},
			));
		}

		panel.into_any_element()
	}
}

#[cfg(test)]
mod tests {

	use crate::shell::agent_surface::install::{
		self, AgentDispatchStateDto, AgentRequestResult, AgentSnapshotDto, AgentSnapshotResult,
		AgentSurface, AgentWorkItemDto, AgentWorkStatusDto,
	};
	use decodex_protocol::{AgentPendingEventDto, AgentWorkKindDto};

	#[gpui::test]
	fn setup_request_can_be_declined_without_an_install_or_accept_action(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, _| {
			s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
				runtime_source: None, workspaces: vec![], dependencies: vec![],
				work_items: vec![AgentWorkItemDto { id: "root".into(), parent_goal_id: None, kind: AgentWorkKindDto::Goal, title: "Agent".into(), codex_thread_id: Some("thread".into()), active_turn_id: None, dispatch_state: AgentDispatchStateDto::Idle, status: AgentWorkStatusDto::Open, next_check_at_micros: None, created_at_micros: 1, updated_at_micros: 1 }],
				pending_events: vec![AgentPendingEventDto { id: 7, source_event_id: "suggestion".into(), work_item_id: "root".into(), event_kind: "server_request_pending".into(), created_at_micros: 1, delivery_claimed: false }],
			})));

			s.request = Some(AgentRequestResult::Available { event_id: 7, work_id: "root".into(), method: "mcpServer/elicitation/request".into(), request_json: decodex_protocol::AgentRequestText::new(install::json!({"serverName":"codex_apps","mode":"form","requestedSchema":{"type":"object","properties":{}},"_meta":{"codex_approval_kind":"tool_suggestion","suggest_type":"install","tool_type":"plugin","tool_id":"sample@market","tool_name":"Sample"}}).to_string()).unwrap() });
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(install::px(1_180.0), install::px(1_400.0)));
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("install-decline").is_some());
		assert!(visual.debug_bounds("install-cancel").is_some());
		assert!(visual.debug_bounds("install-confirm").is_none());
		assert!(visual.debug_bounds("install-continue").is_none());
		assert!(
			visual.debug_bounds("mcp-submit").is_none(),
			"Generic form acceptance must not start plugin setup"
		);
	}
}
