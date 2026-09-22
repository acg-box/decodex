//! Compact, expandable native execution receipts in the continuous conversation.
use super::*;
use decodex_protocol::{ChiefActivityDto, ChiefHistoryEntryDto};

impl ChiefSurface {
	fn toggle_progress(&mut self, key: &str, cx: &mut Context<Self>) {
		if !self.expanded_progress.remove(key) {
			self.expanded_progress.insert(key.into());
		}
		cx.notify();
	}

	pub(super) fn progress_history(
		&self,
		entries: Vec<&ChiefHistoryEntryDto>,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> Vec<gpui::AnyElement> {
		let mut result = Vec::new();
		let mut pending: Vec<&ChiefActivityDto> = Vec::new();
		for entry in &entries {
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
						div()
							.flex()
							.flex_col()
							.gap_1()
							.child(self.anchored_history_entry(entry))
							.child(
								div().debug_selector(|| "capacity-retry-cancel".into()).child(
									self.capacity_retry_control(work.id.clone(), entry.id, cx),
								),
							)
							.into_any_element(),
					);
				} else {
					result.push(self.anchored_history_entry(entry).into_any_element());
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
		items: &[&ChiefActivityDto],
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let first = items[0];
		let key = progress_key(&work.id, &first.turn_id, &first.item_id);
		let expanded = self.expanded_progress.contains(&key);
		let running = items.iter().rev().find(|item| {
			item.status == "running"
				&& work.active_turn_id.as_deref() == Some(item.turn_id.as_str())
		});
		let active = work.dispatch_state == ChiefDispatchStateDto::Running;
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
		let mut rows = div()
			.flex()
			.flex_col()
			.gap(px(4.))
			.pl(px(12.))
			.mt(px(6.))
			.border_l_1()
			.border_color(rgba(0xffffff18));
		for item in items {
			let status = if item.status == "running"
				&& (!active || work.active_turn_id.as_deref() != Some(item.turn_id.as_str()))
			{
				"Unconfirmed"
			} else {
				match item.status.as_str() {
					"running" => "In progress",
					"failed" => "Failed",
					"declined" => "Declined",
					_ => "Done",
				}
			};
			let duration = item
				.duration_ms
				.map_or_else(String::new, |ms| format!(" · {:.1}s", ms as f64 / 1000.));
			rows = rows.child(
				self.detail_row(
					work,
					item,
					div()
						.flex()
						.items_center()
						.gap(px(8.))
						.min_h(px(22.))
						.child(item.label.clone())
						.when(!item.detail.is_empty(), |row| row.child(muted(item.detail.clone())))
						.child(div().flex_1())
						.child(muted(format!("{status}{duration}"))),
					cx,
				),
			);
		}
		div()
			.id(SharedString::from(key))
			.w_full()
			.max_w(px(560.))
			.text_size(px(11.))
			.text_color(rgb(ui_theme::TEXT_MUTED))
			.child(
				div()
					.id("progress-toggle")
					.role(Role::Button)
					.tab_index(0)
					.aria_label("Toggle execution details")
					.cursor_pointer()
					.flex()
					.items_center()
					.gap(px(8.))
					.min_h(px(24.))
					.on_click(cx.listener(move |s, _, _, cx| {
						s.toggle_progress(&toggle_key, cx);
					}))
					.aria_expanded(expanded)
					.on_key_down(cx.listener(move |s, event: &gpui::KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
							s.toggle_progress(&keyboard_key, cx);
							cx.stop_propagation();
						}
					}))
					.child(if expanded { "−" } else { "+" })
					.child(label)
					.smooth(),
			)
			.child(disclosure("progress-details", expanded, rows))
			.into_any_element()
	}
}

#[cfg(any(test, feature = "visual-capture"))]
impl ChiefSurface {
	pub(super) fn visual_progress_fixture(&mut self, expanded: bool, cx: &mut Context<Self>) {
		self.graph_visible = false;
		self.timeline_visible = false;
		let Some((id, ChiefHistoryResult::Available { entries, .. })) = &mut self.history else {
			return;
		};
		entries.clear();
		entries.push(ChiefHistoryEntryDto {
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
			entries.push(ChiefHistoryEntryDto {
				activity: Some(ChiefActivityDto {
					turn_id: "capture-turn".into(),
					item_id: format!("capture-{index}"),
					kind: kind.into(),
					label: label.into(),
					detail: detail.into(),
					status: status.into(),
					duration_ms: (status == "completed").then_some(1200),
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
			work.dispatch_state = ChiefDispatchStateDto::Running;
		}
		cx.notify();
	}
}

fn activity_title(item: &ChiefActivityDto) -> String {
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
	#[test]
	fn steer_segments_and_opaque_identities_have_distinct_disclosures() {
		assert_ne!(
			super::progress_key("work", "turn", "before"),
			super::progress_key("work", "turn", "after")
		);
		assert_ne!(super::progress_key("a-b", "c", "d"), super::progress_key("a", "b-c", "d"));
	}
}
