//! Native Markdown presentation; HTML is text and remote images are not fetched.
use super::*;
use gpui::{AnyElement, FontStyle, HighlightStyle, InteractiveText, StyledText};
use pulldown_cmark::{Event, Options, Parser, Tag};
use std::ops::Range;

#[derive(Clone, Debug)]
enum Kind {
	Paragraph,
	Heading(u8),
	List(Option<u64>),
	Item,
	Quote,
	Code,
	Table,
	Row(bool),
	Cell,
	Strong,
	Emphasis,
	Strike,
	InlineCode,
	Link(String),
	Group,
}
#[derive(Clone, Debug)]
enum Node {
	Text(String),
	Block(Kind, Vec<Self>),
	Rule,
}

fn tag_kind(tag: Tag<'_>) -> Kind {
	match tag {
		Tag::Paragraph => Kind::Paragraph,
		Tag::Heading { level, .. } => Kind::Heading(level as u8),
		Tag::List(start) => Kind::List(start),
		Tag::Item => Kind::Item,
		Tag::BlockQuote => Kind::Quote,
		Tag::CodeBlock(_) => Kind::Code,
		Tag::Table(_) => Kind::Table,
		Tag::TableHead => Kind::Row(true),
		Tag::TableRow => Kind::Row(false),
		Tag::TableCell => Kind::Cell,
		Tag::Strong => Kind::Strong,
		Tag::Emphasis => Kind::Emphasis,
		Tag::Strikethrough => Kind::Strike,
		Tag::Link { dest_url, .. } => Kind::Link(dest_url.into_string()),
		_ => Kind::Group,
	}
}
fn parse(text: &str) -> Vec<Node> {
	let mut stack = vec![(Kind::Group, Vec::new())];
	let mut flattened = 0;
	for event in Parser::new_ext(
		text,
		Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
	) {
		match event {
			Event::Start(tag) =>
				if stack.len() < 64 && flattened == 0 {
					stack.push((tag_kind(tag), Vec::new()));
				} else {
					flattened += 1;
				},
			Event::End(_) =>
				if stack.len() > 1 {
					let (kind, children) = stack.pop().expect("open block");
					stack.last_mut().expect("root").1.push(Node::Block(kind, children));
				},
			event => {
				let node = match event {
					Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) =>
						Node::Text(text.into_string()),
					Event::Code(text) =>
						Node::Block(Kind::InlineCode, vec![Node::Text(text.into_string())]),
					Event::SoftBreak => Node::Text(" ".into()),
					Event::HardBreak => Node::Text("\n".into()),
					Event::Rule => Node::Rule,
					Event::TaskListMarker(done) =>
						Node::Text(if done { "☑ " } else { "☐ " }.into()),
					Event::FootnoteReference(text) => Node::Text(format!("[{text}]")),
					_ => continue,
				};
				stack.last_mut().expect("root").1.push(node);
			},
		}
	}
	stack.pop().expect("root").1
}
#[derive(Default)]
struct Inline {
	text: String,
	highlights: Vec<(Range<usize>, HighlightStyle)>,
	links: Vec<(Range<usize>, String)>,
}
fn append_inline(nodes: &[Node], style: HighlightStyle, link: Option<&str>, out: &mut Inline) {
	for node in nodes {
		match node {
			Node::Text(text) => {
				let start = out.text.len();
				out.text.push_str(text);
				let range = start..out.text.len();
				out.highlights.push((range.clone(), style));
				if let Some(link) = link {
					out.links.push((range, link.into()));
				}
			},
			Node::Block(kind, children) => {
				let mut next = style;
				match kind {
					Kind::Strong => next.font_weight = Some(FontWeight::BOLD),
					Kind::Emphasis => next.font_style = Some(FontStyle::Italic),
					Kind::InlineCode => next.background_color = Some(rgba(0xffffff12).into()),
					Kind::Link(_) => next.color = Some(rgb(ui_theme::BLUE).into()),
					Kind::Strike =>
						next.strikethrough =
							Some(gpui::StrikethroughStyle { thickness: px(1.0), color: None }),
					_ => {},
				}
				let target = if let Kind::Link(url) = kind { Some(url.as_str()) } else { link };
				append_inline(children, next, target, out);
			},
			Node::Rule => {},
		}
	}
}
fn inline(nodes: &[Node], key: &str) -> AnyElement {
	let mut out = Inline::default();
	append_inline(nodes, HighlightStyle::default(), None, &mut out);
	let ranges = out.links.iter().map(|(range, _)| range.clone()).collect();
	InteractiveText::new(
		SharedString::from(key.to_owned()),
		StyledText::new(out.text).with_highlights(out.highlights),
	)
	.on_click(ranges, move |index, _, cx| {
		let url = &out.links[index].1;
		if url.starts_with("https://")
			|| url.starts_with("http://")
			|| url.starts_with("codex://threads/")
		{
			cx.open_url(url);
		} else if url.starts_with('/') {
			let path = url
				.rsplit_once(':')
				.filter(|(_, line)| line.parse::<u32>().is_ok())
				.map_or(url.as_str(), |(path, _)| path);
			cx.reveal_path(std::path::Path::new(path));
		}
	})
	.into_any_element()
}
fn render_node(node: &Node, key: &str) -> AnyElement {
	let Node::Block(kind, children) = node else {
		return match node {
			Node::Rule => div().h(px(1.0)).my_2().bg(rgba(0xffffff18)).into_any_element(),
			_ => div().child(inline(std::slice::from_ref(node), key)).into_any_element(),
		};
	};
	match kind {
		Kind::Paragraph | Kind::Cell | Kind::Heading(_) => div()
			.when(matches!(kind, Kind::Cell), |d| d.flex_1().min_w_0().p_2())
			.when(matches!(kind, Kind::Heading(_)), |d| {
				d.font_weight(FontWeight::SEMIBOLD).mt_2().text_size(px(
					if let Kind::Heading(level) = kind {
						19.0 - (*level as f32) * 1.0
					} else {
						14.0
					},
				))
			})
			.child(inline(children, key))
			.into_any_element(),
		Kind::Code => div()
			.p_3()
			.rounded_md()
			.bg(rgba(0x00000045))
			.font_family("Menlo")
			.text_size(px(12.0))
			.line_height(px(19.0))
			.child(inline(children, key))
			.into_any_element(),
		Kind::List(start) => div()
			.flex()
			.flex_col()
			.gap_2()
			.children(children.iter().enumerate().map(|(index, child)| {
				div()
					.flex()
					.gap_2()
					.child(
						div()
							.w(px(24.0))
							.flex_shrink_0()
							.text_color(rgb(ui_theme::TEXT_MUTED))
							.child(start.map_or_else(
								|| "•".into(),
								|start| format!("{}.", start + index as u64),
							)),
					)
					.child(
						div()
							.flex_1()
							.min_w_0()
							.child(render_node(child, &format!("{key}-{index}"))),
					)
			}))
			.into_any_element(),
		Kind::Row(header) => div()
			.flex()
			.items_stretch()
			.border_b_1()
			.border_color(rgba(0xffffff12))
			.when(*header, |d| d.font_weight(FontWeight::SEMIBOLD).bg(rgba(0xffffff08)))
			.children(
				children.iter().enumerate().map(|(i, n)| render_node(n, &format!("{key}-{i}"))),
			)
			.into_any_element(),
		Kind::Item =>
			div().flex().flex_col().gap_2().children(render_item(children, key)).into_any_element(),
		_ => div()
			.flex()
			.flex_col()
			.gap_2()
			.when(matches!(kind, Kind::Quote), |d| {
				d.pl_3().border_l_2().border_color(rgb(ui_theme::BLUE))
			})
			.when(matches!(kind, Kind::Table), |d| {
				d.border_1().border_color(rgba(0xffffff12)).rounded_md()
			})
			.children(
				children.iter().enumerate().map(|(i, n)| render_node(n, &format!("{key}-{i}"))),
			)
			.into_any_element(),
	}
}
fn render_item(nodes: &[Node], key: &str) -> Vec<AnyElement> {
	let mut result = Vec::new();
	let mut start = 0;
	for (index, node) in nodes.iter().enumerate() {
		if matches!(
			node,
			Node::Block(
				Kind::Paragraph | Kind::List(_) | Kind::Code | Kind::Quote | Kind::Table,
				_
			) | Node::Rule
		) {
			if start < index {
				result.push(
					div()
						.child(inline(&nodes[start..index], &format!("{key}-text-{start}")))
						.into_any_element(),
				);
			}
			result.push(render_node(node, &format!("{key}-block-{index}")));
			start = index + 1;
		}
	}
	if start < nodes.len() {
		result.push(
			div().child(inline(&nodes[start..], &format!("{key}-text-{start}"))).into_any_element(),
		);
	}
	result
}

pub(super) fn plain_text(text: &str) -> String {
	Parser::new_ext(text, Options::ENABLE_TABLES)
		.filter_map(|event| match event {
			Event::Text(text) | Event::Code(text) => Some(text.into_string()),
			Event::SoftBreak | Event::HardBreak | Event::End(_) => Some(" ".into()),
			_ => None,
		})
		.collect::<String>()
		.split_whitespace()
		.collect::<Vec<_>>()
		.join(" ")
}

pub(super) fn render(text: &str, key: &str) -> AnyElement {
	div()
		.flex()
		.flex_col()
		.gap_2()
		.text_size(px(ui_theme::BODY_SIZE))
		.line_height(px(ui_theme::BODY_LINE_HEIGHT))
		.children(
			parse(text)
				.iter()
				.enumerate()
				.map(|(i, node)| render_node(node, &format!("{key}-{i}"))),
		)
		.into_any_element()
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn markdown_retains_unicode_styles_and_link_ranges() {
		let nodes = parse("**中文** and [source](/tmp/a.rs:12) with `code`");
		let mut out = Inline::default();
		append_inline(&nodes, HighlightStyle::default(), None, &mut out);
		assert_eq!(out.text, "中文 and source with code");
		assert!(out.highlights.iter().any(
			|(r, s)| &out.text[r.clone()] == "中文" && s.font_weight == Some(FontWeight::BOLD)
		));
		assert_eq!(&out.text[out.links[0].0.clone()], "source");
	}
	#[test]
	fn tables_lists_and_code_are_structural_blocks() {
		let nodes = parse(
			"# Heading\n\n- item\n\n```rust\nlet x = 1;\n```\n\n| A | B |\n|---|---|\n| one | two |\n",
		);
		assert!(nodes.iter().any(|n| matches!(n, Node::Block(Kind::Table, _))));
		assert!(nodes.iter().any(|n| matches!(n, Node::Block(Kind::Code, _))));
		assert!(nodes.iter().any(|n| matches!(n, Node::Block(Kind::List(_), _))));
	}
}
