//! Desktop integration contracts, independent of Ondine's parser and layout test suite.
use gpui::TestAppContext;
use roxmltree::{Document, Node};

use crate::{
	shell::agent_surface::markdown::mermaid::{self, svg},
	ui_theme::{ColorScheme, ColorTheme, appearance::Theme},
};

#[gpui::test]
fn every_diagram_type_rasterizes_with_the_desktop_theme(cx: &mut TestAppContext) {
	let fixtures: Vec<(String, String)> =
		serde_json::from_str(include_str!("fixtures.json")).expect("diagram fixtures");

	assert_eq!(fixtures.len(), 33);

	let renderer = cx.update(|cx| cx.svg_renderer());

	for (kind, source) in fixtures {
		let diagram = super::prepare(&source, &renderer, Theme::Dark.into())
			.unwrap_or_else(|error| panic!("{kind}: {error}"));

		assert!(
			diagram.image.as_bytes(0).is_some_and(|pixels| {
				pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0)
			}),
			"{kind}: empty image"
		);
	}
}

#[test]
fn supported_diagrams_use_ondine_and_keep_authored_labels() {
	for (source, kind, label) in [
		(
			"flowchart LR\nsubgraph Client\nA[\"用户\"] --> B[(Database)]\nend\nB -.-> A",
			"flowchart",
			"用户",
		),
		(
			"flowchart TD; A([\"请求<br>Ready\"]) --> B[Work<br/>next] --> C{\"Done?<br />Retry?\"}",
			"flowchart",
			"Ready",
		),
		("sequenceDiagram\nautonumber\nA->>+B: Request\nB-->>-A: Reply", "sequence", "Request"),
		(
			"stateDiagram-v2\nstate Work {\n[*] --> Ready\nReady --> Done\n}\n[*] --> Work",
			"state",
			"Ready",
		),
		(
			"classDiagram\nnamespace Models {\nclass Animal\nclass Dog\n}\nAnimal <|-- Dog",
			"class",
			"Animal",
		),
		("erDiagram\nORDER ||--|{ LINE-ITEM : contains", "er", "LINE-ITEM"),
		("pie\n\"Cats\" : 2\n\"Dogs\" : 3", "pie", "Cats"),
		("mindmap\n  root((Project))\n    Design\n    Build", "mindmap", "Project"),
	] {
		let rendered = mermaid::compile(source, Theme::Dark.into()).expect("supported diagram");

		assert_eq!(rendered.r#type, kind);

		let document = Document::parse(&rendered.svg).expect("valid SVG");

		assert!(
			document
				.descendants()
				.any(|node| node.is_text() && node.text().is_some_and(|text| text.contains(label)))
		);

		for node in document.descendants().filter(Node::is_element) {
			for attribute in node.attributes().filter(|attribute| {
				matches!(attribute.name(), "style" | "fill" | "stroke" | "font-weight")
			}) {
				assert!(
					!attribute.value().contains("var("),
					"unresolved presentation: {attribute:?}"
				);
				assert!(!attribute.value().contains("color-mix("));
			}
		}
	}
}

#[test]
fn theme_resolution_preserves_label_text_and_xml_escaping() {
	let input = r#"<svg xmlns="http://www.w3.org/2000/svg" style="--_fg:var(--ondine-fg,#000);--font:&quot;A &amp; B&quot;;font-family:var(--font)"><text fill="var(--_fg)">var(--_fg) &amp; &lt;literal&gt;</text><path stroke="color-mix(in srgb,var(--ondine-surface-alt) 50%,var(--ondine-bg))"/></svg>"#;
	let svg = svg::desktop(input, Theme::Dark.into()).expect("desktop SVG");
	let document = Document::parse(&svg).expect("escaping remains valid XML");
	let root = document.root_element();

	assert_eq!(root.attribute("style"), Some("font-family:\"A & B\""));

	let text = root.first_element_child().expect("text element");

	assert_eq!(text.text(), Some("var(--_fg) & <literal>"));
	assert_eq!(text.attribute("fill"), Some("#dee2ea"));

	let path = root.last_element_child().expect("path element");

	assert_eq!(path.attribute("stroke"), Some("#212833"));
}

#[test]
fn light_and_dark_diagrams_keep_the_same_geometry_and_authored_fills() {
	let source = "flowchart LR\nA[Request] --> B[Response]\nstyle A fill:#ff1234";
	let dark = mermaid::compile(source, Theme::Dark.into()).expect("dark diagram");
	let light = mermaid::compile(source, Theme::Light.into()).expect("light diagram");
	let tokyo = mermaid::compile(source, ColorScheme::new(Theme::Dark, ColorTheme::TokyoNight))
		.expect("Tokyo Night diagram");

	assert_eq!((light.width, light.height), (dark.width, dark.height));
	assert_eq!((tokyo.width, tokyo.height), (dark.width, dark.height));
	assert!(light.svg.contains("#252c39"));
	assert!(dark.svg.contains("#dee2ea"));
	assert!(tokyo.svg.contains("#c0caf5"));

	for diagram in [light, dark, tokyo] {
		assert!(diagram.svg.contains("#ff1234"), "authored fills remain literal");
	}
}

#[test]
fn rectangle_corners_use_svg_geometry_and_keep_authored_overrides() {
	let input = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="40" height="24" rx="2" style="rx:var(--ondine-radius,0);rx:8px;ry:3px;fill:var(--ondine-surface)"/></svg>"#;
	let svg = svg::desktop(input, Theme::Dark.into()).expect("desktop corners");
	let document = Document::parse(&svg).expect("valid SVG");
	let rect = document.root_element().first_element_child().expect("rect");

	assert_eq!(rect.attribute("rx"), Some("8px"));
	assert_eq!(rect.attribute("ry"), Some("3px"));
	assert!(rect.attribute("style").is_some_and(|style| !style.contains("rx:")));
}

#[test]
fn authored_styles_remain_and_links_do_not_become_interactive_resources() {
	let rendered = mermaid::compile(
		"flowchart TD\nA --> B\nstyle A fill:#f00\nclick A \"https://example.test\"",
		Theme::Dark.into(),
	)
	.expect("styled diagram");
	let document = Document::parse(&rendered.svg).expect("valid SVG");

	assert!(
		document
			.descendants()
			.any(|node| node.attributes().any(|attribute| attribute.value().contains("#f00")))
	);
	assert!(rendered.links.is_empty());
	assert!(
		!document
			.descendants()
			.any(|node| matches!(node.tag_name().name(), "a" | "image" | "script"))
	);
}

#[test]
fn invalid_and_excessive_diagrams_return_to_the_source_fallback() {
	for source in [
		"not a diagram".into(),
		"flowchart TD\nA[unclosed".into(),
		"x".repeat(50_001),
		format!("flowchart TD\n{}", "A --> B\n".repeat(513)),
	] {
		assert!(mermaid::compile(&source, Theme::Dark.into()).is_err());
	}

	assert!(mermaid::canvas_fits(1_024., 1_024.));

	for (width, height) in
		[(0., 10.), (f64::NAN, 10.), (f64::INFINITY, 10.), (4_097., 1.), (2_048., 2_048.)]
	{
		assert!(!mermaid::canvas_fits(width, height));
	}
}
