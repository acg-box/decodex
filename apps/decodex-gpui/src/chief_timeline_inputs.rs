//! Current unconfirmed input pages are independent of both transcript cursors.
use super::*;
use decodex_protocol::ChiefInputReceiptsResult;

#[derive(Default)]
pub(super) struct InputReceipts {
	task: Option<Task<()>>,
	after: Option<i64>,
	result: Option<ChiefInputReceiptsResult>,
}

impl ChiefSurface {
	pub(super) fn native_input_receipts_loaded(&self, work: &str) -> bool {
		matches!(&self.native_history.input_receipts.result,Some(ChiefInputReceiptsResult::Available {work_id,..}) if work_id.as_str()==work)
	}

	pub(in super::super) fn refresh_native_input_receipts(&mut self, cx: &mut Context<Self>) {
		if self.native_history.input_receipts.task.is_some() {
			return;
		}
		let Some(binding) = self
			.native_history
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
		let epoch = self.native_history.epoch;
		let after = self.native_history.input_receipts.after;
		let request = cx.background_executor().spawn(async move {
			let runtime =
				tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(ChiefClient::new(profile).input_receipts(work_id, after)).ok()
		});
		self.native_history.input_receipts.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				if surface.native_history.epoch != epoch
					|| surface.selected.as_deref() != Some(&work)
					|| surface.native_history.input_receipts.after != after
				{
					return;
				}
				let receipts = &mut surface.native_history.input_receipts;
				receipts.task = None;
				receipts.result = Some(result.unwrap_or(ChiefInputReceiptsResult::Unavailable));
				cx.notify();
			});
		}));
	}

	fn input_receipt_cursor(&mut self, work: &str, after: Option<i64>, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work)
			|| self.native_history.input_receipts.task.is_some()
		{
			return;
		}
		self.native_history.input_receipts.after = after;
		self.native_history.input_receipts.result = None;
		self.refresh_native_input_receipts(cx);
		cx.notify();
	}

	pub(super) fn native_input_receipts_panel(
		&self,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let state = &self.native_history.input_receipts;
		let mut panel = div().flex().flex_col().gap_2();
		if state.after.is_some() {
			let owner = work.id.clone();
			panel = panel.child(div().debug_selector(|| "input-receipts-first".into()).child(
				self.workspace_action(
					"first-input-receipts".into(),
					"First unconfirmed inputs".into(),
					move |surface, cx| surface.input_receipt_cursor(&owner, None, cx),
					cx,
				),
			));
		}
		match &state.result {
			Some(ChiefInputReceiptsResult::Available {
				work_id,
				entries,
				next_after,
				shortened,
			}) if work_id.as_str() == work.id => {
				for entry in entries {
					panel = panel.child(
						div()
							.debug_selector(|| "unconfirmed-native-input".into())
							.child(muted("Local input · Delivery not confirmed"))
							.child(markdown::render(
								&entry.text,
								&format!("input-receipt-{}", entry.id),
							)),
					);
				}
				if *shortened {
					panel = panel.child(muted("Some local input text is shortened."));
				}
				if let Some(after) = next_after {
					let (owner, after) = (work.id.clone(), *after);
					panel =
						panel.child(div().debug_selector(|| "input-receipts-next".into()).child(
							self.workspace_action(
								"next-input-receipts".into(),
								"More unconfirmed inputs".into(),
								move |surface, cx| {
									surface.input_receipt_cursor(&owner, Some(after), cx)
								},
								cx,
							),
						));
				}
				if entries.is_empty() && state.after.is_some() {
					panel = panel.child(muted("No remaining unconfirmed inputs on this page."));
				}
			},
			None => panel = panel.child(muted("Loading local delivery records…")),
			_ => panel = panel.child(muted("Local delivery records could not be read. Retrying…")),
		}
		panel.into_any_element()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use decodex_protocol::{ChiefHistoryEntryDto, ChiefHistoryReceiptDto, ChiefTimelinePage};
	fn page(work: &str, id: Option<i64>, next_after: Option<i64>) -> ChiefInputReceiptsResult {
		ChiefInputReceiptsResult::Available {
			work_id: EntityId::new(work).unwrap(),
			entries: id
				.into_iter()
				.map(|id| ChiefHistoryEntryDto {
					turn_id: None,
					weather: Vec::new(),
					receipt: Some(ChiefHistoryReceiptDto {
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
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		visual.update(|window, _| window.resize(gpui::size(gpui::px(1000.), gpui::px(700.))));
		let work = surface.update(visual, |surface, cx| {
			surface.visual_workspace_fixture(cx);
			surface.graph_visible = false;
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
			assert!(surface.native_history.replace(
				Binding { work: id.clone(), thread: "thread".into(), account: "account".into() },
				ChiefTimelinePage {
					thread_id: "thread".into(),
					entries: vec![],
					next_cursor: None,
					active_realtime_session_at_page_start: None
				}
			));
			surface.native_history.input_receipts.result = Some(page(&id, Some(1), Some(1)));
			cx.notify();
			id
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("unconfirmed-native-input").is_some());
		let more = visual.debug_bounds("input-receipts-next").unwrap();
		visual.simulate_click(more.center(), Default::default());
		surface.update(visual, |surface, cx| {
			assert_eq!(surface.native_history.input_receipts.after, Some(1));
			assert!(surface.native_history.older_cursor.is_none());
			surface.native_history.input_receipts.result = Some(page(&work, Some(2), None));
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("input-receipts-next").is_none());
		let first = visual.debug_bounds("input-receipts-first").unwrap();
		visual.simulate_click(first.center(), Default::default());
		surface.update(visual, |surface, cx| {
			assert_eq!(surface.native_history.input_receipts.after, None);
			surface.native_history.input_receipts.result = Some(page(&work, None, None));
			assert!(surface.native_input_receipts_loaded(&work));
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("unconfirmed-native-input").is_none());
	}
}
