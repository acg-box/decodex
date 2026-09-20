//! Explicit typed replies to live MCP elicitation requests.
use super::*;
use serde_json::{Value, json};

impl ChiefSurface {
	fn mcp_request_is_current(&self, event: i64) -> bool {
		matches!(&self.request, Some(ChiefRequestResult::Available {event_id, method, ..})
            if *event_id == event && method == "mcpServer/elicitation/request")
	}

	pub(super) fn prepare_mcp_inputs(
		&mut self,
		request: &ChiefRequestResult,
		cx: &mut Context<Self>,
	) {
		let ChiefRequestResult::Available { event_id, method, request_json, .. } = request else {
			return;
		};
		if method != "mcpServer/elicitation/request" || self.mcp_form_event == Some(*event_id) {
			return;
		}
		self.mcp_form_event = Some(*event_id);
		self.mcp_url_opened = None;
		self.mcp_inputs.clear();
		self.mcp_answers.clear();
		let Ok(value) = serde_json::from_str::<Value>(request_json.as_str()) else {
			return;
		};
		if let Ok(fields) = decodex_protocol::mcp_form_fields(&value["requestedSchema"]) {
			for field in fields.into_iter().filter(|field| field.choices.is_empty()) {
				self.mcp_inputs.insert(
					field.id,
					cx.new(|cx| {
						ComposerInput::with_placeholder(40, "Your answer", "MCP form answer", cx)
					}),
				);
			}
		}
	}

	fn submit_mcp_form(&mut self, event: i64, cx: &mut Context<Self>) {
		self.submit_mcp_form_with_scope(event, None, cx);
	}

	fn submit_mcp_form_with_scope(
		&mut self,
		event: i64,
		persist: Option<&str>,
		cx: &mut Context<Self>,
	) {
		let Some(ChiefRequestResult::Available { event_id, method, request_json, .. }) =
			&self.request
		else {
			return;
		};
		if *event_id != event
			|| method != "mcpServer/elicitation/request"
			|| self.mcp_form_event != Some(event)
		{
			return;
		}
		let Ok(value) = serde_json::from_str::<Value>(request_json.as_str()) else {
			return;
		};
		if !matches!(value["mode"].as_str(), Some("form" | "openai/form" | "openaiForm")) {
			return;
		}
		let result = (|| -> Result<Value, String> {
			let fields = decodex_protocol::mcp_form_fields(&value["requestedSchema"])?;
			if fields.is_empty() {
				return Ok(
					if value.pointer("/_meta/codex_approval_kind").and_then(Value::as_str)
						== Some("tool_suggestion")
					{
						json!({})
					} else {
						Value::Null
					},
				);
			}
			let mut answers = self.mcp_answers.clone();
			for field in &fields {
				if let Some(input) = self.mcp_inputs.get(&field.id) {
					let text = input.read(cx).content();
					if text.is_empty() {
						continue;
					}
					let answer = if field.kind == "string" {
						json!(text)
					} else {
						serde_json::from_str(text)
							.map_err(|_| format!("Enter a number for {}.", field.title))?
					};
					answers.insert(field.id.clone(), answer);
				}
			}
			decodex_protocol::mcp_form_content(&fields, &answers)
		})();
		match result {
			Ok(content) => {
				let response = json!({"action":"accept","content":content,"_meta":persist.map(|scope|json!({"persist":scope}))});
				if let Err(message) = decodex_protocol::validate_mcp_response(&value, &response) {
					self.feedback = message;
					cx.notify();
					return;
				}
				self.respond(response.to_string(), cx);
			},
			Err(message) => {
				self.feedback = message;
				cx.notify();
			},
		}
	}

	pub(super) fn mcp_form_panel(
		&self,
		event: i64,
		value: &Value,
		cx: &mut Context<Self>,
	) -> gpui::AnyElement {
		if value["_meta"]["codex_approval_kind"] == "tool_suggestion" {
			return self.installation_panel(event, value, cx);
		}
		let mut panel = div()
			.id("mcp-form-panel")
			.flex()
			.flex_col()
			.gap_2()
			.child(format!("Request from {}", value["serverName"].as_str().unwrap_or("MCP server")))
			.child(
				value["message"]
					.as_str()
					.or_else(|| value["description"].as_str())
					.unwrap_or("")
					.to_owned(),
			);
		if let Some(params) = value
			.pointer("/_meta/tool_params_display")
			.or_else(|| value.pointer("/_meta/tool_params"))
		{
			panel = panel.child(serde_json::to_string_pretty(params).unwrap_or_default());
		}

		panel = self.mcp_verification_link(panel, event, value, cx);
		let fields =
			if matches!(value["mode"].as_str(), Some("form" | "openai/form" | "openaiForm")) {
				decodex_protocol::mcp_form_fields(&value["requestedSchema"])
			} else {
				Err("This request requires a different verification flow.".into())
			};
		match fields {
			Ok(fields) => {
				let empty = fields.is_empty();
				for field in fields {
					panel = panel.child(self.mcp_field_row(event, field, cx));
				}
				if empty {
					for (scope, label) in
						[("session", "Allow for this session"), ("always", "Always allow")]
					{
						let response =
							json!({"action":"accept","content":null,"_meta":{"persist":scope}});
						if decodex_protocol::validate_mcp_response(value, &response).is_ok() {
							panel = panel.child(mcp_button(
								format!("mcp-persist-{scope}"),
								label.into(),
								false,
								cx,
								move |s, cx| s.submit_mcp_form_with_scope(event, Some(scope), cx),
							));
						}
					}
				}
				panel = panel.child(mcp_button(
					"mcp-submit".into(),
					"Submit response".into(),
					false,
					cx,
					move |s, cx| s.submit_mcp_form(event, cx),
				));
			},
			Err(message) =>
				if value["mode"] != "url" {
					panel = panel.child(muted(message));
				},
		}
		for (action, label) in [("decline", "Decline"), ("cancel", "Cancel")] {
			panel = panel.child(mcp_button(
				format!("mcp-{action}"),
				label.into(),
				false,
				cx,
				move |s, cx| {
					if s.mcp_request_is_current(event) {
						s.respond(
							json!({"action":action,"content":null,"_meta":null}).to_string(),
							cx,
						);
					}
				},
			));
		}
		panel
			.on_action(cx.listener(move |s, _: &SubmitComposer, _, cx| {
				s.submit_mcp_form(event, cx);
				cx.stop_propagation();
			}))
			.into_any_element()
	}

	fn mcp_verification_link(
		&self,
		mut panel: gpui::Stateful<gpui::Div>,
		event: i64,
		value: &Value,
		cx: &mut Context<Self>,
	) -> gpui::Stateful<gpui::Div> {
		if value["mode"] == "url" {
			if let Some(url) = value["url"]
				.as_str()
				.and_then(|text| reqwest::Url::parse(text).ok())
				.filter(|url| {
					matches!(url.scheme(), "https" | "http")
						&& url.username().is_empty()
						&& url.password().is_none()
				}) {
				let url = url.to_string();
				let open_url = url.clone();
				panel = panel.child(url.clone()).child(mcp_button(
					"mcp-open-url".into(),
					"Open verification page".into(),
					false,
					cx,
					move |s, cx| {
						if s.mcp_request_is_current(event) {
							cx.open_url(&open_url);
							s.mcp_url_opened = Some((event, open_url.clone()));
							cx.notify();
						}
					},
				));
				if self.mcp_url_opened.as_ref() == Some(&(event, url.clone())) {
					panel = panel.child(mcp_button(
						"mcp-confirm-url".into(),
						"I completed verification".into(),
						false,
						cx,
						move |s, cx| {
							if s.mcp_url_opened.as_ref() == Some(&(event, url.clone()))
								&& s.mcp_request_is_current(event)
							{
								s.respond(
									json!({"action":"accept","content":null,"_meta":null})
										.to_string(),
									cx,
								);
							}
						},
					));
				}
			} else {
				panel =
					panel.child(muted("A valid HTTP or HTTPS verification link is unavailable."));
			}
		}
		panel
	}

	fn mcp_field_row(
		&self,
		event: i64,
		field: decodex_protocol::McpFormField,
		cx: &mut Context<Self>,
	) -> gpui::Div {
		let mut row = div().flex().flex_col().gap_2().child(format!(
			"{}{}",
			field.title,
			if field.required { " *" } else { "" }
		));
		if let Some(description) = field.description {
			row = row.child(muted(description));
		}
		if let Some(input) = self.mcp_inputs.get(&field.id) {
			row = row.child(div().h(px(40.0)).child(input.clone()));
		}
		for (index, choice) in field.choices.into_iter().enumerate() {
			let id = field.id.clone();
			let answer = choice.value;
			let multiple = field.kind == "array";
			let selected = self.mcp_answers.get(&id).is_some_and(|value| {
				if multiple {
					value.as_array().is_some_and(|values| values.contains(&answer))
				} else {
					value == &answer
				}
			});
			row = row.child(mcp_button(
				format!("mcp-{event}-{id}-{index}"),
				choice.label,
				selected,
				cx,
				move |s, cx| {
					if !s.mcp_request_is_current(event) || s.mcp_form_event != Some(event) {
						return;
					}
					if multiple {
						let entry = s.mcp_answers.entry(id.clone()).or_insert_with(|| json!([]));
						if let Some(values) = entry.as_array_mut() {
							if values.contains(&answer) {
								values.retain(|value| value != &answer);
							} else {
								values.push(answer.clone());
							}
						}
					} else {
						s.mcp_answers.insert(id.clone(), answer.clone());
					}
					cx.notify();
				},
			));
		}
		row
	}
}

pub(super) fn mcp_button(
	id: String,
	label: String,
	selected: bool,
	cx: &mut Context<ChiefSurface>,
	action: impl Fn(&mut ChiefSurface, &mut Context<ChiefSurface>) + 'static,
) -> gpui::AnyElement {
	let action = std::rc::Rc::new(action);
	let click = action.clone();
	div()
		.id(SharedString::from(id.clone()))
		.debug_selector(move || id)
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label.clone())
		.p_2()
		.rounded(px(5.0))
		.bg(rgba(if selected { 0xffffff18 } else { 0xffffff06 }))
		.cursor_pointer()
		.on_click(cx.listener(move |s, _, _, cx| click(s, cx)))
		.on_key_down(cx.listener(move |s, key: &gpui::KeyDownEvent, _, cx| {
			if ["enter", "space"].contains(&key.keystroke.key.as_str()) {
				cx.stop_propagation();
				action(s, cx);
			}
		}))
		.child(label)
		.into_any_element()
}

#[cfg(test)]
mod tests {
	use super::*;
	#[gpui::test]
	fn approval_renders_only_offered_persistence_and_rejects_stale_scope(
		cx: &mut gpui::TestAppContext,
	) {
		use decodex_protocol::{ChiefPendingEventDto, ChiefWorkKindDto};
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
            s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
                workspaces: vec![], dependencies: vec![],
                work_items: vec![ChiefWorkItemDto {
                    id:"root".into(), parent_goal_id:None, kind:ChiefWorkKindDto::Goal,
                    title:"Chief".into(),codex_thread_id:Some("thread".into()),active_turn_id:None,
                    dispatch_state:ChiefDispatchStateDto::Idle,status:ChiefWorkStatusDto::Open,
                    next_check_at_micros:None,created_at_micros:1,updated_at_micros:1
                }],
                pending_events:vec![ChiefPendingEventDto { id:7, source_event_id:"approval".into(), work_item_id:"root".into(),event_kind:"user_input_pending".into(),created_at_micros:1,delivery_claimed:false }]
            })));
            let request=ChiefRequestResult::Available {event_id:7,work_id:"root".into(),method:"mcpServer/elicitation/request".into(),request_json:HistoryText::new(json!({"mode":"form","serverName":"test","message":"Allow this tool?","requestedSchema":null,"_meta":{"persist":["session"]}}).to_string()).unwrap()};
            s.prepare_mcp_inputs(&request,cx);
            s.request=Some(request);
            s.submit_mcp_form_with_scope(7,Some("always"),cx);
            assert_eq!(s.feedback,"This persistence scope was not offered");
            s.feedback.clear();
            s.submit_mcp_form_with_scope(6,Some("session"),cx);
            assert!(s.feedback.is_empty());
            assert!(s.command_task.is_none());
        });
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1200.0)));
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("mcp-submit").is_some(), "form submit must render");
		assert!(visual.debug_bounds("mcp-persist-always").is_none());
		let bounds = visual.debug_bounds("mcp-persist-session").expect("offered session action");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.feedback, "No service profile is configured.");
			assert!(s.command_task.is_none());
		});
	}

	#[gpui::test]
	fn url_open_requires_separate_explicit_confirmation(cx: &mut gpui::TestAppContext) {
		use decodex_protocol::{ChiefPendingEventDto, ChiefWorkKindDto};
		let (surface, visual) = cx.add_window_view(|_, cx| ChiefSurface::new(cx));
		surface.update(visual, |s, cx| {
            s.apply_result(Ok(ChiefSnapshotResult::Available(ChiefSnapshotDto {
                workspaces: vec![], dependencies: vec![],
                work_items: vec![ChiefWorkItemDto {
                    id:"root".into(), parent_goal_id:None, kind:ChiefWorkKindDto::Goal,
                    title:"Chief".into(),codex_thread_id:Some("thread".into()),active_turn_id:None,
                    dispatch_state:ChiefDispatchStateDto::Idle,status:ChiefWorkStatusDto::Open,
                    next_check_at_micros:None,created_at_micros:1,updated_at_micros:1
                }],
                pending_events:vec![ChiefPendingEventDto { id:7, source_event_id:"approval".into(), work_item_id:"root".into(),event_kind:"user_input_pending".into(),created_at_micros:1,delivery_claimed:false }]
            })));
            let request=ChiefRequestResult::Available {event_id:7,work_id:"root".into(),method:"mcpServer/elicitation/request".into(),request_json:HistoryText::new(json!({"mode":"url","serverName":"test","message":"Sign in","url":"https://example.test/verify","elicitationId":"verification"}).to_string()).unwrap()};
            s.prepare_mcp_inputs(&request,cx);
            s.request=Some(request);
            cx.notify();
        });
		visual.update(|window, cx| {
			window.resize(gpui::size(px(1180.0), px(1200.0)));
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("mcp-confirm-url").is_none());
		let bounds = visual.debug_bounds("mcp-open-url").expect("verification link");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.mcp_url_opened, Some((7, "https://example.test/verify".into())));
			assert!(s.command_task.is_none());
			assert!(s.feedback.is_empty());
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let bounds =
			visual.debug_bounds("mcp-confirm-url").expect("explicit confirmation after open");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, cx| {
			assert_eq!(s.feedback, "No service profile is configured.");
			let request = ChiefRequestResult::Available {
				event_id: 8,
				work_id: "root".into(),
				method: "mcpServer/elicitation/request".into(),
				request_json: HistoryText::new(
					json!({"mode":"url","url":"file:///tmp/private"}).to_string(),
				)
				.unwrap(),
			};
			s.prepare_mcp_inputs(&request, cx);
			s.request = Some(request);
			s.snapshot.as_mut().unwrap().pending_events[0].id = 8;
			assert!(s.mcp_url_opened.is_none());
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("mcp-open-url").is_none());
		assert!(visual.debug_bounds("mcp-confirm-url").is_none());
	}

	#[gpui::test]
	fn form_defaults_are_not_answers_and_same_request_keeps_drafts(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx,|s,cx| {
            let request=ChiefRequestResult::Available {event_id:7,work_id:"chief".into(),method:"mcpServer/elicitation/request".into(),request_json:HistoryText::new(json!({"mode":"form","requestedSchema":{"type":"object","properties":{"agree":{"type":"boolean","default":true},"name":{"type":"string"}},"required":["agree","name"]}}).to_string()).unwrap()};
            s.prepare_mcp_inputs(&request,cx);s.request=Some(request.clone());s.selected=Some("chief".into());
            s.submit_mcp_form(7,cx);
            assert!(s.feedback.contains("required"));assert!(s.command_task.is_none());
            s.mcp_answers.insert("agree".into(),json!(false));
            s.mcp_inputs["name"].update(cx,|input,cx|input.set_content("My name",cx));
            s.prepare_mcp_inputs(&request,cx);
            assert_eq!(s.mcp_inputs["name"].read(cx).content(),"My name");
            assert_eq!(s.mcp_answers["agree"],false);
            s.submit_mcp_form(6,cx);assert!(s.command_task.is_none());
            s.submit_mcp_form(7,cx);assert_eq!(s.feedback,"No service profile is configured.");
        });
	}
}
