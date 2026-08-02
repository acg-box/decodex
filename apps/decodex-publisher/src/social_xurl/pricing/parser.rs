use std::collections::BTreeMap;

use sha2::{Digest as _, Sha256};

use super::{
	DIAGNOSTIC_SCHEMA, MAX_SOURCE_BYTES, PARSER_CONTRACT, XPricingDiagnostic, XPricingRates,
};

const TARGET_SECTION_HEADING: &str = "## Credit consumption details";
const TARGET_SECTION_UNIT_STATEMENT: &str = "All prices are per resource fetched (reads) or per request (writes/actions). [Purchase credits](https://console.x.com) in the Developer Console.";
const READ_HEADING: &str = "### Read operations";
const WRITE_HEADING: &str = "### Write operations";
const READ_DESCRIPTION: &str = "Charged per resource returned in the response.";
const WRITE_DESCRIPTION: &str = "Charged per request.";
const READ_LABELS: [(&str, &str); 2] = [("Posts: Read", "post_read"), ("User: Read", "user_read")];
const WRITE_LABELS: [(&str, &str); 2] =
	[("Post: Create", "post_create"), ("Post: Create (with URL)", "post_create_with_url")];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PricingParseFailure {
	code: &'static str,
}

impl PricingParseFailure {
	fn new(code: &'static str) -> Self {
		Self { code }
	}

	pub(super) fn code(self) -> &'static str {
		self.code
	}
}

struct VisibleMarkdown {
	lines: Vec<Option<String>>,
	code_fence_count: u64,
}

pub(super) fn parse(raw: &[u8]) -> Result<XPricingRates, PricingParseFailure> {
	if raw.is_empty() {
		return Err(PricingParseFailure::new("x_pricing_source_empty"));
	}
	if raw.len() > MAX_SOURCE_BYTES as usize {
		return Err(PricingParseFailure::new("x_pricing_source_oversize"));
	}
	let text = std::str::from_utf8(raw)
		.map_err(|_| PricingParseFailure::new("x_pricing_source_encoding_invalid"))?;
	let visible = visible_markdown_lines(text);
	let target_indices = target_indices(&visible.lines);
	if target_indices.is_empty() {
		return Err(PricingParseFailure::new("x_pricing_target_section_missing"));
	}
	if target_indices.len() != 1 {
		return Err(PricingParseFailure::new("x_pricing_target_section_duplicate"));
	}
	let section_start = target_indices[0] + 1;
	let section_end = next_h2_index(&visible.lines, section_start);
	let section = &visible.lines[section_start..section_end];
	if section.iter().any(Option::is_none) {
		return Err(PricingParseFailure::new("x_pricing_target_section_fenced"));
	}
	let section_lines = section.iter().filter_map(Option::as_deref).collect::<Vec<_>>();
	if exact_line_indices(&section_lines, TARGET_SECTION_UNIT_STATEMENT).len() != 1 {
		return Err(PricingParseFailure::new("x_pricing_section_unit_statement_invalid"));
	}

	let read_indices = exact_line_indices(&section_lines, READ_HEADING);
	let write_indices = exact_line_indices(&section_lines, WRITE_HEADING);
	if read_indices.len() != 1 || write_indices.len() != 1 {
		return Err(PricingParseFailure::new("x_pricing_operation_sections_invalid"));
	}
	let read_index = read_indices[0];
	let write_index = write_indices[0];
	let h3_indices = section_lines
		.iter()
		.enumerate()
		.filter_map(|(index, line)| is_h3(line.trim()).then_some(index))
		.collect::<Vec<_>>();
	let read_position = h3_indices
		.iter()
		.position(|index| *index == read_index)
		.ok_or_else(|| PricingParseFailure::new("x_pricing_operation_sections_invalid"))?;
	if write_index <= read_index || h3_indices.get(read_position + 1) != Some(&write_index) {
		return Err(PricingParseFailure::new("x_pricing_operation_sections_not_adjacent"));
	}
	let next_write_h3 = h3_indices
		.iter()
		.copied()
		.find(|index| *index > write_index)
		.unwrap_or(section_lines.len());
	let mut rates = parse_operation_table(
		&section_lines[read_index + 1..write_index],
		READ_DESCRIPTION,
		["Resource", "Unit cost"],
		&READ_LABELS,
		"resource",
	)?;
	rates.extend(parse_operation_table(
		&section_lines[write_index + 1..next_write_h3],
		WRITE_DESCRIPTION,
		["Action", "Unit cost"],
		&WRITE_LABELS,
		"request",
	)?);
	require_unique_target_labels(&section_lines)?;
	for key in ["post_create", "post_create_with_url", "post_read", "user_read"] {
		if !rates.contains_key(key) {
			return Err(PricingParseFailure::new("x_pricing_rows_missing"));
		}
	}

	Ok(XPricingRates {
		post_create: rates["post_create"],
		post_create_with_url: rates["post_create_with_url"],
		post_read: rates["post_read"],
		user_read: rates["user_read"],
	})
}

pub(super) fn diagnostic(raw: &[u8], error_code: &str) -> XPricingDiagnostic {
	let text = String::from_utf8_lossy(raw);
	let visible = visible_markdown_lines(&text);
	let indices = target_indices(&visible.lines);
	let target_section = indices.first().map_or_else(Vec::new, |index| {
		let start = index + 1;
		let end = next_h2_index(&visible.lines, start);
		visible.lines[start..end].iter().filter_map(Option::as_deref).collect::<Vec<_>>()
	});
	let target_section_sha256 = (!target_section.is_empty()).then(|| {
		Sha256::digest(target_section.join("\n").as_bytes())
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect()
	});

	XPricingDiagnostic {
		schema: DIAGNOSTIC_SCHEMA.into(),
		parser_contract: PARSER_CONTRACT.into(),
		error_code: error_code.into(),
		raw_sha256: Sha256::digest(raw).iter().map(|byte| format!("{byte:02x}")).collect(),
		source_bytes: raw.len() as u64,
		source_lines: text.lines().count() as u64,
		code_fence_count: visible.code_fence_count,
		target_section_count: indices.len() as u64,
		target_section_lines: target_section.len() as u64,
		target_section_sha256,
		tables: Vec::new(),
	}
}

fn parse_operation_table(
	lines: &[&str],
	description: &str,
	header: [&str; 2],
	labels: &[(&'static str, &'static str)],
	expected_unit: &str,
) -> Result<BTreeMap<&'static str, u64>, PricingParseFailure> {
	if lines.iter().filter(|line| line.trim() == description).count() != 1 {
		return Err(PricingParseFailure::new("x_pricing_operation_unit_statement_invalid"));
	}
	let blocks = table_blocks(lines);
	if blocks.len() != 1 {
		return Err(PricingParseFailure::new("x_pricing_operation_table_count_invalid"));
	}
	let rows = blocks[0].iter().map(|line| table_cells(line)).collect::<Vec<_>>();
	if rows.len() < 3
		|| rows[0] != header
		|| rows[1].len() != 2
		|| rows[1].iter().any(|cell| !table_separator(cell))
	{
		return Err(PricingParseFailure::new("x_pricing_operation_table_header_invalid"));
	}
	let mut rates = BTreeMap::new();
	for cells in &rows[2..] {
		if cells.len() != 2 || cells.iter().any(|cell| cell.is_empty()) {
			return Err(PricingParseFailure::new("x_pricing_operation_row_invalid"));
		}
		let label = plain_markdown_cell(cells[0]);
		if let Some((_, key)) = labels.iter().find(|(expected, _)| *expected == label) {
			if cells[0] != format!("**{label}**") {
				return Err(PricingParseFailure::new("x_pricing_operation_label_markup_invalid"));
			}
			if rates.insert(*key, parse_micro_usd(cells[1], expected_unit)?).is_some() {
				return Err(PricingParseFailure::new("x_pricing_row_duplicate"));
			}
		} else {
			parse_micro_usd(cells[1], expected_unit)?;
		}
	}
	if labels.iter().any(|(_, key)| !rates.contains_key(key)) {
		return Err(PricingParseFailure::new("x_pricing_rows_missing"));
	}
	Ok(rates)
}

fn require_unique_target_labels(lines: &[&str]) -> Result<(), PricingParseFailure> {
	let mut counts = READ_LABELS
		.iter()
		.chain(WRITE_LABELS.iter())
		.map(|(label, _)| (*label, 0_u64))
		.collect::<BTreeMap<_, _>>();
	for line in lines.iter().filter(|line| is_table_line(line)) {
		for cell in table_cells(line) {
			if let Some(count) = counts.get_mut(plain_markdown_cell(cell)) {
				*count += 1;
			}
		}
	}
	if counts.values().any(|count| *count != 1) {
		return Err(PricingParseFailure::new("x_pricing_row_duplicate"));
	}
	Ok(())
}

fn parse_micro_usd(value: &str, expected_unit: &str) -> Result<u64, PricingParseFailure> {
	let value = value
		.strip_prefix("\\$")
		.or_else(|| value.strip_prefix('$'))
		.ok_or_else(|| PricingParseFailure::new("x_pricing_value_ambiguous"))?;
	let (amount, unit) = value
		.split_once(" per ")
		.ok_or_else(|| PricingParseFailure::new("x_pricing_value_ambiguous"))?;
	if unit != expected_unit {
		return Err(PricingParseFailure::new("x_pricing_value_unit_invalid"));
	}
	let mut pieces = amount.split('.');
	let whole = pieces.next().unwrap_or_default();
	let fractional = pieces.next();
	if pieces.next().is_some()
		|| whole.is_empty()
		|| !whole.bytes().all(|byte| byte.is_ascii_digit())
		|| fractional.is_some_and(|value| {
			value.is_empty() || value.len() > 6 || !value.bytes().all(|byte| byte.is_ascii_digit())
		}) {
		return Err(PricingParseFailure::new("x_pricing_value_ambiguous"));
	}
	let whole =
		whole.parse::<u64>().map_err(|_| PricingParseFailure::new("x_pricing_value_ambiguous"))?;
	let fraction = fractional.unwrap_or_default();
	let fraction = if fraction.is_empty() {
		0
	} else {
		format!("{fraction:0<6}")
			.parse::<u64>()
			.map_err(|_| PricingParseFailure::new("x_pricing_value_ambiguous"))?
	};
	let amount = whole
		.checked_mul(1_000_000)
		.and_then(|whole| whole.checked_add(fraction))
		.ok_or_else(|| PricingParseFailure::new("x_pricing_value_out_of_range"))?;
	if amount == 0 || amount > 10_000_000 {
		return Err(PricingParseFailure::new("x_pricing_value_out_of_range"));
	}
	Ok(amount)
}

fn visible_markdown_lines(text: &str) -> VisibleMarkdown {
	let mut lines = Vec::new();
	let mut fence_character = None;
	let mut fence_length = 0;
	let mut code_fence_count = 0;
	for line in text.lines() {
		let stripped = line.trim_start();
		let marker = fence_marker(stripped);
		if let Some((character, length)) = marker {
			if fence_character.is_none() {
				fence_character = Some(character);
				fence_length = length;
				code_fence_count += 1;
				lines.push(None);
				continue;
			}
			if fence_character == Some(character) && length >= fence_length {
				fence_character = None;
				fence_length = 0;
				lines.push(None);
				continue;
			}
		}
		lines.push(fence_character.is_none().then(|| line.to_owned()));
	}
	VisibleMarkdown { lines, code_fence_count }
}

fn fence_marker(value: &str) -> Option<(u8, usize)> {
	let character = *value.as_bytes().first()?;
	if !matches!(character, b'`' | b'~') {
		return None;
	}
	let length = value.bytes().take_while(|byte| *byte == character).count();
	(length >= 3).then_some((character, length))
}

fn target_indices(lines: &[Option<String>]) -> Vec<usize> {
	lines
		.iter()
		.enumerate()
		.filter_map(|(index, line)| {
			line.as_deref()
				.is_some_and(|line| line.trim() == TARGET_SECTION_HEADING)
				.then_some(index)
		})
		.collect()
}

fn next_h2_index(lines: &[Option<String>], start: usize) -> usize {
	(start..lines.len())
		.find(|index| lines[*index].as_deref().is_some_and(|line| is_h2(line.trim())))
		.unwrap_or(lines.len())
}

fn is_h2(value: &str) -> bool {
	heading_level(value) == Some(2)
}

fn is_h3(value: &str) -> bool {
	heading_level(value) == Some(3)
}

fn heading_level(value: &str) -> Option<usize> {
	let level = value.bytes().take_while(|byte| *byte == b'#').count();
	(level > 0 && value.as_bytes().get(level).is_none_or(|byte| byte.is_ascii_whitespace()))
		.then_some(level)
}

fn exact_line_indices(lines: &[&str], expected: &str) -> Vec<usize> {
	lines
		.iter()
		.enumerate()
		.filter_map(|(index, line)| (line.trim() == expected).then_some(index))
		.collect()
}

fn table_blocks<'a>(lines: &'a [&'a str]) -> Vec<Vec<&'a str>> {
	let mut blocks = Vec::new();
	let mut current = Vec::new();
	for line in lines {
		if is_table_line(line) {
			current.push(*line);
		} else if !current.is_empty() {
			blocks.push(std::mem::take(&mut current));
		}
	}
	if !current.is_empty() {
		blocks.push(current);
	}
	blocks
}

fn is_table_line(line: &str) -> bool {
	let line = line.trim();
	line.starts_with('|') && line.ends_with('|')
}

fn table_cells(line: &str) -> Vec<&str> {
	let line = line.trim();
	if !is_table_line(line) {
		return Vec::new();
	}
	line[1..line.len() - 1].split('|').map(str::trim).collect()
}

fn table_separator(value: &str) -> bool {
	let value = value.strip_prefix(':').unwrap_or(value);
	let value = value.strip_suffix(':').unwrap_or(value);
	value.len() >= 3 && value.bytes().all(|byte| byte == b'-')
}

fn plain_markdown_cell(value: &str) -> &str {
	value
		.strip_prefix("**")
		.and_then(|value| value.strip_suffix("**"))
		.map(str::trim)
		.unwrap_or(value)
}
