//! Provider request forms bound to the exact persisted request event.

use gpui::{AnyElement, AppContext as _, Div, KeyDownEvent};
use serde_json::{Map, Value};

#[cfg(test)]
use crate::shell::agent_surface::{
	AgentPendingEventDto, AgentSnapshotResult, AgentWorkKindDto, AgentWorkStatusDto, Duration,
};
use crate::{
	shell::{
		agent_surface,
		agent_surface::{
			AgentDispatchStateDto, AgentRequestResult, AgentSnapshotDto, AgentSurface,
			AgentWorkItemDto, ComposerInput, Context, InteractiveElement, IntoElement, LoadState,
			ParentElement, Role, SharedString, SmoothControl, StatefulInteractiveElement, Styled,
			SubmitComposer, ui_theme::BLUE,
		},
	},
	ui_theme::HOVER_FILL,
};
use decodex_protocol::MAX_HISTORY_INLINE_BYTES;

#[derive(Default)]
pub(super) struct RequestReader {
	event: i64,
	revision: u64,
	offset: usize,
	starts: Vec<usize>,
}
impl RequestReader {
	fn reset(&mut self, event: i64) {
		*self = Self { event, revision: self.revision.wrapping_add(1), offset: 0, starts: vec![0] };
	}

	fn navigate(&mut self, event: i64, revision: u64, from: usize, to: usize) {
		if self.event != event || self.revision != revision || self.offset != from {
			return;
		}
		if !self.starts.contains(&to) {
			self.starts.push(to);
		}

		self.offset = to;
	}
}

/// A timer belongs to one provider request, including after selection changes.
pub(super) struct QuestionTimer {
	started: std::time::Instant,
	disabled: bool,
}
impl QuestionTimer {
	fn new(value: &Value, now: std::time::Instant) -> Self {
		Self { started: now, disabled: value["isBlocking"].as_bool() != Some(false) }
	}

	fn remaining(&self, now: std::time::Instant) -> Option<u64> {
		if self.disabled {
			return None;
		}

		let elapsed = now.saturating_duration_since(self.started).as_secs();

		// Upstream ignores deprecated autoResolutionMs: 60s grace, then 60s visible.
		(elapsed >= 60).then(|| 120_u64.saturating_sub(elapsed))
	}

	fn claim_expired(&mut self, now: std::time::Instant) -> bool {
		if self.remaining(now) != Some(0) {
			return false;
		}

		// Never retry an automatic response, including after an uncertain acknowledgment.
		self.disabled = true;

		true
	}
}

impl AgentSurface {
	pub(super) fn prepare_question_inputs(
		&mut self,
		request: &AgentRequestResult,
		cx: &mut Context<Self>,
	) {
		let event = match request {
			AgentRequestResult::Available { event_id, .. } => *event_id,
			_ => 0,
		};

		self.request_reader.reset(event);
		self.prepare_mcp_inputs(request, cx);
		self.question_inputs.clear();

		if let AgentRequestResult::Available { event_id, method, request_json, .. } = request
			&& method == "item/tool/requestUserInput"
			&& let Ok(value) = serde_json::from_str::<Value>(request_json.as_str())
		{
			self.question_timers
				.entry(*event_id)
				.or_insert_with(|| QuestionTimer::new(&value, std::time::Instant::now()));

			for question in value["questions"].as_array().into_iter().flatten() {
				if let Some(id) = question["id"].as_str() {
					self.question_inputs.insert(
						id.into(),
						cx.new(|cx| {
							let mut input = ComposerInput::with_placeholder(
								40,
								"Your answer",
								"Answer to Agent",
								cx,
							);

							if question["isSecret"].as_bool() == Some(true) {
								input.obscure();
							}

							input
						}),
					);
				}
			}
		}
	}

	pub(super) fn request_panel(
		&self,
		snapshot: &AgentSnapshotDto,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let Some(AgentRequestResult::Available { work_id, event_id, method, request_json }) =
			&self.request
		else {
			return gpui::div().into_any_element();
		};

		if work_id != &work.id || !snapshot.pending_events.iter().any(|event| event.id == *event_id)
		{
			return gpui::div().into_any_element();
		}

		let large = request_json.as_str().len() > MAX_HISTORY_INLINE_BYTES;
		let value: Value = serde_json::from_str(request_json.as_str()).unwrap_or_default();

		if method == "mcpServer/elicitation/request" {
			return if large {
				self.large_request_panel(*event_id, request_json.as_str(), cx)
					.child(self.mcp_form_panel(*event_id, &value, cx))
					.into_any_element()
			} else {
				self.mcp_form_panel(*event_id, &value, cx)
			};
		}

		let mut panel = gpui::div()
			.capture_key_down(cx.listener(|s, _, _, cx| {
				s.snooze_question_timeout();
				cx.notify();
			}))
			.capture_any_mouse_down(cx.listener(|s, _, _, cx| {
				s.snooze_question_timeout();
				cx.notify();
			}))
			.on_action(cx.listener(|s, _: &SubmitComposer, _, cx| {
				s.submit_answers(cx);
				cx.stop_propagation();
			}))
			.p_3()
			.rounded(gpui::px(8.0))
			.border_1()
			.border_color(gpui::rgba(0xffffff18))
			.flex()
			.flex_col()
			.gap_2();

		if large {
			panel = panel.child(self.large_request_panel(*event_id, request_json.as_str(), cx));
		}
		if method == "item/tool/requestUserInput" {
			for question in value["questions"].as_array().into_iter().flatten() {
				panel = panel.child(self.question_form(question, cx));
			}

			panel = panel.child(
				gpui::div()
					.id("agent-answer-questions")
					.role(Role::Button)
					.tab_index(0)
					.aria_label("Send answers")
					.cursor_pointer()
					.text_color(gpui::rgb(BLUE))
					.on_click(cx.listener(|s, _, _, cx| s.submit_answers(cx)))
					.child("Send answers")
					.smooth(),
			);

			if let Some(remaining) = self
				.question_timers
				.get(event_id)
				.and_then(|timer| timer.remaining(std::time::Instant::now()))
			{
				panel = panel.child(agent_surface::muted(format!(
					"Skips unanswered in {remaining}s. Interact to keep this question open."
				)));
			}
		} else {
			panel = self.approval_request_panel(panel, method, request_json.as_str(), &value, cx);
		}

		panel.into_any_element()
	}

	fn large_request_panel(&self, event: i64, text: &str, cx: &mut Context<Self>) -> Div {
		let offset = self.request_reader.offset.min(text.len());
		let mut end = offset.saturating_add(8_192).min(text.len());

		while !text.is_char_boundary(end) {
			end -= 1;
		}

		let generation = self.generation;
		let revision = self.request_reader.revision;
		let section =
			self.request_reader.starts.iter().position(|start| *start == offset).unwrap_or(0) + 1;
		let previous =
			self.request_reader.starts.iter().copied().filter(|start| *start < offset).max();
		let mut panel = gpui::div()
			.flex()
			.flex_col()
			.gap_2()
			.child(format!("Request details · section {section}"))
			.child(
				gpui::div()
					.id("large-request-detail")
					.max_h(gpui::px(280.0))
					.overflow_y_scroll()
					.font_family("Menlo")
					.text_size(gpui::px(12.0))
					.child(text[offset..end].to_owned()),
			);

		for (id, label, target) in [
			("large-request-previous", "Previous section", previous),
			("large-request-next", "Next section", (end < text.len()).then_some(end)),
		] {
			let Some(target) = target else { continue };

			panel = panel.child(gpui::div().id(id).debug_selector(move || id.into())
				.role(Role::Button).tab_index(0).aria_label(label).cursor_pointer().child(label)
				.on_key_down(cx.listener(move |s, key: &KeyDownEvent, _, cx| {
					if ["enter", "space"].contains(&key.keystroke.key.as_str()) && s.generation == generation
						&& matches!(&s.request, Some(AgentRequestResult::Available { event_id, .. }) if *event_id == event) {
						cx.stop_propagation();
						s.request_reader.navigate(event, revision, offset, target); cx.notify();
					}
				}))
				.on_click(cx.listener(move |s, _, _, cx| {
					if s.generation == generation && matches!(&s.request, Some(AgentRequestResult::Available { event_id, .. }) if *event_id == event) {
						s.request_reader.navigate(event, revision, offset, target); cx.notify();
					}
				})));
		}

		if end == text.len() {
			panel = panel.child(agent_surface::muted("End of request"));
		}

		panel
	}

	pub(super) fn request_summary(&self, text: String) -> String {
		if text.len() > 4_096
			&& matches!(&self.request, Some(AgentRequestResult::Available { request_json, .. }) if request_json.as_str().len() > decodex_protocol::MAX_HISTORY_INLINE_BYTES)
		{
			"Complete content is in the request details above.".into()
		} else {
			text
		}
	}

	fn file_approval_details(&self, mut panel: Div, value: &Value) -> Div {
		let details = value["changeDetails"]
			.as_str()
			.unwrap_or("File paths and patch details are unavailable.");

		panel = panel.child(
			gpui::div()
				.id("file-approval-details")
				.max_h(gpui::px(280.0))
				.overflow_y_scroll()
				.text_size(gpui::px(12.0))
				.font_family("Menlo")
				.child(self.request_summary(details.to_owned())),
		);

		if value["changeDetailsTruncated"] == true {
			panel = panel.child(agent_surface::muted("File change details shortened"));
		}

		panel
	}

	fn approval_request_panel(
		&self,
		mut panel: Div,
		method: &str,
		request_json: &str,
		value: &Value,
		cx: &mut Context<Self>,
	) -> Div {
		let (heading, selector) = if method == "item/fileChange/requestApproval" {
			("Allow file changes?", "approval-kind-file-change")
		} else if method == "item/permissions/requestApproval" {
			("Allow requested access for this turn?", "approval-kind-permissions")
		} else if method == "item/commandExecution/requestApproval" && value["kind"] == "writeStdin"
		{
			("Allow input to the running terminal?", "approval-kind-write-stdin")
		} else {
			("Allow this command?", "approval-kind-command")
		};

		panel = panel.child(gpui::div().debug_selector(move || selector.into()).child(heading));

		for key in ["reason", "command", "cwd", "grantRoot"] {
			if let Some(text) = value[key].as_str() {
				panel = panel.child(
					gpui::div()
						.text_size(gpui::px(12.0))
						.child(self.request_summary(text.to_owned())),
				);
			}
		}

		if method == "item/fileChange/requestApproval" {
			panel = self.file_approval_details(panel, value);
		}
		if matches!(
			method,
			"item/permissions/requestApproval" | "item/commandExecution/requestApproval"
		) && let Some(environment) = value["environmentId"].as_str().filter(|id| !id.is_empty())
		{
			panel = panel.child(
				gpui::div()
					.debug_selector(|| "approval-executor-environment".into())
					.text_size(gpui::px(12.0))
					.child(self.request_summary(format!("Execution environment: {environment}"))),
			);
		}
		if method == "item/permissions/requestApproval" {
			panel = panel.child(
				gpui::div()
					.text_size(gpui::px(12.0))
					.child(self.request_summary(permission_summary(&value["permissions"]))),
			);
			panel = panel.child(self.request_choice(
				"decline",
				"Decline",
				serde_json::json!({"permissions":{},"scope":"turn"}),
				cx,
			));
			panel = panel.child(self.request_choice(
				"allow",
				"Allow for this turn",
				serde_json::json!({"permissions":value["permissions"],"scope":"turn"}),
				cx,
			));
		} else {
			let decisions = if method == "item/fileChange/requestApproval" {
				vec!["decline".into(), "accept".into()]
			} else {
				agent_surface::offered_decisions(method, request_json)
			};

			for decision in decisions {
				let label = match decision.as_str() {
					"accept" => "Allow once",
					"acceptForSession" => "Allow for this session",
					"decline" => "Decline",
					"cancel" => "Stop this turn",
					_ => continue,
				};

				panel = panel.child(self.request_choice(
					&decision,
					label,
					serde_json::json!({"decision":decision}),
					cx,
				));
			}
			for (index, decision) in value["availableDecisions"]
				.as_array()
				.into_iter()
				.flatten()
				.filter(|decision| decision.is_object())
				.enumerate()
			{
				let label = if decision.get("acceptWithExecpolicyAmendment").is_some() {
					"Allow with the proposed command rule"
				} else if decision.get("applyNetworkPolicyAmendment").is_some() {
					"Apply the proposed network rule"
				} else {
					continue;
				};

				panel =
					panel.child(gpui::div().text_size(gpui::px(11.0)).child(self.request_summary(
						serde_json::to_string_pretty(decision).unwrap_or_default(),
					)));
				panel = panel.child(self.request_choice(
					&format!("policy-{index}"),
					label,
					serde_json::json!({"decision":decision}),
					cx,
				));
			}
		}

		panel
	}

	fn snooze_question_timeout(&mut self) {
		if let Some(AgentRequestResult::Available { event_id, .. }) = &self.request
			&& let Some(timer) = self.question_timers.get_mut(event_id)
		{
			timer.disabled = true;
		}
	}

	pub(super) fn tick_question_timeout(&mut self, cx: &mut Context<Self>) {
		if self.sending
			|| self.uncertain
			|| self.state != LoadState::Ready
			|| self.profile.is_none()
		{
			return;
		}

		let Some(AgentRequestResult::Available { event_id, work_id, method, .. }) = &self.request
		else {
			return;
		};

		if method != "item/tool/requestUserInput"
			|| self.selected.as_ref() != Some(work_id)
			|| !self.snapshot.as_ref().is_some_and(|snapshot| {
				snapshot
					.pending_events
					.iter()
					.any(|event| event.id == *event_id && &event.work_item_id == work_id)
					&& snapshot.work_items.iter().any(|work| {
						&work.id == work_id
							&& work.dispatch_state == AgentDispatchStateDto::Running
							&& work.active_turn_id.is_some()
					})
			}) {
			return;
		}
		if self.question_inputs.values().any(|input| !input.read(cx).content().is_empty()) {
			self.snooze_question_timeout();

			return;
		}
		if self
			.question_timers
			.get_mut(event_id)
			.is_some_and(|timer| timer.claim_expired(std::time::Instant::now()))
		{
			self.respond(serde_json::json!({"answers":{}}).to_string(), cx);
		}
	}

	fn submit_answers(&mut self, cx: &mut Context<Self>) {
		self.snooze_question_timeout();

		let mut answers = Map::new();

		for (id, input) in &self.question_inputs {
			let text = input.read(cx).content().trim();

			if text.is_empty() {
				self.feedback = "Answer each question before sending.".into();

				cx.notify();

				return;
			}

			answers.insert(id.clone(), serde_json::json!({"answers":[text]}));
		}

		if !answers.is_empty() {
			self.respond(serde_json::json!({"answers":answers}).to_string(), cx);
		}
	}

	pub(super) fn sync_request(&mut self, cx: &mut Context<Self>) {
		if self.request_task.is_some() {
			return;
		}

		if let Some(AgentRequestResult::Available { event_id, work_id, .. }) = &self.request
			&& self.selected.as_ref() == Some(work_id)
			&& self.snapshot.as_ref().is_some_and(|snapshot| {
				snapshot.pending_events.iter().any(|event| event.id == *event_id)
			}) {
			return;
		}

		let event = self
			.snapshot
			.as_ref()
			.and_then(|snapshot| {
				snapshot.pending_events.iter().find(|event| {
					Some(&event.work_item_id) == self.selected.as_ref()
						&& event.event_kind.ends_with("_pending")
				})
			})
			.map(|event| event.id);

		match event {
			Some(id) if !matches!(&self.request,Some(AgentRequestResult::Available {event_id,..}) if *event_id==id) =>
				self.load_request(id, cx),
			None => {
				self.request = None;

				self.question_inputs.clear();
			},
			_ => {},
		}
	}

	fn question_form(&self, question: &Value, cx: &mut Context<Self>) -> AnyElement {
		let id = question["id"].as_str().unwrap_or_default();
		let mut row = gpui::div()
			.flex()
			.flex_col()
			.gap_2()
			.child(question["question"].as_str().unwrap_or_default().to_owned());
		let Some(input) = self.question_inputs.get(id) else {
			return row.into_any_element();
		};

		for (index, option) in question["options"].as_array().into_iter().flatten().enumerate() {
			let Some(label) = option["label"].as_str() else {
				continue;
			};
			let input = input.clone();
			let label = label.to_owned();
			let selected = input.read(cx).content() == label;
			let answer = label.clone();
			let selector = format!("question-{id}-{index}");

			row = row.child(
				gpui::div()
					.id(SharedString::from(format!("question-{id}-{index}")))
					.debug_selector(move || selector)
					.role(Role::Button)
					.tab_index(0)
					.aria_label(label.clone())
					.p_2()
					.rounded(gpui::px(6.0))
					.bg(gpui::rgba(if selected { 0xffffff18 } else { 0xffffff06 }))
					.cursor_pointer()
					.on_click(cx.listener(move |_, _, _, cx| {
						input.update(cx, |input, cx| input.set_content(&answer, cx));
						cx.notify();
					}))
					.child(label)
					.child(agent_surface::muted(
						option["description"].as_str().unwrap_or_default().to_owned(),
					))
					.smooth(),
			);
		}

		row.child(gpui::div().h(gpui::px(36.0)).child(input.clone())).into_any_element()
	}

	fn request_choice(
		&self,
		id: &str,
		label: &'static str,
		response: Value,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let event = match &self.request {
			Some(AgentRequestResult::Available { event_id, .. }) => *event_id,
			_ => 0,
		};
		let generation = self.generation;
		let revision = self.request_reader.revision;
		let response = response.to_string();
		let selector = format!("request-{id}");

		gpui::div()
			.id(SharedString::from(format!("request-{id}")))
			.debug_selector(move || selector)
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label)
			.px_2()
			.h(gpui::px(28.0))
			.flex()
			.items_center()
			.rounded(gpui::px(5.0))
			.cursor_pointer()
			.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
			.text_color(gpui::rgb(BLUE))
			.on_click(cx.listener(move |s, _, _, cx| {
				if s.generation == generation && s.request_reader.revision == revision
					&& matches!(&s.request, Some(AgentRequestResult::Available { event_id, .. }) if *event_id == event) {
					s.respond(response.clone(), cx);
				}
			}))
			.child(label)
			.smooth()
			.into_any_element()
	}
}

fn permission_summary(value: &Value) -> String {
	// Preserve the exact requested paths and access modes, including newer provider fields.
	serde_json::to_string_pretty(value).unwrap_or_else(|_| "Access details unavailable".into())
}

#[cfg(test)]
mod timing_tests {
	use std::{
		thread,
		time::{Duration, Instant},
	};

	#[cfg(test)]
	#[cfg(not(test))]
	use gpui::AppContext as _;
	#[cfg(test)]
	#[cfg(not(test))]
	use gpui::px;

	use crate::shell::agent_surface::requests::{
		self, AgentRequestResult, AgentSurface, QuestionTimer,
	};

	#[test]
	fn nonblocking_timeout_has_grace_countdown_and_single_empty_response_claim() {
		let start = Instant::now();
		let mut timer = QuestionTimer::new(
			&serde_json::json!({"isBlocking":false,"autoResolutionMs":1}),
			start,
		);

		assert_eq!(timer.remaining(start + Duration::from_secs(59)), None);
		assert_eq!(timer.remaining(start + Duration::from_secs(60)), Some(60));
		assert_eq!(timer.remaining(start + Duration::from_secs(119)), Some(1));
		assert!(!timer.claim_expired(start + Duration::from_secs(119)));
		assert!(timer.claim_expired(start + Duration::from_secs(120)));
		assert!(!timer.claim_expired(start + Duration::from_secs(121)));
	}

	#[test]
	fn blocking_missing_malformed_and_snoozed_requests_do_not_auto_resolve() {
		let start = Instant::now();

		for value in [
			serde_json::json!({}),
			serde_json::json!({"isBlocking":true}),
			serde_json::json!({"isBlocking":"false"}),
			serde_json::json!({"autoResolutionMs":1}),
		] {
			assert!(
				!QuestionTimer::new(&value, start).claim_expired(start + Duration::from_secs(500))
			);
		}

		let mut timer = QuestionTimer::new(&serde_json::json!({"isBlocking":false}), start);

		timer.disabled = true;

		assert!(!timer.claim_expired(start + Duration::from_secs(500)));
	}
	#[gpui::test]
	fn large_request_reader_navigates_and_rejects_stale_sections(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx); s.graph_visible = false;

			let work = s.selected.clone().unwrap();

			s.snapshot.as_mut().unwrap().pending_events = vec![decodex_protocol::AgentPendingEventDto { id:902, source_event_id:"large".into(),work_item_id:work.clone(),event_kind:"permission_pending".into(),created_at_micros:1,delivery_claimed:false }];

			let request = AgentRequestResult::Available { event_id:902,work_id:work,method:"item/commandExecution/requestApproval".into(),request_json:decodex_protocol::AgentRequestText::new(serde_json::json!({"command":"echo 界🙂".repeat(2_000),"availableDecisions":["accept","decline"]}).to_string()).unwrap() };

			s.prepare_question_inputs(&request,cx); s.request = Some(request);
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.), gpui::px(1_200.)));
			window.draw(cx).clear();
		});

		// Let the fixture's dock-close animation settle before choosing a
		// scroll offset and clicking the request pagination control.
		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		surface.update(visual, |s, cx| {
			s.transcript_scroll.get(s.selected.as_ref().unwrap()).unwrap().scroll_to_bottom();
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let next = visual.debug_bounds("large-request-next").expect("next section");

		visual.simulate_click(next.center(), gpui::Modifiers::default());
		surface.read_with(visual, |s, _| {
			assert!(s.request_reader.offset > 0);
			assert!(s.submission.command.is_none());
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let first_offset = surface.read_with(visual, |s, _| s.request_reader.offset);

		visual.simulate_keystrokes("space");
		surface.read_with(visual, |s, _| {
			assert!(
				s.request_reader.offset > first_offset,
				"Space advances the focused section control"
			)
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let previous = visual.debug_bounds("large-request-previous").expect("previous section");

		visual.simulate_click(previous.center(), gpui::Modifiers::default());

		surface.update(visual, |s, cx| {
			assert_eq!(s.request_reader.offset, first_offset);

			let revision = s.request_reader.revision;
			let request = s.request.clone().unwrap();

			s.prepare_question_inputs(&request, cx);
			s.request_reader.navigate(902, revision, 0, 8_192);

			assert_eq!(
				s.request_reader.offset, 0,
				"a control from the previous request rendering cannot move the new reader"
			);
			assert!(s.submission.command.is_none());
		});
	}

	#[gpui::test]
	fn large_permission_grant_reaches_dispatch_without_copying_reply(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;

			let work = s.selected.clone().unwrap();

			s.snapshot.as_mut().unwrap().pending_events =
				vec![decodex_protocol::AgentPendingEventDto {
					id: 904,
					source_event_id: "large-permissions".into(),
					work_item_id: work.clone(),
					event_kind: "permission_pending".into(),
					created_at_micros: 1,
					delivery_claimed: false,
				}];

			let request = AgentRequestResult::Available {
				event_id: 904,
				work_id: work,
				method: "item/permissions/requestApproval".into(),
				request_json: decodex_protocol::AgentRequestText::new(
					serde_json::json!({"permissions":{"fileSystem":{"write":["/tmp/界".repeat(10_000)]}}})
						.to_string(),
				)
				.unwrap(),
			};

			s.prepare_question_inputs(&request, cx);

			s.request = Some(request);
		});

		for _ in 0..32 {
			visual.update(|window, cx| {
				window.resize(gpui::size(gpui::px(1_180.), gpui::px(1_200.)));
				window.draw(cx).clear();
			});
			surface.update(visual, |s, cx| {
				s.transcript_scroll.get(s.selected.as_ref().unwrap()).unwrap().scroll_to_bottom();
				cx.notify();
			});
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});

			if visual.debug_bounds("request-allow").is_some() {
				surface.update(visual, |s, cx| {
					s.transcript_scroll
						.get(s.selected.as_ref().unwrap())
						.unwrap()
						.scroll_to_bottom();
					cx.notify();
				});
				visual.update(|window, cx| {
					window.draw(cx).clear();
				});

				let allow = visual.debug_bounds("request-allow").unwrap();

				visual.simulate_click(allow.center(), gpui::Modifiers::default());
				surface.read_with(visual, |s, _| {
					assert_eq!(
						s.feedback, "No service profile is configured.",
						"must reach dispatch, not reject the full grant as oversized"
					);
				});

				return;
			}

			let next =
				visual.debug_bounds("large-request-next").expect("complete permission detail");

			visual.simulate_click(next.center(), gpui::Modifiers::default());
		}

		panic!("permission approval did not become available");
	}

	#[gpui::test]
	fn question_option_interaction_snoozes_only_its_request(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| requests::AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
            s.apply_result(Ok(requests::AgentSnapshotResult::Available(requests::AgentSnapshotDto {
                runtime_source: None,
                workspaces: vec![], dependencies: vec![],
                work_items: vec![requests::AgentWorkItemDto {
                    id:"root".into(), parent_goal_id:None, kind:requests::AgentWorkKindDto::Goal,
                    title:"Agent".into(),codex_thread_id:Some("thread".into()),active_turn_id:Some("turn".into()),
                    dispatch_state:requests::AgentDispatchStateDto::Running,status:requests::AgentWorkStatusDto::Open,
                    next_check_at_micros:None,created_at_micros:1,updated_at_micros:1
                }],
                pending_events:vec![requests::AgentPendingEventDto { id:7, source_event_id:"question".into(), work_item_id:"root".into(),event_kind:"user_input_pending".into(),created_at_micros:1,delivery_claimed:false }]
            })));

            let request=requests::AgentRequestResult::Available {event_id:7,work_id:"root".into(),method:"item/tool/requestUserInput".into(),request_json:decodex_protocol::AgentRequestText::new(serde_json::json!({"isBlocking":false,"questions":[{"id":"format","question":"Which format?","options":[{"label":"PDF","description":"Document"}]}]}).to_string()).unwrap()};

            s.prepare_question_inputs(&request,cx);

            s.question_timers.get_mut(&7).unwrap().started = Instant::now() - requests::Duration::from_secs(61);

            s.question_timers.insert(8, requests::QuestionTimer::new(&serde_json::json!({"isBlocking":false}),Instant::now()));

            s.request=Some(request);
        });

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(1_200.0)));
			window.draw(cx).clear();
		});

		let bounds = visual.debug_bounds("question-format-0").expect("visible option");

		surface.read_with(visual, |s, cx| {
			assert!(s.question_inputs["format"].read(cx).content().is_empty());
			assert!(!s.question_timers[&7].disabled);
		});
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());

		surface.update(visual, |s, cx| {
			assert!(s.question_timers[&7].disabled);
			assert!(!s.question_timers[&8].disabled);
			assert_eq!(s.question_inputs["format"].read(cx).content(), "PDF");

			let request = s.request.clone().unwrap();

			s.prepare_question_inputs(&request, cx);

			assert!(s.question_timers[&7].disabled, "reloading must not rearm the same event");

			// JSON escaping exceeds the wire limit even though the editable answer fits.
			let answer = "\"".repeat(9_000);

			s.question_inputs["format"].update(cx, |input, cx| input.set_content(&answer, cx));
			s.submit_answers(cx);

			assert!(s.feedback.starts_with("Response is too large after encoding"));
			assert_eq!(s.question_inputs["format"].read(cx).content(), answer);
			assert!(s.submission.command.is_none() && !s.sending);
			assert_eq!(s.request, Some(request));

			s.question_inputs["format"].update(cx, |input, cx| input.set_content("PDF", cx));
			s.submit_answers(cx);

			assert_eq!(s.feedback, "No service profile is configured.");
			assert_eq!(s.question_inputs["format"].read(cx).content(), "PDF");
		});
	}
	#[gpui::test]
	fn approval_panels_show_only_the_native_executor(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		for method in ["item/permissions/requestApproval", "item/commandExecution/requestApproval"]
		{
			for (environment, cwd) in [
				(Some("remote/工作"), r"C:\工作\repo"),
				(None, "/workspace"),
				(Some(""), r"\\server\share\repo"),
			] {
				surface.update(visual, |s, cx| {
				s.visual_workspace_fixture(cx);

				s.graph_visible = false;

				let work = s.selected.clone().unwrap();

				s.snapshot.as_mut().unwrap().pending_events = vec![decodex_protocol::AgentPendingEventDto {
					id:902, source_event_id:"executor-request".into(), work_item_id:work.clone(), event_kind:"permission_pending".into(), created_at_micros:1, delivery_claimed:false,
				}];
				s.request = Some(AgentRequestResult::Available {
					event_id:902, work_id:work, method:method.into(),
					request_json:decodex_protocol::AgentRequestText::new(serde_json::json!({"environmentId":environment,"cwd":cwd,"permissions":{"network":{"enabled":true}}}).to_string()).unwrap(),
				});

				cx.notify();
			});

				visual.update(|window, cx| {
					window.resize(gpui::size(gpui::px(1_180.), gpui::px(1_200.)));
					window.draw(cx).clear();
				});

				assert!(
					visual
						.debug_bounds(if method == "item/permissions/requestApproval" {
							"approval-kind-permissions"
						} else {
							"approval-kind-command"
						})
						.is_some()
				);
				assert_eq!(
					visual.debug_bounds("approval-executor-environment").is_some(),
					environment.is_some_and(|id| !id.is_empty())
				);
			}
		}
	}
	#[gpui::test]
	fn terminal_input_approval_is_distinct_from_new_and_legacy_commands(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		for kind in [Some("writeStdin"), Some("command"), None] {
			surface.update(visual, |s, cx| {
				s.visual_workspace_fixture(cx);

				s.graph_visible = false;

				let work = s.selected.clone().unwrap();

				s.snapshot.as_mut().unwrap().pending_events = vec![decodex_protocol::AgentPendingEventDto {
					id: 901, source_event_id: "stdin-request".into(), work_item_id: work.clone(),
					event_kind: "permission_pending".into(), created_at_micros: 1, delivery_claimed: false,
				}];

				let mut value = serde_json::json!({"command":"confirm\n", "cwd":"/workspace", "availableDecisions":["accept","decline"]});

				if let Some(kind) = kind { value["kind"] = serde_json::json!(kind); }

				s.request = Some(AgentRequestResult::Available {
					event_id: 901, work_id: work, method: "item/commandExecution/requestApproval".into(),
					request_json: decodex_protocol::AgentRequestText::new(value.to_string()).unwrap(),
				});

				cx.notify();
			});

			visual.update(|window, cx| {
				window.resize(gpui::size(gpui::px(1_180.), gpui::px(1_200.)));
				window.draw(cx).clear();
			});

			assert_eq!(
				visual.debug_bounds("approval-kind-write-stdin").is_some(),
				kind == Some("writeStdin")
			);
			assert_eq!(
				visual.debug_bounds("approval-kind-command").is_some(),
				kind != Some("writeStdin")
			);
		}
	}
}
