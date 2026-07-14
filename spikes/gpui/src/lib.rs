pub mod history;
pub mod text_input;

use std::{cell::RefCell, rc::Rc};

use gpui::{
	Context, FocusHandle, Focusable, KeyBinding, Render, Role, ScrollStrategy, SharedString,
	UniformListScrollHandle, Window, div, prelude::*, px, rgb, text, uniform_list,
};

use history::{HistorySpec, PagedHistory};
use text_input::TextInput;

pub const GPUI_REVISION: &str = "aeeacf5439b2d30d01e38d65d767e6f31b255ecc";

gpui::actions!(workspace_spike, [FocusNext, FocusPrevious]);

pub fn bind_keys(cx: &mut gpui::App) {
	text_input::bind_keys(cx);
	cx.bind_keys([
		KeyBinding::new("tab", FocusNext, None),
		KeyBinding::new("tab", FocusNext, Some("TextInput")),
		KeyBinding::new("shift-tab", FocusPrevious, None),
		KeyBinding::new("shift-tab", FocusPrevious, Some("TextInput")),
	]);
}

pub struct WorkspaceSpike {
	history: Rc<RefCell<PagedHistory>>,
	input: gpui::Entity<TextInput>,
	root_focus: FocusHandle,
	clear_focus: FocusHandle,
	history_scroll: UniformListScrollHandle,
	async_event_count: usize,
}

impl WorkspaceSpike {
	pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
		let input = cx.new(|cx| TextInput::new(window, cx));
		let input_focus = input.read(cx).focus_handle(cx);
		window.focus(&input_focus, cx);
		Self {
			history: Rc::new(RefCell::new(PagedHistory::new(HistorySpec::large_fixture()))),
			input,
			root_focus: cx.focus_handle(),
			clear_focus: cx.focus_handle().tab_index(1).tab_stop(true),
			history_scroll: UniformListScrollHandle::new(),
			async_event_count: 0,
		}
	}

	pub fn history(&self) -> Rc<RefCell<PagedHistory>> {
		self.history.clone()
	}

	pub fn schedule_async_probe(&self, cx: &mut Context<Self>) -> gpui::Task<()> {
		cx.spawn(async move |this, cx| {
			this.update(cx, |workspace, cx| {
				workspace.async_event_count += 1;
				cx.notify();
			})
			.expect("workspace remains alive for async probe");
		})
	}

	pub const fn async_event_count(&self) -> usize {
		self.async_event_count
	}

	pub fn scroll_to_history_index(&self, index: usize) {
		self.history_scroll.scroll_to_item_strict(index, ScrollStrategy::Top);
	}
}

impl Render for WorkspaceSpike {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let history = self.history.clone();
		let history_scroll = self.history_scroll.clone();
		let item_count = history.borrow().spec().message_count();

		div()
			.id("workspace")
			.role(Role::Application)
			.aria_label("Decodex GPUI feasibility workspace")
			.track_focus(&self.root_focus)
			.on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
			.on_action(|_: &FocusPrevious, window, cx| window.focus_prev(cx))
			.size_full()
			.flex()
			.flex_col()
			.bg(rgb(0x111827))
			.text_color(rgb(0xf3f4f6))
			.child(
				div()
					.id("workspace-heading")
					.role(Role::Heading)
					.aria_level(1)
					.aria_label("Decodex workspace feasibility spike")
					.h(px(52.0))
					.px_4()
					.flex()
					.items_center()
					.justify_between()
					.border_b_1()
					.border_color(rgb(0x374151))
					.child(text!("Decodex · GPUI feasibility"))
					.child(format!("GPUI {}", &GPUI_REVISION[..12])),
			)
			.child(
				div()
					.flex_1()
					.min_h_0()
					.flex()
					.child(
						div()
							.w(px(330.0))
							.min_w(px(330.0))
							.border_r_1()
							.border_color(rgb(0x374151))
							.flex()
							.flex_col()
							.child(
								div()
									.id("conversation-list-label")
									.role(Role::Heading)
									.aria_level(2)
									.aria_label("Virtualized conversation history")
									.p_3()
									.child(format!("History · {} logical messages", item_count)),
							)
							.child(
								uniform_list(
									"conversation-history",
									item_count,
									cx.processor(
										move |_this,
										      range: std::ops::Range<usize>,
										      _window,
										      _cx| {
											let mut history = history.borrow_mut();
											range
												.map(|index| {
													let preview = history.message_preview(index);
													div()
														.id(("history-row", index))
														.role(Role::ListItem)
														.aria_label(SharedString::from(format!(
															"Message {}",
															index + 1
														)))
														.h(px(44.0))
														.px_3()
														.flex()
														.items_center()
														.border_b_1()
														.border_color(rgb(0x1f2937))
														.child(preview)
												})
												.collect::<Vec<_>>()
										},
									),
								)
								.track_scroll(&history_scroll)
								.flex_1()
								.min_h_0(),
							),
					)
					.child(
						div().flex_1().min_w_0().flex().flex_col().child(graph_canvas()).child(
							div()
								.id("message-composer")
								.role(Role::Group)
								.aria_label("Message composer")
								.h(px(74.0))
								.p_3()
								.border_t_1()
								.border_color(rgb(0x374151))
								.flex()
								.tab_group()
								.gap_2()
								.child(self.input.clone())
								.child(
									div()
										.id("clear-composer")
										.role(Role::Button)
										.aria_label("Clear composer")
										.track_focus(&self.clear_focus)
										.px_3()
										.rounded_md()
										.bg(rgb(0x374151))
										.on_click({
											let input = self.input.clone();
											move |_, _, cx| {
												input.update(cx, |input, cx| input.clear(cx));
											}
										})
										.child("Clear"),
								),
						),
					),
			)
	}
}

fn graph_canvas() -> impl IntoElement {
	let mut canvas = div()
		.id("agent-graph")
		.role(Role::Image)
		.aria_label("Bounded agent relationship graph with 120 visible nodes")
		.flex_1()
		.min_h_0()
		.relative()
		.overflow_hidden()
		.bg(rgb(0x0b1220));

	for index in 0..120usize {
		let column = index % 12;
		let row = index / 12;
		canvas = canvas.child(
			div()
				.id(("graph-node", index))
				.role(Role::Image)
				.aria_label(SharedString::from(format!("Agent node {}", index + 1)))
				.absolute()
				.left(px(28.0 + column as f32 * 66.0))
				.top(px(24.0 + row as f32 * 48.0))
				.w(px(38.0))
				.h(px(24.0))
				.rounded_md()
				.bg(if index % 7 == 0 { rgb(0x22c55e) } else { rgb(0x3b82f6) }),
		);
	}
	canvas
}

#[cfg(test)]
mod tests {
	use std::time::Instant;

	use gpui::{TestAppContext, VisualTestContext};

	use super::*;

	fn open_workspace(
		cx: &mut TestAppContext,
	) -> (gpui::Entity<WorkspaceSpike>, &mut VisualTestContext) {
		cx.update(bind_keys);
		cx.add_window_view(WorkspaceSpike::new)
	}

	#[gpui::test]
	async fn workspace_virtualization_and_async_event_loop(cx: &mut TestAppContext) {
		let (workspace, visual) = open_workspace(cx);
		let stats = workspace.read_with(visual, |workspace, _| workspace.history.borrow().stats());
		assert_eq!(stats.cached_pages, 1);
		assert_eq!(stats.generated_messages, 64);
		workspace.update(visual, |workspace, cx| {
			workspace.scroll_to_history_index(80_000);
			cx.notify();
		});
		let scrolled_stats =
			workspace.read_with(visual, |workspace, _| workspace.history.borrow().stats());
		assert_eq!(scrolled_stats.cached_pages, 2);
		assert_eq!(scrolled_stats.generated_messages, 128);

		workspace.update(visual, |workspace, cx| workspace.schedule_async_probe(cx)).await;
		assert_eq!(workspace.read_with(visual, |workspace, _| workspace.async_event_count()), 1);
	}

	#[gpui::test]
	fn workspace_keyboard_focus_order_is_deterministic(cx: &mut TestAppContext) {
		let (workspace, visual) = open_workspace(cx);
		let input_focus =
			workspace.read_with(visual, |workspace, cx| workspace.input.read(cx).focus_handle(cx));
		let clear_focus = workspace.read_with(visual, |workspace, _| workspace.clear_focus.clone());
		assert!(visual.update(|window, _| input_focus.is_focused(window)));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		visual.simulate_keystrokes("tab");
		assert!(visual.update(|window, _| clear_focus.is_focused(window)));

		visual.simulate_keystrokes("shift-tab");
		assert!(visual.update(|window, _| input_focus.is_focused(window)));
	}

	#[gpui::test]
	#[ignore = "measurement command; run with --ignored --nocapture"]
	fn workspace_headless_frame_benchmark(cx: &mut TestAppContext) {
		let (workspace, visual) = open_workspace(cx);
		let frames = 240usize;
		let mut micros = Vec::with_capacity(frames);
		for _ in 0..frames {
			let started = Instant::now();
			workspace.update(visual, |_, cx| cx.notify());
			micros.push(started.elapsed().as_micros());
		}
		micros.sort_unstable();
		let p50 = micros[frames / 2];
		let p95 = micros[frames * 95 / 100];
		let max = micros[frames - 1];
		let stats = workspace.read_with(visual, |workspace, _| workspace.history.borrow().stats());
		println!(
			"{{\"frames\":{frames},\"p50_micros\":{p50},\"p95_micros\":{p95},\"max_micros\":{max},\"history_cached_bytes\":{},\"history_generated_messages\":{},\"graph_visible_nodes\":120}}",
			stats.cached_bytes, stats.generated_messages
		);
	}
}
