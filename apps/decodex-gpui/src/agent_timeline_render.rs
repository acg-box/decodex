//! Render native history without converting provider IDs into local inbox IDs.
use gpui::{AnyElement, Div, KeyDownEvent, Role, StatefulInteractiveElement as _};

use crate::{
	shell::{
		agent_surface,
		agent_surface::{
			native_timeline::{
				self, AgentHistoryResult, AgentSurface, AgentTimelineContent, AgentTimelineEntry,
				AgentWorkItemDto, Context, FluentBuilder, InteractiveElement, IntoElement,
				ParentElement, SharedString, Styled, markdown,
			},
			response_metrics::ResponseMetrics,
			text_reveal::StreamingText,
			voice::history::{VoiceBlock, handoff},
		},
		workspace_symbols,
	},
	ui_theme::{HOVER_FILL, TEXT_MUTED, USER_MESSAGE_ACTION_SIZE, foreground},
};
use decodex_protocol::{
	AgentActivityDto, AgentCollaborationDto, AgentLiveMessageDto, AgentLiveMessageKind,
	AgentTimelineAttachment, NativeAgentDto,
};

fn output_key(work: &AgentWorkItemDto, turn: &str, item: &str) -> String {
	serde_json::json!(["output", work.id, work.codex_thread_id, turn, item]).to_string()
}

fn speaker(text: impl Into<SharedString>) -> Div {
	gpui::div()
		.text_size(gpui::px(foreground::METRIC_SIZE))
		.line_height(gpui::px(crate::ui_theme::CAPTION_LINE_HEIGHT))
		.font_weight(gpui::FontWeight::MEDIUM)
		.text_color(gpui::rgb(foreground::SECONDARY))
		.child(text.into())
}

/// Use source identities for names; keep unknown tool names available verbatim.
pub(in crate::shell::agent_surface) fn activity_label(
	activity: &AgentActivityDto,
	collaboration: Option<&AgentCollaborationDto>,
	agents: &[NativeAgentDto],
) -> String {
	if let Some(call) = collaboration {
		let target = match call.receiver_thread_ids.as_slice() {
			[thread] => agents
				.iter()
				.find(|agent| &agent.thread_id == thread)
				.map(|agent| agent.title.clone()),
			threads if threads.len() > 1 => Some(format!("{} agents", threads.len())),
			_ => None,
		};
		let target = target.unwrap_or_else(|| {
			if activity.kind == "subAgentActivity" && !activity.detail.is_empty() {
				activity.detail.rsplit('/').next().unwrap_or("Agent").replace('_', " ")
			} else {
				"agents".into()
			}
		});
		let label = match call.tool.as_str() {
			"wait" => Some(format!("Wait for {target}")),
			"spawnAgent" => Some(format!("Start {target}")),
			"subAgentActivity/started" => Some(format!("Started {target}")),
			"sendInput" => Some(format!("Message {target}")),
			"subAgentActivity/interacted" => Some(format!("Sent message to {target}")),
			"resumeAgent" => Some(format!("Resume {target}")),
			"closeAgent" => Some(format!("Stop {target}")),
			"subAgentActivity/interrupted" => Some(format!("Stopped {target}")),
			"subAgentActivity/completed" => Some(format!("{target} · Turn complete")),
			_ => None,
		};
		if let Some(label) = label {
			return label;
		}
	}
	if matches!(activity.kind.as_str(), "dynamicToolCall" | "mcpToolCall")
		&& !activity.detail.is_empty()
	{
		// Keep authentication errors and other actionable source labels visible.
		if activity.label != "Using tool" {
			return activity.label.clone();
		}
		let tool = activity.detail.rsplit(" · ").next().unwrap_or(&activity.detail);
		return match tool {
			"agent_list_work" => "Checking tasks",
			"agent_read_work" => "Reading task history",
			"agent_create_work" => "Creating task",
			"agent_continue_worker" => "Continuing agent",
			"agent_disposition" => "Recording task assessment",
			"agent_resolve_goal" => "Recording goal completion",
			"agent_resolve_decision" => "Recording decision",
			"agent_add_dependency" => "Connecting tasks",
			"agent_create_goal" => "Creating goal",
			"agent_create_manager" => "Creating coordinator",
			_ => return activity.detail.clone(),
		}
		.into();
	}
	activity.label.clone()
}

impl AgentSurface {
	pub(in super::super) fn native_live_rows(&self, work: &AgentWorkItemDto) -> Vec<AnyElement> {
		let saved_live = self
			.history
			.as_ref()
			.filter(|(owner, _)| owner == &work.id)
			.and_then(|(_, history)| match history {
				AgentHistoryResult::Available { live, .. } => Some(live.as_slice()),
				_ => None,
			})
			.unwrap_or_default();
		let live = self.streamed_output(work).unwrap_or(saved_live);
		let mut rows = Vec::new();

		for message in live {
			if self.timeline.native.entries.iter().any(|entry| match &entry.content {
				AgentTimelineContent::Item { turn_id, item_id, .. } =>
					turn_id == &message.turn_id && item_id == &message.item_id,
				AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } =>
					turn_id == &message.turn_id,
				_ => false,
			}) {
				continue;
			}

			let key = output_key(work, &message.turn_id, &message.item_id);
			self.timeline.native.arrivals.present_live(&message.item_id);
			self.timeline.native.completions.present_live(&message.item_id);
			let streamed =
				StreamingText::output(markdown::response_text(&message.text), key.clone(), false)
					.settled(self.timeline.native.completions.output(&message.item_id))
					.into_any_element();
			let body = if message.kind == AgentLiveMessageKind::AgentMessage {
				let entry = self.native_message_entry(
					work,
					&message.turn_id,
					&message.text,
					"agentMessage",
				);
				agent_surface::history_entry_presented(
					&entry,
					&key,
					None,
					Some(streamed),
					false,
					Some(self.timeline.native.completions.output(&message.item_id)),
					None,
				)
			} else {
				gpui::div()
					.when(message.kind == AgentLiveMessageKind::Plan, |row| {
						row.child(agent_surface::muted("Proposed plan"))
					})
					.child(streamed)
			};
			rows.push(
				gpui::div()
					.id(SharedString::from(key))
					.w_full()
					.child(
						body.debug_selector({
							let selector = match message.kind {
								AgentLiveMessageKind::ReasoningSummary =>
									"native-live-reasoning-summary",
								AgentLiveMessageKind::Plan => "native-live-plan",
								AgentLiveMessageKind::AgentMessage => "native-live-output",
							};
							move || selector.into()
						})
						.children(message.truncated.then(|| {
							agent_surface::muted(
								"Partial output shortened; waiting for saved result.",
							)
						})),
					)
					.into_any_element(),
			);
		}

		rows
	}

	pub(super) fn native_summary_exchanges(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let mut body = gpui::div()
			.w_full()
			.min_w_0()
			.flex()
			.flex_col()
			.gap(gpui::px(crate::ui_theme::MESSAGE_GAP));
		let mut current_turn = None;
		let mut exchange: Option<gpui::Stateful<Div>> = None;
		for item in &self.timeline.native.summary {
			let Some(row) = self.native_summary_row(work, item, cx) else { continue };
			let AgentTimelineContent::Item { turn_id, item_id, .. } = item else { continue };
			if current_turn != Some(turn_id) {
				if let Some(previous) = exchange.take() {
					body = body.child(previous);
				}
				let identity = serde_json::json!([
					"summary-exchange",
					work.id,
					work.codex_thread_id,
					turn_id,
					item_id
				])
				.to_string();
				let selector = format!("native-summary-exchange-{turn_id}-{item_id}");
				exchange = Some(
					gpui::div()
						.id(SharedString::from(identity))
						.debug_selector(move || selector.clone())
						.role(Role::Group)
						.aria_label("Conversation turn")
						.w_full()
						.min_w_0()
						.flex()
						.flex_col()
						.gap(gpui::px(crate::ui_theme::MESSAGE_GAP)),
				);
				current_turn = Some(turn_id);
			}
			exchange = exchange.map(|group| group.child(row));
		}
		body.children(exchange).into_any_element()
	}

	pub(super) fn native_summary_row(
		&self,
		work: &AgentWorkItemDto,
		item: &AgentTimelineContent,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let AgentTimelineContent::Item {
			turn_id, item_id, kind, text, truncated, attachments, ..
		} = item
		else {
			return None;
		};
		if matches!(kind.as_str(), "userMessage" | "agentMessage")
			&& text.trim().is_empty()
			&& !truncated
			&& attachments.is_empty()
		{
			return None;
		}
		let identity =
			serde_json::json!(["summary", work.id, work.codex_thread_id, turn_id, item_id])
				.to_string();
		if kind == "userMessage"
			&& attachments.is_empty()
			&& let Some(text) = handoff(text)
		{
			return Some(
				VoiceBlock {
					viewport: Some(self.voice_history_viewport(cx)),
					key: identity,
					title: "Voice conversation".into(),
					text,
				}
				.into_any_element(),
			);
		}
		let user = kind == "userMessage";
		let mut body = gpui::div()
			.min_w_0()
			.flex()
			.flex_col()
			.gap(gpui::px(crate::ui_theme::METADATA_GAP))
			.child(markdown::render(text, &identity));

		for attachment in attachments {
			body = body.child(self.native_attachment(work, turn_id, item_id, attachment, cx));
		}

		if *truncated {
			body = body
				.child(agent_surface::muted("Some content was omitted from this history preview."));
		}
		if kind == "agentMessage" && !text.is_empty() {
			body = body.child(markdown::response_copy_button(
				&format!("copy-{identity}"),
				"Copy response",
				text.clone(),
			));
		}
		if kind == "agentMessage"
			&& let Some(action) = self.voice_read_action(&work.id, &identity, text, *truncated, cx)
		{
			body = body.child(action);
		}

		let message = self.native_message_entry(work, turn_id, text, kind);
		let selector = format!("native-summary-content-{turn_id}-{item_id}");
		let body = agent_surface::history_entry_presented(
			&message,
			&identity,
			None,
			Some(body.into_any_element()),
			false,
			None,
			None,
		)
		.debug_selector(move || selector.clone());
		Some(
			gpui::div()
				.id(SharedString::from(format!("summary-{identity}")))
				.debug_selector(|| "native-summary-message".into())
				.role(Role::Group)
				.aria_label(if user { "You" } else { "Assistant" })
				.w_full()
				.min_w_0()
				.flex()
				.child(body)
				.into_any_element(),
		)
	}

	pub(super) fn native_timeline_row(
		&self,
		work: &AgentWorkItemDto,
		entry: &AgentTimelineEntry,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let identity =
			serde_json::json!([work.id, work.codex_thread_id, native_timeline::key(entry)])
				.to_string();
		let content = self.native_timeline_content(work, entry, &identity, false, cx);
		self.native_timeline_row_with_content(work, entry, content, cx)
	}

	pub(super) fn native_timeline_row_with_content(
		&self,
		work: &AgentWorkItemDto,
		entry: &AgentTimelineEntry,
		content: AnyElement,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let identity =
			serde_json::json!([work.id, work.codex_thread_id, native_timeline::key(entry)])
				.to_string();
		let selector = format!("native-history-{identity}");
		let content = self.timeline.native.arrivals.wrap(entry, content);
		let content = self.anchored_native_history_entry(work, entry, content);
		let content = self.native_scroll_row(work, entry, content, cx);

		gpui::div()
			.debug_selector(move || selector)
			.id(SharedString::from(match &entry.content {
				AgentTimelineContent::Item { kind, turn_id, item_id, .. }
					if kind == "agentMessage" =>
					output_key(work, turn_id, item_id),
				_ => identity.clone(),
			}))
			.w_full()
			.min_w_0()
			.child(content)
			.into_any_element()
	}

	pub(super) fn native_offscreen_height(
		&self,
		work: &AgentWorkItemDto,
		entry: &AgentTimelineEntry,
	) -> Option<f32> {
		let can_window = match &entry.content {
			AgentTimelineContent::Item {
				turn_id,
				item_id,
				attachments,
				app_ui,
				text,
				activity,
				..
			} if attachments.is_empty()
				&& (activity.is_none() || self.activity_detail.value.is_none())
				&& !app_ui
				&& !text.contains("![")
				&& work.active_turn_id.as_deref() != Some(turn_id)
				&& !self.timeline.native.weather.contains_key(turn_id)
				&& self.native_live_message(work, turn_id, item_id).is_none() =>
				true,
			AgentTimelineContent::TurnBoundary { completed: true, turn_id, .. }
				if work.active_turn_id.as_deref() != Some(turn_id) =>
				true,
			_ => false,
		};

		can_window
			.then(|| {
				self.timeline.scroll.get(&work.id).and_then(|scroll| {
					self.timeline.native.viewport.offscreen_height(
						entry,
						scroll.offset().y.into(),
						scroll.bounds().size.height.into(),
					)
				})
			})
			.flatten()
	}

	fn native_live_message(
		&self,
		work: &AgentWorkItemDto,
		turn: &str,
		item: &str,
	) -> Option<&AgentLiveMessageDto> {
		let (_, AgentHistoryResult::Available { live, .. }) =
			self.history.as_ref().filter(|(id, _)| id == &work.id)?
		else {
			return None;
		};
		let message = self
			.streamed_output(work)
			.unwrap_or(live.as_slice())
			.iter()
			.find(|message| message.turn_id == turn && message.item_id == item)?;

		// Historical rows have no live draft. Do not scan the entire timeline
		// for every such row on every scroll frame.
		if self.timeline.native.entries.iter().any(|entry| {
			matches!(&entry.content,
			AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } if turn_id == turn)
		}) {
			return None;
		}

		Some(message)
	}

	fn native_message_entry(
		&self,
		work: &AgentWorkItemDto,
		turn_id: &str,
		text: &str,
		kind: &str,
	) -> decodex_protocol::AgentHistoryEntryDto {
		let saved =
			self.history.as_ref().filter(|(id, _)| id == &work.id).and_then(|(_, history)| {
				match history {
					AgentHistoryResult::Available { entries, .. } => entries.iter().find(|entry| {
						matches!(
							(kind, entry.kind.as_str()),
							("userMessage", "user" | "instruction") | ("agentMessage", "assistant")
						) && entry.turn_id.as_deref() == Some(turn_id)
							&& entry.text == text
					}),
					_ => None,
				}
			});
		let mut message =
			saved.cloned().unwrap_or_else(|| decodex_protocol::AgentHistoryEntryDto {
				native_source: None,
				id: 0,
				kind: match kind {
					"userMessage" => "user",
					"agentInput" => "instruction",
					_ => "assistant",
				}
				.into(),
				text: text.into(),
				created_at_micros: 0,
				duration_ms: None,
				usage: None,
				activity: None,
				receipt: None,
				turn_id: Some(turn_id.into()),
				weather: Vec::new(),
			});

		message.text = text.into();

		if kind == "agentMessage"
			&& let Some(forecasts) = self.timeline.native.weather.get(turn_id)
		{
			message.weather = forecasts
				.iter()
				.filter(|f| {
					["weather", "forecast"].iter().any(|kind| {
						text.contains(&format!("\u{e200}{kind}\u{e202}{}\u{e201}", f.reference))
					})
				})
				.cloned()
				.collect();
		}

		message
	}

	pub(super) fn native_timeline_content(
		&self,
		work: &AgentWorkItemDto,
		entry: &AgentTimelineEntry,
		identity: &str,
		archived: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let row = gpui::div().w_full().min_w_0().flex().flex_col().gap_1();

		match &entry.content {
			content @ AgentTimelineContent::Item { .. } =>
				self.native_item_content(work, content, identity, archived, cx),
			AgentTimelineContent::Speech { role, text, truncated, .. } => row
				.child(speaker(if role == "user" { "You" } else { "Assistant" }))
				.child(markdown::render(text, identity))
				.when(*truncated, |r| r.child(agent_surface::muted("Voice transcript shortened.")))
				.into_any_element(),
			AgentTimelineContent::VoiceBoundary { kind, outcome, .. } => row
				.child(agent_surface::muted(if kind == "realtimeSessionStarted" {
					"Voice conversation started"
				} else if outcome.as_deref() == Some("failed") {
					"Voice conversation failed"
				} else {
					"Voice conversation ended"
				}))
				.into_any_element(),
			AgentTimelineContent::TurnBoundary { completed, turn_id, status, error, .. } => {
				let has_reply = self.timeline.native.entries.iter().any(|entry| {
					matches!(&entry.content, AgentTimelineContent::Item {
turn_id: turn, kind, .. } if turn == turn_id && kind == "agentMessage")
				});

				row.when(*completed && !has_reply, |row| {
					row.child(self.native_turn_metrics(&entry.content, identity, cx))
				})
				.when(
					*completed && matches!(status.as_deref(), Some("interrupted" | "failed")),
					|row| {
						row.child(crate::ui_feedback::execution_icon(
							format!("execution-status-{identity}"),
							status.as_deref().unwrap_or_default(),
						))
					},
				)
				.children(error.as_ref().map(|error| gpui::div().child(error.message.clone())))
				.into_any_element()
			},
			content @ AgentTimelineContent::Promotion { .. } =>
				self.native_promotion(work, content, identity, cx),
		}
	}

	fn native_turn_metrics(
		&self,
		boundary: &AgentTimelineContent,
		identity: &str,
		_cx: &mut Context<Self>,
	) -> AnyElement {
		let AgentTimelineContent::TurnBoundary { duration_ms, status, usage, .. } = boundary else {
			return gpui::div().into_any_element();
		};

		ResponseMetrics {
			key: identity.into(),
			duration_ms: *duration_ms,
			status: status.clone(),
			usage: usage.clone(),
		}
		.into_any_element()
	}

	fn native_reply_metrics(
		&self,
		turn: &str,
		item: &str,
		identity: &str,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let last_reply =
			self.timeline.native.entries.iter().rev().find_map(|entry| match &entry.content {
				AgentTimelineContent::Item { turn_id, item_id, kind, .. }
					if turn_id == turn && kind == "agentMessage" =>
					Some(item_id.as_str()),
				_ => None,
			});
		if last_reply != Some(item) {
			return None;
		}
		let boundary = self.timeline.native.entries.iter().find(|entry|
			matches!(&entry.content, AgentTimelineContent::TurnBoundary { turn_id, completed: true, .. } if turn_id == turn))?;
		Some(self.native_turn_metrics(&boundary.content, identity, cx))
	}

	fn native_item_content(
		&self,
		work: &AgentWorkItemDto,
		content: &AgentTimelineContent,
		identity: &str,
		archived: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let AgentTimelineContent::Item {
			app_ui,
			text,
			kind,
			truncated,
			activity,
			turn_id,
			item_id,
			phase,
			attachments,
			..
		} = content
		else {
			unreachable!("item renderer")
		};
		let draft = if matches!(kind.as_str(), "agentMessage" | "plan" | "reasoning") {
			self.native_live_message(work, turn_id, item_id)
		} else {
			None
		};
		let (text, truncated) = draft.map_or((text.as_str(), *truncated), |message| {
			(message.text.as_str(), message.truncated)
		});

		if kind == "agentMessage" && !truncated && attachments.is_empty() {
			if let Some(panel) = self.async_question_item_text(work, item_id) {
				self.timeline.native.completions.output(item_id).set(true);
				return panel;
			}
			if text.is_empty() {
				self.timeline.native.completions.output(item_id).set(true);
				return gpui::div().into_any_element();
			}
		}

		if kind == "userMessage"
			&& attachments.is_empty()
			&& let Some(text) = handoff(text)
		{
			return VoiceBlock {
				viewport: Some(self.voice_history_viewport(cx)),
				key: identity.into(),
				title: "Voice conversation".into(),
				text,
			}
			.into_any_element();
		}

		if archived
			&& (kind == "reasoning"
				|| (kind == "agentMessage" && phase.as_deref() == Some("commentary")))
			&& attachments.is_empty()
		{
			let body = process_text(kind, text, identity, truncated);

			return if kind == "agentMessage"
				&& let Some(action) = self.voice_read_action(
					&work.id,
					identity,
					text,
					truncated || draft.is_some(),
					cx,
				) {
				gpui::div().child(body).child(action).into_any_element()
			} else {
				body
			};
		}
		if matches!(kind.as_str(), "userMessage" | "agentInput" | "agentMessage") {
			let message = self.native_message_entry(work, turn_id, text, kind);
			let mut body = gpui::div().debug_selector(|| "native-promotion-content".into());

			for attachment in attachments {
				body = body.child(self.native_attachment(work, turn_id, item_id, attachment, cx));
			}

			let metrics =
				(!archived && kind == "agentMessage" && phase.as_deref() != Some("commentary"))
					.then(|| self.native_reply_metrics(turn_id, item_id, identity, cx))
					.flatten();
			let complete = metrics.is_some();
			let streamed = (kind == "agentMessage").then(|| {
				StreamingText::output(
					markdown::response_text(text),
					output_key(work, turn_id, item_id),
					draft.is_none(),
				)
				.settled(self.timeline.native.completions.output(item_id))
				.into_any_element()
			});
			body = body.child(agent_surface::history_entry_presented(
				&message,
				identity,
				metrics,
				streamed,
				complete,
				(kind == "agentMessage").then(|| self.timeline.native.completions.output(item_id)),
				(kind == "userMessage")
					.then(|| self.native_prompt_action(work, content, identity, cx))
					.flatten(),
			));

			if kind == "agentMessage"
				&& let Some(action) = self.voice_read_action(
					&work.id,
					identity,
					text,
					truncated || draft.is_some(),
					cx,
				) {
				body = body.child(action);
			}
			if truncated {
				body = body.child(agent_surface::muted(
					"Some content was omitted from this history preview.",
				));
			}
			return body.into_any_element();
		}
		if activity.is_some() && !*app_ui && attachments.is_empty() {
			return self.native_activity_content(work, content, identity, cx);
		}

		self.native_item_text(work, content, identity, text, truncated, cx)
	}

	fn native_item_text(
		&self,
		work: &AgentWorkItemDto,
		content: &AgentTimelineContent,
		identity: &str,
		text: &str,
		truncated: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let AgentTimelineContent::Item { kind, turn_id, item_id, attachments, activity, .. } =
			content
		else {
			unreachable!("item text renderer")
		};
		let row = gpui::div().w_full().min_w_0().flex().flex_col().gap_1();
		let label = match kind.as_str() {
			"userMessage" => "You",
			"agentMessage" => "Assistant",
			"plan" => "Proposed plan",
			"reasoning" => "Thinking",
			"sleep" => "Wait",
			"functionCallOutput" => "Tool result",
			_ => kind,
		};
		let mut row = row.when(kind != "reasoning", |row| {
			if kind == "sleep" {
				// Older cached pages have no activity projection for this native item.
				row.child(
					gpui::div()
						.flex()
						.items_center()
						.gap(gpui::px(8.))
						.py(gpui::px(5.))
						.text_size(gpui::px(12.))
						.line_height(gpui::px(18.))
						.text_color(gpui::rgb(TEXT_MUTED))
						.child(crate::ui_feedback::execution_icon(
							format!("wait-{identity}"),
							"recorded",
						))
						.child(label.to_owned()),
				)
			} else {
				row.child(speaker(label.to_owned()))
			}
		});

		for attachment in attachments {
			row = row.child(self.native_attachment(work, turn_id, item_id, attachment, cx));
		}

		if !text.is_empty() {
			let selector = match kind.as_str() {
				"plan" => "native-plan-content",
				"reasoning" => "native-reasoning-summary",
				_ => "native-promotion-content",
			};

			row = row.child(
				gpui::div()
					.debug_selector(move || selector.into())
					.child(markdown::render(text, identity)),
			);
		}
		if truncated {
			row = row
				.child(agent_surface::muted("Some content was omitted from this history preview."));
		}

		if let Some(activity) = activity {
			return self.detail_row(
				work,
				activity,
				None,
				row.child(
					gpui::div()
						.flex()
						.items_center()
						.gap(gpui::px(8.))
						.child(crate::ui_feedback::execution_icon(
							format!("execution-status-{identity}"),
							&activity.status,
						))
						.child(activity.label.clone()),
				),
				cx,
			);
		}

		row.into_any_element()
	}

	fn native_prompt_action(
		&self,
		work: &AgentWorkItemDto,
		content: &AgentTimelineContent,
		identity: &str,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		if self.native_agents.selected.is_some() {
			return None;
		}
		let AgentTimelineContent::Item { turn_id, item_id, .. } = content else {
			unreachable!("prompt renderer")
		};
		let thread = work.codex_thread_id.as_ref()?;
		let (owner, thread, turn, item) =
			(work.id.clone(), thread.clone(), turn_id.clone(), item_id.clone());
		let group: SharedString = format!("history-input-{identity}").into();
		let keyboard_source = (owner.clone(), thread.clone(), turn.clone(), item.clone());
		let edit = gpui::div()
			.debug_selector(|| "edit-message-action".into())
			.id(SharedString::from(format!("review-prompt-{identity}")))
			.role(Role::Button)
			.tab_index(0)
			.aria_label("Edit message")
			.size(gpui::px(USER_MESSAGE_ACTION_SIZE))
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(6.))
			.opacity(0.)
			.group_hover(group.clone(), |style| style.opacity(1.))
			.focus(|style| style.opacity(1.))
			.cursor_pointer()
			.hover(|style| style.bg(gpui::rgba(HOVER_FILL)))
			.on_click(cx.listener(move |s, _, _, cx| {
				s.review_prompt(&owner, &thread, &turn, &item, cx);
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					let (owner, thread, turn, item) = &keyboard_source;

					s.review_prompt(owner, thread, turn, item, cx);
					cx.stop_propagation();
				}
			}))
			.child(
				gpui::svg()
					.data(br#"<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14"><path d="M2 9L9 2L12 5L5 12L1 13ZM7 4L10 7" fill="none" stroke="white" stroke-width="1.1"/></svg>"#)
					.text_color(gpui::rgb(TEXT_MUTED))
					.size(gpui::px(14.)),
			);

		Some(edit.into_any_element())
	}

	fn native_activity_content(
		&self,
		work: &AgentWorkItemDto,
		content: &AgentTimelineContent,
		identity: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let AgentTimelineContent::Item {
			activity: Some(activity),
			kind,
			turn_id,
			item_id,
			collaboration,
			..
		} = content
		else {
			unreachable!("activity renderer")
		};
		let label = activity_label(
			activity,
			collaboration.as_ref(),
			self.native_agents.lists.get(&work.id).map(Vec::as_slice).unwrap_or_default(),
		);
		let detail_key = crate::shell::agent_surface::detail::supports_activity_detail(kind)
			.then(|| self.activity_detail_key(&(work.id.clone(), turn_id.clone(), item_id.clone())))
			.flatten();
		let expanded = detail_key.as_ref().is_some_and(|key| {
			self.activity_detail.value.as_ref().is_some_and(|(current, _)| current == key)
		});
		let row = gpui::div()
			.w_full()
			.min_w_0()
			.flex()
			.items_center()
			.gap(gpui::px(8.))
			.py(gpui::px(5.))
			.rounded(gpui::px(6.))
			.text_size(gpui::px(12.))
			.line_height(gpui::px(18.))
			.text_color(gpui::rgb(TEXT_MUTED))
			.child(crate::ui_feedback::execution_icon(
				format!("execution-status-{identity}"),
				&activity.status,
			))
			.child(gpui::div().flex_1().min_w_0().text_ellipsis().child(label))
			.when_some(
				(kind == "sleep").then(|| super::waits::timing(activity)).flatten(),
				|d, time| d.child(time),
			)
			.when_some(activity.plugin_id.clone(), |d, plugin| {
				d.child(
					gpui::div()
						.max_w(gpui::px(140.))
						.text_ellipsis()
						.child(format!("Plugin: {plugin}")),
				)
			})
			.when(activity.read_only_hint == Some(true), |d| d.child("Read-only hint"))
			.when_some(activity.duration_ms.filter(|ms| *ms > 0 && kind != "sleep"), |d, ms| {
				d.child(if ms < 1_000 {
					format!("{ms}ms")
				} else {
					format!("{:.1}s", ms as f64 / 1_000.)
				})
			})
			.when(detail_key.is_some(), |row| {
				row.child(
					gpui::div()
						.debug_selector(|| "tool-chevron-bounds".into())
						.flex_none()
						.size(gpui::px(12.))
						.child(workspace_symbols::process_chevron(
							SharedString::from(format!("tool-chevron-{identity}")),
							expanded,
						)),
				)
			});

		self.detail_row(work, activity, collaboration.as_ref(), row, cx)
	}

	fn native_promotion(
		&self,
		work: &AgentWorkItemDto,
		content: &AgentTimelineContent,
		identity: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let AgentTimelineContent::Promotion {
			turn_id,
			agent_item_id,
			presentation,
			index,
			resolved,
			..
		} = content
		else {
			unreachable!("promotion renderer")
		};
		let row = gpui::div().w_full().min_w_0().flex().flex_col().gap_1();
		let target = resolved
			.as_ref()
			.map(|item| {
				(
					item.text.as_str(),
					item.truncated,
					item.activity.as_ref(),
					item.attachments.as_slice(),
				)
			})
			.or_else(|| {
				let mut matches =
					self.timeline.native.entries.iter().filter_map(|entry| match &entry.content {
						AgentTimelineContent::Item {
							turn_id: turn,
							item_id,
							text,
							truncated,
							activity,
							attachments,
							..
						} if turn == turn_id && item_id == agent_item_id => Some((
							text.as_str(),
							*truncated,
							activity.as_ref(),
							attachments.as_slice(),
						)),
						_ => None,
					});
				let first = matches.next()?;

				matches.next().is_none().then_some(first)
			});
		let mut row = row.child(agent_surface::muted("Shared during voice conversation"));

		match target {
			Some((text, truncated, activity, attachments)) => {
				for attachment in attachments {
					row = row.child(self.native_attachment(
						work,
						turn_id,
						agent_item_id,
						attachment,
						cx,
					));
				}

				if presentation == "inlineVisualization" {
					row = row.child(agent_surface::muted(&format!(
						"Visualization {} is in the referenced message.",
						u64::from(index.unwrap_or_default()) + 1
					)));
				} else if !text.is_empty() {
					row = row.child(
						gpui::div()
							.debug_selector(|| "native-promotion-content".into())
							.child(markdown::render(text, identity)),
					);
				}
				if truncated {
					row = row.child(agent_surface::muted("Shared content preview shortened."));
				}

				if let Some(activity) = activity {
					return self.detail_row(
						work,
						activity,
						None,
						row.child(activity.label.clone()),
						cx,
					);
				}
			},
			_ =>
				row = row.child(
					gpui::div().debug_selector(|| "native-promotion-unavailable".into()).child(
						agent_surface::muted(
							"Referenced result is temporarily unavailable. History refresh will retry.",
						),
					),
				),
		}

		row.into_any_element()
	}
}

fn process_text(kind: &str, text: &str, identity: &str, truncated: bool) -> AnyElement {
	gpui::div()
		.when(kind == "reasoning", |row| row.debug_selector(|| "native-reasoning-summary".into()))
		.child(markdown::render_process(text, identity))
		.when(truncated, |row| {
			row.child(agent_surface::muted("Some content was omitted from this history preview."))
		})
		.into_any_element()
}

/// Only expanded, completed process records use the guide gutter.
pub(super) fn process_indent(content: impl IntoElement) -> AnyElement {
	gpui::div()
		.debug_selector(|| "archived-process-content".into())
		.w_full()
		.min_w_0()
		.pl(gpui::px(8.))
		.child(
			gpui::div()
				.w_full()
				.min_w_0()
				.border_l_1()
				.border_color(gpui::rgba(0xffffff14))
				.pl(gpui::px(14.))
				.child(content),
		)
		.into_any_element()
}

pub(super) fn attachment_caption(attachment: &AgentTimelineAttachment) -> String {
	let kind = match attachment.kind.as_str() {
		"image" | "localImage" | "imageView" | "imageGeneration" | "inputImage" => "Image",
		"audio" | "localAudio" | "inputAudio" => "Audio",
		"skill" => "Skill",
		"mention" => "Mention",
		"resource" | "resource_link" => "Resource",
		_ => "Attachment",
	};

	if attachment.label == kind { kind.into() } else { format!("{kind} · {}", attachment.label) }
}

#[cfg(test)]
mod tests {
	use std::thread;

	use gpui::{self, AppContext};

	use crate::shell::agent_surface::native_timeline::{
		self, Binding, Timeline,
		render::{AgentSurface, AgentTimelineContent, AgentTimelineEntry, Context},
	};
	use decodex_protocol::AgentTimelinePage;

	#[test]
	fn activity_labels_name_agents_and_actions_without_masking_errors_or_unknown_tools() {
		let mut activity: decodex_protocol::AgentActivityDto =
			serde_json::from_value(serde_json::json!({
				"turn_id":"turn", "item_id":"item", "kind":"subAgentActivity", "status":"completed",
				"label":"Subagent completed a turn", "detail":"/root/reviewer", "duration_ms":null
			}))
			.unwrap();
		let mut call = decodex_protocol::AgentCollaborationDto {
			results: vec![],
			sender_thread_id: "parent".into(),
			receiver_thread_ids: vec!["child".into()],
			tool: "subAgentActivity/completed".into(),
			status: "completed".into(),
			prompt: String::new(),
		};
		let agents = vec![decodex_protocol::NativeAgentDto {
			thread_id: "child".into(),
			parent_thread_id: "parent".into(),
			title: "Mia".into(),
			task: String::new(),
			status: "idle".into(),
		}];
		assert_eq!(super::activity_label(&activity, Some(&call), &agents), "Mia · Turn complete");
		call.tool = "wait".into();
		assert_eq!(super::activity_label(&activity, Some(&call), &agents), "Wait for Mia");
		activity.kind = "dynamicToolCall".into();
		activity.label = "Using tool".into();
		activity.detail = "agent_list_work".into();
		assert_eq!(super::activity_label(&activity, None, &[]), "Checking tasks");
		activity.detail = "docs · search".into();
		assert_eq!(super::activity_label(&activity, None, &[]), "docs · search");
		activity.label = "Sign-in required".into();
		assert_eq!(super::activity_label(&activity, None, &[]), "Sign-in required");
	}

	#[gpui::test]
	fn native_weather_does_not_require_a_duplicate_local_message(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s.snapshot.as_ref().unwrap().work_items[0].clone();

			s.history = None;

			let forecast = decodex_protocol::WeatherForecast::parse(include_str!(
				"../examples/fixtures/singapore-weather.txt"
			))
			.unwrap();

			s.timeline.native.weather.insert("weather-turn".into(), vec![forecast.clone()]);

			for kind in ["weather", "forecast"] {
				let text = format!("Cloudy. \u{e200}{kind}\u{e202}{}\u{e201}", forecast.reference);

				assert_eq!(
					s.native_message_entry(&work, "weather-turn", &text, "agentMessage").weather,
					vec![forecast.clone()]
				);
				assert!(
					s.native_message_entry(&work, "different-turn", &text, "agentMessage")
						.weather
						.is_empty()
				);
				assert!(
					s.native_message_entry(&work, "weather-turn", &text, "userMessage")
						.weather
						.is_empty()
				);
			}
		});
	}

	#[gpui::test]
	fn native_message_metadata_preserves_the_provider_role(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s
				.snapshot
				.as_ref()
				.unwrap()
				.work_items
				.iter()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap()
				.clone();
			let mut user = s.native_message_entry(&work, "same-turn", "Echo", "userMessage");

			user.id = 91;
			user.kind = "user".into();

			let mut assistant = user.clone();

			assistant.id = 92;
			assistant.kind = "assistant".into();
			assistant.duration_ms = Some(500);

			for rows in
				[vec![user.clone(), assistant.clone()], vec![assistant.clone(), user.clone()]]
			{
				let (_, super::super::AgentHistoryResult::Available { entries, .. }) =
					s.history.as_mut().unwrap()
				else {
					panic!("fixture history")
				};

				*entries = rows;

				let reply = s.native_message_entry(&work, "same-turn", "Echo", "agentMessage");

				assert_eq!(reply.kind, "assistant");
				assert_eq!(reply.id, 92);
				assert_eq!(reply.duration_ms, Some(500));

				let prompt = s.native_message_entry(&work, "same-turn", "Echo", "userMessage");

				assert_eq!(prompt.kind, "user");
				assert_eq!(prompt.id, 91);
				assert_eq!(prompt.duration_ms, None);
			}

			let (_, super::super::AgentHistoryResult::Available { entries, .. }) =
				s.history.as_mut().unwrap()
			else {
				panic!("fixture history")
			};

			user.kind = "instruction".into();
			*entries = vec![user];

			let prompt = s.native_message_entry(&work, "same-turn", "Echo", "userMessage");

			assert_eq!(prompt.kind, "instruction");

			let reply = s.native_message_entry(&work, "same-turn", "Echo", "agentMessage");

			assert_eq!(reply.kind, "assistant");
			assert_eq!(reply.id, 0);
			assert_eq!(reply.text, "Echo");
		});
	}

	#[gpui::test]
	fn live_answer_keeps_its_bounds_when_saved_and_completed(cx: &mut gpui::TestAppContext) {
		use decodex_protocol::{AgentHistoryResult, AgentLiveMessageDto, AgentLiveMessageKind};
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));
		let text = "One received paragraph.\n\nAnother received paragraph.";
		let entry = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.graph_visible = false;
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();
			work.codex_thread_id = Some("native-thread".into());
			s.timeline.native.replace(
				Binding {
					work: work.id.clone(),
					thread: "native-thread".into(),
					account: "account".into(),
				},
				AgentTimelinePage {
					thread_id: "native-thread".into(),
					entries: vec![],
					next_cursor: None,
					weather: Default::default(),
					thread_token_total: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				},
			);
			let (_, AgentHistoryResult::Available { entries, live, .. }) =
				s.history.as_mut().unwrap()
			else {
				panic!("fixture history")
			};
			entries.clear();
			*live = vec![AgentLiveMessageDto {
				kind: AgentLiveMessageKind::AgentMessage,
				turn_id: "turn".into(),
				item_id: "message".into(),
				text: text.into(),
				truncated: false,
			}];
			let mut entry = rendered_entries()[1].clone();
			if let AgentTimelineContent::Item { text: saved, truncated, .. } = &mut entry.content {
				*saved = text.into();
				*truncated = false;
			}
			cx.notify();
			entry
		});
		visual.update(crate::ui_frame::draw_frame);
		std::thread::sleep(std::time::Duration::from_millis(750));
		visual.update(crate::ui_frame::draw_frame);
		// The text's final line was measured in that frame. Let its height settle
		// before comparing the live and saved representations of the same output.
		std::thread::sleep(std::time::Duration::from_millis(240));
		visual.update(crate::ui_frame::draw_frame);
		let live = visual.debug_bounds("native-live-output").unwrap();
		surface.update(visual, |s, cx| {
			s.timeline.native.entries.push(entry);
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		assert!(visual.debug_bounds("native-live-output").is_none());
		assert_eq!(
			visual.debug_bounds("native-promotion-content").unwrap(),
			live,
			"saving must not replay text or change the response footprint"
		);
		surface.update(visual, |s, cx| {
			let (_, AgentHistoryResult::Available { live, .. }) = s.history.as_mut().unwrap()
			else {
				panic!("fixture history")
			};
			live.clear();
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		assert_eq!(
			visual.debug_bounds("native-promotion-content").unwrap(),
			live,
			"the response footer must not change height on completion"
		);
	}

	#[gpui::test]
	fn native_answer_footer_appears_only_after_completion(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		let original = "Draft response\n\n$$\n\\frac{a}{b}";
		let selector = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("native-thread".into());

			let work = work.clone();
			let mut entry = rendered_entries()[1].clone();

			if let AgentTimelineContent::Item { text, truncated, .. } = &mut entry.content {
				*text = original.into();
				*truncated = false;
			}

			let identity =
				serde_json::json!([work.id, work.codex_thread_id, native_timeline::key(&entry)])
					.to_string();

			s.timeline.native.replace(
				Binding {
					work: work.id.clone(),
					thread: "native-thread".into(),
					account: "account".into(),
				},
				AgentTimelinePage {
					thread_id: "native-thread".into(),
					entries: vec![entry],
					next_cursor: None,
					weather: Default::default(),
					thread_token_total: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None,
				},
			);

			let (_, super::super::AgentHistoryResult::Available { entries, live, .. }) =
				s.history.as_mut().unwrap()
			else {
				panic!("fixture history")
			};

			entries.clear();

			*live = vec![decodex_protocol::AgentLiveMessageDto {
				kind: decodex_protocol::AgentLiveMessageKind::AgentMessage,
				turn_id: "turn".into(),
				item_id: "message".into(),
				text: original.into(),
				truncated: false,
			}];

			cx.notify();

			format!("copy-response-{identity}")
		});
		let selector = Box::leak(selector.into_boxed_str());

		for completed in [false, true] {
			if completed {
				surface.update(visual, |s, cx| {
					let (_, super::super::AgentHistoryResult::Available { live, .. }) =
						s.history.as_mut().unwrap()
					else {
						panic!("fixture history")
					};

					live.clear();
					s.timeline.native.entries.push(AgentTimelineEntry {
						position: 2,
						content: AgentTimelineContent::TurnBoundary {
							turn_id: "turn".into(),
							completed: true,
							status: Some("completed".into()),
							duration_ms: Some(1200),
							usage: None,
							usage_summary: None,
							error: None,
						},
					});
					cx.notify();
				});
			}

			visual.update(|window, cx| {
				crate::ui_frame::draw_frame(window, cx);
			});

			// Present received text over frames before reading the copy control bounds.
			if completed {
				let initial = visual.debug_bounds("response-footer").expect("mounted footer slot");
				assert!(
					initial.size.height < gpui::px(24.),
					"new metadata must grow without jumping"
				);
			}
			for _ in 0..32 {
				thread::sleep(std::time::Duration::from_millis(16));

				visual.update(crate::ui_frame::draw_frame);
			}

			if !completed {
				assert!(
					visual.debug_bounds(selector).is_none(),
					"streaming output has no response action"
				);
				continue;
			}
			let copy = visual.debug_bounds(selector).expect("completed response is copyable");
			assert!(visual.debug_bounds("turn-metrics-hover").is_some());
			assert!(visual.debug_bounds("response-footer").unwrap().size.height >= gpui::px(24.));

			visual.simulate_mouse_down(copy.center(), gpui::MouseButton::Left, Default::default());
			visual.update(crate::ui_frame::draw_frame);

			assert_eq!(visual.debug_bounds(selector), Some(copy));

			visual.simulate_mouse_up(copy.center(), gpui::MouseButton::Left, Default::default());
			visual.update(|_, cx| {
				assert_eq!(
					cx.read_from_clipboard().and_then(|item| item.text()),
					Some(original.into())
				);
			});
		}
	}

	fn plan_entry() -> AgentTimelineEntry {
		AgentTimelineEntry {
			position: 6,
			content: AgentTimelineContent::Item {
				collaboration: None,
				phase: None,
				app_ui: false,
				turn_id: "plan-turn".into(),
				item_id: "plan-item".into(),
				kind: "plan".into(),
				text: "## Proposed work\n1. Inspect source\n2. Verify changes".into(),
				truncated: false,
				activity: None,
				attachments: vec![],
			},
		}
	}

	fn rendered_entries() -> Vec<AgentTimelineEntry> {
		vec![
			AgentTimelineEntry {
				position: 0,
				content: AgentTimelineContent::Item { collaboration: None,
phase: None, app_ui: false,
					turn_id: "turn".into(),
					item_id: "input".into(),
					kind: "userMessage".into(),
					text: "Describe this image.".into(),
					truncated: false,
					activity: None,
					attachments: vec![decodex_protocol::AgentTimelineAttachment {
						index: 1,
						kind: "localImage".into(),
						label: "photo.png".into(),
						source: decodex_protocol::AgentTimelineAttachmentSource::Local,
					}],
				},
			},
			AgentTimelineEntry {
				position: 1,
				content: AgentTimelineContent::Item { collaboration: None,
phase: None, app_ui: false,
					turn_id: "turn".into(),
					item_id: "message".into(),
					kind: "agentMessage".into(),
					text: "**Native result**\n\n```rust\nlet value = 1;\n```".into(),
					truncated: true,
					activity: None,
					attachments: vec![],
				},
			},
			AgentTimelineEntry {
				position: 2,
				content: AgentTimelineContent::Speech {
					item_id: "speech".into(),
					session_id: "voice".into(),
					role: "user".into(),
					text: "Explain the result.".into(),
					truncated: false,
				},
			},
			AgentTimelineEntry {
				position: 3,
				content: AgentTimelineContent::Promotion {
					item_id: "promotion".into(),
					session_id: "voice".into(),
					turn_id: "turn".into(),
					agent_item_id: "message".into(),
					presentation: "inlineVisualization".into(),
					resolved: None,
					index: Some(u32::MAX),
				},
			},
			AgentTimelineEntry {
				position: 4,
				content: AgentTimelineContent::Promotion {
					item_id: "off-page-promotion".into(),
					session_id: "voice".into(),
					turn_id: "older-turn".into(),
					agent_item_id: "off-page-message".into(),
					presentation: "inlineMarkdown".into(),
					index: None,
					resolved: Some(decodex_protocol::AgentTimelinePromotedContent {
						text: "**Resolved older message**".into(),
						truncated: false,
						activity: None,
						attachments: vec![],
					}),
				},
			},
			AgentTimelineEntry {
				position: 5,
				content: AgentTimelineContent::TurnBoundary {
					turn_id: "turn".into(),
					completed: true,
					status: Some("failed".into()),
					duration_ms: Some(100),
					usage: Some(decodex_protocol::AgentTurnUsageDto { input_tokens: 120, output_tokens: 30, details: None }), usage_summary: Some(
						"Turn tokens: input 120, output 30.\nThread total tokens: 900.\nObserved responses: 1. Showing 1 recorded amounts; units are provider-defined.\nResponse fixture: 0.12345678901234567890.".into(),
					),
					error: Some(decodex_protocol::AgentTimelineError {
						message: "Model is overloaded. Try again.".into(),
						truncated: true,
					}),
				},
			},
			plan_entry(),
		]
	}

	#[gpui::test]
	fn live_plan_and_reasoning_wait_for_exact_native_items(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("native-thread".into());

			let mut history = Timeline::default();

			assert!(history.replace(
				Binding {
					work: work.id.clone(),
					thread: "native-thread".into(),
					account: "account".into()
				},
				AgentTimelinePage {
					thread_id: "native-thread".into(),
					entries: vec![],
					next_cursor: None,
					weather: Default::default(),
					thread_token_total: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None
				}
			));

			s.timeline.native = history;

			let Some((_, decodex_protocol::AgentHistoryResult::Available { live, .. })) =
				&mut s.history
			else {
				panic!("fixture history")
			};

			live.push(decodex_protocol::AgentLiveMessageDto {
				kind: decodex_protocol::AgentLiveMessageKind::Plan,
				turn_id: "plan-turn".into(),
				item_id: "plan-item".into(),
				text: "Draft proposal".into(),
				truncated: false,
			});
			live.push(decodex_protocol::AgentLiveMessageDto {
				kind: decodex_protocol::AgentLiveMessageKind::ReasoningSummary,
				turn_id: "reasoning-turn".into(),
				item_id: "plan-item".into(),
				text: "Public summary in progress".into(),
				truncated: false,
			});
			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("native-live-plan").is_some());
		assert!(visual.debug_bounds("native-live-reasoning-summary").is_some());

		surface.update(visual, |s, cx| {
			s.timeline.native.entries.push(plan_entry());
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("native-live-plan").is_none());
		assert!(visual.debug_bounds("native-plan-content").is_some());
		assert!(visual.debug_bounds("native-live-reasoning-summary").is_some());

		surface.update(visual, |s, cx| {
			let mut entry = plan_entry();

			entry.position += 1;

			if let AgentTimelineContent::Item { turn_id, kind, text, .. } = &mut entry.content {
				*turn_id = "reasoning-turn".into();
				*kind = "reasoning".into();
				*text = "Final public summary".into();
			}

			s.timeline.native.entries.push(entry);
			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("native-live-reasoning-summary").is_none());
		assert!(visual.debug_bounds("native-reasoning-summary").is_some());
	}

	fn prepare_rendered_history(
		s: &mut AgentSurface,
		cx: &mut Context<AgentSurface>,
	) -> Vec<String> {
		s.visual_workspace_fixture(cx);

		s.workspace.graph_visible = false;

		let work = s
			.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|work| Some(&work.id) == s.selected.as_ref())
			.unwrap();

		work.codex_thread_id = Some("native-thread".into());

		let mut entries = rendered_entries();

		entries.push(AgentTimelineEntry {
			position: 100,
			content: AgentTimelineContent::Item {
				collaboration: None,
				phase: None,
				app_ui: false,
				turn_id: "summary-turn".into(),
				item_id: "summary-item".into(),
				kind: "reasoning".into(),
				text: "Checking the request.".into(),
				truncated: false,
				activity: None,
				attachments: vec![],
			},
		});

		let selectors = entries
			.iter()
			.map(|entry| {
				if matches!(entry.content, AgentTimelineContent::Speech { .. }) {
					return "voice-history-block".into();
				}

				format!(
					"native-history-{}",
					serde_json::json!([work.id, work.codex_thread_id, native_timeline::key(entry)])
				)
			})
			.collect::<Vec<_>>();
		let mut history = Timeline::default();

		assert!(history.replace(
			Binding {
				work: work.id.clone(),
				thread: "native-thread".into(),
				account: "account".into()
			},
			AgentTimelinePage {
				thread_id: "native-thread".into(),
				entries,
				next_cursor: None,
				weather: Default::default(),
				thread_token_total: None,
				thread_metrics: None,
				safety_buffering_turn_id: None,
				active_realtime_session_at_page_start: None
			}
		));

		s.timeline.native = history;

		cx.notify();

		selectors
	}

	#[gpui::test]
	fn native_messages_and_promotions_render_with_distinct_provider_identities(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		let selectors = surface.update(visual, prepare_rendered_history);

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		// Settle the workspace's sidebar entrance before measuring the footer.
		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		let bounds = selectors
			.into_iter()
			.map(|selector| {
				visual
					.debug_bounds(Box::leak(selector.clone().into_boxed_str()))
					.unwrap_or_else(|| panic!("missing {selector}"))
			})
			.collect::<Vec<_>>();

		assert!(bounds.iter().all(|bounds| bounds.size.height > gpui::px(0.)));
		assert!(bounds.windows(2).all(|pair| pair[0].bottom() <= pair[1].top()));
		assert!(visual.debug_bounds("native-turn-usage").is_none());

		let details = visual.debug_bounds("turn-metrics-hover").expect("compact details control");

		visual.simulate_mouse_move(details.center(), gpui::MouseButton::Left, Default::default());
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		let details = visual.debug_bounds("turn-metrics-hover").unwrap();

		visual.simulate_mouse_move(details.center(), gpui::MouseButton::Left, Default::default());
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		thread::sleep(std::time::Duration::from_millis(220));

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert_eq!(
			details,
			visual.debug_bounds("turn-metrics-hover").unwrap(),
			"details must not shift the transcript"
		);
		assert!(visual.debug_bounds("native-turn-usage").is_some());

		visual.simulate_mouse_move(
			gpui::point(gpui::px(1_390.), gpui::px(1_390.)),
			gpui::MouseButton::Left,
			Default::default(),
		);
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("native-turn-usage").is_none(), "moving away closes details");
		assert_eq!(details, visual.debug_bounds("turn-metrics-hover").unwrap());
		assert!(visual.debug_bounds("native-plan-content").is_some());
		assert!(visual.debug_bounds("native-promotion-content").is_some());
		assert!(visual.debug_bounds("native-reasoning-summary").is_some());
		assert!(visual.debug_bounds("saved-local-history").is_none());

		surface.update(visual, |s, cx| {
			s.workspace.details_visible = true;
			cx.notify();
		});
		visual.update(|window, cx| window.draw(cx).clear(cx));
		std::thread::sleep(std::time::Duration::from_millis(250));
		visual.update(|window, cx| window.draw(cx).clear(cx));
		let toggle = visual.debug_bounds("native-history-source-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("saved-local-history").is_some());

		surface.read_with(visual, |s, _| assert!(s.timeline.native.show_saved));

		let toggle = visual.debug_bounds("native-history-source-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("saved-local-history").is_none());

		surface.update(visual, |s, cx| {
			let mut duplicate = s.timeline.native.entries[1].clone();

			duplicate.position = 7;

			s.timeline.native.entries.push(duplicate);
			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("native-promotion-unavailable").is_some());
	}

	#[gpui::test]
	fn unfinished_plan_stays_readable_without_response_actions(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		let original = "## Draft\n\n$$\n\\frac{a}{b}";

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.workspace.graph_visible = false;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == s.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("native-thread".into());

			let work = work.clone();
			let mut partial = s.native_message_entry(&work, "plan-turn", original, "agentMessage");

			partial.id = 991;
			partial.kind = "partial_plan".into();
			partial.native_source = Some(decodex_protocol::AgentHistorySourceDto {
				thread_id: "native-thread".into(),
				turn_id: "plan-turn".into(),
				item_id: "plan-item".into(),
			});

			let (_, super::super::AgentHistoryResult::Available { entries, .. }) =
				s.history.as_mut().unwrap()
			else {
				panic!("fixture history");
			};

			*entries = vec![partial];

			let mut history = Timeline::default();

			assert!(history.replace(
				Binding {
					work: work.id.clone(),
					thread: "native-thread".into(),
					account: "account".into()
				},
				AgentTimelinePage {
					thread_id: "native-thread".into(),
					entries: vec![],
					next_cursor: None,
					weather: Default::default(),
					thread_token_total: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None
				}
			));

			s.timeline.native = history;

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("partial-receipt-991").is_some());
		assert!(visual.debug_bounds("copy-partial-991").is_none());

		for truncated in [true, false] {
			surface.update(visual, |s, cx| {
				let mut entry = plan_entry();

				if let AgentTimelineContent::Item { truncated: value, .. } = &mut entry.content {
					*value = truncated;
				}

				s.timeline.native.entries = vec![entry];

				cx.notify();
			});

			visual.update(|window, cx| {
				window.draw(cx).clear(cx);
			});

			assert_eq!(visual.debug_bounds("partial-receipt-991").is_some(), truncated);
			assert!(visual.debug_bounds("copy-partial-991").is_none());
			assert!(visual.debug_bounds("native-plan-content").is_some());
		}
	}
}
