//! Cached ZenQuotes prompts with a curated, attributed offline collection.
use std::{collections::hash_map::RandomState, hash::BuildHasher, sync::atomic::Ordering};

static CURATED: std::sync::LazyLock<Vec<Quote>> = std::sync::LazyLock::new(|| {
	serde_json::from_str(include_str!("../../../assets/quotes/zenquotes-curated.json"))
		.expect("checked-in quote collection")
});

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Quote {
	pub q: String,
	pub a: String,
}
impl Quote {
	fn display(&self) -> String {
		format!("“{}” — {}", self.q, self.a)
	}
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Cache {
	fetched: u64,
	quotes: Vec<Quote>,
}

static QUOTES: std::sync::LazyLock<std::sync::Mutex<Vec<Quote>>> = std::sync::LazyLock::new(|| {
	std::sync::Mutex::new(read_cache().map(|cache| cache.quotes).unwrap_or_default())
});
static LAST_TEXT: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub(super) fn next() -> String {
	let entropy = RandomState::new().hash_one(super::unique_command()) as usize;
	let mut last = LAST_TEXT.lock().unwrap_or_else(|error| error.into_inner());
	let quotes = QUOTES.lock().unwrap_or_else(|error| error.into_inner());
	let mut choices: Vec<String> =
		quotes.iter().map(Quote::display).filter(|text| text != &*last).collect();
	if choices.is_empty() {
		choices = CURATED.iter().map(Quote::display).filter(|text| text != &*last).collect();
	}

	let text = choices[entropy % choices.len()].to_owned();
	last.clone_from(&text);
	text
}

fn cache_path() -> Option<std::path::PathBuf> {
	Some(
		std::path::PathBuf::from(std::env::var_os("HOME")?)
			.join("Library/Caches/Decodex/quotes.json"),
	)
}
fn now() -> u64 {
	std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}
fn filter_quotes(quotes: Vec<Quote>) -> Vec<Quote> {
	let mut seen = std::collections::BTreeSet::new();
	quotes
		.into_iter()
		.filter(|quote| {
			(8..=65).contains(&quote.q.len())
				&& quote.q.is_ascii()
				&& !quote.q.chars().any(char::is_control)
				&& !quote.q.contains(['<', '>'])
				&& !quote.a.is_empty()
				&& quote.a.len() <= 80
				&& !quote.a.chars().any(char::is_control)
				&& quote.a.is_ascii()
				&& !quote.a.contains(['<', '>'])
				&& seen.insert(quote.q.clone())
		})
		.take(50)
		.collect()
}
fn read_cache() -> Option<Cache> {
	let path = cache_path()?;
	if std::fs::metadata(&path).ok()?.len() > 128 * 1024 {
		return None;
	}
	let mut cache: Cache = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
	cache.quotes = filter_quotes(cache.quotes);
	(!cache.quotes.is_empty()).then_some(cache)
}

/// Refresh outside the UI thread. Never replace a visible prompt while editing.
pub(super) fn refresh_cache() -> bool {
	static LAST_ATTEMPT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
	static REFRESHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
	if REFRESHING.swap(true, Ordering::Relaxed) {
		return false;
	}
	if now().saturating_sub(LAST_ATTEMPT.load(Ordering::Relaxed)) < 60 {
		REFRESHING.store(false, Ordering::Relaxed);
		return false;
	}
	LAST_ATTEMPT.store(now(), Ordering::Relaxed);
	let success = fetch_cache().is_some();
	REFRESHING.store(false, Ordering::Relaxed);
	success
}
fn fetch_cache() -> Option<()> {
	let client = reqwest::blocking::Client::builder()
		.timeout(std::time::Duration::from_secs(6))
		.redirect(reqwest::redirect::Policy::none())
		.build()
		.ok()?;
	let response =
		client.get("https://zenquotes.io/api/quotes").send().ok()?.error_for_status().ok()?;
	use std::io::Read;
	let mut bytes = Vec::new();
	response.take(128 * 1024 + 1).read_to_end(&mut bytes).ok()?;
	if bytes.len() > 128 * 1024 {
		return None;
	}
	let quotes = filter_quotes(serde_json::from_slice(&bytes).ok()?);
	if quotes.is_empty() {
		return None;
	}
	*QUOTES.lock().unwrap_or_else(|error| error.into_inner()) = quotes.clone();
	let cache = Cache { fetched: now(), quotes };
	let path = cache_path()?;
	std::fs::create_dir_all(path.parent()?).ok()?;
	let staging = path.with_extension(format!("{}.tmp", std::process::id()));
	std::fs::write(&staging, serde_json::to_vec(&cache).ok()?).ok()?;
	std::fs::rename(staging, path).ok()?;
	Some(())
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn remote_quotes_reject_markup_long_text_and_duplicates() {
		let quote = |q: &str, a: &str| Quote { q: q.into(), a: a.into() };
		let result = filter_quotes(vec![
			quote("Keep asking questions.", "Example"),
			quote("Keep asking questions.", "Example"),
			quote("<script>bad</script>", "Example"),
			quote(&"x".repeat(66), "Example"),
			quote("A valid short phrase.", "<b>Author</b>"),
		]);
		assert_eq!(result.len(), 1);
	}
	#[test]
	#[ignore = "Requires the public ZenQuotes service; run explicitly for live integration verification"]
	fn live_quote_cache_fetch() {
		assert!(refresh_cache());
		assert!(read_cache().is_some());
		let text = next();
		assert!(text.contains(" — "));
		assert_ne!(next(), text);
	}

	#[test]
	fn offline_quotes_are_short_attributed_and_non_repeating() {
		assert_eq!(CURATED.len(), 12);
		for quote in CURATED.iter() {
			assert!(quote.q.is_ascii() && quote.q.len() <= 80);
			assert!(!quote.a.is_empty());
			assert!(quote.display().ends_with(&quote.a));
		}
		let first = next();
		assert_ne!(next(), first);
	}
}
