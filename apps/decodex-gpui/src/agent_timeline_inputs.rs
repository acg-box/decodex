//! Current unconfirmed input pages are independent of both transcript cursors.
use gpui::AnyElement;
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::native_timeline::Binding;
use crate::{
	shell::{
		agent_surface,
		agent_surface::native_timeline::{
			AgentClient, AgentSurface, AgentWorkItemDto, Context, EntityId, InteractiveElement,
			IntoElement, ParentElement, Task, markdown,
		},
	},
	ui_loading,
};
use decodex_protocol::AgentInputReceiptsResult;

#[derive(Default)]
pub(super) struct InputReceipts {
	task: Option<Task<()>>,
	after: Option<i64>,
	result: Option<AgentInputReceiptsResult>,
}

impl AgentSurface {
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
							.child(agent_surface::muted("Local input · Delivery not confirmed"))
							.child(markdown::render(
								&entry.text,
								&format!("input-receipt-{}", entry.id),
							)))
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
