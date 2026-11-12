//! Cached official API prices. Price reads never delay conversation startup or send.
mod estimate;

use std::{
	collections::BTreeMap,
	path::PathBuf,
	sync::Arc,
	time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub(crate) use estimate::calculate;

pub(crate) const SOURCE: &str = "https://developers.openai.com/api/docs/pricing.md";
const MAX_BYTES: usize = 262_144;
const DAY_MICROS: i64 = 86_400_000_000;

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Catalog {
	pub source: String,
	pub checked_at_micros: i64,
	pub rates: BTreeMap<String, BTreeMap<String, Rates>>,
}

/// Integer USD micros per million tokens: input, cached, 30m write, 12h write, output.
#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Rates {
	pub short: [Option<u64>; 5],
	pub long: Option<[Option<u64>; 5]>,
}

#[derive(Clone)]
pub(crate) struct Prices {
	path: PathBuf,
	state: Arc<Mutex<State>>,
}

struct State {
	catalog: Catalog,
	loaded: bool,
	next_refresh: i64,
	refreshing: bool,
}

impl Prices {
	pub(crate) fn new(path: PathBuf) -> Self {
		let catalog = serde_json::from_str(include_str!("agent_api_pricing/official-prices.json"))
			.expect("bundled official API prices");
		Self {
			path,
			state: Arc::new(Mutex::new(State {
				catalog,
				loaded: false,
				next_refresh: 0,
				refreshing: false,
			})),
		}
	}

	pub(crate) async fn snapshot(&self) -> Catalog {
		let mut state = self.state.lock().await;
		if !state.loaded {
			state.loaded = true;
			let path = self.path.clone();
			let cached = tokio::task::spawn_blocking(move || {
				if std::fs::metadata(&path).ok()?.len() > MAX_BYTES as u64 {
					return None;
				}
				let catalog: Catalog = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
				catalog.valid().then_some(catalog)
			})
			.await
			.ok()
			.flatten();
			if let Some(catalog) = cached {
				state.next_refresh = catalog.checked_at_micros.saturating_add(DAY_MICROS);
				state.catalog = catalog;
			}
		}
		if now() >= state.next_refresh && !state.refreshing {
			state.refreshing = true;
			let this = self.clone();
			tokio::spawn(async move {
				let updated = fetch().await;
				if let Some(catalog) = &updated
					&& let Ok(bytes) = serde_json::to_vec(catalog)
				{
					let path = this.path.clone();
					let _ = tokio::task::spawn_blocking(move || {
						let temporary = path.with_extension("json.tmp");
						std::fs::write(&temporary, bytes)?;
						std::fs::rename(temporary, path)
					})
					.await;
				}
				let mut state = this.state.lock().await;
				state.next_refresh = now().saturating_add(if updated.is_some() {
					DAY_MICROS
				} else {
					3_600_000_000
				});
				if let Some(catalog) = updated {
					state.catalog = catalog;
				}
				state.refreshing = false;
			});
		}
		state.catalog.clone()
	}
}

impl Catalog {
	fn valid(&self) -> bool {
		self.source == SOURCE
			&& self.checked_at_micros > 0
			&& self.rates.len() >= 4
			&& self.rates.get("gpt-6.1-sol").is_some_and(|m| m.contains_key("standard"))
	}

	pub(crate) fn parse(markdown: &str, checked_at_micros: i64) -> Option<Self> {
		let mut rates: BTreeMap<String, BTreeMap<String, Rates>> = BTreeMap::new();
		let mut speed = None;
		let mut columns = None;
		for line in markdown.lines() {
			if line.starts_with("### ") {
				columns = None;
				speed = match line.trim() {
					"### Standard pricing data" => Some("standard"),
					"### Fast pricing data" => Some("fast"),
					"### Ultrafast pricing data" => Some("ultrafast"),
					_ => None,
				};
			}
			if line.starts_with("#### ") {
				speed = None;
				columns = None;
			}
			let Some(speed) = speed else { continue };
			if !line.starts_with('|') {
				continue;
			}
			let cells: Vec<_> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
			if cells.first() == Some(&"Model") {
				// A changed table layout must keep the last verified prices rather
				// than silently applying an input price to another token category.
				columns = Some(match cells.as_slice() {
					["Model", "Input", "Output", "Cached input"] => 4,
					[
						"Model",
						"Short context input",
						"Short context output",
						"Short context cached input",
						"Short context 30m write",
						"Short context 12h write",
						"Long context input",
						"Long context output",
						"Long context cached input",
						"Long context 30m write",
						"Long context 12h write",
					] => 11,
					_ => return None,
				});
				continue;
			}
			if Some(cells.len()) != columns {
				continue;
			}
			let model = cells[0].split_whitespace().next()?;
			if !model.starts_with("gpt-") && !model.starts_with("o3") && !model.starts_with("o1") {
				continue;
			}
			let values = cells[1..].iter().map(|cell| money(cell)).collect::<Option<Vec<_>>>()?;
			let (short, long) = if values.len() == 10 {
				(
					[values[0], values[2], values[3], values[4], values[1]],
					Some([values[5], values[7], values[8], values[9], values[6]]),
				)
			} else {
				([values[0], values[2], None, None, values[1]], None)
			};
			rates.entry(model.into()).or_default().insert(speed.into(), Rates { short, long });
		}
		let result = Self { source: SOURCE.into(), checked_at_micros, rates };
		result.valid().then_some(result)
	}
}

fn money(value: &str) -> Option<Option<u64>> {
	if value == "-" {
		return Some(None);
	}
	let value = value.strip_prefix('$')?;
	let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
	if fraction.len() > 6 || !fraction.bytes().all(|c| c.is_ascii_digit()) {
		return None;
	}
	Some(Some(
		whole
			.parse::<u64>()
			.ok()?
			.checked_mul(1_000_000)?
			.checked_add(format!("{fraction:0<6}").parse::<u64>().ok()?)?,
	))
}

async fn fetch() -> Option<Catalog> {
	let client = reqwest::Client::builder()
		.timeout(std::time::Duration::from_secs(8))
		.redirect(reqwest::redirect::Policy::none())
		.build()
		.ok()?;
	let mut response = client.get(SOURCE).send().await.ok()?.error_for_status().ok()?;
	let mut bytes = Vec::new();
	while let Some(chunk) = response.chunk().await.ok()? {
		if bytes.len() + chunk.len() > MAX_BYTES {
			return None;
		}
		bytes.extend_from_slice(&chunk);
	}
	Catalog::parse(std::str::from_utf8(&bytes).ok()?, now())
}

pub(crate) fn now() -> i64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.ok()
		.and_then(|d| i64::try_from(d.as_micros()).ok())
		.unwrap_or(0)
}

#[cfg(test)]
mod tests {
	use super::*;
	#[tokio::test]
	async fn fresh_cached_prices_are_available_without_a_network_refresh() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("prices.json");
		let mut catalog: Catalog =
			serde_json::from_str(include_str!("agent_api_pricing/official-prices.json")).unwrap();
		catalog.checked_at_micros = now();
		std::fs::write(&path, serde_json::to_vec(&catalog).unwrap()).unwrap();
		let prices = Prices::new(path);
		assert_eq!(prices.snapshot().await.checked_at_micros, catalog.checked_at_micros);
		assert!(!prices.state.lock().await.refreshing);
	}
	#[test]
	fn official_price_columns_preserve_speed_context_and_decimal_precision() {
		let text = "### Standard pricing data\n\
| Model | Short context input | Short context output | Short context cached input | Short context 30m write | Short context 12h write | Long context input | Long context output | Long context cached input | Long context 30m write | Long context 12h write |\n\
| gpt-6.1-sol | $2 | $10 | $0.10 | $2.5 | $5 | $4 | $15 | $0.2 | $5 | $10 |\n\
| gpt-6-astra | $10 | $50 | $1 | $12.5 | $25 | $20 | $75 | $2 | $25 | $50 |\n\
| gpt-6.1-terra | $1 | $5 | $0.05 | $1.25 | $2.5 | $2 | $7.5 | $0.1 | $2.5 | $5 |\n\
| gpt-6-luna | $0.1 | $0.5 | $0.01 | $0.125 | $0.25 | $0.2 | $0.75 | $0.02 | $0.25 | $0.5 |\n\
### Fast pricing data\n\
| Model | Short context input | Short context output | Short context cached input | Short context 30m write | Short context 12h write | Long context input | Long context output | Long context cached input | Long context 30m write | Long context 12h write |\n\
| gpt-6.1-sol | $4 | $20 | $0.2 | $5 | $10 | $8 | $30 | $0.4 | $10 | $20 |";
		let catalog = Catalog::parse(text, 1).unwrap();
		assert_eq!(catalog.rates["gpt-6.1-sol"]["standard"].short[1], Some(100_000));
		assert_eq!(catalog.rates["gpt-6.1-sol"]["fast"].long.unwrap()[4], Some(30_000_000));
		assert!(Catalog::parse(&text.replace("$0.10", "changed"), 1).is_none());
		assert!(Catalog::parse(&text.replace("Short context input", "Input changed"), 1).is_none());
		assert!(Catalog::parse("upstream page unavailable", 1).is_none());
	}
}
