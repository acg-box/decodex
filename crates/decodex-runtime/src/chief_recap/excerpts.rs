//! Preserve whole recent exchanges before excerpting both ends of long fields.
const OMITTED_HISTORY: &str = "[Earlier exchanges omitted]\n\n";
const EXCERPT_MARKER: &str = "\n[... excerpted ...]\n";
pub(super) fn render(exchanges: &[Exchange]) -> String {
	let Some(latest) = exchanges.last() else {
		return String::new();
	};
	let blocks = exchanges
		.iter()
		.map(|exchange| {
			exchange
				.fields()
				.map(|(label, text)| format!("{label}: {text}"))
				.collect::<Vec<_>>()
				.join("\n\n")
		})
		.collect::<Vec<_>>();
	let mut bytes = blocks.iter().map(String::len).sum::<usize>() + 2 * (blocks.len() - 1);
	if bytes <= super::prompt::HISTORY_MAX_BYTES {
		return blocks.join("\n\n");
	}

	// Keep the newest answer and any newer unanswered correction together.
	let retained = if latest.assistant.is_empty() { 2 } else { 1 };
	let oldest_retained = exchanges.len().saturating_sub(retained);
	let mut start = 0;
	while bytes > super::prompt::HISTORY_MAX_BYTES - OMITTED_HISTORY.len()
		&& start < oldest_retained
	{
		bytes -= blocks[start].len() + 2;
		start += 1;
	}
	let omission = if start > 0 { OMITTED_HISTORY } else { "" };
	let budget = super::prompt::HISTORY_MAX_BYTES - omission.len();
	if bytes <= budget {
		return format!("{omission}{}", blocks[start..].join("\n\n"));
	}

	let fields = exchanges[start..].iter().flat_map(Exchange::fields).collect::<Vec<_>>();
	let field_count = fields.len();
	let overhead =
		fields.iter().map(|(label, _)| label.len() + 2).sum::<usize>() + 2 * (field_count - 1);
	let mut remaining = budget.saturating_sub(overhead);
	let excerpts = fields
		.iter()
		.enumerate()
		.map(|(index, (label, text))| {
			let share = remaining / (field_count - index);
			// Reserve a share for later fields, without wasting space on short replies.
			let reserved =
				fields[index + 1..].iter().map(|(_, text)| text.len().min(share)).sum::<usize>();
			let excerpt = excerpt(text, remaining - reserved);
			remaining -= excerpt.len();
			format!("{label}: {excerpt}")
		})
		.collect::<Vec<_>>()
		.join("\n\n");
	format!("{omission}{excerpts}")
}

#[derive(Default)]
pub(super) struct Exchange {
	pub user: String,
	pub assistant: String,
}

impl Exchange {
	fn fields(&self) -> impl Iterator<Item = (&'static str, &str)> {
		let user_label = if self.assistant.is_empty() { "Pending user request" } else { "User" };
		[(user_label, self.user.as_str()), ("Assistant", self.assistant.as_str())]
			.into_iter()
			.filter(|(_, text)| !text.is_empty())
	}
}

fn excerpt(text: &str, max_bytes: usize) -> String {
	if text.len() <= max_bytes {
		return text.to_owned();
	}
	let Some(content_bytes) = max_bytes.checked_sub(EXCERPT_MARKER.len()) else {
		return text[..text.floor_char_boundary(max_bytes)].to_owned();
	};
	let head = text.floor_char_boundary(content_bytes / 2);
	let tail = text.ceil_char_boundary(text.len() - (content_bytes - content_bytes / 2));
	format!("{}{EXCERPT_MARKER}{}", &text[..head], &text[tail..])
}
