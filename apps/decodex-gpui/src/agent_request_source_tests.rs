//! Request readback must still belong to the pending event and service.
use gpui::{AppContext as _, TestAppContext};

use crate::shell::agent_surface::{
	AgentRequestResult, AgentSurface, EntityId, LoadState, RequestReadSource,
};
use decodex_protocol::{AgentPendingEventDto, AgentRequestText};

#[gpui::test]
fn delayed_request_reply_requires_current_pending_source(cx: &mut TestAppContext) {
	let surface = cx.new(AgentSurface::new);

	for change in
		["none", "refresh", "resolved", "replaced", "runtime", "profile", "selection", "disconnect"]
	{
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.request = None;

			let work = s.selected.clone().unwrap();
			let event = AgentPendingEventDto {
				id: 902,
				source_event_id: "pending-source".into(),
				work_item_id: work.clone(),
				event_kind: "user_input_pending".into(),
				created_at_micros: 1,
				delivery_claimed: false,
			};

			s.snapshot.as_mut().unwrap().pending_events = vec![event.clone()];

			let source = RequestReadSource {
				profile_epoch: s.command_epoch,
				runtime_source: s.snapshot.as_ref().unwrap().runtime_source.clone(),
				event,
			};

			match change {
				"refresh" => {
					s.generation += 1;
					s.state = LoadState::Loading;
				},
				"resolved" => s.snapshot.as_mut().unwrap().pending_events.clear(),
				"replaced" =>
					s.snapshot.as_mut().unwrap().pending_events[0].source_event_id =
						"replacement".into(),
				"runtime" =>
					s.snapshot.as_mut().unwrap().runtime_source =
						Some(EntityId::new("replacement-runtime").unwrap()),
				"profile" => s.command_epoch += 1,
				"disconnect" => s.mark_stale(cx),
				"selection" => s.selected = Some("another-work".into()),
				_ => {},
			}

			let result = AgentRequestResult::Available {
				event_id: 902,
				work_id: work,
				method: "item/tool/requestUserInput".into(),
				request_json: AgentRequestText::new("{\"questions\":[]}").unwrap(),
			};

			s.finish_request(source, result, cx);

			assert_eq!(s.request.is_some(), matches!(change, "none" | "refresh"), "{change}");
		});
	}
}
