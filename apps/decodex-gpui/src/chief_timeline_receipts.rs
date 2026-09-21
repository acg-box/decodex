//! Keep local delivery evidence separate from canonical conversation rows.
use super::{
	ChiefHistoryResult, ChiefSurface, ChiefWorkItemDto, Content, Context, IntoElement,
	ParentElement, Styled, auth_recovery_entry, div, markdown, muted,
};
use decodex_protocol::ChiefHistoryEntryDto;
use gpui::InteractiveElement;

impl ChiefSurface {
	pub(in super::super) fn native_receipts_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let mut panel =
			div().flex().flex_col().gap_2().child(self.native_input_receipts_panel(work, cx));
		let Some((_, ChiefHistoryResult::Available { entries, live, .. })) =
			self.history.as_ref().filter(|(id, _)| id == &work.id)
		else {
			return panel
				.child(muted("Local delivery records are unavailable. Retrying…"))
				.into_any_element();
		};
		let mut saved = std::collections::BTreeMap::new();
		if let Some((older, _)) = self.older_history.get(&work.id) {
			for entry in older {
				saved.insert(entry.id, entry);
			}
		}
		for entry in entries {
			saved.insert(entry.id, entry);
		}
		for entry in saved.values() {
			if entry.kind == "auth_recovery" {
				panel = panel.child(auth_recovery_entry(entry));
				continue;
			}
			if receipt_label(entry) == Some("Local input · Delivery not confirmed")
				&& self.native_input_receipts_loaded(&work.id)
			{
				continue;
			}
			let Some(label) = receipt_label(entry) else {
				continue;
			};
			let mut row = div()
				.flex()
				.flex_col()
				.gap_1()
				.child(muted(label))
				.child(markdown::render(&entry.text, &format!("receipt-{}", entry.id)));
			if entry.kind == "capacity_retry_pending" {
				row = row.child(
					div()
						.debug_selector(|| "capacity-retry-cancel".into())
						.child(self.capacity_retry_control(work.id.clone(), entry.id, cx)),
				);
			}
			panel = panel.child(row);
		}
		for message in live {
			if self.native_history.entries.iter().any(|entry| match &entry.content {
				Content::Item { turn_id, item_id, .. } =>
					turn_id == &message.turn_id && item_id == &message.item_id,
				Content::TurnBoundary { turn_id, completed: true, .. } =>
					turn_id == &message.turn_id,
				_ => false,
			}) {
				continue;
			}
			panel = panel.child(
				div()
					.debug_selector({
						let plan = message.kind == decodex_protocol::ChiefLiveMessageKind::Plan;
						move || if plan { "native-live-plan" } else { "native-live-output" }.into()
					})
					.child(muted(if message.kind == decodex_protocol::ChiefLiveMessageKind::Plan {
						"Proposed plan · Live"
					} else {
						"Assistant · In progress"
					}))
					.child(markdown::render(
						&message.text,
						&format!("live-{}-{}", message.turn_id, message.item_id),
					))
					.children(
						message
							.truncated
							.then(|| muted("Partial output shortened; waiting for saved result.")),
					),
			);
		}
		panel.into_any_element()
	}
}

fn receipt_label(entry: &ChiefHistoryEntryDto) -> Option<&'static str> {
	if entry.kind == "capacity_retry_pending" {
		return Some("Automatic retry");
	}
	if entry.kind == "execution_notice" {
		return Some("Execution notice");
	}
	if entry.kind == "automation" {
		return Some("Automation result");
	}
	let receipt = entry.receipt.as_ref()?;
	if ["user_message", "async_question_answer", "work_instruction"]
		.contains(&receipt.event_kind.as_str())
		&& receipt.delivered_turn_id.is_none()
		&& !receipt.disposed
	{
		return Some("Local input · Delivery not confirmed");
	}
	if entry.kind == "system" && receipt.event_kind != "context_compacted" {
		return Some("Local execution record");
	}
	None
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn auth_recovery_history_is_visible_as_a_recorded_notice(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1400.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.graph_visible = false;
			let work = s.snapshot.as_mut().unwrap().work_items.iter_mut().find(|w| Some(&w.id) == s.selected.as_ref()).unwrap();
			work.codex_thread_id = Some("native-thread".into());
			s.history = Some((work.id.clone(), ChiefHistoryResult::Available {
				questions: vec![], questions_truncated: false, questions_recovering: false, misalignment: None, usage: None, has_more: false, next_before: None, live: vec![],
				entries: vec![ChiefHistoryEntryDto {
					id: 91,kind:"auth_recovery".into(),text:"Codex reported that provider sign-in recovery started.\n\nAWS: [Sign in](https://example.invalid)\n\nSaved event; current sign-in status is not confirmed by this record.".into(),created_at_micros:1,receipt:None,activity:None,usage:None,duration_ms:None,
				}],
			}));
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let bounds = visual
			.debug_bounds("auth-recovery-receipt-91")
			.expect("saved authentication notice is rendered");
		assert!(bounds.size.height > gpui::px(0.));
		surface.update(visual, |s, cx| {
			s.native_history.binding = Some(super::super::Binding {
				work: s.selected.clone().unwrap(),
				thread: "native-thread".into(),
				account: "account".into(),
			});
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(
			visual
				.debug_bounds("auth-recovery-receipt-91")
				.expect("native conversation keeps authentication receipts visible")
				.size
				.height > gpui::px(0.)
		);
	}
	#[test]
	fn native_view_preserves_uncertain_input_and_controls_without_text_deduplication() {
		let mut entry = ChiefHistoryEntryDto {
			receipt: Some(decodex_protocol::ChiefHistoryReceiptDto {
				event_kind: "user_message".into(),
				delivered_turn_id: None,
				disposed: false,
			}),
			activity: None,
			usage: None,
			duration_ms: None,
			id: 1,
			kind: "user".into(),
			text: "Repeated text".into(),
			created_at_micros: 1,
		};
		assert_eq!(receipt_label(&entry), Some("Local input · Delivery not confirmed"));
		entry.receipt.as_mut().unwrap().delivered_turn_id = Some("native-turn".into());
		assert_eq!(receipt_label(&entry), None);
		entry.receipt = None;
		assert_eq!(receipt_label(&entry), None);
		entry.kind = "capacity_retry_pending".into();
		assert_eq!(receipt_label(&entry), Some("Automatic retry"));
		entry.kind = "execution_notice".into();
		assert_eq!(receipt_label(&entry), Some("Execution notice"));
	}
}
