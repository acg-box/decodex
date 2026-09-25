//! Compact references to immutable provider-proposed approval decisions.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// A decision whose complete response comes from the exact persisted request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChiefRequestedDecision {
	/// Grant exactly the requested permissions for the current turn.
	PermissionsForTurn,
	/// Select an explicit policy amendment from the provider's decision list.
	CommandPolicy {
		/// Position in the original `availableDecisions` array, including strings.
		index: usize,
	},
}

impl ChiefRequestedDecision {
	/// Find a compact reference only when it reconstructs the exact explicit response.
	pub fn matching_response(method: &str, params: &Value, response: &Value) -> Option<Self> {
		let decision = if method == "item/permissions/requestApproval" {
			Self::PermissionsForTurn
		} else if method == "item/commandExecution/requestApproval" {
			let index = params["availableDecisions"]
				.as_array()?
				.iter()
				.position(|offered| offered == &response["decision"])?;
			Self::CommandPolicy { index }
		} else {
			return None;
		};
		(requested_decision_response(method, params, &decision).as_ref() == Some(response))
			.then_some(decision)
	}
}

/// Reconstruct only an explicitly offered decision from the original request.
/// The caller must separately validate pending event ownership and native liveness.
pub fn requested_decision_response(
	method: &str,
	params: &Value,
	decision: &ChiefRequestedDecision,
) -> Option<Value> {
	match decision {
		ChiefRequestedDecision::PermissionsForTurn
			if method == "item/permissions/requestApproval"
				&& params["permissions"].is_object() =>
			Some(json!({"permissions": params["permissions"], "scope": "turn"})),
		ChiefRequestedDecision::CommandPolicy { index }
			if method == "item/commandExecution/requestApproval" =>
		{
			let proposed = params["availableDecisions"].as_array()?.get(*index)?;
			let object = proposed.as_object()?;
			if object.len() != 1
				|| !(object.get("acceptWithExecpolicyAmendment").is_some_and(Value::is_object)
					|| object.get("applyNetworkPolicyAmendment").is_some_and(Value::is_object))
			{
				return None;
			}
			Some(json!({"decision": proposed}))
		},
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn selected_decisions_preserve_large_values_and_exact_array_positions() {
		let permissions = json!({"fileSystem":{"write":["/tmp/界".repeat(6000)]}});
		let params = json!({"permissions":permissions});
		let result = requested_decision_response(
			"item/permissions/requestApproval",
			&params,
			&ChiefRequestedDecision::PermissionsForTurn,
		)
		.unwrap();
		assert!(result.to_string().len() > crate::MAX_HISTORY_INLINE_BYTES);
		assert_eq!(result, json!({"permissions":permissions,"scope":"turn"}));
		assert_eq!(
			ChiefRequestedDecision::matching_response(
				"item/permissions/requestApproval",
				&params,
				&result
			),
			Some(ChiefRequestedDecision::PermissionsForTurn)
		);
		let mut changed = result;
		changed["scope"] = json!("session");
		assert!(
			ChiefRequestedDecision::matching_response(
				"item/permissions/requestApproval",
				&params,
				&changed
			)
			.is_none()
		);
		assert!(
			requested_decision_response(
				"mcpServer/elicitation/request",
				&params,
				&ChiefRequestedDecision::PermissionsForTurn
			)
			.is_none()
		);
		let policy = json!({"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["command".repeat(6000)]}});
		let params = json!({"availableDecisions":["decline", policy]});
		let selected = ChiefRequestedDecision::CommandPolicy { index: 1 };
		assert_eq!(
			ChiefRequestedDecision::matching_response(
				"item/commandExecution/requestApproval",
				&params,
				&json!({"decision":policy})
			),
			Some(selected.clone())
		);
		let action = crate::ChiefActionDto::RespondWithRequestedDecision {
			work_id: crate::EntityId::new("work").unwrap(),
			event_id: 42,
			decision: selected.clone(),
		};
		assert!(serde_json::to_string(&action).unwrap().len() < 512);
		assert_eq!(
			requested_decision_response(
				"item/commandExecution/requestApproval",
				&params,
				&selected
			),
			Some(json!({"decision":policy}))
		);
		for index in [0, 2, usize::MAX] {
			assert!(
				requested_decision_response(
					"item/commandExecution/requestApproval",
					&params,
					&ChiefRequestedDecision::CommandPolicy { index }
				)
				.is_none()
			);
		}
		assert!(
			requested_decision_response("item/fileChange/requestApproval", &params, &selected)
				.is_none()
		);
	}
}
