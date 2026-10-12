//! Live media controls; subscription signaling and task execution stay in the service.
#[cfg(target_os = "macos")]
#[path = "native_voice_audio.rs"]
mod audio;
#[cfg(target_os = "macos")]
#[path = "native_dictation_capture.rs"]
mod dictation_capture;
#[path = "voice_history_block.rs"] pub(crate) mod history;
#[cfg(target_os = "macos")]
#[path = "native_voice_transport.rs"]
mod transport;

use gpui::prelude::FluentBuilder as _;
#[cfg(all(target_os = "macos", not(test)))] use std::os::unix::ffi::OsStrExt as _;
use std::{collections::BTreeSet, time::Duration};

use gpui::{AnyElement, KeyDownEvent};
use raw_window_handle as _;
use serde_json::Value;
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
			Window, ui_theme,
		},
	},
	ui_motion,
	ui_theme::HOVER_FILL,
};
use decodex_protocol::{
	AgentVoiceOptions, AgentVoicePhase, AgentVoiceRequest, AgentVoiceSettingsResult,
	AgentVoiceStatus, HistoryText, VoiceSdp,
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
	transport: Option<transport::Transport>,
	capture: Option<dictation_capture::Capture>,
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

			if version() != 4 {
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
				transport: None,
				capture: None,
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
			Some("finish") if self.capture.is_some() => {
				self.capture.as_ref().expect("finish matched an active capture").finish();
				return true;
			},
			Some("start" | "dictate" | "stop") => {
				self.transport = None;
				self.capture = None;
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
			if event["type"] == "dictation_prepare_authorized" {
				if let Some(device) = event["device"].as_u64().and_then(|v| u32::try_from(v).ok()) {
					dictation_capture::Capture::prepare(device);
				}
				return None;
			}
			if event["type"] == "dictation_authorized" {
				let started = event["device"]
					.as_u64()
					.and_then(|id| u32::try_from(id).ok())
					.ok_or(audio::owner::AudioError::Prepare)
					.and_then(dictation_capture::Capture::start);
				match started {
					Ok(capture) => self.capture = Some(capture),
					Err(error) =>
						return Some(serde_json::json!({"type":"error","message":error.message()})),
				}
				return None;
			}
			if event["type"] != "voice_authorized" {
				return Some(event);
			}

			let started = event["device"]
				.as_u64()
				.and_then(|id| u32::try_from(id).ok())
				.ok_or(audio::owner::AudioError::Prepare)
				.and_then(transport::Transport::start_device);

			match started {
				Ok(transport) => {
					if self.muted {
						transport.command(transport::Command::Mute(true));
					}

					self.transport = Some(transport);
				},
				Err(error) =>
					return Some(serde_json::json!({"type":"error","message":error.message()})),
			}
		}
		if let Some(capture) = &self.capture
			&& let Some(event) = capture.poll()
		{
			return Some(event);
		}
		if let Some(transport) = &self.transport {
			if let Some(event) = transport.poll() {
				return Some(event);
			}
			if self.level_at.elapsed() >= Duration::from_millis(50) {
				self.level_at = std::time::Instant::now();
				return Some(
					serde_json::json!({"type":"level","level":if self.muted {0.0} else {(transport.level() * 5.0).min(1.0)}}),
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
		self.command(serde_json::json!({"operation":"stop"}));

		// SAFETY: unique host, destroyed exactly once on the GPUI main thread.
		unsafe { (self.destroy)(self.host) };
	}
}

impl Media {
	pub(super) fn prepare_input(window: &Window, input: &str) {
		if let Ok(mut media) = Self::new(window) {
			media.command(serde_json::json!({"operation":"prepare","input":input}));
			let _ = media.poll();
		}
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
	key: String,
	complete: bool,
	turn: String,
	role: &'static str,
	text: String,
}

impl AgentSurface {
	pub(crate) fn stop_voice(&mut self, cx: &mut Context<Self>) {
		self.cancel_dictation(cx);
		let _ = self.retire_voice_media();
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

	fn prepare_voice_ui(
		&mut self,
		options: AgentVoiceOptions,
		media: Media,
		session: &EntityId,
		work: EntityId,
	) {
		if let Some(height) = self.timeline.voice_min_height.get(work.as_str()) {
			height.set(0.);
		}
		self.feedback.clear();
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
			self.feedback = "Send a message before starting voice chat.".into();

			cx.notify();

			return;
		};
		let Some(target) = self.voice_option_target(work.as_str()) else {
			return;
		};
		let generation = self.generation;
		let query_work = work.clone();
		let future = cx.background_executor().spawn(async move {
			let runtime = Builder::new_current_thread().enable_all().build().ok()?;
			runtime.block_on(AgentClient::new(profile).voice_settings(query_work)).ok()
		});
		self.feedback = "Checking voice availability…".into();
		self.voice_task = Some(cx.spawn_in(window, async move |surface, cx| {
            let result = future.await;
            let _ = cx.update(|window, cx| {
                let _ = surface.update(cx, |s, cx| {
                    s.voice_task = None;
                    if s.generation != generation || s.voice_option_target(work.as_str()).as_ref() != Some(&target) {
                        s.feedback = "Voice start canceled because the conversation changed.".into();
                        cx.notify();
                        return;
                    }
                    if matches!(result, Some(AgentVoiceSettingsResult::Available { ref work_id, .. }) if work_id == &work) {
                        s.start_allowed_voice(work, window, cx);
                    } else {
                        s.feedback = "Voice is unavailable or disabled by managed policy.".into();
                        cx.notify();
                    }
                });
            });
        }));
		cx.notify();
	}

	fn start_allowed_voice(&mut self, work: EntityId, window: &mut Window, cx: &mut Context<Self>) {
		if self.selected_is_archived()
			|| self.composer_unavailable_reason().is_some()
			|| self.voice_task.is_some()
			|| self.dictation_task.is_some()
		{
			return;
		}
		let Some(profile) = self.profile.clone() else {
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
				self.feedback = "Voice chat requires the latest signed Decodex app.".into();

				cx.notify();

				return;
			},
		};

		if !media.command(serde_json::json!({"operation":"start","input":self.audio_input})) {
			self.feedback = "The audio host could not start.".into();

			cx.notify();

			return;
		}

		let session =
			EntityId::new(agent_surface::unique_command()).expect("bounded voice identity");

		self.prepare_voice_ui(options, media, &session, work);

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

					let _ = self.retire_voice_media();
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
			self.feedback = self
				.retire_voice_media()
				.unwrap_or_else(|| "Voice stopped because the service connection was lost.".into());

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
					voice.answered = voice
						.media
						.command(serde_json::json!({"operation":"answer","sdp":answer.as_str()}));
				}
			},
			AgentVoicePhase::Ended | AgentVoicePhase::Failed => {
				if let Some(message) = status.message {
					self.feedback = message.as_str().into();
				}

				let _ = self.retire_voice_media();
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
			media.command(serde_json::json!({"operation":"devices"}));

			while let Some(event) = media.poll() {
				if event["type"] == "devices" {
					if let Some(message) = event["message"].as_str() {
						self.feedback = message.to_owned();
					}
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

		gpui::div()
			.id("microphone-device-list")
			.max_h(gpui::px(168.))
			.overflow_y_scroll()
			.flex()
			.flex_col()
			.gap(gpui::px(2.))
			.children(inputs.into_iter().enumerate().map(|(i, input)| {
				let selected = input == self.audio_input;
				let keyboard_input = input.clone();
				let label =
					if input.is_empty() { "System default".to_owned() } else { input.clone() };

				gpui::div()
					.id(("microphone-input", i))
					.role(Role::Button)
					.tab_index(0)
					.aria_label(format!("Use {label}"))
					.h(gpui::px(28.))
					.px(gpui::px(6.))
					.rounded(gpui::px(6.))
					.flex()
					.items_center()
					.gap(gpui::px(8.))
					.cursor_pointer()
					.text_size(gpui::px(12.))
					.child(gpui::div().flex_1().min_w_0().text_ellipsis().child(label))
					.child(gpui::div().w(gpui::px(12.)).child(if selected { "✓" } else { "" }))
					.hover(|d| d.bg(gpui::rgba(HOVER_FILL)))
					.on_key_down(cx.listener(move |s, e: &KeyDownEvent, window, cx| {
						if ["enter", "space"].contains(&e.keystroke.key.as_str()) {
							s.audio_input = keyboard_input.clone();
							if s.voice.is_none() && s.dictation.is_none() {
								Media::prepare_input(window, &s.audio_input);
							}

							cx.stop_propagation();
							cx.notify();
						}
					}))
					.on_click(cx.listener(move |s, _, window, cx| {
						s.audio_input = input.clone();
						if s.voice.is_none() && s.dictation.is_none() {
							Media::prepare_input(window, &s.audio_input);
						}

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
		if !voice.connected {
			return Some(
				gpui::div()
					.id("voice-waiting")
					.debug_selector(|| "voice-waiting".into())
					.h(gpui::px(crate::ui_theme::BODY_LINE_HEIGHT + 10.))
					.flex()
					.items_center()
					.justify_center()
					.text_size(gpui::px(11.))
					.role(Role::Status)
					.w_full()
					.min_w_0()
					.text_color(gpui::rgb(TEXT_MUTED))
					.aria_label("Connecting voice. Microphone off until connected")
					.child(crate::ui_loading::loading(""))
					.into_any_element(),
			);
		}
		Some(Self::microphone_waveform(
			"live-waveform",
			"Live microphone waveform",
			&voice.levels,
			voice.muted,
			window,
			cx,
		))
	}

	pub(super) fn microphone_waveform(
		id: &'static str,
		label: &'static str,
		levels: &std::collections::VecDeque<f32>,
		inactive: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> AnyElement {
		let panel = gpui::div()
			.id(id)
			.debug_selector(move || id.into())
			.role(Role::Image)
			.aria_label(if inactive { "Microphone inactive" } else { label })
			.w_full()
			.min_w_0()
			.h(gpui::px(crate::ui_theme::BODY_LINE_HEIGHT + 10.))
			.px(gpui::px(8.))
			.overflow_hidden()
			.flex()
			.items_center()
			.justify_center()
			.gap(gpui::px(2.));
		let amplitudes: [f32; 7] = std::array::from_fn(|i| {
			let level = levels.iter().rev().nth(i).copied().unwrap_or_default();
			ui_motion::value(
				(id, i),
				if inactive { 0. } else { 22. * level.clamp(0., 1.).sqrt() },
				window,
				cx,
			)
		});
		panel
			.child(
				gpui::canvas(
					|_, _, _| (),
					move |bounds, _, window, _| {
						let width = f32::from(bounds.size.width);
						let count = ((width + 2.) / 4.5).floor().max(1.) as usize;
						let stride =
							if count > 1 { (width - 2.5) / (count - 1) as f32 } else { 0. };
						for i in 0..count {
							let distance = if count > 1 {
								(2. * i as f32 / (count - 1) as f32 - 1.).abs()
							} else {
								0.
							};
							let envelope = 0.25 + 0.75 * (1. - distance).powf(0.7);
							let height =
								2. + amplitudes[(distance * 6.).round() as usize] * envelope;
							let alpha = if inactive { 0.35 } else { 0.3 + 0.7 * envelope };
							let color = gpui::rgba(
								((if inactive { TEXT_MUTED } else { BLUE }) << 8)
									| (alpha * 255.) as u32,
							);
							let bar = gpui::Bounds::new(
								bounds.origin
									+ gpui::point(
										gpui::px(i as f32 * stride),
										(bounds.size.height - gpui::px(height)) / 2.,
									),
								gpui::size(gpui::px(2.5), gpui::px(height)),
							);
							window.paint_quad(gpui::fill(bar, color).corner_radii(gpui::px(1.25)));
						}
					},
				)
				.w_full()
				.h_full(),
			)
			.into_any_element()
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
			gpui::div()
				.flex_none()
				.flex()
				.items_center()
				.gap(gpui::px(8.))
				.text_size(gpui::px(11.))
				.text_color(gpui::rgb(TEXT_MUTED))
				.child(
					gpui::div()
						.id("voice-status")
						.role(Role::Status)
						.aria_label(label.to_owned())
						.when(voice.connected && voice.muted, |view| view.child(label.to_owned())),
				)
				.children(voice.connected.then(|| {
					self.composer_control(
						"voice-mute",
						if voice.muted { "Unmute" } else { "Mute" }.into(),
						"Toggle microphone",
						|s, cx| {
							if let Some(voice) = &mut s.voice {
								let muted = !voice.muted;

								if voice
									.media
									.command(serde_json::json!({"operation":"mute","muted":muted}))
								{
									voice.muted = muted;
								} else {
									s.feedback = s.retire_voice_media().unwrap_or_else(|| {
										"Voice stopped because the microphone control could not be updated.".into()
									});
								}
							}

							cx.notify();
						},
						cx,
					)
				}))
				.child(self.composer_control(
					"voice-end",
					if voice.connected { "End" } else { "Cancel" }.into(),
					"End voice call · Existing work continues",
					|s, cx| {
						let _ = s.retire_voice_media();
						s.load_history(cx);
						cx.notify();
					},
					cx,
				))
				.into_any_element(),
		)
	}

	fn retire_voice_media(&mut self) -> Option<String> {
		let mut failure = None;

		if let Some(mut voice) = self.voice.take() {
			failure = drain_caption_events(&mut voice.captions, || voice.media.poll());

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

		failure
	}

	pub(super) fn reconcile_voice_captions(&mut self, work: &str, history: &AgentHistoryResult) {
		let AgentHistoryResult::Available { entries, .. } = history else { return };

		let native = self.timeline.native.binding.as_ref().is_some_and(|b| b.work == work);
		let ready: BTreeSet<_> = self
			.retired_voice_captions
			.iter()
			.filter(|v| {
				v.work.as_str() == work
					&& self.native_voice_presentation_ready(work, v.session.as_str())
			})
			.map(|v| v.session.as_str().to_owned())
			.collect();
		for call in self.retired_voice_captions.iter_mut().filter(|v| v.work.as_str() == work) {
			if native {
				if ready.contains(call.session.as_str()) {
					call.captions.clear();
				}
			} else {
				reconcile_captions(
					&call.session,
					&mut call.captions,
					&mut call.matched_receipts,
					entries,
				);
			}
		}
		self.retired_voice_captions.retain(|v| !v.captions.is_empty());
		// A save receipt is not a rendered timeline page. Keep active speech in
		// place for the whole call, even when storage has already accepted it.
	}

	fn native_voice_presentation_ready(&self, work: &str, session: &str) -> bool {
		self.timeline.native.binding.as_ref().is_some_and(|b| b.work == work)
            && super::voice::history::groups(&self.timeline.native.entries).iter().any(|group| {
                group.session == session && group.ended
                    && group.indices.iter().any(|i| matches!(&self.timeline.native.entries[*i].content,
                        decodex_protocol::AgentTimelineContent::VoiceBoundary {kind,..} if kind=="realtimeSessionStarted"))
            })
	}

	pub(super) fn live_voice_owns_session(&self, work: &str, session: &str) -> bool {
		self.voice
			.as_ref()
			.is_some_and(|v| v.work.as_str() == work && v.session.as_str() == session)
			|| self.retired_voice_captions.iter().any(|v| {
				v.work.as_str() == work
					&& v.session.as_str() == session
					&& !v.captions.is_empty()
					&& !self.native_voice_presentation_ready(work, session)
			})
	}

	pub(super) fn live_chat_caption(&self, work: &str, cx: &Context<Self>) -> Option<AnyElement> {
		let retired: Vec<_> = self
			.retired_voice_captions
			.iter()
			.filter(|v| {
				v.work.as_str() == work
					&& !self.native_voice_presentation_ready(work, v.session.as_str())
			})
			.collect();
		let captions = self
			.voice
			.as_ref()
			.filter(|v| v.work.as_str() == work)
			.into_iter()
			.flat_map(|v| &v.captions);
		// Keep arrival order across completion and interruption. Sorting unfinished
		// speakers moves an interruption above the reply it interrupted.
		let captions = presentation_captions(captions);

		if captions.is_empty() && retired.is_empty() {
			return None;
		}

		Some(
			gpui::div()
				.debug_selector(|| "live-voice-captions".into())
				.gap(gpui::px(crate::ui_theme::MESSAGE_GAP))
				.flex()
				.flex_col()
				.children(retired.into_iter().map(|call| {
					super::voice::history::VoiceBlock {
						viewport: Some(self.voice_history_viewport(cx)),
						key: format!("retired-{}-{}", work, call.session.as_str()),
						title: "Voice conversation".into(),
						text: call
							.captions
							.iter()
							.map(|c| format!("{}: {}", c.role, c.text))
							.collect::<Vec<_>>()
							.join("\n"),
					}
				}))
				.children(captions.into_iter().map(|caption| {
					agent_surface::speech_bubble(
						caption.role,
						&caption.text,
						&caption.key,
						Some(
							agent_surface::text_reveal::StreamingText {
								text: caption.text.clone(),
								key: caption.key.clone(),
							}
							.into_any_element(),
						),
					)
					.debug_selector({
						let key = caption.key.clone();
						move || format!("voice-caption-{key}")
					})
					.into_any_element()
				}))
				.into_any_element(),
		)
	}

	pub(super) fn live_voice_scroll_observer(
		&self,
		work: &str,
		cx: &Context<Self>,
	) -> Option<AnyElement> {
		let voice = self.voice.as_ref().filter(|v| v.work.as_str() == work && v.follow)?;
		let session = voice.session.clone();
		let scroll = self.timeline.scroll.get(work)?.clone();
		let surface = cx.entity().downgrade();
		Some(
			gpui::canvas(
				|_, _, _| {},
				move |_, _, _, cx| {
					// The new scroll range is available only after layout. A final wrapped
					// line can grow after the render-time follow check has already settled.
					if (scroll.offset().y + scroll.max_offset().y).abs() > gpui::px(0.5) {
						let surface = surface.clone();
						let session = session.clone();
						cx.defer(move |cx| {
							let _ = surface.update(cx, |s, cx| {
								if s.voice.as_ref().is_some_and(|v| {
									v.session == session
										&& v.follow
										&& s.selected.as_deref() == Some(v.work.as_str())
								}) {
									cx.notify();
								}
							});
						});
					}
				},
			)
			.absolute()
			.inset_0()
			.into_any_element(),
		)
	}

	pub(super) fn follow_voice_scroll(&self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(v) = self
			.voice
			.as_ref()
			.filter(|v| v.follow && self.selected.as_deref() == Some(v.work.as_str()))
		else {
			return;
		};
		let Some(scroll) = self.timeline.scroll.get(v.work.as_str()) else {
			return;
		};
		let current = f32::from(scroll.offset().y);
		let target = -f32::from(scroll.max_offset().y);

		if (target - current).abs() > 0.5 {
			let reduced = ui_motion::reduced();
			let next = if reduced { target } else { current + (target - current) * 0.24 };

			scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(next)));

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

fn presentation_captions<'a>(captions: impl Iterator<Item = &'a Caption>) -> Vec<Caption> {
	let mut rows: Vec<Caption> = Vec::new();
	for caption in captions.filter(|caption| !caption.text.is_empty()) {
		if let Some(previous) = rows.last_mut().filter(|previous| previous.role == caption.role) {
			previous.text.push_str("\n\n");
			previous.text.push_str(&caption.text);
			previous.complete = caption.complete;
		} else {
			rows.push(caption.clone());
		}
	}
	rows
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

/// Consume queued text and retain the first failure before releasing the media object.
/// The native mailbox has a 128-event bound; retirement never waits for more input.
fn drain_caption_events(
	captions: &mut Vec<Caption>,
	mut poll: impl FnMut() -> Option<Value>,
) -> Option<String> {
	let mut failure = None;

	for _ in 0..128 {
		let Some(event) = poll() else { break };

		if event["type"] == "caption" {
			update_caption(&event["event"], captions);
		} else if event["type"] == "error" && failure.is_none() {
			failure = event["message"].as_str().map(str::to_owned);
		}
	}

	failure
}

fn append_caption_delta(event: &Value, captions: &mut Vec<Caption>, role: &'static str) {
	let Some(delta) = event.pointer("/item/text").and_then(Value::as_str) else { return };

	if delta.is_empty() {
		return;
	}
	// WebRTC emits both flat transcript events and identified turn events.
	// Once a turn owns this speaker, its deltas are the only append source.
	if captions.iter().any(|c| c.role == role && !c.complete && !c.turn.is_empty()) {
		return;
	}
	if !captions.iter().any(|c| c.role == role && !c.complete) {
		captions.push(Caption {
			key: crate::shell::agent_surface::unique_command(),
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
		append_caption_delta(event, captions, role);

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
			if let Some(caption) = captions
				.iter_mut()
				.rev()
				.find(|c| c.turn.is_empty() && c.role == role && !c.complete)
			{
				// Adopt provisional text; the creation snapshot replaces that same caption.
				caption.turn = id.into();
			} else {
				captions.push(Caption {
					key: crate::shell::agent_surface::unique_command(),
					complete: false,
					turn: id.into(),
					role,
					text: String::new(),
				});
			}
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
				key: crate::shell::agent_surface::unique_command(),
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
			captions.push(Caption {
				key: crate::shell::agent_surface::unique_command(),
				complete: false,
				turn: id.into(),
				role,
				text: String::new(),
			});
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
	use gpui::{EntityInputHandler as _, Focusable as _, InteractiveElement as _, ParentElement};
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
			self.0.update(cx, |s, cx| gpui::div().children(s.voice_toolbar(cx)))
		}
	}

	struct VoiceHistoryView(Entity<AgentSurface>);
	impl Render for VoiceHistoryView {
		fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
			use gpui::Styled as _;
			self.0.update(cx, |s, cx| {
				let work = s
					.snapshot
					.as_ref()
					.unwrap()
					.work_items
					.iter()
					.find(|w| Some(&w.id) == s.selected.as_ref())
					.unwrap()
					.clone();
				gpui::div()
					.w_full()
					.debug_selector(|| "voice-test-history".into())
					.child(s.history_panel(&work, cx))
			})
		}
	}

	#[gpui::test]
	fn connecting_voice_has_no_active_waveform_or_microphone_control(
		cx: &mut gpui::TestAppContext,
	) {
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			surface.update(cx, |s, _| {
				s.prepare_voice_ui(
					Default::default(),
					Media,
					&EntityId::new("waiting").unwrap(),
					EntityId::new("agent").unwrap(),
				)
			});
			VoiceComposerView(surface)
		});
		visual.update(|w, cx| w.draw(cx).clear(cx));
		assert!(visual.debug_bounds("voice-waiting").is_some());
		assert!(visual.debug_bounds("live-waveform").is_none());
		assert!(visual.debug_bounds("composer-voice-mute").is_none());
		assert!(visual.debug_bounds("composer-voice-end").is_some());
		view.update(visual, |v, cx| {
			v.0.update(cx, |s, cx| {
				s.voice.as_mut().unwrap().connected = true;
				cx.notify();
			});
			cx.notify();
		});
		visual.update(|w, cx| w.draw(cx).clear(cx));
		assert!(visual.debug_bounds("voice-waiting").is_none());
		assert!(visual.debug_bounds("live-waveform").is_some());
		assert!(visual.debug_bounds("composer-voice-mute").is_some());
	}

	#[gpui::test]
	fn voice_follow_tracks_layout_growth_and_preserves_manual_reading(
		cx: &mut gpui::TestAppContext,
	) {
		let (surface, visual) = cx.add_window_view(|_, cx| AgentSurface::new(cx));
		visual.simulate_resize(gpui::size(gpui::px(900.), gpui::px(400.)));
		surface.update(visual, |s, cx| {
			s.visual_workspace_fixture(cx);
			s.workspace.graph_visible = false;
			s.timeline.voice_min_height.entry("agent".into()).or_default().set(10_000.);

			s.prepare_voice_ui(
				Default::default(),
				Media,
				&EntityId::new("call").unwrap(),
				EntityId::new("agent").unwrap(),
			);
			assert_eq!(s.timeline.voice_min_height["agent"].get(), 0.);

			s.voice.as_mut().unwrap().captions = vec![Caption {
				key: "growing".into(),
				turn: "growing".into(),
				role: "assistant",
				complete: false,
				text: "Hello".into(),
			}];
			cx.notify();
		});
		std::thread::sleep(std::time::Duration::from_millis(240));
		for _ in 0..50 {
			visual.update(|w, cx| w.draw(cx).clear(cx));
		}
		surface.update(visual, |s, cx| {
			s.voice.as_mut().unwrap().captions[0].text =
				"A long reply that wraps over multiple lines. ".repeat(80);
			cx.notify();
		});
		std::thread::sleep(std::time::Duration::from_millis(150));
		for _ in 0..60 {
			visual.update(|w, cx| w.draw(cx).clear(cx));
		}
		surface.update(visual, |s, cx| {
			let scroll = &s.timeline.scroll["agent"];
			assert!(scroll.max_offset().y > gpui::px(100.));
			assert!(
				(scroll.offset().y + scroll.max_offset().y).abs() < gpui::px(1.),
				"new wrapped lines must remain visible"
			);
			s.scroll_history(
				&gpui::ScrollWheelEvent {
					delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(100.))),
					..Default::default()
				},
				cx,
			);
			assert!(!s.voice.as_ref().unwrap().follow);
		});
		let before = surface.read_with(visual, |s, _| s.timeline.scroll["agent"].offset());
		surface.update(visual, |s, cx| {
			s.voice.as_mut().unwrap().captions[0].text.push_str(&"More speech. ".repeat(80));
			cx.notify();
		});
		std::thread::sleep(std::time::Duration::from_millis(150));
		for _ in 0..10 {
			visual.update(|w, cx| w.draw(cx).clear(cx));
		}
		surface.read_with(visual, |s, _| assert_eq!(s.timeline.scroll["agent"].offset(), before));
	}

	#[gpui::test]
	fn native_history_renders_one_caption_block_and_keeps_interruption_order(
		cx: &mut gpui::TestAppContext,
	) {
		use crate::shell::agent_surface::native_timeline::{Binding, Timeline};
		let (view, visual) = cx.add_window_view(|_, cx| {
			let surface = cx.new(AgentSurface::new);
			surface.update(cx, |s, cx| {
				s.visual_workspace_fixture(cx);
				let work = s
					.snapshot
					.as_mut()
					.unwrap()
					.work_items
					.iter_mut()
					.find(|w| Some(&w.id) == s.selected.as_ref())
					.unwrap();
				work.codex_thread_id = Some("thread".into());
				let id = work.id.clone();
				let mut timeline = Timeline::default();
				assert!(timeline.replace(
					Binding {
						work: id.clone(),
						thread: "thread".into(),
						account: "account".into()
					},
					decodex_protocol::AgentTimelinePage {
						thread_id: "thread".into(),
						entries: vec![],
						next_cursor: None,
						weather: Default::default(),
						safety_buffering_turn_id: None,
						active_realtime_session_at_page_start: None
					}
				));
				s.timeline.native = timeline;
				s.prepare_voice_ui(
					Default::default(),
					Media,
					&EntityId::new("call").unwrap(),
					EntityId::new(id).unwrap(),
				);
			});
			VoiceHistoryView(surface)
		});
		let surface = view.read_with(visual, |v, _| v.0.clone());
		for width in [360., 1000.] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(1200.)));
			surface.update(visual, |s, cx| {
				s.voice.as_mut().unwrap().captions.clear();
				cx.notify();
			});
			visual.update(|w, cx| w.draw(cx).clear(cx));
			let baseline = visual.debug_bounds("voice-test-history").unwrap().size.height;
			surface.update(visual, |s, cx| {
				s.voice.as_mut().unwrap().captions = [
					("first", "user", true, "Count to ten"),
					("reply", "assistant", false, "One, two, three"),
					("interrupt", "user", false, "Stop"),
				]
				.into_iter()
				.map(|(key, role, complete, text)| Caption {
					key: key.into(),
					turn: key.into(),
					role,
					complete,
					text: text.into(),
				})
				.collect();
				cx.notify();
			});
			for completed in [false, true] {
				surface.update(visual, |s, cx| {
					s.voice.as_mut().unwrap().captions[1].complete = completed;
					cx.notify();
				});
				visual.update(|w, cx| w.draw(cx).clear(cx));
				let first = visual.debug_bounds("voice-caption-first").unwrap();
				let reply = visual.debug_bounds("voice-caption-reply").unwrap();
				let interrupt = visual.debug_bounds("voice-caption-interrupt").unwrap();
				assert!(
					first.bottom() + gpui::px(crate::ui_theme::MESSAGE_GAP) <= reply.top()
						&& reply.bottom() + gpui::px(crate::ui_theme::MESSAGE_GAP)
							<= interrupt.top(),
					"Completion must not reorder or overlap speech"
				);
				let block = visual.debug_bounds("live-voice-captions").unwrap();
				let total = visual.debug_bounds("voice-test-history").unwrap().size.height;
				assert!(
					total - baseline
						<= block.size.height + gpui::px(crate::ui_theme::MESSAGE_GAP + 1.),
					"The native history must not insert the live block twice"
				);
				assert!(block.right() <= gpui::px(width + 1.));
				surface.update(visual, |s, cx| {
					let work = s.voice.as_ref().unwrap().work.as_str().to_owned();
					s.reconcile_voice_captions(&work, &saved_voice_history(Some("call")));
					s.timeline.native.entries = vec![decodex_protocol::AgentTimelineEntry {
						position: 0,
						content: decodex_protocol::AgentTimelineContent::Speech {
							item_id: "saved".into(),
							session_id: "call".into(),
							role: "user".into(),
							text: "Count to ten".into(),
							truncated: false,
						},
					}];
					cx.notify();
				});
				visual.update(|w, cx| w.draw(cx).clear(cx));
				assert_eq!(
					visual.debug_bounds("voice-caption-first").unwrap().origin,
					first.origin,
					"History refresh must not move active speech"
				);
				assert!(
					visual.debug_bounds("voice-history-block").is_none(),
					"Do not insert a second live representation from native history"
				);
			}
		}
	}

	#[gpui::test]
	fn live_receipts_and_native_refresh_keep_speech_until_closed_history_is_ready(
		cx: &mut gpui::TestAppContext,
	) {
		use crate::shell::agent_surface::native_timeline::Binding;
		use decodex_protocol::{AgentTimelineContent as Content, AgentTimelineEntry};
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.prepare_voice_ui(
				Default::default(),
				Media,
				&EntityId::new("call").unwrap(),
				EntityId::new("agent").unwrap(),
			);
			s.timeline.native.binding = Some(Binding {
				work: "agent".into(),
				thread: "thread".into(),
				account: "account".into(),
			});
			voice::update_caption(
				&serde_json::json!({"type":"turn.done","turn":{"id":"u","role":"user","transcript":"Hello"}}),
				&mut s.voice.as_mut().unwrap().captions,
			);
			let key = s.voice.as_ref().unwrap().captions[0].key.clone();
			s.reconcile_voice_captions("agent", &saved_voice_history(Some("call")));
			assert_eq!(s.voice.as_ref().unwrap().captions[0].key, key);
			s.timeline.native.entries.push(AgentTimelineEntry {
				position: 0,
				content: Content::VoiceBoundary {
					item_id: "started".into(),
					session_id: "call".into(),
					kind: "realtimeSessionStarted".into(),
					outcome: None,
				},
			});
			s.timeline.native.entries.push(AgentTimelineEntry {
				position: 1,
				content: Content::Speech {
					item_id: "speech".into(),
					session_id: "call".into(),
					role: "user".into(),
					text: "Hello".into(),
					truncated: false,
				},
			});
			s.reconcile_voice_captions("agent", &saved_voice_history(Some("call")));
			assert!(s.live_chat_caption("agent", cx).is_some());
			assert!(s.live_voice_owns_session("agent", "call"));
			assert!(!s.live_voice_owns_session("agent", "other-call"));
			assert!(!s.live_voice_owns_session("other-work", "call"));
			let _ = s.retire_voice_media();
			s.reconcile_voice_captions("agent", &saved_voice_history(Some("call")));
			assert!(
				s.live_chat_caption("agent", cx).is_some(),
				"Do not clear speech before the closed native group arrives"
			);
			assert!(s.live_voice_owns_session("agent", "call"));
			s.timeline.native.entries.push(AgentTimelineEntry {
				position: 2,
				content: Content::VoiceBoundary {
					item_id: "closed".into(),
					session_id: "call".into(),
					kind: "realtimeSessionClosed".into(),
					outcome: Some("ended".into()),
				},
			});
			let start = s.timeline.native.entries.remove(0);
			s.reconcile_voice_captions("agent", &saved_voice_history(Some("call")));
			assert!(
				s.live_chat_caption("agent", cx).is_some(),
				"A partial native page must not discard the full retained call"
			);
			s.timeline.native.entries.insert(0, start);
			assert!(s.live_chat_caption("agent", cx).is_none());
			assert!(
				!s.live_voice_owns_session("agent", "call"),
				"Native group and live text must switch on the same page"
			);
			s.reconcile_voice_captions("agent", &saved_voice_history(Some("call")));
			assert!(s.retired_voice_captions.is_empty());
			cx.notify();
		});
	}

	#[gpui::test]
	fn voice_tail_flush_does_not_render_task_duration_but_preserves_failure(
		cx: &mut gpui::TestAppContext,
	) {
		use crate::shell::agent_surface::native_timeline::Binding;
		use decodex_protocol::{AgentTimelineContent as Content, AgentTimelineEntry};
		let (view, visual) = cx.add_window_view(|_, cx| {
			VoiceHistoryView(cx.new(|cx| {
				let mut surface = AgentSurface::new(cx);
				surface.visual_workspace_fixture(cx);
				surface
			}))
		});
		let surface = view.read_with(visual, |v, _| v.0.clone());
		surface.update(visual,|s,cx|{
            s.visual_workspace_fixture(cx);
            let work = s.snapshot.as_mut().unwrap().work_items.iter_mut().find(|w|Some(&w.id)==s.selected.as_ref()).unwrap();
            work.codex_thread_id=Some("thread".into());
            s.timeline.native.binding=Some(Binding {work:work.id.clone(),thread:"thread".into(),account:"account".into()});
            s.timeline.native.entries=vec![
                AgentTimelineEntry {position:0, content:Content::Speech {item_id:"speech".into(),session_id:"call".into(),role:"user".into(),text:"Hello".into(),truncated:false}},
                AgentTimelineEntry {position:1, content:Content::VoiceBoundary {item_id:"closed".into(),session_id:"call".into(),kind:"realtimeSessionClosed".into(),outcome:Some("ended".into())}},
                AgentTimelineEntry {position:2, content:Content::Item {turn_id:"tail".into(),item_id:"input".into(),kind:"userMessage".into(),phase:None,text:"<realtime_delegation><source>transcript_tail_flush</source><input>Session ended</input><transcript_delta>user: Hello</transcript_delta></realtime_delegation>".into(),attachments:vec![],activity:None,app_ui:false,collaboration:None,truncated:false}},
                AgentTimelineEntry {position:3, content:Content::TurnBoundary {turn_id:"tail".into(),completed:true,status:Some("completed".into()),error:None,duration_ms:Some(0),usage:None,usage_summary:None}},
            ];
            cx.notify();
        });
		visual.update(|w, cx| w.draw(cx).clear(cx));
		assert!(visual.debug_bounds("voice-history-block").is_some());
		assert!(
			visual.debug_bounds("turn-metrics-hover").is_none(),
			"A transcript flush is not a timed user task"
		);
		surface.update(visual, |s, cx| {
			if let Content::TurnBoundary { status, .. } = &mut s.timeline.native.entries[3].content
			{
				*status = Some("failed".into());
			}
			cx.notify();
		});
		visual.update(|w, cx| w.draw(cx).clear(cx));
		assert!(
			visual.debug_bounds("turn-metrics-hover").is_some(),
			"Actual failed turns must remain visible"
		);
	}

	#[gpui::test]
	fn starting_media_retires_availability_feedback(cx: &mut gpui::TestAppContext) {
		let surface = cx.new(AgentSurface::new);
		surface.update(cx, |s, cx| {
			s.feedback = "Checking voice availability…".into();
			s.prepare_voice_ui(
				Default::default(),
				Media,
				&EntityId::new("call").unwrap(),
				EntityId::new("agent").unwrap(),
			);
			assert!(s.feedback.is_empty());
			assert_eq!(s.voice.as_ref().unwrap().connection_status, "Preparing audio…");
			let _ = s.retire_voice_media();
			assert!(
				s.feedback.is_empty(),
				"Ending the call must not resurrect availability feedback"
			);
			cx.notify();
		});
	}

	#[test]
	fn consecutive_speech_appends_to_one_stable_bubble_until_the_speaker_changes() {
		let mut captions = Vec::new();
		for (id, role, text) in [
			("u", "user", "Count to ten"),
			("a1", "assistant", "One, two"),
			("a2", "assistant", "Three, four"),
			("stop", "user", "Stop"),
			("a3", "assistant", "Okay"),
		] {
			voice::update_caption(
				&serde_json::json!({"type":"turn.done","turn":{"id":id,"role":role,"transcript":text}}),
				&mut captions,
			);
		}
		let first = voice::presentation_captions(captions[..2].iter());
		let continued = voice::presentation_captions(captions[..3].iter());
		assert_eq!(continued.len(), 2);
		assert_eq!(first[1].key, continued[1].key);
		assert_eq!(continued[1].text, "One, two\n\nThree, four");
		let interrupted = voice::presentation_captions(captions.iter());
		assert_eq!(
			interrupted.iter().map(|c| c.role).collect::<Vec<_>>(),
			vec!["user", "assistant", "user", "assistant"]
		);
		assert_eq!(captions.len(), 5, "Presentation must not merge source turn identities");
	}

	#[test]
	fn mixed_webrtc_events_keep_one_caption_per_turn_without_fragment_tails() {
		let mut captions = Vec::new();
		for (id, role, first, rest, final_text) in [
			("u1", "user", "Can", " you hear me?", "Can you hear me?"),
			("a1", "assistant", "Yes", ", I can.", "Yes, I can."),
			("u2", "user", "Count", " to ten", "Count to ten."),
			("a2", "assistant", "One", ", two, three", "One, two, three"),
			("u3", "user", "Stop", "", "Stop."),
			("a3", "assistant", "Okay", ", stopped.", "Okay, stopped."),
		] {
			let kind = format!("{role}_transcript.added")
				.replace("user_", "input_")
				.replace("assistant_", "output_");
			voice::update_caption(
				&serde_json::json!({"type":kind,"item":{"text":first}}),
				&mut captions,
			);
			let key = captions.last().unwrap().key.clone();
			voice::update_caption(
				&serde_json::json!({"type":"turn.created","turn":{"id":id,"role":role,"transcript":first}}),
				&mut captions,
			);
			assert_eq!(
				captions.last().unwrap().key,
				key,
				"Turn identity must adopt its provisional caption"
			);
			voice::update_caption(
				&serde_json::json!({"type":kind,"item":{"text":rest}}),
				&mut captions,
			);
			voice::update_caption(
				&serde_json::json!({"type":"turn.delta","turn_id":id,"delta":rest}),
				&mut captions,
			);
			assert_eq!(
				captions.last().unwrap().text,
				format!("{first}{rest}"),
				"Parallel flat events must not double live text"
			);
			voice::update_caption(
				&serde_json::json!({"type":"turn.done","turn":{"id":id,"role":role,"transcript":final_text}}),
				&mut captions,
			);
		}
		assert_eq!(captions.len(), 6);
		assert!(captions.iter().all(|c| c.complete && !c.turn.is_empty()));
		assert_eq!(captions[0].text, "Can you hear me?");
		assert_eq!(captions[4].text, "Stop.");
	}

	#[test]
	fn captions_preserve_both_speakers_and_late_finals() {
		let mut captions = Vec::new();

		for event in [
			serde_json::json!({"type":"turn.created","turn":{"id":"u","role":"user","transcript":"Hello"}}),
			serde_json::json!({"type":"turn.created","turn":{"id":"a","role":"assistant","transcript":"Hi"}}),
			serde_json::json!({"type":"turn.delta","turn_id":"u","delta":" world"}),
			serde_json::json!({"type":"turn.done","turn":{"id":"u","transcript":"Hello, world!"}}),
		] {
			voice::update_caption(&event, &mut captions);
		}

		assert_eq!(
			captions.iter().map(|c| (c.role, c.text.as_str())).collect::<Vec<_>>(),
			vec![("user", "Hello, world!"), ("assistant", "Hi")]
		);

		voice::update_caption(
			&serde_json::json!({"type":"turn.done","turn":{"id":"a","transcript":""}}),
			&mut captions,
		);

		assert_eq!(captions[0].text, "Hello, world!");
		assert!(captions[1].text.is_empty());
	}

	#[test]
	fn frameless_captions_accept_interleaved_deltas_and_idless_finals() {
		let mut captions = Vec::new();

		for event in [
			serde_json::json!({"type":"input_transcript.added","item":{"text":"hello"}}),
			serde_json::json!({"type":"output_transcript.added","item":{"text":"reply"}}),
			serde_json::json!({"type":"input_transcript.added","item":{"text":" world"}}),
			serde_json::json!({"type":"turn.done","turn":{"role":"user","transcript":"Hello, world!"}}),
			serde_json::json!({"type":"turn.done","turn":{"id":"late-id","role":"assistant","transcript":"Reply."}}),
			serde_json::json!({"type":"turn.done","turn":{"id":"final-only","role":"user","transcript":"Another sentence."}}),
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
			&serde_json::json!({"type":"input_transcript.added","item":{"text":"discard"}}),
			&mut captions,
		);
		voice::update_caption(
			&serde_json::json!({"type":"turn.done","turn":{"role":"user","transcript":""}}),
			&mut captions,
		);

		assert_eq!(captions.len(), 4);
		assert!(captions[3].text.is_empty() && captions[3].complete);

		voice::update_caption(
			&serde_json::json!({"type":"input_transcript.added","item":{"text":"界".repeat(12_000)}}),
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
					&serde_json::json!({"type":"turn.done","turn":{"role":"user","transcript":prefix.clone()+suffix}}),
					&mut captions,
				);
			} else {
				for text in [&*prefix, suffix] {
					voice::update_caption(
						&serde_json::json!({"type":"input_transcript.added","item":{"text":text}}),
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
			&serde_json::json!({"type":"input_transcript.added","item":{"text":"uncorrected"}}),
			&mut captions,
		);

		let mut pending = std::collections::VecDeque::from([
			serde_json::json!({"type":"level","level":0.2}),
			serde_json::json!({"type":"caption","event":{"type":"turn.done","turn":{"role":"user","transcript":"Corrected final."}}}),
			serde_json::json!({"type":"caption","event":{"type":"output_transcript.added","item":{"text":"Reply"}}}),
			serde_json::json!({"type":"error","message":"The audio device disconnected."}),
			serde_json::json!({"type":"error","message":"The control channel closed."}),
			serde_json::json!({"type":"ended"}),
		]);

		let failure = voice::drain_caption_events(&mut captions, || pending.pop_front());

		assert_eq!(failure.as_deref(), Some("The audio device disconnected."));

		assert!(pending.is_empty());
		assert_eq!(
			captions.iter().map(|c| (c.role, c.text.as_str(), c.complete)).collect::<Vec<_>>(),
			vec![("user", "Corrected final.", true), ("assistant", "Reply", false)]
		);

		let mut polled = 0;

		let failure = voice::drain_caption_events(&mut captions, || {
			polled += 1;

			Some(serde_json::json!({"type":"level"}))
		});

		assert!(failure.is_none());
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
							key: crate::shell::agent_surface::unique_command(),
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
				window.draw(cx).clear(cx);
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
	fn composer_text_uses_full_width_above_a_stable_action_row(cx: &mut gpui::TestAppContext) {
		let (view, visual) =
			cx.add_window_view(|_, cx| VoiceComposerView(cx.new(AgentSurface::new)));
		let surface = view.read_with(visual, |v, _| v.0.clone());
		for width in [360., 800.] {
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(400.)));
			let mut footer_height = None;
			for text in ["Short draft", "First line\nSecond line\nThird line"] {
				surface.update(visual, |s, cx| {
					s.composer.update(cx, |input, cx| input.set_content(text, cx))
				});
				visual.update(|window, cx| {
					window.draw(cx).clear(cx);
				});
				let editor = visual.debug_bounds("composer-editor-area").unwrap();
				let footer = visual.debug_bounds("composer-action-row").unwrap();
				let send = visual.debug_bounds("composer-send").unwrap();
				assert_eq!(editor.size.width, footer.size.width);
				assert!(editor.origin.y + editor.size.height <= footer.origin.y);
				assert!(send.origin.x + send.size.width <= gpui::px(width));
				assert_eq!(send.origin.y + send.size.height, footer.origin.y + footer.size.height);
				if let Some(height) = footer_height {
					assert_eq!(footer.size.height, height);
				}
				footer_height = Some(footer.size.height);
			}
		}
	}

	#[gpui::test]
	fn active_voice_replaces_the_editor_and_restores_its_draft(cx: &mut gpui::TestAppContext) {
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
			visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(400.)));
			visual.update(|window, cx| {
				window.draw(cx).clear(cx);
				input.update(cx, |input, cx| {
					assert!(
						input.bounds_for_range(0..5, Default::default(), window, cx).is_none(),
						"Live must not render the text editor below the waveform"
					);
					assert_eq!(input.content(), "Draft");
				});
			});
			let wave = visual.debug_bounds("live-waveform").expect("waveform replaces the editor");
			assert_eq!(wave.size.height, gpui::px(crate::ui_theme::BODY_LINE_HEIGHT + 10.));
			assert!(
				wave.origin.x >= gpui::px(0.) && wave.origin.x + wave.size.width <= gpui::px(width),
				"waveform {wave:?} must fit width {width}"
			);
		}
		surface.update(visual, |s, cx| {
			let _ = s.retire_voice_media();
			cx.notify();
		});
		visual.update(|window, cx| {
			window.focus(&input.focus_handle(cx), cx);
			window.draw(cx).clear(cx);
		});
		visual.simulate_keystrokes("shift-enter");
		input.read_with(visual, |input, _| assert_eq!(input.content(), "Draft\n"));
		surface.read_with(visual, |s, _| assert!(s.voice.is_none()));
	}

	fn saved_voice_history(session: Option<&str>) -> AgentHistoryResult {
		AgentHistoryResult::Available {
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
		}
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
					key: crate::shell::agent_surface::unique_command(),
					complete: true,
					turn: "turn".into(),
					role: "user",
					text: "Hello".into(),
				}],
				levels: Default::default(),
				follow: true,
			});

			for sample in [saved_voice_history(None), saved_voice_history(Some("other-call"))] {
				s.reconcile_voice_captions("agent", &sample);

				assert!(s.live_chat_caption("agent", cx).is_some());
			}

			s.reconcile_voice_captions("other-work", &saved_voice_history(Some("call")));

			assert!(s.live_chat_caption("agent", cx).is_some());

			s.reconcile_voice_captions("agent", &saved_voice_history(Some("call")));

			assert!(
				s.live_chat_caption("agent", cx).is_some(),
				"Saving active speech must not remove its visible bubble"
			);

			s.voice.as_mut().unwrap().captions.push(Caption {
				key: crate::shell::agent_surface::unique_command(),
				complete: true,
				turn: "repeat".into(),
				role: "user",
				text: "Hello".into(),
			});
			s.reconcile_voice_captions("agent", &saved_voice_history(Some("call")));

			assert!(
				s.live_chat_caption("agent", cx).is_some(),
				"One receipt must not hide two captions"
			);
			assert!(s.live_chat_caption("other-work", cx).is_none());

			s.voice.as_mut().expect("voice").captions[0].role = "assistant";

			assert!(s.live_chat_caption("agent", cx).is_some());

			s.apply_voice_response(&EntityId::new("old-call").expect("id"), None, cx);

			assert!(s.voice.is_some(), "An old request must not stop the current call");

			s.history = Some(("agent".into(), saved_voice_history(Some("call"))));
			s.apply_voice_response(&EntityId::new("call").expect("id"), None, cx);

			assert!(s.voice.is_none(), "A failed control connection must retire local media");
			assert!(
				s.live_chat_caption("agent", cx).is_some(),
				"Retired media must retain text until history arrives"
			);
			assert!(s.live_chat_caption("other-work", cx).is_none());

			let mut saved = saved_voice_history(Some("call"));

			if let AgentHistoryResult::Available { entries, .. } = &mut saved {
				entries[0].id = 2;
				entries[0].kind = "assistant".into();
				entries[0].receipt.as_mut().unwrap().event_kind = "voice_assistant".into();
			}

			s.reconcile_voice_captions("agent", &saved);

			assert!(s.retired_voice_captions.is_empty());

			s.history = None;

			assert!(
				s.live_chat_caption("agent", cx).is_none(),
				"Pagination cannot resurrect accepted captions"
			);
			assert!(s.feedback.contains("service connection was lost"));
			assert!(s.poll_voice_media(cx).is_none(), "The loop must enter exact-session cleanup");
		});
	}
}
