//! Translate a displayed selection to a contiguous slice of the original Markdown.
use std::{ops::Range, rc::Rc};

#[derive(Default)]
pub(super) struct Source {
	pub original: Option<Rc<str>>,
	pub spans: Vec<(Range<usize>, Range<usize>)>,
}
impl Source {
	pub fn copy(&self, displayed: &str, selected: Range<usize>) -> String {
		let Some(original) = &self.original else { return displayed[selected].into() };

		if selected == (0..displayed.len()) {
			return original.to_string();
		}

		let mut start = self.boundary(original, displayed, selected.start, false);
		let mut end = self.boundary(original, displayed, selected.end, true);

		for (visible, source) in &self.spans {
			if visible.is_empty() {
				continue;
			}
			if visible.start == selected.start && visible.end <= selected.end {
				start = start.min(source.start);
			}
			if visible.end == selected.end && visible.start >= selected.start {
				end = end.max(self.end(original, displayed, visible, source));
			}
		}

		original[start..end].into()
	}

	fn end(
		&self,
		original: &str,
		displayed: &str,
		visible: &Range<usize>,
		source: &Range<usize>,
	) -> usize {
		if displayed[visible.clone()].ends_with('\n') {
			source.end
		} else {
			source.start + original[source.clone()].trim_end_matches(['\r', '\n']).len()
		}
	}

	fn boundary(&self, original: &str, displayed: &str, index: usize, end: bool) -> usize {
		let span = self
			.spans
			.iter()
			.filter(|(visible, _)| {
				if end {
					visible.start < index && index <= visible.end
				} else {
					visible.start <= index && index < visible.end
				}
			})
			.min_by_key(|(visible, _)| visible.len());
		let Some((visible, source)) = span else {
			return if end { original.len() } else { 0 };
		};
		let rendered = &displayed[visible.clone()];

		if let Some(offset) = original[source.clone()].find(rendered) {
			source.start + offset + index - visible.start
		} else if end {
			self.end(original, displayed, visible, source)
		} else {
			source.start
		}
	}
}
