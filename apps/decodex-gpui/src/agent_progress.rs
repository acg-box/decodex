//! Compact, expandable native execution receipts in the continuous conversation.
use gpui::{AnyElement, KeyDownEvent};

use crate::shell::agent_surface::{
	self, AgentDispatchStateDto, AgentHistoryResult, AgentSurface, AgentWorkItemDto, Context,
	FluentBuilder, InteractiveElement, IntoElement, ParentElement, Role, SharedString,
	SmoothControl, StatefulInteractiveElement, Styled, ui_theme::TEXT_MUTED,
};
use decodex_protocol::{AgentActivityDto, AgentHistoryEntryDto};

impl AgentSurface {
	pub(super) fn has_active_compaction(&self, work: &AgentWorkItemDto) -> bool {
		let Some((id, AgentHistoryResult::Available { entries, .. })) = self.history.as_ref()
		else {
			return false;
		};

		if id != &work.id || work.dispatch_state != AgentDispatchStateDto::Running {
			return false;
		}

		entries.iter().filter_map(|entry| entry.activity.as_ref()).any(|item| {
			item.kind == "contextCompaction"
				&& item.status == "running"
				&& work.active_turn_id.as_deref() == Some(item.turn_id.as_str())
				&& !entries.iter().filter_map(|entry| entry.activity.as_ref()).any(|other| {
					other.turn_id == item.turn_id
						&& other.item_id == item.item_id
						&& other.status != "running"
				})
		})
	}

	fn toggle_progress(&mut self, key: &str, cx: &mut Context<Self>) {
		if !self.expanded_progress.remove(key) {
			self.expanded_progress.insert(key.into());
		}

		cx.notify();
	}

	pub(super) fn progress_history(
		&self,
		entries: Vec<&AgentHistoryEntryDto>,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Vec<AnyElement> {
		let mut result = Vec::new();
		let mut pending: Vec<&AgentActivityDto> = Vec::new();

		for entry in &entries {
			if super::startup_feature_warning(entry)
				|| checklist_superseded(entry, entries.iter().copied())
			{
				continue;
			}

			if let Some(activity) = &entry.activity {
				let superseded = activity.status == "running"
					&& entries.iter().any(|other| {
						other.activity.as_ref().is_some_and(|other| {
							other.turn_id == activity.turn_id
								&& other.item_id == activity.item_id
								&& other.status != "running"
						})
					});

				if !superseded {
					if pending.last().is_some_and(|previous| previous.turn_id != activity.turn_id) {
						result.push(self.progress_group(&pending, work, cx));
						pending.clear();
					}

					pending.push(activity);
				}
			} else if entry.kind != "system" {
				if !pending.is_empty() {
					result.push(self.progress_group(&pending, work, cx));
					pending.clear();
				}
				if entry.kind == "capacity_retry_pending" {
					result.push(
						agent_surface::div()
							.flex()
							.flex_col()
							.gap_1()
							.child(self.anchored_history_entry(entry, &work.id, cx))
							.child(
								agent_surface::div()
									.debug_selector(|| "capacity-retry-cancel".into())
									.child(self.capacity_retry_control(
										work.id.clone(),
										entry.id,
										cx,
									)),
							)
							.into_any_element(),
					);
				} else {
					result
						.push(self.anchored_history_entry(entry, &work.id, cx).into_any_element());
				}
			}
		}

		if !pending.is_empty() {
			result.push(self.progress_group(&pending, work, cx));
		}

		result
	}

	fn progress_group(
		&self,
		items: &[&AgentActivityDto],
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let first = items[0];
		let key = progress_key(&work.id, &first.turn_id, &first.item_id);
		let expanded = self.expanded_progress.contains(&key);
		let running = items.iter().rev().find(|item| {
			item.status == "running"
				&& work.active_turn_id.as_deref() == Some(item.turn_id.as_str())
		});
		let active = work.dispatch_state == AgentDispatchStateDto::Running;
		let steps = format!("{} step{}", items.len(), if items.len() == 1 { "" } else { "s" });
		let label = if let Some(item) = running.filter(|_| active) {
			format!("{} · {steps}", activity_title(item))
		} else if items.iter().any(|item| item.status == "failed") {
			format!("Activity · {steps} · Failed")
		} else {
			format!("Activity · {steps}")
		};
		let toggle_key = key.clone();
		let keyboard_key = key.clone();
		let mut rows = agent_surface::div()
			.flex()
			.flex_col()
			.gap(agent_surface::px(4.))
			.pl(agent_surface::px(12.))
			.mt(agent_surface::px(6.))
			.border_l_1()
			.border_color(agent_surface::rgba(0xffffff18));

		for item in items {
			let status = if item.status == "running"
				&& (!active || work.active_turn_id.as_deref() != Some(item.turn_id.as_str()))
			{
				"Unconfirmed"
			} else {
				match item.status.as_str() {
					"running" => "In progress",
					"failed" => "Failed",
					"exited" => "Exited",
					"declined" => "Declined",
					_ => "Done",
				}
			};
			let duration = item
				.duration_ms
				.map_or_else(String::new, |ms| format!(" · {:.1}s", ms as f64 / 1_000.));

			rows = rows.child(
				self.detail_row(
					work,
					item,
					agent_surface::div()
						.flex()
						.items_center()
						.gap(agent_surface::px(8.))
						.min_h(agent_surface::px(22.))
						.child(item.label.clone())
						.when(!item.detail.is_empty(), |row| {
							row.child(agent_surface::muted(item.detail.clone()))
						})
						.child(agent_surface::div().flex_1())
						.child(agent_surface::muted(format!("{status}{duration}"))),
					cx,
				),
			);
		}

		agent_surface::div()
			.id(SharedString::from(key))
			.w_full()
			.max_w(agent_surface::px(560.))
			.text_size(agent_surface::px(11.))
			.text_color(agent_surface::rgb(TEXT_MUTED))
			.child(
				agent_surface::div()
					.id("progress-toggle")
					.role(Role::Button)
					.tab_index(0)
					.aria_label("Toggle execution details")
					.cursor_pointer()
					.flex()
					.items_center()
					.gap(agent_surface::px(8.))
					.min_h(agent_surface::px(24.))
					.on_click(cx.listener(move |s, _, _, cx| {
						s.toggle_progress(&toggle_key, cx);
					}))
					.aria_expanded(expanded)
					.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							s.toggle_progress(&keyboard_key, cx);
							cx.stop_propagation();
						}
					}))
					.child(if expanded { "−" } else { "+" })
					.child(label)
					.smooth(),
			)
			.child(agent_surface::disclosure("progress-details", expanded, rows))
			.into_any_element()
	}
}

#[cfg(any(test, feature = "visual-capture"))]
impl AgentSurface {
	pub(super) fn visual_progress_fixture(&mut self, expanded: bool, cx: &mut Context<Self>) {
		self.graph_visible = false;
		self.timeline_visible = false;

		let Some((id, AgentHistoryResult::Available { entries, .. })) = &mut self.history else {
			return;
		};

		entries.clear();
		entries.push(AgentHistoryEntryDto {
			native_source: None,
			turn_id: None,
			weather: Vec::new(),
			receipt: None,
			activity: None,
			usage: None,
			duration_ms: None,
			id: 1,
			kind: "user".into(),
			text: "Review the changes and check the tests.".into(),
			created_at_micros: 1,
		});

		for (index, (kind, label, detail, status)) in [
			("commandExecution", "Reading files", "", "completed"),
			("mcpToolCall", "Using tool", "docs · search", "completed"),
			("commandExecution", "Running command", "Exit code 0", "completed"),
			("contextCompaction", "Compacting context", "", "running"),
		]
		.into_iter()
		.enumerate()
		{
			entries.push(AgentHistoryEntryDto {
				native_source: None,
				turn_id: None,
				weather: Vec::new(),
				receipt: None,
				activity: Some(AgentActivityDto {
					turn_id: "capture-turn".into(),
					item_id: format!("capture-{index}"),
					kind: kind.into(),
					label: label.into(),
					detail: detail.into(),
					status: status.into(),
					plugin_id: None,
					read_only_hint: None,
					native_timestamp_ms: None,
					duration_ms: (status == "completed").then_some(1_200),
				}),
				usage: None,
				duration_ms: None,
				id: index as i64 + 2,
				kind: "activity".into(),
				text: String::new(),
				created_at_micros: 2,
			});
		}

		if expanded {
			self.expanded_progress.insert(progress_key(id, "capture-turn", "capture-0"));
		}

		if let Some(work) = self
			.snapshot
			.as_mut()
			.and_then(|snapshot| snapshot.work_items.iter_mut().find(|work| &work.id == id))
		{
			work.active_turn_id = Some("capture-turn".into());
			work.dispatch_state = AgentDispatchStateDto::Running;
		}

		cx.notify();
	}
}

/// Collapse older cached pages as well as the latest service page.
pub(super) fn checklist_superseded<'a>(
	entry: &AgentHistoryEntryDto,
	entries: impl IntoIterator<Item = &'a AgentHistoryEntryDto>,
) -> bool {
	if entry.kind != "checklist" {
		return false;
	}

	let Some(turn) = entry.receipt.as_ref().and_then(|receipt| receipt.delivered_turn_id.as_ref())
	else {
		return false;
	};

	entries.into_iter().any(|other| {
		other.kind == "checklist"
			&& other.id > entry.id
			&& other.receipt.as_ref().and_then(|receipt| receipt.delivered_turn_id.as_ref())
				== Some(turn)
	})
}

fn activity_title(item: &AgentActivityDto) -> String {
	if ["mcpToolCall", "dynamicToolCall"].contains(&item.kind.as_str()) && !item.detail.is_empty() {
		format!("{} · {}", item.label, item.detail)
	} else {
		item.label.clone()
	}
}

fn progress_key(work: &str, turn: &str, item: &str) -> String {
	serde_json::json!([work, turn, item]).to_string()
}

#[cfg(test)]
mod tests {

	use crate::shell::agent_surface::progress::{AgentHistoryResult, AgentSurface};

	#[gpui::test]
	fn compaction_status_remains_visible_while_sending(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_progress_fixture(false, cx);

			s.sending = true;

			cx.notify();
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(1_200.0)));
			window.draw(cx).clear();
		});

		surface.read_with(visual, |s, _| {
			let work = s
				.snapshot
				.as_ref()
				.unwrap()
				.work_items
				.iter()
				.find(|w| Some(&w.id) == s.selected.as_ref())
				.unwrap();

			assert!(s.has_active_compaction(work), "selected compaction survives render");
			assert!(s.native_agents.selected.is_none());
		});

		assert!(visual.debug_bounds("conversation-activity-status").is_some());

		surface.update(visual, |s, cx| {
			let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history else {
				panic!("fixture history")
			};

			entries.last_mut().unwrap().activity.as_mut().unwrap().status = "completed".into();

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("conversation-activity-status").is_none());
	}
	#[gpui::test]
	fn live_compaction_yields_only_to_its_completion_or_turn_end(cx: &mut gpui::TestAppContext) {
		use crate::shell::agent_surface::progress::{
			AgentDispatchStateDto, AgentHistoryResult, AgentSurface,
		};

		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.visual_progress_fixture(false, cx);

			let mut work = s
				.snapshot
				.as_ref()
				.unwrap()
				.work_items
				.iter()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap()
				.clone();

			assert!(s.has_active_compaction(&work));

			work.dispatch_state = AgentDispatchStateDto::Idle;

			assert!(!s.has_active_compaction(&work));

			work.dispatch_state = AgentDispatchStateDto::Running;
			work.active_turn_id = Some("new-turn".into());

			assert!(!s.has_active_compaction(&work));

			work.active_turn_id = Some("capture-turn".into());

			let original_work = work.id.clone();

			work.id = "different-work".into();

			assert!(!s.has_active_compaction(&work));

			work.id = original_work;

			let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history else {
				panic!("fixture history")
			};
			let mut tool = entries.last().unwrap().clone();

			tool.id += 1;

			let item = tool.activity.as_mut().unwrap();

			item.item_id = "background-tool".into();
			item.kind = "commandExecution".into();
			item.label = "Running command".into();

			entries.push(tool);

			assert!(s.has_active_compaction(&work));

			let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history else {
				panic!("fixture history")
			};
			let mut finished = entries[4].clone();

			finished.id = 9;
			finished.activity.as_mut().unwrap().status = "completed".into();
			finished.activity.as_mut().unwrap().turn_id = "previous-turn".into();

			entries.push(finished.clone());

			assert!(s.has_active_compaction(&work));

			let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history else {
				panic!("fixture history")
			};

			finished.id = 10;
			finished.activity.as_mut().unwrap().turn_id = "capture-turn".into();

			entries.push(finished);

			assert!(!s.has_active_compaction(&work));

			work.dispatch_state = AgentDispatchStateDto::Idle;

			assert!(!s.has_active_compaction(&work));

			work.dispatch_state = AgentDispatchStateDto::Running;
			work.active_turn_id = Some("new-turn".into());

			assert!(!s.has_active_compaction(&work));
		});
	}
	#[test]
	fn steer_segments_and_opaque_identities_have_distinct_disclosures() {
		assert_ne!(
			super::progress_key("work", "turn", "before"),
			super::progress_key("work", "turn", "after")
		);
		assert_ne!(super::progress_key("a-b", "c", "d"), super::progress_key("a", "b-c", "d"));
	}
}
