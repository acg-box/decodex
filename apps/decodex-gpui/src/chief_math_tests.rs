//! Product behavior tests for upstream math parsing and source-preserving copy.
use super::*;

fn plain(source: &str) -> String {
	let mut out = Inline::default();
	append_inline(&parse(source), HighlightStyle::default(), None, &mut out);
	out.text
}

#[test]
fn formulas_render_without_changing_protected_markdown_or_copy_source() {
	for (source, expected) in [
		(r"Inline $\alpha^2 + \beta_{10}$", "Inline α² + β₁₀"),
		(r"$\hat H^2 + \hbar^2$", "Ĥ² + ℏ²"),
		(r"$\left\langle\vec{v}_i\right\rangle$", "⟨v⃗ᵢ⟩"),
		(r"$$\frac{a+b}{c+d}$$", "a+b\n───\nc+d"),
		(
			r"`$\alpha$` $HOME ${HOME} $(echo x) $5 and $10",
			r"$\alpha$ $HOME ${HOME} $(echo x) $5 and $10",
		),
		(r"$\unknown{x}$", r"$\unknown{x}$"),
		(r"$$\frac{\frac{a}{b}}{c}$$", r"$$\frac{\frac{a}{b}}{c}$$"),
		("$$\n# x\n- y\n", "$$\n# x\n- y\n"),
	] {
		assert_eq!(plain(source), expected, "{source}");
	}
	let source = r"**$\alpha^2$** and $\hat H$";
	let html = clipboard::html(source);
	assert!(html.contains(r"<strong>$\alpha^2$</strong>"));
	assert!(html.contains(r"$\hat H$"));
	assert!(!html.contains('α'));
}

#[test]
fn math_keeps_link_ranges_and_code_bytes_correct() {
	let source = r"$\alpha$ [中文 source](/tmp/$path.rs:12) then $x^2$.";
	let mut out = Inline::default();
	append_inline(&parse(source), HighlightStyle::default(), None, &mut out);
	assert_eq!(out.links.len(), 1);
	let (range, target) = &out.links[0];
	assert_eq!(&out.text[range.clone()], "中文 source");
	assert_eq!(target, "/tmp/$path.rs:12");
	assert!(clipboard::html("```tex\r\n\\frac{a}{b}  \r\n``` ").contains("\\frac{a}{b}  \r\n"));
}

#[test]
fn growing_display_stays_literal_until_closed_and_bounds_preserve_following_text() {
	let source = "$$\n\\frac{a+b}{c+d}\n$$";
	for (offset, _) in source.char_indices().filter(|(offset, _)| *offset >= 2) {
		assert_eq!(plain(&source[..offset]), source[..offset]);
	}
	assert_eq!(plain(source), "a+b\n───\nc+d");
	let oversized = format!("$$\n{}\n$$\n\nAfter $\\alpha$.", "x".repeat(5000));
	assert!(plain(&oversized).ends_with("After α."));
	for formula in [r"\hat{xy}", r"\ddot{ }", r"{a+b}^2", r"\sqrt[3]{x}"] {
		let source = format!("\\({formula}\\)");
		assert_eq!(plain(&source), source);
	}
}

struct Preview {
	text: String,
}
impl gpui::Render for Preview {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		super::render(&self.text, "math-preview")
	}
}
#[gpui::test]
fn display_math_keeps_geometry_on_resize_and_copies_tex(cx: &mut gpui::TestAppContext) {
	let source = r"$$\frac{a+b+c+d+e+f+g+h+i+j+k+l+m+n+o+p}{g+h}$$";
	let (_, visual) = cx.add_window_view(|_, _| Preview { text: source.into() });
	let mut previous_size = None;
	for width in [600., 120.] {
		visual.simulate_resize(gpui::size(px(width), px(300.)));
		visual.update(|window, cx| {
			window.draw(cx).clear();
		});
		let bounds = visual.debug_bounds("math-math-preview-0-formula-0").expect("formula");
		assert!(bounds.size.width <= px(width));
		let text = visual.debug_bounds("math-text-math-preview-0-formula-0").expect("formula text");
		if let Some(size) = previous_size {
			assert_eq!(text.size, size, "spatial rows must not wrap");
		}
		previous_size = Some(text.size);
		if width == 120. {
			visual.simulate_event(gpui::ScrollWheelEvent {
				position: bounds.center(),
				delta: gpui::ScrollDelta::Pixels(gpui::point(px(-60.), px(0.))),
				..Default::default()
			});
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
			let moved =
				visual.debug_bounds("math-text-math-preview-0-formula-0").expect("scrolled text");
			assert!(
				moved.origin.x < text.origin.x,
				"long formula must remain horizontally accessible"
			);
			visual.simulate_event(gpui::ScrollWheelEvent {
				position: bounds.center(),
				delta: gpui::ScrollDelta::Pixels(gpui::point(px(60.), px(0.))),
				..Default::default()
			});
			visual.update(|window, cx| {
				window.draw(cx).clear();
			});
		}

		let copy = visual.debug_bounds("math-copy-math-preview-0-formula-0").expect("copy");
		visual.simulate_click(copy.center(), gpui::Modifiers::default());
		visual.update(|_, cx| {
			assert_eq!(cx.read_from_clipboard().and_then(|v| v.text()), Some(source.into()))
		});
	}
}
