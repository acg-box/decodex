//! Compare real transcript renderers without a service or model request.
use crate::shell::agent_surface::{
	IdempotencyKey, PendingCommand,
	native_timeline::{
		AgentHistoryResult, AgentSurface, AgentTimelineContent, AgentTimelineEntry,
		AgentTimelinePage, Binding, Context,
	},
};
use decodex_protocol::{AgentDispatchStateDto, AgentLiveMessageDto, AgentLiveMessageKind};

const PROMPT: &str = "Review the migration plan. Keep the current conversation and draft.\n\nExplain the compatibility boundary, then show the validation command.";
const ANSWER: &str = "The migration keeps the current conversation and its saved draft.\n\nThe service owns execution; the desktop owns presentation.\n\n- Preserve the native thread identity.\n- Verify the response before changing the local view.\n\n```sh\ncargo test --locked -p decodex-gpui\n```\n\nThe validation result stays available for review.";

impl AgentSurface {
	fn conversation_review(&mut self, mode: &str, cx: &mut Context<Self>) {
		assert!(matches!(mode, "local" | "local-live" | "native" | "summary" | "live" | "preview"));

		self.visual_workspace_fixture(cx);

		self.profile = None;
		self.workspace.graph_visible = false;
		self.workspace.sidebar_visible = false;
		self.workspace.agent_tree_visible = false;

		self.composer.update(cx, |input, cx| input.set_content("", cx));
		self.submission.previews.clear();
		self.timeline.native.reset();
		self.timeline.scroll.clear();

		let native = !mode.starts_with("local");
		let snapshot = self.snapshot.as_mut().unwrap();
		let work = snapshot.work_items.iter_mut().find(|work| work.id == "agent").unwrap();

		work.codex_thread_id = native.then(|| "review-thread".into());
		work.active_turn_id = matches!(mode, "live" | "local-live").then(|| "current".into());
		work.dispatch_state = if work.active_turn_id.is_some() {
			AgentDispatchStateDto::Running
		} else {
			AgentDispatchStateDto::Idle
		};

		let AgentHistoryResult::Available { entries, live, .. } =
			&mut self.history.as_mut().unwrap().1
		else {
			unreachable!()
		};
		let mut base = entries[0].clone();

		base.receipt = None;
		base.activity = None;
		base.usage = None;
		base.duration_ms = Some(800);
		*entries = [
			(10, "before", "assistant", "The previous result is saved."),
			(11, "current", "user", PROMPT),
			(12, "current", "assistant", ANSWER),
		]
		.into_iter()
		.map(|(id, turn, kind, text)| {
			let mut row = base.clone();

			row.id = id;
			row.turn_id = Some(turn.into());
			row.kind = kind.into();
			row.text = text.into();

			row
		})
		.collect();

		live.clear();

		if matches!(mode, "live" | "local-live") {
			entries.pop();
			live.push(AgentLiveMessageDto {
				kind: AgentLiveMessageKind::AgentMessage,
				turn_id: "current".into(),
				item_id: "answer".into(),
				text: ANSWER.into(),
				truncated: false,
			});
		}
		if native {
			self.native_review_history(mode);

			if mode == "preview" {
				self.capture_send_preview(&PendingCommand {
					key: Some(IdempotencyKey::new("review-preview").unwrap()),
					steer: None,
					execution_intent: None,
					epoch: 0,
					attachments: None,
					references: None,
					owner: Some("agent".into()),
					draft: Some(PROMPT.into()),
				});
			}
		}

		cx.notify();
	}

	fn native_review_history(&mut self, mode: &str) {
		let binding = Binding {
			work: "agent".into(),
			thread: "review-thread".into(),
			account: "fixture".into(),
		};
		let mut rows = vec![
			item(0, "before", "before-answer", "agentMessage", "The previous result is saved."),
			completed(1, "before"),
		];

		if mode != "preview" {
			rows.push(item(2, "current", "request", "userMessage", PROMPT));
		}
		if matches!(mode, "native" | "summary") {
			rows.push(item(3, "current", "answer", "agentMessage", ANSWER));
			rows.push(completed(4, "current"));
		}
		if mode == "summary" {
			self.timeline.native.accept_summary(
				binding,
				rows.into_iter()
					.filter(|row| matches!(row.content, AgentTimelineContent::Item { .. }))
					.map(|row| row.content)
					.collect(),
			);
		} else {
			self.timeline.native.replace(
				binding,
				AgentTimelinePage {
					thread_id: "review-thread".into(),
					entries: rows,
					next_cursor: None,
					weather: Default::default(),
					thread_token_total: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				},
			);
		}
	}
}

fn item(position: u64, turn: &str, id: &str, kind: &str, text: &str) -> AgentTimelineEntry {
	AgentTimelineEntry {
		position,
		content: AgentTimelineContent::Item {
			turn_id: turn.into(),
			item_id: id.into(),
			kind: kind.into(),
			text: text.into(),
			phase: None,
			truncated: false,
			app_ui: false,
			attachments: vec![],
			activity: None,
			collaboration: None,
		},
	}
}

fn completed(position: u64, turn: &str) -> AgentTimelineEntry {
	AgentTimelineEntry {
		position,
		content: AgentTimelineContent::TurnBoundary {
			turn_id: turn.into(),
			completed: true,
			status: Some("completed".into()),
			duration_ms: Some(800),
			usage: None,
			timing: None,
			usage_summary: None,
			error: None,
		},
	}
}

#[cfg(test)]
mod tests {
	use std::thread;

	use gpui::{Bounds, Pixels, VisualTestContext};

	use crate::shell::agent_surface::{
		AgentSurface,
		native_timeline::conversation_review::{
			self, ANSWER, AgentDispatchStateDto, AgentLiveMessageDto, AgentLiveMessageKind, PROMPT,
		},
	};
	use decodex_protocol::AgentHistoryResult;

	fn settled(visual: &mut VisualTestContext) {
		visual.update(crate::ui_frame::draw_frame);

		for delay in [750, 240] {
			thread::sleep(std::time::Duration::from_millis(delay));

			visual.update(crate::ui_frame::draw_frame);
		}
	}

	fn selectors(mode: &str) -> [&'static str; 6] {
		match mode {
			"local" => [
				"message-10-0",
				"message-11-0",
				"message-11-1",
				"message-12-0",
				"message-12-4",
				"code-viewport-message-12-3",
			],
			"local-live" => [
				"message-10-0",
				"message-11-0",
				"message-11-1",
				"live-agent-current-answer-0",
				"live-agent-current-answer-4",
				"code-viewport-live-agent-current-answer-3",
			],
			"summary" => [
				r#"["summary","agent","review-thread","before","before-answer"]-0"#,
				r#"["summary","agent","review-thread","current","request"]-0"#,
				r#"["summary","agent","review-thread","current","request"]-1"#,
				r#"["summary","agent","review-thread","current","answer"]-0"#,
				r#"["summary","agent","review-thread","current","answer"]-4"#,
				r#"code-viewport-["summary","agent","review-thread","current","answer"]-3"#,
			],
			"native" | "live" => [
				r#"["output","agent","review-thread","before","before-answer"]-0"#,
				r#"message-["agent","review-thread",[2,1,"request"]]-0"#,
				r#"message-["agent","review-thread",[2,1,"request"]]-1"#,
				r#"["output","agent","review-thread","current","answer"]-0"#,
				r#"["output","agent","review-thread","current","answer"]-4"#,
				r#"code-viewport-["output","agent","review-thread","current","answer"]-3"#,
			],
			"preview" => [
				r#"["output","agent","review-thread","before","before-answer"]-0"#,
				"message-sending-review-preview-0",
				"message-sending-review-preview-1",
				"no-answer-yet",
				"no-answer-yet",
				"no-answer-yet",
			],
			_ => unreachable!(),
		}
	}

	#[gpui::test]
	fn conversation_geometry_matches_across_saved_live_summary_and_preview(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		let mut differences = Vec::new();

		for width in [760., 1_400.] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(1_400.)));

			let mut baseline: Option<[Bounds<Pixels>; 6]> = None;

			for mode in ["summary", "local", "local-live", "native", "live", "preview"] {
				surface.update(visual, |s, cx| s.conversation_review(mode, cx));

				settled(visual);

				let content = visual.debug_bounds("conversation-content").unwrap();
				let mut rows = [Bounds::default(); 6];

				for (index, selector) in selectors(mode).into_iter().enumerate() {
					if mode == "preview" && index >= 3 {
						continue;
					}

					let mut bounds = visual.debug_bounds(selector).expect(selector);

					bounds.origin -= content.origin;
					rows[index] = bounds;
				}

				if let Some(baseline) = baseline {
					for (index, (actual, expected)) in rows.iter().zip(baseline).enumerate() {
						if mode == "preview" && index >= 3 {
							continue;
						}
						if (actual.origin.x - expected.origin.x).abs() > gpui::px(1.)
							|| (actual.origin.y - expected.origin.y).abs() > gpui::px(1.)
							|| (actual.size.width - expected.size.width).abs() > gpui::px(1.)
							|| (actual.size.height - expected.size.height).abs() > gpui::px(1.)
						{
							differences.push(format!(
								"width={width}, {mode}, message={index}: {actual:?} != {expected:?}"
							));
						}
					}
				} else {
					baseline = Some(rows);
				}
			}
		}

		assert!(differences.is_empty(), "{}", differences.join("\n"));
	}

	#[gpui::test]
	fn coalesced_output_save_and_completion_retain_the_live_footer_transition(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));
		surface.update(visual, |s, cx| s.conversation_review("live", cx));

		settled(visual);

		let text = visual.debug_bounds(selectors("live")[3]).unwrap();
		let live = visual.debug_bounds("native-live-output").unwrap();

		surface.update(visual, |s, cx| {
			s.timeline.native.entries.extend([
				conversation_review::item(3, "current", "answer", "agentMessage", ANSWER),
				conversation_review::completed(4, "current"),
			]);

			let (_, AgentHistoryResult::Available { live, .. }) = s.history.as_mut().unwrap()
			else {
				unreachable!()
			};

			live.clear();
			cx.notify();
		});

		// Observe the first presented frame, which can already be part-way
		// through the transition when GPUI delivers its requested frame.
		visual.update(crate::ui_frame::draw_current_frame);

		assert!(
			visual.debug_bounds("response-footer").unwrap().size.height < gpui::px(24.),
			"completion must animate from the live footprint instead of starting at full height"
		);

		let saved = visual.debug_bounds("native-promotion-content").unwrap();

		assert_eq!(saved.origin, live.origin);
		assert_eq!(saved.size.width, live.size.width);

		settled(visual);

		assert_eq!(visual.debug_bounds(selectors("native")[3]).unwrap(), text);
		assert_eq!(visual.debug_bounds("response-footer").unwrap().size.height, gpui::px(28.));
	}

	#[gpui::test]
	fn preview_edit_hover_and_output_handoffs_keep_wrapped_text_in_place(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		for width in [760., 1_400.] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(1_400.)));
			surface.update(visual, |s, cx| s.conversation_review("preview", cx));

			settled(visual);

			let preview = visual.debug_bounds("sending-message-preview").unwrap();

			surface.update(visual, |s, cx| {
				s.timeline.native.entries.push(conversation_review::item(
					2,
					"current",
					"request",
					"userMessage",
					PROMPT,
				));
				cx.notify();
			});
			visual.update(crate::ui_frame::draw_frame);

			assert!(visual.debug_bounds("sending-message-preview").is_none());
			assert_eq!(visual.debug_bounds("native-promotion-content").unwrap(), preview);

			let edit = visual.debug_bounds("edit-message-action").unwrap();
			let prompt = visual.debug_bounds(selectors("native")[1]).unwrap();

			assert!(edit.right() < prompt.left() - gpui::px(12.), "edit stays outside the bubble");

			visual.simulate_mouse_move(edit.center(), None, Default::default());
			visual.update(crate::ui_frame::draw_frame);

			assert_eq!(visual.debug_bounds("native-promotion-content").unwrap(), preview);

			surface.update(visual, |s, cx| {
				let work = s
					.snapshot
					.as_mut()
					.unwrap()
					.work_items
					.iter_mut()
					.find(|w| w.id == "agent")
					.unwrap();

				work.active_turn_id = Some("current".into());
				work.dispatch_state = AgentDispatchStateDto::Running;

				let (_, AgentHistoryResult::Available { live, .. }) = s.history.as_mut().unwrap()
				else {
					unreachable!()
				};

				live.push(review_live_message());
				cx.notify();
			});

			settled(visual);

			let output = visual.debug_bounds("native-live-output").unwrap();
			let text = selectors("live")[3..]
				.iter()
				.map(|selector| visual.debug_bounds(selector).unwrap())
				.collect::<Vec<_>>();

			surface.update(visual, |s, cx| {
				s.timeline.native.entries.push(conversation_review::item(
					3,
					"current",
					"answer",
					"agentMessage",
					ANSWER,
				));
				cx.notify();
			});
			visual.update(crate::ui_frame::draw_frame);

			assert!(visual.debug_bounds("native-live-output").is_none());
			assert_eq!(visual.debug_bounds("native-promotion-content").unwrap(), output);

			surface.update(visual, |s, cx| {
				s.timeline.native.entries.push(conversation_review::completed(4, "current"));

				let (_, AgentHistoryResult::Available { live, .. }) = s.history.as_mut().unwrap()
				else {
					unreachable!()
				};

				live.clear();

				let work = s
					.snapshot
					.as_mut()
					.unwrap()
					.work_items
					.iter_mut()
					.find(|w| w.id == "agent")
					.unwrap();

				work.active_turn_id = None;
				work.dispatch_state = AgentDispatchStateDto::Idle;

				cx.notify();
			});

			settled(visual);

			let saved = selectors("native")[3..]
				.iter()
				.map(|selector| visual.debug_bounds(selector).unwrap())
				.collect::<Vec<_>>();

			assert_eq!(saved, text, "completion must not move or rewrap paragraphs and code");
			assert_eq!(visual.debug_bounds("response-footer").unwrap().size.height, gpui::px(28.));
		}
	}

	fn review_live_message() -> AgentLiveMessageDto {
		AgentLiveMessageDto {
			kind: AgentLiveMessageKind::AgentMessage,
			turn_id: "current".into(),
			item_id: "answer".into(),
			text: ANSWER.into(),
			truncated: false,
		}
	}
}
