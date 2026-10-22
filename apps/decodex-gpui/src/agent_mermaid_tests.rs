//! Closed fences, asynchronous image presentation, literal fallback and original source copy.
use gpui::{Modifiers, Render, ScrollDelta, ScrollEvent, TestAppContext};

use super::{Context, IntoElement, Window};
use crate::shell::agent_surface::markdown::{self, Kind, Node, NodeContent};

struct Preview {
	text: String,
}
impl Render for Preview {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		markdown::render(&self.text, "diagram-preview")
	}
}

fn sources(nodes: &[Node]) -> Vec<(bool, String)> {
	nodes
		.iter()
		.flat_map(|node| match &node.content {
			NodeContent::Block(kind @ (Kind::Code { .. } | Kind::Mermaid { .. }), children) =>
				vec![(matches!(kind, Kind::Mermaid { .. }), markdown::code_text(children))],
			NodeContent::Block(_, children) => sources(children),
			_ => Vec::new(),
		})
		.collect()
}

#[test]
fn closed_fences_preserve_original_source_in_markdown_containers() {
	for markdown in [
		"```mermaid title=example\r\nflowchart TD; A --> B  \r\n```\n\nAfter",
		"```mermaid,title=example\nflowchart TD; A --> B\n```\n",
		"> ~~~~mermaid\n> graph TD; A --> B\n> ~~~~~\n",
		"- Diagram:\n\n  ```mermaid\n  graph LR; A --> B\n  ```\n",
	] {
		let blocks = sources(&markdown::parse(markdown));
		assert_eq!(blocks.len(), 1);
		assert!(blocks[0].0);
		assert!(blocks[0].1.contains("A --> B"));
	}
	let blocks = sources(&markdown::parse("```mermaid\r\nflowchart TD; A --> B  \r\n```"));
	assert_eq!(blocks[0].1, "flowchart TD; A --> B  \r\n");
}

#[test]
fn incomplete_streaming_fences_stay_literal() {
	for markdown in [
		"```mermaid\ngraph TD; A --> B\n",
		"````mermaid\ngraph TD; A --> B\n```\n",
		"> ```mermaid\n> graph TD; A --> B\n",
	] {
		assert!(sources(&markdown::parse(markdown)).iter().all(|(closed, _)| !closed));
	}
}

#[gpui::test]
fn prepared_images_evict_old_sources_within_the_memory_limit(cx: &mut TestAppContext) {
	let fixtures: Vec<(String, String)> =
		serde_json::from_str(include_str!("agent_mermaid/fixtures.json")).expect("fixtures");
	let (_, source) = fixtures.iter().find(|(kind, _)| kind == "sequence").expect("sequence");
	let renderer = cx.update(|cx| cx.svg_renderer());
	let diagram =
		std::sync::Arc::new(markdown::mermaid::prepare(source, &renderer).expect("image"));
	let mut images = super::Images::default();
	for index in 0..80 {
		images.insert(format!("source-{index}").into(), diagram.clone());
		assert!(images.bytes <= 64 * 1_024 * 1_024);
	}
	assert!(images.entries.len() < 64, "the image byte limit must trigger before the item limit");
	assert!(images.get("source-0").is_none());
	let recent = images.entries.front().expect("retained image").0.clone();
	let older = images.entries.get(1).expect("another retained image").0.clone();
	assert!(images.get(&recent).is_some());
	images.insert("new-source".into(), diagram);
	assert!(images.get(&recent).is_some(), "a viewed image stays resident");
	assert!(images.get(&older).is_none(), "the oldest unviewed image is evicted");
}

#[gpui::test]
fn an_evicted_diagram_keeps_its_geometry_while_its_image_is_restored(cx: &mut TestAppContext) {
	let text = "```mermaid\nflowchart TD; A --> B\n```".to_owned();
	let (preview, visual) = cx.add_window_view(|_, _| Preview { text: text.clone() });
	visual.update(|window, cx| window.draw(cx).clear(cx));
	visual.run_until_parked();
	visual.update(|window, cx| window.draw(cx).clear(cx));
	let size = visual.debug_bounds("mermaid-diagram-preview-0").expect("viewport").size;
	preview.update(visual, |s, cx| {
		s.text = "Another transcript row".into();
		cx.notify();
	});
	visual.update(|window, cx| window.draw(cx).clear(cx));
	visual.update(|window, cx| {
		let images = std::mem::take(cx.default_global::<super::Images>());
		for (_, diagram) in images.entries {
			cx.drop_image(diagram.image.clone(), Some(window));
		}
	});
	preview.update(visual, |s, cx| {
		s.text = text;
		cx.notify();
	});
	visual.update(|window, cx| window.draw(cx).clear(cx));
	assert!(visual.debug_bounds("mermaid-image-diagram-preview-0").is_none());
	assert_eq!(
		visual.debug_bounds("mermaid-diagram-preview-0").expect("reserved viewport").size,
		size
	);
	assert!(visual.debug_bounds("copy-code-diagram-preview-0").is_none());
	visual.run_until_parked();
	visual.update(|window, cx| window.draw(cx).clear(cx));
	assert!(visual.debug_bounds("mermaid-image-diagram-preview-0").is_some());
	assert_eq!(
		visual.debug_bounds("mermaid-diagram-preview-0").expect("ready viewport").size,
		size
	);
}

#[gpui::test]
fn a_diagram_returns_without_a_source_flash_after_its_row_is_culled(cx: &mut TestAppContext) {
	let text = "```mermaid\nflowchart TD; A --> B\n```".to_owned();
	let (preview, visual) = cx.add_window_view(|_, _| Preview { text: text.clone() });
	visual.update(|window, cx| window.draw(cx).clear(cx));
	visual.run_until_parked();
	visual.update(|window, cx| window.draw(cx).clear(cx));
	assert!(visual.debug_bounds("mermaid-image-diagram-preview-0").is_some());

	preview.update(visual, |s, cx| {
		s.text = "Another transcript row".into();
		cx.notify();
	});
	visual.update(|window, cx| window.draw(cx).clear(cx));
	assert!(visual.debug_bounds("mermaid-image-diagram-preview-0").is_none());

	preview.update(visual, |s, cx| {
		s.text = text;
		cx.notify();
	});
	visual.update(|window, cx| window.draw(cx).clear(cx));
	assert!(
		visual.debug_bounds("mermaid-image-diagram-preview-0").is_some(),
		"returning rows must reuse the ready image without showing the source first"
	);
}

#[gpui::test]
fn image_scrolls_without_relayout_and_copies_original_source(cx: &mut TestAppContext) {
	let source = "flowchart LR; A([\"Request with a longer label<br/>completed\"]) --> B[Reply with a longer label<br />ready]  \r\n";
	let (preview, visual) =
		cx.add_window_view(|_, _| Preview { text: format!("```mermaid\r\n{source}```") });
	visual.update(|window, cx| {
		window.draw(cx).clear(cx);
	});
	visual.run_until_parked();
	let mut previous_size = None;

	for width in [800., 160.] {
		visual.simulate_resize(gpui::size(gpui::px(width), gpui::px(500.)));
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});
		let bounds = visual.debug_bounds("mermaid-diagram-preview-0").expect("diagram viewport");
		assert!(bounds.size.width <= gpui::px(width));
		let image = visual.debug_bounds("mermaid-image-diagram-preview-0").expect("diagram image");
		if let Some(size) = previous_size {
			assert_eq!(image.size, size);
		}
		previous_size = Some(image.size);
		if width == 160. {
			assert!(image.left() >= bounds.left(), "wide diagrams must keep their start reachable");
			visual.simulate_event(ScrollEvent {
				position: bounds.center(),
				delta: ScrollDelta::Pixels(gpui::point(gpui::px(-60.), gpui::px(0.))),
				..Default::default()
			});
			visual.update(|window, cx| {
				window.draw(cx).clear(cx);
			});
			let moved =
				visual.debug_bounds("mermaid-image-diagram-preview-0").expect("scrolled image");
			assert!(moved.origin.x < image.origin.x);
		}
	}

	let button = visual.debug_bounds("mermaid-copy-diagram-preview-0").expect("source copy");
	visual.simulate_click(button.center(), Modifiers::default());
	visual.update(|_, cx| {
		assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), Some(source.into()));
	});

	// More streamed prose must retain the existing diagram; replacing its source must not.
	preview.update(visual, |s, cx| {
		s.text.push_str("\n\nAfter the diagram");
		cx.notify();
	});
	visual.update(|window, cx| {
		window.draw(cx).clear(cx);
	});
	assert!(visual.debug_bounds("mermaid-image-diagram-preview-0").is_some());

	for text in ["```mermaid\ngraph TD; A --> B\n", "```mermaid\nflowchart TD; A[unclosed\n```\n"] {
		preview.update(visual, |s, cx| {
			s.text = text.into();
			cx.notify();
		});
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});
		visual.run_until_parked();
		visual.update(|window, cx| {
			window.draw(cx).clear(cx);
		});
		assert!(visual.debug_bounds("mermaid-image-diagram-preview-0").is_none());
		assert!(visual.debug_bounds("copy-code-diagram-preview-0").is_some());
	}
}
