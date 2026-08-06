//! PostgreSQL-owned Account Registry routing for ordinary Quick Tasks.
//!
//! The initial command keeps one database transaction open while the core selection kernel runs.
//! The database owns the locked source universe, immutable snapshot, generated coordinates,
//! decision evidence, receipt, activity, and outbox. Later Turns bind immutable lineage here
//! without resolving current Account Registry authority or selecting again.

use decodex_core::{
	AccountId, AccountQuotaObservationError, AccountRegistryQuotaFact,
	AccountRegistryQuotaObservation, AccountRegistryRoutingDecision,
	AccountRegistryRoutingDecisionKind, AccountRegistryRoutingMember,
	AccountRegistryRoutingSnapshot, AccountSelectionMode, ConversationId, ExecutionConsumer,
	QuotaWindowClass, RoutingBlocker, RoutingCommandOutcome, RoutingDecisionCause,
	RoutingRejection, RuntimeSessionId, TurnId, decide_account_registry_routing,
};
use serde_json::{Value, json};

use crate::{
	PostgresStore, StoreError,
	exact_commands::{
		EXACT_COMMAND_PROTOCOL, MAX_EXACT_ATTEMPTS, is_retryable_exact_database_error,
		validate_exact_effect_digest, validate_exact_key,
	},
};

const BEGIN_INITIAL_ROUTE_SQL: &str = "SELECT disposition,response_bytes,snapshot_envelope \
	 FROM decodex.begin_quick_task_initial_route_exact($1,$2,$3::text::uuid,$4)";
const COMPLETE_INITIAL_ROUTE_SQL: &str = "SELECT \
	 decodex.complete_quick_task_initial_route_exact(\
	 $1,$2,$3::text::uuid,$4,$5::text::uuid,$6::text::decodex.routing_decision_kind,\
	 $7::text::uuid,$8,$9)";
const READ_INITIAL_ROUTE_SQL: &str = "SELECT \
	 decodex.read_quick_task_initial_route_exact($1::text::uuid)";
const BIND_CONTINUATION_SQL: &str = "SELECT \
	 decodex.bind_quick_task_continuation_exact(\
	 $1,$2,$3::text::uuid,$4::text::uuid,$5,$6::text::uuid,$7,$8::text::uuid)";

#[cfg(all(test, feature = "test-support"))]
pub(crate) async fn prepare_quick_task_routing_sql(
	client: &tokio_postgres::Client,
) -> Result<usize, StoreError> {
	const SOURCES: [&str; 4] = [
		BEGIN_INITIAL_ROUTE_SQL,
		COMPLETE_INITIAL_ROUTE_SQL,
		READ_INITIAL_ROUTE_SQL,
		BIND_CONTINUATION_SQL,
	];
	for source in SOURCES {
		client.prepare(source).await?;
	}
	Ok(SOURCES.len())
}

/// Exact Conversation coordinates for its sole initial Account Registry route.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouteQuickTaskInitial {
	/// Ordinary Conversation whose initial route is absent.
	pub conversation_id: ConversationId,
	/// Exact open Conversation revision.
	pub expected_conversation_revision: i64,
}

/// Immutable initial Account Registry route and database-owned prospective Turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskInitialRoute {
	/// Database-generated immutable Routing Decision identity.
	pub decision_id: String,
	/// Database-generated semantic operation identity.
	pub operation_id: String,
	/// Exact immutable snapshot consumed by the pure core kernel.
	pub snapshot: AccountRegistryRoutingSnapshot,
	/// Exact initial Conversation Turn consumer.
	pub consumer: ExecutionConsumer,
	/// Database-generated prospective initial user Turn.
	pub turn_id: TurnId,
	/// PostgreSQL-owned decision instant in Unix microseconds.
	pub decided_at_micros: i64,
	/// Revalidated pure-kernel result committed with complete evidence.
	pub decision: AccountRegistryRoutingDecision,
}

/// Exact initial-route result, including stable receipt replay classification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuickTaskInitialRouteOutcome {
	/// The route committed in this call.
	Fresh(QuickTaskInitialRoute),
	/// The same exact command returned its committed immutable response.
	Replayed(QuickTaskInitialRoute),
	/// A stable domain refusal committed in this call.
	Rejected(RoutingRejection),
	/// The same stable domain refusal was replayed exactly.
	ReplayedRejection(RoutingRejection),
}

/// Exact non-selecting continuation binding over one active Quick Task session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindQuickTaskContinuation {
	/// Stable semantic operation identity distinct from the exact command key.
	pub operation_id: String,
	/// Owning ordinary Conversation.
	pub conversation_id: ConversationId,
	/// Exact current Conversation revision.
	pub expected_conversation_revision: i64,
	/// Exact active source RuntimeSession.
	pub source_runtime_session_id: RuntimeSessionId,
	/// Exact current source RuntimeSession revision.
	pub expected_source_runtime_session_revision: i64,
	/// New logical Turn identity, either unmaterialized or the exact active source-session Turn.
	pub turn_id: TurnId,
}

/// Immutable non-selecting later-Turn routing lineage.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuickTaskContinuationBinding {
	/// Identity of the immutable `conversation_continuation` Routing Decision.
	pub decision_id: String,
	/// Exact Conversation Turn consumer committed with the decision.
	pub consumer: ExecutionConsumer,
	/// Initial selected decision whose Account Registry route remains pinned.
	pub initial_decision_id: String,
	/// Existing selected-account snapshot consumed by continuation planning.
	pub account_snapshot_id: String,
	/// Exact source account revision copied into the decision.
	pub account_snapshot_source_revision: i64,
	/// Existing Task profile snapshot consumed by continuation planning.
	pub profile_snapshot_id: String,
	/// Exact source profile revision copied into the decision.
	pub profile_snapshot_source_revision: i64,
	/// PostgreSQL decision instant in Unix microseconds.
	pub decided_at_micros: i64,
}

impl PostgresStore {
	/// Route one ordinary Conversation from one repeatable locked Account Registry universe.
	pub async fn route_quick_task_initial(
		&self,
		idempotency_key: &str,
		request: &RouteQuickTaskInitial,
	) -> Result<QuickTaskInitialRouteOutcome, StoreError> {
		validate_exact_key(idempotency_key)?;
		if request.expected_conversation_revision <= 0 {
			return Err(StoreError::InvalidInput("Conversation revision must be positive"));
		}

		let mut last_retryable = None;
		for _ in 0..MAX_EXACT_ATTEMPTS {
			let mut client = self.pool().get().await?;
			let transaction = match client.transaction().await {
				Ok(transaction) => transaction,
				Err(error) if is_retryable_exact_database_error(&error) => {
					last_retryable = Some(error);
					continue;
				},
				Err(error) => return Err(StoreError::from(error)),
			};
			let row = match transaction
				.query_one(
					BEGIN_INITIAL_ROUTE_SQL,
					&[
						&EXACT_COMMAND_PROTOCOL,
						&idempotency_key,
						&request.conversation_id.as_str(),
						&request.expected_conversation_revision,
					],
				)
				.await
			{
				Ok(row) => row,
				Err(error) if is_retryable_exact_database_error(&error) => {
					last_retryable = Some(error);
					continue;
				},
				Err(error) => return Err(StoreError::from(error)),
			};
			let disposition: &str = row.get(0);
			if disposition != "fresh" {
				let response: Option<Vec<u8>> = row.get(1);
				let response = response.ok_or_else(|| {
					StoreError::Incompatible("completed initial route response is absent".into())
				})?;
				let replayed = disposition == "replayed";
				if !replayed && disposition != "rejected" {
					return incompatible("initial route disposition is unknown");
				}
				let outcome = parse_initial_route_response(&response, request, replayed)?;
				match transaction.commit().await {
					Ok(()) => return Ok(outcome),
					Err(error) if is_retryable_exact_database_error(&error) => {
						last_retryable = Some(error);
						continue;
					},
					Err(error) => return Err(StoreError::from(error)),
				}
			}

			let envelope: Option<Value> = row.get(2);
			let snapshot = parse_snapshot_envelope(
				envelope.as_ref().ok_or_else(|| {
					StoreError::Incompatible("fresh initial route snapshot is absent".into())
				})?,
				request,
			)?;
			let expected = decide_account_registry_routing(&snapshot, snapshot.resolved_at_micros)
				.map_err(|_| {
					StoreError::Incompatible(
						"database-authored Account Registry snapshot is incomplete".into(),
					)
				})?;
			let selected_account = expected.selected_account_id.as_ref().map(AccountId::as_str);
			let causes = routing_causes_json(&expected.causes);
			let exclusions = routing_exclusions_json(&expected.exclusions);
			let kind = decision_kind_sql(expected.kind);
			let response = match transaction
				.query_one(
					COMPLETE_INITIAL_ROUTE_SQL,
					&[
						&EXACT_COMMAND_PROTOCOL,
						&idempotency_key,
						&request.conversation_id.as_str(),
						&request.expected_conversation_revision,
						&snapshot.snapshot_id,
						&kind,
						&selected_account,
						&causes,
						&exclusions,
					],
				)
				.await
			{
				Ok(row) => row.get::<_, Vec<u8>>(0),
				Err(error) if is_retryable_exact_database_error(&error) => {
					last_retryable = Some(error);
					continue;
				},
				Err(error) => return Err(StoreError::from(error)),
			};
			let outcome = parse_initial_route_response_with_snapshot(
				&response,
				request,
				false,
				Some((&snapshot, &expected)),
			)?;
			match transaction.commit().await {
				Ok(()) => return Ok(outcome),
				Err(error) if is_retryable_exact_database_error(&error) => {
					last_retryable = Some(error);
				},
				Err(error) => return Err(StoreError::from(error)),
			}
		}

		Err(StoreError::Database(
			last_retryable
				.expect("an exhausted initial route retry loop retains its infrastructure failure"),
		))
	}

	/// Read the sole committed initial route for recovery without resolving current authority.
	pub async fn read_quick_task_initial_route(
		&self,
		conversation_id: &ConversationId,
	) -> Result<Option<QuickTaskInitialRoute>, StoreError> {
		let row = self
			.pool()
			.get()
			.await?
			.query_one(READ_INITIAL_ROUTE_SQL, &[&conversation_id.as_str()])
			.await?;
		let value: Option<Value> = row.get(0);
		value
			.as_ref()
			.map(|value| {
				validate_exact_effect_digest(value)?;
				parse_initial_route_effect(value, None, None)
			})
			.transpose()
	}

	/// Bind one later Quick Task Turn to its original route without selecting.
	pub async fn bind_quick_task_continuation(
		&self,
		idempotency_key: &str,
		request: &BindQuickTaskContinuation,
	) -> Result<RoutingCommandOutcome<QuickTaskContinuationBinding>, StoreError> {
		validate_exact_key(idempotency_key)?;
		if !is_canonical_uuid(&request.operation_id)
			|| request.expected_conversation_revision <= 0
			|| request.expected_source_runtime_session_revision <= 0
		{
			return Err(StoreError::InvalidInput(
				"Quick Task continuation coordinates are invalid",
			));
		}
		let response = self
			.execute_exact_with_retry(
				BIND_CONTINUATION_SQL,
				&[
					&EXACT_COMMAND_PROTOCOL,
					&idempotency_key,
					&request.operation_id,
					&request.conversation_id.as_str(),
					&request.expected_conversation_revision,
					&request.source_runtime_session_id.as_str(),
					&request.expected_source_runtime_session_revision,
					&request.turn_id.as_str(),
				],
			)
			.await?;
		parse_continuation_binding(&response, request)
	}
}

fn routing_causes_json(causes: &[RoutingDecisionCause]) -> Value {
	Value::Array(
		causes
			.iter()
			.map(|cause| {
				json!({
					"account_id": cause.account_id.as_str(),
					"blocker": cause.blocker.as_sql(),
				})
			})
			.collect(),
	)
}

fn routing_exclusions_json(exclusions: &[decodex_core::AccountRegistryRoutingExclusion]) -> Value {
	Value::Array(
		exclusions
			.iter()
			.map(|exclusion| {
				json!({
					"account_id": exclusion.account_id.as_str(),
					"member_position": exclusion.member_position,
					"window_class": quota_window_sql(exclusion.window),
					"duration_minutes": exclusion.duration_minutes,
					"used_percent": exclusion.used_percent,
					"observed_at_micros": exclusion.observed_at_micros,
					"resets_at_micros": exclusion.resets_at_micros,
				})
			})
			.collect(),
	)
}

fn parse_snapshot_envelope(
	value: &Value,
	request: &RouteQuickTaskInitial,
) -> Result<AccountRegistryRoutingSnapshot, StoreError> {
	if text(value, "operation")? != "route_quick_task_initial"
		|| text(value, "authority_shape")? != "conversation_account_registry"
		|| text(value, "conversation_id")? != request.conversation_id.as_str()
		|| positive_i64(value, "conversation_revision")? != request.expected_conversation_revision
	{
		return incompatible("fresh initial route snapshot is cross-linked");
	}
	parse_snapshot(value)
}

fn parse_snapshot(value: &Value) -> Result<AccountRegistryRoutingSnapshot, StoreError> {
	let mode = match text(value, "account_selection_mode")? {
		"balanced" => {
			if optional_text(value, "fixed_account_id")?.is_some() {
				return incompatible("balanced initial route has a fixed account");
			}
			AccountSelectionMode::Balanced
		},
		"fixed" => AccountSelectionMode::Fixed(account_id(value, "fixed_account_id")?),
		_ => return incompatible("initial route selection mode is unknown"),
	};
	let members = array(value, "members")?
		.iter()
		.map(|member| {
			Ok(AccountRegistryRoutingMember {
				position: positive_usize(member, "position")?,
				account_id: account_id(member, "account_id")?,
				account_revision: positive_i64(member, "account_revision")?,
				blockers: array(member, "blockers")?
					.iter()
					.map(|blocker| {
						blocker
							.as_str()
							.ok_or_else(|| {
								StoreError::Incompatible(
									"initial route member blocker is invalid".into(),
								)
							})
							.and_then(parse_blocker)
					})
					.collect::<Result<Vec<_>, _>>()?,
			})
		})
		.collect::<Result<Vec<_>, StoreError>>()?;
	let quota_facts = array(value, "quota_facts")?
		.iter()
		.map(|fact| {
			let window = parse_quota_window(text(fact, "window_class")?)?;
			let observation = match text(fact, "observation_state")? {
				"missing" => AccountRegistryQuotaObservation::Missing,
				"current" => AccountRegistryQuotaObservation::Current {
					used_percent: bounded_u8(fact, "used_percent")?,
					observed_at_micros: nonnegative_i64(fact, "observed_at_micros")?,
					resets_at_micros: nonnegative_i64(fact, "resets_at_micros")?,
				},
				"observation_error" => AccountRegistryQuotaObservation::ObservationError {
					error: parse_quota_error(text(fact, "error_code")?)?,
					observed_at_micros: nonnegative_i64(fact, "observed_at_micros")?,
				},
				_ => return incompatible("initial route quota state is unknown"),
			};
			Ok(AccountRegistryQuotaFact {
				account_id: account_id(fact, "account_id")?,
				window,
				duration_minutes: u16::try_from(positive_i64(fact, "duration_minutes")?).map_err(
					|_| StoreError::Incompatible("initial route quota duration is invalid".into()),
				)?,
				observation,
			})
		})
		.collect::<Result<Vec<_>, StoreError>>()?;

	Ok(AccountRegistryRoutingSnapshot {
		snapshot_id: canonical_uuid(value, "snapshot_id")?,
		routing_revision: positive_i64(value, "account_routing_revision")?,
		mode,
		task_role_profile_revision: positive_i64(value, "task_role_profile_revision")?,
		resolved_at_micros: nonnegative_i64(value, "resolved_at_micros")?,
		members,
		quota_facts,
	})
}

fn parse_initial_route_response(
	response: &[u8],
	request: &RouteQuickTaskInitial,
	replayed: bool,
) -> Result<QuickTaskInitialRouteOutcome, StoreError> {
	parse_initial_route_response_with_snapshot(response, request, replayed, None)
}

fn parse_initial_route_response_with_snapshot(
	response: &[u8],
	request: &RouteQuickTaskInitial,
	replayed: bool,
	expected: Option<(&AccountRegistryRoutingSnapshot, &AccountRegistryRoutingDecision)>,
) -> Result<QuickTaskInitialRouteOutcome, StoreError> {
	let document: Value = serde_json::from_slice(response)
		.map_err(|_| StoreError::Incompatible("initial route response is invalid".into()))?;
	let classification = text(&document, "classification")?;
	let effect = object(&document, "effect")?;
	validate_exact_effect_digest(effect)?;
	if classification == "stable_domain_rejection" {
		let code = text(effect, "rejection")?;
		if text(effect, "operation")? != "route_quick_task_initial"
			|| !matches!(
				code,
				"malformed_input"
					| "conversation_mismatch"
					| "initial_routing_already_bound"
					| "routing_authority_unavailable"
			) {
			return incompatible("initial route rejection is unknown or cross-linked");
		}
		let rejection = RoutingRejection {
			operation: "route_quick_task_initial".to_owned(),
			code: code.to_owned(),
		};
		return Ok(if replayed {
			QuickTaskInitialRouteOutcome::ReplayedRejection(rejection)
		} else {
			QuickTaskInitialRouteOutcome::Rejected(rejection)
		});
	}
	if classification != "success" {
		return incompatible("initial route response classification is unknown");
	}
	let route = parse_initial_route_effect(effect, Some(request), expected)?;
	Ok(if replayed {
		QuickTaskInitialRouteOutcome::Replayed(route)
	} else {
		QuickTaskInitialRouteOutcome::Fresh(route)
	})
}

fn parse_initial_route_effect(
	effect: &Value,
	request: Option<&RouteQuickTaskInitial>,
	expected: Option<(&AccountRegistryRoutingSnapshot, &AccountRegistryRoutingDecision)>,
) -> Result<QuickTaskInitialRoute, StoreError> {
	if text(effect, "operation")? != "route_quick_task_initial"
		|| text(effect, "authority_shape")? != "conversation_account_registry"
		|| text(effect, "consumer_kind")? != "conversation_turn"
	{
		return incompatible("initial route success is cross-linked");
	}
	let conversation_id = ConversationId::new(text(effect, "conversation_id")?.to_owned())
		.map_err(|_| StoreError::Incompatible("initial route Conversation is invalid".into()))?;
	let conversation_revision = positive_i64(effect, "conversation_revision")?;
	if request.is_some_and(|request| {
		request.conversation_id != conversation_id
			|| request.expected_conversation_revision != conversation_revision
	}) {
		return incompatible("initial route success has different Conversation coordinates");
	}
	let turn_id = TurnId::new(canonical_uuid(effect, "turn_id")?)
		.map_err(|_| StoreError::Incompatible("initial route Turn is invalid".into()))?;
	let snapshot_value = object(effect, "routing_snapshot")?;
	let snapshot = parse_snapshot(snapshot_value)?;
	if text(effect, "snapshot_id")? != snapshot.snapshot_id
		|| positive_i64(effect, "account_routing_revision")? != snapshot.routing_revision
		|| positive_i64(effect, "activity_sequence")? <= 0
		|| positive_i64(effect, "outbox_id")? <= 0
	{
		return incompatible("initial route success has inconsistent committed coordinates");
	}
	let decided_at_micros = nonnegative_i64(effect, "decided_at_micros")?;
	let actual = parse_decision(effect, &snapshot.snapshot_id)?;
	if decided_at_micros != snapshot.resolved_at_micros
		|| !valid_committed_decision_shape(&snapshot, &actual)
		|| expected.is_some_and(|(expected_snapshot, expected_decision)| {
			&snapshot != expected_snapshot || &actual != expected_decision
		}) {
		return incompatible("committed initial route differs from the pure routing kernel");
	}
	Ok(QuickTaskInitialRoute {
		decision_id: canonical_uuid(effect, "decision_id")?,
		operation_id: canonical_uuid(effect, "operation_id")?,
		snapshot,
		consumer: ExecutionConsumer::ConversationTurn {
			conversation_id,
			conversation_revision,
			source_runtime_session_id: None,
			source_runtime_session_revision: None,
			turn_id: turn_id.clone(),
		},
		turn_id,
		decided_at_micros,
		decision: actual,
	})
}

fn valid_committed_decision_shape(
	snapshot: &AccountRegistryRoutingSnapshot,
	decision: &AccountRegistryRoutingDecision,
) -> bool {
	let selected_is_member = decision.selected_account_id.as_ref().is_some_and(|selected| {
		snapshot.members.iter().any(|member| &member.account_id == selected)
			&& match &snapshot.mode {
				AccountSelectionMode::Fixed(fixed) => fixed == selected,
				AccountSelectionMode::Balanced => true,
			}
	});
	let kind_is_valid = match decision.kind {
		AccountRegistryRoutingDecisionKind::Selected => selected_is_member,
		AccountRegistryRoutingDecisionKind::Waiting =>
			decision.selected_account_id.is_none()
				&& !decision.exclusions.is_empty()
				&& decision.causes.is_empty(),
		AccountRegistryRoutingDecisionKind::NoRoute =>
			decision.selected_account_id.is_none() && !decision.causes.is_empty(),
	};
	let causes_are_members = decision.causes.iter().enumerate().all(|(index, cause)| {
		snapshot.members.iter().any(|member| member.account_id == cause.account_id)
			&& decision.causes[..index].iter().all(|prior| prior != cause)
	});
	let exclusions_are_exact = decision.exclusions.iter().enumerate().all(|(index, exclusion)| {
		let member_matches = snapshot.members.iter().any(|member| {
			member.account_id == exclusion.account_id
				&& member.position == exclusion.member_position
		});
		let fact_matches = snapshot.quota_facts.iter().any(|fact| {
			fact.account_id == exclusion.account_id
				&& fact.window == exclusion.window
				&& fact.duration_minutes == exclusion.duration_minutes
				&& matches!(
					&fact.observation,
					AccountRegistryQuotaObservation::Current {
						used_percent,
						observed_at_micros,
						resets_at_micros,
					} if *used_percent == exclusion.used_percent
						&& *used_percent == 100
						&& *observed_at_micros == exclusion.observed_at_micros
						&& *resets_at_micros == exclusion.resets_at_micros
				)
		});
		member_matches
			&& fact_matches
			&& decision.exclusions[..index].iter().all(|prior| {
				prior.account_id != exclusion.account_id || prior.window != exclusion.window
			})
	});
	decision.snapshot_id == snapshot.snapshot_id
		&& kind_is_valid
		&& causes_are_members
		&& exclusions_are_exact
}

fn parse_decision(
	effect: &Value,
	snapshot_id: &str,
) -> Result<AccountRegistryRoutingDecision, StoreError> {
	let kind = match text(effect, "kind")? {
		"selected" => AccountRegistryRoutingDecisionKind::Selected,
		"waiting" => AccountRegistryRoutingDecisionKind::Waiting,
		"no_route" => AccountRegistryRoutingDecisionKind::NoRoute,
		_ => return incompatible("initial route decision kind is unknown"),
	};
	let selected_account_id = optional_text(effect, "selected_account_id")?
		.map(AccountId::new)
		.transpose()
		.map_err(|_| StoreError::Incompatible("selected account is invalid".into()))?;
	let exclusions = array(effect, "exclusions")?
		.iter()
		.map(|value| {
			Ok(decodex_core::AccountRegistryRoutingExclusion {
				account_id: account_id(value, "account_id")?,
				member_position: positive_usize(value, "member_position")?,
				window: parse_quota_window(text(value, "window_class")?)?,
				duration_minutes: u16::try_from(positive_i64(value, "duration_minutes")?).map_err(
					|_| StoreError::Incompatible("route exclusion duration is invalid".into()),
				)?,
				used_percent: bounded_u8(value, "used_percent")?,
				observed_at_micros: nonnegative_i64(value, "observed_at_micros")?,
				resets_at_micros: nonnegative_i64(value, "resets_at_micros")?,
			})
		})
		.collect::<Result<Vec<_>, StoreError>>()?;
	let causes = array(effect, "causes")?
		.iter()
		.map(|value| {
			Ok(RoutingDecisionCause {
				account_id: account_id(value, "account_id")?,
				blocker: parse_blocker(text(value, "blocker")?)?,
			})
		})
		.collect::<Result<Vec<_>, StoreError>>()?;
	Ok(AccountRegistryRoutingDecision {
		snapshot_id: snapshot_id.to_owned(),
		kind,
		selected_account_id,
		exclusions,
		causes,
	})
}

fn parse_continuation_binding(
	response: &[u8],
	request: &BindQuickTaskContinuation,
) -> Result<RoutingCommandOutcome<QuickTaskContinuationBinding>, StoreError> {
	let document: Value = serde_json::from_slice(response).map_err(|_| {
		StoreError::Incompatible("Quick Task continuation response is invalid".into())
	})?;
	let classification = text(&document, "classification")?;
	let effect = object(&document, "effect")?;
	validate_exact_effect_digest(effect)?;
	if classification == "stable_domain_rejection" {
		let code = text(effect, "rejection")?;
		if text(effect, "operation")? != "bind_quick_task_continuation"
			|| !matches!(
				code,
				"malformed_input"
					| "conversation_mismatch"
					| "continuation_already_bound"
					| "continuation_lineage_mismatch"
			) {
			return incompatible("Quick Task continuation rejection is unknown or cross-linked");
		}
		return Ok(RoutingCommandOutcome::Rejected(RoutingRejection {
			operation: "bind_quick_task_continuation".to_owned(),
			code: code.to_owned(),
		}));
	}
	if classification != "success"
		|| text(effect, "operation")? != "bind_quick_task_continuation"
		|| text(effect, "authority_shape")? != "conversation_continuation"
		|| text(effect, "operation_id")? != request.operation_id
		|| text(effect, "consumer_kind")? != "conversation_turn"
		|| text(effect, "conversation_id")? != request.conversation_id.as_str()
		|| positive_i64(effect, "conversation_revision")? != request.expected_conversation_revision
		|| text(effect, "turn_id")? != request.turn_id.as_str()
		|| text(effect, "source_runtime_session_id")? != request.source_runtime_session_id.as_str()
		|| positive_i64(effect, "source_runtime_session_revision")?
			!= request.expected_source_runtime_session_revision
	{
		return incompatible("Quick Task continuation success is cross-linked");
	}
	Ok(RoutingCommandOutcome::Success(QuickTaskContinuationBinding {
		decision_id: canonical_uuid(effect, "decision_id")?,
		consumer: ExecutionConsumer::ConversationTurn {
			conversation_id: request.conversation_id.clone(),
			conversation_revision: request.expected_conversation_revision,
			source_runtime_session_id: Some(request.source_runtime_session_id.clone()),
			source_runtime_session_revision: Some(request.expected_source_runtime_session_revision),
			turn_id: request.turn_id.clone(),
		},
		initial_decision_id: canonical_uuid(effect, "initial_decision_id")?,
		account_snapshot_id: canonical_uuid(effect, "account_snapshot_id")?,
		account_snapshot_source_revision: positive_i64(effect, "account_snapshot_source_revision")?,
		profile_snapshot_id: canonical_uuid(effect, "profile_snapshot_id")?,
		profile_snapshot_source_revision: positive_i64(effect, "profile_snapshot_source_revision")?,
		decided_at_micros: nonnegative_i64(effect, "decided_at_micros")?,
	}))
}

fn parse_blocker(value: &str) -> Result<RoutingBlocker, StoreError> {
	Ok(match value {
		"account_from_future" => RoutingBlocker::AccountFromFuture,
		"account_stale" => RoutingBlocker::AccountStale,
		"account_unavailable" => RoutingBlocker::AccountUnavailable,
		"account_unknown" => RoutingBlocker::AccountUnknown,
		"account_depleted" => RoutingBlocker::AccountDepleted,
		"account_auth_failed" => RoutingBlocker::AccountAuthFailed,
		"account_plugin_unready" => RoutingBlocker::AccountPluginUnready,
		"account_disabled" => RoutingBlocker::AccountDisabled,
		"quota_five_hour_missing" => RoutingBlocker::QuotaFiveHourMissing,
		"quota_five_hour_from_future" => RoutingBlocker::QuotaFiveHourFromFuture,
		"quota_five_hour_stale" => RoutingBlocker::QuotaFiveHourStale,
		"quota_five_hour_unknown" => RoutingBlocker::QuotaFiveHourUnknown,
		"quota_five_hour_reset_elapsed" => RoutingBlocker::QuotaFiveHourResetElapsed,
		"quota_seven_day_missing" => RoutingBlocker::QuotaSevenDayMissing,
		"quota_seven_day_from_future" => RoutingBlocker::QuotaSevenDayFromFuture,
		"quota_seven_day_stale" => RoutingBlocker::QuotaSevenDayStale,
		"quota_seven_day_unknown" => RoutingBlocker::QuotaSevenDayUnknown,
		"quota_seven_day_reset_elapsed" => RoutingBlocker::QuotaSevenDayResetElapsed,
		_ => return incompatible("initial route blocker is unknown"),
	})
}

fn parse_quota_error(value: &str) -> Result<AccountQuotaObservationError, StoreError> {
	Ok(match value {
		"provider_unavailable" => AccountQuotaObservationError::ProviderUnavailable,
		"protocol_unavailable" => AccountQuotaObservationError::ProtocolUnavailable,
		"account_mismatch" => AccountQuotaObservationError::AccountMismatch,
		"unsupported_window" => AccountQuotaObservationError::UnsupportedWindow,
		_ => return incompatible("initial route quota error is unknown"),
	})
}

fn parse_quota_window(value: &str) -> Result<QuotaWindowClass, StoreError> {
	match value {
		"five_hour" => Ok(QuotaWindowClass::FiveHour),
		"seven_day" => Ok(QuotaWindowClass::SevenDay),
		_ => incompatible("initial route quota window is unknown"),
	}
}

const fn quota_window_sql(value: QuotaWindowClass) -> &'static str {
	match value {
		QuotaWindowClass::FiveHour => "five_hour",
		QuotaWindowClass::SevenDay => "seven_day",
	}
}

const fn decision_kind_sql(value: AccountRegistryRoutingDecisionKind) -> &'static str {
	match value {
		AccountRegistryRoutingDecisionKind::Selected => "selected",
		AccountRegistryRoutingDecisionKind::Waiting => "waiting",
		AccountRegistryRoutingDecisionKind::NoRoute => "no_route",
	}
}

fn account_id(value: &Value, key: &str) -> Result<AccountId, StoreError> {
	AccountId::new(text(value, key)?.to_owned())
		.map_err(|_| StoreError::Incompatible(format!("initial route {key} is invalid")))
}

fn canonical_uuid(value: &Value, key: &str) -> Result<String, StoreError> {
	let value = text(value, key)?;
	if !is_canonical_uuid(value) {
		return incompatible("database-authored UUID is invalid");
	}
	Ok(value.to_owned())
}

fn is_canonical_uuid(value: &str) -> bool {
	value.len() == 36
		&& value.as_bytes().iter().enumerate().all(|(index, byte)| match index {
			8 | 13 | 18 | 23 => *byte == b'-',
			14 => *byte == b'4',
			19 => matches!(*byte, b'8' | b'9' | b'a' | b'b'),
			_ => byte.is_ascii_digit() || matches!(*byte, b'a'..=b'f'),
		})
}

fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Value, StoreError> {
	value
		.get(key)
		.filter(|value| value.is_object())
		.ok_or_else(|| StoreError::Incompatible(format!("{key} is not an object")))
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], StoreError> {
	value
		.get(key)
		.and_then(Value::as_array)
		.map(Vec::as_slice)
		.ok_or_else(|| StoreError::Incompatible(format!("{key} is not an array")))
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, StoreError> {
	value
		.get(key)
		.and_then(Value::as_str)
		.ok_or_else(|| StoreError::Incompatible(format!("{key} is not text")))
}

fn optional_text<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, StoreError> {
	match value.get(key) {
		Some(Value::Null) => Ok(None),
		Some(value) => value
			.as_str()
			.map(Some)
			.ok_or_else(|| StoreError::Incompatible(format!("{key} is not optional text"))),
		None => Err(StoreError::Incompatible(format!("{key} is absent"))),
	}
}

fn positive_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.filter(|value| *value > 0)
		.ok_or_else(|| StoreError::Incompatible(format!("{key} is not positive")))
}

fn nonnegative_i64(value: &Value, key: &str) -> Result<i64, StoreError> {
	value
		.get(key)
		.and_then(Value::as_i64)
		.filter(|value| *value >= 0)
		.ok_or_else(|| StoreError::Incompatible(format!("{key} is negative or absent")))
}

fn positive_usize(value: &Value, key: &str) -> Result<usize, StoreError> {
	usize::try_from(positive_i64(value, key)?)
		.map_err(|_| StoreError::Incompatible(format!("{key} does not fit usize")))
}

fn bounded_u8(value: &Value, key: &str) -> Result<u8, StoreError> {
	u8::try_from(nonnegative_i64(value, key)?)
		.map_err(|_| StoreError::Incompatible(format!("{key} does not fit u8")))
}

fn incompatible<T>(message: &str) -> Result<T, StoreError> {
	Err(StoreError::Incompatible(message.into()))
}
