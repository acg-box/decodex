//! Current unconfirmed input pages are independent of both transcript cursors.
use gpui::{
	AnyElement, Div, FontWeight, KeyDownEvent, Role, SharedString, StatefulInteractiveElement,
};
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::native_timeline::Binding;
use crate::{
	shell::{
		agent_surface,
		agent_surface::native_timeline::{
			AgentClient, AgentSurface, AgentWorkItemDto, Context, EntityId, FluentBuilder,
			InteractiveElement, IntoElement, ParentElement, Styled, Task, markdown,
		},
		workspace_symbols::{self, Symbol},
	},
	ui_loading, ui_motion,
	ui_theme::{AMBER, TEXT_MUTED},
};
use decodex_protocol::{AgentHistoryEntryDto, AgentInputReceiptsResult};

#[derive(Default)]
pub(super) struct InputReceipts {
	task: Option<Task<()>>,
	after: Option<i64>,
	result: Option<AgentInputReceiptsResult>,
}

impl AgentSurface {
	pub(super) fn saved_input_card(
		&self,
		work: &str,
		entry: &AgentHistoryEntryDto,
		cx: &mut Context<Self>,
	) -> Div {
		let key = format!("saved-input-{work}-{}", entry.id);
		let expanded = self.timeline.expanded_records.contains(&key);
		let title =
			if entry.kind == "unsent_input" { "Message not sent" } else { "Delivery unconfirmed" };
		let entry = entry.clone();
		gpui::div()
			.w_full()
			.min_w_0()
			.px_3()
			.py(gpui::px(6.))
			.rounded(gpui::px(12.))
			.border_1()
			.border_color(gpui::rgba(0xe0b56f26))
			.bg(gpui::rgba(0xe0b56f07))
			.child(
				gpui::div()
					.flex()
					.items_center()
					.gap_2()
					.child(self.saved_input_toggle(key.clone(), title, expanded, cx))
					.child(markdown::copy_button(
						&format!("{key}-copy"),
						"Copy saved message",
						entry.text.clone(),
					)),
			)
			.child(ui_motion::disclosure_lazy(
				SharedString::from(format!("{key}-disclosure")),
				expanded,
				move |_| {
					gpui::div()
						.debug_selector(move || format!("saved-input-body-{}", entry.id))
						.pt_2()
						.pb_1()
						.flex()
						.flex_col()
						.gap_2()
						.text_size(gpui::px(12.5))
						.line_height(gpui::px(19.))
						.child(super::super::selectable_text::SelectableText::plain(
							format!("{key}-text"),
							entry.text,
						))
						.when_some(
							entry.receipt.and_then(|receipt| receipt.unsent_reason),
							|body, reason| {
								body.child(
									gpui::div()
										.text_size(gpui::px(11.))
										.text_color(gpui::rgb(TEXT_MUTED))
										.child(reason),
								)
							},
						)
						.into_any_element()
				},
			))
	}

	fn saved_input_toggle(
		&self,
		key: String,
		title: &'static str,
		expanded: bool,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let keyboard = key.clone();
		gpui::div()
			.id(SharedString::from(format!("{key}-toggle")))
			.debug_selector(|| "saved-input-toggle".into())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(title)
			.aria_expanded(expanded)
			.flex_1()
			.min_w_0()
			.h(gpui::px(28.))
			.flex()
			.items_center()
			.gap_2()
			.cursor_pointer()
			.text_size(gpui::px(12.))
			.font_weight(FontWeight::MEDIUM)
			.text_color(gpui::rgb(AMBER))
			.child(workspace_symbols::icon_sized(Symbol::AccountWarningAmber, 14.))
			.child(title)
			.child(workspace_symbols::process_chevron(
				SharedString::from(format!("{key}-chevron")),
				expanded,
			))
			.on_click(cx.listener(move |s, _, _, cx| {
				s.toggle_saved_input(&key, cx);
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if !event.is_held && matches!(event.keystroke.key.as_str(), "enter" | "space") {
					s.toggle_saved_input(&keyboard, cx);
					cx.stop_propagation();
				}
			}))
	}

	fn toggle_saved_input(&mut self, key: &str, cx: &mut Context<Self>) {
		if !self.timeline.expanded_records.remove(key) {
			self.timeline.expanded_records.insert(key.into());
			// Saved inputs live at the transcript footer. Reuse its existing
			// navigation so the opening card stays above the floating composer.
			self.jump_to_latest(cx);
		}
		cx.notify();
	}

	pub(super) fn native_input_receipts_loaded(&self, work: &str) -> bool {
		matches!(&self.timeline.native.input_receipts.result,Some(AgentInputReceiptsResult::Available {work_id,..}) if work_id.as_str()==work)
	}

	pub(in super::super) fn refresh_native_input_receipts(&mut self, cx: &mut Context<Self>) {
		if self.timeline.native.input_receipts.task.is_some() {
			return;
		}

		let Some(binding) = self
			.timeline
			.native
			.binding
			.as_ref()
			.filter(|binding| self.selected.as_ref() == Some(&binding.work))
		else {
			return;
		};
		let (Some(profile), Ok(work_id)) =
			(self.profile.clone(), EntityId::new(binding.work.clone()))
		else {
			return;
		};
		let work = binding.work.clone();
		let epoch = self.timeline.native.epoch;
		let after = self.timeline.native.input_receipts.after;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime.block_on(AgentClient::new(profile).input_receipts(work_id, after)).ok()
		});

		self.timeline.native.input_receipts.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				if surface.timeline.native.epoch != epoch
					|| surface.selected.as_deref() != Some(&work)
					|| surface.timeline.native.input_receipts.after != after
				{
					return;
				}

				let receipts = &mut surface.timeline.native.input_receipts;

				receipts.task = None;
				receipts.result = Some(result.unwrap_or(AgentInputReceiptsResult::Unavailable));

				cx.notify();
			});
		}));
	}

	fn input_receipt_cursor(&mut self, work: &str, after: Option<i64>, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work)
			|| self.timeline.native.input_receipts.task.is_some()
		{
			return;
		}

		self.timeline.native.input_receipts.after = after;
		self.timeline.native.input_receipts.result = None;

		self.refresh_native_input_receipts(cx);
		cx.notify();
	}

	pub(super) fn native_input_receipts_panel(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Vec<AnyElement> {
		let state = &self.timeline.native.input_receipts;
		let mut rows = Vec::new();

		if state.after.is_some() {
			let owner = work.id.clone();

			rows.push(
				(gpui::div().debug_selector(|| "input-receipts-first".into()).child(
					self.workspace_action(
						"first-input-receipts".into(),
						"First unconfirmed inputs".into(),
						move |surface, cx| surface.input_receipt_cursor(&owner, None, cx),
						cx,
					),
				))
				.into_any_element(),
			);
		}

		match &state.result {
			Some(AgentInputReceiptsResult::Available {
				work_id,
				entries,
				next_after,
				shortened,
			}) if work_id.as_str() == work.id => {
				for entry in entries {
					if self.preview_covers_receipt(&work.id, entry.id, &entry.text) {
						continue;
					}

					rows.push(
						(gpui::div()
							.debug_selector(|| "unconfirmed-native-input".into())
							.child(self.saved_input_card(&work.id, entry, cx)))
						.into_any_element(),
					);
				}

				if *shortened {
					rows.push(
						(agent_surface::muted("Some local input text is shortened."))
							.into_any_element(),
					);
				}

				if let Some(after) = next_after {
					let (owner, after) = (work.id.clone(), *after);

					rows.push(
						(gpui::div().debug_selector(|| "input-receipts-next".into()).child(
							self.workspace_action(
								"next-input-receipts".into(),
								"More unconfirmed inputs".into(),
								move |surface, cx| {
									surface.input_receipt_cursor(&owner, Some(after), cx)
								},
								cx,
							),
						))
						.into_any_element(),
					);
				}

				if entries.is_empty() && state.after.is_some() {
					rows.push(
						(agent_surface::muted("No remaining unconfirmed inputs on this page."))
							.into_any_element(),
					);
				}
			},
			None => rows.push((ui_loading::loading("Loading delivery records")).into_any_element()),
			_ => rows.push(
				(agent_surface::muted("Local delivery records could not be read. Retrying…"))
					.into_any_element(),
			),
		}

		rows
	}
}

#[cfg(test)]
mod tests {
	#[cfg(test)] use crate::shell::agent_surface::native_timeline::inputs::Binding;
	use crate::shell::agent_surface::native_timeline::inputs::{
		AgentInputReceiptsResult, AgentSurface, EntityId,
	};
	use decodex_protocol::{AgentHistoryEntryDto, AgentHistoryReceiptDto, AgentTimelinePage};

	#[gpui::test]
	fn completed_answer_stays_close_to_the_composer(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1_000.), gpui::px(700.)));
		surface.update(visual, long_response);
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(std::time::Duration::from_millis(240));
		for _ in 0..4 {
			visual.update(crate::ui_frame::draw_frame);
		}
		let answer = visual.debug_bounds("native-promotion-content").unwrap();
		let footer = visual.debug_bounds("floating-composer").unwrap();
		let gap = footer.top() - answer.bottom() + gpui::px(crate::ui_theme::COMPOSER_TOP_GAP);
		assert!(
			gap >= gpui::px(16.) && gap <= gpui::px(28.),
			"completed response must retain only normal reading space, got {gap:?}"
		);
	}

	fn long_response(s: &mut AgentSurface, cx: &mut gpui::Context<AgentSurface>) {
		s.visual_workspace_fixture(cx);
		s.workspace.graph_visible = false;
		let work = s
			.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|w| Some(&w.id) == s.selected.as_ref())
			.unwrap();
		work.codex_thread_id = Some("native-thread".into());
		s.timeline.native.replace(
			Binding {
				work: work.id.clone(),
				thread: "native-thread".into(),
				account: "account".into(),
			},
			AgentTimelinePage {
				thread_id: "native-thread".into(),
				entries: vec![decodex_protocol::AgentTimelineEntry {
					position: 0,
					content: decodex_protocol::AgentTimelineContent::Item {
						turn_id: "turn".into(),
						item_id: "answer".into(),
						kind: "agentMessage".into(),
						text: "A paragraph in the completed response.\n\n".repeat(40),
						truncated: false,
						phase: None,
						collaboration: None,
						app_ui: false,
						activity: None,
						attachments: vec![],
					},
				}],
				next_cursor: None,
				weather: Default::default(),
				safety_buffering_turn_id: None,
				active_realtime_session_at_page_start: None,
			},
		);
		s.timeline.native.input_receipts.result = Some(page("agent", None, None));
		let (_, decodex_protocol::AgentHistoryResult::Available { entries, .. }) =
			s.history.as_mut().unwrap()
		else {
			panic!("fixture history")
		};
		entries.clear();
		s.timeline.latest_follow_work = s.selected.clone();
		cx.notify();
	}

	#[gpui::test]
	fn expanded_saved_input_scrolls_above_the_composer(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1_000.), gpui::px(700.)));
		surface.update(visual, |s, cx| {
			long_response(s, cx);
			let mut inputs = page("agent", Some(7), None);
			if let AgentInputReceiptsResult::Available { entries, .. } = &mut inputs {
				entries[0].kind = "unsent_input".into();
				entries[0].text = "A saved line that must remain readable.\n".repeat(6);
			}
			s.timeline.native.input_receipts.result = Some(inputs);
		});
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(std::time::Duration::from_millis(240));
		for _ in 0..4 {
			visual.update(crate::ui_frame::draw_frame);
		}
		let before = surface.update(visual, |s, _| {
			s.timeline.latest_follow_work = None;
			s.timeline.follow_paused.insert("agent".into());
			s.timeline.scroll["agent"].offset().y
		});
		let toggle = visual.debug_bounds("saved-input-toggle").unwrap();
		visual.simulate_click(toggle.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(std::time::Duration::from_millis(280));
		for _ in 0..8 {
			visual.update(crate::ui_frame::draw_frame);
		}
		let body = visual.debug_bounds("saved-input-body-7").unwrap();
		let composer = visual.debug_bounds("floating-composer").unwrap();
		assert!(body.bottom() <= composer.top(), "{body:?} is covered by {composer:?}");
		surface.read_with(visual, |s, _| {
			assert!(
				s.timeline.scroll["agent"].offset().y < before,
				"opening the footer must reveal its new content"
			);
		});
	}

	fn page(work: &str, id: Option<i64>, next_after: Option<i64>) -> AgentInputReceiptsResult {
		AgentInputReceiptsResult::Available {
			work_id: EntityId::new(work).unwrap(),
			entries: id
				.into_iter()
				.map(|id| AgentHistoryEntryDto {
					native_source: None,
					turn_id: None,
					weather: Vec::new(),
					receipt: Some(AgentHistoryReceiptDto {
						unsent_reason: None,
						voice_session_id: None,
						event_kind: "user_message".into(),
						delivered_turn_id: None,
						disposed: false,
					}),
					activity: None,
					usage: None,
					duration_ms: None,
					id,
					kind: "user".into(),
					text: "Old unconfirmed input".into(),
					created_at_micros: 1,
				})
				.collect(),
			next_after,
			shortened: false,
		}
	}

	#[gpui::test]
	fn native_pending_input_controls_page_independently_of_the_transcript(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.update(|window, _| window.resize(gpui::size(gpui::px(1_000.), gpui::px(700.))));

		let work = surface.update(visual, |surface, cx| {
			surface.visual_workspace_fixture(cx);

			surface.workspace.graph_visible = false;

			let work = surface
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == surface.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("thread".into());

			let id = work.id.clone();

			assert!(surface.timeline.native.replace(
				Binding { work: id.clone(), thread: "thread".into(), account: "account".into() },
				AgentTimelinePage {
					thread_id: "thread".into(),
					entries: vec![],
					next_cursor: None,
					weather: Default::default(),
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None
				}
			));

			surface.timeline.native.input_receipts.result = Some(page(&id, Some(1), Some(1)));

			cx.notify();

			id
		});

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("unconfirmed-native-input").is_some());

		let more = visual.debug_bounds("input-receipts-next").unwrap();

		visual.simulate_click(more.center(), Default::default());

		surface.update(visual, |surface, cx| {
			assert_eq!(surface.timeline.native.input_receipts.after, Some(1));
			assert!(surface.timeline.native.older_cursor.is_none());

			surface.timeline.native.input_receipts.result = Some(page(&work, Some(2), None));

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("input-receipts-next").is_none());

		let first = visual.debug_bounds("input-receipts-first").unwrap();

		visual.simulate_click(first.center(), Default::default());

		surface.update(visual, |surface, cx| {
			assert_eq!(surface.timeline.native.input_receipts.after, None);

			surface.timeline.native.input_receipts.result = Some(page(&work, None, None));

			assert!(surface.native_input_receipts_loaded(&work));

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("unconfirmed-native-input").is_none());
	}
}
