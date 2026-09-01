//! Durable account routing for ordinary Conversation conversations.

use decodex_core::{
	AccountId, AccountLifecycleReadiness, AccountQuotaDisposition, AccountQuotaObservationError,
	AccountQuotaWindowObservation, AccountRecord, AccountRegistryQuotaFact,
	AccountRegistryQuotaObservation, AccountRegistryRoutingDecision,
	AccountRegistryRoutingDecisionKind, AccountRegistryRoutingMember,
	AccountRegistryRoutingSnapshot, AccountSelectionMode, AccountState, ConversationId,
	ExecutionConsumer, QuotaWindowClass, RoutingBlocker, RoutingCommandOutcome,
	RoutingDecisionCause, RoutingRejection, RuntimeSessionId, TurnId,
	decide_account_registry_routing,
};
use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{
	SqliteStore, StoreError,
	account_lifecycle::{random_uuid_v4, read_account_registry_sync, sql_error},
	unix_micros,
};

/// Exact Conversation coordinates for its sole initial account route.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteConversationInitial {
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
}

/// Immutable initial route and database-owned prospective Turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationInitialRoute {
	pub decision_id: String,
	pub operation_id: String,
	pub snapshot: AccountRegistryRoutingSnapshot,
	pub consumer: ExecutionConsumer,
	pub turn_id: TurnId,
	pub decided_at_micros: i64,
	pub decision: AccountRegistryRoutingDecision,
}

/// Exact initial route result with stable replay classification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationInitialRouteOutcome {
	Fresh(ConversationInitialRoute),
	Replayed(ConversationInitialRoute),
	Rejected(RoutingRejection),
	ReplayedRejection(RoutingRejection),
}

/// Exact non-selecting continuation binding over one active Conversation session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindConversationContinuation {
	pub operation_id: String,
	pub conversation_id: ConversationId,
	pub expected_conversation_revision: i64,
	pub source_runtime_session_id: RuntimeSessionId,
	pub expected_source_runtime_session_revision: i64,
	pub turn_id: TurnId,
}

/// Immutable lineage for one later Turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversationContinuationBinding {
	pub decision_id: String,
	pub consumer: ExecutionConsumer,
	pub initial_decision_id: String,
	pub account_snapshot_id: String,
	pub account_snapshot_source_revision: i64,
	pub profile_snapshot_id: String,
	pub profile_snapshot_source_revision: i64,
	pub decided_at_micros: i64,
}

impl SqliteStore {
	/// Route one open ordinary Conversation from one transactionally captured registry universe.
	#[allow(clippy::too_many_lines)] // Keep one atomic initial-route decision together.
	pub async fn route_conversation_initial(
		&self,
		idempotency_key: &str,
		request: &RouteConversationInitial,
	) -> Result<ConversationInitialRouteOutcome, StoreError> {
		validate_key(idempotency_key)?;
		if request.expected_conversation_revision <= 0 {
			return Err(StoreError::InvalidInput("Conversation revision must be positive"));
		}
		let key = idempotency_key.to_owned();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let request_sha = route_request_sha(&request);
			if let Some((stored_sha, decision_id)) = transaction
				.query_row(
					"SELECT request_sha256, routing_decision_id FROM routing_decisions
					 WHERE idempotency_key = ?1",
					params![key],
					|row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
				)
				.optional()
				.map_err(sql_error)?
			{
				if stored_sha != request_sha {
					return Err(StoreError::IdempotencyConflict);
				}
				let route = read_initial_route_by_id(&transaction, &decision_id)?;
				transaction.commit().map_err(sql_error)?;
				return Ok(ConversationInitialRouteOutcome::Replayed(route));
			}

			let conversation_matches: bool = transaction
				.query_row(
					"SELECT EXISTS (
					   SELECT 1 FROM conversations AS c
					   JOIN quick_task_requests AS q USING (conversation_id)
					   WHERE c.conversation_id = ?1 AND c.revision = ?2 AND c.state = 'active'
					 )",
					params![
						request.conversation_id.as_str(),
						request.expected_conversation_revision
					],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if !conversation_matches {
				return Ok(ConversationInitialRouteOutcome::Rejected(RoutingRejection {
					operation: "route_quick_task_initial".to_owned(),
					code: "conversation_mismatch".to_owned(),
				}));
			}
			let already_bound: bool = transaction
				.query_row(
					"SELECT EXISTS (
					   SELECT 1 FROM routing_decisions
					   WHERE conversation_id = ?1
					     AND authority_shape = 'conversation_account_registry'
					 )",
					params![request.conversation_id.as_str()],
					|row| row.get(0),
				)
				.map_err(sql_error)?;
			if already_bound {
				return Ok(ConversationInitialRouteOutcome::Rejected(RoutingRejection {
					operation: "route_quick_task_initial".to_owned(),
					code: "initial_routing_already_bound".to_owned(),
				}));
			}

			let accounts = read_account_registry_sync(&transaction, None, 512)?;
			if accounts.is_empty() {
				return Ok(ConversationInitialRouteOutcome::Rejected(RoutingRejection {
					operation: "route_quick_task_initial".to_owned(),
					code: "routing_authority_unavailable".to_owned(),
				}));
			}
			let (mode, routing_revision) = read_routing_control(&transaction)?;
			let profile_revision: i64 = transaction
				.query_row("SELECT revision FROM role_profiles WHERE role = 'task'", [], |row| {
					row.get(0)
				})
				.map_err(sql_error)?;
			let decided_at_micros = unix_micros().map_err(StoreError::from)?;
			let snapshot = build_snapshot(
				random_uuid_v4()?,
				routing_revision,
				mode,
				profile_revision,
				decided_at_micros,
				&accounts,
			)?;
			let decision =
				decide_account_registry_routing(&snapshot, decided_at_micros).map_err(|_| {
					StoreError::Incompatible("routing snapshot is incomplete".to_owned())
				})?;
			let decision_id = random_uuid_v4()?;
			let operation_id = random_uuid_v4()?;
			let turn_id = TurnId::new(random_uuid_v4()?)
				.map_err(|_| StoreError::Incompatible("generated Turn identity".to_owned()))?;
			let account_revision = decision.selected_account_id.as_ref().and_then(|selected| {
				accounts
					.iter()
					.find(|account| account.account_id == *selected)
					.map(|account| account.revision)
			});
			let quota_classification = quota_classification(&decision, &snapshot);
			transaction
				.execute(
					"INSERT INTO routing_decisions (
					   routing_decision_id, operation_id, idempotency_key, request_sha256,
					   authority_shape, conversation_id, turn_id, conversation_revision,
					   snapshot_id, snapshot_json, decision_kind, account_id, account_revision,
					   routing_revision, quota_classification, causes_json, exclusions_json,
					   created_at_micros
					 ) VALUES (
					   ?1, ?2, ?3, ?4, 'conversation_account_registry', ?5, ?6, ?7,
					   ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17
					 )",
					params![
						decision_id,
						operation_id,
						key,
						request_sha,
						request.conversation_id.as_str(),
						turn_id.as_str(),
						request.expected_conversation_revision,
						snapshot.snapshot_id,
						serialize_snapshot(&snapshot)?.to_string(),
						decision_kind_text(decision.kind),
						decision.selected_account_id.as_ref().map(AccountId::as_str),
						account_revision,
						routing_revision,
						quota_classification,
						serialize_causes(&decision.causes).to_string(),
						serialize_exclusions(&decision.exclusions).to_string(),
						decided_at_micros,
					],
				)
				.map_err(sql_error)?;
			let route = ConversationInitialRoute {
				decision_id,
				operation_id,
				snapshot,
				consumer: ExecutionConsumer::ConversationTurn {
					conversation_id: request.conversation_id,
					conversation_revision: request.expected_conversation_revision,
					source_runtime_session_id: None,
					source_runtime_session_revision: None,
					turn_id: turn_id.clone(),
				},
				turn_id,
				decided_at_micros,
				decision,
			};
			transaction.commit().map_err(sql_error)?;
			Ok(ConversationInitialRouteOutcome::Fresh(route))
		})
		.await
	}

	/// Read the immutable initial route without consulting current account authority.
	pub async fn read_conversation_initial_route(
		&self,
		conversation_id: &ConversationId,
	) -> Result<Option<ConversationInitialRoute>, StoreError> {
		let conversation_id = conversation_id.clone();
		self.run(move |connection| {
			let decision_id = connection
				.query_row(
					"SELECT routing_decision_id FROM routing_decisions
					 WHERE conversation_id = ?1
					   AND authority_shape = 'conversation_account_registry'",
					params![conversation_id.as_str()],
					|row| row.get::<_, String>(0),
				)
				.optional()
				.map_err(sql_error)?;
			decision_id.map(|id| read_initial_route_by_id(connection, &id)).transpose()
		})
		.await
	}

	/// Bind one later Turn to the original selected account without selecting again.
	#[allow(clippy::too_many_lines)] // Keep one atomic continuation binding together.
	pub async fn bind_conversation_continuation(
		&self,
		idempotency_key: &str,
		request: &BindConversationContinuation,
	) -> Result<RoutingCommandOutcome<ConversationContinuationBinding>, StoreError> {
		validate_key(idempotency_key)?;
		if request.expected_conversation_revision <= 0
			|| request.expected_source_runtime_session_revision <= 0
		{
			return Err(StoreError::InvalidInput(
				"Conversation continuation coordinates are invalid",
			));
		}
		let key = idempotency_key.to_owned();
		let request = request.clone();
		self.run(move |connection| {
			let transaction = connection
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(sql_error)?;
			let request_sha = continuation_request_sha(&request);
			if let Some((stored_sha, decision_id)) = transaction
				.query_row(
					"SELECT request_sha256, routing_decision_id FROM routing_decisions
					 WHERE idempotency_key = ?1",
					params![key],
					|row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
				)
				.optional()
				.map_err(sql_error)?
			{
				if stored_sha != request_sha {
					return Err(StoreError::IdempotencyConflict);
				}
				let binding = read_continuation_binding(&transaction, &decision_id)?;
				transaction.commit().map_err(sql_error)?;
				return Ok(RoutingCommandOutcome::Success(binding));
			}

			let session = transaction
				.query_row(
					"SELECT s.account_id, s.account_revision, s.account_snapshot_id,
					        s.profile_snapshot_id, s.profile_revision, initial.routing_decision_id,
					        initial.routing_revision
					 FROM runtime_sessions AS s
					 JOIN conversations AS c ON c.conversation_id = s.conversation_id
					 JOIN routing_decisions AS initial
					   ON initial.conversation_id = c.conversation_id
					  AND initial.authority_shape = 'conversation_account_registry'
					 WHERE s.runtime_session_id = ?1 AND s.conversation_id = ?2
					   AND s.revision = ?3 AND s.state = 'active'
					   AND c.revision = ?4 AND c.state = 'active'
					   AND EXISTS (
					     SELECT 1 FROM turns AS t WHERE t.turn_id = ?5
					       AND t.conversation_id = c.conversation_id
					       AND t.runtime_session_id = s.runtime_session_id AND t.status = 'active'
					   )",
					params![
						request.source_runtime_session_id.as_str(),
						request.conversation_id.as_str(),
						request.expected_source_runtime_session_revision,
						request.expected_conversation_revision,
						request.turn_id.as_str(),
					],
					|row| {
						Ok((
							row.get::<_, String>(0)?,
							row.get::<_, i64>(1)?,
							row.get::<_, String>(2)?,
							row.get::<_, String>(3)?,
							row.get::<_, i64>(4)?,
							row.get::<_, String>(5)?,
							row.get::<_, i64>(6)?,
						))
					},
				)
				.optional()
				.map_err(sql_error)?;
			let Some((
				account_id,
				account_revision,
				account_snapshot_id,
				profile_snapshot_id,
				profile_revision,
				initial_decision_id,
				routing_revision,
			)) = session
			else {
				return Ok(RoutingCommandOutcome::Rejected(RoutingRejection {
					operation: "bind_quick_task_continuation".to_owned(),
					code: "authority_unavailable".to_owned(),
				}));
			};
			let decision_id = random_uuid_v4()?;
			let decided_at_micros = unix_micros().map_err(StoreError::from)?;
			transaction
				.execute(
					"INSERT INTO routing_decisions (
					   routing_decision_id, operation_id, idempotency_key, request_sha256,
					   authority_shape, conversation_id, turn_id, conversation_revision,
					   source_runtime_session_id, source_runtime_session_revision,
					   account_snapshot_id, profile_snapshot_id, decision_kind,
					   account_id, account_revision, routing_revision, quota_classification,
					   causes_json, exclusions_json, created_at_micros
					 ) VALUES (
					   ?1, ?2, ?3, ?4, 'conversation_continuation', ?5, ?6, ?7,
					   ?8, ?9, ?10, ?11, 'selected', ?12, ?13, ?14, 'unknown', '[]', '[]', ?15
					 )",
					params![
						decision_id,
						request.operation_id,
						key,
						request_sha,
						request.conversation_id.as_str(),
						request.turn_id.as_str(),
						request.expected_conversation_revision,
						request.source_runtime_session_id.as_str(),
						request.expected_source_runtime_session_revision,
						account_snapshot_id,
						profile_snapshot_id,
						account_id,
						account_revision,
						routing_revision,
						decided_at_micros,
					],
				)
				.map_err(sql_error)?;
			let binding = ConversationContinuationBinding {
				decision_id,
				consumer: ExecutionConsumer::ConversationTurn {
					conversation_id: request.conversation_id,
					conversation_revision: request.expected_conversation_revision,
					source_runtime_session_id: Some(request.source_runtime_session_id),
					source_runtime_session_revision: Some(
						request.expected_source_runtime_session_revision,
					),
					turn_id: request.turn_id,
				},
				initial_decision_id,
				account_snapshot_id,
				account_snapshot_source_revision: account_revision,
				profile_snapshot_id,
				profile_snapshot_source_revision: profile_revision,
				decided_at_micros,
			};
			transaction.commit().map_err(sql_error)?;
			Ok(RoutingCommandOutcome::Success(binding))
		})
		.await
	}
}

fn build_snapshot(
	snapshot_id: String,
	routing_revision: i64,
	mode: AccountSelectionMode,
	profile_revision: i64,
	resolved_at_micros: i64,
	accounts: &[AccountRecord],
) -> Result<AccountRegistryRoutingSnapshot, StoreError> {
	let members = accounts
		.iter()
		.enumerate()
		.map(|(index, account)| AccountRegistryRoutingMember {
			position: index + 1,
			account_id: account.account_id.clone(),
			account_revision: account.revision,
			blockers: account_blockers(account),
		})
		.collect();
	let mut quota_facts = Vec::with_capacity(accounts.len() * 2);
	for account in accounts {
		quota_facts.push(quota_fact(
			&account.account_id,
			QuotaWindowClass::FiveHour,
			account.five_hour_quota,
		)?);
		quota_facts.push(quota_fact(
			&account.account_id,
			QuotaWindowClass::SevenDay,
			account.seven_day_quota,
		)?);
	}
	Ok(AccountRegistryRoutingSnapshot {
		snapshot_id,
		routing_revision,
		mode,
		task_role_profile_revision: profile_revision,
		resolved_at_micros,
		members,
		quota_facts,
	})
}

fn account_blockers(account: &AccountRecord) -> Vec<RoutingBlocker> {
	let mut blockers = Vec::new();
	if matches!(account.observed_state, AccountState::Unavailable) {
		blockers.push(RoutingBlocker::AccountUnavailable);
	}
	if matches!(
		account.lifecycle_readiness,
		AccountLifecycleReadiness::StoreUnavailable | AccountLifecycleReadiness::OperationUnsettled
	) {
		blockers.push(RoutingBlocker::AccountUnavailable);
	}
	if matches!(account.observed_state, AccountState::AuthFailed)
		|| matches!(
			account.lifecycle_readiness,
			AccountLifecycleReadiness::CredentialAbsent
				| AccountLifecycleReadiness::StoreMismatch
				| AccountLifecycleReadiness::ProviderMismatch
		) {
		blockers.push(RoutingBlocker::AccountAuthFailed);
	}
	if matches!(account.observed_state, AccountState::PluginUnready)
		|| account.lifecycle_readiness == AccountLifecycleReadiness::CallbackCapabilityUnready
	{
		blockers.push(RoutingBlocker::AccountPluginUnready);
	}
	if !account.enabled
		|| account.tombstoned
		|| account.lifecycle_readiness == AccountLifecycleReadiness::Tombstoned
	{
		blockers.push(RoutingBlocker::AccountDisabled);
	}
	blockers.sort();
	blockers.dedup();
	blockers
}

fn quota_fact(
	account_id: &AccountId,
	window: QuotaWindowClass,
	observation: AccountQuotaWindowObservation,
) -> Result<AccountRegistryQuotaFact, StoreError> {
	let duration_minutes = u16::try_from(observation.duration_minutes)
		.map_err(|_| StoreError::Incompatible("quota duration".to_owned()))?;
	let observation = match observation.disposition {
		AccountQuotaDisposition::Unknown => AccountRegistryQuotaObservation::Missing,
		AccountQuotaDisposition::Current(fact) | AccountQuotaDisposition::Stale(fact) =>
			AccountRegistryQuotaObservation::Current {
				used_percent: fact.used_percent,
				observed_at_micros: observation
					.observed_at_unix_micros
					.ok_or_else(|| StoreError::Incompatible("quota observation time".to_owned()))?,
				resets_at_micros: fact.resets_at_unix_micros,
			},
		AccountQuotaDisposition::Error(error) =>
			AccountRegistryQuotaObservation::ObservationError {
				error,
				observed_at_micros: observation
					.observed_at_unix_micros
					.ok_or_else(|| StoreError::Incompatible("quota error time".to_owned()))?,
			},
	};
	Ok(AccountRegistryQuotaFact {
		account_id: account_id.clone(),
		window,
		duration_minutes,
		observation,
	})
}

fn read_routing_control(
	transaction: &Transaction<'_>,
) -> Result<(AccountSelectionMode, i64), StoreError> {
	let (mode, fixed, revision): (String, Option<String>, i64) = transaction
		.query_row(
			"SELECT mode, fixed_account_id, revision FROM account_routing_control WHERE singleton = 1",
			[],
			|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
		)
		.map_err(sql_error)?;
	let mode = match (mode.as_str(), fixed) {
		("balanced", None) => AccountSelectionMode::Balanced,
		("fixed", Some(account)) => AccountSelectionMode::Fixed(
			AccountId::new(account)
				.map_err(|_| StoreError::Incompatible("fixed account".to_owned()))?,
		),
		_ => return Err(StoreError::Incompatible("routing control".to_owned())),
	};
	Ok((mode, revision))
}

fn read_initial_route_by_id(
	connection: &rusqlite::Connection,
	decision_id: &str,
) -> Result<ConversationInitialRoute, StoreError> {
	let row = connection
		.query_row(
			"SELECT operation_id, conversation_id, turn_id, conversation_revision,
			        snapshot_json, decision_kind, account_id, causes_json, exclusions_json,
			        created_at_micros
			 FROM routing_decisions
			 WHERE routing_decision_id = ?1
			   AND authority_shape = 'conversation_account_registry'",
			params![decision_id],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, i64>(3)?,
					row.get::<_, String>(4)?,
					row.get::<_, String>(5)?,
					row.get::<_, Option<String>>(6)?,
					row.get::<_, String>(7)?,
					row.get::<_, String>(8)?,
					row.get::<_, i64>(9)?,
				))
			},
		)
		.map_err(sql_error)?;
	let conversation_id = ConversationId::new(row.1)
		.map_err(|_| StoreError::Incompatible("Conversation identity".to_owned()))?;
	let turn_id =
		TurnId::new(row.2).map_err(|_| StoreError::Incompatible("Turn identity".to_owned()))?;
	let snapshot = parse_snapshot(
		&serde_json::from_str(&row.4)
			.map_err(|_| StoreError::Incompatible("routing snapshot JSON".to_owned()))?,
	)?;
	let selected_account_id = row
		.6
		.map(AccountId::new)
		.transpose()
		.map_err(|_| StoreError::Incompatible("selected account".to_owned()))?;
	let decision = AccountRegistryRoutingDecision {
		snapshot_id: snapshot.snapshot_id.clone(),
		kind: parse_decision_kind(&row.5)?,
		selected_account_id,
		causes: parse_causes(
			&serde_json::from_str(&row.7)
				.map_err(|_| StoreError::Incompatible("routing causes JSON".to_owned()))?,
		)?,
		exclusions: parse_exclusions(
			&serde_json::from_str(&row.8)
				.map_err(|_| StoreError::Incompatible("routing exclusions JSON".to_owned()))?,
		)?,
	};
	Ok(ConversationInitialRoute {
		decision_id: decision_id.to_owned(),
		operation_id: row.0,
		snapshot,
		consumer: ExecutionConsumer::ConversationTurn {
			conversation_id,
			conversation_revision: row.3,
			source_runtime_session_id: None,
			source_runtime_session_revision: None,
			turn_id: turn_id.clone(),
		},
		turn_id,
		decided_at_micros: row.9,
		decision,
	})
}

fn read_continuation_binding(
	connection: &rusqlite::Connection,
	decision_id: &str,
) -> Result<ConversationContinuationBinding, StoreError> {
	let row = connection
		.query_row(
			"SELECT d.conversation_id, d.turn_id, d.conversation_revision,
			        d.source_runtime_session_id, d.source_runtime_session_revision,
			        d.account_snapshot_id, d.account_revision, d.profile_snapshot_id,
			        s.profile_revision, d.created_at_micros, initial.routing_decision_id
			 FROM routing_decisions AS d
			 JOIN runtime_sessions AS s ON s.runtime_session_id = d.source_runtime_session_id
			 JOIN routing_decisions AS initial
			   ON initial.conversation_id = d.conversation_id
			  AND initial.authority_shape = 'conversation_account_registry'
			 WHERE d.routing_decision_id = ?1 AND d.authority_shape = 'conversation_continuation'",
			params![decision_id],
			|row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, i64>(2)?,
					row.get::<_, String>(3)?,
					row.get::<_, i64>(4)?,
					row.get::<_, String>(5)?,
					row.get::<_, i64>(6)?,
					row.get::<_, String>(7)?,
					row.get::<_, i64>(8)?,
					row.get::<_, i64>(9)?,
					row.get::<_, String>(10)?,
				))
			},
		)
		.map_err(sql_error)?;
	let conversation_id = ConversationId::new(row.0)
		.map_err(|_| StoreError::Incompatible("Conversation identity".to_owned()))?;
	let turn_id =
		TurnId::new(row.1).map_err(|_| StoreError::Incompatible("Turn identity".to_owned()))?;
	let runtime_session_id = RuntimeSessionId::new(row.3)
		.map_err(|_| StoreError::Incompatible("RuntimeSession identity".to_owned()))?;
	Ok(ConversationContinuationBinding {
		decision_id: decision_id.to_owned(),
		consumer: ExecutionConsumer::ConversationTurn {
			conversation_id,
			conversation_revision: row.2,
			source_runtime_session_id: Some(runtime_session_id),
			source_runtime_session_revision: Some(row.4),
			turn_id,
		},
		initial_decision_id: row.10,
		account_snapshot_id: row.5,
		account_snapshot_source_revision: row.6,
		profile_snapshot_id: row.7,
		profile_snapshot_source_revision: row.8,
		decided_at_micros: row.9,
	})
}

fn serialize_snapshot(snapshot: &AccountRegistryRoutingSnapshot) -> Result<Value, StoreError> {
	let mode = match &snapshot.mode {
		AccountSelectionMode::Balanced => json!({ "kind": "balanced" }),
		AccountSelectionMode::Fixed(account) => json!({
			"kind": "fixed",
			"account_id": account.as_str(),
		}),
	};
	Ok(json!({
		"snapshot_id": snapshot.snapshot_id,
		"routing_revision": snapshot.routing_revision,
		"mode": mode,
		"task_role_profile_revision": snapshot.task_role_profile_revision,
		"resolved_at_micros": snapshot.resolved_at_micros,
		"members": snapshot.members.iter().map(|member| json!({
			"position": member.position,
			"account_id": member.account_id.as_str(),
			"account_revision": member.account_revision,
			"blockers": member.blockers.iter().map(|blocker| blocker.as_sql()).collect::<Vec<_>>(),
		})).collect::<Vec<_>>(),
		"quota_facts": snapshot.quota_facts.iter().map(|fact| {
			let observation = match fact.observation {
				AccountRegistryQuotaObservation::Missing => json!({ "kind": "missing" }),
				AccountRegistryQuotaObservation::Current { used_percent, observed_at_micros, resets_at_micros } => json!({
					"kind": "current", "used_percent": used_percent,
					"observed_at_micros": observed_at_micros, "resets_at_micros": resets_at_micros,
				}),
				AccountRegistryQuotaObservation::ObservationError { error, observed_at_micros } => json!({
					"kind": "error", "error": quota_error_text(error),
					"observed_at_micros": observed_at_micros,
				}),
			};
			json!({
				"account_id": fact.account_id.as_str(),
				"window": window_text(fact.window),
				"duration_minutes": fact.duration_minutes,
				"observation": observation,
			})
		}).collect::<Vec<_>>(),
	}))
}

fn parse_snapshot(value: &Value) -> Result<AccountRegistryRoutingSnapshot, StoreError> {
	let object = value.as_object().ok_or_else(|| incompatible("routing snapshot"))?;
	let snapshot_id = string(object.get("snapshot_id"))?;
	let routing_revision = integer(object.get("routing_revision"))?;
	let task_role_profile_revision = integer(object.get("task_role_profile_revision"))?;
	let resolved_at_micros = integer(object.get("resolved_at_micros"))?;
	let mode_object = object
		.get("mode")
		.and_then(Value::as_object)
		.ok_or_else(|| incompatible("routing mode"))?;
	let mode = match text(mode_object.get("kind"))? {
		"balanced" => AccountSelectionMode::Balanced,
		"fixed" => AccountSelectionMode::Fixed(
			AccountId::new(string(mode_object.get("account_id"))?)
				.map_err(|_| incompatible("fixed account"))?,
		),
		_ => return Err(incompatible("routing mode")),
	};
	let members = object
		.get("members")
		.and_then(Value::as_array)
		.ok_or_else(|| incompatible("routing members"))?
		.iter()
		.map(parse_member)
		.collect::<Result<Vec<_>, _>>()?;
	let quota_facts = object
		.get("quota_facts")
		.and_then(Value::as_array)
		.ok_or_else(|| incompatible("routing quota"))?
		.iter()
		.map(parse_quota_fact)
		.collect::<Result<Vec<_>, _>>()?;
	Ok(AccountRegistryRoutingSnapshot {
		snapshot_id,
		routing_revision,
		mode,
		task_role_profile_revision,
		resolved_at_micros,
		members,
		quota_facts,
	})
}

fn parse_member(value: &Value) -> Result<AccountRegistryRoutingMember, StoreError> {
	let object = value.as_object().ok_or_else(|| incompatible("routing member"))?;
	let blockers = object
		.get("blockers")
		.and_then(Value::as_array)
		.ok_or_else(|| incompatible("routing blockers"))?
		.iter()
		.map(|value| {
			RoutingBlocker::from_sql(text(Some(value))?)
				.ok_or_else(|| incompatible("routing blocker"))
		})
		.collect::<Result<Vec<_>, _>>()?;
	Ok(AccountRegistryRoutingMember {
		position: usize::try_from(integer(object.get("position"))?)
			.map_err(|_| incompatible("routing position"))?,
		account_id: AccountId::new(string(object.get("account_id"))?)
			.map_err(|_| incompatible("routing account"))?,
		account_revision: integer(object.get("account_revision"))?,
		blockers,
	})
}

fn parse_quota_fact(value: &Value) -> Result<AccountRegistryQuotaFact, StoreError> {
	let object = value.as_object().ok_or_else(|| incompatible("routing quota fact"))?;
	let account_id = AccountId::new(string(object.get("account_id"))?)
		.map_err(|_| incompatible("quota account"))?;
	let window = match text(object.get("window"))? {
		"five_hour" => QuotaWindowClass::FiveHour,
		"seven_day" => QuotaWindowClass::SevenDay,
		_ => return Err(incompatible("quota window")),
	};
	let duration_minutes = u16::try_from(integer(object.get("duration_minutes"))?)
		.map_err(|_| incompatible("quota duration"))?;
	let observation_object = object
		.get("observation")
		.and_then(Value::as_object)
		.ok_or_else(|| incompatible("quota observation"))?;
	let observation = match text(observation_object.get("kind"))? {
		"missing" => AccountRegistryQuotaObservation::Missing,
		"current" => AccountRegistryQuotaObservation::Current {
			used_percent: u8::try_from(integer(observation_object.get("used_percent"))?)
				.map_err(|_| incompatible("quota percent"))?,
			observed_at_micros: integer(observation_object.get("observed_at_micros"))?,
			resets_at_micros: integer(observation_object.get("resets_at_micros"))?,
		},
		"error" => AccountRegistryQuotaObservation::ObservationError {
			error: parse_quota_error(text(observation_object.get("error"))?)?,
			observed_at_micros: integer(observation_object.get("observed_at_micros"))?,
		},
		_ => return Err(incompatible("quota observation kind")),
	};
	Ok(AccountRegistryQuotaFact { account_id, window, duration_minutes, observation })
}

fn serialize_causes(causes: &[RoutingDecisionCause]) -> Value {
	Value::Array(
		causes
			.iter()
			.map(|cause| {
				json!({
					"account_id": cause.account_id.as_str(), "blocker": cause.blocker.as_sql(),
				})
			})
			.collect(),
	)
}

fn parse_causes(value: &Value) -> Result<Vec<RoutingDecisionCause>, StoreError> {
	value
		.as_array()
		.ok_or_else(|| incompatible("routing causes"))?
		.iter()
		.map(|value| {
			let object = value.as_object().ok_or_else(|| incompatible("routing cause"))?;
			Ok(RoutingDecisionCause {
				account_id: AccountId::new(string(object.get("account_id"))?)
					.map_err(|_| incompatible("routing cause account"))?,
				blocker: RoutingBlocker::from_sql(text(object.get("blocker"))?)
					.ok_or_else(|| incompatible("routing cause blocker"))?,
			})
		})
		.collect()
}

fn serialize_exclusions(exclusions: &[decodex_core::AccountRegistryRoutingExclusion]) -> Value {
	Value::Array(
		exclusions
			.iter()
			.map(|exclusion| {
				json!({
					"account_id": exclusion.account_id.as_str(),
					"member_position": exclusion.member_position,
					"window": window_text(exclusion.window),
					"duration_minutes": exclusion.duration_minutes,
					"used_percent": exclusion.used_percent,
					"observed_at_micros": exclusion.observed_at_micros,
					"resets_at_micros": exclusion.resets_at_micros,
				})
			})
			.collect(),
	)
}

fn parse_exclusions(
	value: &Value,
) -> Result<Vec<decodex_core::AccountRegistryRoutingExclusion>, StoreError> {
	value
		.as_array()
		.ok_or_else(|| incompatible("routing exclusions"))?
		.iter()
		.map(|value| {
			let object = value.as_object().ok_or_else(|| incompatible("routing exclusion"))?;
			Ok(decodex_core::AccountRegistryRoutingExclusion {
				account_id: AccountId::new(string(object.get("account_id"))?)
					.map_err(|_| incompatible("routing exclusion account"))?,
				member_position: usize::try_from(integer(object.get("member_position"))?)
					.map_err(|_| incompatible("routing exclusion position"))?,
				window: match text(object.get("window"))? {
					"five_hour" => QuotaWindowClass::FiveHour,
					"seven_day" => QuotaWindowClass::SevenDay,
					_ => return Err(incompatible("routing exclusion window")),
				},
				duration_minutes: u16::try_from(integer(object.get("duration_minutes"))?)
					.map_err(|_| incompatible("routing exclusion duration"))?,
				used_percent: u8::try_from(integer(object.get("used_percent"))?)
					.map_err(|_| incompatible("routing exclusion percentage"))?,
				observed_at_micros: integer(object.get("observed_at_micros"))?,
				resets_at_micros: integer(object.get("resets_at_micros"))?,
			})
		})
		.collect()
}

fn quota_classification(
	decision: &AccountRegistryRoutingDecision,
	snapshot: &AccountRegistryRoutingSnapshot,
) -> &'static str {
	if decision.kind == AccountRegistryRoutingDecisionKind::Waiting {
		return "known_depleted";
	}
	let Some(account) = decision.selected_account_id.as_ref() else {
		return "unknown";
	};
	let complete = snapshot.quota_facts.iter().filter(|fact| &fact.account_id == account).all(|fact| {
		matches!(fact.observation, AccountRegistryQuotaObservation::Current { used_percent, .. } if used_percent < 100)
	});
	if complete { "known_available" } else { "unknown" }
}

fn decision_kind_text(kind: AccountRegistryRoutingDecisionKind) -> &'static str {
	match kind {
		AccountRegistryRoutingDecisionKind::Selected => "selected",
		AccountRegistryRoutingDecisionKind::Waiting => "waiting",
		AccountRegistryRoutingDecisionKind::NoRoute => "no_route",
	}
}

fn parse_decision_kind(value: &str) -> Result<AccountRegistryRoutingDecisionKind, StoreError> {
	match value {
		"selected" => Ok(AccountRegistryRoutingDecisionKind::Selected),
		"waiting" => Ok(AccountRegistryRoutingDecisionKind::Waiting),
		"no_route" => Ok(AccountRegistryRoutingDecisionKind::NoRoute),
		_ => Err(incompatible("routing decision kind")),
	}
}

fn window_text(window: QuotaWindowClass) -> &'static str {
	match window {
		QuotaWindowClass::FiveHour => "five_hour",
		QuotaWindowClass::SevenDay => "seven_day",
	}
}

fn quota_error_text(error: AccountQuotaObservationError) -> &'static str {
	match error {
		AccountQuotaObservationError::ProviderUnavailable => "provider_unavailable",
		AccountQuotaObservationError::ProtocolUnavailable => "protocol_unavailable",
		AccountQuotaObservationError::AccountMismatch => "account_mismatch",
		AccountQuotaObservationError::UnsupportedWindow => "unsupported_window",
	}
}

fn parse_quota_error(value: &str) -> Result<AccountQuotaObservationError, StoreError> {
	match value {
		"provider_unavailable" => Ok(AccountQuotaObservationError::ProviderUnavailable),
		"protocol_unavailable" => Ok(AccountQuotaObservationError::ProtocolUnavailable),
		"account_mismatch" => Ok(AccountQuotaObservationError::AccountMismatch),
		"unsupported_window" => Ok(AccountQuotaObservationError::UnsupportedWindow),
		_ => Err(incompatible("quota error")),
	}
}

fn route_request_sha(request: &RouteConversationInitial) -> String {
	digest(&[request.conversation_id.as_str(), &request.expected_conversation_revision.to_string()])
}

fn continuation_request_sha(request: &BindConversationContinuation) -> String {
	digest(&[
		&request.operation_id,
		request.conversation_id.as_str(),
		&request.expected_conversation_revision.to_string(),
		request.source_runtime_session_id.as_str(),
		&request.expected_source_runtime_session_revision.to_string(),
		request.turn_id.as_str(),
	])
}

fn digest(parts: &[&str]) -> String {
	let mut digest = Sha256::new();
	for part in parts {
		digest.update(part.len().to_be_bytes());
		digest.update(part.as_bytes());
	}
	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_key(key: &str) -> Result<(), StoreError> {
	if key.is_empty() || key.len() > 256 || decodex_core::contains_credential_material(key) {
		return Err(StoreError::InvalidInput("idempotency key is invalid"));
	}
	Ok(())
}

fn string(value: Option<&Value>) -> Result<String, StoreError> {
	text(value).map(str::to_owned)
}

fn text(value: Option<&Value>) -> Result<&str, StoreError> {
	value.and_then(Value::as_str).ok_or_else(|| incompatible("JSON text"))
}

fn integer(value: Option<&Value>) -> Result<i64, StoreError> {
	value.and_then(Value::as_i64).ok_or_else(|| incompatible("JSON integer"))
}

fn incompatible(reason: &'static str) -> StoreError {
	StoreError::Incompatible(format!("stored {reason} is malformed"))
}
