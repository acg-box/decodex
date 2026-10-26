//! Keep local delivery evidence separate from canonical conversation rows.
use std::collections::BTreeMap;

use gpui::{AnyElement, Div, InteractiveElement, StatefulInteractiveElement};

use crate::{
	shell::{
		agent_surface,
		agent_surface::native_timeline::{
			self, AgentHistoryResult, AgentSurface, AgentTimelineContent, AgentWorkItemDto,
			Context, FluentBuilder, IntoElement, ParentElement, Styled, inputs, markdown,
		},
	},
	ui_loading,
};
use decodex_protocol::{AgentHistoryEntryDto, AgentHistorySourceDto, AgentTimelineEntry};

impl AgentSurface {
	pub(in super::super) fn saved_inputs_in_composer(&self, work: &AgentWorkItemDto) -> bool {
		self.native_agents.selected.is_none()
			&& self.native_history_active(work)
			&& !self.selected_is_archived()
			&& !self.workspace.browsing
	}

	fn saved_delivery_entries(&self, work: &str) -> BTreeMap<i64, &AgentHistoryEntryDto> {
		let mut saved = BTreeMap::new();

		if let Some((older, _)) = self.timeline.older_history.get(work) {
			saved.extend(older.iter().map(|entry| (entry.id, entry)));
		}
		if let Some((_, AgentHistoryResult::Available { entries, .. })) =
			self.history.as_ref().filter(|(id, _)| id == work)
		{
			saved.extend(entries.iter().map(|entry| (entry.id, entry)));
		}

		saved
	}

	pub(in super::super) fn saved_input_rows(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Vec<AnyElement> {
		let mut rows = self.native_input_receipts_panel(work, cx);

		for entry in self.saved_delivery_entries(&work.id).values() {
			let Some(label) = receipt_label(entry) else { continue };

			if entry.kind != "unsent_input" && label != "Local input · Delivery not confirmed" {
				continue;
			}
			if inputs::submission_pending(&work.dispatch_state, entry) {
				continue;
			}
			if label == "Local input · Delivery not confirmed"
				&& (self.native_input_receipts_loaded(&work.id)
					|| self.preview_covers_receipt(&work.id, entry.id, &entry.text))
			{
				continue;
			}

			rows.push(self.local_receipt_row(entry, label, work, cx).into_any_element());
		}

		rows
	}

	fn earlier_local_records(&self, available: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
		available.then(|| {
			if self.timeline.loading_older {
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
		let mut rows = Vec::new();

		if !diagnostics && !self.saved_inputs_in_composer(work) {
			rows.extend(self.saved_input_rows(work, cx));
		}

		let Some((_, AgentHistoryResult::Available { next_before, .. })) =
			self.history.as_ref().filter(|(id, _)| id == &work.id)
		else {
			return gpui::div()
				.flex()
				.flex_col()
				.gap_2()
				.children(rows)
				.child(agent_surface::muted("Delivery records unavailable. Retrying…"))
				.into_any_element();
		};
		let cursor =
			self.timeline.older_history.get(&work.id).map_or(*next_before, |(_, cursor)| *cursor);
		let records_key = format!("local-records-{}", work.id);
		let expanded = diagnostics && self.timeline.expanded_records.contains(&records_key);
		let saved = self.saved_delivery_entries(&work.id);
		let has_records = cursor.is_some()
			|| saved.values().any(|entry| {
				entry.kind == "auth_recovery"
					|| receipt_label(entry) == Some("Local execution record")
			});

		if has_records && diagnostics {
			rows.push((self.local_records_toggle(records_key, expanded, cx)).into_any_element());
		}
		if expanded {
			rows.extend(self.earlier_local_records(cursor.is_some(), cx));
		}

		for entry in saved.values() {
			// Saved inputs have one owner, beside the composer or in archived history.
			if entry.kind == "unsent_input"
				|| receipt_label(entry) == Some("Local input · Delivery not confirmed")
			{
				continue;
			}
			if inputs::submission_pending(&work.dispatch_state, entry) {
				continue;
			}
			if entry.kind == "auth_recovery" {
				if diagnostics && !expanded {
					continue;
				}
				if !expanded && entry.receipt.as_ref().is_some_and(|r| r.disposed) {
					continue;
				}

				rows.push((native_timeline::auth_recovery_entry(entry)).into_any_element());

				continue;
			}
			if matches!(entry.kind.as_str(), "partial_answer" | "partial_plan")
				&& entry.native_source.as_ref().is_some_and(|source| {
					partial_replaced(
						source,
						self.timeline
							.native
							.binding
							.as_ref()
							.map(|binding| binding.thread.as_str()),
						&self.timeline.native.entries,
						if entry.kind == "partial_plan" { "plan" } else { "agentMessage" },
					)
				}) {
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

			let row = self.local_receipt_row(entry, label, work, cx);

			rows.push((row).into_any_element());
		}

		if diagnostics {
			return gpui::div().flex().flex_col().gap_2().children(rows).into_any_element();
		}

		let empty = rows.is_empty();

		gpui::div()
			.flex()
			.flex_col()
			.gap_2()
			.when(empty, |panel| panel.hidden())
			.children(rows)
			.into_any_element()
	}

	fn local_receipt_row(
		&self,
		entry: &AgentHistoryEntryDto,
		label: &'static str,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Div {
		if entry.kind == "unsent_input" || label == "Local input · Delivery not confirmed" {
			let id = entry.id;

			return gpui::div()
				.debug_selector(move || format!("local-receipt-{id}"))
				.child(self.saved_input_card(&work.id, entry, cx));
		}

		let mut row = gpui::div()
			.debug_selector({
				let id = entry.id;
				let kind = match entry.kind.as_str() {
					"partial_answer" | "partial_plan" => "partial",
					_ => "local",
				};

				move || format!("{kind}-receipt-{id}")
			})
			.flex()
			.flex_col()
			.gap_1()
			.child(agent_surface::muted(label))
			.child(markdown::render(&entry.text, &format!("receipt-{}", entry.id)));

		if entry.kind == "capacity_retry_pending" {
			row = row.child(
				gpui::div()
					.debug_selector(|| "capacity-retry-cancel".into())
					.child(self.capacity_retry_control(work.id.clone(), entry.id, cx)),
			);
		}

		row
	}

	fn local_records_toggle(
		&self,
		records_key: String,
		expanded: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		gpui::div()
			.id("local-records-toggle")
			.debug_selector(|| "local-records-toggle".into())
			.cursor_pointer()
			.text_size(gpui::px(11.))
			.child(agent_surface::muted(if expanded {
				"Diagnostics ⌄"
			} else {
				"Diagnostics ›"
			}))
			.on_click(cx.listener(move |s, _, _, cx| {
				if !s.timeline.expanded_records.remove(&records_key) {
					s.timeline.expanded_records.insert(records_key.clone());
				}

				cx.notify();
			}))
			.into_any_element()
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
		return None;
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
		// Native history owns turn failures in their original position. These
		// saved diagnostics must not reappear after the latest successful reply.
		if entry.receipt.as_ref().is_some_and(|receipt| {
			matches!(
				receipt.event_kind.as_str(),
				"agent_turn_completed"
					| "worker_turn_completed"
					| "native_warning"
					| "config_warning"
					| "strict_review_notice"
			)
		}) {
			return Some("Local execution record");
		}

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
	use std::thread;

	use crate::shell::agent_surface::native_timeline::receipts::{
		self, AgentHistoryEntryDto, AgentHistoryResult, AgentSurface, AgentTimelineContent,
	};

	#[gpui::test]
	fn unsent_input_shows_its_message_and_stays_separate_from_native_diagnostics(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;
			s.workspace.details_visible = true;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("native-thread".into());

			let Some((_, AgentHistoryResult::Available { entries, next_before, .. })) =
				&mut s.history
			else {
				panic!("fixture history")
			};

			entries.clear();

			*next_before = None;

			let record = |id, kind: &str, event: &str| AgentHistoryEntryDto {
				id,
				kind: kind.into(),
				text: "Saved **local** record".into(),
				created_at_micros: id,
				native_source: None,
				turn_id: None,
				weather: vec![],
				activity: None,
				usage: None,
				duration_ms: None,
				receipt: Some(decodex_protocol::AgentHistoryReceiptDto {
					unsent_reason: None,
					voice_session_id: None,
					event_kind: event.into(),
					delivered_turn_id: None,
					disposed: false,
				}),
			};

			entries.push(record(93, "unsent_input", "user_message"));
			s.timeline.older_history.insert(
				work.id.clone(),
				(
					vec![
						record(91, "execution_notice", "native_warning"),
						record(92, "execution_notice", "agent_turn_completed"),
					],
					None,
				),
			);

			s.timeline.native.binding = Some(super::super::Binding {
				work: work.id.clone(),
				thread: "native-thread".into(),
				account: "account".into(),
			});

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("local-receipt-91").is_none());
		assert!(visual.debug_bounds("local-receipt-92").is_none());
		assert!(visual.debug_bounds("local-receipt-93").is_some());
		assert!(visual.debug_bounds("local-receipt-93").unwrap().size.height <= gpui::px(96.));
		assert!(visual.debug_bounds("saved-input-body-93").is_none());
		assert!(visual.debug_bounds("local-records-toggle").is_some());

		let copy = visual.debug_bounds("saved-input-agent-93-copy").unwrap();

		visual.simulate_click(copy.center(), Default::default());
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()),
				Some("Saved **local** record".into())
			);
		});

		let toggle = visual.debug_bounds("saved-input-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("saved-input-body-93").is_some());

		surface.update(visual, |s, cx| {
			s.workspace.details_visible = true;

			s.timeline
				.expanded_records
				.insert(format!("local-records-{}", s.selected.as_ref().unwrap()));
			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("local-receipt-91").is_some());
		assert!(visual.debug_bounds("local-receipt-92").is_some());
		assert!(visual.debug_bounds("local-receipt-93").is_some());
	}

	#[gpui::test]
	fn unfinished_output_is_visible_in_saved_and_native_views(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

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

		visual.update(crate::ui_frame::draw_current_frame);

		for id in [91, 92] {
			assert!(
				visual
					.debug_bounds(Box::leak(format!("copy-partial-{id}").into_boxed_str()))
					.is_none()
			);
			assert!(
				visual
					.debug_bounds(Box::leak(format!("partial-{id}-0").into_boxed_str()))
					.is_some()
					|| visual
						.debug_bounds(Box::leak(format!("receipt-{id}-0").into_boxed_str()))
						.is_some()
			);
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
			s.timeline.native.binding = Some(super::super::Binding {
				work: work.id.clone(),
				thread: "native-thread".into(),
				account: "account".into(),
			});

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		for id in [91, 92] {
			assert!(
				visual
					.debug_bounds(Box::leak(format!("copy-partial-{id}").into_boxed_str()))
					.is_none()
			);
			assert!(
				visual
					.debug_bounds(Box::leak(format!("partial-{id}-0").into_boxed_str()))
					.is_some()
					|| visual
						.debug_bounds(Box::leak(format!("receipt-{id}-0").into_boxed_str()))
						.is_some()
			);
		}
	}

	#[gpui::test]
	fn recorded_checklist_stays_out_of_saved_and_native_chat_views(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		surface.update(visual, |s,cx| {
            s.visual_workspace_fixture(cx);

            s.workspace.graph_visible=false;

            let Some((_,AgentHistoryResult::Available {entries,..}))=&mut s.history else {panic!("fixture history")};

            entries.clear();
            entries.push(AgentHistoryEntryDto {native_source:None,turn_id:Some("turn".into()),weather:vec![],id:92,kind:"checklist".into(),text:"- **Completed**: Inspect source\n- **Pending**: Verify changes\n\nLast observed checklist for this turn.".into(),created_at_micros:1,receipt:None,activity:None,usage:None,duration_ms:None});

            entries[0].receipt = Some(decodex_protocol::AgentHistoryReceiptDto { unsent_reason: None, voice_session_id: None, event_kind:"plan_updated".into(),delivered_turn_id:Some("turn".into()),disposed:true});

            let mut old = entries[0].clone(); old.id=91; old.text="Stale checklist".into();

            s.timeline.older_history.insert(s.selected.clone().unwrap(),(vec![old],None));
            cx.notify();
        });

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("checklist-receipt-92").is_none());
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
			s.timeline.native.binding = Some(super::super::Binding {
				work: work.id.clone(),
				thread: "native-thread".into(),
				account: "account".into(),
			});

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("checklist-receipt-92").is_none());
		assert!(visual.debug_bounds("checklist-receipt-91").is_none());
	}

	#[gpui::test]
	fn auth_recovery_history_is_visible_as_a_recorded_notice(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

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

		visual.update(crate::ui_frame::draw_current_frame);

		let bounds = visual
			.debug_bounds("auth-recovery-receipt-91")
			.expect("saved authentication notice is rendered");

		assert!(bounds.size.height > gpui::px(0.));

		surface.update(visual, |s, cx| {
			s.timeline.native.binding = Some(super::super::Binding {
				work: s.selected.clone().unwrap(),
				thread: "native-thread".into(),
				account: "account".into(),
			});

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

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
				unsent_reason: None,
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

		entry.receipt = Some(decodex_protocol::AgentHistoryReceiptDto {
			unsent_reason: None,
			voice_session_id: None,
			event_kind: "native_warning".into(),
			delivered_turn_id: None,
			disposed: false,
		});

		for kind in [
			"native_warning",
			"config_warning",
			"agent_turn_completed",
			"worker_turn_completed",
			"strict_review_notice",
		] {
			entry.receipt.as_mut().unwrap().event_kind = kind.into();

			assert_eq!(receipts::receipt_label(&entry), Some("Local execution record"));
		}
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
					collaboration: None,
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
