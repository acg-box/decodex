//! Exact response observations, separate from cumulative tokens and account quota.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::TokenUsageBreakdown;

/// One native response completion. Missing observations cannot be recovered by summing tokens.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseUsage {
	/// Exact native thread identity.
	pub thread_id: String,
	/// Exact native turn identity, including compaction turns.
	pub turn_id: String,
	/// Exact upstream response identity, used to deduplicate observations.
	pub response_id: String,
	/// Reported response counters, without reconstructed totals.
	pub usage: Option<TokenUsageBreakdown>,
	/// Reported metadata. Absent metadata is distinct from a reported zero amount.
	pub usage_metadata: Option<ResponseUsageMetadata>,
}

/// Native usage metadata. An amount has no inferred currency or unit.
#[derive(Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseUsageMetadata {
	/// Exact provider string, without floating-point conversion.
	pub amount: Option<String>,
	/// Bounded opaque provider usage JSON, not display text or instructions.
	pub metadata: Option<Value>,
	/// True when the provider metadata exceeded the storage budget.
	pub metadata_omitted: bool,
}

impl std::fmt::Debug for ResponseUsageMetadata {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		formatter
			.debug_struct("ResponseUsageMetadata")
			.field("has_amount", &self.amount.is_some())
			.field("has_metadata", &self.metadata.is_some())
			.field("metadata_omitted", &self.metadata_omitted)
			.finish()
	}
}

/// Decode a bounded `rawResponse/completed` notification without trusting extra fields.
pub fn decode_response_usage(params: &Value) -> Option<ResponseUsage> {
	let identity = |key| {
		params
			.get(key)?
			.as_str()
			.filter(|value| !value.is_empty() && value.len() <= 512)
			.map(str::to_owned)
	};
	let usage = match params.get("usage") {
		None | Some(Value::Null) => None,
		Some(value) => {
			let usage: TokenUsageBreakdown = serde_json::from_value(value.clone()).ok()?;
			if !usage.is_valid() {
				return None;
			}
			Some(usage)
		},
	};
	let usage_metadata = match params.get("usageMetadata") {
		None | Some(Value::Null) => None,
		Some(Value::Object(value)) => {
			let amount = match value.get("amount") {
				None | Some(Value::Null) => None,
				Some(Value::String(amount)) if amount.len() <= 256 => Some(amount.clone()),
				_ => return None,
			};
			let metadata = value.get("metadata").filter(|value| !value.is_null());
			let mut budget = MetadataBudget(32 * 1024);
			let metadata_omitted =
				metadata.is_some_and(|value| serde_json::to_writer(&mut budget, value).is_err());
			Some(ResponseUsageMetadata {
				amount,
				metadata: metadata.filter(|_| !metadata_omitted).cloned(),
				metadata_omitted,
			})
		},
		_ => return None,
	};
	Some(ResponseUsage {
		thread_id: identity("threadId")?,
		turn_id: identity("turnId")?,
		response_id: identity("responseId")?,
		usage,
		usage_metadata,
	})
}

struct MetadataBudget(usize);

impl std::io::Write for MetadataBudget {
	fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
		self.0 = self
			.0
			.checked_sub(bytes.len())
			.ok_or_else(|| std::io::Error::other("usage metadata exceeds storage budget"))?;
		Ok(bytes.len())
	}

	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::json;

	fn event() -> Value {
		json!({"threadId":"thread","turnId":"turn","responseId":"response","usage":null})
	}

	#[test]
	fn preserves_precision_unknown_fields_and_zero_without_inventing_usage() {
		for amount in [None, Some("0"), Some("0.12345678901234567890")] {
			let mut value = event();
			value["usageMetadata"] =
				json!({"amount":amount,"metadata":{"extra":[0,null,true,"provider"]}});
			let decoded = decode_response_usage(&value).unwrap();
			assert_eq!(decoded.usage, None);
			let metadata = decoded.usage_metadata.unwrap();
			assert_eq!(metadata.amount.as_deref(), amount);
			assert_eq!(metadata.metadata, Some(value["usageMetadata"]["metadata"].clone()));
			assert!(!metadata.metadata_omitted);
			assert!(!format!("{metadata:?}").contains("provider"));
		}
		assert!(decode_response_usage(&event()).unwrap().usage_metadata.is_none());
	}

	#[test]
	fn bounds_opaque_metadata_without_losing_amount_or_trusting_omission_claims() {
		let mut value = event();
		value["usageMetadata"] =
			json!({"amount":"0","metadata":{"data":"界".repeat(12000)},"metadataOmitted":false});
		let metadata = decode_response_usage(&value).unwrap().usage_metadata.unwrap();
		assert_eq!(metadata.amount.as_deref(), Some("0"));
		assert!(metadata.metadata.is_none() && metadata.metadata_omitted);
		value["usageMetadata"] = json!({"metadata":false,"metadataOmitted":true});
		let metadata = decode_response_usage(&value).unwrap().usage_metadata.unwrap();
		assert_eq!(metadata.metadata, Some(json!(false)));
		assert!(!metadata.metadata_omitted);
	}

	#[test]
	fn escaped_identities_and_full_metadata_fit_the_persistence_envelope() {
		let mut value = event();
		for key in ["threadId", "turnId", "responseId"] {
			value[key] = json!("\u{1}".repeat(512));
		}
		value["usageMetadata"] = json!({"amount":"\u{1}".repeat(256),"metadata":"x".repeat(32766)});
		let decoded = decode_response_usage(&value).unwrap();
		assert!(!decoded.usage_metadata.as_ref().unwrap().metadata_omitted);
		assert!(serde_json::to_vec(&decoded).unwrap().len() <= 48 * 1024);
	}

	#[test]
	fn rejects_malformed_identity_amount_and_counters() {
		for (key, invalid) in [
			("threadId", json!("")),
			("turnId", json!(7)),
			("responseId", json!("x".repeat(513))),
			("usage", json!({"totalTokens":-1})),
			("usageMetadata", json!({"amount":0})),
			("usageMetadata", json!({"amount":"x".repeat(257)})),
		] {
			let mut value = event();
			value[key] = invalid;
			assert!(decode_response_usage(&value).is_none());
		}
	}
}
