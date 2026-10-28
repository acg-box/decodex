// Link scanner adapted from openai/codex at e0a64cf2bc4535eb330c22857260a7856c1e8749.
// Copyright OpenAI. Licensed under Apache-2.0; see agent_math/LICENSE-APACHE.
//! Display-only holdback. Newlines and completion always preserve the original source.
use pulldown_cmark::{Event, Parser, Tag};

pub(super) fn visible(text: &str, complete: bool) -> &str {
	if complete {
		return text;
	}
	let line_start = text.rfind('\n').map_or(0, |index| index + 1);
	let mut preview = LinkPreview::default();
	preview.scan(&text[line_start..], 0);
	let Some((start, _)) = preview.destination else {
		return text;
	};
	let end = line_start + start;
	// The scanner handles inline code; the parser supplies fenced and indented block context.
	if Parser::new(text).into_offset_iter().any(|(event, range)| {
		matches!(event, Event::Code(_) | Event::Start(Tag::CodeBlock(_))) && range.contains(&end)
	}) {
		return text;
	}
	&text[..end]
}

/// Scans append-only source without buffering or rescanning a growing destination.
/// Keep the label visible; the opening `(` and everything after it wait for closure.
#[derive(Default)]
struct LinkPreview {
	brackets: usize,
	closed_label: bool,
	destination: Option<(usize, usize)>,
	delimiter: Option<u8>,
	whitespace: bool,
	escaped: bool,
	code_ticks: usize,
	pending_ticks: usize,
}

impl LinkPreview {
	fn scan(&mut self, source: &str, start: usize) {
		for (index, byte) in source.bytes().enumerate().skip(start) {
			if self.destination.is_none() {
				if byte == b'`' && (self.code_ticks > 0 || !self.escaped) {
					self.pending_ticks += 1;
					self.closed_label = false;
					continue;
				}
				if self.pending_ticks > 0 {
					if self.code_ticks == 0 {
						self.code_ticks = self.pending_ticks;
					} else if self.code_ticks == self.pending_ticks {
						self.code_ticks = 0;
					}
					self.pending_ticks = 0;
				}
				if self.code_ticks > 0 {
					continue;
				}
			}
			if self.escaped {
				self.escaped = false;
				self.closed_label = false;
				self.whitespace = false;
				continue;
			}
			if byte == b'\\' {
				self.escaped = true;
				self.closed_label = false;
				continue;
			}
			if let Some((destination_start, depth)) = self.destination.as_mut() {
				if let Some(delimiter) = self.delimiter {
					if byte == delimiter {
						self.delimiter = None;
					}
				} else if byte == b'<'
					&& *depth == 1
					&& (index == *destination_start + 1 || self.whitespace)
				{
					self.delimiter = Some(b'>');
				} else if matches!(byte, b'\'' | b'"') && self.whitespace && *depth == 1 {
					self.delimiter = Some(byte);
				} else if byte == b'(' {
					*depth += 1;
				} else if byte == b')' {
					*depth -= 1;
					if *depth == 0 {
						self.destination = None;
					}
				}
				self.whitespace = byte.is_ascii_whitespace();
			} else {
				match byte {
					b'[' => self.brackets += 1,
					b']' if self.brackets > 0 => {
						self.brackets -= 1;
						self.closed_label = true;
						continue;
					},
					b'(' if self.closed_label => {
						self.destination = Some((index, 1));
						self.whitespace = false;
					},
					_ => {},
				}
				self.closed_label = false;
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::visible;

	#[test]
	fn streaming_links_hold_destinations_until_closed_and_preserve_final_source() {
		for destination in [
			"https://example.com/a_(nested_(path))",
			r"https://example.com/escaped\)parenthesis",
			"<https://example.com/unbalanced)parenthesis>",
			"https://example.com/guide \"a title with ) parentheses\"",
		] {
			let source = format!("See [documentation]({destination}");
			for (end, _) in
				source.char_indices().filter(|(end, _)| *end > "See [documentation]".len())
			{
				assert_eq!(visible(&source[..end], false), "See [documentation]");
			}
			assert_eq!(visible(&source, false), "See [documentation]");
			assert_eq!(visible(&source, true), source);
			let closed = format!("{source}) and continue.");
			assert_eq!(visible(&closed, false), closed);
			let committed = format!("{source}\n");
			assert_eq!(visible(&committed, false), committed);
		}
		let long = format!("See [label](https://example.com/{}", "界".repeat(3_000));
		assert_eq!(visible(&long, false), "See [label]");
	}

	#[test]
	fn streaming_links_leave_code_escapes_and_committed_lines_intact() {
		for source in [
			r"See \[label](https://example.com/unfinished",
			r"See [label\](https://example.com/unfinished",
			"See `[label](https://example.com/unfinished`",
			"See ``[la`bel](https://example.com/unfinished``",
			"```md\n[label](https://example.com/unfinished",
			"    [label](https://example.com/unfinished",
		] {
			assert_eq!(visible(source, false), source);
		}
		let source = "[first](unfinished\nSee [second](https://example.com";
		assert_eq!(visible(source, false), "[first](unfinished\nSee [second]");
	}
}
