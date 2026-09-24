//! Native Markdown presentation; HTML is text and remote images are not fetched.
use super::*;
use gpui::{AnyElement, FontStyle, HighlightStyle};
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
	for (event, range) in Parser::new_ext(
		text,
		Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
	)
	.into_offset_iter()
	{
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
					Event::Text(value) | Event::Html(value) | Event::InlineHtml(value) => {
						let crlf = matches!(stack.last(), Some((Kind::Code, _)))
							&& value.starts_with('\n')
							&& range.start > 0 && text.as_bytes()[range.start - 1] == b'\r';
						Node::Text(if crlf { format!("\r{value}") } else { value.into_string() })
					},
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
	super::selectable_text::SelectableText {
		key: key.into(),
		text: out.text,
		highlights: out.highlights,
		links: out.links,
	}
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
			.flex()
			.flex_col()
			.gap_2()
			.p_3()
			.rounded_md()
			.bg(rgba(0x00000045))
			.font_family("Menlo")
			.text_size(px(12.0))
			.line_height(px(19.0))
			.child(copy_button(&format!("copy-code-{key}"), "Copy code", code_text(children)))
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

fn code_text(children: &[Node]) -> String {
	children
		.iter()
		.filter_map(|node| match node {
			Node::Text(text) => Some(text.as_str()),
			_ => None,
		})
		.collect()
}

pub(super) fn copy_button(key: &str, label: &'static str, text: String) -> AnyElement {
	CopyButton { key: key.into(), label, text }.into_any_element()
}
#[derive(gpui::IntoElement)]
struct CopyButton {
	key: String,
	label: &'static str,
	text: String,
}
impl gpui::RenderOnce for CopyButton {
	fn render(self, window: &mut Window, cx: &mut gpui::App) -> impl IntoElement {
		use crate::ui_motion::SmoothControl as _;
		let state = window.use_keyed_state(
			SharedString::from(format!("copy-state-{}", self.key)),
			cx,
			|_, _| None::<std::time::Instant>,
		);
		let copied =
			state.read(cx).is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(1200));
		if copied {
			crate::ui_motion::request_frame(window, cx);
		}
		let click_state = state.clone();
		let click_text = self.text.clone();
		let selector = self.key.clone();
		div()
			.id(SharedString::from(self.key))
			.debug_selector(move || selector.clone())
			.role(Role::Button)
			.tab_index(0)
			.aria_label(if copied { "Copied" } else { self.label })
			.size(px(24.))
			.flex()
			.items_center()
			.justify_center()
			.rounded(px(6.))
			.cursor_pointer()
			.hover(|s| s.bg(rgba(0xffffff10)))
			.on_click(move |_, _, cx| {
				cx.write_to_clipboard(ClipboardItem::new_string(click_text.clone()));
				click_state.update(cx, |s, cx| {
					*s = Some(std::time::Instant::now());
					cx.notify();
				});
			})
			.on_key_down(move |event: &gpui::KeyDownEvent, _, cx| {
				if ["enter", "space"].contains(&event.keystroke.key.as_str()) {
					cx.write_to_clipboard(ClipboardItem::new_string(self.text.clone()));
					state.update(cx, |s, cx| {
						*s = Some(std::time::Instant::now());
						cx.notify();
					});
					cx.stop_propagation();
				}
			})
			.child(
				gpui::canvas(
					|_, _, _| (),
					move |bounds, _, window, _| {
						let mut path = gpui::PathBuilder::stroke(px(1.1));
						let point = |x: f32, y: f32| {
							bounds.origin + gpui::point(px(x * 0.75), px(y * 0.75))
						};
						if copied {
							path.move_to(point(2., 8.));
							path.line_to(point(6., 12.));
							path.line_to(point(14., 4.));
						} else {
							path.move_to(point(5., 4.));
							path.line_to(point(13., 4.));
							path.line_to(point(13., 14.));
							path.line_to(point(5., 14.));
							path.close();
							path.move_to(point(10., 1.));
							path.line_to(point(2., 1.));
							path.line_to(point(2., 11.));
						}
						if let Ok(path) = path.build() {
							window.paint_path(
								path,
								rgb(if copied { ui_theme::BLUE } else { ui_theme::TEXT_MUTED }),
							);
						}
					},
				)
				.size(px(12.)),
			)
			.smooth()
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

/// Text fallback for the observed weather widget. Keep the stored source intact.
/// Full snapshots are parsed, including incomplete streaming/reveal prefixes.
pub(super) fn response_text(text: &str) -> String {
	const PREFIX: &str = "\u{e200}weather\u{e202}";
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
		let end = if let Some(payload) = tail.strip_prefix(PREFIX) {
			if let Some(end) = payload.find('\u{e201}') {
				let reference = &payload[..end];
				if reference.is_empty()
					|| !reference.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
				{
					continue;
				}
				start + PREFIX.len() + end + '\u{e201}'.len_utf8()
			} else if payload.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
				text.len()
			} else {
				continue;
			}
		} else if PREFIX.starts_with(tail) {
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
	fn weather_fallback_preserves_markdown_and_hides_every_stream_prefix() {
		let marker = "\u{e200}weather\u{e202}turn0forecast0\u{e201}";
		let body = "Singapore: **32°C**, cloudy.";
		assert_eq!(response_text(&format!("{body}\n\n{marker}")), body);
		for end in marker.char_indices().map(|(i, _)| i).chain([marker.len()]) {
			assert_eq!(response_text(&format!("{body}\n\n{}", &marker[..end])).trim_end(), body);
		}
		assert_eq!(response_text(&format!("Before {marker} after {marker}!")), "Before  after !");
		for source in [
			format!("`{marker}`"),
			format!("```text\n{marker}\n```"),
			"\u{e200}unknown\u{e202}data\u{e201}".into(),
		] {
			assert_eq!(response_text(&source), source);
		}
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
					.flat_map(|n| match n {
						Node::Block(Kind::Code, children) => vec![code_text(children)],
						Node::Block(_, children) => blocks(children),
						_ => Vec::new(),
					})
					.collect()
			}
			assert_eq!(blocks(&parse(source)), vec![expected]);
		}
	}

	struct CopyPreview {
		text: String,
	}
	impl gpui::Render for CopyPreview {
		fn render(
			&mut self,
			_: &mut gpui::Window,
			_: &mut gpui::Context<Self>,
		) -> impl gpui::IntoElement {
			super::super::history_entry(&decodex_protocol::ChiefHistoryEntryDto {
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
	#[gpui::test]
	fn weather_card_is_compact_and_response_copy_includes_forecast(cx: &mut gpui::TestAppContext) {
		let (_, visual) = cx.add_window_view(|_, _| CopyPreview {
			text: "Cloudy.\n\n\u{e200}weather\u{e202}turn0forecast0\u{e201}".into(),
		});
		visual.update(|window, cx| {
			window.resize(gpui::size(px(700.), px(500.)));
			window.draw(cx).clear();
		});
		let card = visual.debug_bounds("weather-card-42-0").expect("inline weather card");
		assert_eq!(card.size.width, px(420.));
		assert!(card.size.height < px(240.));
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
			window.resize(gpui::size(px(700.), px(500.)));
			window.draw(cx).clear();
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
			window.draw(cx).clear();
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
			window.resize(gpui::size(px(700.), px(300.)));
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("message-42-0").expect("selectable paragraph");
		visual.simulate_click(bounds.center(), gpui::Modifiers::default());
		visual.simulate_keystrokes("cmd-a cmd-c");
		visual.update(|_, cx| {
			assert_eq!(
				cx.read_from_clipboard().and_then(|item| item.text()),
				Some("Read 中文 text".into())
			)
		});
	}

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
