//! Reuse immutable parsing across scroll frames; streaming text has a new key.
use crate::shell::agent_surface::markdown::{HashMap, Node, OnceCell, Rc, RefCell};

thread_local! {
	static DOCUMENTS: RefCell<Cache> = RefCell::new(Cache::default());
}

const MAX_DOCUMENTS: usize = 256;
const MAX_SOURCE_BYTES: usize = 512 * 1_024;

#[derive(Default)]
pub(super) struct Document {
	pub(super) nodes: OnceCell<Vec<Node>>,
	pub(super) plain: OnceCell<String>,
}

#[derive(Default)]
struct Cache {
	documents: HashMap<Rc<str>, (u64, Rc<Document>)>,
	source_bytes: usize,
	clock: u64,
}
impl Cache {
	fn document(&mut self, text: &str) -> Rc<Document> {
		self.clock += 1;

		if let Some((used, document)) = self.documents.get_mut(text) {
			*used = self.clock;

			return document.clone();
		}

		let document = Rc::new(Document::default());

		if text.len() > MAX_SOURCE_BYTES {
			return document;
		}

		while self.documents.len() >= MAX_DOCUMENTS
			|| self.source_bytes + text.len() > MAX_SOURCE_BYTES
		{
			let oldest = self
				.documents
				.iter()
				.min_by_key(|(_, (used, _))| *used)
				.map(|(source, _)| source.clone())
				.expect("cache is full");

			self.source_bytes -= oldest.len();

			self.documents.remove(&oldest);
		}

		self.source_bytes += text.len();

		self.documents.insert(text.into(), (self.clock, document.clone()));

		document
	}
}

pub(super) fn document(text: &str) -> Rc<Document> {
	DOCUMENTS.with(|cache| cache.borrow_mut().document(text))
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::{markdown, markdown::cache::*};

	use std::{hint, time::Instant};

	#[test]
	#[ignore = "Manual parsing benchmark; not a display FPS measurement"]
	fn repeated_transcript_parsing_benchmark() {
		let messages: Vec<_> = (0..32).map(|i| format!(
			"## Response {i}\n\n{}", "Read **the result**, then review `src/main.rs`.\n\n- First item\n- Second item\n\n".repeat(12)
		)).collect();
		let start = Instant::now();

		for _ in 0..60 {
			for text in &messages {
				hint::black_box(markdown::parse(hint::black_box(text)));
				hint::black_box(markdown::parse_plain_text(hint::black_box(text)));
			}
		}

		let uncached = start.elapsed();
		let mut cache = Cache::default();
		let start = Instant::now();

		for _ in 0..60 {
			for text in &messages {
				let document = cache.document(hint::black_box(text));

				hint::black_box(document.nodes.get_or_init(|| markdown::parse(text)));
				hint::black_box(document.plain.get_or_init(|| markdown::parse_plain_text(text)));
			}
		}

		eprintln!(
			"32 messages x 60 frames: parsing before={uncached:?}, cached={:?}",
			start.elapsed()
		);
	}

	#[test]
	fn scrolling_reuses_parsing_and_stream_changes_do_not_reuse_stale_text() {
		let mut cache = Cache::default();
		let first = cache.document("**Hello**");

		first.nodes.set(markdown::parse("**Hello**")).unwrap();
		first.plain.set(markdown::parse_plain_text("**Hello**")).unwrap();

		let frame = cache.document("**Hello**");

		assert!(Rc::ptr_eq(&first, &frame));
		assert_eq!(frame.plain.get().unwrap(), "Hello");

		let streamed = cache.document("**Hello** world");

		assert!(!Rc::ptr_eq(&first, &streamed));
		assert_eq!(
			streamed.plain.get_or_init(|| markdown::parse_plain_text("**Hello** world")),
			"Hello world"
		);
	}

	#[test]
	fn cache_is_bounded_and_keeps_recent_documents() {
		let mut cache = Cache::default();
		let recent = cache.document("visible");

		for index in 0..MAX_DOCUMENTS * 2 {
			cache.document(&format!("message {index}"));

			assert!(Rc::ptr_eq(&recent, &cache.document("visible")));
		}

		assert_eq!(cache.documents.len(), MAX_DOCUMENTS);

		for index in 0..8 {
			cache.document(&format!("{index}{}", "x".repeat(MAX_SOURCE_BYTES / 3)));
		}

		assert!(cache.source_bytes <= MAX_SOURCE_BYTES);

		let count = cache.documents.len();

		cache.document(&"x".repeat(MAX_SOURCE_BYTES + 1));

		assert_eq!(cache.documents.len(), count);
	}
}
