//! Preview density and uninterrupted pointer navigation through long and short exchanges.
use std::{thread, time::Duration};

use gpui::{
	Context, IntoElement, Render, TestAppContext, Window,
	prelude::{ParentElement as _, Styled as _},
};

use super::{AgentHistoryResult, AgentSurface, HistoryMark, history_preview_content};

struct ContentFixture(HistoryMark);
impl Render for ContentFixture {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.size_full()
			.flex()
			.items_start()
			.child(history_preview_content(&self.0).w(gpui::px(320.)))
	}
}

struct FontFixture {
	mark: HistoryMark,
	family: &'static str,
}
impl Render for FontFixture {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		gpui::div()
			.font_family(self.family)
			.child(history_preview_content(&self.mark).w(gpui::px(320.)))
	}
}

#[gpui::test]
fn step_states_keep_their_slot_and_metadata_aligned_across_fonts(cx: &mut TestAppContext) {
	let (fixture, visual) = cx.add_window_view(|_, _| FontFixture {
		family: crate::ui_theme::FONT_FAMILY,
		mark: HistoryMark {
			position: Default::default(),
			hit_bounds: Default::default(),
			question: "检查布局 Check the layout".into(),
			answer: "已验证 Verified".into(),
			details: super::HistoryDetails {
				started_at_micros: Some(crate::ui_time::now() - 3_600_000_000),
				duration_ms: Some(31_300),
				steps: Some(6),
				usage: Some(decodex_protocol::AgentTurnUsageDto {
					input_tokens: 236_500,
					output_tokens: 328,
					details: None,
				}),
				..Default::default()
			},
		},
	});
	for family in [crate::ui_theme::FONT_FAMILY, "PingFang SC", "Menlo"] {
		fixture.update(visual, |s, cx| {
			s.family = family;
			cx.notify();
		});
		let mut baseline = None;
		for (count, status, missing, long_values) in [
			(Some(6), None, false, false),
			(Some(0), None, false, false),
			(None, None, false, false),
			(None, Some("In progress"), false, false),
			(Some(6), Some("Interrupted"), false, false),
			(Some(99_999), None, false, true),
			(None, None, true, false),
		] {
			fixture.update(visual, |s, cx| {
				s.mark.details.steps = count;
				s.mark.details.status = status;
				s.mark.details.started_at_micros =
					(!missing).then(|| crate::ui_time::now() - 3_600_000_000);
				s.mark.details.duration_ms = (!missing).then_some(if long_values {
					99 * 86_400_000 + 23 * 3_600_000
				} else {
					31_300
				});
				s.mark.details.usage = (!missing).then_some(decodex_protocol::AgentTurnUsageDto {
					input_tokens: if long_values { 999_900_000_000 } else { 236_500 },
					output_tokens: if long_values { 999_949_999 } else { 328 },
					details: None,
				});
				cx.notify();
			});
			visual.update(crate::ui_frame::draw_current_frame);
			let positions = [
				"history-preview-time",
				"history-preview-duration",
				"history-preview-steps",
				"history-preview-input",
				"history-preview-output",
			]
			.map(|id| visual.debug_bounds(id).expect("all five metric slots"));
			if let Some(expected) = baseline {
				assert_eq!(positions, expected, "{family}: {count:?} {status:?}");
			} else {
				baseline = Some(positions);
			}
			for pair in positions.windows(2) {
				assert!(pair[0].right() <= pair[1].left());
				assert_eq!(pair[0].top(), pair[1].top());
				assert_eq!(pair[0].size.width, pair[1].size.width);
			}
			for (index, name) in [
				"history-preview-time-value",
				"history-preview-duration-value",
				"history-preview-steps-value",
				"history-preview-input-value",
				"history-preview-output-value",
			]
			.into_iter()
			.enumerate()
			{
				let content = visual.debug_bounds(name).unwrap();
				assert!(content.left() >= positions[index].left(), "{family}: {name}");
				assert!(content.right() <= positions[index].right(), "{family}: {name}");
				assert!((f32::from(content.center().x - positions[index].center().x)).abs() < 1.);
			}
			let card = visual.debug_bounds("history-hover-preview").unwrap();
			assert!(positions[4].right() <= card.right() - gpui::px(10.));
			assert_eq!(positions[2].size.height, gpui::px(crate::ui_theme::CAPTION_LINE_HEIGHT));
		}
	}
}

#[gpui::test]
fn a_complete_plain_reply_is_zero_but_an_incomplete_reply_is_unknown(cx: &mut TestAppContext) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	surface.update(visual, |s, cx| {
		s.visual_process_page(cx);
		let input = s.timeline.native.entries[0].clone();
		let final_answer = s.timeline.native.entries[10].clone();
		let terminal = s.timeline.native.entries[11].clone();
		s.timeline.native.entries = vec![input, final_answer];
		s.timeline.native.revision += 1;
		s.prepare_history_marks();
		assert_eq!(
			s.timeline.marks.values().next().unwrap().details.step_count(),
			super::StepCount::Unknown
		);
		s.timeline.native.entries.push(terminal);
		s.timeline.native.revision += 1;
		s.prepare_history_marks();
		assert_eq!(
			s.timeline.marks.values().next().unwrap().details.step_count(),
			super::StepCount::Known(0)
		);
	});
}

#[gpui::test]
fn long_previews_clamp_by_lines_and_short_content_releases_space(cx: &mut TestAppContext) {
	let (fixture, visual) = cx.add_window_view(|_, _| {
		ContentFixture(HistoryMark {
			position: Default::default(),
			hit_bounds: Default::default(),
			question: String::new(),
			details: Default::default(),
			answer: String::new(),
		})
	});
	for text in [
		"Check the command results and preserve the original working directory. ".repeat(30),
		"检查步骤布局和费用来源，保留原始记录，不要把工具输出当成最终结论。".repeat(30),
	] {
		fixture.update(visual, |fixture, cx| {
			fixture.0.question.clone_from(&text);
			fixture.0.answer = text;
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let question = visual.debug_bounds("history-preview-question").expect("question");
		let answer = visual.debug_bounds("history-preview-answer").expect("answer");
		let preview = visual.debug_bounds("history-hover-preview").expect("preview");
		assert_eq!(question.size.height, gpui::px(36.));
		assert_eq!(answer.size.height, gpui::px(54.));
		assert_eq!(preview.size.height, gpui::px(142.));
		let metrics = visual.debug_bounds("history-preview-metadata").unwrap();
		assert_eq!(metrics.top() - answer.bottom(), gpui::px(8.));
		assert_eq!(question.left(), answer.left());
		assert_eq!(question.right(), answer.right());
		assert_eq!(answer.top() - question.bottom(), gpui::px(10.));
		let divider = visual.debug_bounds("history-preview-divider").expect("speaker divider");
		assert!(divider.top() > question.bottom());
		assert!(divider.bottom() < answer.top());
		assert_eq!(divider.size.height, gpui::px(0.5));
		fixture.update(visual, |fixture, cx| {
			fixture.0.question = "Check the result".into();
			fixture.0.answer = "Verified.".into();
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		let preview = visual.debug_bounds("history-hover-preview").expect("short preview");
		assert_eq!(preview.size.height, gpui::px(88.));
		let answer = visual.debug_bounds("history-preview-answer").unwrap();
		let metrics = visual.debug_bounds("history-preview-metadata").unwrap();
		assert_eq!(metrics.top() - answer.bottom(), gpui::px(8.));
		fixture.update(visual, |fixture, cx| {
			fixture.0.answer.clear();
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("history-preview-answer").is_none());
		assert!(visual.debug_bounds("history-preview-divider").is_none());
		assert_eq!(
			visual.debug_bounds("history-hover-preview").expect("question only").size.height,
			gpui::px(60.)
		);
	}
}

#[gpui::test]
fn preview_metadata_keeps_the_step_slot_when_other_values_are_missing(cx: &mut TestAppContext) {
	let (fixture, visual) = cx.add_window_view(|_, _| {
		ContentFixture(HistoryMark {
			position: Default::default(),
			hit_bounds: Default::default(),
			question: "Check the release".into(),
			answer: "Verified.".into(),
			details: Default::default(),
		})
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("history-preview-metadata").is_some());
	assert!(visual.debug_bounds("history-preview-steps").is_some());
	fixture.update(visual, |s, cx| {
		s.0.details = super::HistoryDetails {
			started_at_micros: Some(crate::ui_time::now() - 3_600_000_000),
			duration_ms: Some(7_323_000),
			steps: Some(12),
			status: None,
			..Default::default()
		};
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let meta = visual.debug_bounds("history-preview-metadata").expect("time and turn duration");
	let card = visual.debug_bounds("history-hover-preview").unwrap();
	let answer = visual.debug_bounds("history-preview-answer").unwrap();
	assert!(meta.top() > answer.bottom());
	assert!(meta.right() <= card.right());
	assert!(meta.bottom() <= card.bottom());
	assert_eq!(answer.size.height, gpui::px(18.));
	assert!(visual.debug_bounds("history-preview-steps").is_some());
	assert!(visual.debug_bounds("history-preview-input").is_some());
	assert!(visual.debug_bounds("history-preview-output").is_some());
	fixture.update(visual, |s, cx| {
		s.0.details.usage = Some(decodex_protocol::AgentTurnUsageDto {
			input_tokens: 236_500,
			output_tokens: 328,
			details: Some(decodex_protocol::AgentUsageDetailsDto {
				responses: Some(4),
				..Default::default()
			}),
		});
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let age = visual.debug_bounds("history-preview-time").unwrap();
	let duration = visual.debug_bounds("history-preview-duration").unwrap();
	let steps = visual.debug_bounds("history-preview-steps").unwrap();
	let input = visual.debug_bounds("history-preview-input").expect("turn input tokens");
	let output = visual.debug_bounds("history-preview-output").expect("turn output tokens");
	let meta = visual.debug_bounds("history-preview-metadata").unwrap();
	let card = visual.debug_bounds("history-hover-preview").unwrap();
	assert!(visual.debug_bounds("history-preview-calls").is_none());
	assert_eq!(input.top(), duration.top());
	assert_eq!(input.top(), steps.top());
	assert_eq!(input.top(), output.top());
	assert_eq!(meta.size.height, gpui::px(crate::ui_theme::CAPTION_LINE_HEIGHT));
	let gaps = [
		duration.left() - age.right(),
		steps.left() - duration.right(),
		input.left() - steps.right(),
		output.left() - input.right(),
	];
	for gap in gaps {
		assert!(gap.abs() <= gpui::px(1.));
		assert!((gap - gaps[0]).abs() <= gpui::px(1.), "metric gaps: {gaps:?}");
	}
	assert_eq!(output.right(), card.right() - gpui::px(10.));
	assert!(output.bottom() <= card.bottom());
	fixture.update(visual, |s, cx| {
		s.0.details.status = Some("Interrupted");
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("history-preview-steps").is_some());
	assert!(visual.debug_bounds("history-preview-calls").is_none());
	assert!(visual.debug_bounds("history-preview-status").is_none());
	let status = visual.debug_bounds("history-preview-steps").unwrap();
	let output = visual.debug_bounds("history-preview-output").unwrap();
	let meta = visual.debug_bounds("history-preview-metadata").unwrap();
	assert_eq!(status.top(), output.top());
	assert_eq!(meta.size.height, gpui::px(crate::ui_theme::CAPTION_LINE_HEIGHT));
	assert!(output.right() <= card.right() - gpui::px(10.));
	fixture.update(visual, |s, cx| {
		s.0.details.steps = Some(0);
		s.0.details.status = None;
		s.0.details.usage.as_mut().unwrap().details.as_mut().unwrap().responses = None;
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("history-preview-steps").is_some());
	assert!(visual.debug_bounds("history-preview-calls").is_none());
}

#[gpui::test]
fn preview_uses_the_existing_process_count_and_plain_replies_have_zero_steps(
	cx: &mut TestAppContext,
) {
	use decodex_protocol::AgentTimelineContent;
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	surface.update(visual, |s, cx| {
		s.visual_process_page(cx);
		let usage = decodex_protocol::AgentTurnUsageDto {
			input_tokens: 236_500,
			output_tokens: 328,
			details: None,
		};
		if let AgentTimelineContent::TurnBoundary { usage: target, .. } =
			&mut s.timeline.native.entries[11].content
		{
			*target = Some(usage.clone());
		}
		s.prepare_history_marks();
		assert_eq!(s.timeline.marks.len(), 1);
		assert_eq!(s.timeline.marks.values().next().unwrap().details.steps, Some(9));
		assert_eq!(s.timeline.marks.values().next().unwrap().details.usage, Some(usage));
		let mut input = s.timeline.native.entries[0].clone();
		input.position = 12;
		let mut reply = s.timeline.native.entries[10].clone();
		reply.position = 13;
		let mut terminal = s.timeline.native.entries[11].clone();
		terminal.position = 14;
		if let AgentTimelineContent::TurnBoundary { usage, .. } = &mut terminal.content {
			*usage = None;
		}
		for entry in [&mut input, &mut reply, &mut terminal] {
			match &mut entry.content {
				AgentTimelineContent::Item { turn_id, item_id, .. } => {
					*turn_id = "plain-turn".into();
					*item_id = format!("plain-{item_id}");
				},
				AgentTimelineContent::TurnBoundary { turn_id, .. } =>
					*turn_id = "plain-turn".into(),
				_ => unreachable!("fixture contains items and terminal boundary"),
			}
		}
		s.timeline.native.entries.extend([input, reply, terminal]);
		s.timeline.native.revision += 1;
		s.prepare_history_marks();
		assert_eq!(s.timeline.marks.len(), 2);
		assert_eq!(s.timeline.marks.values().last().unwrap().details.steps, Some(0));
		assert_eq!(s.timeline.marks.values().last().unwrap().details.usage, None);
	});
}

#[test]
fn turn_details_use_exact_boundaries_and_normal_completion_adds_no_status() {
	use decodex_protocol::{AgentTimelineContent, AgentTimelineEntry};
	let boundary = |turn: &str, completed, started_at_micros, duration_ms, status: Option<&str>| {
		AgentTimelineEntry {
			position: 0,
			content: AgentTimelineContent::TurnBoundary {
				turn_id: turn.into(),
				completed,
				started_at_micros,
				duration_ms,
				status: status.map(String::from),
				usage_summary: None,
				usage: None,
				timing: None,
				error: None,
			},
		}
	};
	let details = super::history_turn_details(&[
		boundary("a", false, Some(1_800_000_000_000_000), None, None),
		boundary("a", true, None, Some(1250), Some("completed")),
		boundary("b", true, None, None, Some("interrupted")),
	]);
	assert_eq!(details["a"].started_at_micros, Some(1_800_000_000_000_000));
	assert_eq!(details["a"].duration_ms, Some(1250));
	assert_eq!(details["a"].status, None);
	assert_eq!(details["b"].started_at_micros, None);
	assert_eq!(details["b"].duration_ms, None);
	assert_eq!(details["b"].status, Some("Interrupted"));
}

#[gpui::test]
fn worker_wake_process_keeps_its_prompt_count_and_latest_reply(cx: &mut TestAppContext) {
	use decodex_protocol::AgentTimelineContent;
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	surface.update(visual, |s, cx| {
		s.visual_process_page(cx);
		let mut reasoning = s.timeline.native.entries[1].clone();
		reasoning.position = 12;
		let mut reply = s.timeline.native.entries[10].clone();
		reply.position = 13;
		if let AgentTimelineContent::Item { text, .. } = &mut reply.content {
			*text = "Latest worker result".into();
		}
		let mut terminal = s.timeline.native.entries[11].clone();
		terminal.position = 14;
		for entry in [&mut reasoning, &mut reply, &mut terminal] {
			match &mut entry.content {
				AgentTimelineContent::Item { turn_id, item_id, .. } => {
					*turn_id = "worker-wake".into();
					*item_id = format!("wake-{item_id}");
				},
				AgentTimelineContent::TurnBoundary { turn_id, .. } =>
					*turn_id = "worker-wake".into(),
				_ => unreachable!(),
			}
		}
		s.timeline.native.entries.push(reasoning);
		let work = &mut s.snapshot.as_mut().unwrap().work_items[0];
		work.active_turn_id = Some("worker-wake".into());
		work.dispatch_state = decodex_protocol::AgentDispatchStateDto::Running;
		s.timeline.native.revision += 1;
		s.prepare_history_marks();
		let key = s.timeline.marks.keys().next().unwrap();
		assert_eq!(
			s.history_preview_mark(key).unwrap().details.step_count(),
			super::StepCount::InProgress
		);
		let work = &mut s.snapshot.as_mut().unwrap().work_items[0];
		work.active_turn_id = None;
		work.dispatch_state = decodex_protocol::AgentDispatchStateDto::Idle;
		s.timeline.native.entries.extend([reply, terminal]);
		s.timeline.native.revision += 1;
		s.prepare_history_marks();
		assert_eq!(s.timeline.marks.len(), 1);
		let mark = s.timeline.marks.values().next().unwrap();
		// Nine original process items, its superseded answer, and wake reasoning.
		// The latest displayed final answer remains outside the process.
		assert_eq!(mark.details.steps, Some(11));
		assert_eq!(mark.answer, "Latest worker result");
		let mut later_input = s.timeline.native.entries[0].clone();
		later_input.position = 15;
		let mut later_reply = s.timeline.native.entries[10].clone();
		later_reply.position = 16;
		let mut later_terminal = s.timeline.native.entries[11].clone();
		later_terminal.position = 17;
		for entry in [&mut later_input, &mut later_reply, &mut later_terminal] {
			match &mut entry.content {
				AgentTimelineContent::Item { turn_id, item_id, .. } => {
					*turn_id = "later-question".into();
					*item_id = format!("later-{item_id}");
				},
				AgentTimelineContent::TurnBoundary { turn_id, .. } =>
					*turn_id = "later-question".into(),
				_ => unreachable!(),
			}
		}
		let mut late_activity = s.timeline.native.entries[1].clone();
		late_activity.position = 18;
		if let AgentTimelineContent::Item { item_id, .. } = &mut late_activity.content {
			*item_id = "late-original-activity".into();
		}
		s.timeline.native.entries.extend([later_input, later_reply, later_terminal, late_activity]);
		s.timeline.native.revision += 1;
		s.prepare_history_marks();
		let mut marks = s.timeline.marks.values();
		let first = marks.next().unwrap();
		assert_eq!(first.details.steps, Some(12));
		assert_eq!(first.answer, "Latest worker result");
		assert_eq!(
			marks.next().unwrap().details.steps,
			Some(0),
			"late activity keeps its original owner"
		);
		if let AgentTimelineContent::TurnBoundary { status, .. } =
			&mut s.timeline.native.entries[14].content
		{
			*status = Some("failed".into());
		}
		s.timeline.native.revision += 1;
		s.prepare_history_marks();
		let first = s.timeline.marks.values().next().unwrap();
		assert_eq!(first.details.status, Some("Failed"));
		assert_eq!(first.answer, "Latest worker result");
	});
}

#[gpui::test]
fn scrubbing_ticks_updates_one_preview_without_dwell_gaps_or_transcript_movement(
	cx: &mut TestAppContext,
) {
	let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
	visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(900.)));
	surface.update(visual, |s, cx| {
		s.visual_workspace_fixture(cx);
		s.workspace.graph_visible = false;
		let Some((_, AgentHistoryResult::Available { entries, .. })) = &mut s.history else {
			panic!("fixture history");
		};
		let source = entries[0].clone();
		*entries = (0..8)
			.map(|index| {
				let mut entry = source.clone();
				entry.id = index;
				entry.kind = if index % 2 == 0 { "user" } else { "assistant" }.into();
				entry.text = match index {
					0 | 1 => "检查步骤顺序、费用来源和工作目录，保留完整记录。".repeat(20),
					5 => String::new(),
					_ => "Short message".into(),
				};
				entry.created_at_micros = if index < 4 { crate::ui_time::now() } else { 0 };
				entry.duration_ms = (index == 1).then_some(123 * 3_600_000);
				entry.usage =
					(index == 1 || index == 7).then_some(decodex_protocol::AgentTurnUsageDto {
						input_tokens: 999_900_000_000,
						output_tokens: 999_949_999,
						details: None,
					});
				entry
			})
			.collect();
		cx.notify();
	});
	visual.update(crate::ui_frame::draw_current_frame);
	let points = surface.read_with(visual, |s, _| {
		s.timeline
			.marks
			.values()
			.map(|mark| mark.hit_bounds.get().expect("tick").center())
			.collect::<Vec<_>>()
	});
	assert_eq!(points.len(), 4);
	let transcript = visual.debug_bounds("workspace-transcript").expect("transcript");
	let offset = surface.read_with(visual, |s, _| s.timeline.scroll["agent"].offset());
	visual.simulate_mouse_move(points[0], gpui::MouseButton::Left, Default::default());
	visual.run_until_parked();
	visual.executor().advance_clock(crate::ui_motion::HOVER_OPEN_DELAY);
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("history-hover-preview").is_some());
	thread::sleep(crate::ui_theme::POPOVER_ARRIVAL + Duration::from_millis(10));
	visual.update(crate::ui_frame::draw_current_frame);
	let center = visual.debug_bounds("popover-surface").expect("preview surface").center();
	let columns = [
		"history-preview-time",
		"history-preview-duration",
		"history-preview-steps",
		"history-preview-input",
		"history-preview-output",
	];
	let initial_columns = columns.map(|id| visual.debug_bounds(id).unwrap());
	for index in [1, 2, 3, 0, 3, 2, 1, 0].into_iter().cycle().take(32) {
		visual.simulate_mouse_move(points[index], gpui::MouseButton::Left, Default::default());
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);
		let preview = visual.debug_bounds("history-hover-preview").expect("no gap while scrubbing");
		let panel_bounds = visual.debug_bounds("popover-surface").expect("same preview surface");
		assert!((panel_bounds.center().y - center.y).abs() <= gpui::px(0.5));
		assert_eq!(panel_bounds.center().x, center.x);
		assert_eq!(
			visual.debug_bounds("history-preview-question").expect("updated question").size.height,
			gpui::px(if index == 0 { 36. } else { 18. })
		);
		for (id, expected) in columns.into_iter().zip(initial_columns) {
			let column = visual.debug_bounds(id).expect("missing metrics retain their column");
			assert_eq!(column.origin.x, expected.origin.x);
			assert_eq!(column.size, expected.size);
			assert_eq!(column.bottom(), preview.bottom() - gpui::px(10.));
		}
		assert_eq!(visual.debug_bounds("history-preview-answer").is_some(), index != 2);
		assert_eq!(visual.debug_bounds("workspace-transcript"), Some(transcript));
		surface.read_with(visual, |s, _| assert_eq!(s.timeline.scroll["agent"].offset(), offset));
	}
	visual.simulate_mouse_move(
		gpui::point(gpui::px(1.), gpui::px(1.)),
		gpui::MouseButton::Left,
		Default::default(),
	);
	visual.run_until_parked();
	visual.executor().advance_clock(crate::ui_motion::HOVER_CLOSE_DELAY);
	visual.run_until_parked();
	visual.update(crate::ui_frame::draw_current_frame);
	assert!(visual.debug_bounds("history-hover-preview").is_none());
}
