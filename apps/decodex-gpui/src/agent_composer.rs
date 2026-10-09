//! Compact Agent composer. Controls apply to the next submitted message.
#[path = "agent_composer_controls.rs"] mod controls;
#[path = "agent_task_references.rs"] mod task_references;

use std::{
	env,
	f32::consts::{FRAC_PI_2, TAU},
	fs::{DirBuilder, OpenOptions},
	io::{Error, Result, Write},
	os::unix::fs::{DirBuilderExt, OpenOptionsExt},
	time::{Duration, Instant},
};

use gpui::{
	AnyElement, AppContext as _, ClipboardEntry, Div, ExternalPaths, FontWeight, Image,
	ImageFormat, KeyDownEvent, MouseButton, MouseDownEvent, PathBuilder, PathPromptOptions,
	Stateful,
};
use tokio::{runtime::Builder, time};
use ui_theme::{BLUE, CONTROL_SIZE, HOVER_FILL, SELECTED_HOVER_FILL, TEXT, TEXT_MUTED};

#[cfg(test)] use crate::shell::agent_surface::ConversationModel;
#[cfg(not(test))] use crate::shell::agent_surface::prompts;
use crate::{
	shell::{
		agent_surface::{
			self, AgentActionDto, AgentClient, AgentCommandResponse, AgentDispatchStateDto,
			AgentHistoryResult, AgentSnapshotResult, AgentSurface, ClipboardItem, ComposerInput,
			Context, ConversationReasoningEffort, ConversationWorkingDirectory, Entity, EntityId,
			FluentBuilder, HistoryText, IdempotencyKey, InteractiveElement, IntoElement,
			ParentElement, Render, Role, SharedString, SmoothControl, StatefulInteractiveElement,
			Styled, SubmitComposer, Window, model_settings, ui_theme,
		},
		workspace_symbols,
		workspace_symbols::Symbol,
	},
	ui_motion,
};
use controls::{PrimaryMark, PrimaryMode};
use decodex_protocol::AgentAttachmentDto;

struct ComposerTip(String);
impl Render for ComposerTip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		crate::ui_motion::tooltip_surface(gpui::div())
			.px_3()
			.py_2()
			.text_size(gpui::px(11.0))
			.text_color(gpui::rgb(TEXT))
			.child(self.0.clone())
	}
}

impl AgentSurface {
	pub(super) fn running_turn(&self) -> Option<(EntityId, crate::shell::agent_surface::WireText)> {
		if self.native_agents.selected.as_ref().is_some_and(|(owner, thread)| {
			self.snapshot
				.as_ref()
				.and_then(|s| s.work_items.iter().find(|w| &w.id == owner))
				.is_none_or(|w| w.codex_thread_id.as_ref() != Some(thread))
		}) {
			return None;
		}
		let snapshot = self.snapshot.as_ref()?;
		let selected = self.selected.as_ref()?;
		let work = snapshot.work_items.iter().find(|work| &work.id == selected)?;

		if work.dispatch_state != AgentDispatchStateDto::Running {
			return None;
		}

		Some((
			EntityId::new(work.id.clone()).ok()?,
			crate::shell::agent_surface::WireText::new(work.active_turn_id.clone()?).ok()?,
		))
	}

	pub(super) fn configured_send(
		&self,
		root_id: EntityId,
		text: HistoryText,
		attachments: Vec<AgentAttachmentDto>,
	) -> AgentActionDto {
		if let Some((work_id, turn_id)) =
			self.running_turn().filter(|(id, _)| self.steer && id == &root_id)
		{
			AgentActionDto::Steer {
				work_id,
				turn_id,
				text,
				attachments,
				task_references: self.task_references.clone(),
			}
		} else {
			AgentActionDto::SendConfigured {
				execution: self.draft_profiles.execution.choice(root_id.as_str()),
				root_id,
				text,
				attachments,
				task_references: self.task_references.clone(),
			}
		}
	}

	fn awaiting_start(&self, cx: &Context<Self>) -> bool {
		// Steer submits into an existing turn; it must not show new-turn startup UI.
		(self.sending && self.workspace.opening_work.is_none() && self.running_turn().is_none())
			|| (self.running_turn().is_none()
				&& self.composer.read(cx).content().trim().is_empty()
				&& (self.feedback == "Message saved · Waiting for agent…"
					|| self.snapshot.as_ref().is_some_and(|snapshot| {
						snapshot.work_items.iter().any(|work| {
							Some(&work.id) == self.selected.as_ref()
								&& work.dispatch_state == AgentDispatchStateDto::Dispatching
						}) || snapshot.pending_events.iter().any(|event| {
							Some(&event.work_item_id) == self.selected.as_ref()
								&& event.event_kind == "user_message"
								&& !event.delivery_claimed
						})
					})))
	}

	fn stop_button(&self, cx: &Context<Self>) -> bool {
		self.awaiting_start(cx)
			|| self.interrupting.as_ref().is_some_and(|(id, _)| self.selected.as_ref() == Some(id))
			|| self.escape_stop_armed()
			|| (self.running_turn().is_some()
				&& self.composer.read(cx).content().trim().is_empty()
				&& self.attachments.is_empty()
				&& self.task_references.is_empty())
	}

	fn escape_stop_armed(&self) -> bool {
		self.escape_stop.as_ref().is_some_and(|(work, turn, at)| {
			at.elapsed() < Duration::from_secs(2)
				&& self
					.running_turn()
					.is_some_and(|(w, t)| w.as_str() == work && t.as_str() == turn)
		})
	}

	pub(crate) fn escape_interrupt(&mut self, cx: &mut Context<Self>) {
		if self.composer.read(cx).is_composing() {
			return;
		}
		if self.composer_menu.take().is_some() || self.dictation.is_some() {
			self.cancel_dictation(cx);

			self.escape_stop = None;
			self.effort_drag = None;
			self.effort_pointer = None;

			cx.notify();

			return;
		}
		if !self.command_connection_ready() {
			self.escape_stop = None;

			return;
		}
		if self.escape_stop_armed() {
			self.escape_stop = None;

			self.interrupt_current(cx);

			return;
		}

		let Some((work, turn)) = self.running_turn() else {
			self.escape_stop = None;

			return;
		};
		let armed = (work.as_str().to_owned(), turn.as_str().to_owned(), Instant::now());

		self.escape_stop = Some(armed.clone());
		cx.spawn(async move |owner, cx| {
			cx.background_executor().timer(Duration::from_secs(2)).await;

			let _ = owner.update(cx, |s, cx| {
				if s.escape_stop.as_ref() == Some(&armed) {
					s.escape_stop = None;

					cx.notify();
				}
			});
		})
		.detach();

		cx.notify();
	}

	pub(crate) fn interrupt_current(&mut self, cx: &mut Context<Self>) {
		self.escape_stop = None;

		if !self.command_connection_ready() {
			return;
		}

		if let Some((work_id, turn_id)) = self.running_turn() {
			self.execute(AgentActionDto::Interrupt { work_id, turn_id }, None, cx);
		}
	}

	pub(super) fn request_interrupt(
		&mut self,
		work_id: EntityId,
		turn_id: decodex_protocol::WireText,
		cx: &mut Context<Self>,
	) {
		if self.interrupting.is_some() {
			return;
		}

		let Some(profile) = self.profile.clone() else {
			self.feedback = "No service profile is configured.".into();

			cx.notify();

			return;
		};
		let target = (work_id.as_str().to_owned(), turn_id.as_str().to_owned());

		self.interrupting = Some(target.clone());

		let target_for_readback = target.clone();
		let request = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread()
				.enable_all()
				.build()
				.map_err(|_| "Cannot start cancellation".to_string())?;

			runtime.block_on(async {
				let client = AgentClient::new(profile);
				let result = client
					.execute(
						AgentActionDto::Interrupt { work_id, turn_id },
						IdempotencyKey::new(agent_surface::unique_command()).expect("command identity"),
					)
					.await;
				// The turn can finish before interruption reaches Codex. Read back before
				// presenting an error, and never reuse the send/uncertain-delivery state.
				let mut snapshot = client.query().await.ok();

                if !matches!(&result, Ok(AgentCommandResponse::Accepted { .. })) {
                    for _ in 0..2 {
                        let ended = matches!(&snapshot, Some(AgentSnapshotResult::Available(s)) if s.work_items.iter().any(|w| w.id == target_for_readback.0 && w.active_turn_id.as_deref() != Some(target_for_readback.1.as_str())));

                        if ended { break; }

                        time::sleep(Duration::from_millis(80)).await;

                        snapshot = client.query().await.ok();
                    }
                }

				Ok::<_, String>((result, snapshot))
			})
		});

		self.interrupt_task = Some(cx.spawn(async move |surface, cx| {
            let result = request.await;
            let _ = surface.update(cx, |s, cx| {
                if s.interrupting.as_ref() != Some(&target) { return; }

                let accepted = match result {
                    Ok((result, snapshot)) => {
                        if let Some(snapshot) = snapshot { s.apply_result(Ok(snapshot)); }

                        matches!(result, Ok(AgentCommandResponse::Accepted { .. }))
                    }

                    Err(_) => false,
                };

                if !accepted && s.interrupting.as_ref() == Some(&target) {
                    s.interrupting = None;
                    s.feedback = "Stopping could not be confirmed. If the response is still running, press Stop again.".into();
                }

                s.load_history(cx);
                cx.notify();
            });
        }));

		cx.notify();
	}

	pub(super) fn refresh_prompt(cx: &mut Context<Self>) {
		#[cfg(not(test))]
		cx.background_executor().spawn(async { prompts::refresh_cache() }).detach();
		#[cfg(test)]
		let _ = cx;
	}

	pub(super) fn render_composer(
		&self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		#[cfg(all(target_os = "macos", not(test)))]
		if self.native_composer.enabled {
			return self.render_native_composer_anchor(cx);
		}

		gpui::div()
			.w_full()
			.px(gpui::px(crate::ui_theme::CONVERSATION_INSET))
			.pt(gpui::px(crate::ui_theme::COMPOSER_TOP_GAP))
			.pb(gpui::px(crate::ui_theme::COMPOSER_BOTTOM_GAP))
			.flex()
			.flex_col()
			.items_center()
			.child(
				self.render_composer_capsule(false, window, cx)
					.child(self.render_composer_popover(cx)),
			)
			.into_any_element()
	}

	pub(super) fn render_composer_capsule(
		&self,
		native: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let editor = gpui::div()
			.id("composer-editor-area")
			.debug_selector(|| "composer-editor-area".into())
			.flex()
			.flex_col()
			.flex_1()
			.when(self.voice.is_none(), |d| d.flex_none().w_full())
			.min_w_0()
			.when(native, |d| {
				d.on_mouse_down(
					MouseButton::Left,
					cx.listener(|s, _, _, cx| {
						s.composer_menu = None;

						cx.notify();
					}),
				)
			})
			.on_action(cx.listener(|s, _: &SubmitComposer, _, cx| {
				s.submit(cx);
				cx.stop_propagation();
			}))
			.map(|editor| {
				if let Some(waveform) = self.voice_controls(window, cx) {
					editor.child(waveform)
				} else {
					editor.child(self.conversation_composer().clone())
				}
			});

		gpui::div()
			.id("agent-composer")
			.debug_selector(|| "agent-composer".into())
			.occlude()
			.relative()
			.w_full()
			.max_w(gpui::px(crate::ui_theme::CONVERSATION_WIDTH))
			.min_w_0()
			.px(gpui::px(10.))
			.py(gpui::px(7.))
			.rounded(gpui::px(ui_theme::COMPOSER_RADIUS))
			.when(!native, |d| d.bg(gpui::rgb(0x27272b)))
			.when(!native, |d| {
				d.shadow(vec![gpui::BoxShadow {
					inset: false,
					color: gpui::rgba(0x0000001a).into(),
					offset: gpui::point(gpui::px(0.), gpui::px(4.)),
					blur_radius: gpui::px(16.),
					spread_radius: gpui::px(-5.),
				}])
			})
			.flex()
			.flex_col()
			.gap(gpui::px(4.))
			.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
				if e.keystroke.key == "escape" && !e.keystroke.modifiers.shift {
					if !e.is_held {
						s.escape_interrupt(cx);
					}

					cx.stop_propagation();
				}
			}))
			.on_drop(cx.listener(|s, paths: &ExternalPaths, _, cx| {
				if s.native_agents.selected.is_none() {
					s.attach_paths(paths.0.to_vec(), cx);
				}
			}))
			.when(self.native_agents.selected.is_none(), |d| {
				d.children(self.attachment_row(cx)).children(self.task_reference_row(cx))
			})
			.map(|capsule| {
				let controls = gpui::div()
					.id("composer-action-row")
					.debug_selector(|| "composer-action-row".into())
					.w_full()
					.min_w_0()
					.flex()
					.items_center()
					.gap(gpui::px(4.))
					.when(self.native_agents.selected.is_none(), |d| {
						d.child(self.composer_control(
							"attach",
							"+".into(),
							"Attachments, skills and microphone",
							|s, cx| s.toggle_composer_menu("attachments", cx),
							cx,
						))
					});
				if self.voice.is_some() {
					capsule.child(controls.child(editor).child(self.composer_toolbar(cx)))
				} else {
					capsule
						.when(
							!(self.native_agents.selected.is_some()
								&& matches!(
									self.native_agents.connection,
									super::native_agents::NativeConnection::ParentManaged
								)),
							|capsule| capsule.child(editor),
						)
						.child(controls.child(self.composer_toolbar(cx)))
				}
			})
	}

	pub(super) fn render_composer_popover(&self, cx: &mut Context<Self>) -> impl IntoElement {
		if self.workspace_connecting() {
			return gpui::div();
		}
		let menu = self.composer_menu.or(self.composer_menu_content);
		let left = matches!(
			menu,
			Some("attachments" | "microphone" | "tasks" | "skills" | "agent-settings")
		);

		gpui::div().absolute().inset_0().child(
			gpui::deferred(
				gpui::div()
					.absolute()
					.bottom(gpui::relative(1.))
					.mb(gpui::px(if left { 8. } else { 10. }))
					.when(left, |d| d.left(gpui::px(0.)))
					// Align with the model trigger: inset + mic/send widths + toolbar gaps.
					.when(menu == Some("model"), |d| d.left(gpui::px(36.)))
					.when(!left && menu != Some("model"), |d| d.right(gpui::px(70.)))
					.w(gpui::px(if matches!(menu, Some("agent-settings" | "skills")) {
						380.
					} else if left {
						280.
					} else {
						232.
					}))
					.child(
						ui_motion::popover(
							"composer-popover-motion",
							self.composer_menu.is_some(),
							self.composer_options(cx)
								.unwrap_or_else(|| gpui::div().into_any_element()),
						)
						.unframed(menu == Some("model")),
					),
			)
			.priority(2),
		)
	}

	fn attachment_options(&self, cx: &mut Context<Self>) -> AnyElement {
		let device =
			if self.audio_input.is_empty() { "System default" } else { self.audio_input.as_str() };

		gpui::div()
			.flex()
			.flex_col()
			.gap(gpui::px(3.))
			.child(self.composer_control(
				"attachment-item",
				"Add attachments…".into(),
				"Add attachments",
				|s, cx| {
					s.composer_menu = None;

					s.pick_attachments(cx);
				},
				cx,
			))
			.child(self.composer_control(
				"skill-item",
				"Use skill…".into(),
				"Select a native skill",
				|s, cx| s.open_skill_picker(cx),
				cx,
			))
			.child(self.composer_control(
				"task-recap-item",
				"Task recap".into(),
				"Task recap",
				|s, cx| {
					s.composer_menu = None;
					s.workspace.details_visible = true;

					if let Some(work) = s.selected.clone() {
						s.open_recap(&work, cx);
					}

					cx.notify();
				},
				cx,
			))
			.child(self.composer_control(
				"task-reference-item",
				"Reference task…".into(),
				"Select a task to read",
				|s, cx| {
					s.toggle_composer_menu("tasks", cx);
				},
				cx,
			))
			.child(
				gpui::div()
					.flex()
					.flex_col()
					.child(self.composer_control_with_window(
						"audio-item",
						device.to_owned(),
						"Choose microphone",
						|s, window, cx| s.open_audio_menu(window, cx),
						cx,
					))
					.child(ui_motion::disclosure(
						"microphone-devices-disclosure",
						self.composer_menu == Some("microphone"),
						gpui::div().pl(gpui::px(26.)).child(self.audio_palette(cx)),
					)),
			)
			.child(self.composer_control(
				"agent-settings",
				"Agent settings…".into(),
				"Agent settings",
				|s, cx| {
					s.workspace.setup_expanded = true;

					s.toggle_composer_menu("agent-settings", cx);
				},
				cx,
			))
			.child(self.composer_control(
				"delivery",
				if self.steer { "Steer" } else { "Queue" }.into(),
				if self.steer { "Add to the current turn" } else { "Send after the current turn" },
				|s, cx| {
					s.steer = !s.steer;

					cx.notify();
				},
				cx,
			))
			.into_any_element()
	}

	fn composer_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
		if self.native_agents.selected.is_some() {
			use super::native_agents::NativeConnection;
			let mut row = gpui::div()
				.flex_1()
				.min_w_0()
				.h(gpui::px(CONTROL_SIZE))
				.flex()
				.items_center()
				.gap(gpui::px(8.));
			match &self.native_agents.connection {
				NativeConnection::ParentManaged => {
					row = row
						.child(
							gpui::div()
								.flex_1()
								.min_w_0()
								.text_size(gpui::px(11.))
								.text_color(gpui::rgb(TEXT_MUTED))
								.child("Message this agent through its parent."),
						)
						.child(self.workspace_action(
							"native-open-parent".into(),
							"Open parent".into(),
							|s, cx| s.open_native_parent(cx),
							cx,
						));
				},
				NativeConnection::Failed(reason) => {
					row = row
						.child(
							gpui::div()
								.flex_1()
								.min_w_0()
								.text_size(gpui::px(11.))
								.text_color(gpui::rgb(TEXT_MUTED))
								.child(reason.clone()),
						)
						.child(self.workspace_action(
							"native-connect-retry".into(),
							"Retry".into(),
							|s, cx| s.retry_native_connection(cx),
							cx,
						));
				},
				NativeConnection::Checking { .. } => {},
				NativeConnection::Ready => {
					row = row.justify_end().child(
						if self.running_turn().is_some()
							&& self.conversation_composer().read(cx).content().trim().is_empty()
						{
							self.composer_control(
								"stop",
								"".into(),
								"Stop",
								|s, cx| s.interrupt_current(cx),
								cx,
							)
							.into_any_element()
						} else {
							self.composer_control(
								"send",
								"".into(),
								"Send · Enter",
								|s, cx| s.send_native_agent(cx),
								cx,
							)
							.into_any_element()
						},
					);
				},
			}
			return row.into_any_element();
		}

		if let Some(controls) = self.dictation_controls(cx) {
			return controls;
		}
		if let Some(controls) = self.voice_toolbar(cx) {
			return controls;
		}

		self.text_composer_toolbar(cx)
	}

	fn text_composer_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
		let model = self.composer_model_label(cx);

		gpui::div()
			.flex_1()
			.min_w_0()
			.flex()
			.items_center()
			.gap(gpui::px(4.0))
			.child(self.composer_control(
				"model",
				model,
				"Model and reasoning · Applies to the next turn",
				|s, cx| s.toggle_composer_menu("model", cx),
				cx,
			))
			.child(gpui::div().flex_1())
			.children(self.usage_line(cx))
			.child(self.composer_control_with_window(
				"dictation",
				"".into(),
				"Dictate into the draft",
				|s, w, cx| s.start_dictation(w, cx),
				cx,
			))
			.child(self.composer_control_with_window(
				"send",
				"".into(),
				if self.interrupting.is_some() {
					"Stopping response"
				} else if self.awaiting_start(cx) {
					"Starting response"
				} else if self.stop_button(cx) {
					"Stop response · Esc twice"
				} else if self.composer.read(cx).content().trim().is_empty()
					&& self.attachments.is_empty()
					&& self.task_references.is_empty()
				{
					"Start voice chat"
				} else {
					"Send · Enter"
				},
				|s, window, cx| {
					if s.awaiting_start(cx) || s.interrupting.is_some() {
						return;
					}
					if s.stop_button(cx) {
						s.interrupt_current(cx);
					} else if s.composer.read(cx).content().trim().is_empty()
						&& s.attachments.is_empty()
						&& s.task_references.is_empty()
					{
						s.start_voice(window, cx);
					} else {
						s.submit(cx);
					}
				},
				cx,
			))
			.into_any_element()
	}

	pub(super) fn composer_control(
		&self,
		id: &'static str,
		label: String,
		tip: &'static str,
		action: fn(&mut Self, &mut Context<Self>),
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		self.composer_control_with_window(id, label, tip, move |s, _, cx| action(s, cx), cx)
	}

	pub(super) fn composer_control_with_window(
		&self,
		id: &'static str,
		label: String,
		tip: &'static str,
		action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Copy + 'static,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let send = id == "send";
		let disabled = send && self.composer_unavailable_reason().is_some();
		let menu_active = self.composer_menu == Some(id);
		let target = cx.entity().downgrade();
		let tooltip = if disabled {
			"Sending paused until the connection is restored".to_owned()
		} else if id == "model" {
			format!("Model and reasoning · {label}")
		} else {
			tip.to_owned()
		};

		gpui::div()
			.id(SharedString::from(format!("composer-{id}")))
			.debug_selector(move || format!("composer-{id}"))
			.role(Role::Button)
			.tab_index(if disabled { -1 } else { 0 })
			.aria_label(tooltip.clone())
			.h(gpui::px(CONTROL_SIZE))
			.px(gpui::px(6.0))
			.flex_none()
			.rounded(gpui::px(if send { 8.0 } else { 7.0 }))
			.when(
				![
					"model",
					"delivery",
					"effort",
					"send",
					"dictation-cancel",
					"dictation-finish",
					"voice-mute",
					"voice-end",
					"attachment-item",
					"audio-item",
				]
				.contains(&id),
				|d| d.w(gpui::px(CONTROL_SIZE)).px_0(),
			)
			.flex()
			.items_center()
			.justify_center()
			.text_size(gpui::px(12.0))
			.line_height(gpui::px(16.0))
			.text_color(gpui::rgb(if send { TEXT } else { TEXT_MUTED }))
			.when(id == "model", |d| {
				d.px(gpui::px(4.))
					.text_size(gpui::px(11.))
					.font_weight(FontWeight::NORMAL)
					.flex_shrink(1.)
					.min_w_0()
					.max_w(gpui::px(220.))
					.overflow_hidden()
			})
			.when(["attachment-item", "audio-item", "delivery"].contains(&id), |d| {
				d.w_full().h(gpui::px(32.)).justify_start().text_size(gpui::px(12.))
			})
			.when(self.composer_menu == Some(id), |d| d.bg(gpui::rgba(0xffffff12)))
			.when(send, |d| {
				d.w(gpui::px(28.)).h(gpui::px(28.)).rounded_full().bg(gpui::rgb(0x515155))
			})
			.when(id == "audio-item", |d| d.aria_expanded(self.composer_menu == Some("microphone")))
			.when(disabled, |d| d.opacity(0.35))
			.when(!disabled, |d| d.cursor_pointer())
			.hover(move |d| {
				if disabled {
					return d;
				}
				d.bg(if send {
					gpui::rgb(0x606064)
				} else {
					gpui::rgba(if menu_active { SELECTED_HOVER_FILL } else { HOVER_FILL })
				})
			})
			.focus(|d| d.bg(gpui::rgba(SELECTED_HOVER_FILL)))
			.when(
				![
					"model",
					"effort",
					"attachment-item",
					"audio-item",
					"skill-item",
					"task-recap-item",
					"task-reference-item",
					"agent-settings",
				]
				.contains(&id),
				|d| d.tooltip(move |_, cx| cx.new(|_| ComposerTip(tooltip.clone())).into()),
			)
			.on_click(cx.listener(move |s, _, window, cx| {
				if !disabled {
					action(s, window, cx);
				}
			}))
			.on_key_down(cx.listener(move |s, e: &KeyDownEvent, window, cx| {
				if !disabled && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
					action(s, window, cx);

					cx.stop_propagation();
				}
			}))
			.child(self.composer_control_content(id, label, cx))
			.when(["model", "effort", "attach"].contains(&id), |d| {
				d.child(
					gpui::canvas(
						move |bounds, _, cx| {
							let _ = target.update(cx, |s, _| {
								s.menu_trigger_bounds.insert(id, bounds);
							});
						},
						|_, _, _, _| {},
					)
					.absolute()
					.inset_0(),
				)
			})
			.smooth()
	}

	fn composer_control_content(&self, id: &str, label: String, cx: &Context<Self>) -> AnyElement {
		match id {
			"send" => PrimaryMark {
				mode: if self.native_agents.selected.is_some() {
					PrimaryMode::Send
				} else if self.dictation.is_some() {
					PrimaryMode::Done
				} else if self.stop_button(cx) {
					PrimaryMode::Stop
				} else if self.composer.read(cx).content().trim().is_empty()
					&& self.attachments.is_empty()
					&& self.task_references.is_empty()
				{
					PrimaryMode::Live
				} else {
					PrimaryMode::Send
				},
				armed: self.escape_stop_armed(),
				pending: self.awaiting_start(cx) || self.interrupting.is_some(),
			}
			.into_any_element(),
			"attach" => workspace_symbols::icon(Symbol::Plus),
			"attachment-item" => gpui::div()
				.flex()
				.items_center()
				.gap(gpui::px(10.))
				.child(workspace_symbols::icon(Symbol::Plus))
				.child("Add attachments…")
				.into_any_element(),
			"audio-item" => gpui::div()
				.w_full()
				.flex()
				.items_center()
				.gap(gpui::px(10.))
				.child(workspace_symbols::icon(Symbol::Microphone))
				.child("Microphone")
				.child(gpui::div().flex_1())
				.child(
					gpui::div()
						.max_w(gpui::px(110.))
						.text_ellipsis()
						.text_color(gpui::rgb(TEXT_MUTED))
						.child(label),
				)
				.child(workspace_symbols::disclosure_chevron(
					"microphone-chevron",
					self.composer_menu == Some("microphone"),
				))
				.into_any_element(),
			"dictation" => workspace_symbols::icon_sized(Symbol::Microphone, 18.),
			"delivery" => gpui::div()
				.w_full()
				.flex()
				.items_center()
				.gap(gpui::px(10.))
				.child(gpui::div().w(gpui::px(16.)).flex_none())
				.child("Send mode")
				.child(gpui::div().flex_1())
				.child(gpui::div().text_color(gpui::rgb(TEXT)).child(label))
				.child(gpui::div().w(gpui::px(12.)).flex_none())
				.into_any_element(),
			"model" => gpui::div()
				.flex()
				.items_center()
				.gap(gpui::px(2.))
				.whitespace_nowrap()
				.text_color(gpui::rgb(TEXT))
				.when(self.fast, |d| {
					d.child(
						gpui::div()
							.text_color(gpui::rgb(BLUE))
							.child(workspace_symbols::icon(Symbol::Fast)),
					)
				})
				.child(gpui::div().max_w(gpui::px(180.)).text_ellipsis().child(label))
				.child(gpui::div().text_color(gpui::rgb(TEXT_MUTED)).child("·"))
				.child(controls::effort_indicator(&self.composer_effort_value()))
				.into_any_element(),
			_ => gpui::div().child(label).into_any_element(),
		}
	}

	fn toggle_composer_menu(&mut self, name: &'static str, cx: &mut Context<Self>) {
		self.escape_stop = None;

		let same_menu = self.composer_menu == Some(name)
			|| (name == "attachments" && self.composer_menu == Some("microphone"));

		self.composer_menu = if same_menu { None } else { Some(name) };

		if self.composer_menu.is_some() {
			self.composer_menu_content = self.composer_menu;

			self.load_capabilities(cx);
			self.refresh_composer_model_settings(cx);
		}

		cx.notify();
	}

	fn composer_options(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
		// Keep content mounted while the disclosure animates closed.
		let menu = self.composer_menu.or(self.composer_menu_content)?;
		let anchor = self.menu_trigger_bounds.get("attach").copied();
		#[cfg(all(target_os = "macos", not(test)))]
		let anchor =
			if self.native_composer.enabled { self.native_composer.bounds } else { anchor };
		// The native composer is a child window. Its parent-space anchor, rather than
		// the child control bounds, gives the space available above the popover.
		let settings_height =
			anchor.map_or(320., |bounds| (f32::from(bounds.origin.y) - 32.).clamp(1., 480.));

		Some(
			gpui::div()
				.id("composer-menu-popover")
				.occlude()
				.on_key_down(cx.listener(|s, event: &KeyDownEvent, _, cx| {
					if event.keystroke.key == "escape" {
						s.composer_menu = None;
						s.effort_drag = None;
						s.effort_pointer = None;

						cx.notify();
						cx.stop_propagation();
					}
				}))
				.on_mouse_down_out(cx.listener(|s, event: &MouseDownEvent, _, cx| {
					let trigger_hit = {
						#[cfg(all(target_os = "macos", not(test)))]
						let same_window = !s.native_composer.enabled;
						#[cfg(not(all(target_os = "macos", not(test))))]
						let same_window = true;

						same_window
							&& s.menu_trigger_bounds
								.values()
								.any(|bounds| bounds.contains(&event.position))
					};

					if trigger_hit {
						return;
					}

					s.composer_menu = None;
					s.effort_drag = None;
					s.effort_pointer = None;

					cx.notify();
				}))
				.p(gpui::px(if menu == "model" { 0. } else { 8. }))
				.w_full()
				.flex()
				.flex_col()
				.gap(gpui::px(10.))
				.child(if menu == "tasks" {
					self.task_reference_options(cx)
				} else if menu == "skills" {
					self.skill_options(cx)
				} else if menu == "agent-settings" {
					gpui::div()
						.id("agent-settings-scroll")
						.max_h(gpui::px(settings_height))
						.overflow_y_scroll()
						.child(self.render_preferences(cx))
						.into_any_element()
				} else if matches!(menu, "attachments" | "microphone") {
					self.attachment_options(cx)
				} else {
					gpui::div()
						.flex()
						.flex_col()
						.gap(gpui::px(6.))
						.child(
							ui_motion::menu_surface(gpui::div())
								.p(gpui::px(5.))
								.child(self.model_palette(cx))
								.child(
									gpui::div()
										.mt(gpui::px(6.))
										.child(self.service_tier_picker(cx)),
								),
						)
						.child(
							ui_motion::menu_surface(gpui::div())
								.px(gpui::px(7.))
								.py(gpui::px(2.))
								.rounded_full()
								.flex()
								.items_center()
								.child(self.creation_effort_toggle(cx))
								.child(gpui::div().flex_1().min_w_0().child(self.effort_scale(cx))),
						)
						.into_any_element()
				})
				.into_any_element(),
		)
	}

	pub(super) fn select_composer_option(
		&mut self,
		menu: &str,
		value: &str,
		cx: &mut Context<Self>,
	) {
		if menu == "model" {
			self.model.update(cx, |input, cx| input.set_content(value, cx));
			self.mark_model_intent(cx);
			self.reconcile_selected_model_effort(cx);
			self.reconcile_model_options(cx);
		} else {
			let Ok(effort) = ConversationReasoningEffort::new(value) else { return };

			self.effort = effort;

			self.mark_effort_intent(cx);
		}

		self.save_draft_document(cx);
		cx.notify();
	}

	pub(super) fn usage_line(&self, _cx: &mut Context<Self>) -> Option<AnyElement> {
		let (id, AgentHistoryResult::Available { usage: Some(usage), .. }) =
			self.history.as_ref()?
		else {
			return None;
		};

		if self.selected.as_ref() != Some(id) || usage.context_tokens == 0 {
			return None;
		}

		let capacity = usage.context_window.filter(|size| *size > 0)?;
		let percent = usage.context_tokens as f64 / capacity as f64 * 100.0;
		Some(
			gpui::div()
				.id("composer-context-detail")
				.debug_selector(|| "composer-context-detail".into())
				.aria_label(format!("Context: {} of {} tokens", usage.context_tokens, capacity))
				.flex_none()
				.px(gpui::px(6.))
				.flex()
				.justify_end()
				.items_center()
				.gap(gpui::px(4.))
				.text_size(gpui::px(11.))
				.line_height(gpui::px(16.))
				.text_color(gpui::rgb(TEXT_MUTED))
				.child(context_ring((percent / 100.0).clamp(0.0, 1.0) as f32))
				.child(format!(
					"CTX {} / {}",
					agent_surface::compact_tokens(usage.context_tokens),
					agent_surface::compact_tokens(capacity),
				))
				.into_any_element(),
		)
	}

	fn attachment_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
		if self.attachments.is_empty() {
			return None;
		}

		let mut row = gpui::div().flex().flex_wrap().gap_1().px_1();

		for file in &self.attachments {
			let path = std::path::PathBuf::from(file.path.as_str());
			let label = file
				.skill_name
				.as_ref()
				.map(|name| format!("Skill: {}", name.as_str()))
				.unwrap_or_else(|| {
					path.file_name().unwrap_or_default().to_string_lossy().into_owned()
				});
			let remove = file.clone();

			row = row.child(
				gpui::div()
					.id(SharedString::from(
						serde_json::json!([
							"attachment",
							file.path.as_str(),
							file.skill_name
								.as_ref()
								.map(crate::shell::agent_surface::WireText::as_str)
						])
						.to_string(),
					))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Remove attachment {label}"))
					.h(gpui::px(30.0))
					.max_w(gpui::px(220.0))
					.px_2()
					.flex()
					.items_center()
					.gap_2()
					.rounded(gpui::px(6.0))
					.bg(gpui::rgba(0xffffff0a))
					.text_size(gpui::px(11.0))
					.cursor_pointer()
					.when(file.image, |d| {
						d.child(gpui::img(path).size(gpui::px(24.0)).rounded(gpui::px(3.0)))
					})
					.child(gpui::div().min_w_0().overflow_hidden().text_ellipsis().child(label))
					.child("×")
					.on_click(cx.listener({
						let remove = remove.clone();

						move |s, _, _, cx| {
							s.attachments.retain(|f| f != &remove);
							cx.notify();
						}
					}))
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						if ["enter", "space", "backspace"].contains(&e.keystroke.key.as_str()) {
							s.attachments.retain(|f| f != &remove);
							cx.notify();
							cx.stop_propagation();
						}
					}))
					.smooth(),
			);
		}

		Some(row.into_any_element())
	}

	fn pick_attachments(&mut self, cx: &mut Context<Self>) {
		let epoch = self.command_epoch;
		let owner = self.composer_manager.clone();
		let result = cx.prompt_for_paths(PathPromptOptions {
			files: true,
			directories: true,
			multiple: true,
			prompt: Some("Add to message".into()),
		});

		cx.spawn(async move |s, cx| {
			if let Ok(Ok(Some(paths))) = result.await {
				let _ = s.update(cx, |s, cx| {
					if s.command_epoch == epoch && s.composer_manager == owner {
						s.attach_paths(paths, cx);
					}
				});
			}
		})
		.detach();
	}

	fn attach_paths(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
		for path in paths {
			if self.attachments.len() >= 16 {
				self.feedback = "Attach at most 16 files or folders.".into();

				break;
			}

			let Some(path) = path.canonicalize().ok().filter(|p| p.is_file() || p.is_dir()) else {
				self.feedback = "The selected file or folder is not available.".into();

				continue;
			};
			let image = path.is_file()
				&& path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
					["png", "jpg", "jpeg", "webp", "gif"].contains(&s.to_ascii_lowercase().as_str())
				});
			let Ok(path) = ConversationWorkingDirectory::new(path.to_string_lossy().as_ref())
			else {
				continue;
			};
			let file = AgentAttachmentDto { path, image, skill_name: None };

			if !self.attachments.contains(&file) {
				self.attachments.push(file);
			}
		}

		cx.notify();
	}

	pub(super) fn attach_clipboard(&mut self, item: &ClipboardItem, cx: &mut Context<Self>) {
		for entry in &item.entries {
			match entry {
				ClipboardEntry::ExternalPaths(paths) => self.attach_paths(paths.0.to_vec(), cx),
				ClipboardEntry::Image(image) => match save_clipboard_image(image) {
					Ok(path) => self.attach_paths(vec![path], cx),
					Err(error) => {
						self.feedback = format!("Cannot attach clipboard image: {error}");

						cx.notify();
					},
				},
				_ => {},
			}
		}
	}
}

fn save_clipboard_image(image: &Image) -> Result<std::path::PathBuf> {
	let home = env::var_os("HOME").ok_or_else(|| Error::other("Home directory unavailable"))?;
	let dir = std::path::PathBuf::from(home).join(".decodex/attachments");

	DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;

	let ext = match image.format {
		ImageFormat::Png => "png",
		ImageFormat::Jpeg => "jpg",
		ImageFormat::Webp => "webp",
		ImageFormat::Gif => "gif",
		_ => return Err(Error::other("Paste a PNG, JPEG, WebP, or GIF image")),
	};
	let path = dir.join(format!("{}.{ext}", agent_surface::unique_command()));

	OpenOptions::new()
		.write(true)
		.create_new(true)
		.mode(0o600)
		.open(&path)?
		.write_all(&image.bytes)?;

	Ok(path)
}

fn context_ring(fraction: f32) -> impl IntoElement {
	let fill = gpui::rgb(if fraction >= 0.9 {
		0xed8585
	} else if fraction >= 0.7 {
		ui_theme::AMBER
	} else {
		0xa5a0ed
	});
	gpui::canvas(
		|_, _, _| (),
		move |bounds, _, window, _| {
			for (portion, color) in [(1.0, gpui::rgba(0xffffff24)), (fraction, fill)] {
				if portion <= 0.0 {
					continue;
				}

				let steps = (portion * 64.0).ceil() as usize;
				let mut path = PathBuilder::stroke(gpui::px(1.6));

				for step in 0..=steps {
					let angle = -FRAC_PI_2 + TAU * portion * step as f32 / steps as f32;
					let point = bounds.center()
						+ gpui::point(gpui::px(angle.cos() * 5.7), gpui::px(angle.sin() * 5.7));

					if step == 0 {
						path.move_to(point);
					} else {
						path.line_to(point);
					}
				}

				if let Ok(path) = path.build() {
					window.paint_path(path, color);
				}
			}
		},
	)
	.size(gpui::px(16.0))
}

#[cfg(test)]
mod tests {
	use std::fs;

	use gpui::AppContext as _;

	use crate::shell::agent_surface::composer::{
		AgentActionDto, AgentDispatchStateDto, AgentSnapshotResult, AgentSurface,
		ConversationModel, ConversationReasoningEffort, EntityId, HistoryText,
	};

	#[gpui::test]
	fn native_and_main_composers_keep_the_same_geometry(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1200.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let main = visual.debug_bounds("agent-composer").unwrap();
		let main_send = visual.debug_bounds("composer-send").unwrap();
		surface.update(visual, |s, cx| {
			s.native_agents.selected = Some(("agent".into(), "child".into()));
			s.native_agents.connection =
				crate::shell::agent_surface::native_agents::NativeConnection::Ready;
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let native = visual.debug_bounds("agent-composer").unwrap();
		let native_send = visual.debug_bounds("composer-send").unwrap();
		assert_eq!(main.size, native.size);
		assert_eq!(main.right() - main_send.right(), native.right() - native_send.right());
		assert_eq!(main.bottom() - main_send.bottom(), native.bottom() - native_send.bottom());
	}

	#[gpui::test]
	fn attachment_picker_keeps_the_opening_draft_owner(cx: &mut gpui::TestAppContext) {
		let directory = tempfile::tempdir().unwrap();
		let file = directory.path().join("reference.txt");

		fs::write(&file, "Fixture reference").unwrap();

		for change in ["none", "edit", "selection", "manager", "profile", "cancel"] {
			let surface = cx.new(AgentSurface::new);

			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);

				s.snapshot
					.as_mut()
					.unwrap()
					.work_items
					.iter_mut()
					.find(|work| work.id == "release")
					.unwrap()
					.kind = decodex_protocol::AgentWorkKindDto::Manager;

				s.open_page("agent", cx);
				s.composer.update(cx, |input, cx| input.set_content("Opening draft", cx));
				s.pick_attachments(cx);
			});

			assert!(cx.did_prompt_for_paths());

			surface.update(cx, |s, cx| match change {
				"edit" => s.composer.update(cx, |input, cx| input.set_content("Later edit", cx)),
				"selection" => s.open_page("verify", cx),
				"manager" => s.open_page("release", cx),
				"profile" => s.bind_profile(None, cx),
				_ => {},
			});
			cx.simulate_path_prompt_response(|options| {
				assert!(options.files && options.directories && options.multiple);

				(change != "cancel").then(|| vec![file.clone()])
			});
			cx.run_until_parked();
			surface.update(cx, |s, cx| {
				assert_eq!(
					s.attachments.len(),
					usize::from(matches!(change, "none" | "edit" | "selection")),
					"{change}"
				);
				assert_eq!(
					s.composer.read(cx).content(),
					match change {
						"edit" => "Later edit",
						"manager" => "",
						_ => "Opening draft",
					}
				);
				assert!(!s.sending);
			});
		}
	}

	#[gpui::test]
	fn composer_accepts_directory_references_without_image_conversion(
		cx: &mut gpui::TestAppContext,
	) {
		let directory = tempfile::tempdir().unwrap();
		let folder = directory.path().join("notes.png");

		fs::create_dir(&folder).unwrap();

		let file = directory.path().join("reference.txt");

		fs::write(&file, "Fixture reference").unwrap();

		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.attach_paths(vec![folder.clone(), file.clone(), folder.clone()], cx);

			assert_eq!(s.attachments.len(), 2);
			assert!(!s.attachments.iter().any(|attachment| attachment.image));
			assert_eq!(
				s.attachments[0].path.as_str(),
				folder.canonicalize().unwrap().to_str().unwrap()
			);
		});
	}

	#[gpui::test]
	fn delivered_messages_do_not_keep_the_composer_waiting_after_stop(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.composer.update(cx, |input, cx| input.clear(cx));
			s.snapshot.as_mut().unwrap().pending_events.push(
				decodex_protocol::AgentPendingEventDto {
					id: 99,
					source_event_id: "test".into(),
					work_item_id: "agent".into(),
					event_kind: "user_message".into(),
					created_at_micros: 1,
					delivery_claimed: true,
				},
			);

			s.feedback = "Message saved · Waiting for agent…".into();

			s.apply_result(Ok(AgentSnapshotResult::Available(s.snapshot.clone().unwrap())));

			assert!(!s.awaiting_start(cx));
			assert!(!s.stop_button(cx));

			s.snapshot.as_mut().unwrap().pending_events.last_mut().unwrap().delivery_claimed =
				false;

			assert!(s.awaiting_start(cx), "undelivered input still waits for its turn");
		});
	}

	#[gpui::test]
	fn cancellation_is_separate_from_send_and_clears_when_the_turn_ends(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));

			s.interrupting = Some(("agent".into(), "cancelled-turn".into()));

			assert!(s.stop_button(cx));
			assert!(!s.sending);
			assert!(!s.awaiting_start(cx));

			s.apply_result(Ok(AgentSnapshotResult::Available(s.snapshot.clone().unwrap())));

			assert!(s.interrupting.is_none());
			assert!(!s.uncertain);
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");
			assert!(s.status_notice().is_none());
		});
	}

	#[gpui::test]
	fn model_selection_uses_native_default_and_capabilities(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.capabilities = Some(decodex_protocol::AgentCapabilitiesResult::Available {
				models: vec![decodex_protocol::AgentModelDto {
					model: ConversationModel::new("custom-model").unwrap(),
					name: "Custom".into(),
					efforts: vec![
						ConversationReasoningEffort::Low,
						ConversationReasoningEffort::Medium,
					],
					default_effort: Some(ConversationReasoningEffort::Medium),
					supports_fast: false,
					service_tiers: vec![],
					default_service_tier: None,
					available_cyber_programs: None,
					specialty: None,
					supports_images: false,
					availability: None,
					upgrade: None,
				}],
				memory_enabled: Some(true),
			});
			s.composer_menu = Some("model");
			s.effort = ConversationReasoningEffort::Ultra;
			s.fast = true;

			s.select_composer_option("model", "custom-model", cx);

			assert_eq!(s.composer_model_label(cx), "Custom");
			assert_eq!(s.composer_model_value(cx).as_deref(), Some("custom-model"));

			if let Some(decodex_protocol::AgentCapabilitiesResult::Available { models, .. }) =
				&mut s.capabilities
			{
				models[0].name = "Renamed catalog label".into();
			}

			assert_eq!(s.composer_model_label(cx), "Renamed catalog label");
			assert_eq!(s.composer_model_value(cx).as_deref(), Some("custom-model"));
			assert_eq!(s.effort, ConversationReasoningEffort::Medium);
			assert!(!s.fast);
			assert_eq!(s.model_efforts(cx).len(), 2);

			s.select_composer_option("effort", "low", cx);
			s.select_composer_option("model", "custom-model", cx);

			assert_eq!(s.effort, ConversationReasoningEffort::Low);
			assert_eq!(s.composer_menu, Some("model"));

			if let Some(decodex_protocol::AgentCapabilitiesResult::Available { models, .. }) =
				&mut s.capabilities
			{
				let mut other = models[0].clone();

				other.model = ConversationModel::new("other-model").unwrap();

				models.push(other);
			}

			s.select_composer_option("model", "other-model", cx);

			assert_eq!(s.composer_model_label(cx), "Renamed catalog label");
			assert_eq!(s.composer_model_value(cx).as_deref(), Some("other-model"));

			s.capabilities = None;

			assert_eq!(s.composer_model_label(cx), "other-model");
		});
	}

	#[gpui::test]
	fn escape_requires_two_presses_for_the_same_current_turn(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "agent")
				.unwrap();

			work.dispatch_state = AgentDispatchStateDto::Running;
			work.active_turn_id = Some("turn".into());

			s.feedback.clear();

			s.workspace.details_visible = true;
			s.composer_menu = Some("model");

			s.escape_interrupt(cx);

			assert!(s.composer_menu.is_none());
			assert!(!s.escape_stop_armed());

			s.escape_interrupt(cx);

			assert!(s.escape_stop_armed());
			assert!(s.workspace.details_visible, "inspection must not intercept Escape");
			assert!(s.feedback.is_empty(), "first Escape must not dispatch an interrupt");

			s.escape_stop.as_mut().unwrap().2 -= std::time::Duration::from_secs(3);

			s.escape_interrupt(cx);

			assert!(s.feedback.is_empty(), "expired confirmation must only rearm");

			s.escape_stop.as_mut().unwrap().1 = "old-turn".into();

			s.escape_interrupt(cx);

			assert!(s.feedback.is_empty(), "confirmation must not cross turn identities");

			s.escape_interrupt(cx);

			assert!(!s.escape_stop_armed());
			assert_eq!(
				s.feedback, "No service profile is configured.",
				"second Escape reaches the native interrupt path"
			);
		});
	}

	#[gpui::test]
	fn accepted_message_does_not_flash_the_live_voice_control(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.composer.update(cx, |input, cx| input.clear(cx));

			s.sending = true;

			assert!(s.awaiting_start(cx));

			s.sending = false;
			s.feedback = "Message saved · Waiting for agent…".into();

			assert!(s.awaiting_start(cx));

			s.feedback.clear();

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "agent")
				.unwrap();

			work.dispatch_state = AgentDispatchStateDto::Dispatching;

			assert!(s.awaiting_start(cx));

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "agent")
				.unwrap();

			work.dispatch_state = AgentDispatchStateDto::Running;
			work.active_turn_id = Some("turn".into());

			assert!(!s.awaiting_start(cx));
			assert!(s.stop_button(cx));
		});
	}

	#[gpui::test]
	fn delivery_mode_and_stop_follow_the_selected_turn(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "agent")
				.unwrap();

			work.dispatch_state = AgentDispatchStateDto::Running;
			work.active_turn_id = Some("exact-turn".into());

			let action = |s: &AgentSurface| {
				s.configured_send(
					EntityId::new("agent").unwrap(),
					HistoryText::new("Supplement").unwrap(),
					vec![],
				)
			};

			assert!(
				matches!(action(s),AgentActionDto::Steer {turn_id,..} if turn_id.as_str()=="exact-turn")
			);

			s.composer.update(cx, |i, cx| i.clear(cx));

			assert!(s.stop_button(cx));

			s.uncertain = true;

			s.interrupt_current(cx);

			assert_eq!(s.feedback, "No service profile is configured.");

			s.uncertain = false;

			s.composer.update(cx, |i, cx| i.set_content("Supplement", cx));

			assert!(!s.stop_button(cx));

			s.sending = true;

			assert!(!s.awaiting_start(cx), "steering does not restart the current turn");
			assert!(!s.stop_button(cx), "keep the send glyph while the supplement is submitted");

			s.sending = false;
			s.steer = false;

			assert!(matches!(action(s), AgentActionDto::SendConfigured { .. }));

			s.steer = true;

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "agent")
				.unwrap();

			work.dispatch_state = AgentDispatchStateDto::Idle;
			work.active_turn_id = None;

			assert!(matches!(action(s), AgentActionDto::SendConfigured { .. }));
			assert!(!s.stop_button(cx));
		});
	}
}
