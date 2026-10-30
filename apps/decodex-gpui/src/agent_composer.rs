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
	Anchor, AnyElement, AppContext as _, ClipboardEntry, Div, ExternalPaths, Focusable as _,
	FontWeight, Image, ImageFormat, KeyDownEvent, MouseButton, MouseDownEvent, PathBuilder,
	PathPromptOptions, Rgba, Stateful,
};
use tokio::{runtime::Builder, time};
use ui_theme::{
	BLUE, CONTROL_SIZE, HOVER_FILL, SELECTED_HOVER_FILL, TEXT, TEXT_MUTED,
	material::{Material, Surface},
};

#[cfg(test)] use crate::shell::agent_surface::ConversationModel;
use crate::{
	shell::{
		FocusNext,
		agent_surface::{
			self, AgentActionDto, AgentClient, AgentCommandResponse, AgentDispatchStateDto,
			AgentHistoryResult, AgentSnapshotResult, AgentSurface, ClipboardItem, ComposerInput,
			Context, ConversationReasoningEffort, ConversationWorkingDirectory, Entity, EntityId,
			FluentBuilder, HistoryText, IdempotencyKey, InteractiveElement, IntoElement,
			ParentElement, Render, Role, SharedString, SmoothControl, StatefulInteractiveElement,
			Styled, SubmitComposer, Window, model_settings, prompts,
			selectable_text::SelectableText,
			ui_theme,
			ui_theme::{
				BODY_SIZE, COMPOSER_MATERIAL, COMPOSER_MIN_HEIGHT, COMPOSER_RADIUS,
				CONVERSATION_WIDTH, ERROR, INFO, LINE_STRONG, ON_PRIMARY, PRIMARY_FILL, WARNING,
			},
			workspace::ARCHIVED_MESSAGE,
		},
		workspace_symbols,
		workspace_symbols::Symbol,
	},
	ui_feedback::{self, Feedback, Tone},
	ui_motion,
	ui_theme::{COMPOSER_BOTTOM_GAP, COMPOSER_TOP_GAP, CONVERSATION_INSET},
};
use controls::{PrimaryMark, PrimaryMode};
use decodex_protocol::AgentAttachmentDto;

pub(super) fn selection_menu(menu: Option<&str>) -> bool {
	matches!(
		menu,
		Some(
			"model"
				| "permissions"
				| "attachments"
				| "microphone"
				| "tasks"
				| "skills"
				| "agent-settings"
		)
	)
}

pub(super) fn composer_menu_trigger(menu: &str) -> &str {
	match menu {
		"model" | "permissions" => menu,
		_ => "attach",
	}
}

pub(super) fn composer_menu_width(menu: Option<&str>) -> f32 {
	match menu {
		Some("agent-settings" | "skills") => 380.,
		Some("attachments" | "microphone" | "tasks") => 280.,
		_ => 248.,
	}
}

pub(super) struct ComposerTip(pub(super) String);
impl Render for ComposerTip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		ui_motion::tooltip_surface(gpui::div())
			.px_3()
			.py_2()
			.text_size(gpui::px(11.0))
			.text_color(crate::ui_theme::rgb(TEXT))
			.child(self.0.clone())
	}
}

impl AgentSurface {
	#[cfg(any(test, target_os = "macos"))]
	pub(super) fn native_composer_requested(&self, allowed: bool, supported: bool) -> bool {
		allowed
			&& supported
			&& !self.conversation_find_open()
			&& !self.workspace_connecting()
			&& !self.workspace.browsing
			&& self.snapshot.is_some()
			&& (self.native_agents.selected.is_some() || self.selected_is_agent())
	}

	/// Move keyboard and accessibility focus into the current message editor.
	pub(crate) fn focus_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.composer_unavailable_reason().is_some() {
			return;
		}
		#[cfg(all(target_os = "macos", not(test)))]
		if self.focus_native_message(cx) {
			return;
		}

		window.focus(&self.conversation_composer().focus_handle(cx), cx);
	}

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

		if work.dispatch_state != AgentDispatchStateDto::Running
			|| self.current_turn_completed(work)
		{
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
				&& (self.feedback.message == "Message saved · Waiting for agent…"
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
		if self.dismiss_relation_usage_options(cx) {
			self.escape_stop = None;

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
			self.feedback = Feedback::error("No service profile is configured.");

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
				if s.interrupting.as_ref() != Some(&target) {
					return;
				}

				let accepted = match result {
					Ok((result, snapshot)) => {
						if let Some(snapshot) = snapshot {
							s.apply_result(Ok(snapshot));
						}

						matches!(result, Ok(AgentCommandResponse::Accepted { .. }))
					},

					Err(_) => false,
				};

				if !accepted && s.interrupting.as_ref() == Some(&target) {
					s.interrupting = None;
					s.feedback =
						Feedback::warning("Stop unconfirmed. Press Stop again if still running.");
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
		if self.native_controls.composer.enabled {
			return self.render_native_composer_anchor(cx);
		}

		gpui::div()
			.w_full()
			.px(gpui::px(CONVERSATION_INSET))
			.pt(gpui::px(COMPOSER_TOP_GAP))
			.pb(gpui::px(COMPOSER_BOTTOM_GAP))
			.flex()
			.flex_col()
			.items_center()
			.child(
				self.render_composer_capsule(false, window, cx)
					.child(self.render_composer_popover(cx)),
			)
			.into_any_element()
	}

	fn update_message_placeholder(&self, cx: &mut Context<Self>) {
		let placeholder = if self
			.visible_conversation_feedback()
			.is_some_and(|feedback| matches!(feedback.tone, Tone::Error | Tone::Warning))
		{
			"Message".to_owned()
		} else {
			prompts::session_quote()
		};
		let input = if self.native_agents.selected.is_some() {
			self.conversation_composer()
		} else {
			&self.composer
		};

		input.update(cx, |input, cx| input.set_placeholder(placeholder, cx));
	}

	pub(super) fn render_composer_capsule(
		&self,
		native: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let capsule = composer_capsule(native);
		let (blocked, opacity) = ui_feedback::composer_block(
			self.feedback_scope(),
			self.composer_unavailable_reason(),
			window,
			cx,
		);
		let ready = ui_motion::value(
			"composer-ready-opacity",
			if blocked.is_none() { 1. } else { 0. },
			window,
			cx,
		);

		if let Some(reason) = blocked {
			return self.render_blocked_composer(capsule, reason, opacity, window, cx);
		}

		self.update_message_placeholder(cx);

		let editor = self.composer_editor(native, window, cx);

		capsule
			.on_key_down(cx.listener(|s, e: &KeyDownEvent, _, cx| {
				if e.keystroke.key == "escape" && !e.keystroke.modifiers.shift {
					if !e.is_held {
						s.escape_interrupt(cx);
					}

					cx.stop_propagation();
				}
			}))
			.on_drop(cx.listener(|s, paths: &ExternalPaths, _, cx| {
				if s.native_agents.selected.is_none() && s.async_question_composer().is_none() {
					s.attach_paths(paths.0.to_vec(), cx);
				}
			}))
			.child(
				gpui::div()
					.w_full()
					.flex()
					.flex_col()
					.gap(gpui::px(4.))
					.opacity(ready)
					.when(
						self.native_agents.selected.is_none()
							&& !self.voice_on_current_conversation(),
						|d| d.child(self.composer_question_panel(cx)),
					)
					.when(
						self.native_agents.selected.is_none()
							&& self.async_question_composer().is_none(),
						|d| {
							d.children(self.attachment_row(cx))
								.children(self.task_reference_row(cx))
						},
					)
					.map(|capsule| {
						let controls = gpui::div()
							.id("composer-action-row")
							.debug_selector(|| "composer-action-row".into())
							.w_full()
							.min_w_0()
							.flex()
							.items_center()
							.gap(gpui::px(4.))
							.when(
								self.native_agents.selected.is_none()
									&& self.dictation.is_none()
									&& !self.voice_on_current_conversation()
									&& self.async_question_composer().is_none(),
								|d| {
									d.child(self.composer_control(
										"attach",
										"+".into(),
										"Attachments, skills and microphone",
										|s, cx| s.toggle_composer_menu("attachments", cx),
										cx,
									))
								},
							);

						if self.voice_on_current_conversation() {
							capsule.child(
								controls.child(editor).child(self.composer_toolbar(window, cx)),
							)
						} else {
							capsule
								.child(editor)
								.child(controls.child(self.composer_toolbar(window, cx)))
						}
					}),
			)
	}

	fn composer_editor(
		&self,
		native: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		gpui::div()
			.id("composer-editor-area")
			.debug_selector(|| "composer-editor-area".into())
			.flex()
			.flex_col()
			.flex_1()
			.when(!self.voice_on_current_conversation(), |d| d.flex_none().w_full())
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
			.on_action(cx.listener(|s, _: &FocusNext, window, cx| {
				if s.composer.focus_handle(cx).is_focused(window)
					&& s.can_accept_prompt_suggestion(cx)
				{
					s.accept_prompt_suggestion(cx);
					cx.stop_propagation();
				} else {
					cx.propagate();
				}
			}))
			.map(|editor| {
				if let Some(waveform) = self.voice_controls(window, cx) {
					editor.child(waveform)
				} else {
					editor.child(self.conversation_composer().clone())
				}
			})
	}

	fn render_blocked_composer(
		&self,
		capsule: Stateful<Div>,
		reason: &'static str,
		opacity: f32,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let input = self.conversation_composer();

		if input.focus_handle(cx).is_focused(window) {
			window.blur(cx);
		}
		if self.composer_menu.is_some() {
			let owner = cx.entity().downgrade();

			cx.defer(move |cx| {
				let _ = owner.update(cx, |s, cx| {
					s.composer_menu_content = None;

					s.dismiss_composer_menu(cx);
				});
			});
		}

		let draft = input.read(cx).content().to_owned();
		let archived = reason == ARCHIVED_MESSAGE;
		let draft_key = format!(
			"composer-saved-draft-{:?}-{}",
			input.entity_id(),
			self.selected.as_deref().unwrap_or("new")
		);

		capsule.justify_center().child(
			gpui::div()
				.id("composer-blocked")
				.debug_selector(|| "composer-blocked".into())
				.opacity(opacity)
				.w_full()
				.flex_1()
				.px(gpui::px(8.))
				.flex()
				.flex_col()
				.justify_center()
				.when(archived, |d| {
					d.children(self.conversation_work().map(|work| self.archive_notice(&work, cx)))
				})
				.when(!archived && draft.is_empty(), |d| {
					d.child(
						gpui::div()
							.debug_selector(|| "composer-blocked-placeholder".into())
							.text_color(crate::ui_theme::rgb(TEXT_MUTED))
							.child("Message"),
					)
				})
				.when(!archived && !draft.is_empty(), |d| {
					d.child(
						gpui::div()
							.id("composer-retained-draft")
							.debug_selector(|| "composer-retained-draft".into())
							.w_full()
							.max_h(gpui::px(120.))
							.pb(gpui::px(4.))
							.overflow_y_scroll()
							.text_color(crate::ui_theme::rgb(TEXT_MUTED))
							.child(SelectableText::plain(draft_key, draft)),
					)
				})
				.when(self.locked_branch_source().is_some(), |d| {
					d.child(self.workspace_action(
						"locked-thread-branch".into(),
						"Continue in a new branch…".into(),
						|s, cx| s.review_locked_branch(cx),
						cx,
					))
				})
				.when(self.voice_on_current_conversation() || self.dictation.is_some(), |d| {
					d.child(self.workspace_action(
						"composer-stop-recording".into(),
						"Stop recording".into(),
						|s, cx| s.stop_voice(cx),
						cx,
					))
				}),
		)
	}

	pub(super) fn render_composer_popover(&self, cx: &mut Context<Self>) -> impl IntoElement {
		if self.workspace_connecting() || self.composer_unavailable_reason().is_some() {
			return gpui::div().absolute();
		}
		let menu = self.composer_menu.or(self.composer_menu_content);
		if !selection_menu(menu) {
			return gpui::div().absolute();
		}

		#[cfg(all(target_os = "macos", not(test)))]
		if self.native_controls.composer.enabled && !self.native_controls.menu.failed {
			// The attached menu owns composition above the native composer.
			return gpui::div().absolute().inset_0().on_mouse_down_out(cx.listener(
				|s, _, _, cx| {
					s.dismiss_composer_menu(cx);
				},
			));
		}
		let Some(anchor) =
			menu.and_then(|key| self.menu_trigger_bounds.get(composer_menu_trigger(key)))
		else {
			return gpui::div().absolute();
		};
		gpui::div().absolute().inset_0().child(
			gpui::deferred(
				gpui::anchored()
					.anchor(Anchor::BottomLeft)
					.snap_to_window()
					.position(gpui::point(
						anchor.left() - gpui::px(ui_theme::POPOVER_PADDING),
						anchor.top(),
					))
					.child(
						gpui::div().w(gpui::px(composer_menu_width(menu))).child(
							ui_motion::popover(
								"composer-popover-motion",
								self.composer_menu.is_some(),
								self.composer_options(cx)
									.unwrap_or_else(|| gpui::div().into_any_element()),
							),
						),
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
			.when(self.can_open_sample(), |options| {
				options
					.child(self.composer_control(
						"task-suggest-item",
						"Suggest next message…".into(),
						"Generate a suggestion for your next message",
						|s, cx| {
							s.composer_menu = None;
							s.suggest_next_prompt(cx);
						},
						cx,
					))
					.child(self.composer_control(
						"task-sample-item",
						"Ask separately…".into(),
						"Ask separately",
						|s, cx| {
							s.composer_menu = None;

							s.open_sample(cx);
						},
						cx,
					))
			})
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

	fn composer_toolbar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		if self.native_agents.selected.is_some() {
			return gpui::div()
				.flex_1()
				.min_w_0()
				.h(gpui::px(CONTROL_SIZE))
				.flex()
				.items_center()
				.gap(gpui::px(8.))
				.child(
					gpui::div()
						.flex_1()
						.min_w_0()
						.text_ellipsis()
						.text_size(gpui::px(11.))
						.text_color(crate::ui_theme::rgb(TEXT_MUTED))
						.child(match &self.native_agents.detail {
							Some(decodex_protocol::NativeAgentsResult::Conversation {
								permission_mode,
								model,
								reasoning_effort,
								service_tier,
								..
							}) => {
								let permissions = permission_mode
									.map_or("Permissions not reported", |mode| mode.label());
								let speed = match service_tier.as_deref() {
									Some("priority") => "Fast",
									Some("default") => "Standard",
									Some(tier) => tier,
									None => "Speed not reported",
								};
								format!(
									"{} · {} · {permissions} · {speed}",
									model.as_deref().unwrap_or("Model unknown"),
									reasoning_effort.as_deref().unwrap_or("Effort unknown")
								)
							},
							_ => "Permissions and speed are managed by the parent Agent".into(),
						}),
				)
				.justify_end()
				.child(
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
				)
				.into_any_element();
		}

		if let Some(controls) = self.dictation_controls(window, cx) {
			return controls;
		}
		if self.voice_on_current_conversation()
			&& let Some(controls) = self.voice_toolbar(cx)
		{
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
			.child(self.composer_control(
				"permissions",
				self.permission_label(),
				"Permissions for this Agent",
				|s, cx| s.toggle_composer_menu("permissions", cx),
				cx,
			))
			.child(gpui::div().flex_1())
			.children(self.usage_line(cx))
			.when(self.async_question_composer().is_none(), |row| {
				row.child(self.composer_control_with_window(
					"dictation",
					"".into(),
					"Dictate into the draft",
					|s, w, cx| s.start_dictation(w, cx),
					cx,
				))
			})
			.child(self.composer_control_with_window(
				"send",
				"".into(),
				if self.async_question_composer().is_some() {
					"Send answer · Enter"
				} else if self.interrupting.is_some() {
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
					if s.async_question_composer().is_some() {
						s.submit(cx);

						return;
					}
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
	) -> AnyElement {
		let send = id == "send";
		let dictation_action = self.dictation.is_some()
			&& matches!(id, "send" | "dictation-cancel" | "dictation-finish");
		let disabled = send && self.composer_unavailable_reason().is_some();
		let menu_active = self.composer_menu.is_some_and(|menu| composer_menu_trigger(menu) == id);
		let target = cx.entity().downgrade();
		let tooltip = if disabled {
			"Sending paused until the connection is restored".to_owned()
		} else if id == "model" {
			format!("Model and reasoning · {label} · Speed: {}", self.composer_speed_label())
		} else if id == "permissions" && label != "Permissions" {
			format!("Permissions · {label}")
		} else {
			tip.to_owned()
		};

		if matches!(
			id,
			"attachment-item"
				| "audio-item"
				| "delivery"
				| "skill-item"
				| "task-suggest-item"
				| "task-sample-item"
				| "task-recap-item"
				| "task-reference-item"
				| "agent-settings"
		) {
			return ui_theme::menu_row(format!("composer-{id}"), true)
				.debug_selector(move || format!("composer-{id}"))
				.aria_label(tooltip)
				.flex()
				.items_center()
				.gap(gpui::px(10.))
				.when(id == "audio-item", |d| {
					d.aria_expanded(self.composer_menu == Some("microphone"))
				})
				.child(self.composer_control_content(id, label, cx))
				.on_click(cx.listener(move |s, _, window, cx| action(s, window, cx)))
				.on_key_down(cx.listener(move |s, e: &KeyDownEvent, window, cx| {
					if !e.is_held && ["enter", "space"].contains(&e.keystroke.key.as_str()) {
						action(s, window, cx);
						cx.stop_propagation();
					}
				}))
				.smooth()
				.into_any_element();
		}

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
			.when(!["model", "effort", "send", "voice-mute", "voice-end"].contains(&id), |d| {
				d.w(gpui::px(CONTROL_SIZE)).px_0()
			})
			.flex()
			.items_center()
			.justify_center()
			.text_size(gpui::px(12.0))
			.line_height(gpui::px(16.0))
			.whitespace_nowrap()
			.text_color(ui_theme::rgb(if send { ON_PRIMARY } else { TEXT_MUTED }))
			.when(id == "model", |d| {
				d.px(gpui::px(4.))
					.text_size(gpui::px(BODY_SIZE))
					.font_weight(FontWeight::NORMAL)
					.flex_shrink(1.)
					.min_w_0()
					.max_w(gpui::px(220.))
					.overflow_hidden()
			})
			.when(menu_active, |d| d.bg(crate::ui_theme::rgba(0xffffff12)))
			.when(send, |d| {
				d.w(gpui::px(28.)).h(gpui::px(28.)).rounded_full().bg(ui_theme::rgb(PRIMARY_FILL))
			})
			.when(dictation_action, |d| d.w(gpui::px(26.)).h(gpui::px(26.)).px_0().rounded_full())
			.when(disabled, |d| d.opacity(0.35))
			.when(!disabled, |d| d.cursor_pointer())
			.hover(move |d| {
				if disabled {
					return d;
				}

				d.bg(composer_hover_fill(send, menu_active))
			})
			.focus(move |d| d.bg(composer_hover_fill(send, menu_active)))
			.when(!["model", "effort"].contains(&id), |d| {
				d.tooltip(move |_, cx| cx.new(|_| ComposerTip(tooltip.clone())).into())
			})
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
			.when(["model", "effort", "attach", "permissions"].contains(&id), |d| {
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
			.when(["model", "attach", "permissions"].contains(&id), |d| {
				d.active(|d| d.bg(ui_theme::rgba(ui_theme::PRESSED_FILL)))
			})
			.smooth()
			.into_any_element()
	}

	fn composer_control_content(&self, id: &str, label: String, cx: &Context<Self>) -> AnyElement {
		match id {
			"send" => PrimaryMark {
				mode: if self.native_agents.selected.is_some()
					|| self.dictation.is_some()
					|| self.async_question_composer().is_some()
				{
					PrimaryMode::Send
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
			"permissions" => workspace_symbols::icon(self.permission_symbol()),
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
						.text_color(crate::ui_theme::rgb(TEXT_MUTED))
						.child(label),
				)
				.child(workspace_symbols::disclosure_chevron(
					"microphone-chevron",
					self.composer_menu == Some("microphone"),
				))
				.into_any_element(),
			"dictation" => workspace_symbols::icon_sized(Symbol::Microphone, 18.),
			"dictation-cancel" => workspace_symbols::icon_sized(Symbol::Close, 14.),
			"dictation-finish" => workspace_symbols::icon_sized(Symbol::Confirm, 14.),
			"delivery" => gpui::div()
				.w_full()
				.flex()
				.items_center()
				.gap(gpui::px(10.))
				.child(workspace_symbols::icon(Symbol::ActivityHandoff))
				.child("Send mode")
				.child(gpui::div().flex_1())
				.child(gpui::div().text_color(crate::ui_theme::rgb(TEXT)).child(label))
				.child(gpui::div().w(gpui::px(12.)).flex_none())
				.into_any_element(),
			"model" => gpui::div()
				.flex()
				.items_center()
				.gap(gpui::px(2.))
				.whitespace_nowrap()
				.text_color(crate::ui_theme::rgb(TEXT))
				.when(self.fast, |d| {
					d.child(
						gpui::div()
							.text_color(crate::ui_theme::rgb(BLUE))
							.child(workspace_symbols::icon(Symbol::Fast)),
					)
				})
				.child(gpui::div().max_w(gpui::px(180.)).text_ellipsis().child(label))
				.when_some(self.composer_effort_value(cx), |row, effort| {
					row.child(gpui::div().text_color(crate::ui_theme::rgb(TEXT_MUTED)).child("·"))
						.child(controls::effort_indicator(&effort))
				})
				.into_any_element(),
			"skill-item"
			| "task-suggest-item"
			| "task-sample-item"
			| "task-recap-item"
			| "task-reference-item"
			| "agent-settings" => gpui::div()
				.w_full()
				.min_w_0()
				.flex()
				.items_center()
				.gap(gpui::px(10.))
				.child(workspace_symbols::icon(match id {
					"skill-item" => Symbol::ActivityCommand,
					"task-suggest-item" => Symbol::ActivitySearch,
					"task-sample-item" => Symbol::Plus,
					"task-reference-item" => Symbol::Link,
					"agent-settings" => Symbol::Settings,
					_ => Symbol::Document,
				}))
				.child(gpui::div().flex_1().min_w_0().child(label))
				.when(matches!(id, "skill-item" | "task-reference-item" | "agent-settings"), |d| {
					d.child(workspace_symbols::icon_sized(Symbol::Forward, 12.))
				})
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

			if name == "permissions" {
				self.refresh_composer_permissions(true, cx);
			}
			self.load_capabilities(cx);
			self.refresh_composer_model_settings(cx);
		}

		cx.notify();
	}

	pub(super) fn dismiss_composer_menu(&mut self, cx: &mut Context<Self>) {
		self.composer_menu = None;
		self.effort_drag = None;
		self.effort_pointer = None;

		cx.notify();
	}

	pub(super) fn composer_options(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
		// Keep content mounted while the disclosure animates closed.
		let menu = self.composer_menu.or(self.composer_menu_content)?;
		let anchor = self.menu_trigger_bounds.get("attach").copied();
		#[cfg(all(target_os = "macos", not(test)))]
		let anchor = if self.native_controls.composer.enabled {
			self.native_controls.composer.bounds
		} else {
			anchor
		};
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
						s.dismiss_composer_menu(cx);
						cx.stop_propagation();
					}
				}))
				.on_mouse_down_out(cx.listener(|s, event: &MouseDownEvent, _, cx| {
					let trigger_hit = {
						#[cfg(all(target_os = "macos", not(test)))]
						let same_window = !s.native_controls.composer.enabled;
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

					s.dismiss_composer_menu(cx);
				}))
				.p(gpui::px(ui_theme::POPOVER_PADDING))
				.w_full()
				.flex()
				.flex_col()
				.gap(gpui::px(10.))
				.child(if menu == "tasks" {
					self.task_reference_options(cx)
				} else if menu == "skills" {
					self.skill_options(cx)
				} else if menu == "permissions" {
					self.permission_options(cx)
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
					self.model_options(cx)
				})
				.into_any_element(),
		)
	}

	pub(super) fn model_options(&self, cx: &mut Context<Self>) -> AnyElement {
		gpui::div()
			.id("model-options")
			.debug_selector(|| "model-options".into())
			.flex()
			.flex_col()
			.gap(gpui::px(8.))
			.child(self.model_palette(cx))
			.child(self.service_tier_picker(cx))
			.child(
				gpui::div()
					.flex()
					.items_center()
					.child(gpui::div().flex_1().min_w_0().child(self.effort_scale(cx))),
			)
			.into_any_element()
	}

	pub(super) fn select_composer_option(
		&mut self,
		menu: &str,
		value: &str,
		cx: &mut Context<Self>,
	) {
		if menu == "model" {
			if let Some(effort) = self
				.composer_effort_value(cx)
				.and_then(|effort| ConversationReasoningEffort::new(effort).ok())
			{
				self.effort = effort;
			}

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
				.flex_shrink(1.)
				.min_w_0()
				.px(gpui::px(6.))
				.flex()
				.justify_end()
				.items_center()
				.gap(gpui::px(4.))
				.text_size(gpui::px(11.))
				.line_height(gpui::px(16.))
				.text_color(crate::ui_theme::rgb(TEXT_MUTED))
				.child(context_ring((percent / 100.0).clamp(0.0, 1.0) as f32))
				.child(gpui::div().min_w_0().text_ellipsis().child(format!(
					"CTX {} / {}",
					agent_surface::compact_tokens(usage.context_tokens),
					agent_surface::compact_tokens(capacity),
				)))
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
					.bg(crate::ui_theme::rgba(0xffffff0a))
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
				self.feedback = Feedback::warning("Attach at most 16 files or folders.");

				break;
			}

			let Some(path) = path.canonicalize().ok().filter(|p| p.is_file() || p.is_dir()) else {
				self.feedback = Feedback::error("File or folder unavailable.");

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
						self.feedback = Feedback::error(format!("Could not attach image: {error}"));

						cx.notify();
					},
				},
				_ => {},
			}
		}
	}
}

#[cfg(any(test, target_os = "macos"))]
pub(super) fn composer_menu_bounds(
	anchor: gpui::Bounds<gpui::Pixels>,
	height: f32,
	width: f32,
	viewport: gpui::Size<gpui::Pixels>,
) -> gpui::Bounds<gpui::Pixels> {
	let x = f32::from(anchor.left()).clamp(8., (f32::from(viewport.width) - width - 8.).max(8.));
	let y = (f32::from(anchor.top()) - height).max(8.);

	gpui::Bounds::new(
		gpui::point(gpui::px(x), gpui::px(y)),
		gpui::size(gpui::px(width), gpui::px(height)),
	)
}

pub(super) fn composer_capsule(native: bool) -> Stateful<Div> {
	gpui::div()
		.id("agent-composer")
		.debug_selector(|| "agent-composer".into())
		.occlude()
		.relative()
		.w_full()
		.max_w(gpui::px(CONVERSATION_WIDTH))
		.min_w_0()
		.min_h(gpui::px(COMPOSER_MIN_HEIGHT))
		.px(gpui::px(10.))
		.py(gpui::px(7.))
		.rounded(gpui::px(COMPOSER_RADIUS))
		.when(!native || Surface::InputBar.material() == Material::Thin, |d| {
			d.border_1().border_color(crate::ui_theme::rgb(LINE_STRONG).opacity(0.4))
		})
		// Native materials can take on a desktop tint. Keep the editor's reading
		// surface in the selected palette while AppKit retains the material edge.
		.when(native, |d| d.bg(crate::ui_theme::rgba(COMPOSER_MATERIAL)))
		.when(!native, |d| d.bg(crate::ui_theme::rgba(COMPOSER_MATERIAL)))
		.when(!native, |d| {
			d.shadow(vec![gpui::BoxShadow {
				inset: false,
				color: crate::ui_theme::rgba(0x0000001a).into(),
				offset: gpui::point(gpui::px(0.), gpui::px(4.)),
				blur_radius: gpui::px(16.),
				spread_radius: gpui::px(-5.),
			}])
		})
		.flex()
		.flex_col()
		.gap(gpui::px(4.))
}

fn composer_hover_fill(send: bool, menu_active: bool) -> Rgba {
	if send {
		ui_theme::rgb(PRIMARY_FILL).blend(gpui::rgb(0xffffff).opacity(0.08))
	} else {
		crate::ui_theme::rgba(if menu_active { SELECTED_HOVER_FILL } else { HOVER_FILL })
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
	let fill = crate::ui_theme::rgb(if fraction >= 0.9 {
		ERROR
	} else if fraction >= 0.7 {
		WARNING
	} else {
		INFO
	});

	gpui::canvas(
		|_, _, _| (),
		move |bounds, _, window, _| {
			for (portion, color) in [(1.0, crate::ui_theme::rgba(0xffffff24)), (fraction, fill)] {
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
	use std::{fs, thread};

	use gpui::AppContext as _;

	use crate::shell::agent_surface::{
		composer::{
			AgentActionDto, AgentDispatchStateDto, AgentSnapshotResult, AgentSurface,
			ConversationModel, ConversationReasoningEffort, EntityId, HistoryText,
		},
		native_timeline::Binding,
	};
	use decodex_protocol::{AgentTimelineContent, AgentTimelineEntry};

	#[gpui::test]
	fn plus_menu_rows_use_the_available_width_and_one_alignment(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_000.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.composer_menu = Some("attachments");
			s.composer_menu_content = Some("attachments");
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_frame);
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_frame);
		let mut previous: Option<gpui::Bounds<gpui::Pixels>> = None;
		for id in [
			"composer-attachment-item",
			"composer-skill-item",
			"composer-task-recap-item",
			"composer-task-reference-item",
			"composer-audio-item",
			"composer-agent-settings",
			"composer-delivery",
		] {
			let row = visual.debug_bounds(id).unwrap();
			assert!(f32::from(row.size.width) >= 240., "{id} cannot inherit an icon button width");
			if let Some(previous) = previous {
				assert_eq!(row.left(), previous.left());
				assert_eq!(row.right(), previous.right());
				assert!(row.top() >= previous.bottom());
			}
			previous = Some(row);
		}
	}

	#[gpui::test]
	fn graph_and_inspector_keep_native_glass_and_the_current_draft(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.composer.update(cx, |input, cx| input.set_content("Keep this draft", cx));

			assert!(s.native_composer_requested(true, true));

			s.resources = Some((
				s.selected.clone().unwrap(),
				Some(decodex_protocol::AgentResourcesResult::Unavailable),
			));
			s.workspace.details_visible = true;

			assert!(s.native_composer_requested(true, true));

			s.workspace.details_visible = false;

			assert!(s.native_composer_requested(true, true));

			s.workspace.graph_visible = true;
			s.workspace.graph_expanded = true;

			assert!(s.graph_has_composer());
			assert!(s.native_composer_requested(true, true));

			s.workspace.details_visible = true;

			assert!(s.native_composer_requested(true, true));
			assert!(!s.native_composer_requested(false, true));
			assert!(!s.native_composer_requested(true, false));

			crate::ui_theme::material::Surface::InputBar
				.select(crate::ui_theme::material::Material::Thin, cx);

			assert!(s.native_composer_requested(true, true));
			assert_eq!(s.composer.read(cx).content(), "Keep this draft");

			crate::ui_theme::material::Surface::InputBar
				.select(crate::ui_theme::material::Material::LiquidGlass, cx);

			assert!(s.native_composer_requested(true, true));
		});
	}

	#[gpui::test]
	fn archived_and_locked_states_keep_the_standard_input_bar(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.snapshot.as_mut().unwrap().pending_events.clear();
			s.composer.update(cx, |input, cx| input.set_content("", cx));
			cx.notify();
		});
		visual.simulate_resize(gpui::size(gpui::px(1_180.), gpui::px(900.)));
		visual.update(crate::ui_frame::draw_frame);

		let ready = visual.debug_bounds("agent-composer").expect("ready input bar");

		for archived in [true, false] {
			surface.update(visual, |s, cx| {
				let snapshot = s.snapshot.as_mut().unwrap();

				snapshot.archived_work_ids.clear();

				if archived {
					snapshot.archived_work_ids.insert("agent".into());
				} else {
					snapshot.pending_events.push(decodex_protocol::AgentPendingEventDto {
						id: 99,
						source_event_id: "writer-lock".into(),
						work_item_id: "agent".into(),
						event_kind: "thread_in_use_needs_attention".into(),
						created_at_micros: 1,
						delivery_claimed: false,
					});
				}

				assert!(s.composer_unavailable_reason().is_some());
				assert!(s.native_composer_requested(true, true));

				cx.notify();
			});

			visual.update(crate::ui_frame::draw_frame);

			thread::sleep(std::time::Duration::from_millis(220));

			visual.update(crate::ui_frame::draw_frame);

			let blocked = visual.debug_bounds("agent-composer").expect("disabled input bar");

			assert_eq!(blocked.size, ready.size, "read-only state uses the normal input geometry");
			assert_eq!(blocked.bottom(), ready.bottom());
			assert!(visual.debug_bounds("composer-blocked").is_some());
			assert!(visual.debug_bounds("composer-editor-area").is_none());
			assert!(surface.read_with(visual, |s, _| s.graph_has_composer()));
		}
	}

	#[test]
	fn native_menu_fits_above_the_trigger_without_covering_it() {
		let viewport = gpui::size(gpui::px(900.), gpui::px(720.));

		for x in [100., 850.] {
			let trigger = gpui::Bounds::new(
				gpui::point(gpui::px(x), gpui::px(650.)),
				gpui::size(gpui::px(120.), gpui::px(28.)),
			);
			let menu = super::composer_menu_bounds(
				trigger,
				240.,
				super::composer_menu_width(Some("model")),
				viewport,
			);

			assert_eq!(menu.bottom(), trigger.top());
			assert!(menu.left() >= gpui::px(8.));
			assert!(menu.right() <= viewport.width - gpui::px(8.));
		}
	}

	#[gpui::test]
	fn model_menu_stays_near_its_button_in_a_tall_composer(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_000.)));

		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.composer.update(cx, |input, cx| input.set_content(&"Draft line\n".repeat(6), cx));

			s.composer_menu = Some("model");
			s.composer_menu_content = Some("model");

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);
		visual.update(|w, _| w.refresh());
		visual.update(crate::ui_frame::draw_current_frame);

		let trigger = visual.debug_bounds("composer-model").unwrap();
		let menu = visual.debug_bounds("popover-surface").unwrap();
		let composer = visual.debug_bounds("agent-composer").unwrap();
		let gap = f32::from(trigger.top() - menu.bottom());

		assert!((4. ..=16.).contains(&gap), "menu gap: {gap}");
		assert!(menu.bottom() > composer.top() + gpui::px(50.));
	}

	#[gpui::test]
	fn permission_icon_keeps_toolbar_compact_and_speed_in_model_menu(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.new_work_conversation(cx);
		});
		for width in [1_180., 1_400.] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(1_000.)));
			visual.update(crate::ui_frame::draw_current_frame);
			let model = visual.debug_bounds("composer-model").unwrap();
			let permissions = visual.debug_bounds("composer-permissions").unwrap();
			assert_eq!(permissions.size.width, gpui::px(super::CONTROL_SIZE));
			assert!(visual.debug_bounds("composer-speed").is_none());
			assert!(model.right() < permissions.left());
		}
		surface.update(visual, |s, cx| s.toggle_composer_menu("model", cx));
		visual.update(crate::ui_frame::draw_current_frame);
		assert!(visual.debug_bounds("tier-default").is_some());
		assert!(visual.debug_bounds("tier-priority").is_some());
	}

	#[gpui::test]
	fn native_and_main_composers_keep_the_same_geometry(cx: &mut gpui::TestAppContext) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));

		visual.simulate_resize(gpui::size(gpui::px(1_400.), gpui::px(1_200.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			cx.notify();
		});
		visual.update(crate::ui_frame::draw_current_frame);

		let main = visual.debug_bounds("agent-composer").unwrap();
		let main_send = visual.debug_bounds("composer-send").unwrap();

		surface.update(visual, |s, cx| {
			s.native_agents.selected = Some(("agent".into(), "child".into()));
			s.native_agents.connection =
				crate::shell::agent_surface::native_agents::NativeConnection::Ready;

			cx.notify();
		});

		visual.update(crate::ui_frame::draw_current_frame);

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
	fn native_completion_stops_response_ui_before_the_snapshot_catches_up(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.composer.update(cx, |input, cx| input.clear(cx));

			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "agent")
				.unwrap();

			work.codex_thread_id = Some("thread".into());
			work.active_turn_id = Some("turn".into());
			work.dispatch_state = AgentDispatchStateDto::Running;

			let work = work.clone();

			s.timeline.native.binding = Some(Binding {
				work: work.id.clone(),
				thread: "thread".into(),
				account: "account".into(),
			});

			assert!(s.response_in_progress(&work));
			assert!(s.stop_button(cx));

			for status in ["completed", "interrupted", "failed"] {
				s.timeline.native.entries = vec![AgentTimelineEntry {
					position: 1,
					content: AgentTimelineContent::TurnBoundary {
						turn_id: "turn".into(),
						completed: true,
						status: Some(status.into()),
						duration_ms: None,
						timing: None,
						usage_summary: None,
						usage: None,
						error: None,
					},
				}];

				assert!(!s.response_in_progress(&work));
				assert!(s.running_turn().is_none());
				assert!(!s.stop_button(cx));
			}

			let mut newer = work.clone();

			newer.active_turn_id = Some("next-turn".into());

			assert!(s.response_in_progress(&newer), "an old completion must not stop new work");

			s.timeline.native.binding.as_mut().unwrap().thread = "other-thread".into();

			assert!(s.response_in_progress(&work), "completion belongs to its exact conversation");
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

			s.feedback =
				crate::ui_feedback::Feedback::neutral("Message saved · Waiting for agent…");

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
				s.feedback.message, "No service profile is configured.",
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
			s.feedback =
				crate::ui_feedback::Feedback::neutral("Message saved · Waiting for agent…");

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

			assert_eq!(s.feedback.message, "No service profile is configured.");

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
