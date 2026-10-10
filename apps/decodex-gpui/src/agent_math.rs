// Adapted from openai/codex at 595cc91e8cbb1c2ca822d0311dcf12709410c582.
// Copyright OpenAI. Licensed under Apache-2.0; see LICENSE-APACHE.
// Decodex changes: module paths and iterator adaptation for the desktop renderer.
//! Recognize math before Markdown consumes TeX escapes, retaining exact source offsets.
//!
//! Only the rendering copy is masked. Unsupported expressions are restored verbatim; code,
//! links, and HTML stay under the ordinary Markdown renderer. Standalone display tracking outlives
//! the conversion budget; rejected prose-prefixed openers have bounded pairing lookahead.

#[path = "agent_math/render.rs"] mod render;

use std::{borrow::Cow, iter::Peekable, ops::Range, slice::Iter, vec::IntoIter};

use pulldown_cmark::{Event, Options, Parser, Tag};

const MAX_MATH_BYTES: usize = 4_096;

pub(super) struct MathMarkdown<'a> {
	pub(super) markdown: Cow<'a, str>,
	pub(super) pending_start: Option<usize>,
	pub(super) display_ranges: Vec<Range<usize>>,
	replacements: Vec<(Range<usize>, String)>,
}
impl<'a> MathMarkdown<'a> {
	pub(super) fn new(input: &'a str, options: Options, width: Option<usize>) -> Self {
		let mut result = Self {
			markdown: Cow::Borrowed(input),
			pending_start: None,
			display_ranges: Vec::new(),
			replacements: Vec::new(),
		};

		if !input.contains('$') && !input.contains("\\(") && !input.contains("\\[") {
			return result;
		}

		let (protected, containers) = protected_ranges(input, options);
		let mut protected = protected.iter().peekable();
		let mut containers = containers.into_iter().peekable();
		let mut cursor = MathCursor::default();

		while cursor.offset < input.len() {
			let Some((start, open, close, display)) =
				cursor.next_opener(input, &mut protected, &mut containers)
			else {
				break;
			};
			let body = &input[cursor.offset..];
			let limit = conversion_limit(body);
			let rejected_display = display && cursor.line_has_text;
			// Only standalone displays can retain an arbitrarily distant closer.
			let search = if display && !rejected_display { body } else { &body[..limit] };
			let (end, rejected_close) = find_close(
				input,
				search,
				close,
				cursor.offset,
				display,
				rejected_display,
				protected.clone(),
			);

			if display && (!rejected_display || end.is_some() || body.len() < MAX_MATH_BYTES) {
				result.display_ranges.push(start..end.map_or(input.len(), |end| end + close.len()));
			}
			// Retain rejected pairing only within the lookahead window, so shell PID dollars
			// cannot keep the entire streamed response mutable in the rendering cache.
			if rejected_display || rejected_close {
				let Some(next) =
					next_after_rejected(input, cursor.offset, end, open, close, rejected_display)
				else {
					break;
				};

				cursor.offset = next;

				continue;
			}

			let Some(end) = end else {
				if display {
					if body.len() < MAX_MATH_BYTES {
						result.pending_start.get_or_insert(cursor.line_start);
					}

					let span = start..input.len();

					result.markdown.to_mut().replace_range(span.clone(), &"$".repeat(span.len()));
					result.replacements.push((span, input[start..].to_owned()));

					break;
				}

				continue;
			};
			let span = start..end + close.len();
			let formula = &input[cursor.offset..end];

			// Every matched display owns its closer, including rejected expressions.
			if display {
				cursor.offset = span.end;
			}
			if protected.peek().is_some_and(|range| range.start < span.end) {
				continue;
			}
			if !display && formula.contains('\n') {
				continue;
			}
			if open == "$" {
				let next = input[span.end..].chars().next();

				if formula.ends_with(char::is_whitespace) || next.is_some_and(char::is_alphanumeric)
				{
					continue;
				}
				if currency_or_environment(formula) {
					cursor.offset = span.end;

					continue;
				}
			}

			let rendered = render_formula(
				input,
				&span,
				formula,
				display,
				containers.peek().is_some_and(|range| range.contains(&start)),
				width,
			);

			// Dollars are ordinary text in the Markdown parser and cannot form an HTML tag.
			result.markdown.to_mut().replace_range(span.clone(), &"$".repeat(span.len()));

			cursor.offset = span.end;

			result.replacements.push((span, rendered));
		}

		result
	}

	pub(super) fn span(&self, range: &Range<usize>) -> Option<bool> {
		let index =
			self.replacements.binary_search_by_key(&range.start, |(span, _)| span.start).ok()?;

		(self.replacements[index].0 == *range)
			.then(|| self.display_ranges.iter().any(|span| span == range))
	}

	pub(super) fn events<'s>(
		&'s self,
		events: impl Iterator<Item = (Event<'s>, Range<usize>)>,
	) -> impl Iterator<Item = (Event<'s>, Range<usize>)> {
		let mut replacements = self.replacements.iter().peekable();

		events.flat_map(move |(event, range)| {
			while replacements.next_if(|(span, _)| span.end <= range.start).is_some() {}

			let Event::Text(text) = event else {
				return vec![(event, range)].into_iter();
			};

			if replacements.peek().is_none_or(|(span, _)| span.start >= range.end) {
				return vec![(Event::Text(text), range)].into_iter();
			}

			let mut output = Vec::new();
			let mut offset = range.start;

			while let Some((span, text)) = replacements.next_if(|(span, _)| span.end <= range.end) {
				if offset < span.start {
					output.push((
						Event::Text(self.markdown[offset..span.start].into()),
						offset..span.start,
					));
				}

				output.push((Event::Text(text.as_str().into()), span.clone()));

				offset = span.end;
			}

			if offset < range.end {
				output.push((
					Event::Text(self.markdown[offset..range.end].into()),
					offset..range.end,
				));
			}

			output.into_iter()
		})
	}
}

#[derive(Default)]
struct MathCursor {
	offset: usize,
	scanned: usize,
	line_start: usize,
	line_has_text: bool,
}
impl MathCursor {
	fn next_opener(
		&mut self,
		input: &str,
		protected: &mut Peekable<Iter<'_, Range<usize>>>,
		containers: &mut Peekable<IntoIter<Range<usize>>>,
	) -> Option<(usize, &'static str, &'static str, bool)> {
		while self.offset < input.len() {
			if let Some(index) = input[self.scanned..self.offset].rfind('\n') {
				self.line_start = self.scanned + index + 1;
				self.line_has_text = false;
			}

			self.line_has_text |=
				!input[self.scanned.max(self.line_start)..self.offset].trim().is_empty();
			self.scanned = self.offset;

			while protected.next_if(|range| range.end <= self.offset).is_some() {}
			while containers.next_if(|range| range.end <= self.offset).is_some() {}

			if let Some(range) = protected.peek()
				&& range.contains(&self.offset)
			{
				self.offset = range.end;

				continue;
			}

			let rest = &input[self.offset..];
			let Some((open, close, display)) = delimiters(rest) else {
				self.offset += rest.chars().next().expect("nonempty remainder").len_utf8();

				continue;
			};
			let start = self.offset;

			self.offset += open.len();

			if escaped(input, start) {
				continue;
			}

			let body = &input[self.offset..];

			if open == "$"
				&& (body.starts_with(char::is_whitespace) || body.starts_with(['(', '{']))
			{
				continue;
			}

			return Some((start, open, close, display));
		}

		None
	}
}

fn escaped(input: &str, offset: usize) -> bool {
	input[..offset].bytes().rev().take_while(|byte| *byte == b'\\').count() % 2 == 1
}

fn protected_ranges(input: &str, options: Options) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
	let parser = Parser::new_ext(input, options);
	let mut protected: Vec<_> =
		parser.reference_definitions().iter().map(|(_, def)| def.span.clone()).collect();
	let mut containers = Vec::new();

	protected.extend(parser.into_offset_iter().filter_map(|(event, range)| {
		if matches!(event, Event::Start(Tag::List(_) | Tag::BlockQuote(_))) {
			containers.push(range.clone());
		}

		matches!(
			event,
			Event::Code(_)
				| Event::Html(_)
				| Event::InlineHtml(_)
				| Event::Start(Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. })
		)
		.then_some(range)
	}));
	protected.sort_unstable_by_key(|range| range.start);
	containers.sort_unstable_by_key(|range| range.start);

	(protected, containers)
}

fn render_formula(
	input: &str,
	span: &Range<usize>,
	formula: &str,
	display: bool,
	nested: bool,
	width: Option<usize>,
) -> String {
	let rendered =
		if formula.len() < MAX_MATH_BYTES { render::render(formula, display) } else { None };

	rendered
		.filter(|text| {
			// Nested Markdown prefixes have their own width; keep spatial layouts at the top level.
			// Never wrap a spatial layout into misleading pieces.
			!text.contains('\n')
				|| !nested
					&& width.is_none_or(|width| {
						text.lines().all(|line| {
							render::display_width(line) <= width.saturating_sub(/* rhs */ 4)
						})
					})
		})
		.unwrap_or_else(|| input[span.clone()].to_owned())
}

fn find_close<'a>(
	input: &str,
	search: &str,
	close: &str,
	offset: usize,
	display: bool,
	rejected_display: bool,
	protected: impl Iterator<Item = &'a Range<usize>>,
) -> (Option<usize>, bool) {
	let mut multiline_close = false;
	let mut closing_protected = protected.peekable();
	let mut rejected_close = false;
	let end = search.match_indices(&close[..1]).find_map(|(index, _)| {
		let end = offset + index;

		if !search[index..].starts_with(close) || escaped(input, end) {
			return None;
		}
		if display {
			while closing_protected.next_if(|range| range.end <= end).is_some() {}

			if closing_protected.peek().is_some_and(|range| range.start < end + close.len()) {
				return None;
			}
			if !rejected_display
				&& input[end + close.len()..]
					.chars()
					.take_while(|ch| *ch != '\n')
					.any(|ch| !ch.is_whitespace())
			{
				if multiline_close || input[offset..end].contains('\n') {
					multiline_close = true;

					return None;
				}

				rejected_close = true;
			}
		}

		Some(end)
	});

	(end, rejected_close)
}

fn delimiters(rest: &str) -> Option<(&'static str, &'static str, bool)> {
	[("$$", "$$", true), ("\\[", "\\]", true), ("\\(", "\\)", false), ("$", "$", false)]
		.into_iter()
		.find(|(open, _, _)| rest.starts_with(open))
}

fn currency_or_environment(formula: &str) -> bool {
	formula.starts_with(|ch: char| ch.is_ascii_digit())
		&& !formula.contains(['\\', '^', '_', '=', '+', '-', '*', '/', '<', '>'])
		|| formula.len() > 1 && formula.chars().all(|ch| ch.is_ascii_uppercase())
}

fn conversion_limit(body: &str) -> usize {
	body.char_indices()
		.map(|(index, _)| index)
		.find(|index| *index >= MAX_MATH_BYTES)
		.unwrap_or(body.len())
}

fn next_after_rejected(
	input: &str,
	offset: usize,
	end: Option<usize>,
	open: &str,
	close: &str,
	rejected_display: bool,
) -> Option<usize> {
	if let Some(end) = end
		&& !input[offset..end].trim().is_empty()
	{
		// Leave a later line's opening dollars available for its own equation.
		if rejected_display
			&& open == "$$"
			&& input[offset..end]
				.rsplit_once('\n')
				.is_some_and(|(_, prefix)| prefix.trim().is_empty())
			&& input[end + close.len()..]
				.chars()
				.take_while(|ch| *ch != '\n')
				.any(|ch| !ch.is_whitespace())
		{
			return Some(offset);
		}

		return Some(end + close.len());
	}

	if end.is_none() && open == "\\[" && input.len() - offset < MAX_MATH_BYTES {
		return None;
	}

	Some(offset)
}
