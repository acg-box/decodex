//! Public app-server token facts. These counters are not account quota or monetary cost.

use serde::{Deserialize, Serialize};

/// Provider-reported counters for a response or a thread.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsageBreakdown {
	/// Provider total; do not reconstruct it by adding overlapping subtotals.
	pub total_tokens: u64,
	/// Input tokens reported by the provider.
	pub input_tokens: u64,
	/// Cached input, already included in input tokens.
	pub cached_input_tokens: u64,
	/// Cache writes, omitted by older providers.
	#[serde(default)]
	pub cache_write_input_tokens: u64,
	/// Output tokens reported by the provider.
	pub output_tokens: u64,
	/// Reasoning output, a provider-reported subtotal.
	pub reasoning_output_tokens: u64,
}

impl TokenUsageBreakdown {
	pub(crate) fn is_valid(&self) -> bool {
		[
			self.total_tokens,
			self.input_tokens,
			self.cached_input_tokens,
			self.cache_write_input_tokens,
			self.output_tokens,
			self.reasoning_output_tokens,
		]
		.into_iter()
		.all(|value| value <= i64::MAX as u64)
	}
}

/// One native `thread/tokenUsage/updated` observation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadTokenUsage {
	/// Cumulative thread usage, not usage for just the current turn.
	pub total: TokenUsageBreakdown,
	/// Usage for the last model response, not necessarily the entire turn.
	pub last: TokenUsageBreakdown,
	/// Reported context capacity. Missing capacity is unknown.
	pub model_context_window: Option<u64>,
}

impl ThreadTokenUsage {
	/// Validate the signed-integer bounds of the official protocol.
	pub fn is_valid(&self) -> bool {
		self.total.is_valid()
			&& self.last.is_valid()
			&& self.model_context_window.is_none_or(|value| value > 0 && value <= i64::MAX as u64)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	#[test]
	fn native_usage_preserves_distinct_totals_and_unknown_context() {
		let counts = json!({"totalTokens":1200,"inputTokens":1000,"cachedInputTokens":500,
			"outputTokens":200,"reasoningOutputTokens":100});
		let mut value = json!({"total":counts,"last":counts,"modelContextWindow":null});
		value["total"]["totalTokens"] = json!(9000);
		let usage: ThreadTokenUsage = serde_json::from_value(value.clone()).unwrap();
		assert!(usage.is_valid());
		assert_eq!(usage.total.total_tokens, 9000);
		assert_eq!(usage.last.total_tokens, 1200);
		assert_eq!(usage.last.cache_write_input_tokens, 0);
		assert_eq!(usage.model_context_window, None);
		value["last"]["inputTokens"] = json!(-1);
		assert!(serde_json::from_value::<ThreadTokenUsage>(value).is_err());
	}
}
