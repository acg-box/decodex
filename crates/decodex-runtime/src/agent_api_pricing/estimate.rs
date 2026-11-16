//! Price each observed request with its recorded selection, never today's selected model.
use std::collections::BTreeMap;

use decodex_database::AgentCostObservation;
use decodex_protocol::ApiCostEstimate;
use serde::Deserialize;
use serde_json::Value;

use super::Catalog;

#[derive(Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Tokens {
	input_tokens: u64,
	cached_input_tokens: u64,
	#[serde(default)]
	cache_write_input_tokens: u64,
	output_tokens: u64,
}
impl Tokens {
	fn difference(&self, before: &Self) -> Option<Self> {
		Some(Self {
			input_tokens: self.input_tokens.checked_sub(before.input_tokens)?,
			cached_input_tokens: self
				.cached_input_tokens
				.checked_sub(before.cached_input_tokens)?,
			cache_write_input_tokens: self
				.cache_write_input_tokens
				.checked_sub(before.cache_write_input_tokens)?,
			output_tokens: self.output_tokens.checked_sub(before.output_tokens)?,
		})
	}
}

pub(crate) fn calculate(
	catalog: &Catalog,
	thread: &str,
	rows: &[AgentCostObservation],
	partial: bool,
) -> Option<ApiCostEstimate> {
	calculate_all(catalog, thread, rows, partial).0
}

pub(crate) fn calculate_turns(
	catalog: &Catalog,
	thread: &str,
	rows: &[AgentCostObservation],
	partial: bool,
) -> BTreeMap<String, ApiCostEstimate> {
	calculate_all(catalog, thread, rows, partial).1
}

#[derive(Default)]
struct Amount {
	numerator: u128,
	responses: u64,
	partial: bool,
}
impl Amount {
	fn add(&mut self, amount: Option<u128>, partial: bool) {
		self.partial |= partial || amount.is_none();
		if let Some(amount) = amount {
			match self.numerator.checked_add(amount) {
				Some(total) => {
					self.numerator = total;
					self.responses += 1;
				},
				None => self.partial = true,
			}
		}
	}

	fn finish(self, thread: &str, catalog: &Catalog) -> Option<ApiCostEstimate> {
		(self.responses > 0).then_some(())?;
		Some(ApiCostEstimate {
			thread_id: thread.into(),
			usd_micros: u64::try_from((self.numerator + 500_000) / 1_000_000).ok()?,
			priced_responses: self.responses,
			partial: self.partial,
			pricing_checked_at_micros: catalog.checked_at_micros,
		})
	}
}

fn calculate_all(
	catalog: &Catalog,
	thread: &str,
	rows: &[AgentCostObservation],
	partial: bool,
) -> (Option<ApiCostEstimate>, BTreeMap<String, ApiCostEstimate>) {
	let mut settings = Value::Null;
	let mut selections = BTreeMap::new();
	let mut previous = Tokens::default();
	let mut total = Amount { partial, ..Default::default() };
	let mut turns: BTreeMap<String, Amount> = BTreeMap::new();
	let mut malformed = partial;
	for row in rows {
		let Ok(value) = serde_json::from_str::<Value>(&row.payload) else {
			malformed = true;
			total.partial = true;
			continue;
		};
		match row.kind.as_str() {
			"native_task_models" => settings = value["settings"].clone(),
			"turn_execution" => {
				if let (Some(turn), Some(model)) =
					(value["turnId"].as_str(), value["execution"]["model"].as_str())
				{
					// Keep the selection at admission; a later dropdown change is not this
					// request's rate.
					let tier = value["execution"]["serviceTier"]
						.as_str()
						.map(str::to_owned)
						.or_else(|| settings["serviceTier"].as_str().map(str::to_owned))
						.or_else(|| settings.is_object().then(|| "default".into()));
					selections.insert(turn.to_owned(), (model.to_owned(), tier));
				}
			},
			"token_usage" => {
				let (Ok(current), Ok(last)) = (
					serde_json::from_value::<Tokens>(value["tokenUsage"]["total"].clone()),
					serde_json::from_value::<Tokens>(value["tokenUsage"]["last"].clone()),
				) else {
					malformed = true;
					total.partial = true;
					continue;
				};
				if current == previous {
					continue;
				}
				let incomplete = current.difference(&previous).as_ref() != Some(&last)
					|| last.cache_write_input_tokens > 0;
				previous = current;
				let turn = value["turnId"].as_str();
				let selected = turn.and_then(|turn| selections.get(turn));
				let amount = selected
					.and_then(|(model, tier)| price(catalog, &last, model, tier.as_deref()));
				total.add(amount, incomplete);
				if let Some(turn) = turn {
					turns.entry(turn.into()).or_default().add(amount, incomplete);
				}
			},
			_ => {},
		}
	}
	let turns = turns
		.into_iter()
		.filter_map(|(turn, mut amount)| {
			amount.partial |= malformed;
			amount.finish(thread, catalog).map(|estimate| (turn, estimate))
		})
		.collect();
	(total.finish(thread, catalog), turns)
}

fn price(catalog: &Catalog, tokens: &Tokens, model: &str, tier: Option<&str>) -> Option<u128> {
	let speed = match tier? {
		"default" | "standard" => "standard",
		"priority" | "fast" => "fast",
		"ultrafast" => "ultrafast",
		_ => return None,
	};
	let rates = catalog.rates.get(model)?.get(speed)?;
	let prices = if tokens.input_tokens > 272_000 {
		rates.long.as_ref().unwrap_or(&rates.short)
	} else {
		&rates.short
	};
	let input = tokens
		.input_tokens
		.checked_sub(tokens.cached_input_tokens)?
		.checked_sub(tokens.cache_write_input_tokens)?;
	let counts = [
		input,
		tokens.cached_input_tokens,
		tokens.cache_write_input_tokens,
		0,
		tokens.output_tokens,
	];
	counts.into_iter().zip(prices).try_fold(0_u128, |sum, (count, price)| {
		if count == 0 {
			return Some(sum);
		}
		sum.checked_add(u128::from(count) * u128::from((*price)?))
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	fn row(kind: &str, payload: Value) -> AgentCostObservation {
		AgentCostObservation { kind: kind.into(), payload: payload.to_string() }
	}
	#[test]
	fn prices_recorded_models_cached_tokens_and_speed_without_double_counting() {
		let catalog: Catalog = serde_json::from_str(include_str!("official-prices.json")).unwrap();
		let usage = |input, cached, output| serde_json::json!({"inputTokens":input,"cachedInputTokens":cached,"outputTokens":output});
		let mut rows = vec![
			row("native_task_models", serde_json::json!({"settings":{"serviceTier":"default"}})),
			row(
				"turn_execution",
				serde_json::json!({"turnId":"a","execution":{"model":"gpt-6.1-sol"}}),
			),
			row(
				"token_usage",
				serde_json::json!({"turnId":"a","tokenUsage":{"total":usage(1000,500,100),"last":usage(1000,500,100)}}),
			),
		];
		assert_eq!(calculate(&catalog, "thread", &rows, false).unwrap().usd_micros, 2_050);
		rows.push(row("token_usage", serde_json::json!({"turnId":"a","tokenUsage":{"total":usage(1000,500,100),"last":usage(1000,500,100)}})));
		rows.push(row(
			"native_task_models",
			serde_json::json!({"settings":{"serviceTier":"fast"}}),
		));
		rows.push(row(
			"turn_execution",
			serde_json::json!({"turnId":"b","execution":{"model":"gpt-6.1-terra"}}),
		));
		rows.push(row("token_usage", serde_json::json!({"turnId":"b","tokenUsage":{"total":usage(2000,1000,200),"last":usage(1000,500,100)}})));
		let estimate = calculate(&catalog, "thread", &rows, false).unwrap();
		assert_eq!(estimate.usd_micros, 4_100);
		assert_eq!(estimate.priced_responses, 2);
		assert!(!estimate.partial);
		let turns = calculate_turns(&catalog, "thread", &rows, false);
		assert_eq!(turns["a"].usd_micros, 2_050);
		assert_eq!(turns["b"].usd_micros, 2_050);
		assert_eq!(turns.values().map(|turn| turn.usd_micros).sum::<u64>(), estimate.usd_micros);
	}

	#[test]
	fn turn_price_retains_the_admitted_speed_when_settings_change_before_usage_arrives() {
		let catalog: Catalog = serde_json::from_str(include_str!("official-prices.json")).unwrap();
		let counts =
			serde_json::json!({"inputTokens":1000,"cachedInputTokens":500,"outputTokens":100});
		let rows = vec![
			row("native_task_models", serde_json::json!({"settings":{"serviceTier":"default"}})),
			row(
				"turn_execution",
				serde_json::json!({"turnId":"a","execution":{"model":"gpt-6.1-sol"}}),
			),
			row("native_task_models", serde_json::json!({"settings":{"serviceTier":"ultrafast"}})),
			row(
				"token_usage",
				serde_json::json!({"turnId":"a","tokenUsage":{"last":counts,"total":counts}}),
			),
		];
		assert_eq!(calculate_turns(&catalog, "thread", &rows, false)["a"].usd_micros, 2050);
	}
	#[test]
	fn missing_history_or_unknown_models_are_marked_as_partial() {
		let catalog: Catalog = serde_json::from_str(include_str!("official-prices.json")).unwrap();
		let rows = vec![
			row("native_task_models", serde_json::json!({"settings":{"serviceTier":"default"}})),
			row(
				"turn_execution",
				serde_json::json!({"turnId":"a","execution":{"model":"gpt-6.1-sol"}}),
			),
			row(
				"token_usage",
				serde_json::json!({"turnId":"a","tokenUsage":{"total":{"inputTokens":2000,"cachedInputTokens":0,"outputTokens":100},"last":{"inputTokens":1000,"cachedInputTokens":0,"outputTokens":100}}}),
			),
		];
		assert!(calculate(&catalog, "thread", &rows, false).unwrap().partial);
		assert!(calculate(&catalog, "thread", &rows[2..], false).is_none());
	}

	#[test]
	fn long_context_rate_applies_to_the_whole_request() {
		let catalog: Catalog = serde_json::from_str(include_str!("official-prices.json")).unwrap();
		let usage = serde_json::json!({"inputTokens":300_000,"cachedInputTokens":200_000,"outputTokens":100});
		let rows = vec![
			row("native_task_models", serde_json::json!({"settings":{"serviceTier":"default"}})),
			row(
				"turn_execution",
				serde_json::json!({"turnId":"a","execution":{"model":"gpt-6.1-sol"}}),
			),
			row(
				"token_usage",
				serde_json::json!({"turnId":"a","tokenUsage":{"total":usage,"last":usage}}),
			),
		];
		let result = calculate(&catalog, "thread", &rows, false).unwrap();
		assert_eq!(result.usd_micros, 441_500);
		assert!(!result.partial);
	}
}
