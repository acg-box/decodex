//! Explicit native memory inspection; it never resumes a conversation or edits memory.
use crate::shell::agent_surface::{
	AgentClient, AgentSnapshotDto, AgentSurface, Context, EntityId, IntoElement, ParentElement,
	Styled, Task, WireText, mcp_forms,
};
use decodex_protocol::{AgentMemoryResult, MAX_MEMORY_BYTES, MEMORY_PAGE_BYTES};
use gpui::{AnyElement, ClipboardItem, InteractiveElement as _, StatefulInteractiveElement as _};

#[derive(Default)]
pub(super) struct Panel {
	owner: Option<(String, String)>,
	text: Option<String>,
	feedback: String,
	task: Option<Task<()>>,
	epoch: u64,
}

impl AgentSurface {
	pub(super) fn reset_memory_summary(&mut self) {
		self.memory_summary =
			Panel { epoch: self.memory_summary.epoch.wrapping_add(1), ..Default::default() };
	}

	pub(super) fn invalidate_memory_summary(&mut self, next: &AgentSnapshotDto) {
		let Some((work, thread)) = &self.memory_summary.owner else { return };
		if self.snapshot.as_ref().is_none_or(|s| s.runtime_source != next.runtime_source)
			|| !next
				.work_items
				.iter()
				.any(|w| &w.id == work && w.codex_thread_id.as_ref() == Some(thread))
		{
			self.reset_memory_summary();
		}
	}

	fn load_memory_summary(&mut self, work: &str, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work)
			|| self.native_agents.selected.is_some()
			|| (self.memory_summary.task.is_some()
				&& self.memory_summary.owner.as_ref().is_some_and(|(id, _)| id == work))
		{
			return;
		}
		let Some(profile) = self.profile.clone() else { return };
		let Some(thread) = self
			.snapshot
			.as_ref()
			.and_then(|s| s.work_items.iter().find(|w| w.id == work))
			.and_then(|w| w.codex_thread_id.clone())
		else {
			return;
		};
		self.reset_memory_summary();
		self.memory_summary.owner = Some((work.into(), thread.clone()));
		self.memory_summary.feedback = "Reading native memory…".into();
		let epoch = self.memory_summary.epoch;
		let generation = self.generation;
		let owner = work.to_owned();
		let captured = (owner.clone(), thread.clone());
		let future = cx.background_executor().spawn(async move {
			let runtime = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "Memory read is unavailable.")?;
			runtime.block_on(read(&AgentClient::new(profile), &owner, &thread))
		});
		self.memory_summary.task = Some(cx.spawn(async move |surface, cx| {
			let result = future.await;
			let _ = surface.update(cx, |s, cx| {
				if s.generation != generation
					|| s.memory_summary.epoch != epoch
					|| s.memory_summary.owner.as_ref() != Some(&captured)
				{
					return;
				}
				s.memory_summary.task = None;
				match result {
					Ok(Some(text)) => {
						s.memory_summary.text = Some(text);
						s.memory_summary.feedback = "Native memory summary · Read-only".into();
					},
					Ok(None) => s.memory_summary.feedback =
						"No summary is available. Native memory may be disabled, missing or empty."
							.into(),
					Err(error) => s.memory_summary.feedback = error.into(),
				}
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn memory_summary_panel(&self, work: &str, cx: &mut Context<Self>) -> AnyElement {
		if self.native_agents.selected.is_some() {
			return gpui::div().into_any_element();
		}
		let owner = work.to_owned();
		let opened = self.memory_summary.owner.as_ref().is_some_and(|(id, _)| id == work);
		let mut panel = gpui::div().flex().flex_col().gap_2().child(mcp_forms::mcp_button(
			"memory-summary-toggle".into(),
			"Memory summary".into(),
			opened,
			cx,
			move |s, cx| {
				if s.memory_summary.owner.as_ref().is_some_and(|(id, _)| id == &owner) {
					s.reset_memory_summary();
					cx.notify();
				} else {
					s.load_memory_summary(&owner, cx);
				}
			},
		));
		if !opened {
			return panel.into_any_element();
		}
		panel = panel.child(self.memory_summary.feedback.clone());
		if let Some(text) = &self.memory_summary.text {
			let content = text.clone();
			panel = panel
				.child(
					gpui::div()
						.id("memory-summary-text")
						.max_h(gpui::px(300.))
						.overflow_y_scroll()
						.child(text.clone()),
				)
				.child(mcp_forms::mcp_button(
					"memory-summary-copy".into(),
					"Copy summary".into(),
					false,
					cx,
					move |_, cx| cx.write_to_clipboard(ClipboardItem::new_string(content.clone())),
				));
		}
		if self.memory_summary.task.is_none() {
			let owner = work.to_owned();
			panel = panel.child(mcp_forms::mcp_button(
				"memory-summary-refresh".into(),
				"Refresh summary".into(),
				false,
				cx,
				move |s, cx| s.load_memory_summary(&owner, cx),
			));
		}
		panel.into_any_element()
	}
}

async fn read(
	client: &AgentClient,
	work: &str,
	thread: &str,
) -> Result<Option<String>, &'static str> {
	let work = EntityId::new(work).map_err(|_| "Memory source is unavailable.")?;
	let thread = WireText::new(thread).map_err(|_| "Memory source is unavailable.")?;
	let mut complete = String::new();
	let mut digest: Option<WireText> = None;
	let mut total = None;
	loop {
		let page = client
			.memory_summary(work.clone(), thread.clone(), digest.clone(), complete.len())
			.await
			.map_err(|_| "Memory read failed. Refresh to try again.")?;
		match page {
			AgentMemoryResult::Empty { .. } if complete.is_empty() => return Ok(None),
			AgentMemoryResult::Page {
				digest: actual,
				offset,
				total_bytes,
				text,
				next_offset,
				..
			} => {
				if offset != complete.len()
					|| text.is_empty()
					|| text.len() > MEMORY_PAGE_BYTES
					|| total_bytes > MAX_MEMORY_BYTES
					|| total.is_some_and(|n| n != total_bytes)
					|| digest.as_ref().is_some_and(|d| d.as_str() != actual)
				{
					return Err("Memory changed during reading. Refresh the summary.");
				}
				digest =
					Some(WireText::new(actual).map_err(|_| "Memory identity is unavailable.")?);
				total = Some(total_bytes);
				complete.push_str(&text);
				if complete.len() > total_bytes {
					return Err("Memory page exceeds its declared length.");
				}
				if let Some(next) = next_offset {
					if next != complete.len() || next <= offset {
						return Err("Memory pages did not advance.");
					}
				} else {
					return if complete.len() == total_bytes {
						Ok(Some(complete))
					} else {
						Err("Memory read is incomplete.")
					};
				}
			},
			_ =>
				return Err("Memory is unavailable. Open the conversation normally, then refresh."),
		}
	}
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::AgentSurface;
	use gpui::{AppContext as _, Context, Entity, IntoElement, Render, TestAppContext, Window};

	struct MemoryView {
		surface: Entity<AgentSurface>,
	}
	impl Render for MemoryView {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			self.surface.update(cx, |s, cx| s.memory_summary_panel("agent", cx))
		}
	}

	#[gpui::test]
	fn memory_view_copies_original_text_preserves_the_draft_and_discards_a_rebound_source(
		cx: &mut TestAppContext,
	) {
		const TEXT: &str = "v1\n<instruction>This is memory text, not an action.</instruction> 界";
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			cx.observe(&surface, |_, _, cx| cx.notify()).detach();
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.visual_workbench_page("workbench", cx);
				s.selected = Some("agent".into());
				let thread = s
					.snapshot
					.as_ref()
					.unwrap()
					.work_items
					.iter()
					.find(|w| w.id == "agent")
					.unwrap()
					.codex_thread_id
					.clone()
					.unwrap();
				s.memory_summary.owner = Some(("agent".into(), thread));
				s.memory_summary.text = Some(TEXT.into());
				s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));
			});
			MemoryView { surface }
		});
		visual.simulate_resize(gpui::size(gpui::px(600.), gpui::px(500.)));
		visual.update(crate::ui_frame::draw_frame);
		let copy = visual.debug_bounds("memory-summary-copy").expect("explicit copy action");
		visual.simulate_click(copy.center(), Default::default());
		visual.update(|_, cx| {
			assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some(TEXT))
		});
		let surface = view.read_with(visual, |v, _| v.surface.clone());
		surface.update(visual, |s, cx| {
			assert_eq!(s.selected.as_deref(), Some("agent"));
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
			let mut next = s.snapshot.clone().unwrap();
			next.work_items.iter_mut().find(|w| w.id == "agent").unwrap().codex_thread_id =
				Some("replacement-thread".into());
			s.invalidate_memory_summary(&next);
			assert!(s.memory_summary.text.is_none());
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("memory-summary-copy").is_none());
	}
}
