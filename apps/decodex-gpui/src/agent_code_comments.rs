//! Read-only presentation of native review directives; stored Markdown stays intact.
use std::collections::HashMap;

use crate::shell::agent_surface::markdown::{Kind, Node};

pub(super) fn nodes(source: &str) -> Option<Vec<Node>> {
	source.trim().lines().map(comment).collect()
}

fn comment(line: &str) -> Option<Node> {
	let line = line.trim();
	let markers = line.bytes().take_while(|c| *c == b':').count();

	if !(1..=3).contains(&markers) {
		return None;
	}

	let source = line[markers..].strip_prefix("code-comment{")?.strip_suffix('}')?;
	let fields = attributes(source)?;
	let title = fields.get("title")?.trim();
	let body = fields.get("body")?.trim();
	let file = fields.get("file")?.trim();

	if title.is_empty() || body.is_empty() || file.is_empty() {
		return None;
	}

	let integer = |key: &str| fields.get(key)?.trim_start_matches(['P', 'p']).parse::<i64>().ok();
	let start = integer("start").unwrap_or(1).max(1);
	let end = integer("end").unwrap_or(start).max(start);
	let title = match integer("priority") {
		Some(priority @ 0..=3) if !title.starts_with("[P") && !title.starts_with("[p") =>
			format!("[P{priority}] {title}"),
		_ => title.to_owned(),
	};
	let location =
		if start == end { format!("{file}:{start}") } else { format!("{file}:{start}-{end}") };

	Some(Node::Block(
		Kind::Quote,
		vec![
			Node::Block(Kind::Paragraph, vec![Node::Block(Kind::Strong, vec![Node::Text(title)])]),
			Node::Block(Kind::Paragraph, vec![Node::Text(body.to_owned())]),
			Node::Block(
				Kind::Paragraph,
				vec![Node::Block(Kind::InlineCode, vec![Node::Text(location)])],
			),
		],
	))
}

fn attributes(mut source: &str) -> Option<HashMap<String, String>> {
	let mut fields = HashMap::new();

	while !source.trim().is_empty() {
		source = source.trim_start();

		let equals = source.find('=')?;
		let key = source[..equals].trim();

		if key.is_empty() || key.chars().any(char::is_whitespace) {
			return None;
		}

		source = source[equals + 1..].trim_start();

		let value;

		if let Some(quoted) = source.strip_prefix('"') {
			let mut result = String::new();
			let mut chars = quoted.char_indices();
			let end = loop {
				let (index, ch) = chars.next()?;

				if ch == '"' {
					break index + 1;
				}
				if ch == '\\' {
					let (_, escaped) = chars.next()?;

					if !matches!(escaped, '"' | '\\') {
						result.push('\\');
					}

					result.push(escaped);
				} else {
					result.push(ch);
				}
			};

			source = &quoted[end..];
			value = result;
		} else {
			let end = source.find(char::is_whitespace).unwrap_or(source.len());

			value = source[..end].to_owned();
			source = &source[end..];
		}

		if fields.insert(key.to_owned(), value).is_some() {
			return None;
		}
	}

	Some(fields)
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::markdown::code_comments::{self, Node};
	fn text(nodes: &[Node]) -> String {
		nodes
			.iter()
			.map(|node| match node {
				Node::Text(value) => value.clone(),
				Node::Block(_, children) => text(children),
				Node::Rule => String::new(),
			})
			.collect::<Vec<_>>()
			.join(" ")
	}
	#[test]
	fn review_comments_render_readably_without_interpreting_body_directives() {
		let source = r#"::code-comment{title="Fix parsing" body="Keep role=\"tab\", ::git-stage{cwd=/tmp}, file=, and \n literal." file="/tmp/中文.rs" start=8 end=2 priority="P2"}"#;
		let rendered = text(&super::super::parse(source));

		assert!(rendered.contains("[P2] Fix parsing"));
		assert!(rendered.contains(r#"role="tab", ::git-stage{cwd=/tmp}, file=, and \n literal."#));
		assert!(rendered.contains("/tmp/中文.rs:8"));
		assert!(!rendered.contains("::code-comment"));
		assert!(
			text(&super::super::parse(&format!("```text\n{source}\n```")))
				.contains("::code-comment")
		);
		assert!(text(&super::super::parse(&format!("`{source}`"))).contains("::code-comment"));

		for (offset, _) in source.char_indices().skip(1) {
			assert!(
				code_comments::nodes(&source[..offset]).is_none(),
				"incomplete directive at {offset}"
			);
		}
	}
	#[test]
	fn review_comment_defaults_and_invalid_records_preserve_meaning() {
		let source = r#":::code-comment{title="[P1] Review" body="Details" file="src/a.rs"}"#;
		let rendered = text(&super::super::parse(source));

		assert!(rendered.contains("[P1] Review"));
		assert!(rendered.contains("src/a.rs:1"));

		for source in [
			r#"::code-comment{title="Only title"}"#,
			r#"::code-comment{title="" body="Body" file="a"}"#,
			r#"::code-comment{title="A" title="B" body="Body" file="a"}"#,
		] {
			assert!(code_comments::nodes(source).is_none());
			assert!(text(&super::super::parse(source)).contains("::code-comment"));
		}
	}
}
