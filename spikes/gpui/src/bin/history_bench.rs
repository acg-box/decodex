use std::time::Instant;

use decodex_gpui_spike::history::{HistorySpec, PagedHistory};

fn main() {
	let spec = HistorySpec::large_fixture();
	let mut history = PagedHistory::new(spec);
	let started = Instant::now();
	let probes = 128usize;

	for step in 0..probes {
		let index = step * (spec.message_count() - 1) / (probes - 1);
		std::hint::black_box(history.message_preview(index));
	}

	let stats = history.stats();
	println!(
		"{{\"logical_bytes\":{},\"message_count\":{},\"probes\":{},\"page_misses\":{},\"generated_messages\":{},\"cached_pages\":{},\"cached_bytes\":{},\"peak_cached_bytes\":{},\"cache_limit_bytes\":{},\"elapsed_micros\":{}}}",
		spec.logical_bytes,
		spec.message_count(),
		probes,
		stats.page_misses,
		stats.generated_messages,
		stats.cached_pages,
		stats.cached_bytes,
		stats.peak_cached_bytes,
		spec.cache_limit_bytes(),
		started.elapsed().as_micros()
	);
}
