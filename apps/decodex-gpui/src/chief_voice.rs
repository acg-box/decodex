//! Live media controls; subscription signaling and task execution stay in the service.
use super::*;
use decodex_protocol::{ChiefVoicePhase, ChiefVoiceRequest, VoiceSdp};
use raw_window_handle as _;
use serde_json::{Value, json};
use std::time::Duration;

pub(super) struct VoiceUi {
	media: Media,
	session: EntityId,
	work: EntityId,
	request: Option<ChiefVoiceRequest>,
	answered: bool,
	signaling: bool,
	connected: bool,
	connection_status: String,
	muted: bool,
	caption: String,
	caption_role: &'static str,
	caption_turn: String,
}
impl ChiefSurface {
	pub(crate) fn stop_voice(&mut self, cx: &mut Context<Self>) {
		self.cancel_dictation(cx);
		self.voice = None;
		cx.notify();
	}

	pub(super) fn start_voice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.voice_task.is_some() || self.dictation_task.is_some() {
			return;
		}
		let Some(profile) = self.profile.clone() else { return };
		let Some(work) = self
			.composer_manager
			.clone()
			.or_else(|| self.root_id())
			.and_then(|id| EntityId::new(id).ok())
		else {
			self.feedback = "Start a Chief conversation before opening Live voice.".into();
			cx.notify();
			return;
		};
		let mut media = match Media::new(window) {
			Ok(media) => media,
			Err(()) => {
				self.feedback = "Live voice requires the current signed Decodex.app build.".into();
				cx.notify();
				return;
			},
		};
		if !media.command(json!({"operation":"start"})) {
			self.feedback = "The audio host could not start.".into();
			cx.notify();
			return;
		}
		let session = EntityId::new(unique_command()).expect("bounded voice identity");
		self.voice = Some(VoiceUi {
			media,
			session: session.clone(),
			work,
			request: None,
			answered: false,
			signaling: false,
			connected: false,
			connection_status: "Preparing audio…".into(),
			muted: false,
			caption: String::new(),
			caption_role: "You",
			caption_turn: String::new(),
		});
		self.voice_task = Some(cx.spawn(async move |surface, cx| {
			loop {
				let request = surface.update(cx, |s, cx| s.poll_voice_media(cx)).ok().flatten();
				let Some(request) = request else {
					let profile = profile.clone();
					let session = session.clone();
					cx.background_executor()
						.spawn(async move {
							if let Ok(runtime) =
								tokio::runtime::Builder::new_current_thread().enable_all().build()
							{
								let _ =
									runtime
										.block_on(ChiefClient::new(profile).voice(
											ChiefVoiceRequest::Stop { session_id: session },
										));
							}
						})
						.await;
					break;
				};
				// Wait for the local offer before asking the service to start a call.
				if let Some(request) = request {
					let profile = profile.clone();
					let result = cx
						.background_executor()
						.spawn(async move {
							let runtime = tokio::runtime::Builder::new_current_thread()
								.enable_all()
								.build()
								.ok()?;
							runtime.block_on(ChiefClient::new(profile).voice(request)).ok()
						})
						.await;
					let _ = surface.update(cx, |s, cx| {
						if let Some(result) = result {
							s.apply_voice_status(result, cx);
						}
					});
				}
				cx.background_executor().timer(Duration::from_millis(100)).await;
			}
			let _ = surface.update(cx, |s, cx| {
				s.voice_task = None;
				cx.notify();
			});
		}));
		cx.notify();
	}

	fn poll_voice_media(&mut self, cx: &mut Context<Self>) -> Option<Option<ChiefVoiceRequest>> {
		let voice = self.voice.as_mut()?;
		for _ in 0..128 {
			let Some(event) = voice.media.poll() else { break };
			match event["type"].as_str() {
				Some("offer") => {
					let offer = event["sdp"].as_str().and_then(|s| VoiceSdp::new(s.into()).ok())?;
					voice.signaling = true;
					voice.connection_status = "Connecting…".into();
					voice.request = Some(ChiefVoiceRequest::Start {
						session_id: voice.session.clone(),
						work_id: voice.work.clone(),
						offer,
					});
				},
				Some("connected") => voice.connected = true,
				Some("status") =>
					voice.connection_status =
						event["message"].as_str().unwrap_or("Connecting…").into(),
				Some("caption") => {
					update_caption(
						&event["event"],
						&mut voice.caption_turn,
						&mut voice.caption_role,
						&mut voice.caption,
					);
				},
				Some("error" | "ended") => {
					if event["type"] == "error" {
						self.feedback =
							event["message"].as_str().unwrap_or("Voice disconnected.").into();
					}
					self.voice = None;
					cx.notify();
					return None;
				},
				_ => {},
			}
		}
		cx.notify();
		Some(voice.request.take().or_else(|| {
			voice.signaling.then(|| ChiefVoiceRequest::Poll { session_id: voice.session.clone() })
		}))
	}

	fn apply_voice_status(
		&mut self,
		status: decodex_protocol::ChiefVoiceStatus,
		cx: &mut Context<Self>,
	) {
		let Some(voice) = self.voice.as_mut().filter(|v| v.session == status.session_id) else {
			return;
		};
		match status.phase {
			ChiefVoicePhase::Connecting => {
				voice.request = Some(ChiefVoiceRequest::Poll { session_id: voice.session.clone() });
			},
			ChiefVoicePhase::Ready => {
				if !voice.answered
					&& let Some(answer) = status.answer
				{
					voice.answered =
						voice.media.command(json!({"operation":"answer","sdp":answer.as_str()}));
				}
			},
			ChiefVoicePhase::Ended | ChiefVoicePhase::Failed => {
				if let Some(message) = status.message {
					self.feedback = message.as_str().into();
				}
				self.voice = None;
			},
		}
		cx.notify();
	}

	pub(super) fn voice_controls(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
		let voice = self.voice.as_ref()?;
		let label = if !voice.connected {
			voice.connection_status.clone()
		} else if voice.muted {
			"Microphone muted".into()
		} else {
			"Live".into()
		};
		Some(
			div()
				.w_full()
				.flex()
				.flex_col()
				.gap(px(8.))
				.pb(px(8.))
				.border_b_1()
				.border_color(rgba(0xffffff14))
				.child(
					div()
						.flex()
						.items_center()
						.gap(px(8.))
						.text_size(px(11.))
						.text_color(rgb(ui_theme::TEXT_MUTED))
						.child(div().size(px(5.)).rounded_full().bg(rgb(if voice.connected {
							0x8eb6a1
						} else {
							0x8b8893
						})))
						.child(
							div()
								.id("voice-status")
								.role(Role::Status)
								.aria_label(label.clone())
								.child(label),
						)
						.child(div().flex_1())
						.child(self.composer_control(
							"voice-mute",
							if voice.muted { "Unmute" } else { "Mute" }.into(),
							"Toggle microphone",
							|s, cx| {
								if let Some(voice) = &mut s.voice {
									voice.muted = !voice.muted;
									voice
										.media
										.command(json!({"operation":"mute","muted":voice.muted}));
									cx.notify();
								}
							},
							cx,
						))
						.child(self.composer_control(
							"voice-end",
							"End".into(),
							"End voice call · Existing work continues",
							|s, cx| {
								s.voice = None;
								cx.notify();
							},
							cx,
						)),
				)
				.when(!voice.caption.is_empty(), |d| {
					d.child(
						div()
							.max_h(px(72.))
							.overflow_hidden()
							.text_size(px(12.))
							.text_color(rgb(ui_theme::TEXT))
							.child(format!("{} · {}", voice.caption_role, voice.caption)),
					)
				})
				.into_any_element(),
		)
	}
}

#[cfg(all(target_os = "macos", not(test)))]
pub(super) struct Media {
	host: *mut std::ffi::c_void,
	command_fn: unsafe extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_char) -> bool,
	poll_fn: unsafe extern "C" fn(*mut std::ffi::c_void) -> *const std::ffi::c_char,
	destroy: unsafe extern "C" fn(*mut std::ffi::c_void),
	_main_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}
#[cfg(all(target_os = "macos", not(test)))]
impl Media {
	pub(super) fn new(window: &Window) -> Result<Self, ()> {
		use crate::native_menu_bar::{bundled_library_path, symbol};
		use std::{ffi::CString, os::unix::ffi::OsStrExt as _};
		let path =
			bundled_library_path(&std::env::current_exe().map_err(|_| ())?).map_err(|_| ())?;
		if !std::fs::symlink_metadata(&path).map_err(|_| ())?.file_type().is_file() {
			return Err(());
		}
		let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| ())?;
		// SAFETY: fixed signed-app library and exact versioned C ABI; this object cannot cross
		// threads.
		unsafe {
			let image = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
			if image.is_null() {
				return Err(());
			}
			let version: unsafe extern "C" fn() -> u32 =
				symbol(image, c"decodex_voice_media_abi_version").map_err(|_| ())?;
			if version() != 2 {
				return Err(());
			}
			let create: unsafe extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void =
				symbol(image, c"decodex_voice_media_create").map_err(|_| ())?;
			let command_fn = symbol(image, c"decodex_voice_media_command").map_err(|_| ())?;
			let poll_fn = symbol(image, c"decodex_voice_media_poll").map_err(|_| ())?;
			let destroy = symbol(image, c"decodex_voice_media_destroy").map_err(|_| ())?;
			let native =
				raw_window_handle::HasWindowHandle::window_handle(window).map_err(|_| ())?;
			let raw_window_handle::RawWindowHandle::AppKit(handle) = native.as_raw() else {
				return Err(());
			};
			let host = create(handle.ns_view.as_ptr());
			if host.is_null() {
				return Err(());
			}
			// Keep the image loaded: WebKit can complete cleanup asynchronously.
			Ok(Self { host, command_fn, poll_fn, destroy, _main_thread: std::marker::PhantomData })
		}
	}

	pub(super) fn command(&mut self, value: Value) -> bool {
		let Ok(text) = std::ffi::CString::new(value.to_string()) else { return false };
		// SAFETY: retained native host; copied UTF-8 argument lives through the synchronous call.
		unsafe { (self.command_fn)(self.host, text.as_ptr()) }
	}

	pub(super) fn poll(&mut self) -> Option<Value> {
		// SAFETY: native data remains valid until the next poll or destroy. Copy it immediately.
		unsafe {
			let event = (self.poll_fn)(self.host);
			if event.is_null() {
				None
			} else {
				serde_json::from_slice(std::ffi::CStr::from_ptr(event).to_bytes()).ok()
			}
		}
	}
}
#[cfg(all(target_os = "macos", not(test)))]
impl Drop for Media {
	fn drop(&mut self) {
		self.command(json!({"operation":"stop"}));
		// SAFETY: unique host, destroyed exactly once on the GPUI main thread.
		unsafe { (self.destroy)(self.host) };
	}
}
#[cfg(any(not(target_os = "macos"), test))]
pub(super) struct Media;
#[cfg(any(not(target_os = "macos"), test))]
impl Media {
	pub(super) fn new(_: &Window) -> Result<Self, ()> {
		Err(())
	}

	pub(super) fn command(&mut self, _: Value) -> bool {
		false
	}

	pub(super) fn poll(&mut self) -> Option<Value> {
		None
	}
}

/// Use turn identity so delayed final text cannot replace the other speaker's caption.
fn update_caption(event: &Value, turn: &mut String, role: &mut &'static str, text: &mut String) {
	match event["type"].as_str() {
		Some("turn.created") => {
			let Some(id) = event.pointer("/turn/id").and_then(Value::as_str) else { return };
			*turn = id.into();
			*role = if event.pointer("/turn/role").and_then(Value::as_str) == Some("assistant") {
				"Chief"
			} else {
				"You"
			};
			*text = event
				.pointer("/turn/transcript")
				.and_then(Value::as_str)
				.unwrap_or_default()
				.into();
		},
		Some("turn.delta") if event["turn_id"].as_str() == Some(turn.as_str()) => {
			text.push_str(event["delta"].as_str().unwrap_or_default());
		},
		Some("turn.done")
			if event.pointer("/turn/id").and_then(Value::as_str) == Some(turn.as_str()) =>
		{
			if let Some(final_text) = event.pointer("/turn/transcript").and_then(Value::as_str) {
				*text = final_text.into();
			}
		},
		_ => return,
	}
	if text.chars().count() > 220 {
		*text = text.chars().rev().take(220).collect::<String>().chars().rev().collect();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn captions_follow_turns_and_ignore_late_other_speaker_final() {
		let (mut turn, mut role, mut text) = (String::new(), "You", String::new());
		for event in [
			json!({"type":"turn.created","turn":{"id":"u","role":"user","transcript":"Hello"}}),
			json!({"type":"turn.delta","turn_id":"u","delta":" world"}),
		] {
			update_caption(&event, &mut turn, &mut role, &mut text);
		}
		assert_eq!(text, "Hello world");
		update_caption(
			&json!({"type":"turn.created","turn":{"id":"a","role":"assistant","transcript":"Hi"}}),
			&mut turn,
			&mut role,
			&mut text,
		);
		update_caption(
			&json!({"type":"turn.done","turn":{"id":"u","transcript":"Hello, world!"}}),
			&mut turn,
			&mut role,
			&mut text,
		);
		assert_eq!((role, text.as_str()), ("Chief", "Hi"));
		update_caption(
			&json!({"type":"turn.done","turn":{"id":"a","transcript":"Hi!"}}),
			&mut turn,
			&mut role,
			&mut text,
		);
		assert_eq!(text, "Hi!");
	}
}
