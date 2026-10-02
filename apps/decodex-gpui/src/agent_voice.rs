//! Live media controls; subscription signaling and task execution stay in the service.
#[cfg(target_os = "macos")]
#[path = "native_voice_audio.rs"]
mod audio;
#[cfg(target_os = "macos")]
#[path = "native_voice_transport.rs"]
mod transport;

#[cfg(all(target_os = "macos", not(test)))] use std::os::unix::ffi::OsStrExt as _;
use std::{collections::BTreeSet, time::Duration};

use gpui::{AnyElement, KeyDownEvent};
use raw_window_handle as _;
use serde_json::{Value, json};
use tokio::runtime::Builder;
use ui_theme::{BLUE, TEXT_MUTED};

#[cfg(all(target_os = "macos", not(test)))] use crate::native_menu_bar::{self};
#[cfg(test)] use crate::shell::agent_surface::{Entity, Render};
use crate::{
	shell::{
		agent_surface,
		agent_surface::{
			AgentClient, AgentHistoryResult, AgentSurface, Context, EntityId, InteractiveElement,
			IntoElement, ParentElement, Role, SmoothControl, StatefulInteractiveElement, Styled,
			Window, div, px, ui_theme,
		},
	},
	ui_motion,
	ui_theme::HOVER_FILL,
};
use decodex_protocol::{
	AgentVoiceOptions, AgentVoicePhase, AgentVoiceRequest, AgentVoiceStatus, HistoryText, VoiceSdp,
};

pub(super) struct VoiceUi {
	options: AgentVoiceOptions,
	media: Media,
	session: EntityId,
	work: EntityId,
	request: Option<AgentVoiceRequest>,
	answered: bool,
	signaling: bool,
	connected: bool,
	connection_status: String,
	muted: bool,
	captions: Vec<Caption>,
	matched_receipts: BTreeSet<i64>,
	levels: std::collections::VecDeque<f32>,
	follow: bool,
}

#[cfg(all(target_os = "macos", not(test)))]
pub(super) struct Media {
	host: *mut std::ffi::c_void,
	command_fn: unsafe extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_char) -> bool,
	poll_fn: unsafe extern "C" fn(*mut std::ffi::c_void) -> *const std::ffi::c_char,
	destroy: unsafe extern "C" fn(*mut std::ffi::c_void),
	audio: Option<audio::Device>,
	transport: Option<transport::Transport>,
	muted: bool,
	level_at: std::time::Instant,
	_main_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(any(not(target_os = "macos"), test))]
pub(super) struct Media;
#[cfg(all(target_os = "macos", not(test)))]
impl Media {
	pub(super) fn new(window: &Window) -> Result<Self, ()> {
		let path = native_menu_bar::bundled_library_path(&std::env::current_exe().map_err(|_| ())?)
			.map_err(|_| ())?;

		if !std::fs::symlink_metadata(&path).map_err(|_| ())?.file_type().is_file() {
			return Err(());
		}

		let path = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| ())?;

		// SAFETY: fixed signed-app library and exact versioned C ABI; this object cannot cross
		// threads.
		unsafe {
			let image = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);

			if image.is_null() {
				return Err(());
			}

			let version: unsafe extern "C" fn() -> u32 =
				native_menu_bar::symbol(image, c"decodex_voice_media_abi_version")
					.map_err(|_| ())?;

			if version() != 3 {
				return Err(());
			}

			let create: unsafe extern "C" fn(*mut std::ffi::c_void) -> *mut std::ffi::c_void =
				native_menu_bar::symbol(image, c"decodex_voice_media_create").map_err(|_| ())?;
			let command_fn =
				native_menu_bar::symbol(image, c"decodex_voice_media_command").map_err(|_| ())?;
			let poll_fn =
				native_menu_bar::symbol(image, c"decodex_voice_media_poll").map_err(|_| ())?;
			let destroy =
				native_menu_bar::symbol(image, c"decodex_voice_media_destroy").map_err(|_| ())?;
			let native =
				raw_window_handle::HasWindowHandle::window_handle(window).map_err(|_| ())?;
			let raw_window_handle::RawWindowHandle::AppKit(handle) = native.as_raw() else {
				return Err(());
			};
			let host = create(handle.ns_view.as_ptr());

			if host.is_null() {
				return Err(());
			}

			// Keep the signed platform library loaded for native callbacks.
			Ok(Self {
				host,
				command_fn,
				poll_fn,
				destroy,
				audio: None,
				transport: None,
				muted: false,
				level_at: std::time::Instant::now(),
				_main_thread: std::marker::PhantomData,
			})
		}
	}

	pub(super) fn command(&mut self, value: Value) -> bool {
		match value["operation"].as_str() {
			Some("answer") =>
				return value["sdp"].as_str().is_some_and(|sdp| {
					self.transport.as_ref().is_some_and(|transport| {
						transport.command(transport::Command::Answer(sdp.into()))
					})
				}),
			Some("mute") => {
				let muted = value["muted"].as_bool().unwrap_or(false);
				let accepted = self
					.transport
					.as_ref()
					.is_none_or(|transport| transport.command(transport::Command::Mute(muted)));

				if accepted {
					self.muted = muted;
				}

				return accepted;
			},
			Some("start" | "dictate" | "stop") => {
				self.audio = None;
				self.transport = None;
			},
			_ => {},
		}

		let Ok(text) = std::ffi::CString::new(value.to_string()) else { return false };

		// SAFETY: retained native host; argument is copied by the synchronous call.
		unsafe { (self.command_fn)(self.host, text.as_ptr()) }
	}

	pub(super) fn poll(&mut self) -> Option<Value> {
		// SAFETY: native event data lives until the next poll/destroy; copy it immediately.
		let platform: Option<Value> = unsafe {
			let event = (self.poll_fn)(self.host);

			if event.is_null() {
				None
			} else {
				serde_json::from_slice(std::ffi::CStr::from_ptr(event).to_bytes()).ok()
			}
		};

		if let Some(event) = platform {
			if event["type"] != "voice_authorized" {
				return Some(event);
			}

			let started = event["device"]
				.as_u64()
				.and_then(|id| u32::try_from(id).ok())
				.and_then(|id| audio::Device::start(id).ok())
				.and_then(|(device, pcm)| {
					transport::Transport::start(pcm).ok().map(|transport| (device, transport))
				});

			match started {
				Some((device, transport)) => {
					self.audio = Some(device);

					if self.muted {
						transport.command(transport::Command::Mute(true));
					}

					self.transport = Some(transport);
				},
				None =>
					return Some(
						json!({"type":"error","message":"The selected audio device could not start."}),
					),
			}
		}
		if let Some(transport) = &self.transport
			&& let Some(event) = transport.poll()
		{
			return Some(event);
		}
		if let Some(audio) = &self.audio {
			if !audio.running() {
				return Some(
					json!({"type":"error","message":"The audio device stopped. Select a device and start a new call."}),
				);
			}
			if self.level_at.elapsed() >= Duration::from_millis(50) {
				self.level_at = std::time::Instant::now();

				return Some(
					json!({"type":"level","level":if self.muted { 0.0 } else { (audio.level() * 5.0).min(1.0) }}),
				);
			}
		}

		None
	}
}

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

#[cfg(all(target_os = "macos", not(test)))]
impl Drop for Media {
	fn drop(&mut self) {
		self.command(json!({"operation":"stop"}));

		// SAFETY: unique host, destroyed exactly once on the GPUI main thread.
		unsafe { (self.destroy)(self.host) };
	}
}

pub(super) struct CaptionHistory {
	session: EntityId,
	work: EntityId,
	captions: Vec<Caption>,
	matched_receipts: BTreeSet<i64>,
}

#[derive(Clone, Debug)]
struct Caption {
	complete: bool,
	turn: String,
	role: &'static str,
	text: String,
}

impl AgentSurface {
	pub(crate) fn stop_voice(&mut self, cx: &mut Context<Self>) {
		self.cancel_dictation(cx);
		self.retire_voice_media();
		cx.notify();
	}

	pub(super) fn voice_read_action(
		&self,
		work: &str,
		identity: &str,
		text: &str,
		partial: bool,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let voice = self.voice.as_ref().filter(|v| {
			v.work.as_str() == work && v.connected && v.answered && v.request.is_none()
		})?;

		if text.trim().is_empty() {
			return None;
		}

		let (work, text, session) = (work.to_owned(), text.to_owned(), voice.session.clone());

		Some(self.workspace_action(
			format!("voice-read-{identity}"),
			if partial { "Read shown text" } else { "Read aloud" }.into(),
			move |s, cx| s.queue_voice_speech(&work, &session, &text, cx),
			cx,
		))
	}

	fn queue_voice_speech(
		&mut self,
		work: &str,
		session: &EntityId,
		text: &str,
		cx: &mut Context<Self>,
	) {
		let Some(voice) = self.voice.as_mut().filter(|v| {
			v.work.as_str() == work
				&& &v.session == session
				&& v.connected
				&& v.answered
				&& v.request.is_none()
		}) else {
			return;
		};

		if text.trim().is_empty() {
			return;
		}

		match HistoryText::new(text) {
			Ok(text) => {
				voice.request =
					Some(AgentVoiceRequest::Speak { session_id: session.clone(), text });
				self.feedback = "Sending read-aloud request…".into();
			},
			Err(_) => self.feedback = "This reply is too long to read aloud in one request.".into(),
		}

		cx.notify();
	}

	pub(super) fn start_voice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.selected_is_archived() || self.composer_unavailable_reason().is_some() {
			return;
		}
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
			self.feedback = "Start an Agent conversation before opening Live voice.".into();

			cx.notify();

			return;
		};
		let options = match self.voice_call_options(work.as_str(), cx) {
			Ok(options) => options,
			Err(message) => {
				self.feedback = message.into();

				cx.notify();

				return;
			},
		};
		let mut media = match Media::new(window) {
			Ok(media) => media,
			Err(()) => {
				self.feedback = "Live voice requires the current signed Decodex.app build.".into();

				cx.notify();

				return;
			},
		};

		if !media.command(json!({"operation":"start","input":self.audio_input})) {
			self.feedback = "The audio host could not start.".into();

			cx.notify();

			return;
		}

		let session =
			EntityId::new(agent_surface::unique_command()).expect("bounded voice identity");

		self.voice = Some(VoiceUi {
			options,
			media,
			session: session.clone(),
			work,
			request: None,
			answered: false,
			signaling: false,
			connected: false,
			connection_status: "Preparing audio…".into(),
			muted: false,
			captions: Vec::new(),
			matched_receipts: Default::default(),
			levels: std::collections::VecDeque::from(vec![0.; 40]),
			follow: true,
		});
		self.voice_task = Some(cx.spawn(async move |surface, cx| {
			loop {
				let request = surface.update(cx, |s, cx| s.poll_voice_media(cx)).ok().flatten();
				let Some(request) = request else {
					let profile = profile.clone();
					let session = session.clone();

					cx.background_executor()
						.spawn(async move {
							if let Ok(runtime) = Builder::new_current_thread().enable_all().build()
							{
								let _ =
									runtime
										.block_on(AgentClient::new(profile).voice(
											AgentVoiceRequest::Stop { session_id: session },
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
							let runtime =
								Builder::new_current_thread().enable_all().build().ok()?;

							runtime.block_on(AgentClient::new(profile).voice(request)).ok()
						})
						.await;
					let _ = surface.update(cx, |s, cx| {
						s.apply_voice_response(&session, result, cx);
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

	fn poll_voice_media(&mut self, cx: &mut Context<Self>) -> Option<Option<AgentVoiceRequest>> {
		let voice = self.voice.as_mut()?;

		for _ in 0..128 {
			let Some(event) = voice.media.poll() else { break };

			match event["type"].as_str() {
				Some("offer") => {
					let offer = event["sdp"].as_str().and_then(|s| VoiceSdp::new(s.into()).ok())?;

					voice.signaling = true;
					voice.connection_status = "Connecting…".into();
					voice.request = Some(AgentVoiceRequest::Start {
						session_id: voice.session.clone(),
						work_id: voice.work.clone(),
						offer,
						options: voice.options.clone(),
					});
				},
				Some("connected") => voice.connected = true,
				Some("status") =>
					voice.connection_status =
						event["message"].as_str().unwrap_or("Connecting…").into(),
				Some("level") => {
					voice.levels.pop_front();
					voice
						.levels
						.push_back(event["level"].as_f64().unwrap_or_default().clamp(0., 1.) as f32);
				},
				Some("caption") => {
					update_caption(&event["event"], &mut voice.captions);
				},
				Some("error" | "ended") => {
					if event["type"] == "error" {
						self.feedback =
							event["message"].as_str().unwrap_or("Voice disconnected.").into();
					}

					self.retire_voice_media();
					cx.notify();

					return None;
				},
				_ => {},
			}
		}

		let request = voice.request.take().or_else(|| {
			voice.signaling.then(|| AgentVoiceRequest::Poll { session_id: voice.session.clone() })
		});

		if let Some((work, history)) = self.history.take() {
			self.reconcile_voice_captions(&work, &history);

			self.history = Some((work, history));
		}

		cx.notify();

		Some(request)
	}

	fn apply_voice_response(
		&mut self,
		session: &EntityId,
		status: Option<AgentVoiceStatus>,
		cx: &mut Context<Self>,
	) {
		if !self.voice.as_ref().is_some_and(|voice| &voice.session == session) {
			return;
		}

		if let Some(status) = status {
			self.apply_voice_status(status, cx);
		} else {
			// Signaling can disconnect while WebRTC still sends microphone audio.
			// Dropping media stops local capture; the loop then stops this exact session.
			self.retire_voice_media();

			self.feedback = "Voice stopped because the service connection was lost.".into();

			cx.notify();
		}
	}

	fn apply_voice_status(&mut self, status: AgentVoiceStatus, cx: &mut Context<Self>) {
		let Some(voice) = self.voice.as_mut().filter(|v| v.session == status.session_id) else {
			return;
		};

		match status.phase {
			AgentVoicePhase::Connecting => {
				voice.request = Some(AgentVoiceRequest::Poll { session_id: voice.session.clone() });
			},
			AgentVoicePhase::Ready => {
				if let Some(message) = status.message {
					self.feedback = message.as_str().into();
				}

				if !voice.answered
					&& let Some(answer) = status.answer
				{
					voice.answered =
						voice.media.command(json!({"operation":"answer","sdp":answer.as_str()}));
				}
			},
			AgentVoicePhase::Ended | AgentVoicePhase::Failed => {
				if let Some(message) = status.message {
					self.feedback = message.as_str().into();
				}

				self.retire_voice_media();
			},
		}

		cx.notify();
	}

	pub(super) fn open_audio_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.composer_menu == Some("microphone") {
			self.composer_menu = Some("attachments");
			self.composer_menu_content = self.composer_menu;

			cx.notify();

			return;
		}

		if let Ok(mut media) = Media::new(window) {
			media.command(json!({"operation":"devices"}));

			while let Some(event) = media.poll() {
				if event["type"] == "devices" {
					self.audio_inputs = event["inputs"]
						.as_array()
						.map(|values| {
							values.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()
						})
						.unwrap_or_default();
				}
			}
		}

		self.composer_menu = Some("microphone");
		self.composer_menu_content = self.composer_menu;

		cx.notify();
	}

	pub(super) fn audio_palette(&self, cx: &mut Context<Self>) -> AnyElement {
		let mut inputs = vec![String::new()];

		inputs.extend(self.audio_inputs.clone());

		div()
			.id("microphone-device-list")
			.max_h(px(168.))
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap(px(2.))
			.children(inputs.into_iter().enumerate().map(|(i, input)| {
				let selected = input == self.audio_input;
				let keyboard_input = input.clone();
				let label =
					if input.is_empty() { "System default".to_owned() } else { input.clone() };

				div()
					.id(("microphone-input", i))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Use {label}"))
					.h(px(28.))
					.px(px(6.))
					.rounded(px(6.))
					.flex()
					.items_center()
					.gap(px(8.))
					.cursor_pointer()
					.text_size(px(12.))
					.child(div().flex_1().min_w_0().text_ellipsis().child(label))
					.child(div().w(px(12.)).child(if selected { "✓" } else { "" }))
					.hover(|d| d.bg(agent_surface::rgba(HOVER_FILL)))
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.audio_input = keyboard_input.clone();

							cx.stop_propagation();
							cx.notify();
						}
					}))
					.on_click(cx.listener(move |s, _, _, cx| {
						s.audio_input = input.clone();

						cx.notify();
					}))
					.smooth()
			}))
			.into_any_element()
	}

	pub(super) fn voice_controls(
		&self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let voice = self.voice.as_ref()?;

		Some(
			div()
				.w_full()
				.h(px(30.))
				.flex()
				.items_center()
				.justify_center()
				.gap(px(3.))
				.children(voice.levels.iter().enumerate().map(|(i, level)| {
					let height = ui_motion::value(
						("live-wave-height", i),
						if voice.muted { 2. } else { 2. + level * 40. },
						window,
						cx,
					);

					div()
						.id(("live-wave", i))
						.w(px(3.))
						.h(px(height))
						.rounded_full()
						.bg(agent_surface::rgb(if voice.muted { TEXT_MUTED } else { BLUE }))
				}))
				.into_any_element(),
		)
	}

	pub(super) fn voice_toolbar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
		let voice = self.voice.as_ref()?;
		let label = if !voice.connected {
			voice.connection_status.as_str()
		} else if voice.muted {
			"Microphone muted"
		} else {
			"Live"
		};

		Some(
			div()
				.flex_none()
				.flex()
				.items_center()
				.gap(px(8.))
				.text_size(px(11.))
				.text_color(agent_surface::rgb(TEXT_MUTED))
				.child(
					div()
						.id("voice-status")
						.role(Role::Status)
						.aria_label(label.to_owned())
						.child(label.to_owned()),
				)
				.child(self.composer_control(
					"voice-mute",
					if voice.muted { "Unmute" } else { "Mute" }.into(),
					"Toggle microphone",
					|s, cx| {
						if let Some(voice) = &mut s.voice {
							let muted = !voice.muted;

							if voice.media.command(json!({"operation":"mute","muted":muted})) {
								voice.muted = muted;
							} else {
								s.retire_voice_media();

								s.feedback = "Voice stopped because the microphone control could not be updated.".into();
							}
						}

						cx.notify();
					},
					cx,
				))
				.child(self.composer_control(
					"voice-end",
					"End".into(),
					"End voice call · Existing work continues",
					|s, cx| {
						s.retire_voice_media();
						s.load_history(cx);
						cx.notify();
					},
					cx,
				))
				.into_any_element(),
		)
	}

	fn retire_voice_media(&mut self) {
		if let Some(mut voice) = self.voice.take() {
			drain_caption_events(&mut voice.captions, || voice.media.poll());

			for caption in &mut voice.captions {
				caption.complete = true;
			}

			self.retired_voice_captions.push(CaptionHistory {
				session: voice.session,
				matched_receipts: voice.matched_receipts,
				work: voice.work,
				captions: voice.captions,
			});
		}
		if let Some((work, history)) = self.history.take() {
			self.reconcile_voice_captions(&work, &history);

			self.history = Some((work, history));
		}
	}

	pub(super) fn reconcile_voice_captions(&mut self, work: &str, history: &AgentHistoryResult) {
		let AgentHistoryResult::Available { entries, .. } = history else { return };

		for call in self.retired_voice_captions.iter_mut().filter(|v| v.work.as_str() == work) {
			reconcile_captions(
				&call.session,
				&mut call.captions,
				&mut call.matched_receipts,
				entries,
			);
		}

		self.retired_voice_captions.retain(|v| !v.captions.is_empty());

		if let Some(call) = self.voice.as_mut().filter(|v| v.work.as_str() == work) {
			reconcile_captions(
				&call.session,
				&mut call.captions,
				&mut call.matched_receipts,
				entries,
			);
		}
	}

	pub(super) fn live_chat_caption(&self, work: &str) -> Option<AnyElement> {
		let captions = self
			.retired_voice_captions
			.iter()
			.filter(|v| v.work.as_str() == work)
			.flat_map(|v| &v.captions)
			.chain(
				self.voice
					.as_ref()
					.filter(|v| v.work.as_str() == work)
					.into_iter()
					.flat_map(|v| &v.captions),
			);
		let mut captions: Vec<_> = captions.filter(|c| !c.text.is_empty()).collect();

		// Keep completed order; duplex live user text stays above the live reply.
		captions.sort_by_key(|c| {
			if c.complete {
				0
			} else if c.role == "user" {
				1
			} else {
				2
			}
		});

		if captions.is_empty() {
			return None;
		}

		Some(
			div()
				.flex()
				.flex_col()
				.children(captions.into_iter().enumerate().map(|(i, caption)| {
					agent_surface::history_entry(&decodex_protocol::AgentHistoryEntryDto {
						native_source: None,
						turn_id: None,
						weather: Vec::new(),
						receipt: None,
						activity: None,
						usage: None,
						duration_ms: None,
						id: -(i as i64) - 1,
						kind: caption.role.into(),
						text: caption.text.clone(),
						created_at_micros: 0,
					})
					.into_any_element()
				}))
				.into_any_element(),
		)
	}

	pub(super) fn follow_voice_scroll(&self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(v) = self.voice.as_ref().filter(|v| v.follow) else { return };
		let Some(scroll) = self.transcript_scroll.get(v.work.as_str()) else { return };
		let current = f32::from(scroll.offset().y);
		let target = -f32::from(scroll.max_offset().y);

		if (target - current).abs() > 0.5 {
			let reduced = ui_motion::reduced();
			let next = if reduced { target } else { current + (target - current) * 0.24 };

			scroll.set_offset(gpui::point(px(0.), px(next)));

			if !reduced {
				ui_motion::request_frame(window, cx);
			}

			cx.notify();
		}
	}

	pub(super) fn set_voice_follow(&mut self, following: bool) {
		if let Some(voice) = &mut self.voice {
			voice.follow = following;
		}
	}
}

fn reconcile_captions(
	session: &EntityId,
	captions: &mut Vec<Caption>,
	matched: &mut BTreeSet<i64>,
	entries: &[decodex_protocol::AgentHistoryEntryDto],
) {
	captions.retain(|caption| {
		if !caption.complete {
			return true;
		}
		if caption.text.is_empty() {
			return false;
		}

		let found = entries.iter().find(|entry| {
			!matched.contains(&entry.id)
				&& entry.kind == caption.role
				&& entry.text.trim() == caption.text.trim()
				&& entry.receipt.as_ref().is_some_and(|receipt| {
					receipt.voice_session_id.as_deref() == Some(session.as_str())
						&& receipt.event_kind == format!("voice_{}", caption.role)
						&& receipt.disposed
				})
		});

		if let Some(entry) = found {
			matched.insert(entry.id);

			false
		} else {
			true
		}
	});
}

/// Consume the host's already queued text before releasing the media object.
/// The native mailbox has a 128-event bound; retirement never waits for more input.
fn drain_caption_events(captions: &mut Vec<Caption>, mut poll: impl FnMut() -> Option<Value>) {
	for _ in 0..128 {
		let Some(event) = poll() else { break };

		if event["type"] == "caption" {
			update_caption(&event["event"], captions);
		}
	}
}

/// Keep each turn until history can replace it, including interleaved final updates.
fn update_caption(event: &Value, captions: &mut Vec<Caption>) {
	let kind = event["type"].as_str().unwrap_or_default();
	let added_role = match kind {
		"input_transcript.added" => Some("user"),
		"output_transcript.added" => Some("assistant"),
		_ => None,
	};

	if let Some(role) = added_role {
		let Some(delta) = event.pointer("/item/text").and_then(Value::as_str) else { return };

		if delta.is_empty() {
			return;
		}
		if !captions.iter().any(|c| c.role == role && !c.complete) {
			captions.push(Caption {
				complete: false,
				turn: String::new(),
				role,
				text: String::new(),
			});
		}

		let caption = captions
			.iter_mut()
			.rev()
			.find(|c| c.role == role && !c.complete)
			.expect("unfinished caption exists after insertion");

		caption.text.push_str(delta);

		bound_caption(caption);

		return;
	}

	let id = if kind == "turn.delta" {
		event["turn_id"].as_str()
	} else {
		event.pointer("/turn/id").and_then(Value::as_str)
	}
	.filter(|id| !id.is_empty());
	let role = match event.pointer("/turn/role").and_then(Value::as_str) {
		Some("user") => Some("user"),
		Some("assistant") => Some("assistant"),
		_ => None,
	};

	if kind == "turn.created" {
		let (Some(id), Some(role)) = (id, role) else { return };

		if !captions.iter().any(|c| c.turn == id) {
			captions.push(Caption { complete: false, turn: id.into(), role, text: String::new() });
		}
	}
	// Frameless v3 finals can omit the turn ID, including a final with no deltas.
	if kind == "turn.done" && id.is_none() {
		let Some(role) = role else { return };
		let Some(text) = event.pointer("/turn/transcript").and_then(Value::as_str) else { return };

		if !captions.iter().any(|c| c.role == role && !c.complete) {
			if text.is_empty() {
				return;
			}

			captions.push(Caption {
				complete: false,
				turn: String::new(),
				role,
				text: String::new(),
			});
		}

		let caption = captions
			.iter_mut()
			.rev()
			.find(|c| c.role == role && !c.complete)
			.expect("unfinished caption exists after insertion");

		caption.text = text.into();
		caption.complete = true;

		bound_caption(caption);

		return;
	}

	let Some(id) = id else { return };

	if kind == "turn.done"
		&& !captions.iter().any(|c| c.turn == id)
		&& let Some(role) = role
	{
		if let Some(caption) =
			captions.iter_mut().rev().find(|c| c.turn.is_empty() && c.role == role && !c.complete)
		{
			// Some peers add identity only to the final.
			caption.turn = id.into();
		} else if event
			.pointer("/turn/transcript")
			.and_then(Value::as_str)
			.is_some_and(|text| !text.is_empty())
		{
			// The native parser also accepts a final when no delta was received.
			captions.push(Caption { complete: false, turn: id.into(), role, text: String::new() });
		}
	}

	let Some(caption) = captions.iter_mut().find(|c| c.turn == id) else { return };

	if kind == "turn.done" {
		caption.complete = true;
	}

	match kind {
		"turn.created" | "turn.done" => {
			if let Some(text) = event.pointer("/turn/transcript").and_then(Value::as_str) {
				caption.text = text.into();
			}
		},
		"turn.delta" => caption.text.push_str(event["delta"].as_str().unwrap_or_default()),
		_ => return,
	}

	bound_caption(caption);
}

fn bound_caption(caption: &mut Caption) {
	// Match persisted voice tails so history can replace the live caption.
	let mut start = caption.text.len().saturating_sub(32_768);

	while !caption.text.is_char_boundary(start) {
		start += 1;
	}

	caption.text.drain(..start);
}

#[cfg(test)]
mod tests {
	#[cfg(test)] use gpui::AppContext as _;
	use gpui::{EntityInputHandler as _, Focusable as _, ParentElement};
	use raw_window_handle as _;

	#[cfg(any(all(target_os = "macos", not(test)), any(not(target_os = "macos"), test)))]
	use crate::shell::agent_surface::voice::Media;
	use crate::shell::agent_surface::voice::{
		self, AgentHistoryResult, AgentSurface, AgentVoicePhase, AgentVoiceRequest, Caption,
		Context, EntityId, IntoElement, VoiceUi, Window,
	};
	#[cfg(test)] use crate::shell::agent_surface::voice::{Entity, Render};

	struct VoiceComposerView(Entity<AgentSurface>);

	struct VoiceToolbarView(Entity<AgentSurface>);

	impl Render for VoiceComposerView {
		fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			self.0.update(cx, |s, cx| s.render_composer_capsule(false, window, cx))
		}
	}

	impl Render for VoiceToolbarView {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			self.0.update(cx, |s, cx| voice::div().children(s.voice_toolbar(cx)))
		}
	}

	#[test]
	fn captions_preserve_both_speakers_and_late_finals() {
		let mut captions = Vec::new();

		for event in [
			voice::json!({"type":"turn.created","turn":{"id":"u","role":"user","transcript":"Hello"}}),
			voice::json!({"type":"turn.created","turn":{"id":"a","role":"assistant","transcript":"Hi"}}),
			voice::json!({"type":"turn.delta","turn_id":"u","delta":" world"}),
			voice::json!({"type":"turn.done","turn":{"id":"u","transcript":"Hello, world!"}}),
		] {
			voice::update_caption(&event, &mut captions);
		}

		assert_eq!(
			captions.iter().map(|c| (c.role, c.text.as_str())).collect::<Vec<_>>(),
			vec![("user", "Hello, world!"), ("assistant", "Hi")]
		);

		voice::update_caption(
			&voice::json!({"type":"turn.done","turn":{"id":"a","transcript":""}}),
			&mut captions,
		);

		assert_eq!(captions[0].text, "Hello, world!");
		assert!(captions[1].text.is_empty());
	}

	#[test]
	fn frameless_captions_accept_interleaved_deltas_and_idless_finals() {
		let mut captions = Vec::new();

		for event in [
			voice::json!({"type":"input_transcript.added","item":{"text":"hello"}}),
			voice::json!({"type":"output_transcript.added","item":{"text":"reply"}}),
			voice::json!({"type":"input_transcript.added","item":{"text":" world"}}),
			voice::json!({"type":"turn.done","turn":{"role":"user","transcript":"Hello, world!"}}),
			voice::json!({"type":"turn.done","turn":{"id":"late-id","role":"assistant","transcript":"Reply."}}),
			voice::json!({"type":"turn.done","turn":{"id":"final-only","role":"user","transcript":"Another sentence."}}),
		] {
			voice::update_caption(&event, &mut captions);
		}

		assert_eq!(
			captions.iter().map(|c| (c.role, c.text.as_str(), c.complete)).collect::<Vec<_>>(),
			vec![
				("user", "Hello, world!", true),
				("assistant", "Reply.", true),
				("user", "Another sentence.", true)
			]
		);

		voice::update_caption(
			&voice::json!({"type":"input_transcript.added","item":{"text":"discard"}}),
			&mut captions,
		);
		voice::update_caption(
			&voice::json!({"type":"turn.done","turn":{"role":"user","transcript":""}}),
			&mut captions,
		);

		assert_eq!(captions.len(), 4);
		assert!(captions[3].text.is_empty() && captions[3].complete);

		voice::update_caption(
			&voice::json!({"type":"input_transcript.added","item":{"text":"界".repeat(12_000)}}),
			&mut captions,
		);

		assert_eq!(captions[4].text.len(), 32_766);
	}

	#[test]
	fn long_live_captions_keep_the_latest_utf8_correction() {
		let prefix = "Old opening ".to_owned() + &"界".repeat(11_000);
		let suffix = " The latest correction must remain.";

		for finalized in [false, true] {
			let mut captions = Vec::new();

			if finalized {
				voice::update_caption(
					&voice::json!({"type":"turn.done","turn":{"role":"user","transcript":prefix.clone()+suffix}}),
					&mut captions,
				);
			} else {
				for text in [&*prefix, suffix] {
					voice::update_caption(
						&voice::json!({"type":"input_transcript.added","item":{"text":text}}),
						&mut captions,
					);
				}
			}

			assert_eq!(captions.len(), 1);
			assert!(captions[0].text.len() <= 32_768);
			assert!(captions[0].text.ends_with(suffix));
			assert!(!captions[0].text.contains("Old opening"));
			assert_eq!(captions[0].complete, finalized);
		}
	}

	#[test]
	fn retiring_media_reads_queued_corrections_before_finalizing_captions() {
		let mut captions = Vec::new();

		voice::update_caption(
			&voice::json!({"type":"input_transcript.added","item":{"text":"uncorrected"}}),
			&mut captions,
		);

		let mut pending = std::collections::VecDeque::from([
			voice::json!({"type":"level","level":0.2}),
			voice::json!({"type":"caption","event":{"type":"turn.done","turn":{"role":"user","transcript":"Corrected final."}}}),
			voice::json!({"type":"caption","event":{"type":"output_transcript.added","item":{"text":"Reply"}}}),
			voice::json!({"type":"ended"}),
		]);

		voice::drain_caption_events(&mut captions, || pending.pop_front());

		assert!(pending.is_empty());
		assert_eq!(
			captions.iter().map(|c| (c.role, c.text.as_str(), c.complete)).collect::<Vec<_>>(),
			vec![("user", "Corrected final.", true), ("assistant", "Reply", false)]
		);

		let mut polled = 0;

		voice::drain_caption_events(&mut captions, || {
			polled += 1;

			Some(voice::json!({"type":"level"}))
		});

		assert_eq!(polled, 128, "Retirement must not wait for an ongoing producer");
	}

	#[gpui::test]
	fn rejected_microphone_control_stops_voice_and_retains_text(cx: &mut gpui::TestAppContext) {
		for muted in [false, true] {
			let (view, visual) = cx.add_window_view(|_, cx| {
				let surface = cx.new(AgentSurface::new);

				surface.update(cx, |s, cx| {
					s.composer.update(cx, |input, cx| input.set_content("Draft remains", cx));

					s.voice = Some(VoiceUi {
						options: Default::default(),
						media: Media,
						session: EntityId::new("call").unwrap(),
						work: EntityId::new("agent").unwrap(),
						request: None,
						answered: true,
						signaling: true,
						connected: true,
						connection_status: "Live".into(),
						muted,
						captions: vec![Caption {
							complete: false,
							turn: "turn".into(),
							role: "user",
							text: "Keep this caption".into(),
						}],
						matched_receipts: Default::default(),
						levels: Default::default(),
						follow: true,
					});
				});

				VoiceToolbarView(surface)
			});

			visual.update(|window, cx| {
				window.draw(cx).clear();
			});

			let button = visual.debug_bounds("composer-voice-mute").expect("microphone control");

			visual.simulate_click(button.center(), Default::default());

			view.read_with(visual, |view, cx| {
				let s = view.0.read(cx);

				assert!(s.voice.is_none(), "A rejected microphone command must stop local media");
				assert!(s.feedback.contains("microphone"));
				assert_eq!(s.composer.read(cx).content(), "Draft remains");
				assert_eq!(s.retired_voice_captions.len(), 1);

				let caption = &s.retired_voice_captions[0].captions[0];

				assert_eq!(caption.text, "Keep this caption");
				assert!(caption.complete);
			});
		}
	}

	#[gpui::test]
	fn selected_speech_stays_bound_to_the_call_and_preserves_the_draft(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.composer.update(cx, |input, cx| input.set_content("Draft", cx));

			let id = EntityId::new("call").unwrap();

			s.voice = Some(VoiceUi {
				options: Default::default(),
				media: Media,
				session: id.clone(),
				work: EntityId::new("agent").unwrap(),
				request: None,
				answered: true,
				signaling: true,
				connected: true,
				connection_status: "Live".into(),
				muted: false,
				captions: Vec::new(),
				matched_receipts: Default::default(),
				levels: Default::default(),
				follow: true,
			});

			s.queue_voice_speech("other", &id, "Wrong work", cx);
			s.queue_voice_speech(
				"agent",
				&EntityId::new("previous-call").unwrap(),
				"Stale button",
				cx,
			);

			assert!(s.voice.as_ref().unwrap().request.is_none());

			s.queue_voice_speech("agent", &id, &"x".repeat(65_537), cx);

			assert!(s.voice.as_ref().unwrap().request.is_none(), "no silent truncation");

			s.queue_voice_speech("agent", &id, "Selected reply", cx);
			s.queue_voice_speech("agent", &id, "Do not replace pending output", cx);

			assert_eq!(
				s.poll_voice_media(cx).unwrap().unwrap(),
				AgentVoiceRequest::Speak {
					session_id: id.clone(),
					text: decodex_protocol::HistoryText::new("Selected reply").unwrap(),
				}
			);
			assert!(matches!(
				s.poll_voice_media(cx).unwrap().unwrap(),
				AgentVoiceRequest::Poll { .. }
			));

			s.apply_voice_status(
				decodex_protocol::AgentVoiceStatus {
					session_id: id,
					phase: AgentVoicePhase::Ready,
					answer: None,
					message: decodex_protocol::WireText::new("Read-aloud could not be confirmed.")
						.ok(),
				},
				cx,
			);

			assert!(s.voice.as_ref().unwrap().connected);
			assert!(s.feedback.contains("not be confirmed"));
			assert_eq!(s.composer.read(cx).content(), "Draft");
		});
	}

	#[gpui::test]
	fn active_voice_keeps_the_draft_visible_and_editable(cx: &mut gpui::TestAppContext) {
		cx.update(crate::composer_input::bind_keys);

		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);

			surface.update(cx, |s, cx| {
				s.composer.update(cx, |input, cx| input.set_content("Draft", cx));

				s.voice = Some(VoiceUi {
					options: Default::default(),
					media: Media,
					session: EntityId::new("call").unwrap(),
					work: EntityId::new("agent").unwrap(),
					request: None,
					answered: true,
					signaling: true,
					connected: true,
					connection_status: "Live".into(),
					muted: false,
					captions: Vec::new(),
					matched_receipts: Default::default(),
					levels: Default::default(),
					follow: true,
				});
			});

			VoiceComposerView(surface)
		});
		let surface = view.read_with(visual, |v, _| v.0.clone());
		let input = surface.read_with(visual, |s, _| s.composer.clone());

		for width in [320., 800.] {
			visual.update(|window, cx| {
				window.resize(gpui::size(voice::px(width), voice::px(400.)));
				window.focus(&input.focus_handle(cx), cx);
				window.draw(cx).clear();
			});

			visual.update(|window, cx| {
				input.update(cx, |input, cx| {
					let text = input
						.bounds_for_range(0..5, Default::default(), window, cx)
						.expect("the visible draft must have text layout");

					assert!(text.size.width > voice::px(0.) && text.size.height > voice::px(0.));
					assert!(text.origin.x >= voice::px(0.) && text.origin.y >= voice::px(0.));
					assert!(text.origin.x + text.size.width <= voice::px(width));
				});
			});
		}

		visual.simulate_keystrokes("shift-enter");
		input.read_with(visual, |input, _| assert_eq!(input.content(), "Draft\n"));
		surface.read_with(visual, |s, _| assert!(s.voice.is_some()));
	}

	#[gpui::test]
	fn saved_voice_caption_and_disconnect_remain_bound_to_the_current_call(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(AgentSurface::new);

		surface.update(cx, |s, cx| {
			s.voice = Some(VoiceUi {
				options: Default::default(),
				media: Media,
				session: EntityId::new("call").expect("id"),
				work: EntityId::new("agent").expect("id"),
				request: None,
				answered: true,
				signaling: true,
				connected: true,
				connection_status: "Live".into(),
				muted: false,
				matched_receipts: Default::default(),
				captions: vec![Caption {
					complete: true,
					turn: "turn".into(),
					role: "user",
					text: "Hello".into(),
				}],
				levels: Default::default(),
				follow: true,
			});

			let history = |session: Option<&str>| AgentHistoryResult::Available {
				questions: vec![],
				questions_truncated: false,
				questions_recovering: false,
				misalignment: None,
				usage: None,
				has_more: false,
				next_before: None,
				live: vec![],
				entries: vec![decodex_protocol::AgentHistoryEntryDto {
					native_source: None,
					turn_id: None,
					weather: Vec::new(),
					receipt: session.map(|session| decodex_protocol::AgentHistoryReceiptDto {
						voice_session_id: Some(session.into()),
						event_kind: "voice_user".into(),
						delivered_turn_id: None,
						disposed: true,
					}),
					activity: None,
					usage: None,
					duration_ms: None,
					id: 1,
					kind: "user".into(),
					text: "Hello".into(),
					created_at_micros: 100,
				}],
			};

			for sample in [history(None), history(Some("other-call"))] {
				s.reconcile_voice_captions("agent", &sample);

				assert!(s.live_chat_caption("agent").is_some());
			}

			s.reconcile_voice_captions("other-work", &history(Some("call")));

			assert!(s.live_chat_caption("agent").is_some());

			s.reconcile_voice_captions("agent", &history(Some("call")));

			assert!(s.live_chat_caption("agent").is_none());

			s.voice.as_mut().unwrap().captions.push(Caption {
				complete: true,
				turn: "repeat".into(),
				role: "user",
				text: "Hello".into(),
			});
			s.reconcile_voice_captions("agent", &history(Some("call")));

			assert!(
				s.live_chat_caption("agent").is_some(),
				"One receipt must not hide two captions"
			);
			assert!(s.live_chat_caption("other-work").is_none());

			s.voice.as_mut().expect("voice").captions[0].role = "assistant";

			assert!(s.live_chat_caption("agent").is_some());

			s.apply_voice_response(&EntityId::new("old-call").expect("id"), None, cx);

			assert!(s.voice.is_some(), "An old request must not stop the current call");

			s.apply_voice_response(&EntityId::new("call").expect("id"), None, cx);

			assert!(s.voice.is_none(), "A failed control connection must retire local media");
			assert!(
				s.live_chat_caption("agent").is_some(),
				"Retired media must retain text until history arrives"
			);
			assert!(s.live_chat_caption("other-work").is_none());

			let mut saved = history(Some("call"));

			if let AgentHistoryResult::Available { entries, .. } = &mut saved {
				entries[0].id = 2;
				entries[0].kind = "assistant".into();
				entries[0].receipt.as_mut().unwrap().event_kind = "voice_assistant".into();
			}

			s.reconcile_voice_captions("agent", &saved);

			assert!(s.retired_voice_captions.is_empty());

			s.history = None;

			assert!(
				s.live_chat_caption("agent").is_none(),
				"Pagination cannot resurrect accepted captions"
			);
			assert!(s.feedback.contains("service connection was lost"));
			assert!(s.poll_voice_media(cx).is_none(), "The loop must enter exact-session cleanup");
		});
	}
}
