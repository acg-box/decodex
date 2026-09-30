//! Compact references to immutable provider-proposed approval decisions.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A decision whose complete response comes from the exact persisted request.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentRequestedDecision {
	/// Grant exactly the requested permissions for the current turn.
	PermissionsForTurn,
	/// Select an explicit policy amendment from the provider's decision list.
	CommandPolicy {
		/// Position in the original `availableDecisions` array, including strings.
		index: usize,
	},
}
impl AgentRequestedDecision {
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
	decision: &AgentRequestedDecision,
) -> Option<Value> {
	match decision {
		AgentRequestedDecision::PermissionsForTurn
			if method == "item/permissions/requestApproval"
				&& params["permissions"].is_object() =>
			Some(serde_json::json!({"permissions": params["permissions"], "scope": "turn"})),
		AgentRequestedDecision::CommandPolicy { index }
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

			Some(serde_json::json!({"decision": proposed}))
		},
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use crate::{AgentRequestedDecision, agent_requested_decision};

	#[test]
	fn selected_decisions_preserve_large_values_and_exact_array_positions() {
		let permissions = serde_json::json!({"fileSystem":{"write":["/tmp/界".repeat(6_000)]}});
		let params = serde_json::json!({"permissions":permissions});
		let result = agent_requested_decision::requested_decision_response(
			"item/permissions/requestApproval",
			&params,
			&AgentRequestedDecision::PermissionsForTurn,
		)
		.unwrap();

		assert!(result.to_string().len() > crate::MAX_HISTORY_INLINE_BYTES);
		assert_eq!(result, serde_json::json!({"permissions":permissions,"scope":"turn"}));
		assert_eq!(
			AgentRequestedDecision::matching_response(
				"item/permissions/requestApproval",
				&params,
				&result
			),
			Some(AgentRequestedDecision::PermissionsForTurn)
		);

		let mut changed = result;

		changed["scope"] = serde_json::json!("session");

		assert!(
			AgentRequestedDecision::matching_response(
				"item/permissions/requestApproval",
				&params,
				&changed
			)
			.is_none()
		);
		assert!(
			agent_requested_decision::requested_decision_response(
				"mcpServer/elicitation/request",
				&params,
				&AgentRequestedDecision::PermissionsForTurn
			)
			.is_none()
		);

		let policy = serde_json::json!({"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["command".repeat(6_000)]}});
		let params = serde_json::json!({"availableDecisions":["decline", policy]});
		let selected = AgentRequestedDecision::CommandPolicy { index: 1 };

		assert_eq!(
			AgentRequestedDecision::matching_response(
				"item/commandExecution/requestApproval",
				&params,
				&serde_json::json!({"decision":policy})
			),
			Some(selected.clone())
		);

		let action = crate::AgentActionDto::RespondWithRequestedDecision {
			work_id: crate::EntityId::new("work").unwrap(),
			event_id: 42,
			decision: selected.clone(),
		};

		assert!(serde_json::to_string(&action).unwrap().len() < 512);
		assert_eq!(
			agent_requested_decision::requested_decision_response(
				"item/commandExecution/requestApproval",
				&params,
				&selected
			),
			Some(serde_json::json!({"decision":policy}))
		);

		for index in [0, 2, usize::MAX] {
			assert!(
				agent_requested_decision::requested_decision_response(
					"item/commandExecution/requestApproval",
					&params,
					&AgentRequestedDecision::CommandPolicy { index }
				)
				.is_none()
			);
		}

		assert!(
			agent_requested_decision::requested_decision_response(
				"item/fileChange/requestApproval",
				&params,
				&selected
			)
			.is_none()
		);
	}
}
