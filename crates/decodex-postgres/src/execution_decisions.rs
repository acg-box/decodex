//! Read-only, non-authorizing projection of one immutable Routing Decision execution decision.

use decodex_core::{
	AccountId, ConversationId, ExecutionConsumer, ManagedExecutionId, ManagedRunId,
	QuotaWindowClass, RoutingBlocker, RoutingDecisionCause, RoutingDecisionKind, RuntimeSessionId,
	TurnId,
};
use serde_json::Value;

use crate::{PostgresStore, StoreError};

/// One independent positive quota-depletion fact retained by Routing Decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionQuotaExclusion {
	/// Account path excluded by the fact.
	pub account_id: AccountId,
	/// Exact independent quota-window class.
	pub window: QuotaWindowClass,
	/// Exact duration in minutes.
	pub duration_minutes: u16,
	/// Positive source observation revision.
	pub observation_revision: i64,
	/// Exact future reset instant in Unix microseconds.
	pub resets_at_micros: i64,
}

/// Read-only immutable Routing Decision projection.
///
/// This value grants no account selection, ProcessGeneration, ProviderAttempt, dispatch, retry,
/// receipt, wake, or coordinator authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionDecisionReadback {
	/// Immutable Routing Decision identity.
	pub decision_id: String,
	/// Exact ordinary or managed execution consumer.
	pub consumer: ExecutionConsumer,
	/// Exact route kind.
	pub kind: RoutingDecisionKind,
	/// Selected account only for a selected route.
	pub selected_account_id: Option<AccountId>,
	/// Exact ready instant only for pure positive quota depletion.
	pub waiting_ready_at_micros: Option<i64>,
	/// Complete account-scoped causes for non-selected routes.
	pub causes: Vec<RoutingDecisionCause>,
	/// Independent exact positive quota-depletion exclusions.
	pub quota_exclusions: Vec<ExecutionQuotaExclusion>,
}

impl PostgresStore {
	/// Read one immutable decision through the least-privilege V26 function.
	pub async fn execution_decision(
		&self,
		decision_id: &str,
	) -> Result<Option<ExecutionDecisionReadback>, StoreError> {
		if !is_uuid(decision_id) {
			return Err(StoreError::InvalidInput("execution decision identity"));
		}
		let row = self
			.pool()
			.get()
			.await?
			.query_one(
				"SELECT decodex.read_execution_decision_exact($1::text::uuid)",
				&[&decision_id],
			)
			.await?;
		let value: Option<Value> = row.get(0);
		value.map(parse_readback).transpose()
	}
}

fn parse_readback(value: Value) -> Result<ExecutionDecisionReadback, StoreError> {
	let decision_id = uuid(&value, "decision_id")?;
	let consumer = parse_consumer(&value)?;
	let kind = match text(&value, "kind")? {
		"selected" => RoutingDecisionKind::Selected,
		"waiting_usage" => RoutingDecisionKind::WaitingUsage,
		"waiting_reconciliation" => RoutingDecisionKind::WaitingReconciliation,
		"no_route" => RoutingDecisionKind::NoRoute,
		_ => return incompatible("execution route kind is unknown"),
	};
	let selected_account_id = optional_text(&value, "selected_account_id")?
		.map(|value| {
			AccountId::new(value.to_owned())
				.map_err(|_| incompatible_error("selected account identity is malformed"))
		})
		.transpose()?;
	let waiting_ready_at_micros = optional_nonnegative(&value, "waiting_ready_at_micros")?;
	let causes = array(&value, "causes")?
		.iter()
		.map(|cause| {
			Ok(RoutingDecisionCause {
				account_id: AccountId::new(uuid(cause, "account_id")?).map_err(|_| {
					incompatible_error("execution cause account identity is malformed")
				})?,
				blocker: RoutingBlocker::from_sql(text(cause, "blocker")?)
					.ok_or_else(|| incompatible_error("execution cause is unknown"))?,
			})
		})
		.collect::<Result<Vec<_>, StoreError>>()?;
	let quota_exclusions = array(&value, "quota_exclusions")?
		.iter()
		.map(|exclusion| {
			let (window, expected_duration) = match text(exclusion, "window_class")? {
				"five_hour" => (QuotaWindowClass::FiveHour, 300),
				"seven_day" => (QuotaWindowClass::SevenDay, 10_080),
				_ => return incompatible("execution quota window is unknown"),
			};
			let duration = u16::try_from(positive(exclusion, "duration_minutes")?)
				.map_err(|_| incompatible_error("execution quota duration is malformed"))?;
			if duration != expected_duration {
				return incompatible("execution quota duration is cross-linked");
			}
			Ok(ExecutionQuotaExclusion {
				account_id: AccountId::new(uuid(exclusion, "account_id")?).map_err(|_| {
					incompatible_error("execution exclusion account identity is malformed")
				})?,
				window,
				duration_minutes: duration,
				observation_revision: positive(exclusion, "observation_revision")?,
				resets_at_micros: nonnegative(exclusion, "resets_at_micros")?,
			})
		})
		.collect::<Result<Vec<_>, StoreError>>()?;

	let no_route_reason = optional_text(&value, "no_route_reason")?;
	let shape_valid = match kind {
		RoutingDecisionKind::Selected =>
			selected_account_id.is_some()
				&& waiting_ready_at_micros.is_none()
				&& no_route_reason.is_none()
				&& causes.is_empty(),
		RoutingDecisionKind::WaitingUsage =>
			selected_account_id.is_none()
				&& waiting_ready_at_micros.is_some()
				&& no_route_reason.is_none()
				&& !causes.is_empty()
				&& !quota_exclusions.is_empty()
				&& causes.iter().all(|cause| is_depletion(cause.blocker))
				&& causes.len() == quota_exclusions.len(),
		RoutingDecisionKind::WaitingReconciliation =>
			selected_account_id.is_none()
				&& waiting_ready_at_micros.is_none()
				&& no_route_reason.is_none()
				&& !causes.is_empty()
				&& quota_exclusions.is_empty()
				&& causes.iter().all(|cause| is_reconciliation(cause.blocker)),
		RoutingDecisionKind::NoRoute =>
			selected_account_id.is_none()
				&& waiting_ready_at_micros.is_none()
				&& no_route_reason == Some("blocked_evidence")
				&& !causes.is_empty()
				&& quota_exclusions.is_empty(),
	};
	if !shape_valid {
		return incompatible("execution decision projection is internally inconsistent");
	}
	Ok(ExecutionDecisionReadback {
		decision_id,
		consumer,
		kind,
		selected_account_id,
		waiting_ready_at_micros,
		causes,
		quota_exclusions,
	})
}

fn parse_consumer(value: &Value) -> Result<ExecutionConsumer, StoreError> {
	match text(value, "consumer_kind")? {
		"conversation_turn" => {
			let source_runtime_session_id = optional_text(value, "source_runtime_session_id")?
				.map(|value| RuntimeSessionId::new(value.to_owned()))
				.transpose()
				.map_err(|_| incompatible_error("source RuntimeSession identity is malformed"))?;
			let source_runtime_session_revision =
				optional_positive(value, "source_runtime_session_revision")?;
			if source_runtime_session_id.is_some() != source_runtime_session_revision.is_some() {
				return incompatible("execution decision source lineage is partial");
			}
			Ok(ExecutionConsumer::ConversationTurn {
				conversation_id: ConversationId::new(uuid(value, "conversation_id")?).map_err(
					|_| incompatible_error("execution Conversation identity is malformed"),
				)?,
				conversation_revision: positive(value, "conversation_revision")?,
				source_runtime_session_id,
				source_runtime_session_revision,
				turn_id: TurnId::new(uuid(value, "turn_id")?)
					.map_err(|_| incompatible_error("execution Turn identity is malformed"))?,
			})
		},
		"managed_run_execution" => Ok(ExecutionConsumer::ManagedRunExecution {
			managed_run_id: ManagedRunId::new(uuid(value, "managed_run_id")?)
				.map_err(|_| incompatible_error("ManagedRun identity is malformed"))?,
			managed_run_revision: positive(value, "managed_run_revision")?,
			execution_id: ManagedExecutionId::new(uuid(value, "managed_execution_id")?)
				.map_err(|_| incompatible_error("managed execution identity is malformed"))?,
		}),
		_ => incompatible("execution consumer kind is unknown"),
	}
}

const fn is_depletion(blocker: RoutingBlocker) -> bool {
	matches!(blocker, RoutingBlocker::QuotaFiveHourDepleted | RoutingBlocker::QuotaSevenDayDepleted)
}

const fn is_reconciliation(blocker: RoutingBlocker) -> bool {
	matches!(
		blocker,
		RoutingBlocker::ProcessGenerationUnresolved | RoutingBlocker::ProviderAttemptUnresolved
	)
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| incompatible_error("execution decision text is malformed"))
}

fn optional_text<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::String(value)) => Ok(Some(value)),
		_ => incompatible("execution decision optional text is malformed"),
	}
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], StoreError> {
	value
		.get(key)
		.and_then(Value::as_array)
		.map(Vec::as_slice)
		.ok_or_else(|| incompatible_error("execution decision array is malformed"))
}

fn positive(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.filter(|value| *value > 0)
		.ok_or_else(|| incompatible_error("execution decision positive integer is malformed"))
}

fn optional_positive(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::Number(number)) =>
			number.as_i64().filter(|value| *value > 0).map(Some).ok_or_else(|| {
				incompatible_error("execution decision optional revision is malformed")
			}),
		_ => incompatible("execution decision optional revision is malformed"),
	}
}

fn nonnegative(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.filter(|value| *value >= 0)
		.ok_or_else(|| incompatible_error("execution decision integer is malformed"))
}

fn optional_nonnegative(value: &Value, key: &str) -> Result<Option<i64>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(Value::Number(number)) =>
			number.as_i64().filter(|value| *value >= 0).map(Some).ok_or_else(|| {
				incompatible_error("execution decision optional integer is malformed")
			}),
		_ => incompatible("execution decision optional integer is malformed"),
	}
}

fn uuid(value: &Value, key: &str) -> Result<String, StoreError> {
	let value = text(value, key)?;
	if is_uuid(value) {
		Ok(value.to_owned())
	} else {
		incompatible("execution decision UUID is malformed")
	}
}

fn is_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.bytes().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => byte == b'-',
			_ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
		})
}

fn incompatible<T>(reason: &str) -> Result<T, StoreError> {
	Err(incompatible_error(reason))
}

fn incompatible_error(reason: &str) -> StoreError {
	StoreError::Incompatible(reason.to_owned())
}
