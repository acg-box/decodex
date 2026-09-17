//! Subscription dictation edits a draft; only the ordinary Send action starts work.
use super::{voice::Media, *};
use decodex_protocol::{DictationBuffer, DictationPhase, DictationRequest, DictationStatus};
use gpui::AnyElement;
use std::{
	collections::VecDeque,
	time::{Duration, Instant},
};

pub(super) struct DictationUi {
	media: Media,
	session: EntityId,
	original: String,
	expected: String,
	request: Option<DictationRequest>,
	audio: VecDeque<DictationBuffer>,
	capture_started: bool,
	network_ready: bool,
	capture_ended: bool,
	finishing: bool,
	finish_sent: bool,
	status: String,
	level: f32,
	started: Instant,
}
impl ChiefSurface {
	pub(super) fn start_dictation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.dictation_task.is_some() || self.voice_task.is_some() {
			return;
		}
		let Some(profile) = self.profile.clone() else { return };
		let Ok(mut media) = Media::new(window) else {
			self.feedback = "Dictation requires the current signed macOS application.".into();
			cx.notify();
			return;
		};
		if !media.command(serde_json::json!({"operation":"dictate","input":self.audio_input})) {
			self.feedback = "The microphone could not start.".into();
			cx.notify();
			return;
		}
		let id = EntityId::new(unique_command()).expect("dictation identity");
		let original = self.composer.read(cx).content().to_owned();
		self.dictation = Some(DictationUi {
			media,
			session: id.clone(),
			original: original.clone(),
			expected: original,
			request: Some(DictationRequest::Start { session_id: id.clone() }),
			audio: VecDeque::new(),
			capture_started: true,
			network_ready: false,
			capture_ended: false,
			finishing: false,
			finish_sent: false,
			status: "Connecting dictation…".into(),
			level: 0.,
			started: Instant::now(),
		});
		self.dictation_task = Some(cx.spawn(async move |surface, cx| {
			loop {
				let request = surface.update(cx, |s, cx| s.poll_dictation(cx)).ok().flatten();
				let Some(request) = request else { break };
				let profile = profile.clone();
				let response = cx
					.background_executor()
					.spawn(async move {
						let runtime = tokio::runtime::Builder::new_current_thread()
							.enable_all()
							.build()
							.ok()?;
						runtime.block_on(ChiefClient::new(profile).dictation(request)).ok()
					})
					.await;
				let _ = surface.update(cx, |s, cx| {
					if let Some(response) = response {
						s.apply_dictation(response, cx)
					} else {
						s.dictation = None;
						s.feedback="Dictation connection was lost. Your received text remains in the draft.".into();
						cx.notify();
					}
				});
				cx.background_executor().timer(Duration::from_millis(20)).await;
			}
			cx.background_executor()
				.spawn(async move {
					if let Ok(runtime) =
						tokio::runtime::Builder::new_current_thread().enable_all().build()
					{
						let _ = runtime.block_on(
							ChiefClient::new(profile)
								.dictation(DictationRequest::Cancel { session_id: id }),
						);
					}
				})
				.await;
			let _ = surface.update(cx, |s, cx| {
				s.dictation_task = None;
				cx.notify();
			});
		}));
		cx.notify();
	}

	pub(super) fn cancel_dictation(&mut self, cx: &mut Context<Self>) {
		if let Some(dictation) = self.dictation.take() {
			// A manual edit belongs to the user and must not be replaced by an old draft.
			if self.composer.read(cx).content() == dictation.expected {
				self.composer.update(cx, |input, cx| input.set_content(&dictation.original, cx));
			}
		}
		cx.notify();
	}

	pub(super) fn finish_dictation(&mut self, cx: &mut Context<Self>) {
		if let Some(dictation) = &mut self.dictation
			&& !dictation.finishing
		{
			dictation.finishing = true;
			dictation.status = "Finishing dictation…".into();
			if !dictation.capture_started {
				dictation.capture_ended = true;
			}
			dictation.media.command(serde_json::json!({"operation":"finish"}));
		}
		cx.notify();
	}

	fn poll_dictation(&mut self, cx: &mut Context<Self>) -> Option<DictationRequest> {
		let dictation = self.dictation.as_mut()?;
		if self.composer.read(cx).content() != dictation.expected {
			self.dictation = None;
			self.feedback = "Dictation stopped to preserve your edit.".into();
			cx.notify();
			return None;
		}
		for _ in 0..64 {
			let Some(event) = dictation.media.poll() else { break };
			match event["type"].as_str() {
				Some("pcm") => {
					if let Some(audio) =
						event["audio"].as_str().and_then(|s| DictationBuffer::new(s).ok())
					{
						dictation.audio.push_back(audio);
					}
					dictation.level =
						event["level"].as_f64().unwrap_or_default().clamp(0., 1.) as f32;
					if dictation.audio.len() > 128 {
						self.dictation = None;
						self.feedback="Dictation stopped because audio delivery fell behind. Your received text remains in the draft.".into();
						cx.notify();
						return None;
					}
				},
				Some("status") =>
					dictation.status =
						event["message"].as_str().unwrap_or("Opening microphone…").into(),
				Some("dictation_ready") => {
					dictation.status = "Listening".into();
					dictation.started = Instant::now();
				},
				Some("ended") => dictation.capture_ended = true,
				Some("error") => {
					self.feedback =
						event["message"].as_str().unwrap_or("Microphone disconnected.").into();
					self.dictation = None;
					cx.notify();
					return None;
				},
				_ => {},
			}
		}
		cx.notify();
		if let Some(request) = dictation.request.take() {
			return Some(request);
		}
		if dictation.network_ready
			&& let Some(audio) = dictation.audio.pop_front()
		{
			return Some(DictationRequest::Audio { session_id: dictation.session.clone(), audio });
		}
		if dictation.network_ready && dictation.capture_ended && !dictation.finish_sent {
			dictation.finish_sent = true;
			return Some(DictationRequest::Finish { session_id: dictation.session.clone() });
		}
		Some(DictationRequest::Poll { session_id: dictation.session.clone() })
	}

	fn apply_dictation(&mut self, status: DictationStatus, cx: &mut Context<Self>) {
		let Some(dictation) = self.dictation.as_mut().filter(|d| d.session == status.session_id)
		else {
			return;
		};
		if self.composer.read(cx).content() != dictation.expected {
			self.dictation = None;
			cx.notify();
			return;
		}
		let text = merge_draft(&dictation.original, status.text.as_str());
		if text != dictation.expected {
			dictation.expected = text.clone();
			self.composer.update(cx, |input, cx| input.set_content(&text, cx));
		}
		match status.phase {
			DictationPhase::Listening => dictation.network_ready = true,
			DictationPhase::Finalizing => dictation.status = "Final correction…".into(),
			DictationPhase::Complete => self.dictation = None,
			DictationPhase::Failed => {
				self.feedback = status
					.message
					.map_or_else(|| "Dictation failed.".into(), |m| m.as_str().into());
				self.dictation = None;
			},
			_ => {},
		}
		cx.notify();
	}

	pub(super) fn dictation_controls(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
		let d = self.dictation.as_ref()?;
		let label = if d.status == "Listening" {
			format!("Listening · {}s", d.started.elapsed().as_secs())
		} else {
			d.status.clone()
		};
		Some(
			div()
				.w_full()
				.flex()
				.items_center()
				.gap(px(8.))
				.text_size(px(11.))
				.text_color(rgb(ui_theme::TEXT_MUTED))
				.child(div().h(px(12.)).w(px(26.)).flex().items_center().gap(px(2.)).children(
					(0..5).map(|i| {
						div()
							.w(px(2.))
							.h(px(3. + d.level * 9. * (1. - (i as f32 - 2.).abs() / 4.)))
							.rounded_full()
							.bg(rgb(ui_theme::BLUE))
					}),
				))
				.child(
					div()
						.id("dictation-status")
						.role(Role::Status)
						.aria_label(label.clone())
						.child(label),
				)
				.child(div().flex_1())
				.child(self.composer_control(
					"dictation-cancel",
					"Cancel".into(),
					"Cancel dictation and restore the draft",
					|s, cx| s.cancel_dictation(cx),
					cx,
				))
				.child(self.composer_control(
					"dictation-finish",
					"Done".into(),
					"Finish dictation · Keep text in the draft",
					|s, cx| s.finish_dictation(cx),
					cx,
				))
				.into_any_element(),
		)
	}
}
fn merge_draft(original: &str, transcript: &str) -> String {
	if transcript.is_empty() {
		return original.into();
	}
	if original.is_empty() || original.ends_with(char::is_whitespace) {
		format!("{original}{transcript}")
	} else {
		format!("{original} {transcript}")
	}
}
#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn provisional_revisions_replace_only_the_dictated_suffix() {
		let original = "Keep this draft\n";
		assert_eq!(merge_draft(original, "partial"), "Keep this draft\npartial");
		assert_eq!(merge_draft(original, "Final."), "Keep this draft\nFinal.");
		assert_eq!(merge_draft(original, ""), original);
	}
	fn recording(original: &str) -> DictationUi {
		DictationUi {
			media: Media,
			session: EntityId::new("dictation-test").expect("id"),
			original: original.into(),
			expected: original.into(),
			request: None,
			audio: VecDeque::new(),
			capture_started: true,
			network_ready: true,
			capture_ended: false,
			finishing: false,
			finish_sent: false,
			status: "Listening".into(),
			level: 0.,
			started: Instant::now(),
		}
	}

	#[gpui::test]
	fn cancellation_restores_the_draft_and_final_text_waits_for_send(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			let original = "Existing draft";
			let set_recording = |s: &mut ChiefSurface, cx: &mut Context<ChiefSurface>| {
				s.composer.update(cx, |input, cx| input.set_content(original, cx));
				s.dictation = Some(recording(original));
			};
			let response = |phase, text| DictationStatus {
				session_id: EntityId::new("dictation-test").expect("id"),
				phase,
				text: DictationBuffer::new(text).expect("text"),
				message: None,
			};
			set_recording(s, cx);
			s.apply_dictation(response(DictationPhase::Listening, "partial"), cx);
			assert_eq!(s.composer.read(cx).content(), "Existing draft partial");
			s.cancel_dictation(cx);
			assert_eq!(s.composer.read(cx).content(), original);
			set_recording(s, cx);
			s.apply_dictation(response(DictationPhase::Listening, "partial"), cx);
			s.apply_dictation(response(DictationPhase::Complete, "Final correction."), cx);
			assert_eq!(s.composer.read(cx).content(), "Existing draft Final correction.");
			assert!(s.dictation.is_none());
			assert!(!s.sending);
			set_recording(s, cx);
			s.composer.update(cx, |input, cx| input.set_content("My manual edit", cx));
			s.cancel_dictation(cx);
			assert_eq!(s.composer.read(cx).content(), "My manual edit");
		});
	}
	#[gpui::test]
	fn early_audio_waits_for_subscription_ready_and_drains_before_finish(
		cx: &mut gpui::TestAppContext,
	) {
		let surface = cx.new(ChiefSurface::new);
		surface.update(cx, |s, cx| {
			s.composer.update(cx, |input, cx| input.set_content("", cx));
			let mut capture = recording("");
			capture.network_ready = false;
			capture.capture_ended = true;
			capture.audio.push_back(DictationBuffer::new("AAA=").expect("frame"));
			s.dictation = Some(capture);
			assert!(matches!(s.poll_dictation(cx), Some(DictationRequest::Poll { .. })));
			s.apply_dictation(
				DictationStatus {
					session_id: EntityId::new("dictation-test").expect("id"),
					phase: DictationPhase::Listening,
					text: DictationBuffer::new("").expect("text"),
					message: None,
				},
				cx,
			);
			assert!(matches!(s.poll_dictation(cx), Some(DictationRequest::Audio { .. })));
			assert!(matches!(s.poll_dictation(cx), Some(DictationRequest::Finish { .. })));
			assert!(matches!(s.poll_dictation(cx), Some(DictationRequest::Poll { .. })));
		});
	}
}
