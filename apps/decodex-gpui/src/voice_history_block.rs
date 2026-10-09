#[path = "voice_conversation_groups.rs"] mod conversations;
pub(crate) use conversations::groups;
// Presentation of Codex realtime handoff envelopes. Stored messages stay unchanged.
use crate::{
	shell::agent_surface::markdown,
	ui_motion,
	ui_theme::{HOVER_FILL, TEXT_MUTED},
};
use gpui::{App, IntoElement, KeyDownEvent, RenderOnce, Role, SharedString, Window, prelude::*};

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

fn transcript_markdown(text: &str) -> String {
	text.lines()
		.map(|line| {
			if let Some(text) = line.strip_prefix("user: ") {
				format!("**You:** {text}")
			} else if let Some(text) = line.strip_prefix("assistant: ") {
				format!("**Assistant:** {text}")
			} else {
				line.to_owned()
			}
		})
		.collect::<Vec<_>>()
		.join("\n\n")
}

#[derive(IntoElement)]
pub(crate) struct VoiceBlock {
	pub key: String,
	pub title: String,
	pub expanded: bool,
	pub text: String,
}
impl RenderOnce for VoiceBlock {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let expanded = window.use_keyed_state(
			SharedString::from(format!("voice-block-{}", self.key)),
			cx,
			|_, _| self.expanded,
		);
		let open = *expanded.read(cx);
		let click = expanded.clone();
		let keyboard = expanded.clone();
		gpui::div()
			.w_full()
			.min_w_0()
			.rounded(gpui::px(10.))
			.overflow_hidden()
			.border_1()
			.border_color(gpui::rgba(0xffffff12))
			.bg(gpui::rgba(0xffffff05))
			.child(
				gpui::div()
					.id(SharedString::from(format!("voice-block-toggle-{}", self.key)))
					.debug_selector(|| "voice-history-block".into())
					.role(Role::Button)
					.aria_label(self.title.clone())
					.aria_expanded(open)
					.tab_index(0)
					.w_full()
					.flex()
					.items_center()
					.justify_between()
					.gap_2()
					.px_3()
					.py_3()
					.cursor_pointer()
					.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
					.focus(|s| s.bg(gpui::rgba(HOVER_FILL)))
					.child(self.title)
					.child(
						gpui::div()
							.text_size(gpui::px(11.))
							.text_color(gpui::rgb(TEXT_MUTED))
							.child(if open { "Hide transcript" } else { "Show transcript" }),
					)
					.on_click(move |_, _, cx| {
						click.update(cx, |open, cx| {
							*open = !*open;
							cx.notify();
						})
					})
					.on_key_down(move |e: &KeyDownEvent, _, cx| {
						if ["enter", "space"].contains(&e.keystroke.key.as_str()) && !e.is_held {
							keyboard.update(cx, |open, cx| {
								*open = !*open;
								cx.notify();
							});
							cx.stop_propagation();
						}
					}),
			)
			.child(ui_motion::disclosure(
				SharedString::from(format!("voice-transcript-{}", self.key)),
				open,
				gpui::div()
					.debug_selector(|| "voice-history-transcript".into())
					.px_3()
					.pb_3()
					.pt_2()
					.child(markdown::render(
						&transcript_markdown(&self.text),
						&format!("voice-transcript-{}", self.key),
					))
					.child(markdown::response_copy_button(
						&format!("voice-copy-{}", self.key),
						"Copy voice transcript",
						self.text,
					)),
			))
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
				title: "Voice conversation".into(),
				expanded: false,
				text: "user: Hello\nassistant: Hi".into(),
			}
		}
	}
	#[gpui::test]
	fn voice_block_expands_from_its_header(cx: &mut gpui::TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, _| Fixture);
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("voice-history-transcript").is_none());
		let button = visual.debug_bounds("voice-history-block").unwrap();
		visual.simulate_click(button.center(), Default::default());
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		assert!(visual.debug_bounds("voice-history-transcript").is_some());
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
