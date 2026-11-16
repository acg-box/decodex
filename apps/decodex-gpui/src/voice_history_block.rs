#[path = "voice_conversation_groups.rs"] mod conversations;
#[path = "voice_history_sticky.rs"] mod sticky;

pub(crate) use self::{
	conversations::{Conversation, groups, saved_groups, tail_flush_turns},
	sticky::{ReadingSpace, Viewport},
};

// Presentation of Codex realtime handoff envelopes. Stored messages stay unchanged.
use std::rc::Rc;

use gpui::{
	App, Div, IntoElement, KeyDownEvent, Pixels, RenderOnce, SharedString, Stateful, Window,
	prelude::{InteractiveElement, ParentElement, StatefulInteractiveElement, Styled},
};

use crate::{
	shell::agent_surface::{
		self, conversation_divider, markdown,
		response_metrics::{self, ResponseMetrics},
	},
	ui_motion,
	ui_theme::MESSAGE_GAP,
};
use sticky::{Group, HEADER_HEIGHT, Toggle as _};

#[derive(IntoElement)]
pub(crate) struct VoiceBlock {
	pub key: String,
	pub action: Option<gpui::AnyElement>,
	pub title: String,
	pub text: String,
	pub viewport: Option<Viewport>,
	pub presentation: Option<decodex_protocol::AgentVoicePresentation>,
}
impl RenderOnce for VoiceBlock {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let expanded = window.use_keyed_state(
			SharedString::from(format!("voice-block-{}", self.key)),
			cx,
			|_, _| Expansion::default(),
		);
		let open = expanded.read(cx).open;
		let closing_shift = expanded.read(cx).closing_shift;
		let opacity = ui_motion::value(
			SharedString::from(format!("voice-opacity-{}", self.key)),
			if open { 1. } else { 0. },
			window,
			cx,
		);
		let click = expanded.clone();
		let keyboard = expanded.clone();
		let position = Rc::new(std::cell::Cell::new((gpui::px(0.), gpui::px(0.))));
		let click_position = position.clone();
		let keyboard_position = position.clone();
		let click_viewport = self.viewport.clone();
		let keyboard_viewport = self.viewport.clone();
		let transcript_key = self.key.clone();
		let header = voice_header(&self.key, self.title, open)
			.on_click(move |_, _, cx| {
				click_viewport.toggle(click.read(cx).open, &click_position, cx);

				click.update(cx, |state, cx| {
					state.flip(&click_position);
					cx.notify();
				})
			})
			.on_key_down(move |e: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&e.keystroke.key.as_str()) && !e.is_held {
					keyboard_viewport.toggle(keyboard.read(cx).open, &keyboard_position, cx);
					keyboard.update(cx, |state, cx| {
						state.flip(&keyboard_position);
						cx.notify();
					});
					cx.stop_propagation();
				}
			});
		let body = gpui::div()
			.group(SharedString::from(format!("history-response-{}", self.key)))
			.w_full()
			.min_w_0()
			.child(gpui::div().h(gpui::px(HEADER_HEIGHT)))
			.child(ui_motion::disclosure(
				SharedString::from(format!("voice-transcript-{}", self.key)),
				// Keep speech at its reading position until it is invisible. Then
				// shrink the document, so bottom clamping cannot drag fading text.
				open || opacity > 0.001,
				gpui::div().w_full().debug_selector(|| "voice-history-transcript".into()).child(
					gpui::div()
						.relative()
						.top(-closing_shift)
						.opacity(opacity)
						.debug_selector(move || {
							format!("voice-history-transcript-{transcript_key}")
						})
						.pt(gpui::px(crate::ui_theme::PROCESS_CONTENT_GAP))
						.flex()
						.flex_col()
						.gap(gpui::px(MESSAGE_GAP))
						.children(transcript_turns(&self.text).into_iter().enumerate().map(
							|(index, (role, text))| {
								agent_surface::speech_bubble(
									role,
									&text,
									&format!("voice-transcript-{}-{index}", self.key),
									None,
								)
								.debug_selector(move || format!("voice-transcript-turn-{index}"))
							},
						)),
				),
			))
			.children((!open).then(|| {
				gpui::div()
					.debug_selector(|| "voice-history-preview".into())
					.mt(gpui::px(crate::ui_theme::PROCESS_CONTENT_GAP))
					.child(agent_surface::selectable_prose(
						format!("voice-preview-{}", self.key),
						preview(&self.text),
					))
			}))
			.child(response_metrics::actions_footer(
				&format!("voice-{}", self.key),
				ResponseMetrics {
					spend: self.presentation.as_ref().and_then(|p| p.spend.clone()),
					key: format!("voice-{}", self.key),
					duration_ms: self.presentation.as_ref().and_then(|p| p.duration_ms),
					status: None,
					usage: None,
					timing: None,
				}
				.into_any_element(),
				Some(("Copy voice transcript", self.text)),
				self.action,
			));

		Group {
			body: body.into_any_element(),
			header: Some(
				conversation_divider::row(
					gpui::div()
						.debug_selector(|| "voice-history-block".into())
						.child(header)
						.into_any_element(),
					conversation_divider::Style::Section,
				)
				.debug_selector(|| "voice-history-divider".into())
				.into_any_element(),
			),
			open,
			position,
		}
	}
}

#[derive(Default)]
struct Expansion {
	open: bool,
	closing_shift: Pixels,
}
impl Expansion {
	fn flip(&mut self, position: &std::cell::Cell<(Pixels, Pixels)>) {
		let (group_top, header_top) = position.get();

		self.closing_shift = if self.open { header_top - group_top } else { gpui::px(0.) };
		self.open = !self.open;
	}
}

pub(crate) struct Delegation<'a> {
	pub source: &'a str,
	input: &'a str,
	pub transcript: Option<&'a str>,
}

pub(crate) fn delegation(text: &str) -> Option<Delegation<'_>> {
	let body = text
		.trim()
		.strip_prefix("<realtime_delegation>")?
		.strip_suffix("</realtime_delegation>")?
		.trim();
	let (source, body) = if let Some(body) = body.strip_prefix("<source>") {
		let (source, body) = body.split_once("</source>")?;

		if !matches!(source.trim(), "handoff" | "transcript_tail_flush") {
			return None;
		}

		(source.trim(), body.trim())
	} else {
		("handoff", body)
	};
	let (input, rest) = body.strip_prefix("<input>")?.split_once("</input>")?;
	let rest = rest.trim();
	let transcript = if rest.is_empty() {
		None
	} else {
		Some(rest.strip_prefix("<transcript_delta>")?.strip_suffix("</transcript_delta>")?.trim())
	};
	Some(Delegation { source, input: input.trim(), transcript })
}

pub(crate) fn handoff(text: &str) -> Option<String> {
	let Delegation { source, input, transcript } = delegation(text)?;
	let text = if source == "transcript_tail_flush" {
		transcript?.to_owned()
	} else if let Some(transcript) = transcript.filter(|t| *t != input.trim()) {
		format!("{}\n\n{}", input.trim(), transcript)
	} else {
		input.trim().to_owned()
	};

	Some(unescape(&text))
}

pub(crate) fn unescape(text: &str) -> String {
	text.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// Preview the first user utterance, or the first available speech without user input.
pub(crate) fn preview(text: &str) -> String {
	let turns = transcript_turns(text);
	let turn = turns
		.iter()
		.find(|(role, text)| *role == "user" && !text.trim().is_empty())
		.or_else(|| turns.iter().find(|(_, text)| !text.trim().is_empty()));
	let plain = turn
		.map(|(_, text)| markdown::plain_text(text))
		.unwrap_or_default()
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ");
	let mut excerpt = plain.chars().take(160).collect::<String>();
	if plain.chars().count() > 160 {
		excerpt.push('…')
	}
	excerpt
}

fn transcript_turns(text: &str) -> Vec<(&str, String)> {
	let mut turns: Vec<(&str, String)> = Vec::new();

	for line in text.lines() {
		let speech = line
			.strip_prefix("user: ")
			.map(|text| ("user", text))
			.or_else(|| line.strip_prefix("assistant: ").map(|text| ("assistant", text)));

		if let Some((role, text)) = speech {
			if let Some(last) = turns.last_mut().filter(|last| last.0 == role) {
				last.1.push_str("\n\n");
				last.1.push_str(text);
			} else {
				turns.push((role, text.into()));
			}
		} else if let Some(last) = turns.last_mut() {
			last.1.push('\n');
			last.1.push_str(line);
		} else {
			turns.push(("assistant", line.into()));
		}
	}

	turns
}

fn voice_header(key: &str, title: String, open: bool) -> Stateful<Div> {
	let selector = format!("voice-history-block-{key}");
	conversation_divider::control(
		format!("voice-block-toggle-{key}"),
		title,
		open,
		conversation_divider::Style::Section,
	)
	.debug_selector(move || selector.clone())
}

#[cfg(test)]
mod tests {
	use gpui::{Context, Render};

	use crate::shell::agent_surface::voice::history::{self, IntoElement, VoiceBlock, Window};
	struct Fixture;
	impl Render for Fixture {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			VoiceBlock {
				action: None,
	presentation: Some(decodex_protocol::AgentVoicePresentation {
		spend: Some(decodex_protocol::AgentSpendDto {
			usd_micros: 7_500,
			estimated: true,
			partial: false,
			explanation: "Voice usage estimated from elapsed call time.".into(),
		}),
		duration_ms: Some(9000),
	}),
				key: "fixture".into(),
				viewport: None,
				title: "Voice conversation".into(),
				text: "user: Hello\nassistant: Hi\nuser: Keep the entire call\nassistant: All four turns are retained".into(),
			}
		}
	}
	#[gpui::test]
	fn voice_block_expands_from_its_header(cx: &mut gpui::TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, _| Fixture);

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("voice-history-transcript-fixture").is_none());
		assert!(visual.debug_bounds("voice-history-preview").is_some());
		assert!(visual.debug_bounds("turn-metrics-hover").is_some());
		for metric in [
			"conversation-metric-Duration",
			"conversation-metric-Input tokens",
			"conversation-metric-Output tokens",
			"conversation-metric-Spend",
		] {
			assert!(visual.debug_bounds(metric).is_some());
		}
		let copy = visual.debug_bounds("copy-response-voice-fixture").unwrap();
		visual.simulate_click(copy.center(), Default::default());
		visual.update(|_,cx|assert_eq!(cx.read_from_clipboard().and_then(|item|item.text()).as_deref(),Some("user: Hello\nassistant: Hi\nuser: Keep the entire call\nassistant: All four turns are retained")));

		let button = visual.debug_bounds("voice-history-block-fixture").unwrap();

		visual.simulate_click(button.center(), Default::default());
		visual.run_until_parked();
		visual.executor().advance_clock(std::time::Duration::from_millis(250));
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("voice-history-transcript-fixture").is_some());
		for selector in [
			"voice-transcript-turn-0",
			"voice-transcript-turn-1",
			"voice-transcript-turn-2",
			"voice-transcript-turn-3",
		] {
			assert!(visual.debug_bounds(selector).is_some());
		}
		assert!(visual.debug_bounds("voice-history-preview").is_none());
		for metric in [
			"conversation-metric-Duration",
			"conversation-metric-Input tokens",
			"conversation-metric-Output tokens",
			"conversation-metric-Spend",
		] {
			assert!(visual.debug_bounds(metric).is_some());
		}
		let divider = visual.debug_bounds("voice-history-divider").unwrap();
		let first = visual.debug_bounds("voice-transcript-turn-0").unwrap();
		let last = visual.debug_bounds("voice-transcript-turn-3").unwrap();
		let metrics = visual.debug_bounds("turn-metrics-hover").unwrap();
		assert_eq!(divider.size.height, gpui::px(super::HEADER_HEIGHT));
		assert_eq!(first.top() - divider.bottom(), gpui::px(crate::ui_theme::PROCESS_CONTENT_GAP));
		assert_eq!(metrics.top() - last.bottom(), gpui::px(crate::ui_theme::METADATA_GAP));
	}

	#[test]
	fn preview_uses_first_user_speech_or_original_acknowledgement() {
		assert_eq!(
			history::preview("user: Check **the tests**\nassistant: Working"),
			"Check the tests"
		);
		assert_eq!(history::preview("assistant: Mm-hmm."), "Mm-hmm.");
		assert_eq!(
			history::preview("assistant: Hello\nuser: Explain the metrics\nassistant: Sure"),
			"Explain the metrics"
		);
		assert!(history::preview(" \nuser: \nassistant:   ").is_empty());
		assert_eq!(
			history::preview(&format!("assistant: {}", "字".repeat(161))),
			format!("{}…", "字".repeat(160))
		);
	}

	#[test]
	fn transcript_turns_preserve_multiline_speech_and_merge_adjacent_speakers() {
		assert_eq!(
			history::transcript_turns(
				"user: Hello\nsecond line\nassistant: Hi\nassistant: More\nuser: Stop"
			),
			vec![
				("user", "Hello\nsecond line".into()),
				("assistant", "Hi\n\nMore".into()),
				("user", "Stop".into()),
			]
		);
		assert_eq!(
			history::transcript_turns("Unlabelled text"),
			vec![("assistant", "Unlabelled text".into())]
		);
	}

	#[test]
	fn handoff_preserves_transcripts_and_leaves_ordinary_text_untouched() {
		let tail = "<realtime_delegation><source>transcript_tail_flush</source><input>Internal handoff instruction</input><transcript_delta>user: 你好\nassistant: Hello</transcript_delta></realtime_delegation>";

		assert_eq!(history::handoff(tail).as_deref(), Some("user: 你好\nassistant: Hello"));
		assert_eq!(
			history::handoff("<realtime_delegation><input>Say hello</input></realtime_delegation>")
				.as_deref(),
			Some("Say hello")
		);
		assert_eq!(
			history::handoff(
				"<realtime_delegation><input>a &lt; b &amp; c &gt; d &amp;lt;</input></realtime_delegation>"
			)
			.as_deref(),
			Some("a < b & c > d &lt;")
		);

		for plain in [
			"Discuss </transcript_delta>",
			"<realtime_delegation>broken",
			"<realtime_delegation><source>other</source><input>Keep me</input></realtime_delegation>",
		] {
			assert!(history::handoff(plain).is_none());
		}
	}
}
