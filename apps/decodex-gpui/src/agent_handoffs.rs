//! Global human handoffs derived from work facts, with host-local read receipts.
use std::collections::BTreeMap;

use decodex_protocol::{
	AgentDispatchStateDto as Dispatch, AgentSnapshotDto, AgentWorkItemDto,
	AgentWorkStatusDto as Status,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Handoff {
	pub work: String,
	pub key: String,
	pub label: &'static str,
	pub reason: &'static str,
	pub result: bool,
}

#[derive(Default)]
pub(super) struct Handoffs {
	scope: Option<String>,
	seen: BTreeMap<String, String>,
	pub relations: bool,
	pub read_on_open: Option<(String, String)>,
}

fn receipt(work: &AgentWorkItemDto) -> String {
	serde_json::json!([work.codex_thread_id, work.updated_at_micros]).to_string()
}

fn direct_work(snapshot: &AgentSnapshotDto, work: &AgentWorkItemDto) -> bool {
	work.parent_goal_id.as_ref().is_none_or(|parent| {
		snapshot.work_items.iter().any(|w| &w.id == parent && w.parent_goal_id.is_none())
	})
}

fn completed(work: &AgentWorkItemDto) -> bool {
	work.status == Status::Resolved && work.dispatch_state == Dispatch::Idle
}

impl Handoffs {
	pub fn observe(&mut self, scope: String, snapshot: &AgentSnapshotDto) {
		if snapshot.connection_initializing || self.scope.as_ref() == Some(&scope) {
			return;
		}
		let stored = crate::ui_preferences::string(&format!("DecodexHandoffs-{scope}"), None)
			.and_then(|text| serde_json::from_str::<BTreeMap<String, String>>(&text).ok());
		// First use seeds historical completions; it never turns the archive into an inbox.
		self.seen = stored.unwrap_or_else(|| {
			snapshot
				.work_items
				.iter()
				.filter(|w| completed(w))
				.map(|w| (w.id.clone(), receipt(w)))
				.collect()
		});
		self.scope = Some(scope);
		self.relations = false;
		self.read_on_open = None;
		self.save();
	}

	fn save(&self) {
		if let Some(scope) = &self.scope {
			let text = serde_json::to_string(&self.seen).expect("read receipt strings");
			crate::ui_preferences::string(&format!("DecodexHandoffs-{scope}"), Some(&text));
		}
	}

	pub fn acknowledge(&mut self, work: &str, key: &str, snapshot: &AgentSnapshotDto) {
		if snapshot.work_items.iter().any(|w| w.id == work && completed(w) && receipt(w) == key) {
			self.seen.retain(|id, _| snapshot.work_items.iter().any(|w| &w.id == id));
			self.seen.insert(work.into(), key.into());
			self.save();
		}
	}

	pub fn items(&self, snapshot: &AgentSnapshotDto) -> Vec<Handoff> {
		if self.scope.is_none() || snapshot.connection_initializing {
			return Vec::new();
		}
		let mut work: Vec<_> = snapshot.work_items.iter().collect();
		work.sort_by_key(|w| (w.created_at_micros, w.id.as_str()));
		work.into_iter()
			.filter_map(|work| {
				let request = snapshot.pending_events.iter().find(|event| {
					event.work_item_id == work.id
						&& ["permission_pending", "user_input_pending", "server_request_pending"]
							.contains(&event.event_kind.as_str())
				});
				let (key, label, reason, result) = if let Some(event) = request {
					let permission = event.event_kind == "permission_pending";
					(
						format!("request:{}", event.id),
						if permission { "Permission needed" } else { "Answer needed" },
						if permission {
							"Review the original request before granting permission."
						} else {
							"A question is waiting for your response."
						},
						false,
					)
				} else if work.status == Status::UserDecision
					&& work.dispatch_state == Dispatch::Idle
				{
					(
						receipt(work),
						"Decision needed",
						"This task explicitly asks for your decision.",
						false,
					)
				} else if direct_work(snapshot, work) && work.dispatch_state == Dispatch::Unknown {
					(
						receipt(work),
						"Check execution",
						"Execution could not be confirmed. Check the conversation before retrying.",
						false,
					)
				} else if direct_work(snapshot, work)
					&& completed(work)
					&& self.seen.get(&work.id) != Some(&receipt(work))
				{
					(
						receipt(work),
						"Result ready",
						"This task reported completion. Review its result and verification.",
						true,
					)
				} else {
					return None;
				};
				Some(Handoff { work: work.id.clone(), key, label, reason, result })
			})
			.collect()
	}

	#[cfg(any(test, feature = "visual-capture"))]
	pub fn fixture(&mut self) {
		self.scope = Some("visual-fixture".into());
		self.seen.clear();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::shell::agent_surface::AgentSurface;
	use gpui::AppContext;

	#[gpui::test]
	fn handoffs_ignore_history_and_subordinate_results_but_keep_explicit_requests(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let mut snapshot = s.snapshot.clone().unwrap();
			let mut handoffs = Handoffs::default();
			handoffs.observe("fixture-test".into(), &snapshot);
			assert_eq!(
				handoffs.items(&snapshot).iter().map(|h| h.work.as_str()).collect::<Vec<_>>(),
				vec!["release"]
			);
			let release = snapshot.work_items.iter_mut().find(|w| w.id == "release").unwrap();
			release.status = Status::Resolved;
			release.updated_at_micros += 1;
			let result = handoffs.items(&snapshot).pop().unwrap();
			assert!(result.result);
			handoffs.acknowledge(&result.work, &result.key, &snapshot);
			assert!(handoffs.items(&snapshot).is_empty());
			snapshot.pending_events.push(decodex_protocol::AgentPendingEventDto {
				id: 99,
				source_event_id: "request".into(),
				work_item_id: "verify".into(),
				event_kind: "user_input_pending".into(),
				created_at_micros: 99,
				delivery_claimed: true,
			});
			assert_eq!(handoffs.items(&snapshot)[0].work, "verify");
			snapshot.pending_events.clear();
			assert!(handoffs.items(&snapshot).is_empty());
			let release = snapshot.work_items.iter_mut().find(|w| w.id == "release").unwrap();
			release.updated_at_micros += 1;
			handoffs.acknowledge(&result.work, &result.key, &snapshot);
			assert_eq!(
				handoffs.items(&snapshot).len(),
				1,
				"an old read must not consume a newer result"
			);
		});
	}
}
