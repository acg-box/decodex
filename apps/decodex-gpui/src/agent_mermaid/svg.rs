//! Resolve Ondine's presentation variables for GPUI's SVG 1.1 renderer, without changing labels.
// Variable resolution follows Ondine's terminal adapter; see NOTICE.md and LICENSE-ONDINE.
use std::{collections::HashMap, ops::Range};

use crate::ui_theme::{ACCENT, AMBER, BLUE, CANVAS, ERROR, GREEN, TEXT, TEXT_MUTED};

pub(super) fn desktop(svg: &str) -> Result<String, String> {
	let document = roxmltree::Document::parse(svg).map_err(|e| e.to_string())?;
	let style = document.root_element().attribute("style").unwrap_or_default();
	let mut variables = HashMap::new();

	for declaration in declarations(style) {
		if let Some((name, value)) = declaration.split_once(':')
			&& name.trim().starts_with("--")
		{
			variables.insert(name.trim().to_owned(), value.trim().to_owned());
		}
	}

	let tint =
		|color, opacity| u32::from(gpui::rgb(CANVAS).blend(gpui::rgb(color).alpha(opacity))) >> 8;
	for (name, color) in [
		("bg", CANVAS),
		("fg", TEXT),
		("muted", TEXT_MUTED),
		("line", tint(TEXT_MUTED, 0.8)),
		("border", tint(TEXT_MUTED, 0.72)),
		("accent", BLUE),
		("surface", tint(BLUE, 0.08)),
		// This fill marks excluded periods and completed work, so it must remain visible.
		("surface-alt", tint(TEXT_MUTED, 0.2)),
		("critical", ERROR),
	] {
		variables.insert(format!("--ondine-{name}"), format!("#{color:06x}"));
	}
	for (index, color) in
		[BLUE, GREEN, AMBER, ACCENT, ERROR, TEXT_MUTED, 0xc4a7e7, 0x7dc4cf].into_iter().enumerate()
	{
		variables.insert(format!("--ondine-series-{}", index + 1), format!("#{color:06x}"));
	}
	variables.insert("--ondine-font".into(), "Menlo, monospace".into());
	for (name, value) in [
		("stroke", "1.25px"),
		("radius", "6px"),
		("title-weight", "600"),
		("heading-weight", "600"),
		("bold-weight", "600"),
	] {
		variables.insert(format!("--ondine-{name}"), value.into());
	}

	let mut replacements = Vec::new();
	for node in document.descendants().filter(roxmltree::Node::is_element) {
		let corners = rect_corners(node, &variables)?;
		for attribute in node.attributes() {
			let value = match attribute.name() {
				"style" => declarations(attribute.value())
					.into_iter()
					.filter(|declaration| !declaration.trim_start().starts_with("--"))
					.filter(|declaration| {
						!declaration.split_once(':').is_some_and(|(name, _)| {
							corners.iter().any(|(corner, _)| *corner == name.trim())
						})
					})
					.map(|declaration| resolve(declaration, &variables, 0))
					.collect::<Result<Vec<_>, _>>()?
					.join(";"),
				"rx" | "ry" => corners
					.iter()
					.find(|(name, _)| *name == attribute.name())
					.map(|(_, value)| Ok(value.clone()))
					.unwrap_or_else(|| resolve(attribute.value(), &variables, 0))?,
				"fill" | "stroke" | "color" | "font-family" | "font-weight" | "font-size"
				| "stroke-width" | "stroke-dasharray" | "opacity" | "fill-opacity"
				| "stroke-opacity" | "stop-color" | "stop-opacity" =>
					resolve(attribute.value(), &variables, 0)?,
				_ => continue,
			};
			if value != attribute.value() {
				replacements.push((attribute.range_value(), escape_attribute(&value)));
			}
		}
		insert_corners(node, corners, &mut replacements);
	}
	replacements.sort_unstable_by_key(|(range, _)| range.start);

	let mut output = String::with_capacity(svg.len());
	let mut cursor = 0;
	for (range, value) in replacements {
		output.push_str(&svg[cursor..range.start]);
		output.push_str(&value);
		cursor = range.end;
	}
	output.push_str(&svg[cursor..]);

	Ok(output)
}

// usvg supports SVG 1.1 geometry attributes, but ignores CSS rx/ry emitted by Ondine.
fn rect_corners(
	node: roxmltree::Node<'_, '_>,
	variables: &HashMap<String, String>,
) -> Result<Vec<(&'static str, String)>, String> {
	let mut corners = Vec::new();
	if node.has_tag_name("rect") {
		let style = declarations(node.attribute("style").unwrap_or_default());
		for name in ["rx", "ry"] {
			if let Some((_, value)) = style
				.iter()
				.filter_map(|declaration| declaration.split_once(':'))
				.rfind(|(property, _)| property.trim() == name)
			{
				corners.push((name, resolve(value.trim(), variables, 0)?));
			}
		}
	}
	Ok(corners)
}

fn insert_corners(
	node: roxmltree::Node<'_, '_>,
	corners: Vec<(&str, String)>,
	replacements: &mut Vec<(Range<usize>, String)>,
) {
	let mut attributes = String::new();
	for (name, value) in corners {
		if !node.has_attribute(name) {
			attributes.push_str(&format!(" {name}=\"{}\"", escape_attribute(&value)));
		}
	}
	if !attributes.is_empty() {
		let after_name = node.range().start + "<rect".len();
		replacements.push((after_name..after_name, attributes));
	}
}

fn declarations(style: &str) -> Vec<&str> {
	let (mut depth, mut start) = (0, 0);
	let mut parts = Vec::new();
	for (index, ch) in style.char_indices() {
		match ch {
			'(' => depth += 1,
			')' => depth -= 1,
			';' if depth == 0 => {
				parts.push(&style[start..index]);
				start = index + 1;
			},
			_ => {},
		}
	}
	parts.push(&style[start..]);
	parts
}

fn resolve(
	text: &str,
	variables: &HashMap<String, String>,
	depth: usize,
) -> Result<String, String> {
	if depth > 8 {
		return Err("SVG theme contains a variable cycle".into());
	}
	let mut output = String::new();
	let mut rest = text;
	while let Some(start) = rest.find("var(") {
		output.push_str(&rest[..start]);
		let after = &rest[start + 4..];
		let mut nesting = 1;
		let end = after
			.char_indices()
			.find_map(|(index, ch)| {
				if ch == '(' {
					nesting += 1;
				}
				if ch == ')' {
					nesting -= 1;
				}
				(nesting == 0).then_some(index)
			})
			.ok_or("Unclosed SVG theme variable")?;
		let (name, fallback) = after[..end]
			.split_once(',')
			.map(|(name, value)| (name, Some(value)))
			.unwrap_or((&after[..end], None));
		let value = variables
			.get(name.trim())
			.map(String::as_str)
			.or(fallback)
			.ok_or("Unknown SVG theme variable")?;
		output.push_str(&resolve(value, variables, depth + 1)?);
		rest = &after[end + 1..];
	}
	output.push_str(rest);
	mix_colors(&output)
}

// Ondine uses hex sRGB mixes for cluster-title halos. Resolve them before resvg sees them.
fn mix_colors(text: &str) -> Result<String, String> {
	const PREFIX: &str = "color-mix(in srgb,";
	let mut output = String::new();
	let mut rest = text;
	while let Some(start) = rest.find(PREFIX) {
		output.push_str(&rest[..start]);
		let after = &rest[start + PREFIX.len()..];
		let end = after.find(')').ok_or("Unclosed SVG color mix")?;
		let (first, second) = after[..end].split_once(',').ok_or("Invalid SVG color mix")?;
		let (color, share) = first.trim().rsplit_once(' ').ok_or("Missing SVG color share")?;
		let weight = share
			.strip_suffix('%')
			.ok_or("Invalid SVG color share")?
			.parse::<f32>()
			.map_err(|e| e.to_string())?
			/ 100.;
		let first = gpui::Rgba::try_from(color).map_err(|e| e.to_string())?;
		let second = gpui::Rgba::try_from(second).map_err(|e| e.to_string())?;
		let mixed = u32::from(second.blend(first.alpha(weight))) >> 8;
		output.push_str(&format!("#{mixed:06x}"));
		rest = &after[end + 1..];
	}
	output.push_str(rest);
	Ok(output)
}

fn escape_attribute(value: &str) -> String {
	value
		.replace('&', "&amp;")
		.replace('"', "&quot;")
		.replace('\'', "&apos;")
		.replace('<', "&lt;")
		.replace('>', "&gt;")
}
