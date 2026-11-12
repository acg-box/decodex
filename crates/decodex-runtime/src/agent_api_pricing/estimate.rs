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
	mut partial: bool,
) -> Option<ApiCostEstimate> {
	let mut settings = Value::Null;
	let mut selections = BTreeMap::new();
	let mut previous = Tokens::default();
	let mut numerator = 0_u128;
	let mut priced = 0;
	for row in rows {
		let Ok(value) = serde_json::from_str::<Value>(&row.payload) else {
			partial = true;
			continue;
		};
		match row.kind.as_str() {
			"native_task_models" => {
				settings = value["settings"].clone();
			},
			"turn_execution" => {
				if let (Some(turn), Some(model)) =
					(value["turnId"].as_str(), value["execution"]["model"].as_str())
				{
					selections.insert(turn.to_owned(), model.to_owned());
				}
			},
			"token_usage" => {
				let usage = &value["tokenUsage"];
				let (Ok(total), Ok(last)) = (
					serde_json::from_value::<Tokens>(usage["total"].clone()),
					serde_json::from_value::<Tokens>(usage["last"].clone()),
				) else {
					partial = true;
					continue;
				};
				if total == previous {
					continue;
				}
				partial |= total.difference(&previous).as_ref() != Some(&last);
				// Native token receipts do not identify cache retention. Use the
				// published 30-minute write rate and mark longer-retention cost unknown.
				partial |= last.cache_write_input_tokens > 0;
				previous = total;
				let model = value["turnId"].as_str().and_then(|turn| selections.get(turn));
				let speed = match settings["serviceTier"].as_str() {
					Some("default" | "standard") => Some("standard"),
					Some("priority" | "fast") => Some("fast"),
					Some("ultrafast") => Some("ultrafast"),
					None if settings.is_object() => Some("standard"),
					_ => None,
				};
				let amount = model.zip(speed).and_then(|(model, speed)| {
					let rates = catalog.rates.get(model)?.get(speed)?;
					let prices = if last.input_tokens > 272_000 {
						rates.long.as_ref().unwrap_or(&rates.short)
					} else {
						&rates.short
					};
					let input = last
						.input_tokens
						.checked_sub(last.cached_input_tokens)?
						.checked_sub(last.cache_write_input_tokens)?;
					let counts = [
						input,
						last.cached_input_tokens,
						last.cache_write_input_tokens,
						0,
						last.output_tokens,
					];
					counts.into_iter().zip(prices).try_fold(0_u128, |sum, (count, price)| {
						if count == 0 {
							return Some(sum);
						}
						sum.checked_add(u128::from(count) * u128::from((*price)?))
					})
				});
				if let Some(amount) = amount {
					numerator = numerator.checked_add(amount)?;
					priced += 1;
				} else {
					partial = true;
				}
			},
			_ => {},
		}
	}
	if priced == 0 {
		return None;
	}
	Some(ApiCostEstimate {
		thread_id: thread.into(),
		usd_micros: u64::try_from((numerator + 500_000) / 1_000_000).ok()?,
		priced_responses: priced,
		partial,
		pricing_checked_at_micros: catalog.checked_at_micros,
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
