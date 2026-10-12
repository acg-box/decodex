#[path = "voice_conversation_groups.rs"] mod conversations;
pub(crate) use conversations::{groups, tail_flush_turns};
// Presentation of Codex realtime handoff envelopes. Stored messages stay unchanged.
use crate::{
	shell::agent_surface::{self, markdown},
	ui_motion,
	ui_theme::{HOVER_FILL, TEXT_MUTED},
};
use gpui::{App, IntoElement, KeyDownEvent, RenderOnce, Role, SharedString, Window, prelude::*};
#[path = "voice_history_sticky.rs"] mod sticky;
use crate::shell::workspace_symbols::{self, Symbol};
use sticky::Toggle as _;
pub(crate) use sticky::Viewport;

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
			|_, _| false,
		);
		let open = *expanded.read(cx);
		let click = expanded.clone();
		let keyboard = expanded.clone();
		let position = std::rc::Rc::new(std::cell::Cell::new((gpui::px(0.), gpui::px(0.))));
		let click_position = position.clone();
		let keyboard_position = position.clone();
		let click_viewport = self.viewport.clone();
		let keyboard_viewport = self.viewport.clone();
		let header_key = self.key.clone();
		let transcript_key = self.key.clone();
		let header = gpui::div()
			.id(SharedString::from(format!("voice-block-toggle-{}", self.key)))
			.debug_selector(move || format!("voice-history-block-{header_key}"))
			.role(Role::Button)
			.aria_label(self.title.clone())
			.aria_expanded(open)
			.tab_index(0)
			.rounded(gpui::px(6.))
			.w_full()
			.flex()
			.items_center()
			.text_size(gpui::px(12.))
			.text_color(gpui::rgb(TEXT_MUTED))
			.gap_2()
			.px_3()
			.h(gpui::px(sticky::HEADER_HEIGHT))
			.cursor_pointer()
			.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
			.focus(|s| s.bg(gpui::rgba(HOVER_FILL)))
			.child(workspace_symbols::icon(Symbol::Microphone))
			.child(self.title)
			.child(workspace_symbols::process_chevron(
				SharedString::from(format!("voice-chevron-{}", self.key)),
				open,
			))
			.on_click(move |_, _, cx| {
				click_viewport.toggle(*click.read(cx), &click_position, cx);
				click.update(cx, |open, cx| {
					*open = !*open;
					cx.notify();
				})
			})
			.on_key_down(move |e: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&e.keystroke.key.as_str()) && !e.is_held {
					keyboard_viewport.toggle(*keyboard.read(cx), &keyboard_position, cx);
					keyboard.update(cx, |open, cx| {
						*open = !*open;
						cx.notify();
					});
					cx.stop_propagation();
				}
			});
		let body = gpui::div()
			.w_full()
			.min_w_0()
			.child(gpui::div().h(gpui::px(sticky::HEADER_HEIGHT)))
			.child(ui_motion::disclosure(
				SharedString::from(format!("voice-transcript-{}", self.key)),
				open,
				gpui::div().w_full().debug_selector(|| "voice-history-transcript".into()).child(
					gpui::div()
						.debug_selector(move || {
							format!("voice-history-transcript-{transcript_key}")
						})
						.px_3()
						.pb_3()
						.pt_2()
						.flex()
						.flex_col()
						.gap(gpui::px(crate::ui_theme::MESSAGE_GAP))
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
		sticky::Group {
			body: body.into_any_element(),
			header: Some(
				gpui::div()
					.debug_selector(|| "voice-history-block".into())
					.w_full()
					.child(header)
					.into_any_element(),
			),
			open,
			position,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::{Context, Render};
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
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});
		assert!(visual.debug_bounds("voice-history-transcript-fixture").is_none());
		let button = visual.debug_bounds("voice-history-block-fixture").unwrap();
		visual.simulate_click(button.center(), Default::default());
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});
		assert!(visual.debug_bounds("voice-history-transcript-fixture").is_some());
	}

	#[test]
	fn transcript_turns_preserve_multiline_speech_and_merge_adjacent_speakers() {
		assert_eq!(
			transcript_turns(
				"user: Hello\nsecond line\nassistant: Hi\nassistant: More\nuser: Stop"
			),
			vec![
				("user", "Hello\nsecond line".into()),
				("assistant", "Hi\n\nMore".into()),
				("user", "Stop".into()),
			]
		);
		assert_eq!(
			transcript_turns("Unlabelled text"),
			vec![("assistant", "Unlabelled text".into())]
		);
	}

	#[test]
	fn handoff_preserves_transcripts_and_leaves_ordinary_text_untouched() {
		let tail = "<realtime_delegation><source>transcript_tail_flush</source><input>Internal handoff instruction</input><transcript_delta>user: 你好\nassistant: Hello</transcript_delta></realtime_delegation>";
		assert_eq!(handoff(tail).as_deref(), Some("user: 你好\nassistant: Hello"));
		assert_eq!(
			handoff("<realtime_delegation><input>Say hello</input></realtime_delegation>")
				.as_deref(),
			Some("Say hello")
		);
		assert_eq!(
			handoff(
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
			assert!(handoff(plain).is_none());
		}
	}
}
