//! Current unconfirmed input pages are independent of both transcript cursors.
use std::rc::Rc;

use gpui::{
	AnyElement, AppContext as _, Div, KeyDownEvent, Role, StatefulInteractiveElement,
	prelude::FluentBuilder as _,
};
use tokio::runtime::Builder;

#[cfg(test)] use crate::shell::agent_surface::native_timeline::Binding;
use crate::{
	shell::{
		agent_surface,
		agent_surface::{
			composer::ComposerTip,
			native_timeline::{
				AgentClient, AgentSurface, AgentWorkItemDto, Context, EntityId, InteractiveElement,
				IntoElement, ParentElement, Styled, Task,
			},
			selectable_text::SelectableText,
		},
	},
	ui_feedback::{Feedback, Tone},
	ui_loading,
	ui_motion::SmoothControl as _,
	ui_theme::{self, BLUE, HOVER_FILL, TEXT, USER_MESSAGE_ACTION_SIZE, USER_MESSAGE_INSET},
};
use decodex_protocol::{
	AgentActionDto, AgentCommandResponse, AgentDispatchStateDto, AgentHistoryEntryDto,
	AgentInputReceiptsResult, DesktopMessage, DesktopMessageState, IdempotencyKey, WireText,
};

#[derive(Default)]
pub(super) struct InputReceipts {
	task: Option<Task<()>>,
	after: Option<i64>,
	result: Option<AgentInputReceiptsResult>,
}
impl InputReceipts {
	pub(super) fn cancel_read(&mut self) {
		self.task = None;
	}
}

struct MessageCard {
	key: String,
	title: &'static str,
	tone: Tone,
	text: String,
	reason: Option<String>,
	action: Option<AnyElement>,
}

impl AgentSurface {
	pub(in super::super) fn local_message_cards(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
		if self.selected_is_archived() {
			return Vec::new();
		}

		let work = self.conversation_work();
		let owner = work
			.as_ref()
			.map(|work| work.id.clone())
			.or_else(|| self.conversation_page())
			.or_else(|| self.workspace.new_conversation.clone());

		self.retained_messages()
			.into_iter()
			.filter(|(_, message)| {
				message.owner.as_ref().map(|owner| owner.as_str()) == owner.as_deref()
					&& match &message.action {
						AgentActionDto::NativeAgentInput { thread_id, .. } => self
							.native_agents
							.selected
							.as_ref()
							.is_some_and(|(_, thread)| thread == thread_id.as_str()),
						_ => self.native_agents.selected.is_none(),
					}
					&& !(self.sending && message.state == DesktopMessageState::Sending)
			})
			.map(|(key, message)| {
				let failed = message.state == DesktopMessageState::Failed;
				let tone = if failed { Tone::Error } else { Tone::Warning };
				let title = if failed { "Not sent" } else { "Delivery unconfirmed" };
				let id = format!("local-message-{}", key.as_str());
				let target = key.clone();
				let original = message.clone();
				let can_retry = failed
					&& self.local_message_target_matches(&message)
					&& self.command_connection_ready()
					&& !self.sending
					&& !self.uncertain
					&& !self.selected_is_archived()
					&& self.composer_unavailable_reason().is_none();
				let action = if can_retry {
					Some(self.message_receipt_action(
						format!("{id}-retry"),
						"Resend",
						move |s, cx| s.retry_local_message(&target, &original, cx),
						cx,
					))
				} else if !failed && self.read_connection_ready() {
					Some(self.message_receipt_action(
						format!("{id}-check"),
						"Check status",
						move |s, cx| s.check_local_message(&target, &original, cx),
						cx,
					))
				} else {
					None
				};

				Self::message_card(MessageCard {
					key: id,
					title,
					tone,
					text: message.text,
					reason: if failed {
						message_reason(message.error.as_deref())
					} else {
						message.error
					},
					action,
				})
				.into_any_element()
			})
			.collect()
	}

	fn local_message_target_matches(&self, message: &DesktopMessage) -> bool {
		let owner = self.conversation_work();
		let local_owner = owner
			.as_ref()
			.map(|owner| owner.id.clone())
			.or_else(|| self.conversation_page())
			.or_else(|| self.workspace.new_conversation.clone());

		if message.owner.as_ref().map(|owner| owner.as_str()) != local_owner.as_deref() {
			return false;
		}

		let thread = match &message.action {
			AgentActionDto::NativeAgentInput { .. } =>
				self.native_agents.selected.as_ref().map(|(_, thread)| thread.as_str()),
			_ if self.native_agents.selected.is_none() =>
				owner.as_ref().and_then(|owner| owner.codex_thread_id.as_deref()),
			_ => return false,
		};

		message.thread_id.as_ref().map(WireText::as_str) == thread
	}

	fn retry_local_message(
		&mut self,
		key: &IdempotencyKey,
		message: &DesktopMessage,
		cx: &mut Context<Self>,
	) {
		if message.state != DesktopMessageState::Failed
			|| !self.local_message_target_matches(message)
			|| !self.command_connection_ready()
			|| self.sending
			|| self.uncertain
			|| self.selected_is_archived()
			|| self.composer_unavailable_reason().is_some()
			|| !self
				.retained_messages()
				.iter()
				.any(|(saved, original)| saved == key && original == message)
		{
			return;
		}

		self.submit_command(message.action.clone(), None, Some((key.clone(), message.clone())), cx);
	}

	fn check_local_message(
		&mut self,
		key: &IdempotencyKey,
		message: &DesktopMessage,
		cx: &mut Context<Self>,
	) {
		if self.sending || self.submission.receipt_task.is_some() {
			return;
		}
		if matches!(&message.action, AgentActionDto::Steer { .. }) {
			self.refresh_steer_receipt(cx);

			return;
		}

		let (Some(profile), Some(owner)) = (self.profile.clone(), message.owner.clone()) else {
			return;
		};
		let key = key.clone();
		let requested = key.clone();
		let epoch = self.command_epoch;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime.block_on(AgentClient::new(profile).message_received(owner, requested)).ok()
		});

		self.submission.receipt_task = Some(cx.spawn(async move |surface, cx| {
			let received = request.await;
			let _ = surface.update(cx, |s, cx| {
				if s.command_epoch != epoch {
					return;
				}

				s.submission.receipt_task = None;

				if received == Some(true)
					&& let Some(owner) = s
						.retained_messages()
						.into_iter()
						.find(|(saved, _)| saved == &key)
						.and_then(|(_, message)| message.owner)
				{
					s.settle_message(
						Some(&key),
						&Ok(AgentCommandResponse::Accepted { work_id: owner }),
					);
					s.submission.unconfirmed.retain(|saved| saved != &key);

					s.uncertain = !s.submission.unconfirmed.is_empty();
					s.feedback = Feedback::neutral("Delivery confirmed.");

					s.save_draft_document(cx);
					s.refresh(cx);
				}

				cx.notify();
			});
		}));
	}

	pub(super) fn saved_input_card(
		&self,
		work: &str,
		entry: &AgentHistoryEntryDto,
		cx: &mut Context<Self>,
	) -> Div {
		let key = format!("saved-input-{work}-{}", entry.id);
		let failed = entry.kind == "unsent_input";
		let tone = if failed { Tone::Error } else { Tone::Warning };
		let target = work.to_owned();
		let saved = entry.clone();
		let can_retry = self.can_retry_saved_input(work, entry);
		let action = if can_retry {
			Some(self.message_receipt_action(
				format!("{key}-retry"),
				"Resend",
				move |s, cx| s.retry_saved_input(&target, &saved, cx),
				cx,
			))
		} else if !failed && self.read_connection_ready() {
			Some(self.message_receipt_action(
				format!("{key}-refresh"),
				"Check status",
				|s, cx| {
					s.refresh_steer_receipt(cx);
					s.refresh_native_input_receipts(cx);
					s.refresh(cx);
				},
				cx,
			))
		} else {
			None
		};

		Self::message_card(MessageCard {
			key,
			title: if failed { "Not sent" } else { "Delivery unconfirmed" },
			tone,
			text: entry.text.clone(),
			reason: message_reason(entry.receipt.as_ref().and_then(|r| r.unsent_reason.as_deref())),
			action,
		})
	}

	fn message_receipt_action(
		&self,
		key: String,
		label: &'static str,
		action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let label_key = format!("{key}-text");
		let selector = key.clone();
		let action = Rc::new(action);
		let keyboard = action.clone();

		gpui::div()
			.id(key)
			.debug_selector(move || selector.clone())
			.role(Role::Button)
			.aria_label(label)
			.tab_index(0)
			.h(gpui::px(USER_MESSAGE_ACTION_SIZE))
			.flex_none()
			.flex()
			.items_center()
			.whitespace_nowrap()
			.text_color(ui_theme::rgb(TEXT))
			.cursor_pointer()
			.rounded(gpui::px(4.))
			.hover(|s| s.text_color(ui_theme::rgb(BLUE)))
			.focus_visible(|s| s.bg(ui_theme::rgba(HOVER_FILL)))
			.on_click(cx.listener(move |s, _, _, cx| {
				action(s, cx);

				cx.stop_propagation();
			}))
			.on_key_down(cx.listener(move |s, event: &KeyDownEvent, _, cx| {
				if !event.is_held && ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					keyboard(s, cx);

					cx.stop_propagation();
				}
			}))
			.child(gpui::div().debug_selector(move || label_key.clone()).child(label))
			.smooth()
			.into_any_element()
	}

	fn message_card(card: MessageCard) -> Div {
		let MessageCard { key, title, tone, text, reason, action } = card;
		let status_key = format!("{key}-status");
		let message_key = format!("{key}-message");
		let status = tone
			.row(status_key.clone(), title)
			.h(gpui::px(USER_MESSAGE_ACTION_SIZE))
			.items_center()
			.debug_selector(move || status_key.clone())
			.when_some(reason, |status, reason| {
				status
					.aria_description(reason.clone())
					.tooltip(move |_, cx| cx.new(|_| ComposerTip(reason.clone())).into())
			});

		gpui::div()
			.debug_selector(|| "message-delivery".into())
			.w_full()
			.min_w_0()
			.flex_none()
			.flex()
			.flex_col()
			.gap_1()
			.child(
				gpui::div()
					.id(format!("{key}-content"))
					.w_full()
					.min_w_0()
					.overflow_x_hidden()
					.whitespace_normal()
					.child(agent_surface::speech_bubble(
						"user",
						"",
						&key,
						Some(
							gpui::div()
								.w_full()
								.min_w_0()
								.whitespace_normal()
								.child(SelectableText::plain(message_key, text))
								.into_any_element(),
						),
					)),
			)
			.child(
				gpui::div()
					.debug_selector(move || format!("{key}-footer"))
					.w_full()
					.pr(gpui::px(USER_MESSAGE_INSET))
					.flex()
					.justify_end()
					.items_center()
					.gap(gpui::px(12.))
					.text_size(gpui::px(12.))
					.line_height(gpui::px(18.))
					.child(status)
					.children(action),
			)
	}

	fn can_retry_saved_input(&self, work: &str, entry: &AgentHistoryEntryDto) -> bool {
		entry.kind == "unsent_input"
			&& entry.receipt.as_ref().is_some_and(|receipt| receipt.event_kind == "user_message")
			&& self.conversation_work().is_some_and(|owner| {
				owner.id == work
					&& owner.codex_thread_id.is_some()
					&& owner.dispatch_state == AgentDispatchStateDto::Idle
			})
			&& self.native_agents.selected.is_none()
			&& self.command_connection_ready()
			&& !self.sending
			&& !self.uncertain
			&& self.composer_unavailable_reason().is_none()
	}

	fn retry_saved_input(
		&mut self,
		work: &str,
		entry: &AgentHistoryEntryDto,
		cx: &mut Context<Self>,
	) {
		if !self.can_retry_saved_input(work, entry) {
			return;
		}

		let Some(owner) = self.conversation_work() else {
			return;
		};
		let (Ok(work_id), Some(thread)) = (EntityId::new(work), owner.codex_thread_id) else {
			return;
		};
		let Ok(thread_id) = WireText::new(thread) else {
			return;
		};

		self.execute(
			AgentActionDto::RetryInput { work_id, thread_id, event_id: entry.id },
			None,
			cx,
		);
	}

	pub(super) fn native_input_receipts_loaded(&self, work: &str) -> bool {
		matches!(&self.timeline.native.input_receipts.result,Some(AgentInputReceiptsResult::Available {work_id,..}) if work_id.as_str()==work)
	}

	pub(in super::super) fn refresh_native_input_receipts(&mut self, cx: &mut Context<Self>) {
		if self.timeline.native.input_receipts.task.is_some() {
			return;
		}

		let Some((work, _)) = self
			.timeline
			.native
			.requested
			.as_ref()
			.filter(|(work, _)| self.selected.as_ref() == Some(work))
		else {
			return;
		};
		let (Some(profile), Ok(work_id)) = (self.profile.clone(), EntityId::new(work.clone()))
		else {
			return;
		};
		let work = work.clone();
		let epoch = self.timeline.native.epoch;
		let after = self.timeline.native.input_receipts.after;
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;

			runtime.block_on(AgentClient::new(profile).input_receipts(work_id, after)).ok()
		});

		self.timeline.native.input_receipts.task = Some(cx.spawn(async move |surface, cx| {
			let result = request.await;
			let _ = surface.update(cx, |surface, cx| {
				if surface.timeline.native.epoch != epoch
					|| surface.selected.as_deref() != Some(&work)
					|| surface.timeline.native.input_receipts.after != after
				{
					return;
				}

				let receipts = &mut surface.timeline.native.input_receipts;

				receipts.task = None;

				let result = result.unwrap_or(AgentInputReceiptsResult::Unavailable);

				if receipts.result.as_ref() != Some(&result) {
					receipts.result = Some(result);

					cx.notify();
				}
			});
		}));
	}

	fn input_receipt_cursor(&mut self, work: &str, after: Option<i64>, cx: &mut Context<Self>) {
		if self.selected.as_deref() != Some(work)
			|| self.timeline.native.input_receipts.task.is_some()
		{
			return;
		}

		self.timeline.native.input_receipts.after = after;
		self.timeline.native.input_receipts.result = None;

		self.refresh_native_input_receipts(cx);
		cx.notify();
	}

	pub(super) fn native_input_receipts_panel(
		&self,
		work: &AgentWorkItemDto,
		cx: &mut Context<Self>,
	) -> Vec<AnyElement> {
		let state = &self.timeline.native.input_receipts;
		let mut rows = Vec::new();

		if state.after.is_some() {
			let owner = work.id.clone();

			rows.push(
				(gpui::div().debug_selector(|| "input-receipts-first".into()).child(
					self.workspace_action(
						"first-input-receipts".into(),
						"First unconfirmed inputs".into(),
						move |surface, cx| surface.input_receipt_cursor(&owner, None, cx),
						cx,
					),
				))
				.into_any_element(),
			);
		}

		match &state.result {
			Some(AgentInputReceiptsResult::Available {
				work_id,
				entries,
				next_after,
				shortened,
			}) if work_id.as_str() == work.id => {
				for entry in entries {
					if submission_pending(&work.dispatch_state, entry)
						|| self.preview_covers_receipt(&work.id, entry.id, &entry.text)
					{
						continue;
					}

					rows.push(
						(gpui::div()
							.debug_selector(|| "unconfirmed-native-input".into())
							.child(self.saved_input_card(&work.id, entry, cx)))
						.into_any_element(),
					);
				}

				if *shortened {
					rows.push(
						(agent_surface::muted("Some local input text is shortened."))
							.into_any_element(),
					);
				}

				if let Some(after) = next_after {
					let (owner, after) = (work.id.clone(), *after);

					rows.push(
						(gpui::div().debug_selector(|| "input-receipts-next".into()).child(
							self.workspace_action(
								"next-input-receipts".into(),
								"More unconfirmed inputs".into(),
								move |surface, cx| {
									surface.input_receipt_cursor(&owner, Some(after), cx)
								},
								cx,
							),
						))
						.into_any_element(),
					);
				}

				if entries.is_empty() && state.after.is_some() {
					rows.push(
						(agent_surface::muted("No more unconfirmed inputs on this page."))
							.into_any_element(),
					);
				}
			},
			None => rows.push((ui_loading::loading("Loading delivery records")).into_any_element()),
			_ => rows.push(
				(agent_surface::muted("Delivery records unavailable. Retrying…"))
					.into_any_element(),
			),
		}

		rows
	}
}

pub(super) fn submission_pending(
	state: &AgentDispatchStateDto,
	entry: &AgentHistoryEntryDto,
) -> bool {
	entry.kind != "unsent_input"
		&& matches!(state, AgentDispatchStateDto::Dispatching | AgentDispatchStateDto::Running)
		&& entry
			.receipt
			.as_ref()
			.is_some_and(|receipt| !receipt.disposed && receipt.delivered_turn_id.is_none())
}

fn message_reason(reason: Option<&str>) -> Option<String> {
	reason.map(|reason| {
		if reason.contains("in use elsewhere") {
			"Another app was using this conversation when you sent the message.".into()
		} else {
			reason.strip_prefix("Not sent: ").unwrap_or(reason).to_owned()
		}
	})
}

#[cfg(test)]
mod tests {
	use std::{future, thread};

	use futures_util::{SinkExt as _, StreamExt as _};
	use gpui::{AppContext as _, ParentElement as _, Styled as _};
	use tokio_tungstenite::tungstenite::Message;

	#[cfg(test)] use crate::shell::agent_surface::native_timeline::inputs::Binding;
	use crate::shell::agent_surface::{
		drafts::tests,
		native_timeline::inputs::{AgentInputReceiptsResult, AgentSurface, EntityId},
		wire_test_support,
	};
	use decodex_protocol::{
		AgentActionDto, AgentHistoryEntryDto, AgentHistoryReceiptDto, AgentTimelinePage,
		CURRENT_VERSION, ClientMessage, CommandPayload, DesktopMessage, DesktopMessageState,
		HistoryText, IdempotencyKey, QueryPayload, QueryResultEnvelope, QueryResultPayload,
		ServerId, ServerMessage,
	};

	struct RetryCard(gpui::Entity<AgentSurface>, AgentHistoryEntryDto);
	impl gpui::Render for RetryCard {
		fn render(
			&mut self,
			_: &mut gpui::Window,
			cx: &mut gpui::Context<Self>,
		) -> impl gpui::IntoElement {
			gpui::div()
				.w_full()
				.font_family(crate::ui_theme::FONT_FAMILY)
				.text_size(gpui::px(crate::ui_theme::BODY_SIZE))
				.line_height(gpui::px(crate::ui_theme::BODY_LINE_HEIGHT))
				.child(self.0.update(cx, |s, cx| s.saved_input_card("agent", &self.1, cx)))
		}
	}

	#[gpui::test]
	fn delivery_labels_share_a_baseline_and_the_bubble_text_edge(cx: &mut gpui::TestAppContext) {
		for (width, scale, retry, text) in [
			(280., 1., true, "A short message.".into()),
			(280., 2., true, "请保留完整内容，并检查失败提示和重发入口的对齐。".repeat(12)),
			(740., 2., true, "Keep the full original message readable. ".repeat(12)),
			(740., 2., false, "Keep the full original message readable. ".repeat(12)),
			(880., 1., false, "A short message.".into()),
		] {
			let (_, visual) = cx.add_window_view(|_, cx| {
				let surface = cx.new(AgentSurface::new);
				let entry = surface.update(cx, |s, cx| {
					long_response(s, cx);

					s.uncertain = !retry;

					let AgentInputReceiptsResult::Available { mut entries, .. } =
						page("agent", Some(7), None)
					else {
						panic!("fixture input");
					};
					let mut entry = entries.remove(0);

					entry.kind = "unsent_input".into();
					entry.text = text;
					entry.receipt.as_mut().unwrap().disposed = true;

					entry
				});

				RetryCard(surface, entry)
			});

			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(900.)));
			visual.simulate_scale_factor_change(scale);
			visual.update(crate::ui_frame::draw_frame);

			let message = visual.debug_bounds("saved-input-agent-7-message").unwrap();
			let status = visual.debug_bounds("saved-input-agent-7-status-text").unwrap();
			let action = visual.debug_bounds("saved-input-agent-7-retry-text");

			assert_eq!(action.is_some(), retry);
			assert!(status.top() >= message.bottom(), "receipt belongs below its message");

			let trailing = if let Some(action) = action {
				assert!((status.top() - action.top()).abs() < gpui::px(0.5));
				assert_eq!(status.size.height, action.size.height);
				assert!(action.left() > status.right() + gpui::px(7.));

				action
			} else {
				status
			};

			assert!(
				(trailing.right() - message.right()).abs() < gpui::px(0.5),
				"receipt text must align with bubble text at {width}pt / {scale}x: {trailing:?}, {message:?}"
			);
		}
	}

	#[gpui::test]
	fn local_message_retry_preserves_original_action_and_the_later_editor(
		cx: &mut gpui::TestAppContext,
	) {
		let (_root, profile, _) = tests::profiles();

		for state in [DesktopMessageState::Failed, DesktopMessageState::Unconfirmed] {
			let surface = cx.new(AgentSurface::new);

			surface.update(cx, |s, cx| {
				s.bind_profile(Some(profile.clone()), cx);

				long_response(s, cx);

				s.composer.update(cx, |input, cx| input.set_content("Later input stays here", cx));

				let key = IdempotencyKey::new("original").unwrap();
				let message = DesktopMessage {
					action: AgentActionDto::Send {
						root_id: EntityId::new("agent").unwrap(),
						text: HistoryText::new("Original input").unwrap(),
					},
					owner: Some(EntityId::new("agent").unwrap()),
					thread_id: Some(decodex_protocol::WireText::new("native-thread").unwrap()),
					text: "Original input".into(),
					state,
					error: None,
				};

				s.stage_message(&key, message.clone(), None, cx).unwrap();
				s.retry_local_message(&key, &message, cx);

				let saved = s.retained_messages();

				assert_eq!(saved.len(), 1);
				assert_eq!(saved[0].1.action, message.action);
				assert_eq!(s.composer.read(cx).content(), "Later input stays here");

				if state == DesktopMessageState::Failed {
					assert_ne!(saved[0].0, key);
					assert_eq!(saved[0].1.state, DesktopMessageState::Sending);
					assert!(s.sending && s.submission.command.is_some());
				} else {
					assert_eq!(saved[0].0, key);
					assert!(s.submission.command.is_none() && s.submission.waiting.is_none());
				}
			});
		}
	}

	#[gpui::test]
	fn input_retry_preserves_the_new_editor_and_never_resends_an_unconfirmed_message(
		cx: &mut gpui::TestAppContext,
	) {
		let (_root, profile, _) = tests::profiles();
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			long_response(s, cx);

			s.profile = Some(profile);

			s.composer.update(cx, |input, cx| input.set_content("Later input stays here", cx));

			let AgentInputReceiptsResult::Available { mut entries, .. } =
				page("agent", Some(7), None)
			else {
				panic!("saved input fixture")
			};
			let entry = &mut entries[0];

			assert!(!s.can_retry_saved_input("agent", entry));

			s.retry_saved_input("agent", entry, cx);

			assert!(s.submission.waiting.is_none());

			entry.kind = "unsent_input".into();
			entry.receipt.as_mut().unwrap().disposed = true;

			assert!(s.can_retry_saved_input("agent", entry));

			s.retry_saved_input("agent", entry, cx);

			assert!(s.sending && s.submission.command.is_some());
			assert_eq!(s.submission.unconfirmed.len(), 1);
			assert_eq!(s.composer.read(cx).content(), "Later input stays here");
			assert!(s.retained_messages().is_empty());

			s.submission.waiting = None;
			s.sending = false;

			s.snapshot.as_mut().unwrap().archived_work_ids.insert("agent".into());

			assert!(!s.can_retry_saved_input("agent", entry));
		});
	}

	#[gpui::test]
	fn resend_stays_visible_and_one_click_preserves_the_later_editor(
		cx: &mut gpui::TestAppContext,
	) {
		let (_root, profile, server) = wire_test_support::fixture(|listener| async move {
			let mut socket = wire_test_support::accept(&listener).await;
			let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
				panic!("command");
			};
			let request: ClientMessage = serde_json::from_str(&text).unwrap();
			let ClientMessage::Command(command) = request else {
				panic!("expected resend command, got {request:?}");
			};
			let CommandPayload::Agent { action } = command.payload else {
				panic!("Agent action");
			};

			// A lost response must not make the UI submit a second command.
			socket.close(None).await.unwrap();

			*action
		});
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			let entry = surface.update(cx, |s, cx| {
				long_response(s, cx);

				s.profile = Some(profile);

				s.composer.update(cx, |input, cx| input.set_content("Keep my later input", cx));

				let mut inputs = page("agent", Some(7), None);
				let AgentInputReceiptsResult::Available { entries, .. } = &mut inputs else {
					panic!("inputs");
				};

				entries[0].kind = "unsent_input".into();
				entries[0].receipt.as_mut().unwrap().disposed = true;

				let entry = entries[0].clone();

				s.timeline.native.input_receipts.result = Some(inputs);

				entry
			});

			cx.observe(&surface, |_, _, cx| cx.notify()).detach();

			RetryCard(surface, entry)
		});
		let surface = view.read_with(visual, |view, _| view.0.clone());

		visual.simulate_resize(gpui::size(gpui::px(1_000.), gpui::px(700.)));
		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("saved-input-agent-7-message").is_some());

		let status = visual.debug_bounds("saved-input-agent-7-status").unwrap();

		visual.simulate_click(status.center(), Default::default());
		visual.update(crate::ui_frame::draw_frame);

		assert!(
			visual.debug_bounds("saved-input-agent-7-message").is_some(),
			"clicking the status never hides the original message"
		);

		let retry =
			visual.debug_bounds("saved-input-agent-7-retry").expect("visible one-click resend");

		assert!((status.center().y - retry.center().y).abs() < gpui::px(1.));
		assert_eq!(status.size.height, retry.size.height);

		visual.simulate_click(retry.center(), Default::default());
		visual.run_until_parked();

		let action = server.join().unwrap();

		assert!(matches!(action, AgentActionDto::RetryInput { work_id, thread_id, event_id:7 }
            if work_id.as_str()=="agent" && thread_id.as_str()=="native-thread"));

		surface.read_with(visual, |s, cx| {
			assert_eq!(s.submission.unconfirmed.len(), 1);
			assert_eq!(s.composer.read(cx).content(), "Keep my later input");
		});
	}

	#[gpui::test]
	fn archived_message_does_not_offer_resend_or_an_action_banner(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_000.), gpui::px(700.)));

		surface.update(visual, |s, cx| {
			long_response(s, cx);

			let mut inputs = page("agent", Some(7), None);
			let AgentInputReceiptsResult::Available { entries, .. } = &mut inputs else {
				panic!("inputs");
			};

			entries[0].kind = "unsent_input".into();
			s.timeline.native.input_receipts.result = Some(inputs);

			s.snapshot.as_mut().unwrap().archived_work_ids.insert("agent".into());
			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("saved-input-agent-7-retry").is_none());
		assert!(visual.debug_bounds("saved-input-banner").is_none());

		surface.read_with(visual, |s, _| {
			assert!(s.submission.command.is_none() && s.submission.waiting.is_none());
		});
	}

	#[gpui::test]
	fn returning_to_a_conversation_releases_receipt_reads_and_keeps_loaded_records(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			long_response(s, cx);

			for work in &mut s.snapshot.as_mut().unwrap().work_items {
				if work.id != "agent" {
					work.codex_thread_id = Some(format!("thread-{}", work.id));
				}
			}

			s.timeline.native.requested = Some(("agent".into(), "native-thread".into()));

			let retained = s.timeline.native.input_receipts.result.clone();

			s.timeline.native.input_receipts.task =
				Some(cx.spawn(async |_, _| future::pending::<()>().await));

			s.open_page("release", cx);

			let cached = &s.timeline.native_cache[&("agent".into(), "native-thread".into())];

			assert!(cached.input_receipts.task.is_none(), "a parked reader cannot remain busy");
			assert_eq!(cached.input_receipts.result, retained);

			s.open_page("agent", cx);

			assert!(s.timeline.native.input_receipts.task.is_none());
			assert!(s.native_input_receipts_loaded("agent"));
			assert_eq!(s.timeline.native.input_receipts.result, retained);
		});
	}

	#[gpui::test]
	fn local_receipts_load_before_any_native_history_binding(cx: &mut gpui::TestAppContext) {
		let (_root, profile, server) = wire_test_support::fixture(|listener| async move {
			let mut socket = wire_test_support::accept(&listener).await;
			let Message::Text(text) = socket.next().await.unwrap().unwrap() else {
				panic!("query")
			};
			let ClientMessage::Query(query) = serde_json::from_str(&text).unwrap() else {
				panic!("query")
			};

			assert!(
				matches!(&query.payload, QueryPayload::GetAgentInputReceipts { work_id, after: None } if work_id.as_str() == "agent")
			);

			let response = ServerMessage::QueryResult(QueryResultEnvelope {
				version: CURRENT_VERSION,
				server_id: ServerId::new(wire_test_support::SERVER).unwrap(),
				query_id: query.query_id,
				payload: QueryResultPayload::AgentInputReceipts(page("agent", None, None)),
			});

			socket
				.send(Message::Text(serde_json::to_string(&response).unwrap().into()))
				.await
				.unwrap();
		});
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			s.profile = Some(profile);
			s.timeline.native.requested = Some(("agent".into(), "history-not-loaded".into()));

			assert!(s.timeline.native.binding.is_none());

			s.refresh_native_input_receipts(cx);
		});

		cx.run_until_parked();
		server.join().unwrap();
		cx.run_until_parked();
		surface.read_with(cx, |s, _| {
			assert!(s.timeline.native.binding.is_none());
			assert!(s.native_input_receipts_loaded("agent"));
			assert!(s.timeline.native.input_receipts.task.is_none());
		});
	}

	#[test]
	fn sending_is_not_reported_as_an_unknown_or_failed_delivery() {
		let mut entry = AgentHistoryEntryDto {
			id: 1,
			kind: "user".into(),
			text: "A novel".into(),
			created_at_micros: 1,
			native_source: None,
			turn_id: None,
			weather: vec![],
			activity: None,
			usage: None,
			duration_ms: None,
			receipt: Some(AgentHistoryReceiptDto {
				unsent_reason: None,
				voice_session_id: None,
				event_kind: "async_question_answer".into(),
				delivered_turn_id: None,
				disposed: false,
			}),
		};

		assert!(super::submission_pending(
			&decodex_protocol::AgentDispatchStateDto::Dispatching,
			&entry
		));
		assert!(super::submission_pending(
			&decodex_protocol::AgentDispatchStateDto::Running,
			&entry
		));
		assert!(!super::submission_pending(
			&decodex_protocol::AgentDispatchStateDto::Unknown,
			&entry
		));
		assert!(!super::submission_pending(&decodex_protocol::AgentDispatchStateDto::Idle, &entry));

		entry.kind = "unsent_input".into();

		assert!(!super::submission_pending(
			&decodex_protocol::AgentDispatchStateDto::Running,
			&entry
		));
	}

	#[gpui::test]
	fn completed_answer_stays_close_to_the_composer(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_000.), gpui::px(700.)));
		surface.update(visual, long_response);
		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		for _ in 0..4 {
			visual.update(crate::ui_frame::draw_frame);
		}

		let answer = visual.debug_bounds("native-promotion-content").unwrap();
		let footer = visual.debug_bounds("floating-composer").unwrap();
		let gap = footer.top() - answer.bottom() + gpui::px(crate::ui_theme::COMPOSER_TOP_GAP);

		assert!(
			gap >= gpui::px(16.) && gap <= gpui::px(28.),
			"completed response must retain only normal reading space, got {gap:?}"
		);
	}

	#[gpui::test]
	fn locked_conversation_keeps_failed_message_in_history_and_preserves_the_editor(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_000.), gpui::px(700.)));

		surface.update(visual, |s, cx| {
			long_response(s, cx);

			let decodex_protocol::AgentTimelineContent::Item { text, .. } =
				&mut s.timeline.native.entries[0].content
			else {
				panic!("fixture answer")
			};

			*text = "Enjoy a quiet walk along a familiar route.".into();

			let mut inputs = page("agent", Some(7), None);
			let AgentInputReceiptsResult::Available { entries, .. } = &mut inputs else {
				panic!("fixture inputs")
			};
			let entry = &mut entries[0];

			entry.kind = "unsent_input".into();
			entry.text = "Keep **this** unsent message.".into();
			entry.receipt.as_mut().unwrap().disposed = true;

			let (_, decodex_protocol::AgentHistoryResult::Available { entries: history, .. }) =
				s.history.as_mut().unwrap()
			else {
				panic!("fixture history")
			};

			history.push(entry.clone());
			s.composer.update(cx, |input, cx| input.set_content("An independent draft", cx));

			s.snapshot.as_mut().unwrap().pending_events = vec![writer_in_use_event()];

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		for _ in 0..4 {
			visual.update(crate::ui_frame::draw_frame);
		}

		let card = visual.debug_bounds("message-delivery").unwrap();

		assert!(visual.debug_bounds("composer-saved-inputs").is_none());

		let input = visual.debug_bounds("composer-input-region").unwrap();

		assert!(card.bottom() <= input.top());
		assert!(visual.debug_bounds("composer-blocked").is_some());
		assert!(visual.debug_bounds("composer-editor-area").is_none());
		assert!(visual.debug_bounds("saved-input-agent-7-message").is_some());
		assert!(visual.debug_bounds("saved-input-toggle").is_none());

		surface.update(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "An independent draft");

			s.snapshot.as_mut().unwrap().pending_events.clear();
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("composer-blocked").is_none());
		assert!(visual.debug_bounds("composer-saved-inputs").is_none());

		surface.update(visual, |s, cx| {
			assert_eq!(s.composer.read(cx).content(), "An independent draft");

			s.snapshot.as_mut().unwrap().archived_work_ids.insert("agent".into());
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);

		assert!(visual.debug_bounds("composer-saved-inputs").is_none());
		assert!(visual.debug_bounds("local-receipt-7").is_none());
	}

	fn writer_in_use_event() -> decodex_protocol::AgentPendingEventDto {
		decodex_protocol::AgentPendingEventDto {
			id: 99,
			source_event_id: "external-writer".into(),
			work_item_id: "agent".into(),
			event_kind: "thread_in_use_needs_attention".into(),
			created_at_micros: 1,
			delivery_claimed: false,
		}
	}

	fn long_response(s: &mut AgentSurface, cx: &mut gpui::Context<AgentSurface>) {
		s.visual_workspace_fixture(cx);

		s.workspace.graph_visible = false;

		let work = s
			.snapshot
			.as_mut()
			.unwrap()
			.work_items
			.iter_mut()
			.find(|w| Some(&w.id) == s.selected.as_ref())
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
				entries: vec![
					decodex_protocol::AgentTimelineEntry {
						position: 0,
						content: decodex_protocol::AgentTimelineContent::Item {
							turn_id: "turn".into(),
							item_id: "answer".into(),
							kind: "agentMessage".into(),
							text: "A paragraph in the completed response.\n\n".repeat(40),
							truncated: false,
							phase: None,
							collaboration: None,
							app_ui: false,
							activity: None,
							attachments: vec![],
						},
					},
					decodex_protocol::AgentTimelineEntry {
						position: 1,
						content: decodex_protocol::AgentTimelineContent::TurnBoundary {
							turn_id: "turn".into(),
							completed: true,
							status: Some("completed".into()),
							duration_ms: Some(1_200),
							usage: None,
							usage_summary: None,
							error: None,
						},
					},
				],
				next_cursor: None,
				weather: Default::default(),
				thread_token_total: None,
				thread_metrics: None,
				safety_buffering_turn_id: None,
				active_realtime_session_at_page_start: None,
			},
		);

		s.timeline.native.input_receipts.result = Some(page("agent", None, None));

		let (_, decodex_protocol::AgentHistoryResult::Available { entries, .. }) =
			s.history.as_mut().unwrap()
		else {
			panic!("fixture history")
		};

		entries.clear();

		s.timeline.latest_follow_work = s.selected.clone();

		cx.notify();
	}

	#[gpui::test]
	fn failed_message_wraps_and_scrolls_with_history_instead_of_covering_the_editor(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_000.), gpui::px(700.)));

		surface.update(visual, |s, cx| {
			long_response(s, cx);

			let mut inputs = page("agent", Some(7), None);
			let AgentInputReceiptsResult::Available { entries, .. } = &mut inputs else {
				panic!("inputs")
			};

			entries[0].kind = "unsent_input".into();
			entries[0].text =
				"A complete original sentence wraps in the message bubble without being cut off. "
					.repeat(12);
			s.timeline.native.input_receipts.result = Some(inputs);

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_frame);

		thread::sleep(std::time::Duration::from_millis(240));

		visual.update(crate::ui_frame::draw_frame);

		let frame = visual.debug_bounds("message-delivery").unwrap();
		let message = visual.debug_bounds("saved-input-agent-7-message").unwrap();
		let footer = visual.debug_bounds("composer-input-region").unwrap();

		assert!(message.left() > frame.left());
		assert!(message.right() <= frame.right() - gpui::px(8.));
		assert!(
			message.size.height > gpui::px(144.),
			"the full long message stays in the transcript"
		);
		assert!(frame.bottom() <= footer.top(), "failed input must not cover the composer");
		assert!(visual.debug_bounds("composer-saved-inputs").is_none());
		assert!(visual.debug_bounds("expandable-notice-card").is_none());
		assert!(visual.debug_bounds("saved-input-toggle").is_none());
		assert!(visual.debug_bounds("saved-input-agent-7-copy").is_none());

		surface.update(visual, |s, _| {
			s.timeline.latest_follow_work = None;

			s.timeline.follow_paused.insert("agent".into());

			let scroll = &s.timeline.scroll["agent"];

			scroll.set_offset(gpui::point(gpui::px(0.), scroll.offset().y + gpui::px(80.)));
		});

		visual.update(crate::ui_frame::draw_frame);

		let moved = visual.debug_bounds("message-delivery").unwrap();

		assert!(
			moved.top() > frame.top() + gpui::px(60.),
			"the failure bubble must scroll with messages"
		);
		assert_eq!(visual.debug_bounds("composer-input-region").unwrap().bottom(), footer.bottom());
	}

	fn page(work: &str, id: Option<i64>, next_after: Option<i64>) -> AgentInputReceiptsResult {
		AgentInputReceiptsResult::Available {
			work_id: EntityId::new(work).unwrap(),
			entries: id
				.into_iter()
				.map(|id| AgentHistoryEntryDto {
					native_source: None,
					turn_id: None,
					weather: Vec::new(),
					receipt: Some(AgentHistoryReceiptDto {
						unsent_reason: None,
						voice_session_id: None,
						event_kind: "user_message".into(),
						delivered_turn_id: None,
						disposed: false,
					}),
					activity: None,
					usage: None,
					duration_ms: None,
					id,
					kind: "user".into(),
					text: "Old unconfirmed input".into(),
					created_at_micros: 1,
				})
				.collect(),
			next_after,
			shortened: false,
		}
	}

	#[gpui::test]
	fn native_pending_input_controls_page_independently_of_the_transcript(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.update(|window, _| window.resize(gpui::size(gpui::px(1_000.), gpui::px(700.))));

		let work = surface.update(visual, |surface, cx| {
			surface.visual_workspace_fixture(cx);

			surface.workspace.graph_visible = false;

			let work = surface
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|work| Some(&work.id) == surface.selected.as_ref())
				.unwrap();

			work.codex_thread_id = Some("thread".into());

			let id = work.id.clone();

			assert!(surface.timeline.native.replace(
				Binding { work: id.clone(), thread: "thread".into(), account: "account".into() },
				AgentTimelinePage {
					thread_id: "thread".into(),
					entries: vec![],
					next_cursor: None,
					weather: Default::default(),
					thread_token_total: None,
					thread_metrics: None,
					safety_buffering_turn_id: None,
					active_realtime_session_at_page_start: None
				}
			));

			surface.timeline.native.input_receipts.result = Some(page(&id, Some(1), Some(1)));

			cx.notify();

			id
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("unconfirmed-native-input").is_some());

		let more = visual.debug_bounds("input-receipts-next").unwrap();

		visual.simulate_click(more.center(), Default::default());

		surface.update(visual, |surface, cx| {
			assert_eq!(surface.timeline.native.input_receipts.after, Some(1));
			assert!(surface.timeline.native.older_cursor.is_none());

			surface.timeline.native.input_receipts.result = Some(page(&work, Some(2), None));

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("input-receipts-next").is_none());

		let first = visual.debug_bounds("input-receipts-first").unwrap();

		visual.simulate_click(first.center(), Default::default());

		surface.update(visual, |surface, cx| {
			assert_eq!(surface.timeline.native.input_receipts.after, None);

			surface.timeline.native.input_receipts.result = Some(page(&work, None, None));

			assert!(surface.native_input_receipts_loaded(&work));

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("unconfirmed-native-input").is_none());
	}
}
