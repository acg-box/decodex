//! Complete source sentences for the graph, independent of its pixel width.
use pulldown_cmark::{Event, Options, Parser, TagEnd};
use unicode_segmentation::UnicodeSegmentation;

/// Keep one complete thought. Markdown headings and lead-ins do not displace
/// the first content paragraph. The full response remains in native history.
pub(in super::super) fn message(source: &str) -> String {
	let mut paragraph = String::new();
	let mut fallback = String::new();
	for event in Parser::new_ext(source, Options::ENABLE_TABLES) {
		match event {
			Event::Text(text) | Event::Code(text) => paragraph.push_str(&text),
			Event::SoftBreak | Event::HardBreak => paragraph.push(' '),
			Event::End(TagEnd::Heading(_)) => {
				fallback = std::mem::take(&mut paragraph);
			},
			Event::End(TagEnd::Paragraph | TagEnd::Item | TagEnd::CodeBlock) => {
				if paragraph.trim().is_empty() {
					continue;
				}
				if paragraph.trim_end().ends_with(':') {
					fallback = std::mem::take(&mut paragraph);
				} else {
					break;
				}
			},
			_ => {},
		}
	}
	if paragraph.trim().is_empty() {
		paragraph = fallback;
	}
	let paragraph = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
	paragraph.unicode_sentences().next().unwrap_or(&paragraph).trim().to_owned()
}

/// A work title describes its purpose, while the number identifies the agent.
/// Strip request politeness only; do not infer a new responsibility from output.
pub(in super::super) fn purpose(source: &str) -> String {
	let sentence = message(source);
	let mut title = sentence.as_str();
	for prefix in ["Please ", "please ", "Help me ", "help me ", "Could you ", "Can you "] {
		if let Some(rest) = title.strip_prefix(prefix) {
			title = rest;
		}
	}
	let title = title.trim().trim_end_matches(['.', '。']);
	let mut characters = title.chars();
	characters.next().map_or_else(String::new, |first| {
		first.to_uppercase().collect::<String>() + characters.as_str()
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn long_sentences_keep_the_object_and_ending() {
		let source = "Simmer canned tomatoes, white beans, and vegetable broth for 10 minutes, then stir in a pinch of dried basil. Divide the soup between two bowls.";
		assert_eq!(
			message(source),
			"Simmer canned tomatoes, white beans, and vegetable broth for 10 minutes, then stir in a pinch of dried basil."
		);
		assert_eq!(
			purpose("Help me plan a small soup dinner for two. Reply in English without tools."),
			"Plan a small soup dinner for two"
		);
	}

	#[test]
	fn headings_formatting_and_lead_ins_do_not_hide_the_action() {
		assert_eq!(
			message(
				"## Result\n\nHere is the result:\n\n**All 47 checks passed**, including reconnect and recovery. More evidence follows."
			),
			"All 47 checks passed, including reconnect and recovery."
		);
		assert_eq!(message("## Done"), "Done");
	}

	#[test]
	fn native_sentences_preserve_unicode_and_decimal_values() {
		assert_eq!(
			message("检查已完成，47 项测试全部通过，输入栏未修改。稍后提供截图。"),
			"检查已完成，47 项测试全部通过，输入栏未修改。"
		);
		assert_eq!(
			message("Measured 12.5 ms in src/graph.rs after the update. Next run follows."),
			"Measured 12.5 ms in src/graph.rs after the update."
		);
	}
}
