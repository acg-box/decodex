//! Keep local delivery evidence separate from canonical conversation rows.
use std::collections::BTreeMap;

use gpui::{AnyElement, InteractiveElement, StatefulInteractiveElement};

use crate::{
	shell::agent_surface::{
		native_timeline::{
			self, AgentHistoryResult, AgentSurface, AgentTimelineContent, AgentWorkItemDto,
			Context, IntoElement, ParentElement, Styled, markdown,
		},
		progress,
	},
	ui_loading,
};
use decodex_protocol::{
	AgentHistoryEntryDto, AgentHistorySourceDto, AgentLiveMessageDto, AgentLiveMessageKind,
	AgentTimelineEntry,
};

impl AgentSurface {
	fn earlier_local_records(&self, available: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
		available.then(|| {
			if self.loading_older {
				return ui_loading::loading("Loading earlier records").into_any_element();
			}

			gpui::div()
				.id("native-earlier-local-records")
				.debug_selector(|| "native-earlier-local-records".into())
				.cursor_pointer()
				.on_click(cx.listener(|surface, _, _, cx| surface.load_older_history(cx)))
				.child("Load earlier local records")
				.into_any_element()
		})
	}

	pub(in super::super) fn native_receipts_panel(
		&self,
		work: &AgentWorkItemDto,
		diagnostics: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut panel = gpui::div().flex().flex_col().gap_2();

		if !diagnostics {
			panel = panel.child(self.native_input_receipts_panel(work, cx));
		}

		let Some((_, AgentHistoryResult::Available { entries, live, next_before, .. })) =
			self.history.as_ref().filter(|(id, _)| id == &work.id)
		else {
			return panel
				.child(native_timeline::muted("Local delivery records are unavailable. Retrying…"))
				.into_any_element();
		};
		let cursor = self.older_history.get(&work.id).map_or(*next_before, |(_, cursor)| *cursor);
		let records_key = format!("local-records-{}", work.id);
		let expanded = diagnostics && self.expanded_records.contains(&records_key);
		let has_records = cursor.is_some()
			|| entries.iter().any(|entry| {
				entry.kind == "auth_recovery"
					|| receipt_label(entry) == Some("Local execution record")
			});

		if has_records && diagnostics {
			panel = panel.child(self.local_records_toggle(records_key, expanded, cx));
		}
		if expanded {
			panel = panel.children(self.earlier_local_records(cursor.is_some(), cx));
		}

		let mut saved = BTreeMap::new();

		if let Some((older, _)) = self.older_history.get(&work.id) {
			for entry in older {
				saved.insert(entry.id, entry);
			}
		}

		saved.extend(entries.iter().map(|entry| (entry.id, entry)));

		for entry in saved.values() {
			if receipt_label(entry) == Some("Local input · Delivery not confirmed")
				&& self.preview_covers_receipt(&work.id, entry.id, &entry.text)
			{
				continue;
			}
			if progress::checklist_superseded(entry, saved.values().copied()) {
				continue;
			}
			if entry.kind == "auth_recovery" {
				if diagnostics && !expanded {
					continue;
				}
				if !expanded && entry.receipt.as_ref().is_some_and(|r| r.disposed) {
					continue;
				}

				panel = panel.child(native_timeline::auth_recovery_entry(entry));

				continue;
			}
			if matches!(entry.kind.as_str(), "partial_answer" | "partial_plan")
				&& entry.native_source.as_ref().is_some_and(|source| {
					partial_replaced(
						source,
						self.native_history.binding.as_ref().map(|binding| binding.thread.as_str()),
						&self.native_history.entries,
						if entry.kind == "partial_plan" { "plan" } else { "agentMessage" },
					)
				}) {
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

			if diagnostics && label != "Local execution record" {
				continue;
			}
			if label == "Local execution record" && !expanded {
				continue;
			}

			let mut row = gpui::div()
				.debug_selector({
					let id = entry.id;
					let kind = match entry.kind.as_str() {
						"checklist" => "checklist",
						"partial_answer" | "partial_plan" => "partial",
						_ => "local",
					};

					move || format!("{kind}-receipt-{id}")
				})
				.flex()
				.flex_col()
				.gap_1()
				.child(native_timeline::muted(label))
				.child(markdown::render(&entry.text, &format!("receipt-{}", entry.id)));

			if matches!(entry.kind.as_str(), "partial_answer" | "partial_plan") {
				row = row.child(markdown::response_copy_button(
					&format!("copy-partial-{}", entry.id),
					"Copy unfinished output",
					entry.text.clone(),
				));
			}
			if entry.kind == "capacity_retry_pending" {
				row = row.child(
					gpui::div()
						.debug_selector(|| "capacity-retry-cancel".into())
						.child(self.capacity_retry_control(work.id.clone(), entry.id, cx)),
				);
			}

			panel = panel.child(row);
		}

		if diagnostics {
			return panel.into_any_element();
		}

		panel.children(self.native_live_receipts(live)).into_any_element()
	}

	fn local_records_toggle(
		&self,
		records_key: String,
		expanded: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		gpui::div()
			.id("local-records-toggle")
			.cursor_pointer()
			.text_size(gpui::px(11.))
			.child(native_timeline::muted(if expanded {
				"Diagnostics ⌄"
			} else {
				"Diagnostics ›"
			}))
			.on_click(cx.listener(move |s, _, _, cx| {
				if !s.expanded_records.remove(&records_key) {
					s.expanded_records.insert(records_key.clone());
				}

				cx.notify();
			}))
			.into_any_element()
	}

	fn native_live_receipts(&self, live: &[AgentLiveMessageDto]) -> Vec<AnyElement> {
		let mut rows = Vec::new();

		for message in live {
			if self.native_history.entries.iter().any(|entry| match &entry.content {
				AgentTimelineContent::Item { turn_id, item_id, .. } =>
					turn_id == &message.turn_id && item_id == &message.item_id,
				AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } =>
					turn_id == &message.turn_id,
				_ => false,
			}) {
				continue;
			}

			rows.push(
				gpui::div()
					.debug_selector({
						let selector = match message.kind {
							AgentLiveMessageKind::ReasoningSummary =>
								"native-live-reasoning-summary",
							AgentLiveMessageKind::Plan => "native-live-plan",
							AgentLiveMessageKind::AgentMessage => "native-live-output",
						};

						move || selector.into()
					})
					.child(native_timeline::muted(match message.kind {
						AgentLiveMessageKind::ReasoningSummary => "Reasoning summary",
						AgentLiveMessageKind::AgentMessage => "Assistant · In progress",
						AgentLiveMessageKind::Plan => "Proposed plan · In progress",
					}))
					.child(markdown::render(
						&message.text,
						&format!("live-{}-{}", message.turn_id, message.item_id),
					))
					.children(message.truncated.then(|| {
						native_timeline::muted(
							"Partial output shortened; waiting for saved result.",
						)
					}))
					.into_any_element(),
			);
		}

		rows
	}
}

fn partial_replaced(
	source: &AgentHistorySourceDto,
	thread: Option<&str>,
	entries: &[AgentTimelineEntry],
	expected_kind: &str,
) -> bool {
	thread == Some(source.thread_id.as_str()) && entries.iter().any(|entry| matches!(
		&entry.content, AgentTimelineContent::Item { turn_id, item_id, kind, text, truncated: false, .. }
		if turn_id == &source.turn_id && item_id == &source.item_id && kind == expected_kind && !text.is_empty()
	))
}

fn receipt_label(entry: &AgentHistoryEntryDto) -> Option<&'static str> {
	if super::super::startup_feature_warning(entry) {
		return None;
	}
	if entry.kind == "checklist" {
		return Some("Recorded checklist");
	}
	if entry.kind == "partial_plan" {
		return Some("Proposed plan · Unfinished");
	}
	if entry.kind == "partial_answer" {
		return Some("Assistant · Unfinished");
	}
	if entry.kind == "unsent_input" {
		return Some("Local input · Not sent");
	}
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
	use crate::shell::agent_surface::native_timeline::receipts::{
		self, AgentHistoryEntryDto, AgentHistoryResult, AgentSurface, AgentTimelineContent,
	};
	#[gpui::test]
	fn unfinished_output_is_visible_in_saved_and_native_views(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;

			let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history else {
				panic!("fixture history")
			};

			entries.clear();

			for (id, kind) in [(91, "partial_answer"), (92, "partial_plan")] {
				entries.push(AgentHistoryEntryDto {
					native_source: None,
					turn_id: None,
					weather: Vec::new(),
					id,
					kind: kind.into(),
					text: "Unfinished source\n\n$$\n\\frac{a+b}{c}".into(),
					created_at_micros: 1,
					receipt: None,
					activity: None,
					usage: None,
					duration_ms: None,
				});
			}

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		for copy_id in ["copy-partial-91", "copy-partial-92"] {
			let copy = visual.debug_bounds(copy_id).expect("visible unfinished output");

			visual.simulate_click(copy.center(), Default::default());
			visual.update(|_, cx| {
				assert_eq!(
					cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
					Some("Unfinished source\n\n$$\n\\frac{a+b}{c}")
				);
			});
		}

		surface.update(visual, |s, cx| {
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| Some(&w.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("native-thread".into());
			s.native_history.binding = Some(super::super::Binding {
				work: work.id.clone(),
				thread: "native-thread".into(),
				account: "account".into(),
			});

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		for copy_id in ["copy-partial-91", "copy-partial-92"] {
			let copy = visual.debug_bounds(copy_id).expect("visible unfinished output");

			visual.simulate_click(copy.center(), Default::default());
			visual.update(|_, cx| {
				assert_eq!(
					cx.read_from_clipboard().and_then(|item| item.text()).as_deref(),
					Some("Unfinished source\n\n$$\n\\frac{a+b}{c}")
				);
			});
		}
	}

	#[gpui::test]
	fn recorded_checklist_renders_in_saved_and_native_views(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		surface.update(visual, |s,cx| {
            s.visual_workspace_fixture(cx);

            s.graph_visible=false;

            let Some((_,AgentHistoryResult::Available {entries,..}))=&mut s.history else {panic!("fixture history")};

            entries.clear();
            entries.push(AgentHistoryEntryDto {native_source:None,turn_id:Some("turn".into()),weather:vec![],id:92,kind:"checklist".into(),text:"- **Completed**: Inspect source\n- **Pending**: Verify changes\n\nLast observed checklist for this turn.".into(),created_at_micros:1,receipt:None,activity:None,usage:None,duration_ms:None});

            entries[0].receipt = Some(decodex_protocol::AgentHistoryReceiptDto { voice_session_id: None, event_kind:"plan_updated".into(),delivered_turn_id:Some("turn".into()),disposed:true});

            let mut old = entries[0].clone(); old.id=91; old.text="Stale checklist".into();

            s.older_history.insert(s.selected.clone().unwrap(),(vec![old],None));
            cx.notify();
        });

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("checklist-receipt-92").is_some());
		assert!(visual.debug_bounds("checklist-receipt-91").is_none());

		surface.update(visual, |s, cx| {
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| Some(&w.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("native-thread".into());
			s.native_history.binding = Some(super::super::Binding {
				work: work.id.clone(),
				thread: "native-thread".into(),
				account: "account".into(),
			});

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("checklist-receipt-92").is_some());
		assert!(visual.debug_bounds("checklist-receipt-91").is_none());
	}

	#[gpui::test]
	fn auth_recovery_history_is_visible_as_a_recorded_notice(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;

			let work = s.snapshot.as_mut().unwrap().work_items.iter_mut().find(|w| Some(&w.id) == s.selected.as_ref()).unwrap();

			work.codex_thread_id = Some("native-thread".into());
			s.history = Some((work.id.clone(), AgentHistoryResult::Available {
				questions: vec![], questions_truncated: false, questions_recovering: false, misalignment: None, usage: None, has_more: false, next_before: None, live: vec![],
				entries: vec![AgentHistoryEntryDto {
					native_source: None, turn_id: None, weather: vec![], id: 91,kind:"auth_recovery".into(),text:"Codex reported that provider sign-in recovery started.\n\nAWS: [Sign in](https://example.invalid)\n\nSaved event; current sign-in status is not confirmed by this record.".into(),created_at_micros:1,receipt:None,activity:None,usage:None,duration_ms:None,
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
				.height
				> gpui::px(0.)
		);
	}

	#[test]
	fn native_view_preserves_uncertain_input_and_controls_without_text_deduplication() {
		let mut entry = AgentHistoryEntryDto {
			native_source: None,
			turn_id: None,
			weather: Vec::new(),
			receipt: Some(decodex_protocol::AgentHistoryReceiptDto {
				voice_session_id: None,
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

		assert_eq!(receipts::receipt_label(&entry), Some("Local input · Delivery not confirmed"));

		entry.receipt.as_mut().unwrap().delivered_turn_id = Some("native-turn".into());

		assert_eq!(receipts::receipt_label(&entry), None);

		entry.receipt = None;

		assert_eq!(receipts::receipt_label(&entry), None);

		entry.kind = "unsent_input".into();

		assert_eq!(receipts::receipt_label(&entry), Some("Local input · Not sent"));

		entry.kind = "capacity_retry_pending".into();

		assert_eq!(receipts::receipt_label(&entry), Some("Automatic retry"));

		entry.kind = "execution_notice".into();

		assert_eq!(receipts::receipt_label(&entry), Some("Execution notice"));

		entry.text = "Codex warning: Under-development features enabled: chronicle.".into();

		assert_eq!(receipts::receipt_label(&entry), None);

		entry.text = "Codex warning: Previous instructions retained".into();

		assert_eq!(receipts::receipt_label(&entry), Some("Execution notice"));
	}
	#[test]
	fn partial_replacement_requires_complete_exact_native_content() {
		let source = decodex_protocol::AgentHistorySourceDto {
			thread_id: "thread".into(),
			turn_id: "turn".into(),
			item_id: "item".into(),
		};
		let item = |turn: &str, id: &str, text: &str, truncated: bool| {
			decodex_protocol::AgentTimelineEntry {
				position: 1,
				content: AgentTimelineContent::Item {
					phase: None,
					app_ui: false,
					turn_id: turn.into(),
					item_id: id.into(),
					kind: "plan".into(),
					text: text.into(),
					truncated,
					activity: None,
					attachments: vec![],
				},
			}
		};

		for (thread, turn, id, text, truncated, expected) in [
			(Some("thread"), "turn", "item", "Final plan", false, true),
			(Some("foreign"), "turn", "item", "Final plan", false, false),
			(None, "turn", "item", "Final plan", false, false),
			(Some("thread"), "other", "item", "Final plan", false, false),
			(Some("thread"), "turn", "other", "Final plan", false, false),
			(Some("thread"), "turn", "item", "", false, false),
			(Some("thread"), "turn", "item", "Final plan", true, false),
		] {
			assert_eq!(
				receipts::partial_replaced(
					&source,
					thread,
					&[item(turn, id, text, truncated)],
					"plan"
				),
				expected
			);
		}

		assert!(!receipts::partial_replaced(&source, Some("thread"), &[], "plan"));
		assert!(!receipts::partial_replaced(
			&source,
			Some("thread"),
			&[item("turn", "item", "Final plan", false)],
			"agentMessage"
		));

		let terminal = decodex_protocol::AgentTimelineEntry {
			position: 2,
			content: AgentTimelineContent::TurnBoundary {
				turn_id: "turn".into(),
				completed: true,
				status: None,
				duration_ms: None,
				usage: None,
				usage_summary: None,
				error: None,
			},
		};

		assert!(!receipts::partial_replaced(&source, Some("thread"), &[terminal], "plan"));
	}
}
