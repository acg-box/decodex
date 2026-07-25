//! Contract tests for the pure V16 routing kernel.

use getrandom as _;
#[cfg(unix)] use libc as _;
use regex as _;
use serde as _;
use serde_json as _;
use sha2 as _;
use tempfile as _;
use toml as _;

use decodex_core::{
	AccountId, CodexCapability, ObservationConfidence, QuotaWindowClass, RoutingBlocker,
	RoutingDecision, RoutingDecisionCandidate, RoutingDecisionCause, RoutingDecisionExclusion,
	RoutingDecisionKind, RoutingDecisionQuotaFact, RoutingDecisionSnapshot, RoutingKernelError,
	RoutingMemberDisposition, RoutingNoRouteReason, RoutingSnapshotCapabilityFact,
	RoutingTimestampPrecision, RoutingTimestampProvenance, decide_routing,
};

const SNAPSHOT_ID: &str = "routing-snapshot-acceptance";
const SOURCE_ID: &str = "codex-account-readback";
const DECIDED_AT: i64 = 1_000_000_000;
const OBSERVED_AT: i64 = DECIDED_AT - 100;
const OBSERVED_AT_RAW: &str = "999999900";

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
