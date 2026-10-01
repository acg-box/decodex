//! Native diagram presentation with source copy and complete-source fallback.
use ui_theme::{BLUE, TEXT_MUTED};

#[cfg(test)] use crate::shell::agent_surface::markdown::{Context, Kind, Window, parse};
use crate::shell::agent_surface::{
	markdown::{
		self, AnyElement, HighlightStyle, Inline, InteractiveElement, IntoElement, Node,
		ParentElement, Range, SharedString, StatefulInteractiveElement, Styled, code_text, mermaid,
		mermaid::Role, px, ui_theme,
	},
	selectable_text::SelectableText,
};

// Adapted from the upstream closing-fence check; see agent_mermaid/NOTICE.md.
pub(super) fn has_closing_fence(input: &str, range: Range<usize>, content_end: usize) -> bool {
	let Some(block) = input.get(range.clone()) else { return false };
	let Some(marker @ (b'`' | b'~')) = block.as_bytes().first().copied() else { return false };
	let opening_len = block.bytes().take_while(|byte| *byte == marker).count();
	let Some(suffix) = input.get(content_end..range.end) else { return false };

	suffix
		.trim_end_matches([' ', '\t', '\r', '\n'])
		.bytes()
		.rev()
		.take_while(|byte| *byte == marker)
		.count()
		>= opening_len
}

pub(super) fn render(children: &[Node], key: &str) -> Option<AnyElement> {
	let source = code_text(children);
	let out = diagram(&source)?;
	let selector = format!("mermaid-{key}");

	Some(
		markdown::div()
			.flex()
			.flex_col()
			.gap_2()
			.min_w_0()
			.w_full()
			.p_3()
			.rounded_md()
			.bg(markdown::rgba(0x00000045))
			.font_family("Menlo")
			.text_size(px(12.))
			.line_height(px(19.))
			.child(markdown::copy_button(
				&format!("mermaid-copy-{key}"),
				"Copy Mermaid source",
				source,
			))
			.child(
				markdown::div()
					.id(SharedString::from(selector.clone()))
					.debug_selector(move || selector.clone())
					.w_full()
					.min_w_0()
					.flex()
					.flex_col()
					.items_start()
					.overflow_x_scroll()
					.whitespace_nowrap()
					.child(SelectableText {
						key: format!("mermaid-text-{key}"),
						text: out.text,
						highlights: out.highlights,
						links: Vec::new(),
					}),
			)
			.into_any_element(),
	)
}

fn diagram(source: &str) -> Option<Inline> {
	// The desktop scrolls wide diagrams; preserve bounded layout instead of wrapping edges.
	let lines = mermaid::render_spans(source, 256).ok()?;
	let mut out = Inline::default();

	for (index, line) in lines.into_iter().enumerate() {
		if index != 0 {
			out.text.push('\n');
		}

		for span in line {
			let start = out.text.len();

			out.text.push_str(&span.text);

			let color = match span.role {
				Role::Node => Some(markdown::rgb(BLUE).into()),
				Role::Edge => Some(markdown::rgb(TEXT_MUTED).into()),
				Role::Text => None,
			};

			out.highlights
				.push((start..out.text.len(), HighlightStyle { color, ..Default::default() }));
		}
	}

	Some(out)
}

#[cfg(test)]
#[path = "agent_mermaid_tests.rs"]
mod tests;
