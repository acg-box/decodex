//! Diagram integration: closed fences, literal fallback, resizing and source copy.
use super::*;

fn sources(nodes: &[Node]) -> Vec<(bool, String)> {
	nodes
		.iter()
		.flat_map(|node| match node {
			Node::Block(kind @ (Kind::Code | Kind::Mermaid { .. }), children) =>
				vec![(matches!(kind, Kind::Mermaid { .. }), code_text(children))],
			Node::Block(_, children) => sources(children),
			_ => Vec::new(),
		})
		.collect()
}

#[test]
fn closed_mermaid_fences_render_all_families_and_preserve_source() {
	for source in [
		"flowchart TD; A[请求] --> B[Reply]",
		"sequenceDiagram; A->>B: request; B-->>A: response",
		"stateDiagram-v2; [*] --> Active; Active --> [*]",
		"classDiagram; Order \"1\" *-- \"many\" Item : contains",
		"erDiagram; CUSTOMER ||--o{ ORDER : places",
	] {
		let markdown = format!("```mermaid title=example\r\n{source}  \r\n```\n\nAfter");
		let blocks = sources(&parse(&markdown));
		assert_eq!(blocks, vec![(true, format!("{source}  \r\n"))]);
		assert_eq!(
			diagram(&blocks[0].1).expect("supported family").text,
			mermaid::render(source, 256).expect("upstream diagram")
		);
	}
	for markdown in [
		"```mermaid,title=example\nflowchart TD; A --> B\n```\n",
		"> ~~~~mermaid\n> graph TD; A --> B\n> ~~~~~\n",
		"- Diagram:\n\n  ```mermaid\n  graph LR; A --> B\n  ```\n",
	] {
		let blocks = sources(&parse(markdown));
		assert_eq!(blocks.len(), 1);
		assert!(blocks[0].0);
		assert!(diagram(&blocks[0].1).is_some());
	}
}

#[test]
fn streaming_and_unsupported_blocks_keep_complete_literal_source() {
	for markdown in [
		"```mermaid\ngraph TD; A --> B\n",
		"````mermaid\ngraph TD; A --> B\n```\n",
		"> ```mermaid\n> graph TD; A --> B\n",
	] {
		assert!(sources(&parse(markdown)).iter().all(|(closed, _)| !closed));
	}
	for source in [
		"pie; Cats: 2".to_owned(),
		"flowchart TD; A[unclosed".to_owned(),
		"flowchart TD; click A https://example.test".to_owned(),
		"x".repeat(16 * 1024 + 1),
	] {
		let blocks = sources(&parse(&format!("```mermaid\n{source}\n```\n\nAfter")));
		assert_eq!(blocks, vec![(true, format!("{source}\n"))]);
		assert!(diagram(&blocks[0].1).is_none());
	}
}

struct Preview {
	text: String,
}
impl gpui::Render for Preview {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		super::super::render(&self.text, "diagram-preview")
	}
}

#[gpui::test]
fn mermaid_view_scrolls_without_wrapping_and_copies_original(cx: &mut gpui::TestAppContext) {
	let source =
		"flowchart LR; A[Request with a longer label] --> B[Reply with a longer label]  \r\n";
	let (preview, visual) =
		cx.add_window_view(|_, _| Preview { text: format!("```mermaid\r\n{source}```") });
	let mut previous_size = None;
	for width in [800., 160.] {
		visual.simulate_resize(gpui::size(px(width), px(500.)));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("mermaid-diagram-preview-0").expect("diagram viewport");
		assert!(bounds.size.width <= px(width));
		let text = visual.debug_bounds("mermaid-text-diagram-preview-0").expect("diagram text");
		if let Some(size) = previous_size {
			assert_eq!(text.size, size, "diagram must not wrap");
		}
		previous_size = Some(text.size);
		if width == 160. {
			visual.simulate_event(gpui::ScrollWheelEvent {
				position: bounds.center(),
				delta: gpui::ScrollDelta::Pixels(gpui::point(px(-60.), px(0.))),
				..Default::default()
			});
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
			let moved =
				visual.debug_bounds("mermaid-text-diagram-preview-0").expect("scrolled diagram");
			assert!(moved.origin.x < text.origin.x);
		}
	}
	let button = visual.debug_bounds("mermaid-copy-diagram-preview-0").expect("source copy");
	visual.simulate_click(button.center(), gpui::Modifiers::default());
	visual.update(|_, cx| {
		assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), Some(source.into()));
	});
	preview.update(visual, |s, cx| {
		s.text = "```mermaid\ngraph TD; A --> B\n".into();
		cx.notify();
	});
	visual.update(|window, cx| {
		window.draw(cx).clear();
	});
	assert!(visual.debug_bounds("mermaid-diagram-preview-0").is_none());
	assert!(visual.debug_bounds("copy-code-diagram-preview-0").is_some());
}
