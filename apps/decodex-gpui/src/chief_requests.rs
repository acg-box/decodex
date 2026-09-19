//! Provider request forms bound to the exact persisted request event.
use super::*;

impl ChiefSurface {
	pub(super) fn prepare_question_inputs(
		&mut self,
		request: &ChiefRequestResult,
		cx: &mut Context<Self>,
	) {
		self.question_inputs.clear();
		if let ChiefRequestResult::Available { method, request_json, .. } = request
			&& method == "item/tool/requestUserInput"
			&& let Ok(value) = serde_json::from_str::<serde_json::Value>(request_json.as_str())
		{
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
		let mut panel = div()
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
		} else {
			panel = panel.child(if method == "item/fileChange/requestApproval" {
				"Allow file changes?"
			} else if method == "item/permissions/requestApproval" {
				"Allow requested access for this turn?"
			} else {
				"Allow this command?"
			});
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
				panel = panel.child(
					div().text_size(px(12.0)).child(permission_summary(&value["permissions"])),
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
					offered_decisions(method, request_json.as_str())
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
		}
		panel.into_any_element()
	}

	fn submit_answers(&mut self, cx: &mut Context<Self>) {
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
			row = row.child(
				div()
					.id(SharedString::from(format!("question-{id}-{index}")))
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
