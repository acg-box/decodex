//! Render native history without converting provider IDs into local inbox IDs.
use gpui::{AnyElement, Div, KeyDownEvent, PathBuilder, Role, StatefulInteractiveElement as _};

use crate::{
	shell::{
		agent_surface::{
			native_timeline::{
				self, AgentHistoryResult, AgentSurface, AgentTimelineEntry, AgentWorkItemDto,
				Content, Context, FluentBuilder, InteractiveElement, IntoElement, ParentElement,
				SharedString, Styled, key, markdown,
			},
			response_metrics::ResponseMetrics,
			text_reveal::StreamingText,
		},
		workspace_symbols,
	},
	ui_loading,
	ui_theme::{HOVER_FILL, TEXT_MUTED, USER_MESSAGE_ACTION_SIZE},
};
use decodex_protocol::{AgentLiveMessageDto, AgentTimelineAttachment};

impl AgentSurface {
	pub(super) fn native_summary_row(
		&self,
		work: &AgentWorkItemDto,
		item: &Content,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let Content::Item { turn_id, item_id, kind, text, truncated, attachments, .. } = item
		else {
			return gpui::div().into_any_element();
		};
		let identity =
			serde_json::json!(["summary", work.id, work.codex_thread_id, turn_id, item_id])
				.to_string();
		let mut row = gpui::div()
			.w_full()
			.min_w_0()
			.flex()
			.flex_col()
			.gap_1()
			.debug_selector(|| "native-summary-message".into())
			.child(native_timeline::muted(if kind == "userMessage" { "You" } else { "Assistant" }))
			.child(markdown::render(text, &identity));

		for attachment in attachments {
			row = row.child(self.native_attachment(work, turn_id, item_id, attachment, cx));
		}

		if *truncated {
			row = row.child(native_timeline::muted(
				"Some content was omitted from this history preview.",
			));
		}
		if kind == "agentMessage" && !text.is_empty() {
			row = row.child(markdown::response_copy_button(
				&format!("copy-{identity}"),
				"Copy response",
				text.clone(),
			));
		}
		if kind == "agentMessage"
			&& let Some(action) = self.voice_read_action(&work.id, &identity, text, *truncated, cx)
		{
			row = row.child(action);
		}

		row.into_any_element()
	}

	pub(super) fn native_timeline_row(
		&self,
		work: &AgentWorkItemDto,
		entry: &AgentTimelineEntry,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let identity = serde_json::json!([work.id, work.codex_thread_id, key(entry)]).to_string();
		let selector = format!("native-history-{identity}");
		let content = self.native_timeline_content(work, entry, &identity, cx);
		let process = matches!(&entry.content, Content::Item { kind, phase, activity, attachments, app_ui: false, .. }
            if attachments.is_empty() && (activity.is_some() || matches!(kind.as_str(), "reasoning" | "plan")
                || (kind == "agentMessage" && phase.as_deref() == Some("commentary"))));
		let content = if process { process_indent(content) } else { content };
		let content = self.anchored_native_history_entry(work, entry, content);
		let content = self.native_scroll_row(work, entry, content, cx);

		gpui::div()
			.debug_selector(move || selector)
			.id(SharedString::from(identity.clone()))
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
			Content::Item { turn_id, item_id, attachments, app_ui, text, activity, .. }
				if attachments.is_empty()
					&& (activity.is_none() || self.activity_detail.value.is_none())
					&& !app_ui
					&& !text.contains("![")
					&& work.active_turn_id.as_deref() != Some(turn_id)
					&& !self.native_history.weather.contains_key(turn_id)
					&& self.native_live_message(work, turn_id, item_id).is_none() =>
				true,
			Content::TurnBoundary { completed: true, turn_id, .. }
				if work.active_turn_id.as_deref() != Some(turn_id) =>
				true,
			_ => false,
		};

		can_window
			.then(|| {
				self.transcript_scroll.get(&work.id).and_then(|scroll| {
					self.native_history.viewport.offscreen_height(
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
		if self.native_history.entries.iter().any(|entry| {
			matches!(&entry.content,
			Content::TurnBoundary { turn_id, completed: true, .. } if turn_id == turn)
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
				kind: if kind == "userMessage" { "user" } else { "assistant" }.into(),
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
			&& let Some(forecasts) = self.native_history.weather.get(turn_id)
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
		cx: &mut Context<Self>,
	) -> AnyElement {
		let row = gpui::div().w_full().min_w_0().flex().flex_col().gap_1();

		match &entry.content {
			content @ Content::Item { .. } => self.native_item_content(work, content, identity, cx),
			Content::Speech { role, text, truncated, .. } => row
				.child(native_timeline::muted(if role == "user" {
					"You · Voice"
				} else {
					"Assistant · Voice"
				}))
				.child(text.clone())
				.when(*truncated, |r| {
					r.child(native_timeline::muted("Voice transcript shortened."))
				})
				.into_any_element(),
			Content::VoiceBoundary { kind, outcome, .. } => row
				.child(native_timeline::muted(if kind == "realtimeSessionStarted" {
					"Voice conversation started"
				} else if outcome.as_deref() == Some("failed") {
					"Voice conversation failed"
				} else {
					"Voice conversation ended"
				}))
				.into_any_element(),
			Content::TurnBoundary { completed, turn_id, status, error, .. } => {
				let has_reply = self.native_history.entries.iter().any(|entry| matches!(&entry.content, Content::Item { turn_id: turn, kind, .. } if turn == turn_id && kind == "agentMessage"));

				row.when(*completed && !has_reply, |row| {
					row.child(self.native_turn_metrics(&entry.content, identity, cx))
				})
				.when(
					*completed && matches!(status.as_deref(), Some("interrupted" | "failed")),
					|row| {
						row.child(native_timeline::muted(
							if status.as_deref() == Some("interrupted") {
								"Stopped"
							} else {
								"Failed"
							},
						))
					},
				)
				.children(error.as_ref().map(|error| gpui::div().child(error.message.clone())))
				.into_any_element()
			},
			content @ Content::Promotion { .. } =>
				self.native_promotion(work, content, identity, cx),
		}
	}

	fn native_turn_metrics(
		&self,
		boundary: &Content,
		identity: &str,
		_cx: &mut Context<Self>,
	) -> AnyElement {
		let Content::TurnBoundary { duration_ms, status, usage, .. } = boundary else {
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

	fn native_item_content(
		&self,
		work: &AgentWorkItemDto,
		content: &Content,
		identity: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let Content::Item {
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

		if kind == "agentMessage"
			&& phase.as_deref() == Some("commentary")
			&& attachments.is_empty()
		{
			let body = markdown::render_process(text, identity);

			return if let Some(action) =
				self.voice_read_action(&work.id, identity, text, truncated || draft.is_some(), cx)
			{
				gpui::div().child(body).child(action).into_any_element()
			} else {
				body
			};
		}
		if matches!(kind.as_str(), "userMessage" | "agentMessage") {
			let message = self.native_message_entry(work, turn_id, text, kind);
			let mut body = gpui::div().debug_selector(|| "native-promotion-content".into());

			for attachment in attachments {
				body = body.child(self.native_attachment(work, turn_id, item_id, attachment, cx));
			}

			body = if draft.is_some() {
				body.child(StreamingText { text: text.into(), key: identity.into() }).child(
					markdown::response_copy_button(
						&format!("copy-response-{identity}"),
						"Copy response",
						text.to_owned(),
					),
				)
			} else {
				let last_reply = self.native_history.entries.iter().rev().find_map(|entry| {
					match &entry.content {
						Content::Item { turn_id: turn, item_id, kind, .. }
							if turn == turn_id && kind == "agentMessage" =>
							Some(item_id),
						_ => None,
					}
				});
				let metrics = (kind == "agentMessage" && last_reply == Some(item_id)).then(|| self.native_history.entries.iter().find(|entry| matches!(&entry.content, Content::TurnBoundary { turn_id: turn, completed: true, .. } if turn == turn_id))).flatten().map(|entry| self.native_turn_metrics(&entry.content, identity, cx));

				body.child(super::super::history_entry_with_metrics(&message, identity, metrics))
			};

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
				body = body.child(native_timeline::muted(
					"Some content was omitted from this history preview.",
				));
			}
			if kind == "userMessage" {
				body = self.native_prompt_row(work, content, identity, body, cx);
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
		content: &Content,
		identity: &str,
		text: &str,
		truncated: bool,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let Content::Item { kind, turn_id, item_id, attachments, activity, .. } = content else {
			unreachable!("item text renderer")
		};
		let row = gpui::div().w_full().min_w_0().flex().flex_col().gap_1();
		let label = match kind.as_str() {
			"userMessage" => "You",
			"agentMessage" => "Assistant",
			"plan" => "Proposed plan",
			"reasoning" => "Thinking",
			"functionCallOutput" => "Tool result",
			_ => kind,
		};
		let mut row = row.when(kind != "reasoning", |row| row.child(native_timeline::muted(label)));

		for attachment in attachments {
			row = row.child(self.native_attachment(work, turn_id, item_id, attachment, cx));
		}

		if !text.is_empty() {
			let selector = match kind.as_str() {
				"plan" => "native-plan-content",
				"reasoning" => "native-reasoning-summary",
				_ => "native-promotion-content",
			};

			row = row.child(gpui::div().debug_selector(move || selector.into()).child(
				if kind == "reasoning" {
					markdown::render_process(text, identity)
				} else {
					markdown::render(text, identity)
				},
			));
		}
		if truncated {
			row = row.child(native_timeline::muted(
				"Some content was omitted from this history preview.",
			));
		}
		if matches!(kind.as_str(), "agentMessage" | "plan") && !text.is_empty() {
			row = row.child(markdown::response_copy_button(
				&format!("copy-{identity}"),
				if kind == "plan" { "Copy plan" } else { "Copy response" },
				text.to_owned(),
			));
		}

		if let Some(activity) = activity {
			return self.detail_row(
				work,
				activity,
				row.child(format!("{} · {}", activity.label, activity.status)),
				cx,
			);
		}

		row.into_any_element()
	}

	fn native_prompt_row(
		&self,
		work: &AgentWorkItemDto,
		content: &Content,
		identity: &str,
		body: Div,
		cx: &mut Context<Self>,
	) -> Div {
		let Content::Item { turn_id, item_id, .. } = content else {
			unreachable!("prompt renderer")
		};
		let Some(thread) = &work.codex_thread_id else { return body };
		let (owner, thread, turn, item) =
			(work.id.clone(), thread.clone(), turn_id.clone(), item_id.clone());
		let group: SharedString = format!("history-input-{identity}").into();
		let keyboard_source = (owner.clone(), thread.clone(), turn.clone(), item.clone());
		let edit = gpui::div()
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
				gpui::canvas(
					|_, _, _| (),
					|bounds, _, window, _| {
						let point = |x, y| bounds.origin + gpui::point(gpui::px(x), gpui::px(y));
						let mut path = PathBuilder::stroke(gpui::px(1.1));

						path.move_to(point(2., 9.));
						path.line_to(point(9., 2.));
						path.line_to(point(12., 5.));
						path.line_to(point(5., 12.));
						path.line_to(point(1., 13.));
						path.close();
						path.move_to(point(7., 4.));
						path.line_to(point(10., 7.));

						if let Ok(path) = path.build() {
							window.paint_path(path, gpui::rgb(TEXT_MUTED));
						}
					},
				)
				.size(gpui::px(14.)),
			);

		body.group(group).child(gpui::div().flex().justify_end().child(edit))
	}

	fn native_activity_content(
		&self,
		work: &AgentWorkItemDto,
		content: &Content,
		identity: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let Content::Item { activity: Some(activity), kind, turn_id, item_id, .. } = content else {
			unreachable!("activity renderer")
		};
		let label = if matches!(kind.as_str(), "dynamicToolCall" | "mcpToolCall")
			&& !activity.detail.is_empty()
		{
			activity.detail.clone()
		} else {
			activity.label.clone()
		};
		let expanded = self
			.activity_detail_key(&(work.id.clone(), turn_id.clone(), item_id.clone()))
			.is_some_and(|key| {
				self.activity_detail.value.as_ref().is_some_and(|(current, _)| current == &key)
			});
		let symbol = match activity.status.as_str() {
			"failed" | "declined" => "!",
			"running" => "◌",
			_ => "✓",
		};
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
			.child(
				// Status glyph advances differ; reserve one stable column for every state.
				gpui::div()
					.w(gpui::px(16.))
					.h(gpui::px(18.))
					.flex_none()
					.flex()
					.items_center()
					.justify_center()
					.child(if activity.status == "running" {
						ui_loading::loading("").into_any_element()
					} else {
						gpui::div().child(symbol).into_any_element()
					}),
			)
			.child(gpui::div().flex_1().min_w_0().text_ellipsis().child(label))
			.when_some(activity.plugin_id.clone(), |d, plugin| {
				d.child(
					gpui::div()
						.max_w(gpui::px(140.))
						.text_ellipsis()
						.child(format!("Plugin: {plugin}")),
				)
			})
			.when(activity.read_only_hint == Some(true), |d| d.child("Read-only hint"))
			.when(matches!(activity.status.as_str(), "failed" | "declined"), |d| {
				d.child(activity.status.clone())
			})
			.when_some(activity.duration_ms.filter(|ms| *ms > 0), |d, ms| {
				d.child(if ms < 1_000 {
					format!("{ms}ms")
				} else {
					format!("{:.1}s", ms as f64 / 1_000.)
				})
			})
			.child(
				gpui::div()
					.debug_selector(|| "tool-chevron-bounds".into())
					.flex_none()
					.size(gpui::px(12.))
					.child(workspace_symbols::process_chevron(
						SharedString::from(format!("tool-chevron-{identity}")),
						expanded,
					)),
			);

		self.detail_row(work, activity, row, cx)
	}

	fn native_promotion(
		&self,
		work: &AgentWorkItemDto,
		content: &Content,
		identity: &str,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let Content::Promotion { turn_id, agent_item_id, presentation, index, resolved, .. } =
			content
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
					self.native_history.entries.iter().filter_map(|entry| match &entry.content {
						Content::Item {
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
		let mut row = row.child(native_timeline::muted("Shared during voice conversation"));

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
					row = row.child(native_timeline::muted(&format!(
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
					row = row.child(native_timeline::muted("Shared content preview shortened."));
				}

				if let Some(activity) = activity {
					return self.detail_row(work, activity, row.child(activity.label.clone()), cx);
				}
			},
			_ =>
				row = row.child(
					gpui::div().debug_selector(|| "native-promotion-unavailable".into()).child(
						native_timeline::muted(
							"Referenced result is temporarily unavailable. History refresh will retry.",
						),
					),
				),
		}

		row.into_any_element()
	}
}

/// Keep the guide gutter inside the available transcript width in both rendering paths.
pub(super) fn process_indent(content: impl IntoElement) -> AnyElement {
	gpui::div()
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
		Binding, Timeline,
		render::{AgentSurface, AgentTimelineEntry, Content, Context},
	};
	use decodex_protocol::AgentTimelinePage;

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

			s.native_history.weather.insert("weather-turn".into(), vec![forecast.clone()]);

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
	fn native_answer_stays_copyable_during_streaming(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		let original = "Draft response\n\n$$\n\\frac{a}{b}";
		let selector = surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;

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

			if let Content::Item { text, .. } = &mut entry.content {
				*text = original.into();
			}

			let identity =
				serde_json::json!([work.id, work.codex_thread_id, super::key(&entry)]).to_string();

			s.native_history.replace(
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
					cx.notify();
				});
			}

			visual.update(|window, cx| {
				window.draw(cx).clear();
			});

			let copy = visual.debug_bounds(selector).expect("native response remains copyable");

			visual.simulate_click(copy.center(), Default::default());
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
			content: Content::Item {
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
				content: Content::Item { phase: None, app_ui: false,
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
				content: Content::Item { phase: None, app_ui: false,
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
				content: Content::Speech {
					item_id: "speech".into(),
					session_id: "voice".into(),
					role: "user".into(),
					text: "Explain the result.".into(),
					truncated: false,
				},
			},
			AgentTimelineEntry {
				position: 3,
				content: Content::Promotion {
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
				content: Content::Promotion {
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
				content: Content::TurnBoundary {
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

			s.graph_visible = false;

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
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None
				}
			));

			s.native_history = history;

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
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("native-live-plan").is_some());
		assert!(visual.debug_bounds("native-live-reasoning-summary").is_some());

		surface.update(visual, |s, cx| {
			s.native_history.entries.push(plan_entry());
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("native-live-plan").is_none());
		assert!(visual.debug_bounds("native-plan-content").is_some());
		assert!(visual.debug_bounds("native-live-reasoning-summary").is_some());

		surface.update(visual, |s, cx| {
			let mut entry = plan_entry();

			entry.position += 1;

			if let Content::Item { turn_id, kind, text, .. } = &mut entry.content {
				*turn_id = "reasoning-turn".into();
				*kind = "reasoning".into();
				*text = "Final public summary".into();
			}

			s.native_history.entries.push(entry);
			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("native-live-reasoning-summary").is_none());
		assert!(visual.debug_bounds("native-reasoning-summary").is_some());
	}

	fn prepare_rendered_history(
		s: &mut AgentSurface,
		cx: &mut Context<AgentSurface>,
	) -> Vec<String> {
		s.visual_workspace_fixture(cx);

		s.graph_visible = false;

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
			content: Content::Item {
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
				format!(
					"native-history-{}",
					serde_json::json!([work.id, work.codex_thread_id, super::key(entry)])
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
				safety_buffering_turn_id: None,
				active_realtime_session_at_page_start: None
			}
		));

		s.native_history = history;

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
			window.draw(cx).clear();
		});

		// Settle the workspace's sidebar entrance before measuring the footer.
		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let bounds = selectors
			.into_iter()
			.map(|selector| visual.debug_bounds(Box::leak(selector.into_boxed_str())).unwrap())
			.collect::<Vec<_>>();

		assert!(bounds.iter().all(|bounds| bounds.size.height > gpui::px(0.)));
		assert!(bounds.windows(2).all(|pair| pair[0].bottom() <= pair[1].top()));
		assert!(visual.debug_bounds("native-turn-usage").is_none());

		let details = visual.debug_bounds("turn-metrics-hover").expect("compact details control");

		visual.simulate_mouse_move(details.center(), gpui::MouseButton::Left, Default::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let details = visual.debug_bounds("turn-metrics-hover").unwrap();

		visual.simulate_mouse_move(details.center(), gpui::MouseButton::Left, Default::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		thread::sleep(std::time::Duration::from_millis(220));

		visual.update(|window, cx| {
			window.draw(cx).clear();
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
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("native-turn-usage").is_none(), "moving away closes details");
		assert_eq!(details, visual.debug_bounds("turn-metrics-hover").unwrap());
		assert!(visual.debug_bounds("native-plan-content").is_some());
		assert!(visual.debug_bounds("native-promotion-content").is_some());
		assert!(visual.debug_bounds("native-reasoning-summary").is_some());
		assert!(visual.debug_bounds("saved-local-history").is_none());

		let toggle = visual.debug_bounds("native-history-source-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("saved-local-history").is_some());

		surface.read_with(visual, |s, _| assert!(s.native_history.show_saved));

		let toggle = visual.debug_bounds("native-history-source-toggle").unwrap();

		visual.simulate_click(toggle.center(), Default::default());
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("saved-local-history").is_none());

		surface.update(visual, |s, cx| {
			let mut duplicate = s.native_history.entries[1].clone();

			duplicate.position = 7;

			s.native_history.entries.push(duplicate);
			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		assert!(visual.debug_bounds("native-promotion-unavailable").is_some());
	}

	#[gpui::test]
	fn unfinished_plan_stays_copyable_until_exact_native_history_is_complete(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_400.)));

		let original = "## Draft\n\n$$\n\\frac{a}{b}";

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.graph_visible = false;

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
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None
				}
			));

			s.native_history = history;

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear();
		});

		let copy = visual.debug_bounds("copy-partial-991").expect("retained plan is visible");

		visual.simulate_click(copy.center(), Default::default());
		visual.update(|_, cx| {
			assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), Some(original.into()))
		});

		for truncated in [true, false] {
			surface.update(visual, |s, cx| {
				let mut entry = plan_entry();

				if let Content::Item { truncated: value, .. } = &mut entry.content {
					*value = truncated;
				}

				s.native_history.entries = vec![entry];

				cx.notify();
			});

			visual.update(|window, cx| {
				window.draw(cx).clear();
			});

			assert_eq!(visual.debug_bounds("copy-partial-991").is_some(), truncated);
			assert!(visual.debug_bounds("native-plan-content").is_some());
		}
	}
}
