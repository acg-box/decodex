//! Cached ZenQuotes prompts with a curated, attributed offline collection.
use std::{
	collections::{BTreeSet, hash_map::RandomState},
	env, fs,
	hash::BuildHasher,
	io::Read as _,
	process,
	sync::atomic::Ordering,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};

static CURATED: std::sync::LazyLock<Vec<Quote>> = std::sync::LazyLock::new(|| {
	serde_json::from_str(include_str!("../../../assets/quotes/zenquotes-curated.json"))
		.expect("checked-in quote collection")
});
static QUOTES: std::sync::LazyLock<std::sync::Mutex<Vec<Quote>>> = std::sync::LazyLock::new(|| {
	std::sync::Mutex::new(read_cache().map(|cache| cache.quotes).unwrap_or_default())
});
static SESSION_TEXT: std::sync::LazyLock<String> = std::sync::LazyLock::new(choose_quote);

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Quote {
	pub q: String,
	pub a: String,
}
impl Quote {
	fn display(&self) -> String {
		format!("{} — {}", self.q, self.a)
	}
}

#[derive(Deserialize, Serialize)]
struct Cache {
	fetched: u64,
	quotes: Vec<Quote>,
}

/// Keep the placeholder stable across windows, conversations, and cache refreshes.
pub(super) fn session_quote() -> String {
	SESSION_TEXT.clone()
}

fn choose_quote() -> String {
	let entropy = RandomState::new().hash_one(super::unique_command()) as usize;
	let quotes = QUOTES.lock().unwrap_or_else(|error| error.into_inner());
	let choices = if quotes.is_empty() { CURATED.as_slice() } else { quotes.as_slice() };
	choices[entropy % choices.len()].display()
}

/// Refresh outside the UI thread. Updated quotes are used on the next app launch.
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

fn cache_path() -> Option<std::path::PathBuf> {
	Some(std::path::PathBuf::from(env::var_os("HOME")?).join("Library/Caches/Decodex/quotes.json"))
}

fn now() -> u64 {
	SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

fn filter_quotes(quotes: Vec<Quote>) -> Vec<Quote> {
	let mut seen = BTreeSet::new();

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

	if fs::metadata(&path).ok()?.len() > 128 * 1_024 {
		return None;
	}

	let mut cache: Cache = serde_json::from_slice(&fs::read(path).ok()?).ok()?;

	cache.quotes = filter_quotes(cache.quotes);

	(!cache.quotes.is_empty()).then_some(cache)
}

fn fetch_cache() -> Option<()> {
	let client =
		Client::builder().timeout(Duration::from_secs(6)).redirect(Policy::none()).build().ok()?;
	let response =
		client.get("https://zenquotes.io/api/quotes").send().ok()?.error_for_status().ok()?;
	let mut bytes = Vec::new();

	response.take(128 * 1_024 + 1).read_to_end(&mut bytes).ok()?;

	if bytes.len() > 128 * 1_024 {
		return None;
	}

	let quotes = filter_quotes(serde_json::from_slice(&bytes).ok()?);

	if quotes.is_empty() {
		return None;
	}

	*QUOTES.lock().unwrap_or_else(|error| error.into_inner()) = quotes.clone();

	let cache = Cache { fetched: now(), quotes };
	let path = cache_path()?;

	fs::create_dir_all(path.parent()?).ok()?;

	let staging = path.with_extension(format!("{}.tmp", process::id()));

	fs::write(&staging, serde_json::to_vec(&cache).ok()?).ok()?;
	fs::rename(staging, path).ok()?;

	Some(())
}

#[cfg(test)]
mod tests {
	use crate::shell::agent_surface::prompts::{self, CURATED, Quote};
	#[test]
	fn remote_quotes_reject_markup_long_text_and_duplicates() {
		let quote = |q: &str, a: &str| Quote { q: q.into(), a: a.into() };
		let result = prompts::filter_quotes(vec![
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
		assert!(prompts::refresh_cache());
		assert!(prompts::read_cache().is_some());

		let text = prompts::session_quote();

		assert!(text.contains(" — "));
		assert_eq!(prompts::session_quote(), text);
	}

	#[test]
	fn offline_quotes_are_short_attributed_and_stable() {
		for quote in CURATED.iter() {
			assert!(quote.q.is_ascii() && quote.q.len() <= 80);
			assert!(!quote.a.is_empty());
			assert!(quote.display().ends_with(&quote.a));
		}

		let first = prompts::session_quote();

		assert_eq!(prompts::session_quote(), first);
		assert_eq!(
			Quote { q: "Keep asking questions.".into(), a: "Example".into() }.display(),
			"Keep asking questions. — Example"
		);
	}
}
