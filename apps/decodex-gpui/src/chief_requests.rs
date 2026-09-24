//! Provider request forms bound to the exact persisted request event.
use super::*;

/// A timer belongs to one provider request, including after selection changes.
pub(super) struct QuestionTimer {
	started: std::time::Instant,
	disabled: bool,
}

impl QuestionTimer {
	fn new(value: &serde_json::Value, now: std::time::Instant) -> Self {
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

impl ChiefSurface {
	pub(super) fn prepare_question_inputs(
		&mut self,
		request: &ChiefRequestResult,
		cx: &mut Context<Self>,
	) {
		self.prepare_mcp_inputs(request, cx);
		self.question_inputs.clear();
		if let ChiefRequestResult::Available { event_id, method, request_json, .. } = request
			&& method == "item/tool/requestUserInput"
			&& let Ok(value) = serde_json::from_str::<serde_json::Value>(request_json.as_str())
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
								"Answer to Chief",
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
		snapshot: &ChiefSnapshotDto,
		work: &ChiefWorkItemDto,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let Some(ChiefRequestResult::Available { work_id, event_id, method, request_json }) =
			&self.request
		else {
			return div().into_any_element();
		};
		if work_id != &work.id || !snapshot.pending_events.iter().any(|event| event.id == *event_id)
		{
			return div().into_any_element();
		}
		let value: serde_json::Value =
			serde_json::from_str(request_json.as_str()).unwrap_or_default();
		if method == "mcpServer/elicitation/request" {
			return self.mcp_form_panel(*event_id, &value, cx);
		}
		let mut panel = div()
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
			.rounded(px(8.0))
			.border_1()
			.border_color(rgba(0xffffff18))
			.flex()
			.flex_col()
			.gap_2();
		if method == "item/tool/requestUserInput" {
			for question in value["questions"].as_array().into_iter().flatten() {
				panel = panel.child(self.question_form(question, cx));
			}
			panel = panel.child(
				div()
					.id("chief-answer-questions")
					.role(Role::Button)
					.tab_index(0)
					.aria_label("Send answers")
					.cursor_pointer()
					.text_color(rgb(ui_theme::BLUE))
					.on_click(cx.listener(|s, _, _, cx| s.submit_answers(cx)))
					.child("Send answers")
					.smooth(),
			);
			if let Some(remaining) = self
				.question_timers
				.get(event_id)
				.and_then(|timer| timer.remaining(std::time::Instant::now()))
			{
				panel = panel.child(muted(format!(
					"Skips unanswered in {remaining}s. Interact to keep this question open."
				)));
			}
		} else {
			panel = self.approval_request_panel(panel, method, request_json.as_str(), &value, cx);
		}
		panel.into_any_element()
	}

	fn approval_request_panel(
		&self,
		mut panel: gpui::Div,
		method: &str,
		request_json: &str,
		value: &serde_json::Value,
		cx: &mut Context<Self>,
	) -> gpui::Div {
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
		panel = panel.child(div().debug_selector(move || selector.into()).child(heading));
		for key in ["reason", "command", "cwd", "grantRoot"] {
			if let Some(text) = value[key].as_str() {
				panel = panel.child(div().text_size(px(12.0)).child(text.to_owned()));
			}
		}
		if method == "item/fileChange/requestApproval" {
			let details = value["changeDetails"]
				.as_str()
				.unwrap_or("File paths and patch details are unavailable.");
			panel = panel.child(
				div()
					.id("file-approval-details")
					.max_h(px(280.0))
					.overflow_y_scroll()
					.text_size(px(12.0))
					.font_family("Menlo")
					.child(details.to_owned()),
			);
			if value["changeDetailsTruncated"] == true {
				panel = panel.child(muted("File change details shortened"));
			}
		}

		if method == "item/permissions/requestApproval" {
			panel = panel
				.child(div().text_size(px(12.0)).child(permission_summary(&value["permissions"])));
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
				offered_decisions(method, request_json)
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
				panel = panel.child(
					div()
						.text_size(px(11.0))
						.child(serde_json::to_string_pretty(decision).unwrap_or_default()),
				);
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
		if let Some(ChiefRequestResult::Available { event_id, .. }) = &self.request
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
		let Some(ChiefRequestResult::Available { event_id, work_id, method, .. }) = &self.request
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
							&& work.dispatch_state == ChiefDispatchStateDto::Running
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
		let mut answers = serde_json::Map::new();
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
		if let Some(ChiefRequestResult::Available { event_id, work_id, .. }) = &self.request
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
			Some(id) if !matches!(&self.request,Some(ChiefRequestResult::Available {event_id,..}) if *event_id==id) =>
				self.load_request(id, cx),
			None => {
				self.request = None;
				self.question_inputs.clear();
			},
			_ => {},
		}
	}

	fn question_form(
		&self,
		question: &serde_json::Value,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let id = question["id"].as_str().unwrap_or_default();
		let mut row = div()
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
				div()
					.id(SharedString::from(format!("question-{id}-{index}")))
					.debug_selector(move || selector)
					.role(Role::Button)
					.tab_index(0)
					.aria_label(label.clone())
					.p_2()
					.rounded(px(6.0))
					.bg(rgba(if selected { 0xffffff18 } else { 0xffffff06 }))
					.cursor_pointer()
					.on_click(cx.listener(move |_, _, _, cx| {
						input.update(cx, |input, cx| input.set_content(&answer, cx));
						cx.notify();
					}))
					.child(label)
					.child(muted(option["description"].as_str().unwrap_or_default().to_owned()))
					.smooth(),
			);
		}
		row.child(div().h(px(36.0)).child(input.clone())).into_any_element()
	}

	fn request_choice(
		&self,
		id: &str,
		label: &'static str,
		response: serde_json::Value,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		let response = response.to_string();
		div()
			.id(SharedString::from(format!("request-{id}")))
			.role(Role::Button)
			.tab_index(0)
			.aria_label(label)
			.px_2()
			.h(px(28.0))
			.flex()
			.items_center()
			.rounded(px(5.0))
			.cursor_pointer()
			.hover(|s| s.bg(rgba(0xffffff10)))
			.text_color(rgb(ui_theme::BLUE))
			.on_click(cx.listener(move |s, _, _, cx| s.respond(response.clone(), cx)))
			.child(label)
			.smooth()
			.into_any_element()
	}
}

fn permission_summary(value: &serde_json::Value) -> String {
	// Preserve the exact requested paths and access modes, including newer provider fields.
	serde_json::to_string_pretty(value).unwrap_or_else(|_| "Access details unavailable".into())
}

#[cfg(test)]
mod timing_tests {
	use super::QuestionTimer;
	use serde_json::json;
	use std::time::{Duration, Instant};

	#[test]
	fn nonblocking_timeout_has_grace_countdown_and_single_empty_response_claim() {
		let start = Instant::now();
		let mut timer =
			QuestionTimer::new(&json!({"isBlocking":false,"autoResolutionMs":1}), start);
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
			json!({}),
			json!({"isBlocking":true}),
			json!({"isBlocking":"false"}),
			json!({"autoResolutionMs":1}),
		] {
			assert!(
				!QuestionTimer::new(&value, start).claim_expired(start + Duration::from_secs(500))
			);
		}
		let mut timer = QuestionTimer::new(&json!({"isBlocking":false}), start);
		timer.disabled = true;
		assert!(!timer.claim_expired(start + Duration::from_secs(500)));
	}
	#[gpui::test]
	fn question_option_interaction_snoozes_only_its_request(cx: &mut gpui::TestAppContext) {
		use super::*;
		use decodex_protocol::{ChiefPendingEventDto, ChiefWorkKindDto};
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
            s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
                runtime_source: None,
                workspaces: vec![], dependencies: vec![],
                work_items: vec![ChiefWorkItemDto {
                    id:"root".into(), parent_goal_id:None, kind:ChiefWorkKindDto::Goal,
                    title:"Chief".into(),codex_thread_id:Some("thread".into()),active_turn_id:Some("turn".into()),
                    dispatch_state:ChiefDispatchStateDto::Running,status:ChiefWorkStatusDto::Open,
                    next_check_at_micros:None,created_at_micros:1,updated_at_micros:1
                }],
                pending_events:vec![ChiefPendingEventDto { id:7, source_event_id:"question".into(), work_item_id:"root".into(),event_kind:"user_input_pending".into(),created_at_micros:1,delivery_claimed:false }]
            })));
            let request=ChiefRequestResult::Available {event_id:7,work_id:"root".into(),method:"item/tool/requestUserInput".into(),request_json:HistoryText::new(json!({"isBlocking":false,"questions":[{"id":"format","question":"Which format?","options":[{"label":"PDF","description":"Document"}]}]}).to_string()).unwrap()};
            s.prepare_question_inputs(&request,cx);
            s.question_timers.get_mut(&7).unwrap().started = Instant::now() - Duration::from_secs(61);
            s.question_timers.insert(8, QuestionTimer::new(&json!({"isBlocking":false}),Instant::now()));
            s.request=Some(request);
        });
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1200.0)));
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
		});
	}
	#[gpui::test]
	fn terminal_input_approval_is_distinct_from_new_and_legacy_commands(
		cx: &mut gpui::TestAppContext,
	) {
		use super::*;
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		for kind in [Some("writeStdin"), Some("command"), None] {
			surface.update(visual, |s, cx| {
				s.visual_workspace_fixture(cx);
				s.graph_visible = false;
				let work = s.selected.clone().unwrap();
				s.snapshot.as_mut().unwrap().pending_events = vec![decodex_protocol::ChiefPendingEventDto {
					id: 901, source_event_id: "stdin-request".into(), work_item_id: work.clone(),
					event_kind: "permission_pending".into(), created_at_micros: 1, delivery_claimed: false,
				}];
				let mut value = json!({"command":"confirm\n", "cwd":"/workspace", "availableDecisions":["accept","decline"]});
				if let Some(kind) = kind { value["kind"] = json!(kind); }
				s.request = Some(ChiefRequestResult::Available {
					event_id: 901, work_id: work, method: "item/commandExecution/requestApproval".into(),
					request_json: HistoryText::new(value.to_string()).unwrap(),
				});
				cx.notify();
			});
			visual.update(|window, cx| {
				window.resize(gpui::size(px(1180.), px(1200.)));
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
