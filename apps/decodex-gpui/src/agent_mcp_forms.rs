//! Explicit typed replies to live MCP elicitation requests.
use std::rc::Rc;

use super::requests::{parameter_label, readable_value};

use gpui::{AnyElement, AppContext as _, Div, KeyDownEvent, Stateful};
use reqwest::Url;
use serde_json::Value;

#[cfg(test)]
use crate::shell::agent_surface::{
	AgentDispatchStateDto, AgentSnapshotDto, AgentSnapshotResult, AgentWorkItemDto,
	AgentWorkStatusDto,
};
use crate::{
	shell::agent_surface::{
		self, AgentRequestResult, AgentSurface, ComposerInput, Context, InteractiveElement,
		IntoElement, ParentElement, Role, SharedString, StatefulInteractiveElement, Styled,
		SubmitComposer,
	},
	ui_feedback::Feedback,
	ui_theme,
};
use decodex_protocol::McpFormField;

impl AgentSurface {
	fn mcp_request_is_current(&self, event: i64) -> bool {
		matches!(&self.request, Some(AgentRequestResult::Available {event_id, method, ..})
            if *event_id == event && method == "mcpServer/elicitation/request")
	}

	pub(super) fn prepare_mcp_inputs(
		&mut self,
		request: &AgentRequestResult,
		cx: &mut Context<Self>,
	) {
		let AgentRequestResult::Available { event_id, method, request_json, .. } = request else {
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

		if let Ok(fields) = decodex_protocol::mcp_request_fields(&value) {
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
		let Some(AgentRequestResult::Available { event_id, method, request_json, .. }) =
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

		let fields = match decodex_protocol::mcp_request_fields(&value) {
			Ok(fields) => fields,
			Err(message) => {
				self.feedback = Feedback::error(message);

				cx.notify();

				return;
			},
		};
		let result = (|| -> Result<Value, String> {
			if fields.is_empty() {
				return Ok(Value::Null);
			}

			let mut answers = self.mcp_answers.clone();

			for field in &fields {
				if let Some(input) = self.mcp_inputs.get(&field.id) {
					let text = input.read(cx).content();

					if text.is_empty() {
						continue;
					}

					let answer = if field.kind == "string" {
						serde_json::json!(text)
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
				let response = serde_json::json!({"action":"accept","content":content,"_meta":persist.map(|scope|serde_json::json!({"persist":scope}))});

				if let Err(message) = decodex_protocol::validate_mcp_response(&value, &response) {
					self.feedback = Feedback::error(message);

					cx.notify();

					return;
				}

				self.respond(response.to_string(), cx);
			},
			Err(message) => {
				self.feedback = Feedback::warning(message);

				cx.notify();
			},
		}
	}

	pub(super) fn mcp_form_panel(
		&self,
		event: i64,
		value: &Value,
		large_request: Option<&str>,
		cx: &mut Context<Self>,
	) -> AnyElement {
		if value["_meta"]["codex_approval_kind"] == "tool_suggestion" {
			let install = self.installation_panel(event, value, cx);
			return gpui::div()
				.child(install)
				.children(large_request.map(|raw| self.large_request_panel(event, raw, cx)))
				.into_any_element();
		}
		let fields =
			if matches!(value["mode"].as_str(), Some("form" | "openai/form" | "openaiForm")) {
				decodex_protocol::mcp_request_fields(value)
			} else {
				Err("This request needs another verification method.".into())
			};
		let empty = fields.as_ref().is_ok_and(Vec::is_empty);
		let tool = empty && value["_meta"]["codex_approval_kind"] == "mcp_tool_call";
		let meta = &value["_meta"];
		let source = meta["connector_name"]
			.as_str()
			.or(value["serverName"].as_str())
			.unwrap_or("MCP server");
		let message = value["message"].as_str().or(value["description"].as_str()).unwrap_or("");
		let title = meta["tool_title"].as_str().filter(|title| !title.trim().is_empty());
		let standard_prompt = meta["tool_name"]
			.as_str()
			.is_some_and(|name| message == format!("Allow {source} to run tool \"{name}\"?"));
		let mut panel = gpui::div()
			.id("mcp-form-panel")
			.debug_selector(|| "mcp-form-panel".into())
			.w_full()
			.min_w_0()
			.flex()
			.flex_col()
			.gap_2()
			.p_3()
			.rounded_md()
			.border_1()
			.border_color(ui_theme::rgba(ui_theme::PANEL_DIVIDER))
			.child(agent_surface::muted(format!("Request from {source}")));
		if tool && let Some(title) = title {
			panel = panel.child(
				gpui::div()
					.text_size(gpui::px(13.))
					.font_weight(gpui::FontWeight::SEMIBOLD)
					.child(title.to_owned()),
			);
		}
		let explanation = if tool && title.is_some() && standard_prompt {
			meta["tool_description"].as_str().unwrap_or("")
		} else {
			message
		};
		if !explanation.is_empty() {
			let text = if self.request_reader.details_open {
				self.request_summary(explanation.into())
			} else if tool && standard_prompt {
				preview_text(explanation.trim().lines().next().unwrap_or(""), 180)
			} else {
				self.request_summary(explanation.into())
			};
			panel =
				panel.child(agent_surface::selectable_prose(format!("mcp-message-{event}"), text));
		}

		panel = self.mcp_argument_details(panel, event, value, tool, large_request, cx);
		panel = self.mcp_verification_link(panel, event, value, cx);
		let mut actions = gpui::div().flex().flex_wrap().items_center().gap_2();
		match fields {
			Ok(fields) => {
				for field in fields {
					panel = panel.child(self.mcp_field_row(event, field, cx));
				}
				actions = actions.child(request_button(
					"mcp-submit",
					if empty { "Allow once" } else { "Submit response" },
					true,
					cx,
					move |s, cx| s.submit_mcp_form(event, cx),
				));
			},
			Err(message) if value["mode"] != "url" => {
				panel = panel.child(agent_surface::muted(message));
			},
			_ => {},
		}
		panel = panel.child(self.mcp_decisions(actions, event, value, tool, empty, cx));
		panel
			.on_action(cx.listener(move |s, _: &SubmitComposer, _, cx| {
				s.submit_mcp_form(event, cx);
				cx.stop_propagation();
			}))
			.into_any_element()
	}

	fn mcp_argument_details(
		&self,
		mut panel: Stateful<Div>,
		event: i64,
		value: &Value,
		tool: bool,
		large_request: Option<&str>,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let meta = &value["_meta"];
		let params = display_params(meta);
		let account = mcp_account_label(value);
		let expanded = self.request_reader.details_open;
		let full_fields = expanded && large_request.is_none();
		let mut rows = params.iter().enumerate().collect::<Vec<_>>();
		if !full_fields {
			rows.sort_by_key(|(_, (key, _, _))| technical_id(key));
			rows.truncate(3);
		}
		let shown = rows.len();
		for (index, (_, label, v)) in rows {
			let preview = match *v {
				Value::String(s) if !full_fields => preview_text(s, 180),
				Value::String(s) => s.clone(),
				Value::Array(a) if !full_fields => format!("{} items", a.len()),
				Value::Object(o) if !full_fields => format!("{} fields", o.len()),
				v => readable_value(v),
			};

			panel = panel.child(
				gpui::div()
					.w_full()
					.min_w_0()
					.flex()
					.gap_2()
					.items_start()
					.child(
						gpui::div()
							.w(gpui::px(96.))
							.flex_none()
							.child(agent_surface::muted(parameter_label(label))),
					)
					.child(gpui::div().flex_1().min_w_0().child(agent_surface::selectable_prose(
						format!("mcp-arg-{event}-{index}"),
						preview,
					))),
			);
		}
		let extra = shown < params.len()
			|| params.iter().any(|(_, _, v)| {
				v.is_object()
					|| v.is_array()
					|| v.as_str().is_some_and(|s| s.char_indices().nth(180).is_some())
			});
		let tool_name = tool.then(|| meta["tool_name"].as_str()).flatten();
		if expanded && let Some(name) = tool_name {
			panel = panel.child(agent_surface::selectable_prose(
				format!("mcp-tool-{event}"),
				format!("Tool: {name}"),
			));
		}
		if let Some(account) = &account
			&& (!tool || expanded)
		{
			panel = panel.child(
				gpui::div()
					.debug_selector(|| "mcp-account-context".into())
					.child(agent_surface::muted(account.clone())),
			);
		}
		if large_request.is_some()
			|| extra
			|| tool_name.is_some()
			|| (tool && account.is_some())
			|| expanded
		{
			let detail = request_disclosure(
				"mcp-request-details",
				if expanded { "Hide details" } else { "Show details" },
				expanded,
				cx,
				move |s, cx| {
					if s.mcp_request_is_current(event) {
						s.request_reader.details_open = !s.request_reader.details_open;
						cx.notify();
					}
				},
			);
			panel = panel.child(gpui::div().flex().child(detail));
			if expanded && let Some(raw) = large_request {
				panel = panel.child(self.large_request_panel(event, raw, cx));
			}
		}
		panel
	}

	fn mcp_decisions(
		&self,
		mut actions: Div,
		event: i64,
		value: &Value,
		tool: bool,
		empty: bool,
		cx: &mut Context<Self>,
	) -> Div {
		let cancel = |action: &'static str, label: &'static str, cx: &mut Context<Self>| {
			request_button(
				if action == "cancel" { "mcp-cancel" } else { "mcp-decline" },
				label,
				false,
				cx,
				move |s, cx| {
					if s.mcp_request_is_current(event) {
						s.respond(
							serde_json::json!({"action":action,"content":null,"_meta":null})
								.to_string(),
							cx,
						);
					}
				},
			)
		};
		actions = actions.child(if tool {
			cancel("cancel", "Cancel tool call", cx)
		} else {
			cancel("decline", "Decline", cx)
		});
		let scopes = [("session", "Allow for this session"), ("always", "Always allow")]
			.into_iter()
			.filter(|(scope, _)| {
				empty
					&& decodex_protocol::validate_mcp_response(
						value,
						&serde_json::json!({"action":"accept","content":null,"_meta":{"persist":scope}}),
					)
					.is_ok()
			})
			.collect::<Vec<_>>();
		if !scopes.is_empty() {
			let more = self.request_reader.options_open;
			actions = actions.child(gpui::div().flex_1()).child(request_disclosure(
				"mcp-more-options",
				if more { "Fewer options" } else { "More options" },
				more,
				cx,
				move |s, cx| {
					if s.mcp_request_is_current(event) {
						s.request_reader.options_open = !s.request_reader.options_open;
						cx.notify();
					}
				},
			));
			let mut group = gpui::div().flex().flex_col().gap_2().child(actions);
			if more {
				let mut options = gpui::div()
					.flex()
					.flex_wrap()
					.items_center()
					.gap_2()
					.pt_2()
					.border_t_1()
					.border_color(ui_theme::rgba(ui_theme::PANEL_DIVIDER));
				for (scope, label) in scopes {
					options = options.child(request_button(
						format!("mcp-persist-{scope}"),
						label,
						false,
						cx,
						move |s, cx| s.submit_mcp_form_with_scope(event, Some(scope), cx),
					));
				}
				if !tool {
					options = options.child(cancel("cancel", "Cancel request", cx));
				}
				group = group.child(options);
			}
			return group;
		}
		if !tool {
			actions = actions.child(cancel("cancel", "Cancel request", cx));
		}
		actions
	}

	fn mcp_verification_link(
		&self,
		mut panel: Stateful<Div>,
		event: i64,
		value: &Value,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		if value["mode"] == "url" {
			if let Some(url) =
				value["url"].as_str().and_then(|text| Url::parse(text).ok()).filter(|url| {
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
									serde_json::json!({"action":"accept","content":null,"_meta":null})
										.to_string(),
									cx,
								);
							}
						},
					));
				}
			} else {
				panel = panel.child(agent_surface::muted("Verification link unavailable."));
			}
		}

		panel
	}

	fn mcp_field_row(&self, event: i64, field: McpFormField, cx: &mut Context<Self>) -> Div {
		let mut row = gpui::div().flex().flex_col().gap_2().child(format!(
			"{}{}",
			field.title,
			if field.required { " *" } else { "" }
		));

		if let Some(description) = field.description {
			row = row.child(agent_surface::selectable_prose(
				format!("mcp-description-{event}-{}", field.id),
				description,
			));
		}
		if let Some(input) = self.mcp_inputs.get(&field.id) {
			row = row.child(gpui::div().h(gpui::px(40.0)).child(input.clone()));
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
						let entry = s
							.mcp_answers
							.entry(id.clone())
							.or_insert_with(|| serde_json::json!([]));

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

/// Native display parameters can omit or redact values from the raw arguments.
fn display_params(meta: &Value) -> Vec<(&str, &str, &Value)> {
	let display = meta["tool_params_display"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| {
			let key = row["name"].as_str()?.trim();
			let label = row["display_name"]
				.as_str()
				.map(str::trim)
				.filter(|s| !s.is_empty())
				.unwrap_or(key);
			if key.is_empty() {
				return None;
			}
			Some((key, label, row.get("value")?))
		})
		.collect::<Vec<_>>();
	if !display.is_empty() {
		return display;
	}
	meta["tool_params"]
		.as_object()
		.into_iter()
		.flatten()
		.map(|(k, v)| (k.as_str(), k.as_str(), v))
		.collect()
}
fn technical_id(key: &str) -> bool {
	key == "id" || key.ends_with("Id") || key.ends_with("ID") || key.ends_with("_id")
}
fn preview_text(text: &str, limit: usize) -> String {
	let mut chars = text.chars();
	let mut out = chars.by_ref().take(limit).collect::<String>();
	if chars.next().is_some() {
		out.push('…');
	}
	out
}
fn request_button(
	id: impl Into<SharedString>,
	label: &'static str,
	primary: bool,
	cx: &mut Context<AgentSurface>,
	action: impl Fn(&mut AgentSurface, &mut Context<AgentSurface>) + 'static,
) -> AnyElement {
	let id = id.into();
	let button = if primary {
		ui_theme::settings_primary_button(id, label)
	} else {
		ui_theme::settings_button(id, label)
	};
	on_activate(button, cx, action)
}

fn request_disclosure(
	id: &'static str,
	label: &'static str,
	expanded: bool,
	cx: &mut Context<AgentSurface>,
	action: impl Fn(&mut AgentSurface, &mut Context<AgentSurface>) + 'static,
) -> AnyElement {
	let button = ui_theme::settings_button_base(id, label)
		.cursor_pointer()
		.gap_1()
		.border_color(ui_theme::rgba(0))
		.bg(ui_theme::rgba(0))
		.text_color(ui_theme::rgb(ui_theme::TEXT_MUTED))
		.hover(|s| {
			s.bg(ui_theme::rgba(ui_theme::HOVER_FILL)).text_color(ui_theme::rgb(ui_theme::TEXT))
		})
		.focus_visible(|s| s.border_color(ui_theme::rgb(ui_theme::ACCENT)))
		.child(crate::shell::workspace_symbols::process_chevron(
			SharedString::from(format!("{id}-chevron")),
			expanded,
		));
	on_activate(button, cx, action)
}

fn on_activate(
	button: Stateful<Div>,
	cx: &mut Context<AgentSurface>,
	action: impl Fn(&mut AgentSurface, &mut Context<AgentSurface>) + 'static,
) -> AnyElement {
	let action = Rc::new(action);
	let click = action.clone();
	button
		.on_click(cx.listener(move |s, _, _, cx| click(s, cx)))
		.on_key_down(cx.listener(move |s, key: &KeyDownEvent, _, cx| {
			if !key.is_held && matches!(key.keystroke.key.as_str(), "enter" | "space") {
				cx.stop_propagation();
				action(s, cx);
			}
		}))
		.into_any_element()
}

pub(super) fn mcp_button(
	id: String,
	label: String,
	selected: bool,
	cx: &mut Context<AgentSurface>,
	action: impl Fn(&mut AgentSurface, &mut Context<AgentSurface>) + 'static,
) -> AnyElement {
	let button = gpui::div()
		.id(SharedString::from(id.clone()))
		.debug_selector(move || id)
		.role(Role::Button)
		.tab_index(0)
		.aria_label(label.clone())
		.p_2()
		.rounded(gpui::px(5.0))
		.bg(ui_theme::rgba(if selected { 0xffffff18 } else { 0xffffff06 }))
		.cursor_pointer()
		.child(label);
	on_activate(button, cx, action)
}

fn mcp_account_label(value: &Value) -> Option<String> {
	if value["serverName"] != "codex_apps" {
		return None;
	}

	let link = value.pointer("/_meta/link_id")?.as_str().filter(|link| {
		!link.trim().is_empty() && link.len() <= 4_096 && !link.chars().any(char::is_control)
	})?;

	Some(format!("Connected account link: {link}"))
}

#[cfg(test)]
mod tests {
	use gpui::AppContext as _;

	use crate::shell::agent_surface::mcp_forms::{self, AgentRequestResult, AgentSurface, Value};
	#[cfg(test)]
	use crate::shell::agent_surface::mcp_forms::{
		AgentDispatchStateDto, AgentSnapshotDto, AgentSnapshotResult, AgentWorkItemDto,
		AgentWorkStatusDto,
	};
	use decodex_protocol::{AgentPendingEventDto, AgentWorkKindDto};

	#[test]
	fn tool_arguments_prefer_native_display_values() {
		let metadata = serde_json::json!({"tool_params":{"jobId":"raw-id","unlisted":"private"},"tool_params_display":[{"name":"jobId","display_name":"Job ID","value":"masked"}]});
		assert_eq!(
			mcp_forms::display_params(&metadata),
			vec![("jobId", "Job ID", &serde_json::json!("masked"))]
		);
		let raw = serde_json::json!({"tool_params":{"jobId":"raw-id"}});
		assert_eq!(
			mcp_forms::display_params(&raw),
			vec![("jobId", "jobId", &serde_json::json!("raw-id"))]
		);
		assert_eq!(mcp_forms::parameter_label("conversationId"), "Conversation ID");
		let grant = serde_json::json!({"fileSystem":{"read":["/tmp/Case Sensitive"]},"network":{"enabled":true}});
		assert_eq!(
			mcp_forms::readable_value(&grant),
			"File System\n  Read\n    • /tmp/Case Sensitive\nNetwork\n  Enabled  true"
		);
		assert_eq!(mcp_forms::preview_text("中文摘要", 3), "中文摘…");
	}

	#[test]
	fn account_label_uses_only_native_apps_metadata() {
		let request = serde_json::json!({"serverName":"codex_apps","_meta":{"connector_id":"calendar","link_id":" work/link "},"tool_params":{"link_id":"personal"}});

		assert_eq!(
			mcp_forms::mcp_account_label(&request),
			Some("Connected account link:  work/link ".into())
		);

		let mut other = request.clone();

		other["serverName"] = serde_json::json!("custom_mcp");

		assert_eq!(mcp_forms::mcp_account_label(&other), None);

		other = request;

		for link in [
			Value::Null,
			serde_json::json!(" "),
			serde_json::json!("account\nname"),
			serde_json::json!("x".repeat(4_097)),
		] {
			other["_meta"]["link_id"] = link;

			assert_eq!(mcp_forms::mcp_account_label(&other), None);
		}
	}

	#[gpui::test]
	fn approval_renders_only_offered_persistence_and_rejects_stale_scope(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
            s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
		agents: Default::default(),
		assistant: Default::default(),
		guardian: Default::default(),
		portfolio: Default::default(),
                archived_work_ids: Default::default(),
                agent_slots: Default::default(),
                conversation_titles: Default::default(),
                context_references: vec![],
connection_initializing: false,
                runtime_source: None,
                projects: vec![], dependencies: vec![],
                work_items: vec![AgentWorkItemDto {
                    id:"root".into(), parent_goal_id:None, kind:AgentWorkKindDto::Goal,
                    title:"Agent".into(),codex_thread_id:Some("thread".into()),active_turn_id:None,
                    dispatch_state:AgentDispatchStateDto::Idle,status:AgentWorkStatusDto::Open,
                    next_check_at_micros:None,created_at_micros:1,updated_at_micros:1
                }],
                pending_events:vec![AgentPendingEventDto { id:7, source_event_id:"approval".into(), work_item_id:"root".into(),event_kind:"user_input_pending".into(),created_at_micros:1,delivery_claimed:false }]
            })));

            let request=AgentRequestResult::Available {event_id:7,work_id:"root".into(),method:"mcpServer/elicitation/request".into(),request_json:decodex_protocol::AgentRequestText::new(serde_json::json!({"mode":"form","serverName":"codex_apps","message":"Allow this tool?","requestedSchema":{"type":"object","properties":{}},"_meta":{"persist":["session"],"codex_approval_kind":"mcp_tool_call","tool_name":"calendar.create","connector_name":"Calendar","connector_id":"calendar","link_id":"work","tool_params_display":[{"name":"subject","display_name":"Subject","value":"Design review"}]}}).to_string()).unwrap()};

            s.prepare_mcp_inputs(&request,cx);

            s.request=Some(request);

            s.submit_mcp_form_with_scope(7,Some("always"),cx);

            assert_eq!(s.feedback.message,"This persistence scope was not offered");

            s.feedback.clear();
            s.submit_mcp_form_with_scope(6,Some("session"),cx);

            assert!(s.feedback.is_empty());
            assert!(s.submission.command.is_none());
        });
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(1_200.0)));
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("mcp-submit").is_some(), "form submit must render");
		assert!(visual.debug_bounds("mcp-cancel").is_some());
		assert!(visual.debug_bounds("mcp-decline").is_none());
		assert!(visual.debug_bounds("mcp-account-context").is_none());
		let details = visual.debug_bounds("mcp-request-details").expect("tool details");
		visual.simulate_click(details.center(), gpui::Modifiers::default());
		visual.update(|window, cx| window.draw(cx).clear(cx));
		assert!(visual.debug_bounds("mcp-account-context").is_some());
		assert!(visual.debug_bounds("mcp-persist-always").is_none());

		assert!(visual.debug_bounds("mcp-persist-session").is_none());
		let more = visual.debug_bounds("mcp-more-options").expect("permission options");
		visual.simulate_click(more.center(), gpui::Modifiers::default());
		visual.update(|window, cx| window.draw(cx).clear(cx));
		let bounds = visual.debug_bounds("mcp-persist-session").expect("offered session action");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.feedback.message, "No service profile is configured.");
			assert!(s.submission.command.is_none());
		});

		for schema in [
			Value::Null,
			serde_json::json!(true),
			serde_json::json!({"type":"object","properties":{"template":{"type":"string","oneOf":[{"const":"a","title":"A","x-openai-preview":{"src":"fixture"}}]}}}),
		] {
			surface.update(visual, |s, cx| {
				let request = AgentRequestResult::Available {
					event_id: 7,
					work_id: "root".into(),
					method: "mcpServer/elicitation/request".into(),
					request_json: decodex_protocol::AgentRequestText::new(
						serde_json::json!({"mode":"openaiForm","requestedSchema":schema})
							.to_string(),
					)
					.unwrap(),
				};

				s.prepare_mcp_inputs(&request, cx);

				s.request = Some(request);

				s.submit_mcp_form(7, cx);

				assert_eq!(
					s.feedback.message,
					if schema.is_object() {
						"Unsupported choice constraints"
					} else {
						"This form schema is not supported."
					}
				);
				assert!(s.submission.command.is_none());

				cx.notify();
			});

			visual.update(crate::ui_frame::draw_current_frame);

			assert!(visual.debug_bounds("mcp-submit").is_none());
			assert!(visual.debug_bounds("mcp-decline").is_some());
			assert!(visual.debug_bounds("mcp-cancel").is_some());
		}
	}

	#[gpui::test]
	fn url_open_requires_separate_explicit_confirmation(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
            s.apply_result(Ok(AgentSnapshotResult::Available(AgentSnapshotDto {
		agents: Default::default(),
		assistant: Default::default(),
		guardian: Default::default(),
		portfolio: Default::default(),
                archived_work_ids: Default::default(),
                agent_slots: Default::default(),
                conversation_titles: Default::default(),
                context_references: vec![],
connection_initializing: false,
                runtime_source: None,
                projects: vec![], dependencies: vec![],
                work_items: vec![AgentWorkItemDto {
                    id:"root".into(), parent_goal_id:None, kind:AgentWorkKindDto::Goal,
                    title:"Agent".into(),codex_thread_id:Some("thread".into()),active_turn_id:None,
                    dispatch_state:AgentDispatchStateDto::Idle,status:AgentWorkStatusDto::Open,
                    next_check_at_micros:None,created_at_micros:1,updated_at_micros:1
                }],
                pending_events:vec![AgentPendingEventDto { id:7, source_event_id:"approval".into(), work_item_id:"root".into(),event_kind:"user_input_pending".into(),created_at_micros:1,delivery_claimed:false }]
            })));

            let request=AgentRequestResult::Available {event_id:7,work_id:"root".into(),method:"mcpServer/elicitation/request".into(),request_json:decodex_protocol::AgentRequestText::new(serde_json::json!({"mode":"url","serverName":"test","message":"Sign in","url":"https://example.test/verify","elicitationId":"verification"}).to_string()).unwrap()};

            s.prepare_mcp_inputs(&request,cx);

            s.request=Some(request);

            cx.notify();
        });
		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(1_180.0), gpui::px(1_200.0)));
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("mcp-confirm-url").is_none());

		let bounds = visual.debug_bounds("mcp-open-url").expect("verification link");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		surface.update(visual, |s, _| {
			assert_eq!(s.mcp_url_opened, Some((7, "https://example.test/verify".into())));
			assert!(s.submission.command.is_none());
			assert!(s.feedback.is_empty());
		});
		visual.update(crate::ui_frame::draw_current_frame);

		let bounds =
			visual.debug_bounds("mcp-confirm-url").expect("explicit confirmation after open");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());

		surface.update(visual, |s, cx| {
			assert_eq!(s.feedback.message, "No service profile is configured.");

			let request = AgentRequestResult::Available {
				event_id: 8,
				work_id: "root".into(),
				method: "mcpServer/elicitation/request".into(),
				request_json: decodex_protocol::AgentRequestText::new(
					serde_json::json!({"mode":"url","url":"file:///tmp/private"}).to_string(),
				)
				.unwrap(),
			};

			s.prepare_mcp_inputs(&request, cx);

			s.request = Some(request);
			s.snapshot.as_mut().unwrap().pending_events[0].id = 8;

			assert!(s.mcp_url_opened.is_none());

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("mcp-open-url").is_none());
		assert!(visual.debug_bounds("mcp-confirm-url").is_none());
	}

	#[gpui::test]
	fn form_defaults_are_not_answers_and_same_request_keeps_drafts(cx: &mut gpui::TestAppContext) {
		for mode in ["form", "openai/form", "openaiForm"] {
			let surface = cx.new(AgentSurface::new);

			surface.update(cx,|s,cx| {
            let request=AgentRequestResult::Available {event_id:7,work_id:"agent".into(),method:"mcpServer/elicitation/request".into(),request_json:decodex_protocol::AgentRequestText::new(serde_json::json!({"mode":mode,"requestedSchema":{"type":"object","properties":{"agree":{"type":"boolean","default":true},"name":{"type":"string"}},"required":["agree","name"]}}).to_string()).unwrap()};

            s.prepare_mcp_inputs(&request,cx);s.request=Some(request.clone());s.selected=Some("agent".into());
            s.submit_mcp_form(7,cx);

            assert!(s.feedback.contains("required"));assert!(s.submission.command.is_none());
            assert_eq!(s.feedback.tone, crate::ui_feedback::Tone::Warning);

            s.mcp_answers.insert("agree".into(),serde_json::json!(false));
            s.mcp_inputs["name"].update(cx,|input,cx|input.set_content("My name",cx));
            s.prepare_mcp_inputs(&request,cx);

            assert_eq!(s.mcp_inputs["name"].read(cx).content(),"My name");
            assert_eq!(s.mcp_answers["agree"],false);

            s.submit_mcp_form(6,cx);assert!(s.submission.command.is_none());
            s.submit_mcp_form(7,cx);assert_eq!(s.feedback.message,"No service profile is configured.");

            assert_eq!(s.feedback.tone, crate::ui_feedback::Tone::Error);
        });
		}
	}
}
