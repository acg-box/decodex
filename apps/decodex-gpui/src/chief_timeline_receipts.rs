//! Keep local delivery evidence separate from canonical conversation rows.
use super::{
	ChiefHistoryResult, ChiefSurface, ChiefWorkItemDto, Content, Context, IntoElement,
	ParentElement, Styled, div, markdown, muted,
};
use decodex_protocol::ChiefHistoryEntryDto;
use gpui::{InteractiveElement, StatefulInteractiveElement};

impl ChiefSurface {
	pub(in super::super) fn native_receipts_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let mut panel =
			div().flex().flex_col().gap_2().child(self.native_input_receipts_panel(work, cx));
		let Some((_, ChiefHistoryResult::Available { entries, live, next_before, .. })) =
			self.history.as_ref().filter(|(id, _)| id == &work.id)
		else {
			return panel
				.child(muted("Local delivery records are unavailable. Retrying…"))
				.into_any_element();
		};
		let cursor = self.older_history.get(&work.id).map_or(*next_before, |(_, cursor)| *cursor);
		if cursor.is_some() {
			panel = panel.child(
				div()
					.id("native-earlier-local-records")
					.debug_selector(|| "native-earlier-local-records".into())
					.cursor_pointer()
					.on_click(cx.listener(|surface, _, _, cx| surface.load_older_history(cx)))
					.child(if self.loading_older {
						"Loading earlier records…"
					} else {
						"Load earlier local records"
					}),
			);
		}
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
					.child(muted("Assistant · In progress"))
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
	#[test]
	fn native_view_preserves_uncertain_input_and_controls_without_text_deduplication() {
		let mut entry = ChiefHistoryEntryDto {
			turn_id: None,
			weather: Vec::new(),
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
