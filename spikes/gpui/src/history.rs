use std::collections::{HashMap, VecDeque};

pub const GIB: usize = 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct HistorySpec {
	pub logical_bytes: usize,
	pub message_bytes: usize,
	pub page_messages: usize,
	pub max_cached_pages: usize,
}

impl HistorySpec {
	pub const fn large_fixture() -> Self {
		Self {
			logical_bytes: 3 * GIB,
			message_bytes: 32 * 1024,
			page_messages: 64,
			max_cached_pages: 4,
		}
	}

	pub const fn message_count(self) -> usize {
		self.logical_bytes / self.message_bytes
	}

	pub const fn cache_limit_bytes(self) -> usize {
		self.message_bytes * self.page_messages * self.max_cached_pages
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HistoryStats {
	pub cached_pages: usize,
	pub cached_bytes: usize,
	pub peak_cached_bytes: usize,
	pub generated_messages: usize,
	pub page_misses: usize,
}

pub struct PagedHistory {
	spec: HistorySpec,
	pages: HashMap<usize, Vec<String>>,
	lru: VecDeque<usize>,
	stats: HistoryStats,
}

impl PagedHistory {
	pub fn new(spec: HistorySpec) -> Self {
		assert!(spec.message_bytes >= 64);
		assert!(spec.page_messages > 0);
		assert!(spec.max_cached_pages > 0);
		Self { spec, pages: HashMap::new(), lru: VecDeque::new(), stats: HistoryStats::default() }
	}

	pub const fn spec(&self) -> HistorySpec {
		self.spec
	}

	pub const fn stats(&self) -> HistoryStats {
		self.stats
	}

	pub fn message_preview(&mut self, index: usize) -> String {
		assert!(index < self.spec.message_count());
		let page_index = index / self.spec.page_messages;
		self.ensure_page(page_index);
		let page_offset = index % self.spec.page_messages;
		let message = &self.pages[&page_index][page_offset];
		let preview_end = message.floor_char_boundary(56.min(message.len()));
		format!("#{index:06} {}", &message[..preview_end])
	}

	fn ensure_page(&mut self, page_index: usize) {
		if self.pages.contains_key(&page_index) {
			self.touch(page_index);
			return;
		}

		let start = page_index * self.spec.page_messages;
		let remaining = self.spec.message_count().saturating_sub(start);
		let count = remaining.min(self.spec.page_messages);
		let page = (0..count)
			.map(|offset| synthetic_message(start + offset, self.spec.message_bytes))
			.collect::<Vec<_>>();
		self.stats.generated_messages += count;
		self.stats.page_misses += 1;
		self.pages.insert(page_index, page);
		self.lru.push_back(page_index);

		while self.pages.len() > self.spec.max_cached_pages {
			if let Some(evicted) = self.lru.pop_front() {
				self.pages.remove(&evicted);
			}
		}
		self.update_resident_stats();
	}

	fn touch(&mut self, page_index: usize) {
		if let Some(position) = self.lru.iter().position(|candidate| *candidate == page_index) {
			self.lru.remove(position);
		}
		self.lru.push_back(page_index);
	}

	fn update_resident_stats(&mut self) {
		self.stats.cached_pages = self.pages.len();
		self.stats.cached_bytes =
			self.pages.values().flat_map(|page| page.iter()).map(String::len).sum();
		self.stats.peak_cached_bytes = self.stats.peak_cached_bytes.max(self.stats.cached_bytes);
	}
}

fn synthetic_message(index: usize, bytes: usize) -> String {
	let prefix = format!("conversation-message-{index:08} ");
	let mut message = String::with_capacity(bytes);
	message.push_str(&prefix);
	message.extend(std::iter::repeat_n('x', bytes - prefix.len()));
	message
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn multi_gib_history_is_paged_and_bounded() {
		let spec = HistorySpec::large_fixture();
		let mut history = PagedHistory::new(spec);
		let count = spec.message_count();

		for step in 0..128 {
			let index = step * (count - 1) / 127;
			let preview = history.message_preview(index);
			assert!(preview.starts_with(&format!("#{index:06}")));
			assert!(history.stats().cached_pages <= spec.max_cached_pages);
			assert!(history.stats().cached_bytes <= spec.cache_limit_bytes());
		}

		let stats = history.stats();
		assert_eq!(spec.logical_bytes, 3 * GIB);
		assert_eq!(spec.message_count(), 98_304);
		assert_eq!(stats.cached_pages, 4);
		assert_eq!(stats.peak_cached_bytes, spec.cache_limit_bytes());
		assert!(stats.generated_messages < spec.message_count());
	}
}
