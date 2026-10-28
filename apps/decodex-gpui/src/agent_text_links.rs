//! Web links in prose keep their visible source and byte ranges.
use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag};

pub(super) fn prose_links(text: &str) -> Vec<(Range<usize>, String)> {
	let protected: Vec<_> = Parser::new(text)
		.into_offset_iter()
		.filter_map(|(event, range)| {
			matches!(
				event,
				Event::Code(_)
					| Event::Start(Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. })
			)
			.then_some(range)
		})
		.collect();
	web_links(text)
		.into_iter()
		.filter(|(range, _)| {
			!protected.iter().any(|blocked| range.start < blocked.end && blocked.start < range.end)
		})
		.collect()
}

pub fn web_links(text: &str) -> Vec<(Range<usize>, String)> {
	let mut links = Vec::new();
	let mut offset = 0;
	while let Some(start) = text[offset..].find("http").map(|index| offset + index) {
		offset = start + 4;
		if !text[start..].starts_with("https://") && !text[start..].starts_with("http://") {
			continue;
		}
		if text[..start]
			.chars()
			.next_back()
			.is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '@'))
		{
			continue;
		}
		let mut end = text[start..]
			.char_indices()
			.find_map(|(index, ch)| {
				(ch.is_whitespace()
					|| ch.is_control()
					|| matches!(
						ch,
						'<' | '>'
							| '"'
							| '\''
							| '`'
							| '['
							| ']'
							| '{'
							| '}'
							| '。'
							| '，'
							| '；'
							| '！'
							| '？'
							| '、'
							| '）'
							| '（'
					))
				.then_some(start + index)
			})
			.unwrap_or(text.len());
		loop {
			let candidate = &text[start..end];
			let Some(last) = candidate.chars().next_back() else { break };
			if matches!(last, '.' | ',' | ';' | ':' | '!' | '?')
				|| last == ')'
					&& candidate.bytes().filter(|ch| *ch == b')').count()
						> candidate.bytes().filter(|ch| *ch == b'(').count()
			{
				end -= last.len_utf8();
			} else {
				break;
			}
		}
		if let Ok(url) = reqwest::Url::parse(&text[start..end])
			&& url.host_str().is_some()
		{
			links.push((start..end, text[start..end].to_owned()));
			offset = end;
		}
	}
	links
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn prose_urls_preserve_unicode_parentheses_and_skip_code_and_destinations() {
		let text = "See https://example.com/文档_(一). And (https://example.org/path). `https://code.test` [named](https://target.test)\n```\nhttps://fence.test\n```";
		let links = prose_links(text);
		assert_eq!(
			links.iter().map(|(_, url)| url.as_str()).collect::<Vec<_>>(),
			["https://example.com/文档_(一)", "https://example.org/path"]
		);
		for (range, url) in links {
			assert_eq!(&text[range], url);
		}
		assert!(web_links("file:///tmp/test javascript:alert(1) xhttps://example.com").is_empty());
	}
}
