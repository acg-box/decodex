//! Compact Chief composer. Controls apply to the next submitted message.
use super::*;
use decodex_protocol::ChiefAttachmentDto;
#[path = "chief_composer_controls.rs"] mod controls;

struct ComposerTip(String);
impl Render for ComposerTip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
			.px_3()
			.py_2()
			.rounded(px(7.0))
			.bg(rgb(0x242429))
			.text_size(px(11.0))
			.text_color(rgb(ui_theme::TEXT))
			.child(self.0.clone())
	}
}

struct QuoteTip(prompts::Quote);
impl Render for QuoteTip {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div()
			.p_3()
			.max_w(px(320.0))
			.rounded(px(8.0))
			.bg(rgb(0x242429))
			.text_size(px(11.0))
			.text_color(rgb(ui_theme::TEXT))
			.flex()
			.flex_col()
			.gap_1()
			.child(self.0.q.clone())
			.child(muted(format!("— {}", self.0.a)))
			.child(
				div()
					.id("quote-source")
					.role(Role::Link)
					.cursor_pointer()
					.text_color(rgb(ui_theme::BLUE))
					.child("Inspirational quotes provided by ZenQuotes API ↗")
					.on_click(|_, _, cx| cx.open_url("https://zenquotes.io/")),
			)
	}
}

impl ChiefSurface {
	pub(super) fn running_turn(&self) -> Option<(EntityId, WireText)> {
		let snapshot = self.snapshot.as_ref()?;
		let selected = self.selected.as_ref()?;
		let work = snapshot.work_items.iter().find(|work| &work.id == selected)?;
		if work.dispatch_state != ChiefDispatchStateDto::Running {
			return None;
		}
		Some((
			EntityId::new(work.id.clone()).ok()?,
			WireText::new(work.active_turn_id.clone()?).ok()?,
		))
	}

	pub(super) fn configured_send(
		&self,
		root_id: EntityId,
		text: HistoryText,
		execution: decodex_protocol::ConversationExecutionSettings,
		attachments: Vec<ChiefAttachmentDto>,
	) -> ChiefActionDto {
		if let Some((work_id, turn_id)) =
			self.running_turn().filter(|(id, _)| self.steer && id == &root_id)
		{
			ChiefActionDto::Steer { work_id, turn_id, text, attachments }
		} else {
			ChiefActionDto::SendConfigured { root_id, text, execution, attachments }
		}
	}

	fn stop_button(&self, cx: &Context<Self>) -> bool {
		self.running_turn().is_some()
			&& self.composer.read(cx).content().trim().is_empty()
			&& self.attachments.is_empty()
	}

	pub(crate) fn interrupt_current(&mut self, cx: &mut Context<Self>) {
		if let Some((work_id, turn_id)) = self.running_turn() {
			self.execute(ChiefActionDto::Interrupt { work_id, turn_id }, None, cx);
		}
	}

	pub(super) fn refresh_prompt(cx: &mut Context<Self>) {
		#[cfg(not(test))]
		{
			let fetch = cx.background_executor().spawn(async { prompts::refresh_cache() });
			cx.spawn(async move |surface, cx| {
				if fetch.await {
					let _ = surface.update(cx, |s, cx| {
						if s.composer.read(cx).content().is_empty() && !s.sending {
							s.composer
								.update(cx, |input, cx| input.set_placeholder(prompts::next(), cx));
						}
					});
				}
			})
			.detach();
		}
		#[cfg(test)]
		let _ = cx;
	}

	pub(super) fn render_composer(
		&self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let menu = self.composer_menu.or(self.composer_menu_content);
		let left = matches!(menu, Some("attachments" | "microphone"));
		let quote = prompts::attribution(self.composer.read(cx).placeholder());
		let editor = div()
			.id("composer-editor-area")
			.flex_1()
			.min_w_0()
			.when(self.composer.read(cx).content().is_empty(), |d| {
				if let Some(quote) = quote {
					d.tooltip(move |_, cx| cx.new(|_| QuoteTip(quote.clone())).into())
				} else {
					d
				}
			})
			.on_action(cx.listener(|s, _: &SubmitComposer, _, cx| {
				s.submit(cx);
				cx.stop_propagation();
			}))
			.when(self.voice.is_none(), |d| d.child(self.composer.clone()))
			.children(self.voice_controls(window, cx));
		div().w_full().px_4().pt(px(12.)).pb(px(20.)).flex().justify_center().child(
			div()
				.id("chief-composer")
				.relative()
				.w_full()
				.max_w(px(820.))
				.min_w_0()
				.px(px(10.))
				.py(px(7.))
				.rounded(px(24.))
				.bg(rgba(0x292c34f5))
				.shadow(vec![gpui::BoxShadow {
					inset: false,
					color: rgba(0x0000001a).into(),
					offset: gpui::point(px(0.), px(4.)),
					blur_radius: px(16.),
					spread_radius: px(-5.),
				}])
				.flex()
				.flex_col()
				.gap(px(4.))
				.on_key_down(cx.listener(|s, e: &gpui::KeyDownEvent, _, cx| {
					if e.keystroke.key == "escape" {
						s.cancel_dictation(cx);
						s.composer_menu = None;
						cx.notify();
						cx.stop_propagation();
					}
				}))
				.on_drop(cx.listener(|s, paths: &gpui::ExternalPaths, _, cx| {
					s.attach_paths(paths.0.to_vec(), cx)
				}))
				.children(if self.voice.is_none() { self.attachment_row(cx) } else { None })
				.child(
					div()
						.w_full()
						.flex()
						.items_center()
						.gap(px(6.))
						.child(self.composer_control(
							"attach",
							"+".into(),
							"Attachments and microphone",
							|s, cx| s.toggle_composer_menu("attachments", cx),
							cx,
						))
						.child(editor)
						.child(self.composer_toolbar(cx)),
				)
				.child(
					gpui::deferred(
						div()
							.absolute()
							.bottom(gpui::relative(1.))
							.mb(px(8.))
							.when(left, |d| d.left(px(0.)))
							.when(!left, |d| d.right(px(64.)))
							.w(px(if left {
								280.
							} else if menu == Some("effort") {
								264.
							} else {
								232.
							}))
							.child(crate::ui_motion::popover(
								menu.unwrap_or("model"),
								self.composer_menu.is_some(),
								self.composer_options(cx)
									.unwrap_or_else(|| div().into_any_element()),
							)),
					)
					.priority(2),
				),
		)
	}

	fn attachment_options(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let device =
			if self.audio_input.is_empty() { "System default" } else { self.audio_input.as_str() };
		div()
			.flex()
			.flex_col()
			.gap(px(3.))
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
			.child(self.composer_control_with_window(
				"audio-item",
				device.to_owned(),
				"Choose microphone",
				|s, window, cx| s.open_audio_menu(window, cx),
				cx,
			))
			.child(self.composer_control(
				"delivery",
				if self.steer { "Steer" } else { "Queue" }.into(),
				"Switch message delivery",
				|s, cx| {
					s.steer = !s.steer;
					cx.notify();
				},
				cx,
			))
			.into_any_element()
	}

	fn composer_toolbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		if let Some(controls) = self.dictation_controls(cx) {
			return controls;
		}
		if let Some(controls) = self.voice_toolbar(cx) {
			return controls;
		}
		self.text_composer_toolbar(cx)
	}

	fn text_composer_toolbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
		let model = self.model.read(cx).content().to_owned();
		div()
			.flex_none()
			.flex()
			.items_center()
			.gap(px(4.0))
			.children(self.usage_line())
			.child(self.composer_control(
				"fast",
				"Fast".into(),
				"Toggle Fast",
				|s, cx| {
					if s.selected_model(cx).is_some_and(|m| m.supports_fast) {
						s.fast = !s.fast;
					}
					cx.notify();
				},
				cx,
			))
			.child(self.composer_control(
				"model",
				model,
				"Choose model · Applies to the next turn",
				|s, cx| s.toggle_composer_menu("model", cx),
				cx,
			))
			.child(div().text_color(rgb(ui_theme::TEXT_MUTED)).child("·"))
			.child(self.composer_control(
				"effort",
				self.effort.as_str().into(),
				"Adjust reasoning",
				|s, cx| s.toggle_composer_menu("effort", cx),
				cx,
			))
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
				if self.stop_button(cx) {
					"Stop response · Control-C"
				} else if self.composer.read(cx).content().trim().is_empty()
					&& self.attachments.is_empty()
				{
					"Start Live"
				} else {
					"Send · Enter"
				},
				|s, window, cx| {
					if s.stop_button(cx) {
						s.interrupt_current(cx);
					} else if s.composer.read(cx).content().trim().is_empty()
						&& s.attachments.is_empty()
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
		let target = cx.entity().downgrade();
		let tooltip = if id == "model" { "Choose model".to_owned() } else { tip.to_owned() };
		div()
			.id(SharedString::from(format!("composer-{id}")))
			.role(Role::Button)
			.tab_index(0)
			.aria_label(tooltip.clone())
			.h(px(ui_theme::CONTROL_SIZE))
			.px(px(6.0))
			.flex_none()
			.rounded(px(if send { 8.0 } else { 7.0 }))
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
					"audio-back",
				]
				.contains(&id),
				|d| d.w(px(ui_theme::CONTROL_SIZE)).px_0(),
			)
			.when(id == "microphone-device", |d| d.w(px(16.)).px_0())
			.flex()
			.items_center()
			.justify_center()
			.text_size(px(12.0))
			.line_height(px(16.0))
			.text_color(rgb(if send {
				ui_theme::TEXT
			} else if id == "fast" && self.fast {
				ui_theme::BLUE
			} else {
				ui_theme::TEXT_MUTED
			}))
			.when(id == "model", |d| d.px(px(6.)))
			.when(["attachment-item", "audio-item", "audio-back", "delivery"].contains(&id), |d| {
				d.w_full().justify_start().text_size(px(12.))
			})
			.when(self.composer_menu == Some(id), |d| d.bg(rgba(0xb8acf21a)))
			.when(send, |d| d.w(px(28.)).h(px(28.)).rounded_full().ml(px(5.)).bg(rgb(0x555b6b)))
			.cursor_pointer()
			.hover(move |d| d.bg(if send { rgba(0xffffff24) } else { rgba(0xffffff0c) }))
			.tooltip(move |_, cx| cx.new(|_| ComposerTip(tooltip.clone())).into())
			.on_click(cx.listener(move |s, _, window, cx| action(s, window, cx)))
			.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, window, cx| {
				if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
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

	fn composer_control_content(
		&self,
		id: &str,
		label: String,
		cx: &Context<Self>,
	) -> gpui::AnyElement {
		use super::super::workspace_symbols::{Symbol, icon};
		match id {
			"send" if self.dictation.is_some() => div().child("✓").into_any_element(),
			"send" if self.stop_button(cx) =>
				div().size(px(9.0)).rounded(px(2.0)).bg(rgb(ui_theme::CANVAS)).into_any_element(),
			"send"
				if self.composer.read(cx).content().trim().is_empty()
					&& self.attachments.is_empty() =>
				controls::live_mark(),
			"send" if !self.sending => controls::launch_mark().into_any_element(),
			"send" => div().child("…").into_any_element(),
			"attach" => icon(Symbol::Plus),
			"attachment-item" => div()
				.flex()
				.items_center()
				.gap(px(10.))
				.child(icon(Symbol::Plus))
				.child("Add attachments…")
				.into_any_element(),
			"audio-item" => div()
				.w_full()
				.flex()
				.items_center()
				.gap(px(10.))
				.child(icon(Symbol::Microphone))
				.child("Microphone")
				.child(div().flex_1())
				.child(
					div()
						.max_w(px(110.))
						.text_ellipsis()
						.text_color(rgb(ui_theme::TEXT_MUTED))
						.child(label),
				)
				.child(icon(Symbol::Forward))
				.into_any_element(),
			"audio-back" => div()
				.flex()
				.items_center()
				.gap(px(10.))
				.child(icon(Symbol::Back))
				.child("Microphone")
				.into_any_element(),

			"voice" => icon(Symbol::Voice),
			"dictation" => icon(Symbol::Microphone),
			"microphone-device" =>
				div().size(px(12.)).child(icon(Symbol::ChevronDown)).into_any_element(),
			"fast" => div()
				.flex()
				.items_center()
				.gap(px(3.))
				.opacity(if self.fast { 1.0 } else { 0.45 })
				.child(icon(Symbol::Fast))
				.into_any_element(),
			"delivery" => div()
				.w_full()
				.flex()
				.items_center()
				.justify_between()
				.child("Message delivery")
				.child(div().text_color(rgb(ui_theme::TEXT)).child(label))
				.into_any_element(),
			"effort" => controls::effort_indicator(self.effort.as_str()),
			"model" => div()
				.text_color(rgb(ui_theme::TEXT))
				.child(controls::compact_model_label(&label))
				.into_any_element(),
			_ => div().child(label).into_any_element(),
		}
	}

	fn toggle_composer_menu(&mut self, name: &'static str, cx: &mut Context<Self>) {
		self.composer_menu = if self.composer_menu == Some(name) { None } else { Some(name) };
		if self.composer_menu.is_some() {
			self.composer_menu_content = self.composer_menu;
			self.load_capabilities(cx);
		}
		cx.notify();
	}

	fn composer_options(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
		// Keep content mounted while the disclosure animates closed.
		let menu = self.composer_menu.or(self.composer_menu_content)?;
		Some(
			div()
				.id("composer-menu-popover")
				.occlude()
				.on_mouse_down_out(cx.listener(|s, event: &gpui::MouseDownEvent, _, cx| {
					if s.menu_trigger_bounds.values().any(|bounds| bounds.contains(&event.position))
					{
						return;
					}
					s.composer_menu = None;
					s.effort_drag = None;
					s.effort_pointer = None;
					cx.notify();
				}))
				.p(px(10.))
				.w_full()
				.flex()
				.flex_col()
				.gap(px(10.))
				.child(if menu == "attachments" {
					self.attachment_options(cx)
				} else if menu == "microphone" {
					div()
						.flex()
						.flex_col()
						.gap(px(6.))
						.child(self.composer_control(
							"audio-back",
							"Microphone".into(),
							"Back to attachments",
							|s, cx| {
								s.composer_menu = Some("attachments");
								s.composer_menu_content = s.composer_menu;
								cx.notify();
							},
							cx,
						))
						.child(self.audio_palette(cx))
						.into_any_element()
				} else if menu == "effort" {
					self.effort_scale(cx)
				} else {
					div()
						.flex()
						.flex_col()
						.gap(px(5.))
						.child(self.model_palette(cx))
						.into_any_element()
				})
				.into_any_element(),
		)
	}

	fn select_composer_option(&mut self, menu: &str, value: &str, cx: &mut Context<Self>) {
		if menu == "model" {
			self.model.update(cx, |input, cx| input.set_content(value, cx));
			self.reconcile_model_options(cx);
		} else {
			self.effort = match value {
				"none" => ConversationReasoningEffort::None,
				"minimal" => ConversationReasoningEffort::Minimal,
				"low" => ConversationReasoningEffort::Low,
				"medium" => ConversationReasoningEffort::Medium,
				"xhigh" => ConversationReasoningEffort::XHigh,
				"max" => ConversationReasoningEffort::Max,
				"ultra" => ConversationReasoningEffort::Ultra,
				_ => ConversationReasoningEffort::High,
			};
		}
		self.composer_menu = None;
		cx.notify();
	}

	pub(super) fn usage_line(&self) -> Option<gpui::AnyElement> {
		let (id, ChiefHistoryResult::Available { usage: Some(usage), .. }) =
			self.history.as_ref()?
		else {
			return None;
		};
		if self.selected.as_ref() != Some(id) || usage.context_tokens == 0 {
			return None;
		}
		let capacity = usage.context_window.filter(|size| *size > 0)?;
		let percent = usage.context_tokens as f64 / capacity as f64 * 100.0;
		let detail = format!(
			"Context · {percent:.0}%\n{} / {} tokens",
			compact_tokens(usage.context_tokens),
			compact_tokens(capacity)
		);
		Some(
			div()
				.id("composer-context")
				.size(px(ui_theme::CONTROL_SIZE))
				.flex_none()
				.flex()
				.items_center()
				.text_size(px(10.5))
				.text_color(rgb(ui_theme::TEXT_MUTED))
				.aria_label(format!("Context {percent:.0}%"))
				.tooltip(move |_, cx| cx.new(|_| ComposerTip(detail.clone())).into())
				.justify_center()
				.child(context_ring((percent / 100.0).clamp(0.0, 1.0) as f32))
				.into_any_element(),
		)
	}

	fn attachment_row(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
		if self.attachments.is_empty() {
			return None;
		}
		let mut row = div().flex().flex_wrap().gap_1().px_1();
		for file in &self.attachments {
			let path = std::path::PathBuf::from(file.path.as_str());
			let label = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
			let remove = file.clone();
			row = row.child(
				div()
					.id(SharedString::from(format!("attachment-{}", file.path.as_str())))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Remove attachment {label}"))
					.h(px(30.0))
					.max_w(px(220.0))
					.px_2()
					.flex()
					.items_center()
					.gap_2()
					.rounded(px(6.0))
					.bg(rgba(0xffffff0a))
					.text_size(px(11.0))
					.cursor_pointer()
					.when(file.image, |d| d.child(gpui::img(path).size(px(24.0)).rounded(px(3.0))))
					.child(div().min_w_0().overflow_hidden().text_ellipsis().child(label))
					.child("×")
					.on_click(cx.listener({
						let remove = remove.clone();
						move |s, _, _, cx| {
							s.attachments.retain(|f| f != &remove);
							cx.notify();
						}
					}))
					.on_key_down(cx.listener(move |s, e: &gpui::KeyDownEvent, _, cx| {
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
		let result = cx.prompt_for_paths(gpui::PathPromptOptions {
			files: true,
			directories: false,
			multiple: true,
			prompt: Some("Add to message".into()),
		});
		cx.spawn(async move |s, cx| {
			if let Ok(Ok(Some(paths))) = result.await {
				let _ = s.update(cx, |s, cx| s.attach_paths(paths, cx));
			}
		})
		.detach();
	}

	fn attach_paths(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
		for path in paths {
			if self.attachments.len() >= 16 {
				self.feedback = "Attach at most 16 files.".into();
				break;
			}
			let Some(path) = path.canonicalize().ok().filter(|p| p.is_file()) else {
				self.feedback = "The attached file is not available.".into();
				continue;
			};
			let image = path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
				["png", "jpg", "jpeg", "webp", "gif"].contains(&s.to_ascii_lowercase().as_str())
			});
			let Ok(path) = ConversationWorkingDirectory::new(path.to_string_lossy().as_ref())
			else {
				continue;
			};
			let file = ChiefAttachmentDto { path, image };
			if !self.attachments.contains(&file) {
				self.attachments.push(file);
			}
		}
		cx.notify();
	}

	pub(super) fn attach_clipboard(&mut self, item: &ClipboardItem, cx: &mut Context<Self>) {
		for entry in &item.entries {
			match entry {
				gpui::ClipboardEntry::ExternalPaths(paths) =>
					self.attach_paths(paths.0.to_vec(), cx),
				gpui::ClipboardEntry::Image(image) => match save_clipboard_image(image) {
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

fn model_label(model: &str) -> String {
	let mut parts = model.split('-');
	let first = parts.next().unwrap_or_default().to_uppercase();
	let version = parts.next().unwrap_or_default();
	let family = parts
		.map(|part| {
			let mut chars = part.chars();
			chars
				.next()
				.map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
				.unwrap_or_default()
		})
		.collect::<Vec<_>>()
		.join(" ");
	format!("{first}-{version} {family}").trim().into()
}

fn save_clipboard_image(image: &gpui::Image) -> std::io::Result<std::path::PathBuf> {
	use std::{
		io::Write,
		os::unix::fs::{DirBuilderExt, OpenOptionsExt},
	};
	let home = std::env::var_os("HOME")
		.ok_or_else(|| std::io::Error::other("Home directory unavailable"))?;
	let dir = std::path::PathBuf::from(home).join(".decodex/attachments");
	std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
	let ext = match image.format {
		gpui::ImageFormat::Png => "png",
		gpui::ImageFormat::Jpeg => "jpg",
		gpui::ImageFormat::Webp => "webp",
		gpui::ImageFormat::Gif => "gif",
		_ => return Err(std::io::Error::other("Paste a PNG, JPEG, WebP, or GIF image")),
	};
	let path = dir.join(format!("{}.{ext}", unique_command()));
	std::fs::OpenOptions::new()
		.write(true)
		.create_new(true)
		.mode(0o600)
		.open(&path)?
		.write_all(&image.bytes)?;
	Ok(path)
}

fn context_ring(fraction: f32) -> impl IntoElement {
	gpui::canvas(
		|_, _, _| (),
		move |bounds, _, window, _| {
			for (portion, color) in [(1.0, rgba(0xffffff24)), (fraction, rgba(0xc2becbe0))] {
				if portion <= 0.0 {
					continue;
				}
				let mut path = gpui::PathBuilder::stroke(px(1.6));
				let steps = (portion * 64.0).ceil() as usize;
				for step in 0..=steps {
					let angle = -std::f32::consts::FRAC_PI_2
						+ std::f32::consts::TAU * portion * step as f32 / steps as f32;
					let point =
						bounds.center() + gpui::point(px(angle.cos() * 5.7), px(angle.sin() * 5.7));
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
	.size(px(16.0))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[gpui::test]
	fn model_selection_uses_native_default_and_capabilities(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			s.capabilities = Some(decodex_protocol::ChiefCapabilitiesResult::Available {
				models: vec![decodex_protocol::ChiefModelDto {
					model: ConversationModel::new("custom-model").unwrap(),
					name: "Custom".into(),
					efforts: vec![
						ConversationReasoningEffort::Low,
						ConversationReasoningEffort::Medium,
					],
					default_effort: Some(ConversationReasoningEffort::Medium),
					supports_fast: false,
					supports_images: false,
				}],
				memory_enabled: Some(true),
			});
			s.effort = ConversationReasoningEffort::Ultra;
			s.fast = true;
			s.select_composer_option("model", "custom-model", cx);
			assert_eq!(s.effort, ConversationReasoningEffort::Medium);
			assert!(!s.fast);
			assert_eq!(s.model_efforts(cx).len(), 2);
			s.select_composer_option("effort", "low", cx);
			s.select_composer_option("model", "custom-model", cx);
			assert_eq!(s.effort, ConversationReasoningEffort::Low);
			assert!(s.composer_menu.is_none());
		});
	}

	#[gpui::test]
	fn delivery_mode_and_stop_follow_the_selected_turn(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			s.visual_workspace_fixture(cx);
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "chief")
				.unwrap();
			work.dispatch_state = ChiefDispatchStateDto::Running;
			work.active_turn_id = Some("exact-turn".into());
			let action = |s: &ChiefSurface| {
				s.configured_send(
					EntityId::new("chief").unwrap(),
					HistoryText::new("Supplement").unwrap(),
					decodex_protocol::ConversationExecutionSettings {
						model: ConversationModel::new("gpt-6-astra").unwrap(),
						reasoning_effort: s.effort,
						fast: false,
					},
					vec![],
				)
			};
			assert!(
				matches!(action(s),ChiefActionDto::Steer {turn_id,..} if turn_id.as_str()=="exact-turn")
			);
			s.composer.update(cx, |i, cx| i.clear(cx));
			assert!(s.stop_button(cx));
			s.uncertain = true;
			s.interrupt_current(cx);
			assert_eq!(s.feedback, "No service profile is configured.");
			s.uncertain = false;
			s.composer.update(cx, |i, cx| i.set_content("Supplement", cx));
			assert!(!s.stop_button(cx));
			s.steer = false;
			assert!(matches!(action(s), ChiefActionDto::SendConfigured { .. }));
			s.steer = true;
			let work = s
				.snapshot
				.as_mut()
				.unwrap()
				.work_items
				.iter_mut()
				.find(|w| w.id == "chief")
				.unwrap();
			work.dispatch_state = ChiefDispatchStateDto::Idle;
			work.active_turn_id = None;
			assert!(matches!(action(s), ChiefActionDto::SendConfigured { .. }));
			assert!(!s.stop_button(cx));
		});
	}
}
