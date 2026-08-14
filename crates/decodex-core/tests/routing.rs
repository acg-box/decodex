//! Contract tests for the pure routing kernels.

use getrandom as _;
#[cfg(unix)] use libc as _;
use regex as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use tempfile as _;
use toml as _;

use decodex_core::{
	AccountId, AccountQuotaObservationError, AccountRegistryQuotaFact,
	AccountRegistryQuotaObservation, AccountRegistryRoutingDecision,
	AccountRegistryRoutingDecisionKind, AccountRegistryRoutingExclusion,
	AccountRegistryRoutingKernelError, AccountRegistryRoutingMember,
	AccountRegistryRoutingSnapshot, AccountSelectionMode, CodexCapability, ObservationConfidence,
	QuotaWindowClass, RoutingAuthorityShape, RoutingBlocker, RoutingDecision,
	RoutingDecisionCandidate, RoutingDecisionCause, RoutingDecisionExclusion, RoutingDecisionKind,
	RoutingDecisionQuotaFact, RoutingDecisionSnapshot, RoutingKernelError,
	RoutingMemberDisposition, RoutingNoRouteReason, RoutingSnapshotCapabilityFact,
	RoutingTimestampPrecision, RoutingTimestampProvenance, decide_account_registry_routing,
	decide_routing,
};

const SNAPSHOT_ID: &str = "routing-snapshot-acceptance";
const SOURCE_ID: &str = "codex-account-readback";
const DECIDED_AT: i64 = 1_000_000_000;
const OBSERVED_AT: i64 = DECIDED_AT - 100;
const OBSERVED_AT_RAW: &str = "999999900";
const MAX_TIMESTAMP_MICROS: i64 = 253_402_300_799_999_999;

fn account(number: u8) -> AccountId {
	AccountId::new(format!("10000000-0000-4000-8000-{number:012}"))
		.expect("routing account fixture is a canonical UUID")
}

fn candidate(
	position: usize,
	account_id: AccountId,
	sticky: bool,
	blockers: Vec<RoutingBlocker>,
) -> RoutingDecisionCandidate {
	RoutingDecisionCandidate {
		position,
		account_id,
		disposition: RoutingMemberDisposition::Included,
		sticky,
		blockers,
	}
}

fn quota_fact(
	account_id: AccountId,
	window: QuotaWindowClass,
	remaining_percent: u8,
	resets_at_micros: i64,
) -> RoutingDecisionQuotaFact {
	let duration_minutes = window.duration_minutes() as u16;
	let observation_revision = match window {
		QuotaWindowClass::FiveHour => 11,
		QuotaWindowClass::SevenDay => 12,
	};
	RoutingDecisionQuotaFact {
		account_id,
		window,
		duration_minutes,
		observation_revision: Some(observation_revision),
		remaining_percent: Some(remaining_percent),
		resets_at_micros: Some(resets_at_micros),
		observed_at_micros: Some(OBSERVED_AT),
		confidence: Some(ObservationConfidence::High),
		observed_at_provenance: Some(provenance(OBSERVED_AT, observation_revision)),
		resets_at_provenance: Some(provenance(resets_at_micros, observation_revision)),
	}
}

fn provenance(value: i64, evidence_revision: i64) -> RoutingTimestampProvenance {
	RoutingTimestampProvenance {
		raw_value: value.to_string(),
		source_id: SOURCE_ID.to_owned(),
		precision: RoutingTimestampPrecision::UnixMicrosecond,
		evidence_revision,
	}
}

fn expected_exclusion(
	account_id: AccountId,
	member_position: usize,
	window: QuotaWindowClass,
	duration_minutes: u16,
	observation_revision: i64,
	resets_at_micros: i64,
	reset_raw_value: &str,
) -> RoutingDecisionExclusion {
	RoutingDecisionExclusion {
		account_id,
		member_position,
		window,
		duration_minutes,
		observation_revision,
		remaining_percent: 0,
		observed_at_micros: OBSERVED_AT,
		resets_at_micros,
		confidence: ObservationConfidence::High,
		observed_at_provenance: RoutingTimestampProvenance {
			raw_value: OBSERVED_AT_RAW.to_owned(),
			source_id: SOURCE_ID.to_owned(),
			precision: RoutingTimestampPrecision::UnixMicrosecond,
			evidence_revision: observation_revision,
		},
		resets_at_provenance: RoutingTimestampProvenance {
			raw_value: reset_raw_value.to_owned(),
			source_id: SOURCE_ID.to_owned(),
			precision: RoutingTimestampPrecision::UnixMicrosecond,
			evidence_revision: observation_revision,
		},
	}
}

fn expected_selected(
	selected_account_id: AccountId,
	exclusions: Vec<RoutingDecisionExclusion>,
) -> RoutingDecision {
	RoutingDecision {
		snapshot_id: SNAPSHOT_ID.to_owned(),
		kind: RoutingDecisionKind::Selected,
		selected_account_id: Some(selected_account_id),
		ready_at_micros: None,
		no_route_reason: None,
		exclusions,
		causes: Vec::new(),
	}
}

fn expected_no_route(account_id: &AccountId, blockers: &[RoutingBlocker]) -> RoutingDecision {
	RoutingDecision {
		snapshot_id: SNAPSHOT_ID.to_owned(),
		kind: RoutingDecisionKind::NoRoute,
		selected_account_id: None,
		ready_at_micros: None,
		no_route_reason: Some(RoutingNoRouteReason::BlockedEvidence),
		exclusions: Vec::new(),
		causes: blockers
			.iter()
			.copied()
			.map(|blocker| RoutingDecisionCause { account_id: account_id.clone(), blocker })
			.collect(),
	}
}

fn quota_pair(
	account_id: &AccountId,
	five_hour_remaining: u8,
	five_hour_reset: i64,
	seven_day_remaining: u8,
	seven_day_reset: i64,
) -> Vec<RoutingDecisionQuotaFact> {
	vec![
		quota_fact(
			account_id.clone(),
			QuotaWindowClass::FiveHour,
			five_hour_remaining,
			five_hour_reset,
		),
		quota_fact(
			account_id.clone(),
			QuotaWindowClass::SevenDay,
			seven_day_remaining,
			seven_day_reset,
		),
	]
}

fn snapshot(members: Vec<RoutingDecisionCandidate>) -> RoutingDecisionSnapshot {
	let mut quota_facts = Vec::new();
	let mut capability_facts = Vec::new();
	for member in &members {
		quota_facts.extend(quota_pair(
			&member.account_id,
			50,
			DECIDED_AT + 300,
			50,
			DECIDED_AT + 10_080,
		));
		capability_facts.extend(CodexCapability::ALL.map(|capability| {
			RoutingSnapshotCapabilityFact {
				account_id: member.account_id.clone(),
				capability,
				applicable: false,
				evidence_state: None,
			}
		}));
	}
	RoutingDecisionSnapshot {
		snapshot_id: SNAPSHOT_ID.to_owned(),
		decided_at_micros: DECIDED_AT,
		members,
		quota_facts,
		capability_facts,
	}
}

#[test]
fn quota_windows_are_duration_owned_complete_and_canonical() {
	let base = snapshot(vec![candidate(1, account(1), false, vec![])]);
	assert_eq!(decide_routing(&base), Ok(expected_selected(account(1), Vec::new())));

	let mut cases = Vec::new();
	let mut missing = base.clone();
	missing.quota_facts.pop();
	cases.push(("missing", missing));
	let mut duplicated = base.clone();
	duplicated.quota_facts.push(duplicated.quota_facts[0].clone());
	cases.push(("duplicated", duplicated));
	let mut reordered = base.clone();
	reordered.quota_facts.swap(0, 1);
	cases.push(("reordered", reordered));
	let mut wrong_duration = base.clone();
	wrong_duration.quota_facts[0].duration_minutes = 301;
	cases.push(("wrong duration", wrong_duration));
	let mut foreign_member = base;
	foreign_member.quota_facts[0].account_id = account(99);
	cases.push(("foreign member", foreign_member));

	for (case, input) in cases {
		assert_eq!(decide_routing(&input), Err(RoutingKernelError::MalformedSnapshot), "{case}");
	}
}

#[test]
fn sticky_affinity_requires_both_windows_and_depleted_sticky_yields_with_exact_exclusions() {
	let preferred = account(2);
	let ordinary = account(1);
	let sticky_healthy = snapshot(vec![
		candidate(1, ordinary.clone(), false, vec![]),
		candidate(2, preferred.clone(), true, vec![]),
	]);
	assert_eq!(decide_routing(&sticky_healthy), Ok(expected_selected(preferred, Vec::new())));

	let sticky = account(3);
	let fallback = account(4);
	let mut depleted = snapshot(vec![
		candidate(1, sticky.clone(), true, vec![RoutingBlocker::QuotaFiveHourDepleted]),
		candidate(2, fallback.clone(), false, vec![]),
	]);
	depleted
		.quota_facts
		.splice(0..2, quota_pair(&sticky, 0, DECIDED_AT + 500, 50, DECIDED_AT + 50_000));
	assert_eq!(
		decide_routing(&depleted),
		Ok(expected_selected(
			fallback,
			vec![expected_exclusion(
				sticky,
				1,
				QuotaWindowClass::FiveHour,
				300,
				11,
				DECIDED_AT + 500,
				"1000000500",
			)],
		))
	);
}

#[test]
fn waiting_usage_uses_minimum_account_maximum_and_retains_each_window() {
	let first = account(5);
	let second = account(6);
	let depletion_blockers =
		vec![RoutingBlocker::QuotaFiveHourDepleted, RoutingBlocker::QuotaSevenDayDepleted];
	let mut input = snapshot(vec![
		candidate(1, first.clone(), false, depletion_blockers.clone()),
		candidate(2, second.clone(), false, depletion_blockers),
	]);
	input.quota_facts = [
		quota_pair(&first, 0, DECIDED_AT + 500, 0, DECIDED_AT + 2_000),
		quota_pair(&second, 0, DECIDED_AT + 1_700, 0, DECIDED_AT + 1_800),
	]
	.concat();

	let expected = RoutingDecision {
		snapshot_id: SNAPSHOT_ID.to_owned(),
		kind: RoutingDecisionKind::WaitingUsage,
		selected_account_id: None,
		ready_at_micros: Some(DECIDED_AT + 1_800),
		no_route_reason: None,
		exclusions: vec![
			expected_exclusion(
				first.clone(),
				1,
				QuotaWindowClass::FiveHour,
				300,
				11,
				DECIDED_AT + 500,
				"1000000500",
			),
			expected_exclusion(
				first.clone(),
				1,
				QuotaWindowClass::SevenDay,
				10_080,
				12,
				DECIDED_AT + 2_000,
				"1000002000",
			),
			expected_exclusion(
				second.clone(),
				2,
				QuotaWindowClass::FiveHour,
				300,
				11,
				DECIDED_AT + 1_700,
				"1000001700",
			),
			expected_exclusion(
				second.clone(),
				2,
				QuotaWindowClass::SevenDay,
				10_080,
				12,
				DECIDED_AT + 1_800,
				"1000001800",
			),
		],
		causes: vec![
			RoutingDecisionCause {
				account_id: first.clone(),
				blocker: RoutingBlocker::QuotaFiveHourDepleted,
			},
			RoutingDecisionCause {
				account_id: first,
				blocker: RoutingBlocker::QuotaSevenDayDepleted,
			},
			RoutingDecisionCause {
				account_id: second.clone(),
				blocker: RoutingBlocker::QuotaFiveHourDepleted,
			},
			RoutingDecisionCause {
				account_id: second,
				blocker: RoutingBlocker::QuotaSevenDayDepleted,
			},
		],
	};
	assert_eq!(decide_routing(&input), Ok(expected.clone()));
	assert_eq!(decide_routing(&input), Ok(expected));
}

#[test]
fn non_authoritative_depletion_evidence_never_selects_or_waits() {
	let depleted = account(7);
	let mut exact_bounds = snapshot(vec![candidate(1, account(70), false, vec![])]);
	for fact in &mut exact_bounds.quota_facts {
		fact.observed_at_micros = Some(DECIDED_AT - 300_000_000);
		fact.observed_at_provenance =
			Some(provenance(DECIDED_AT - 300_000_000, fact.observation_revision.unwrap()));
		fact.resets_at_micros = Some(DECIDED_AT + 1);
		fact.resets_at_provenance =
			Some(provenance(DECIDED_AT + 1, fact.observation_revision.unwrap()));
	}
	assert_eq!(decide_routing(&exact_bounds), Ok(expected_selected(account(70), Vec::new())));

	let base = || {
		let mut input = snapshot(vec![candidate(
			1,
			depleted.clone(),
			false,
			vec![RoutingBlocker::QuotaFiveHourDepleted, RoutingBlocker::QuotaSevenDayDepleted],
		)]);
		input.quota_facts = quota_pair(&depleted, 0, DECIDED_AT + 500, 0, DECIDED_AT + 10_000);
		input
	};

	let mut cases = Vec::new();
	let mut unknown = base();
	unknown.quota_facts[0].confidence = Some(ObservationConfidence::Unknown);
	cases.push(("unknown", unknown));
	let mut low_confidence = base();
	low_confidence.quota_facts[0].confidence = Some(ObservationConfidence::Low);
	cases.push(("low confidence", low_confidence));
	let mut stale = base();
	stale.quota_facts[0].observed_at_micros = Some(DECIDED_AT - 300_000_001);
	stale.quota_facts[0].observed_at_provenance = Some(provenance(
		DECIDED_AT - 300_000_001,
		stale.quota_facts[0].observation_revision.unwrap(),
	));
	cases.push(("stale", stale));
	let mut future = base();
	future.quota_facts[0].observed_at_micros = Some(DECIDED_AT + 1);
	future.quota_facts[0].observed_at_provenance =
		Some(provenance(DECIDED_AT + 1, future.quota_facts[0].observation_revision.unwrap()));
	cases.push(("future observation", future));
	let mut elapsed_reset = base();
	elapsed_reset.quota_facts[0].resets_at_micros = Some(DECIDED_AT);
	elapsed_reset.quota_facts[0].resets_at_provenance =
		Some(provenance(DECIDED_AT, elapsed_reset.quota_facts[0].observation_revision.unwrap()));
	cases.push(("elapsed reset", elapsed_reset));
	let mut non_depletion = base();
	for fact in &mut non_depletion.quota_facts {
		fact.remaining_percent = Some(1);
	}
	cases.push(("non-depletion", non_depletion));

	for (case, input) in cases {
		assert_eq!(
			decide_routing(&input),
			Ok(expected_no_route(
				&depleted,
				&[RoutingBlocker::QuotaFiveHourDepleted, RoutingBlocker::QuotaSevenDayDepleted,],
			)),
			"{case}",
		);
	}
}

#[test]
fn malformed_timestamp_provenance_is_a_structural_error() {
	let depleted = account(8);
	let fallback = account(9);
	let base = || {
		let mut input = snapshot(vec![
			candidate(1, depleted.clone(), false, vec![RoutingBlocker::QuotaFiveHourDepleted]),
			candidate(2, fallback.clone(), false, vec![]),
		]);
		input
			.quota_facts
			.splice(0..2, quota_pair(&depleted, 0, DECIDED_AT + 500, 50, DECIDED_AT + 10_000));
		input
	};

	let mut cases = Vec::new();
	let mut raw_value = base();
	raw_value.quota_facts[0].observed_at_provenance.as_mut().unwrap().raw_value =
		format!("{}000", OBSERVED_AT);
	cases.push(("raw microsecond value", raw_value));
	let mut source = base();
	source.quota_facts[0].observed_at_provenance.as_mut().unwrap().source_id.clear();
	cases.push(("source identity", source));
	let mut revision = base();
	revision.quota_facts[0].observed_at_provenance.as_mut().unwrap().evidence_revision += 1;
	cases.push(("observation revision", revision));

	for (case, input) in cases {
		assert_eq!(decide_routing(&input), Err(RoutingKernelError::IncompleteEvidence), "{case}");
	}
}

fn account_registry_member(
	position: usize,
	account_id: AccountId,
	blockers: Vec<RoutingBlocker>,
) -> AccountRegistryRoutingMember {
	AccountRegistryRoutingMember {
		position,
		account_id,
		account_revision: position as i64,
		blockers,
	}
}

fn account_registry_current_fact(
	account_id: AccountId,
	window: QuotaWindowClass,
	used_percent: u8,
	observed_at_micros: i64,
	resets_at_micros: i64,
) -> AccountRegistryQuotaFact {
	AccountRegistryQuotaFact {
		account_id,
		window,
		duration_minutes: window.duration_minutes() as u16,
		observation: AccountRegistryQuotaObservation::Current {
			used_percent,
			observed_at_micros,
			resets_at_micros,
		},
	}
}

fn account_registry_quota_pair(
	account_id: &AccountId,
	five_hour_used_percent: u8,
	five_hour_reset: i64,
	seven_day_used_percent: u8,
	seven_day_reset: i64,
) -> Vec<AccountRegistryQuotaFact> {
	vec![
		account_registry_current_fact(
			account_id.clone(),
			QuotaWindowClass::FiveHour,
			five_hour_used_percent,
			OBSERVED_AT,
			five_hour_reset,
		),
		account_registry_current_fact(
			account_id.clone(),
			QuotaWindowClass::SevenDay,
			seven_day_used_percent,
			OBSERVED_AT,
			seven_day_reset,
		),
	]
}

fn account_registry_snapshot(
	mode: AccountSelectionMode,
	members: Vec<AccountRegistryRoutingMember>,
) -> AccountRegistryRoutingSnapshot {
	let quota_facts = members
		.iter()
		.flat_map(|member| {
			account_registry_quota_pair(
				&member.account_id,
				50,
				DECIDED_AT + 500,
				50,
				DECIDED_AT + 10_000,
			)
		})
		.collect();
	AccountRegistryRoutingSnapshot {
		snapshot_id: SNAPSHOT_ID.to_owned(),
		routing_revision: 1,
		mode,
		task_role_profile_revision: 1,
		resolved_at_micros: DECIDED_AT - 1,
		members,
		quota_facts,
	}
}

fn account_registry_exclusion(
	account_id: AccountId,
	member_position: usize,
	window: QuotaWindowClass,
	observed_at_micros: i64,
	resets_at_micros: i64,
) -> AccountRegistryRoutingExclusion {
	AccountRegistryRoutingExclusion {
		account_id,
		member_position,
		window,
		duration_minutes: window.duration_minutes() as u16,
		used_percent: 100,
		observed_at_micros,
		resets_at_micros,
	}
}

fn account_registry_selected(
	account_id: AccountId,
	exclusions: Vec<AccountRegistryRoutingExclusion>,
) -> AccountRegistryRoutingDecision {
	AccountRegistryRoutingDecision {
		snapshot_id: SNAPSHOT_ID.to_owned(),
		kind: AccountRegistryRoutingDecisionKind::Selected,
		selected_account_id: Some(account_id),
		exclusions,
		causes: vec![],
	}
}

fn account_registry_waiting(
	exclusions: Vec<AccountRegistryRoutingExclusion>,
) -> AccountRegistryRoutingDecision {
	AccountRegistryRoutingDecision {
		snapshot_id: SNAPSHOT_ID.to_owned(),
		kind: AccountRegistryRoutingDecisionKind::Waiting,
		selected_account_id: None,
		exclusions,
		causes: vec![],
	}
}

fn account_registry_no_route(
	exclusions: Vec<AccountRegistryRoutingExclusion>,
	causes: Vec<RoutingDecisionCause>,
) -> AccountRegistryRoutingDecision {
	AccountRegistryRoutingDecision {
		snapshot_id: SNAPSHOT_ID.to_owned(),
		kind: AccountRegistryRoutingDecisionKind::NoRoute,
		selected_account_id: None,
		exclusions,
		causes,
	}
}

fn account_registry_cause(account_id: AccountId, blocker: RoutingBlocker) -> RoutingDecisionCause {
	RoutingDecisionCause { account_id, blocker }
}

#[test]
fn account_registry_balanced_selects_later_member_with_exact_prior_exclusions() {
	let first = account(20);
	let second = account(21);
	let mut input = account_registry_snapshot(
		AccountSelectionMode::Balanced,
		vec![
			account_registry_member(1, first.clone(), vec![]),
			account_registry_member(2, second.clone(), vec![]),
		],
	);
	input.quota_facts = [
		account_registry_quota_pair(&first, 100, DECIDED_AT + 500, 100, DECIDED_AT + 10_000),
		account_registry_quota_pair(&second, 50, DECIDED_AT + 600, 50, DECIDED_AT + 20_000),
	]
	.concat();

	assert_eq!(
		decide_account_registry_routing(&input, DECIDED_AT),
		Ok(account_registry_selected(
			second,
			vec![
				account_registry_exclusion(
					first.clone(),
					1,
					QuotaWindowClass::FiveHour,
					OBSERVED_AT,
					DECIDED_AT + 500,
				),
				account_registry_exclusion(
					first,
					1,
					QuotaWindowClass::SevenDay,
					OBSERVED_AT,
					DECIDED_AT + 10_000,
				),
			],
		)),
	);
}

#[test]
fn account_registry_balanced_prefers_known_capacity_then_falls_back_to_unknown_order() {
	let unknown = account(40);
	let known = account(41);
	let mut input = account_registry_snapshot(
		AccountSelectionMode::Balanced,
		vec![
			account_registry_member(1, unknown.clone(), vec![]),
			account_registry_member(2, known.clone(), vec![]),
		],
	);
	input.quota_facts[0].observation = AccountRegistryQuotaObservation::Missing;
	input.quota_facts[1].observation = AccountRegistryQuotaObservation::Missing;

	assert_eq!(
		decide_account_registry_routing(&input, DECIDED_AT),
		Ok(AccountRegistryRoutingDecision {
			snapshot_id: SNAPSHOT_ID.to_owned(),
			kind: AccountRegistryRoutingDecisionKind::Selected,
			selected_account_id: Some(known),
			exclusions: vec![],
			causes: vec![
				account_registry_cause(unknown.clone(), RoutingBlocker::QuotaFiveHourMissing,),
				account_registry_cause(unknown, RoutingBlocker::QuotaSevenDayMissing),
			],
		}),
	);

	for fact in &mut input.quota_facts[2..] {
		fact.observation = AccountRegistryQuotaObservation::Missing;
	}
	assert_eq!(
		decide_account_registry_routing(&input, DECIDED_AT),
		Ok(account_registry_selected(account(40), vec![])),
	);
}

#[test]
fn account_registry_fixed_blocked_target_never_falls_back_to_eligible_non_target() {
	let target = account(22);
	let non_target = account(23);
	let mut input = account_registry_snapshot(
		AccountSelectionMode::Fixed(target.clone()),
		vec![
			account_registry_member(1, target.clone(), vec![RoutingBlocker::AccountDisabled]),
			account_registry_member(2, non_target, vec![]),
		],
	);

	assert_eq!(
		decide_account_registry_routing(&input, DECIDED_AT),
		Ok(account_registry_no_route(
			vec![],
			vec![account_registry_cause(target, RoutingBlocker::AccountDisabled)],
		)),
	);

	let absent = account(24);
	input.mode = AccountSelectionMode::Fixed(absent.clone());
	assert_eq!(
		decide_account_registry_routing(&input, DECIDED_AT),
		Err(AccountRegistryRoutingKernelError::FixedTargetAbsent { account_id: absent }),
	);
}

#[test]
fn account_registry_canonical_member_and_window_causes_retain_order() {
	let account_id = account(25);
	let mut input = account_registry_snapshot(
		AccountSelectionMode::Balanced,
		vec![account_registry_member(
			1,
			account_id.clone(),
			vec![RoutingBlocker::AccountFromFuture, RoutingBlocker::AccountDisabled],
		)],
	);
	input.quota_facts[0].observation = AccountRegistryQuotaObservation::Missing;
	let observation_error = AccountRegistryQuotaObservation::ObservationError {
		error: AccountQuotaObservationError::AccountMismatch,
		observed_at_micros: OBSERVED_AT,
	};
	input.quota_facts[1].observation = observation_error.clone();

	assert_eq!(
		decide_account_registry_routing(&input, DECIDED_AT),
		Ok(account_registry_no_route(
			vec![],
			vec![
				account_registry_cause(account_id.clone(), RoutingBlocker::AccountFromFuture),
				account_registry_cause(account_id.clone(), RoutingBlocker::AccountDisabled),
				account_registry_cause(account_id.clone(), RoutingBlocker::QuotaFiveHourMissing),
				account_registry_cause(account_id, RoutingBlocker::QuotaSevenDayUnknown),
			],
		)),
	);
	assert_eq!(&input.quota_facts[1].observation, &observation_error);
}

#[test]
fn account_registry_closed_observation_errors_are_unknown_capacity_not_depletion() {
	let cases = [
		(AccountQuotaObservationError::ProviderUnavailable, QuotaWindowClass::FiveHour),
		(AccountQuotaObservationError::ProtocolUnavailable, QuotaWindowClass::SevenDay),
		(AccountQuotaObservationError::AccountMismatch, QuotaWindowClass::FiveHour),
		(AccountQuotaObservationError::UnsupportedWindow, QuotaWindowClass::SevenDay),
	];

	for (index, (error, window)) in cases.into_iter().enumerate() {
		let account_id = account(34 + index as u8);
		let mut input = account_registry_snapshot(
			AccountSelectionMode::Balanced,
			vec![account_registry_member(1, account_id.clone(), vec![])],
		);
		let observation = AccountRegistryQuotaObservation::ObservationError {
			error,
			observed_at_micros: OBSERVED_AT,
		};
		let fact_index = match window {
			QuotaWindowClass::FiveHour => 0,
			QuotaWindowClass::SevenDay => 1,
		};
		input.quota_facts[fact_index].observation = observation.clone();
		assert_eq!(
			decide_account_registry_routing(&input, DECIDED_AT),
			Ok(account_registry_selected(account_id, vec![])),
			"{error:?}",
		);
		assert_eq!(&input.quota_facts[fact_index].observation, &observation);
	}
}

#[test]
fn account_registry_split_depletion_waiting_never_pools_accounts_or_windows() {
	let first = account(26);
	let second = account(27);
	let mut input = account_registry_snapshot(
		AccountSelectionMode::Balanced,
		vec![
			account_registry_member(1, first.clone(), vec![]),
			account_registry_member(2, second.clone(), vec![]),
		],
	);
	input.quota_facts = [
		account_registry_quota_pair(&first, 100, DECIDED_AT + 500, 50, DECIDED_AT + 2_000),
		account_registry_quota_pair(&second, 50, DECIDED_AT + 1_700, 100, DECIDED_AT + 1_800),
	]
	.concat();

	assert_eq!(
		decide_account_registry_routing(&input, DECIDED_AT),
		Ok(account_registry_waiting(vec![
			account_registry_exclusion(
				first.clone(),
				1,
				QuotaWindowClass::FiveHour,
				OBSERVED_AT,
				DECIDED_AT + 500,
			),
			account_registry_exclusion(
				second,
				2,
				QuotaWindowClass::SevenDay,
				OBSERVED_AT,
				DECIDED_AT + 1_800,
			),
		])),
	);
}

#[test]
fn account_registry_freshness_boundary_is_inclusive_and_future_is_typed() {
	let account_id = account(28);
	let mut exact = account_registry_snapshot(
		AccountSelectionMode::Balanced,
		vec![account_registry_member(1, account_id.clone(), vec![])],
	);
	exact.quota_facts[0] = account_registry_current_fact(
		account_id.clone(),
		QuotaWindowClass::FiveHour,
		50,
		DECIDED_AT - 300_000_000,
		DECIDED_AT + 500,
	);
	assert_eq!(
		decide_account_registry_routing(&exact, DECIDED_AT),
		Ok(account_registry_selected(account_id.clone(), vec![])),
	);

	let mut stale = exact.clone();
	stale.quota_facts[0] = account_registry_current_fact(
		account_id.clone(),
		QuotaWindowClass::FiveHour,
		50,
		DECIDED_AT - 300_000_001,
		DECIDED_AT + 500,
	);
	assert_eq!(
		decide_account_registry_routing(&stale, DECIDED_AT),
		Ok(account_registry_selected(account_id.clone(), vec![])),
	);

	let mut elapsed = exact.clone();
	elapsed.quota_facts[0] = account_registry_current_fact(
		account_id.clone(),
		QuotaWindowClass::FiveHour,
		50,
		OBSERVED_AT,
		DECIDED_AT,
	);
	assert_eq!(
		decide_account_registry_routing(&elapsed, DECIDED_AT),
		Ok(account_registry_selected(account_id.clone(), vec![])),
	);

	let mut future = exact;
	future.quota_facts[0] = account_registry_current_fact(
		account_id.clone(),
		QuotaWindowClass::FiveHour,
		50,
		DECIDED_AT + 1,
		DECIDED_AT + 500,
	);
	assert_eq!(
		decide_account_registry_routing(&future, DECIDED_AT),
		Ok(account_registry_no_route(
			vec![],
			vec![account_registry_cause(account_id, RoutingBlocker::QuotaFiveHourFromFuture,)],
		)),
	);
}

#[test]
fn account_registry_timestamp_product_bound_is_closed() {
	let account_id = account(29);
	let mut epoch = account_registry_snapshot(
		AccountSelectionMode::Balanced,
		vec![account_registry_member(1, account_id.clone(), vec![])],
	);
	epoch.resolved_at_micros = 0;
	epoch.quota_facts = vec![
		account_registry_current_fact(account_id.clone(), QuotaWindowClass::FiveHour, 50, 0, 1),
		account_registry_current_fact(account_id.clone(), QuotaWindowClass::SevenDay, 50, 0, 2),
	];
	assert_eq!(
		decide_account_registry_routing(&epoch, 0),
		Ok(account_registry_selected(account_id.clone(), vec![])),
	);

	let mut near_maximum = account_registry_snapshot(
		AccountSelectionMode::Balanced,
		vec![account_registry_member(1, account_id.clone(), vec![])],
	);
	near_maximum.resolved_at_micros = MAX_TIMESTAMP_MICROS - 1;
	near_maximum.quota_facts = vec![
		account_registry_current_fact(
			account_id.clone(),
			QuotaWindowClass::FiveHour,
			50,
			MAX_TIMESTAMP_MICROS - 100,
			MAX_TIMESTAMP_MICROS,
		),
		account_registry_current_fact(
			account_id.clone(),
			QuotaWindowClass::SevenDay,
			50,
			MAX_TIMESTAMP_MICROS - 100,
			MAX_TIMESTAMP_MICROS,
		),
	];
	assert_eq!(
		decide_account_registry_routing(&near_maximum, MAX_TIMESTAMP_MICROS - 1),
		Ok(account_registry_selected(account_id.clone(), vec![])),
	);

	let mut maximum = near_maximum;
	maximum.resolved_at_micros = MAX_TIMESTAMP_MICROS;
	for fact in &mut maximum.quota_facts {
		fact.observation = AccountRegistryQuotaObservation::ObservationError {
			error: AccountQuotaObservationError::ProviderUnavailable,
			observed_at_micros: MAX_TIMESTAMP_MICROS,
		};
	}
	assert_eq!(
		decide_account_registry_routing(&maximum, MAX_TIMESTAMP_MICROS),
		Ok(account_registry_selected(account_id.clone(), vec![])),
	);
	assert_account_registry_timestamp_product_bound_rejections(account_id, epoch, maximum);
}

fn assert_account_registry_timestamp_product_bound_rejections(
	account_id: AccountId,
	epoch: AccountRegistryRoutingSnapshot,
	maximum: AccountRegistryRoutingSnapshot,
) {
	assert_eq!(
		decide_account_registry_routing(&maximum, -1),
		Err(AccountRegistryRoutingKernelError::InvalidDecidedAtMicros { decided_at_micros: -1 }),
	);
	assert_eq!(
		decide_account_registry_routing(&maximum, MAX_TIMESTAMP_MICROS + 1),
		Err(AccountRegistryRoutingKernelError::InvalidDecidedAtMicros {
			decided_at_micros: MAX_TIMESTAMP_MICROS + 1,
		}),
	);

	let mut resolved_after_decision = account_registry_snapshot(
		AccountSelectionMode::Balanced,
		vec![account_registry_member(1, account_id.clone(), vec![])],
	);
	resolved_after_decision.resolved_at_micros = DECIDED_AT + 1;
	assert_eq!(
		decide_account_registry_routing(&resolved_after_decision, DECIDED_AT),
		Err(AccountRegistryRoutingKernelError::InvalidResolvedAtMicros {
			resolved_at_micros: DECIDED_AT + 1,
		}),
	);
	for resolved_at_micros in [-1, MAX_TIMESTAMP_MICROS + 1] {
		let mut invalid = maximum.clone();
		invalid.resolved_at_micros = resolved_at_micros;
		assert_eq!(
			decide_account_registry_routing(&invalid, MAX_TIMESTAMP_MICROS),
			Err(AccountRegistryRoutingKernelError::InvalidResolvedAtMicros { resolved_at_micros }),
		);
	}

	for observed_at_micros in [-1, MAX_TIMESTAMP_MICROS + 1] {
		let mut invalid = account_registry_snapshot(
			AccountSelectionMode::Balanced,
			vec![account_registry_member(1, account_id.clone(), vec![])],
		);
		invalid.quota_facts[0] = account_registry_current_fact(
			account_id.clone(),
			QuotaWindowClass::FiveHour,
			50,
			observed_at_micros,
			MAX_TIMESTAMP_MICROS,
		);
		assert_eq!(
			decide_account_registry_routing(&invalid, DECIDED_AT),
			Err(AccountRegistryRoutingKernelError::InvalidQuotaFactObservedAtMicros {
				account_id: account_id.clone(),
				window: QuotaWindowClass::FiveHour,
				observed_at_micros,
			}),
		);
	}

	for resets_at_micros in [-1, MAX_TIMESTAMP_MICROS + 1] {
		let mut invalid = account_registry_snapshot(
			AccountSelectionMode::Balanced,
			vec![account_registry_member(1, account_id.clone(), vec![])],
		);
		invalid.quota_facts[0] = account_registry_current_fact(
			account_id.clone(),
			QuotaWindowClass::FiveHour,
			50,
			OBSERVED_AT,
			resets_at_micros,
		);
		assert_eq!(
			decide_account_registry_routing(&invalid, DECIDED_AT),
			Err(AccountRegistryRoutingKernelError::InvalidQuotaFactResetsAtMicros {
				account_id: account_id.clone(),
				window: QuotaWindowClass::FiveHour,
				observed_at_micros: OBSERVED_AT,
				resets_at_micros,
			}),
		);
	}

	let mut nonincreasing_reset = epoch;
	nonincreasing_reset.quota_facts[0] =
		account_registry_current_fact(account_id.clone(), QuotaWindowClass::FiveHour, 50, 0, 0);
	assert_eq!(
		decide_account_registry_routing(&nonincreasing_reset, 0),
		Err(AccountRegistryRoutingKernelError::InvalidQuotaFactResetsAtMicros {
			account_id,
			window: QuotaWindowClass::FiveHour,
			observed_at_micros: 0,
			resets_at_micros: 0,
		}),
	);
}

#[test]
fn account_registry_rejects_noncanonical_duplicate_missing_extra_and_mismatched_facts() {
	let account_id = account(30);
	let base = account_registry_snapshot(
		AccountSelectionMode::Balanced,
		vec![account_registry_member(1, account_id.clone(), vec![])],
	);

	let mut reordered = base.clone();
	reordered.quota_facts.swap(0, 1);
	let mut duplicate = base.clone();
	duplicate.quota_facts.push(duplicate.quota_facts[0].clone());
	let mut missing = base.clone();
	missing.quota_facts.pop();
	let extra_account = account(31);
	let mut extra = base.clone();
	extra.quota_facts[0].account_id = extra_account.clone();
	let mut mismatched = base;
	mismatched.quota_facts[0].duration_minutes = 301;

	let cases = vec![
		(
			"noncanonical order",
			reordered,
			AccountRegistryRoutingKernelError::NonCanonicalQuotaFact {
				fact_position: 1,
				account_id: account_id.clone(),
				window: QuotaWindowClass::SevenDay,
				expected_account_id: account_id.clone(),
				expected_window: QuotaWindowClass::FiveHour,
			},
		),
		(
			"duplicate",
			duplicate,
			AccountRegistryRoutingKernelError::DuplicateQuotaFact {
				account_id: account_id.clone(),
				window: QuotaWindowClass::FiveHour,
			},
		),
		(
			"missing",
			missing,
			AccountRegistryRoutingKernelError::MissingQuotaFact {
				account_id: account_id.clone(),
				window: QuotaWindowClass::SevenDay,
			},
		),
		(
			"extra",
			extra,
			AccountRegistryRoutingKernelError::ExtraQuotaFact {
				account_id: extra_account,
				window: QuotaWindowClass::FiveHour,
			},
		),
		(
			"window-duration mismatch",
			mismatched,
			AccountRegistryRoutingKernelError::QuotaFactWindowDurationMismatch {
				account_id,
				window: QuotaWindowClass::FiveHour,
				expected_duration_minutes: 300,
				duration_minutes: 301,
			},
		),
	];

	for (case, input, expected) in cases {
		assert_eq!(decide_account_registry_routing(&input, DECIDED_AT), Err(expected), "{case}");
	}
}

#[test]
fn account_registry_rejects_invalid_revisions_and_member_inventory() {
	let account_id = account(33);
	let base = account_registry_snapshot(
		AccountSelectionMode::Balanced,
		vec![account_registry_member(1, account_id.clone(), vec![])],
	);

	let mut routing_revision = base.clone();
	routing_revision.routing_revision = 0;
	assert_eq!(
		decide_account_registry_routing(&routing_revision, DECIDED_AT),
		Err(AccountRegistryRoutingKernelError::InvalidRoutingRevision { routing_revision: 0 }),
	);

	let mut profile_revision = base.clone();
	profile_revision.task_role_profile_revision = 0;
	assert_eq!(
		decide_account_registry_routing(&profile_revision, DECIDED_AT),
		Err(AccountRegistryRoutingKernelError::InvalidTaskRoleProfileRevision {
			task_role_profile_revision: 0,
		}),
	);

	let empty = account_registry_snapshot(AccountSelectionMode::Balanced, vec![]);
	assert_eq!(
		decide_account_registry_routing(&empty, DECIDED_AT),
		Err(AccountRegistryRoutingKernelError::EmptyMembers),
	);

	let mut noncanonical = base.clone();
	noncanonical.members[0].position = 2;
	assert_eq!(
		decide_account_registry_routing(&noncanonical, DECIDED_AT),
		Err(AccountRegistryRoutingKernelError::NonCanonicalMember {
			account_id: account_id.clone(),
			member_position: 2,
			expected_member_position: 1,
		}),
	);

	let mut invalid_account_revision = base.clone();
	invalid_account_revision.members[0].account_revision = 0;
	assert_eq!(
		decide_account_registry_routing(&invalid_account_revision, DECIDED_AT),
		Err(AccountRegistryRoutingKernelError::InvalidMemberAccountRevision {
			account_id: account_id.clone(),
			account_revision: 0,
		}),
	);

	let duplicate = account_registry_snapshot(
		AccountSelectionMode::Balanced,
		vec![
			account_registry_member(1, account_id.clone(), vec![]),
			account_registry_member(2, account_id.clone(), vec![]),
		],
	);
	assert_eq!(
		decide_account_registry_routing(&duplicate, DECIDED_AT),
		Err(AccountRegistryRoutingKernelError::DuplicateMember {
			account_id,
			first_position: 1,
			duplicate_position: 2,
		}),
	);
}

#[test]
fn account_registry_rejects_forbidden_duplicate_and_reordered_member_blockers() {
	let account_id = account(38);
	let snapshot_with = |blockers| {
		account_registry_snapshot(
			AccountSelectionMode::Balanced,
			vec![account_registry_member(1, account_id.clone(), blockers)],
		)
	};
	let cases = vec![
		(
			"forbidden",
			snapshot_with(vec![RoutingBlocker::ExcludedByPolicy]),
			AccountRegistryRoutingKernelError::ForbiddenMemberBlocker {
				account_id: account_id.clone(),
				member_position: 1,
				blocker_position: 1,
				blocker: RoutingBlocker::ExcludedByPolicy,
			},
		),
		(
			"duplicate",
			snapshot_with(vec![RoutingBlocker::AccountStale, RoutingBlocker::AccountStale]),
			AccountRegistryRoutingKernelError::DuplicateMemberBlocker {
				account_id: account_id.clone(),
				member_position: 1,
				blocker: RoutingBlocker::AccountStale,
				first_blocker_position: 1,
				duplicate_blocker_position: 2,
			},
		),
		(
			"reordered",
			snapshot_with(vec![RoutingBlocker::AccountDisabled, RoutingBlocker::AccountFromFuture]),
			AccountRegistryRoutingKernelError::NonCanonicalMemberBlocker {
				account_id,
				member_position: 1,
				blocker_position: 2,
				previous_blocker: RoutingBlocker::AccountDisabled,
				blocker: RoutingBlocker::AccountFromFuture,
			},
		),
	];

	for (case, input, expected) in cases {
		assert_eq!(decide_account_registry_routing(&input, DECIDED_AT), Err(expected), "{case}");
	}
}

#[test]
fn routing_authority_shape_discriminators_and_selection_contract_are_closed() {
	let cases = [
		(RoutingAuthorityShape::ConversationAccountRegistry, "conversation_account_registry", true),
		(RoutingAuthorityShape::ManagedRunProjectPolicy, "managed_run_project_policy", true),
		(RoutingAuthorityShape::ConversationContinuation, "conversation_continuation", false),
	];

	for (shape, discriminator, is_selecting) in cases {
		assert_eq!(shape.as_sql(), discriminator);
		assert_eq!(RoutingAuthorityShape::from_sql(discriminator), Some(shape));
		assert_eq!(shape.is_selecting(), is_selecting);
	}
	assert_eq!(RoutingAuthorityShape::from_sql("unknown"), None);
}
