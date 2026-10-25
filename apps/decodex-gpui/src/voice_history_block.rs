#[path = "voice_conversation_groups.rs"] mod conversations;
#[path = "voice_history_sticky.rs"] mod sticky;

pub(crate) use self::{
	conversations::{Conversation, groups, saved_groups, tail_flush_turns},
	sticky::{ReadingSpace, Viewport},
};

// Presentation of Codex realtime handoff envelopes. Stored messages stay unchanged.
use std::rc::Rc;

use gpui::{
	App, Div, FontWeight, IntoElement, KeyDownEvent, Pixels, RenderOnce, Role, SharedString,
	Stateful, Window,
	prelude::{InteractiveElement, ParentElement, StatefulInteractiveElement, Styled},
};

use crate::{
	shell::{
		agent_surface::{self, markdown},
		workspace_symbols::{self, Symbol},
	},
	ui_motion,
	ui_theme::{self, HOVER_FILL, MESSAGE_GAP, PANEL_HEADER_TINT, PRESSED_FILL, TEXT_MUTED},
};
use sticky::{Group, HEADER_HEIGHT, Toggle as _};

#[derive(IntoElement)]
pub(crate) struct VoiceBlock {
	pub key: String,
	pub title: String,
	pub text: String,
	pub viewport: Option<Viewport>,
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
						.bg(ui_theme::rgba(0xffffff03))
						.px_4()
						.pb_3()
						.pt_2()
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
						))
						.child(markdown::response_copy_button(
							&format!("voice-copy-{}", self.key),
							"Copy voice transcript",
							self.text,
						)),
				),
			));

		Group {
			body: body.into_any_element(),
			header: Some(
				gpui::div()
					.w_full()
					.flex()
					.items_start()
					.child(
						gpui::div()
							.w_full()
							.debug_selector(|| "voice-history-block".into())
							.child(header),
					)
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

pub(crate) fn handoff(text: &str) -> Option<String> {
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
	let text = if source == "transcript_tail_flush" {
		transcript?.to_owned()
	} else if let Some(transcript) = transcript.filter(|t| *t != input.trim()) {
		format!("{}\n\n{}", input.trim(), transcript)
	} else {
		input.trim().to_owned()
	};

	Some(text.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"))
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
	let header_key = key.to_owned();

	gpui::div()
		.id(SharedString::from(format!("voice-block-toggle-{}", key)))
		.debug_selector(move || format!("voice-history-block-{header_key}"))
		.role(Role::Button)
		.aria_label(title.clone())
		.aria_expanded(open)
		.tab_index(0)
		.rounded(gpui::px(4.))
		.flex_none()
		.w_full()
		.bg(ui_theme::rgba(PANEL_HEADER_TINT))
		.flex()
		.items_center()
		.text_size(gpui::px(12.))
		.font_weight(FontWeight::NORMAL)
		.text_color(ui_theme::rgb(TEXT_MUTED))
		.gap_2()
		.px_3()
		.h(gpui::px(HEADER_HEIGHT))
		.cursor_pointer()
		.hover(|s| s.bg(ui_theme::rgba(HOVER_FILL)))
		.focus(|s| s.bg(ui_theme::rgba(HOVER_FILL)))
		.active(|s| s.bg(ui_theme::rgba(PRESSED_FILL)))
		.child(
			gpui::div().opacity(0.65).child(workspace_symbols::icon_sized(Symbol::Waveform, 12.)),
		)
		.child(title)
		.child(gpui::div().flex_1())
		.child(workspace_symbols::process_chevron(
			SharedString::from(format!("voice-chevron-{}", key)),
			open,
		))
}

#[cfg(test)]
mod tests {
	use gpui::{Context, Render};

	use crate::shell::agent_surface::voice::history::{self, IntoElement, VoiceBlock, Window};
	struct Fixture;
	impl Render for Fixture {
		fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
			VoiceBlock {
				key: "fixture".into(),
				viewport: None,
				title: "Voice conversation".into(),
				text: "user: Hello\nassistant: Hi".into(),
			}
		}
	}
	#[gpui::test]
	fn voice_block_expands_from_its_header(cx: &mut gpui::TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, _| Fixture);

		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("voice-history-transcript-fixture").is_none());

		let button = visual.debug_bounds("voice-history-block-fixture").unwrap();

		visual.simulate_click(button.center(), Default::default());
		visual.run_until_parked();
		visual.update(crate::ui_frame::draw_current_frame);

		assert!(visual.debug_bounds("voice-history-transcript-fixture").is_some());
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
