//! Native Markdown presentation; HTML is text and remote images are not fetched.
#[path = "agent_markdown_cache.rs"] mod cache;
#[path = "agent_clipboard.rs"] mod clipboard;
#[path = "agent_markdown_code.rs"] mod code;
#[path = "agent_code_comments.rs"] mod code_comments;
#[path = "agent_math.rs"] mod math;
#[path = "agent_mermaid.rs"] mod mermaid;
#[path = "agent_mermaid_view.rs"] mod mermaid_view;
#[cfg(test)]
#[path = "agent_markdown_selection_tests.rs"]
mod selection_tests;
#[cfg(test)]
#[path = "agent_markdown_task_lists_tests.rs"]
mod task_lists_tests;

use std::{
	cell::{OnceCell, RefCell},
	collections::HashMap,
	ops::Range,
	rc::Rc,
	slice,
	sync::{Arc, OnceLock},
	time::Duration,
};

use gpui::{
	AnyElement, App, FontStyle, HighlightStyle, KeyDownEvent, RenderOnce, StrikethroughStyle,
};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};
use ui_theme::{BLUE, BODY_LINE_HEIGHT, BODY_SIZE, TEXT_MUTED};

#[cfg(test)] use crate::shell::agent_surface::{ClipboardItem, Context};
use crate::{
	shell::agent_surface::{
		FluentBuilder, FontWeight, InteractiveElement, IntoElement, ParentElement, Role,
		SharedString, StatefulInteractiveElement, Styled, Window, selectable_text, ui_theme,
	},
	ui_motion::{self, SmoothControl as _},
	ui_theme::HOVER_FILL,
};
use math::MathMarkdown;

#[derive(Clone, Debug)]
enum Kind {
	Paragraph,
	Heading(u8),
	List(Option<u64>),
	Item,
	Quote,
	Code { language: Option<String> },
	Mermaid { fence: Range<usize>, content_end: usize, geometry: Arc<OnceLock<(f32, f32)>> },
	Table,
	Row(bool),
	Cell,
	Strong,
	Emphasis,
	Strike,
	InlineCode,
	Math { source: String, display: bool },
	Link(String),
	Group,
}

#[derive(Clone, Debug)]
struct Node {
	content: NodeContent,
	source: Option<Range<usize>>,
}
#[derive(Clone, Debug)]
enum NodeContent {
	Text(String),
	Block(Kind, Vec<Node>),
	Rule,
}
impl Node {
	fn text(text: String) -> Self {
		Self { content: NodeContent::Text(text), source: None }
	}

	fn block(kind: Kind, children: Vec<Self>) -> Self {
		Self { content: NodeContent::Block(kind, children), source: None }
	}

	fn located(mut self, source: Range<usize>) -> Self {
		self.source = Some(source);
		self
	}
}

#[derive(Default)]
struct Inline {
	text: String,
	highlights: Vec<(Range<usize>, HighlightStyle)>,
	links: Vec<(Range<usize>, String)>,
	sources: Vec<(Range<usize>, Range<usize>)>,
}

#[derive(gpui::IntoElement)]
struct CopyButton {
	key: String,
	label: &'static str,
	text: String,
	rich: bool,
}
impl RenderOnce for CopyButton {
	fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
		let state = window.use_keyed_state(
			SharedString::from(format!("copy-state-{}", self.key)),
			cx,
			|_, _| None::<std::time::Instant>,
		);
		let copied = state.read(cx).is_some_and(|at| at.elapsed() < Duration::from_millis(1_200));

		if copied {
			ui_motion::request_frame(window, cx);
		}

		let click_state = state.clone();
		let click_text = self.text.clone();
		let selector = self.key.clone();

		gpui::div()
			.id(SharedString::from(self.key))
			.debug_selector(move || selector.clone())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(if copied { "Copied" } else { self.label })
			.size(gpui::px(24.))
			.flex()
			.items_center()
			.justify_center()
			.rounded(gpui::px(6.))
			.cursor_pointer()
			.hover(|s| s.bg(gpui::rgba(HOVER_FILL)))
			.on_click(move |_, _, cx| {
				clipboard::copy(click_text.clone(), self.rich, cx);

				click_state.update(cx, |s, cx| {
					*s = Some(std::time::Instant::now());

					cx.notify();
				});
			})
			.on_key_down(move |event: &KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					clipboard::copy(self.text.clone(), self.rich, cx);

					state.update(cx, |s, cx| {
						*s = Some(std::time::Instant::now());

						cx.notify();
					});

					cx.stop_propagation();
				}
			})
			.child(
				gpui::svg()
					.data(if copied {
						br#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12"><path d="M1.5 6L4.5 9L10.5 3" fill="none" stroke="white" stroke-width="1.1"/></svg>"#.as_slice()
					} else {
						br#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12"><path d="M3.75 3H9.75V10.5H3.75ZM7.5 .75H1.5V8.25" fill="none" stroke="white" stroke-width="1.1"/></svg>"#.as_slice()
					})
					.text_color(gpui::rgb(if copied { BLUE } else { TEXT_MUTED }))
					.size(gpui::px(12.)),
			)
			.smooth()
	}
}

pub(super) fn without_line_column(path: &str) -> &str {
	let mut path = path;

	for _ in 0..2 {
		let Some((prefix, number)) = path.rsplit_once(':') else { break };

		if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
			break;
		}

		path = prefix;
	}

	path
}

pub(super) fn copy_button(key: &str, label: &'static str, text: String) -> AnyElement {
	CopyButton { key: key.into(), label, text, rich: false }.into_any_element()
}

pub(super) fn response_copy_button(key: &str, label: &'static str, text: String) -> AnyElement {
	CopyButton { key: key.into(), label, text, rich: false }.into_any_element()
}

pub(super) fn plain_text(text: &str) -> String {
	cache::document(text).plain.get_or_init(|| parse_plain_text(text)).clone()
}

/// Text fallback for the observed weather widget. Keep the stored source intact.
/// Full snapshots are parsed, including incomplete streaming/reveal prefixes.
pub(super) fn response_text(text: &str) -> String {
	const PREFIXES: [&str; 2] = ["\u{e200}weather\u{e202}", "\u{e200}forecast\u{e202}"];

	if !text.contains('\u{e200}') {
		return text.into();
	}

	let mut code = Vec::new();

	for (event, range) in Parser::new(text).into_offset_iter() {
		if matches!(event, Event::Code(_) | Event::Start(Tag::CodeBlock(_))) {
			code.push(range);
		}
	}

	let mut out = String::with_capacity(text.len());
	let mut cursor = 0;

	for (start, _) in text.match_indices('\u{e200}') {
		if start < cursor || code.iter().any(|range| range.contains(&start)) {
			continue;
		}

		let tail = &text[start..];
		let end = if let Some(prefix) = PREFIXES.iter().find(|prefix| tail.starts_with(**prefix)) {
			let payload = &tail[prefix.len()..];

			if let Some(end) = payload.find('\u{e201}') {
				let reference = &payload[..end];

				if reference.is_empty()
					|| !reference.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
				{
					continue;
				}

				start + prefix.len() + end + '\u{e201}'.len_utf8()
			} else if payload.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
				text.len()
			} else {
				continue;
			}
		} else if PREFIXES.iter().any(|prefix| prefix.starts_with(tail)) {
			text.len()
		} else {
			continue;
		};

		out.push_str(&text[cursor..start]);

		cursor = end;
	}

	out.push_str(&text[cursor..]);

	out.trim_end().into()
}

pub(super) fn render_process(text: &str, key: &str) -> AnyElement {
	let document = cache::document(text);
	let mut selection = selectable_text::Document::markdown(document.source.clone());

	let content = gpui::div()
		.w_full()
		.min_w_0()
		.flex()
		.flex_col()
		.gap_1()
		.text_size(gpui::px(12.))
		.line_height(gpui::px(19.))
		.text_color(gpui::rgb(TEXT_MUTED))
		.children(document.nodes.get_or_init(|| parse(text)).iter().enumerate().map(
			|(i, node)| {
				let key = format!("{key}-{i}");

				if let NodeContent::Block(Kind::Heading(_) | Kind::Paragraph, children) =
					&node.content
				{
					gpui::div()
						.font_weight(FontWeight::NORMAL)
						.child({
							let mut out = Inline::default();

							append_inline(children, HighlightStyle::default(), None, &mut out);

							for (_, style) in &mut out.highlights {
								style.font_weight = None;
							}

							let start = selection.checkpoint();
							let element = selectable(out, &key, &mut selection);
							selection.mark(start, node.source.clone());
							element
						})
						.into_any_element()
				} else {
					render_node(node, &key, &mut selection)
				}
			},
		));
	selection.finish(key, content).into_any_element()
}

pub(super) fn render(text: &str, key: &str) -> AnyElement {
	let document = cache::document(text);
	let mut selection = selectable_text::Document::markdown(document.source.clone());

	let content = gpui::div()
		.flex()
		.flex_col()
		.gap_2()
		.text_size(gpui::px(BODY_SIZE))
		.line_height(gpui::px(BODY_LINE_HEIGHT))
		.children(
			document
				.nodes
				.get_or_init(|| parse(text))
				.iter()
				.enumerate()
				.map(|(i, node)| render_node(node, &format!("{key}-{i}"), &mut selection)),
		);
	selection.finish(key, content).into_any_element()
}

fn tag_kind(tag: Tag<'_>, range: Range<usize>) -> Kind {
	match tag {
		Tag::Paragraph => Kind::Paragraph,
		Tag::Heading { level, .. } => Kind::Heading(level as u8),
		Tag::List(start) => Kind::List(start),
		Tag::Item => Kind::Item,
		Tag::BlockQuote(_) => Kind::Quote,
		Tag::CodeBlock(CodeBlockKind::Fenced(info))
			if info.split([',', ' ', '\t']).next() == Some("mermaid") =>
			Kind::Mermaid { content_end: range.start, fence: range, geometry: Arc::default() },
		Tag::CodeBlock(CodeBlockKind::Fenced(info)) => Kind::Code {
			language: info
				.split([',', ' ', '\t'])
				.next()
				.filter(|s| !s.is_empty())
				.map(str::to_owned),
		},
		Tag::CodeBlock(CodeBlockKind::Indented) => Kind::Code { language: None },
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
	let options =
		Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
	let math = MathMarkdown::new(text, options, None);
	let mut stack = vec![(Kind::Group, Vec::new(), 0..text.len())];
	let mut flattened = 0;

	let events = math.events(Parser::new_ext(&math.markdown, options).into_offset_iter());
	for (event, range) in events {
		match event {
			Event::Start(tag) =>
				if stack.len() < 64 && flattened == 0 {
					stack.push((tag_kind(tag, range.clone()), Vec::new(), range));
				} else {
					flattened += 1;
				},
			Event::End(_) =>
				if flattened > 0 {
					flattened -= 1;
				} else if stack.len() > 1 {
					let (mut kind, children, source) = stack.pop().expect("open block");

					if matches!(kind, Kind::Paragraph)
						&& let Some(comments) = code_comments::nodes(&text[range.clone()])
					{
						stack
							.last_mut()
							.expect("root")
							.1
							.push(Node::block(Kind::Group, comments).located(range.clone()));

						continue;
					}

					if let Kind::Mermaid { fence, content_end, .. } = &kind
						&& !mermaid_view::has_closing_fence(text, fence.clone(), *content_end)
					{
						kind = Kind::Code { language: Some("mermaid".into()) };
					}

					stack
						.last_mut()
						.expect("root")
						.1
						.push(Node::block(kind, children).located(source));
				},
			event => {
				let node = match event {
					Event::Text(value) | Event::Html(value) | Event::InlineHtml(value) => {
						if let Some((Kind::Mermaid { content_end, .. }, _, _)) = stack.last_mut() {
							*content_end = range.end;
						}
						if let Some(display) = math.span(&range) {
							stack.last_mut().expect("root").1.push(
								Node::block(
									Kind::Math { source: text[range.clone()].to_owned(), display },
									vec![Node::text(value.into_string()).located(range.clone())],
								)
								.located(range.clone()),
							);

							continue;
						}

						let crlf = matches!(
							stack.last(),
							Some((Kind::Code { .. } | Kind::Mermaid { .. }, _, _))
						) && value.starts_with('\n')
							&& range.start > 0
							&& text.as_bytes()[range.start - 1] == b'\r';

						Node::text(if crlf { format!("\r{value}") } else { value.into_string() })
					},
					Event::Code(text) =>
						Node::block(Kind::InlineCode, vec![Node::text(text.into_string())]),
					Event::SoftBreak => Node::text(" ".into()),
					Event::HardBreak => Node::text("\n".into()),
					Event::Rule => Node { content: NodeContent::Rule, source: None },
					Event::TaskListMarker(done) =>
						Node::text(if done { "☑ " } else { "☐ " }.into()),
					Event::FootnoteReference(text) => Node::text(format!("[{text}]")),
					_ => continue,
				};

				stack.last_mut().expect("root").1.push(node.located(range));
			},
		}
	}

	stack.pop().expect("root").1
}

fn append_inline(nodes: &[Node], style: HighlightStyle, link: Option<&str>, out: &mut Inline) {
	for node in nodes {
		let start = out.text.len();
		match &node.content {
			NodeContent::Text(text) => {
				let start = out.text.len();

				out.text.push_str(text);

				let range = start..out.text.len();

				out.highlights.push((range.clone(), style));

				if let Some(link) = link {
					out.links.push((range, link.into()));
				}
			},
			NodeContent::Block(kind, children) => {
				let mut next = style;

				match kind {
					Kind::Strong => next.font_weight = Some(FontWeight::BOLD),
					Kind::Emphasis => next.font_style = Some(FontStyle::Italic),
					Kind::InlineCode => next.background_color = Some(gpui::rgba(0xffffff12).into()),
					Kind::Link(_) => next.color = Some(gpui::rgb(BLUE).into()),
					Kind::Strike =>
						next.strikethrough =
							Some(StrikethroughStyle { thickness: gpui::px(1.0), color: None }),
					_ => {},
				}

				let target = if let Kind::Link(url) = kind { Some(url.as_str()) } else { link };

				append_inline(children, next, target, out);
			},
			NodeContent::Rule => {},
		}
		if let Some(source) = &node.source {
			out.sources.push((start..out.text.len(), source.clone()));
		}
	}
}

fn inline(nodes: &[Node], key: &str, selection: &mut selectable_text::Document) -> AnyElement {
	let mut out = Inline::default();

	append_inline(nodes, HighlightStyle::default(), None, &mut out);

	selectable(out, key, selection)
}

fn selectable(out: Inline, key: &str, selection: &mut selectable_text::Document) -> AnyElement {
	selection
		.push_mapped(
			super::selectable_text::SelectableText {
				key: key.into(),
				text: out.text,
				highlights: out.highlights,
				links: out.links,
			},
			out.sources,
		)
		.into_any_element()
}

fn render_math_paragraph(
	nodes: &[Node],
	key: &str,
	selection: &mut selectable_text::Document,
) -> AnyElement {
	let mut elements = Vec::new();
	let mut start = 0;

	for (index, node) in nodes.iter().enumerate() {
		if matches!(&node.content, NodeContent::Block(Kind::Math { display: true, .. }, _)) {
			if start < index {
				elements.push(inline(
					&nodes[start..index],
					&format!("{key}-before-{index}"),
					selection,
				));
			}

			elements.push(render_node(node, &format!("{key}-formula-{index}"), selection));

			start = index + 1;
		}
	}

	if start < nodes.len() {
		elements.push(inline(&nodes[start..], &format!("{key}-after"), selection));
	}

	gpui::div().w_full().min_w_0().flex().flex_col().gap_2().children(elements).into_any_element()
}

fn render_node(node: &Node, key: &str, selection: &mut selectable_text::Document) -> AnyElement {
	let start = selection.checkpoint();
	let result = render_node_content(node, key, selection);
	selection.mark(start, node.source.clone());
	result
}

fn render_node_content(
	node: &Node,
	key: &str,
	selection: &mut selectable_text::Document,
) -> AnyElement {
	let NodeContent::Block(kind, children) = &node.content else {
		return match &node.content {
			NodeContent::Rule =>
				gpui::div().h(gpui::px(1.0)).my_2().bg(gpui::rgba(0xffffff18)).into_any_element(),
			_ =>
				gpui::div().child(inline(slice::from_ref(node), key, selection)).into_any_element(),
		};
	};

	if matches!(kind, Kind::Paragraph)
		&& children.iter().any(|node| {
			matches!(&node.content, NodeContent::Block(Kind::Math { display: true, .. }, _))
		}) {
		return render_math_paragraph(children, key, selection);
	}
	if let Kind::Mermaid { geometry, .. } = kind {
		return mermaid_view::render(children, key, geometry.clone(), selection);
	}

	match kind {
		Kind::Math { source, display: true } => gpui::div()
			.id(SharedString::from(format!("math-{key}")))
			.debug_selector({
				let key = key.to_owned();

				move || format!("math-{key}")
			})
			.flex()
			.flex_col()
			.items_start()
			.w_full()
			.min_w_0()
			.overflow_x_scroll()
			.font_family("Menlo")
			.whitespace_nowrap()
			.child(inline(children, &format!("math-text-{key}"), selection))
			.child(copy_button(&format!("math-copy-{key}"), "Copy formula", source.clone()))
			.into_any_element(),
		Kind::Paragraph | Kind::Cell | Kind::Heading(_) => gpui::div()
			.when(matches!(kind, Kind::Cell), |d| d.flex_1().min_w_0().p_2())
			.when(matches!(kind, Kind::Heading(_)), |d| {
				d.font_weight(FontWeight::SEMIBOLD).mt_2().text_size(gpui::px(
					if let Kind::Heading(level) = kind {
						19.0 - (*level as f32) * 1.0
					} else {
						14.0
					},
				))
			})
			.child(inline(children, key, selection))
			.into_any_element(),
		Kind::Code { language } => code::render(children, language.as_deref(), key, selection),
		Kind::Mermaid { .. } => code::render(children, Some("mermaid"), key, selection),
		Kind::List(start) => render_list(start, children, key, selection),

		Kind::Row(header) => gpui::div()
			.flex()
			.items_stretch()
			.border_b_1()
			.border_color(gpui::rgba(0xffffff12))
			.when(*header, |d| d.font_weight(FontWeight::SEMIBOLD).bg(gpui::rgba(0xffffff08)))
			.children(children.iter().enumerate().map(|(i, n)| {
				if i > 0 {
					selection.separate("\t");
				}
				render_node(n, &format!("{key}-{i}"), selection)
			}))
			.into_any_element(),
		Kind::Item => gpui::div()
			.w_full()
			.min_w_0()
			.flex()
			.flex_col()
			.gap_2()
			.children(render_item(children, key, selection))
			.into_any_element(),
		_ => gpui::div()
			.flex()
			.flex_col()
			.gap_2()
			.when(matches!(kind, Kind::Quote), |d| {
				d.pl_3().border_l_2().border_color(gpui::rgb(BLUE))
			})
			.when(matches!(kind, Kind::Table), |d| {
				d.border_1().border_color(gpui::rgba(0xffffff12)).rounded_md()
			})
			.children(children.iter().enumerate().map(|(i, n)| {
				if matches!(kind, Kind::Table) && i > 0 {
					selection.separate("\n");
				}
				render_node(n, &format!("{key}-{i}"), selection)
			}))
			.into_any_element(),
	}
}

fn render_list(
	start: &Option<u64>,
	children: &[Node],
	key: &str,
	selection: &mut selectable_text::Document,
) -> AnyElement {
	// Keep a short hanging indent. Numbered markers share a right edge even
	// when the list crosses a digit boundary; bullets need no number gutter.
	let marker_width = start.map_or(8., |start| {
		let last = start + children.len().saturating_sub(1) as u64;
		last.to_string().len() as f32 * BODY_SIZE * 0.65 + 4.
	});
	gpui::div()
		.w_full()
		.min_w_0()
		.flex()
		.flex_col()
		.gap_2()
		.children(children.iter().enumerate().map(|(index, child)| {
			if index > 0 {
				selection.separate("\n");
			}
			gpui::div()
				.w_full()
				.flex()
				.gap(gpui::px(6.))
				.child(
					gpui::div()
						.debug_selector({
							let key = format!("markdown-list-marker-{key}-{index}");
							move || key.clone()
						})
						.w(gpui::px(marker_width))
						.flex_shrink_0()
						.text_right()
						.text_color(gpui::rgb(TEXT_MUTED))
						.child(start.map_or_else(
							|| "•".into(),
							|start| format!("{}.", start + index as u64),
						)),
				)
				.child(
					gpui::div()
						.debug_selector({
							let key = format!("markdown-list-content-{key}-{index}");
							move || key.clone()
						})
						.flex_1()
						.min_w_0()
						.child(render_node(child, &format!("{key}-{index}"), selection)),
				)
		}))
		.into_any_element()
}

fn code_text(children: &[Node]) -> String {
	children
		.iter()
		.filter_map(|node| match &node.content {
			NodeContent::Text(text) => Some(text.as_str()),
			_ => None,
		})
		.collect()
}

fn render_item(
	nodes: &[Node],
	key: &str,
	selection: &mut selectable_text::Document,
) -> Vec<AnyElement> {
	let mut result = Vec::new();
	let mut start = 0;

	for (index, node) in nodes.iter().enumerate() {
		if matches!(
			&node.content,
			NodeContent::Block(
				Kind::Paragraph
					| Kind::List(_)
					| Kind::Code { .. }
					| Kind::Mermaid { .. }
					| Kind::Quote
					| Kind::Table,
				_
			) | NodeContent::Rule
		) {
			if start < index {
				result.push(
					gpui::div()
						.child(inline(
							&nodes[start..index],
							&format!("{key}-text-{start}"),
							selection,
						))
						.into_any_element(),
				);
			}

			result.push(render_node(node, &format!("{key}-block-{index}"), selection));

			start = index + 1;
		}
	}

	if start < nodes.len() {
		result.push(
			gpui::div()
				.child(inline(&nodes[start..], &format!("{key}-text-{start}"), selection))
				.into_any_element(),
		);
	}

	result
}

fn parse_plain_text(text: &str) -> String {
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

#[cfg(test)]
mod tests {

	use crate::shell::agent_surface::markdown::{
		self, ClipboardItem, FontWeight, HighlightStyle, Inline, Kind, Node, NodeContent,
	};

	struct CopyPreview {
		text: String,
	}
	impl gpui::Render for CopyPreview {
		fn render(
			&mut self,
			_: &mut gpui::Window,
			_: &mut gpui::Context<Self>,
		) -> impl gpui::IntoElement {
			super::super::history_entry(&decodex_protocol::AgentHistoryEntryDto {
				native_source: None,
				turn_id: None,
				weather: if self.text.contains("\u{e200}weather\u{e202}") {
					vec![
						decodex_protocol::WeatherForecast::parse(include_str!(
							"../examples/fixtures/singapore-weather.txt"
						))
						.unwrap(),
					]
				} else {
					Vec::new()
				},
				receipt: None,
				id: 42,
				kind: "assistant".into(),
				text: self.text.clone(),
				created_at_micros: 0,
				activity: None,
				usage: None,
				duration_ms: None,
			})
		}
	}

	#[test]
	fn weather_fallback_preserves_markdown_and_hides_every_stream_prefix() {
		let marker = "\u{e200}weather\u{e202}turn0forecast0\u{e201}";
		let body = "Singapore: **32°C**, cloudy.";

		assert_eq!(markdown::response_text(&format!("{body}\n\n{marker}")), body);

		for end in marker.char_indices().map(|(i, _)| i).chain([marker.len()]) {
			assert_eq!(
				markdown::response_text(&format!("{body}\n\n{}", &marker[..end])).trim_end(),
				body
			);
		}

		assert_eq!(
			markdown::response_text(&format!("Before {marker} after {marker}!")),
			"Before  after !"
		);

		for source in [
			format!("`{marker}`"),
			format!("```text\n{marker}\n```"),
			"\u{e200}unknown\u{e202}data\u{e201}".into(),
		] {
			assert_eq!(markdown::response_text(&source), source);
		}
	}

	#[test]
	fn forecast_marker_and_stream_prefixes_are_hidden_without_changing_code() {
		let marker = "\u{e200}forecast\u{e202}turn0forecast0\u{e201}";

		for end in marker.char_indices().map(|(i, _)| i).chain([marker.len()]) {
			assert_eq!(
				markdown::response_text(&format!("Cloudy.\n{}", &marker[..end])).trim_end(),
				"Cloudy."
			);
		}

		assert_eq!(markdown::response_text(&format!("`{marker}`")), format!("`{marker}`"));
	}

	#[test]
	fn deep_markdown_preserves_text_and_following_blocks() {
		for depth in [63, 64, 65, 128] {
			let source = format!("{}Deep 中文\n\nAfter **limit**\n", "> ".repeat(depth));
			let nodes = markdown::parse(&source);
			let mut out = Inline::default();

			markdown::append_inline(&nodes, HighlightStyle::default(), None, &mut out);

			assert_eq!(out.text, "Deep 中文After limit", "depth {depth}");
			assert!(out.highlights.iter().any(|(range, style)| {
				&out.text[range.clone()] == "limit" && style.font_weight == Some(FontWeight::BOLD)
			}));
		}
	}

	#[test]
	fn local_link_locations_do_not_become_part_of_the_revealed_filename() {
		for (target, expected) in [
			("/tmp/中文.rs:12:3", "/tmp/中文.rs"),
			("/tmp/a.rs:12", "/tmp/a.rs"),
			("/tmp/a:b.rs:12:3", "/tmp/a:b.rs"),
			("/tmp/a:b.rs", "/tmp/a:b.rs"),
			("/tmp/a.rs:", "/tmp/a.rs:"),
			("/tmp/a.rs:12x", "/tmp/a.rs:12x"),
		] {
			assert_eq!(markdown::without_line_column(target), expected);
		}

		let mut out = Inline::default();

		markdown::append_inline(
			&markdown::parse("| Source |\n|---|\n| [**Read** `parser`](/tmp/中文.rs:12:3) |"),
			HighlightStyle::default(),
			None,
			&mut out,
		);

		let labels: String = out.links.iter().map(|(range, _)| &out.text[range.clone()]).collect();

		assert_eq!(labels, "Read parser");
		assert!(out.links.iter().all(|(_, target)| target == "/tmp/中文.rs:12:3"));
	}

	#[test]
	fn copied_code_preserves_source_content() {
		for (source, expected) in [
			(
				"```rust\r\nlet 中文 = 1;  \r\n\tprintln!(\"{}\", 中文);\r\n```",
				"let 中文 = 1;  \r\n\tprintln!(\"{}\", 中文);\r\n",
			),
			("> ```sh\n> printf 'a'  \n> ```", "printf 'a'  \n"),
			("```\n<tool>not executable</tool>\n", "<tool>not executable</tool>\n"),
		] {
			fn blocks(nodes: &[Node]) -> Vec<String> {
				nodes
					.iter()
					.flat_map(|n| match &n.content {
						NodeContent::Block(Kind::Code { .. }, children) =>
							vec![markdown::code_text(children)],
						NodeContent::Block(_, children) => blocks(children),
						_ => Vec::new(),
					})
					.collect()
			}

			assert_eq!(blocks(&markdown::parse(source)), vec![expected]);
		}
	}

	#[gpui::test]
	fn weather_card_renders_and_response_copy_includes_forecast(cx: &mut gpui::TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, _| CopyPreview {
			text: "Cloudy.\n\n\u{e200}weather\u{e202}turn0forecast0\u{e201}".into(),
		});

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(700.), gpui::px(500.)));
			window.draw(cx).clear(cx);
		});

		assert!(visual.debug_bounds("weather-card-42-0").is_some(), "inline weather card");

		let copy = visual.debug_bounds("copy-response-42").unwrap();

		visual.simulate_click(copy.center(), gpui::Modifiers::default());

		visual.update(|_, cx| {
			let text = cx.read_from_clipboard().and_then(|item| item.text()).unwrap();

			assert!(text.contains("| 02:00 AM | Showers | 28 |"));
			assert!(!text.contains('\u{e200}'));
		});
	}

	#[gpui::test]
	fn response_and_code_copy_use_the_displayed_message(cx: &mut gpui::TestAppContext) {
		let original = "Answer **中文**\n\n```sh\r\nprintf 'hello'  \r\n```";
		let (preview, visual) = cx.add_window_view(|_, _| CopyPreview { text: original.into() });

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(700.), gpui::px(500.)));
			window.draw(cx).clear(cx);
		});

		let bounds = visual.debug_bounds("copy-code-message-42-1").expect("code copy control");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()),
				Some("printf 'hello'  \r\n".into())
			)
		});

		let bounds = visual.debug_bounds("copy-response-42").expect("response copy control");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		visual.update(|_, cx| {
			assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), Some(original.into()))
		});

		preview.update(visual, |s, cx| {
			s.text = "Next response".into();

			cx.notify();
		});

		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
			cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into()));
		});

		assert!(visual.debug_bounds("copy-code-message-42-1").is_none());

		visual.simulate_keystrokes("space");
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()),
				Some("Next response".into())
			)
		});
	}

	#[gpui::test]
	fn message_text_can_be_selected_and_copied_without_editing(cx: &mut gpui::TestAppContext) {
		let (_, visual) =
			cx.add_window_view(|_, _| CopyPreview { text: "Read **中文** text".into() });

		visual.update(|window, cx| {
			window.resize(gpui::size(gpui::px(700.), gpui::px(300.)));
			window.draw(cx).clear(cx);
		});

		let bounds = visual.debug_bounds("message-42-0").expect("selectable paragraph");

		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		visual.simulate_keystrokes("cmd-a cmd-c");
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()),
				Some("Read **中文** text".into())
			)
		});
	}

	#[test]
	fn markdown_retains_unicode_styles_and_link_ranges() {
		let nodes = markdown::parse("**中文** and [source](/tmp/a.rs:12) with `code`");
		let mut out = Inline::default();

		markdown::append_inline(&nodes, HighlightStyle::default(), None, &mut out);

		assert_eq!(out.text, "中文 and source with code");
		assert!(out.highlights.iter().any(
			|(r, s)| &out.text[r.clone()] == "中文" && s.font_weight == Some(FontWeight::BOLD)
		));
		assert_eq!(&out.text[out.links[0].0.clone()], "source");
	}

	#[test]
	fn tables_lists_and_code_are_structural_blocks() {
		let nodes = markdown::parse(
			"# Heading\n\n- item\n\n```rust\nlet x = 1;\n```\n\n| A | B |\n|---|---|\n| one | two |\n",
		);

		assert!(nodes.iter().any(|n| matches!(&n.content, NodeContent::Block(Kind::Table, _))));
		assert!(
			nodes.iter().any(|n| matches!(&n.content, NodeContent::Block(Kind::Code { .. }, _)))
		);
		assert!(nodes.iter().any(|n| matches!(&n.content, NodeContent::Block(Kind::List(_), _))));
	}
}
#[cfg(test)]
#[path = "agent_math_tests.rs"]
mod math_tests;
