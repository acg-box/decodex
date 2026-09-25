//! Rich response copies reuse the displayed Markdown tree and keep source text intact.
use super::{Kind, Node, parse};

fn escape(text: &str) -> String {
	text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub(super) fn html(text: &str) -> String {
	let mut output = String::new();
	for node in parse(text) {
		render(&node, &mut output);
	}
	output
}

fn render(node: &Node, output: &mut String) {
	let Node::Block(kind, children) = node else {
		match node {
			Node::Text(text) => output.push_str(&escape(text)),
			Node::Rule => output.push_str("<hr>"),
			Node::Block(_, _) => unreachable!(),
		}
		return;
	};
	let (open, close) = match kind {
		Kind::Paragraph => ("<p>".into(), "</p>".into()),
		Kind::Heading(level) => (format!("<h{level}>"), format!("</h{level}>")),
		Kind::List(Some(start)) => (format!("<ol start=\"{start}\">"), "</ol>".into()),
		Kind::List(None) => ("<ul>".into(), "</ul>".into()),
		Kind::Item => ("<li>".into(), "</li>".into()),
		Kind::Quote => ("<blockquote>".into(), "</blockquote>".into()),
		Kind::Code | Kind::Mermaid { .. } => ("<pre><code>".into(), "</code></pre>".into()),
		Kind::Table => ("<table>".into(), "</table>".into()),
		Kind::Row(_) => ("<tr>".into(), "</tr>".into()),
		Kind::Cell => ("<td>".into(), "</td>".into()),
		Kind::Strong => ("<strong>".into(), "</strong>".into()),
		Kind::Emphasis => ("<em>".into(), "</em>".into()),
		Kind::Strike => ("<del>".into(), "</del>".into()),
		Kind::Math { source, display } => {
			if *display {
				output.push_str("<pre>");
			}
			output.push_str(&escape(source));
			if *display {
				output.push_str("</pre>");
			}
			return;
		},
		Kind::InlineCode => ("<code>".into(), "</code>".into()),
		Kind::Link(destination) => {
			let web = reqwest::Url::parse(destination).ok().filter(|url| {
				["http", "https"].contains(&url.scheme()) && url.host_str().is_some()
			});
			match web {
				Some(url) => (format!("<a href=\"{}\">", escape(url.as_str())), "</a>".into()),
				None => (String::new(), format!(" ({})", escape(destination))),
			}
		},
		Kind::Group => (String::new(), String::new()),
	};
	output.push_str(&open);
	for child in children {
		render(child, output);
	}
	output.push_str(&close);
}

pub(super) fn copy(text: String, rich: bool, cx: &mut gpui::App) {
	cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
	if rich {
		let markup = html(&text);
		#[cfg(all(target_os = "macos", not(test)))]
		append_native_html(&text, &markup);
		#[cfg(any(not(target_os = "macos"), test))]
		let _ = markup;
	}
}

#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
fn append_native_html(text: &str, html: &str) {
	use objc2::{
		msg_send,
		rc::Retained,
		runtime::{AnyClass, AnyObject},
	};
	// SAFETY: AppKit's general pasteboard and NSString arguments live through each
	// synchronous call. This runs on GPUI's UI thread after its plain-text write.
	unsafe {
		let Some(class) = AnyClass::get(c"NSPasteboard") else { return };
		let board: Retained<AnyObject> = msg_send![class, generalPasteboard];
		append_html_to_board(&board, text, html);
	}
}

#[cfg(target_os = "macos")]
fn append_html_to_board(board: &objc2::runtime::AnyObject, text: &str, html: &str) {
	use objc2::{msg_send, rc::Retained};
	use objc2_foundation::NSString;
	let plain_type = NSString::from_str("public.utf8-plain-text");
	let html_type = NSString::from_str("public.html");
	// SAFETY: The caller supplies an NSPasteboard; all strings are retained for
	// the duration of the calls. Preserve a clipboard replaced since the copy.
	unsafe {
		let current: Option<Retained<NSString>> = msg_send![board, stringForType: &*plain_type];
		if current.as_ref().is_some_and(|value| value.to_string() == text) {
			let markup = NSString::from_str(html);
			let _: bool = msg_send![board, setString: &*markup, forType: &*html_type];
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	#[cfg(target_os = "macos")]
	#[test]
	fn native_pasteboard_keeps_both_formats_and_preserves_replaced_content() {
		use objc2::{
			msg_send,
			rc::Retained,
			runtime::{AnyClass, AnyObject},
		};
		use objc2_foundation::NSString;
		// SAFETY: Use an isolated named pasteboard, never the user's clipboard.
		unsafe {
			let board: Retained<AnyObject> = msg_send![
				AnyClass::get(c"NSPasteboard").expect("AppKit"),
				pasteboardWithUniqueName
			];
			let plain_type = NSString::from_str("public.utf8-plain-text");
			let html_type = NSString::from_str("public.html");
			let original = "**中文**\n";
			let text = NSString::from_str(original);
			let _: isize = msg_send![&*board, clearContents];
			let set: bool = msg_send![&*board, setString: &*text, forType: &*plain_type];
			assert!(set);
			let markup = html(original);
			append_html_to_board(&board, original, &markup);
			let plain: Option<Retained<NSString>> = msg_send![&*board, stringForType: &*plain_type];
			let rich: Option<Retained<NSString>> = msg_send![&*board, stringForType: &*html_type];
			assert_eq!(plain.map(|v| v.to_string()).as_deref(), Some(original));
			assert_eq!(rich.map(|v| v.to_string()), Some(markup));
			let _: isize = msg_send![&*board, clearContents];
			let other = NSString::from_str("replacement");
			let _: bool = msg_send![&*board, setString: &*other, forType: &*plain_type];
			append_html_to_board(&board, original, "<p>stale</p>");
			let rich: Option<Retained<NSString>> = msg_send![&*board, stringForType: &*html_type];
			assert!(rich.is_none());
			let _: () = msg_send![&*board, releaseGlobally];
		}
	}
	#[test]
	fn response_html_keeps_formatting_and_inert_content() {
		let rendered = html(
			"# Title\n\n**Bold** *italic* ~~old~~ `a<b`\n\n<script>x</script>\n\n![alt](https://test/image)\n\n[local](/tmp/a) [bad](javascript:alert(1)) [web](https://example.com)\n\n| A | B |\n|---|---|\n| x | y |\n",
		);
		for part in [
			"<h1>Title</h1>",
			"<strong>Bold</strong>",
			"<em>italic</em>",
			"<del>old</del>",
			"<code>a&lt;b</code>",
			"&lt;script&gt;",
			"local (/tmp/a)",
			"bad (javascript:alert(1))",
			"<a href=\"https://example.com/\">web</a>",
			"<table>",
			"<td>x</td>",
		] {
			assert!(rendered.contains(part), "missing {part}: {rendered}");
		}
		assert!(!rendered.contains("<img"));
		assert!(!rendered.contains("https://test/image"));
		assert!(!rendered.contains("<script>"));
	}
}
