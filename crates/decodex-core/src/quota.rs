//! Pure, duration-typed quota observations and fail-closed classification.

/// The only quota-window identities accepted by the vNext policy.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum QuotaWindowClass {
	/// A rolling five-hour window.
	FiveHour,
	/// A rolling seven-day window.
	SevenDay,
}
impl QuotaWindowClass {
	/// Resolve a window identity from its duration rather than a source-field position.
	pub const fn from_duration_minutes(
		duration_minutes: u32,
	) -> Result<Self, UnknownWindowDuration> {
		match duration_minutes {
			300 => Ok(Self::FiveHour),
			10_080 => Ok(Self::SevenDay),
			_ => Err(UnknownWindowDuration),
		}
	}

	/// Return the canonical duration that defines this window identity.
	pub const fn duration_minutes(self) -> u32 {
		match self {
			Self::FiveHour => 300,
			Self::SevenDay => 10_080,
		}
	}
}

/// Confidence attached to one otherwise well-formed observation.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ObservationConfidence {
	/// The source did not establish a confidence level.
	Unknown,
	/// The evidence is explicitly below the policy threshold.
	Low,
	/// The evidence meets the policy threshold.
	High,
}

/// A closed malformed-observation category supplied by a boundary adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MalformedObservation {
	/// The remaining percentage was outside zero through one hundred.
	RemainingOutOfRange,
	/// The reset instant preceded the observation instant.
	ResetBeforeObservation,
}

/// A closed unknown-observation category supplied by a boundary adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnknownObservation {
	/// The source explicitly reported no usable value.
	Unreported,
}

/// Source evidence for a window duration before closed-class resolution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowDurationObservation {
	/// A source-reported duration in minutes.
	Minutes(u32),
	/// The source omitted the duration that establishes window identity.
	Missing,
	/// The source duration could not be represented as minutes.
	Malformed,
}

/// Evidence values for one duration-tagged quota window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuotaWindowValueObservation {
	/// A complete observation requiring freshness and confidence classification.
	Observed(ObservedQuotaWindow),
	/// The source omitted the observation values for this duration.
	Missing,
	/// The source values were present but structurally malformed.
	Malformed(MalformedObservation),
	/// The source could not report usable observation values.
	Unknown(UnknownObservation),
}

/// Authentication evidence classified independently from quota windows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthenticationObservation {
	/// Authentication was freshly observed as valid.
	Valid,
	/// Authentication was rejected or absent.
	Failed,
	/// No current evidence establishes authentication readiness.
	Unknown,
}

/// Why a window is fail-closed and requires new evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeReason {
	/// A source observation omitted the duration that establishes identity.
	MissingDuration,
	/// A source duration was not representable as minutes.
	MalformedDuration,
	/// A source duration was not one of the two closed window classes.
	UnknownDuration,
	/// More than one source observation claimed the same duration identity.
	DuplicateWindow,
	/// The required duration-typed observation is missing.
	Missing,
	/// The source observation is malformed.
	Malformed(MalformedObservation),
	/// The source explicitly reported an unknown observation.
	Unknown(UnknownObservation),
	/// The observation instant is later than the explicit caller clock.
	ObservationFromFuture,
	/// The observation is older than the accepted maximum age.
	Stale,
	/// The source explicitly reported low confidence.
	LowConfidence,
	/// The source did not establish an observation confidence.
	UnknownConfidence,
	/// The observed window's reset has elapsed without a fresh replacement observation.
	ResetElapsed,
}

/// Pure classification of one duration-typed window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuotaWindowState {
	/// Fresh evidence reports a positive amount remaining.
	Available,
	/// Fresh evidence reports zero remaining and a future reset.
	Depleted {
		/// The hypothetical instant at which a new probe may establish availability.
		reset_at: ObservationInstant,
	},
	/// Current evidence cannot establish availability or fresh depletion.
	UnknownProbeRequired {
		/// The deterministic reason that new evidence is required.
		reason: ProbeReason,
	},
}

/// Deterministic account-level quota decision fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountQuotaClassification {
	/// Both required windows are fresh and report remaining capacity.
	Available {
		/// The explicit caller clock used for this hypothetical result.
		ready_at: ObservationInstant,
	},
	/// At least one fresh window is depleted and every other window is known.
	UsageDepleted {
		/// The maximum future reset among this account's depleted windows.
		ready_at: ObservationInstant,
	},
	/// Unknown quota or authentication evidence requires a probe.
	UnknownProbeRequired,
	/// Authentication evidence excludes the account independently of usage.
	AuthenticationExcluded,
}

/// Synthetic result over a closed set of already classified account facts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AllAccountsQuotaFacts<AccountId> {
	/// Every account is excluded only by fresh usage evidence.
	AllUsageDepleted {
		/// Separate per-account readiness facts in deterministic account-id order.
		accounts: Vec<AccountReadyAt<AccountId>>,
		/// Minimum readiness instant across the usage-only excluded accounts.
		earliest_ready_at: ObservationInstant,
	},
	/// At least one account is available, unknown, or authentication-excluded.
	NotAllUsageDepleted,
	/// The synthetic input repeated an account identity.
	DuplicateAccount,
}

/// One source observation whose identity is carried only by its duration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuotaWindowObservation {
	duration: WindowDurationObservation,
	value: QuotaWindowValueObservation,
}
impl QuotaWindowObservation {
	/// Assemble a duration-tagged source observation without using a label or position.
	pub const fn new(
		duration: WindowDurationObservation,
		value: QuotaWindowValueObservation,
	) -> Self {
		Self { duration, value }
	}

	/// Assemble a source observation from its reported duration in minutes.
	pub const fn from_duration_minutes(
		duration_minutes: u32,
		value: QuotaWindowValueObservation,
	) -> Self {
		Self::new(WindowDurationObservation::Minutes(duration_minutes), value)
	}

	/// Return the source duration evidence.
	pub const fn duration(self) -> WindowDurationObservation {
		self.duration
	}

	/// Return the source value evidence.
	pub const fn value(self) -> QuotaWindowValueObservation {
		self.value
	}
}

/// A duration that is not one of the two closed quota-window classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnknownWindowDuration;

/// A nonnegative instant supplied by the caller's explicit clock.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ObservationInstant(u64);
impl ObservationInstant {
	/// Construct an instant from a caller-owned clock value in seconds.
	pub const fn from_seconds(seconds: u64) -> Self {
		Self(seconds)
	}

	/// Return the underlying clock value in seconds.
	pub const fn seconds(self) -> u64 {
		self.0
	}

	/// Add a duration without wrapping.
	pub const fn checked_add(self, duration: ObservationDuration) -> Result<Self, TimeOverflow> {
		match self.0.checked_add(duration.0) {
			Some(seconds) => Ok(Self(seconds)),
			None => Err(TimeOverflow),
		}
	}
}

/// A nonnegative duration in seconds.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ObservationDuration(u64);
impl ObservationDuration {
	/// Construct a duration in seconds.
	pub const fn from_seconds(seconds: u64) -> Self {
		Self(seconds)
	}

	/// Return the duration in seconds.
	pub const fn seconds(self) -> u64 {
		self.0
	}
}

/// Checked instant arithmetic could not represent the result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimeOverflow;

/// A validated quota amount remaining in one window.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct RemainingPercent(u8);
impl RemainingPercent {
	/// Validate a percentage in the inclusive range zero through one hundred.
	pub const fn new(value: u16) -> Result<Self, MalformedObservation> {
		if value <= 100 {
			Ok(Self(value as u8))
		} else {
			Err(MalformedObservation::RemainingOutOfRange)
		}
	}

	/// Return the validated percentage.
	pub const fn get(self) -> u8 {
		self.0
	}

	/// Return whether fresh evidence reports this window as depleted.
	pub const fn is_depleted(self) -> bool {
		self.0 == 0
	}
}

/// One complete, structurally valid observation of a duration-typed window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObservedQuotaWindow {
	remaining: RemainingPercent,
	reset_at: ObservationInstant,
	observed_at: ObservationInstant,
	confidence: ObservationConfidence,
}
impl ObservedQuotaWindow {
	/// Assemble a complete observation without consulting a clock or external state.
	pub const fn new(
		remaining: RemainingPercent,
		reset_at: ObservationInstant,
		observed_at: ObservationInstant,
		confidence: ObservationConfidence,
	) -> Self {
		Self { remaining, reset_at, observed_at, confidence }
	}

	/// Return the observed amount remaining.
	pub const fn remaining(self) -> RemainingPercent {
		self.remaining
	}

	/// Return the observed reset instant.
	pub const fn reset_at(self) -> ObservationInstant {
		self.reset_at
	}

	/// Return the instant at which this evidence was observed.
	pub const fn observed_at(self) -> ObservationInstant {
		self.observed_at
	}

	/// Return the evidence confidence.
	pub const fn confidence(self) -> ObservationConfidence {
		self.confidence
	}
}

/// One account's independent authentication and quota observations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountQuotaObservation {
	authentication: AuthenticationObservation,
	windows: Vec<QuotaWindowObservation>,
}
impl AccountQuotaObservation {
	/// Assemble one account observation without merging or positionally identifying windows.
	pub const fn new(
		authentication: AuthenticationObservation,
		windows: Vec<QuotaWindowObservation>,
	) -> Self {
		Self { authentication, windows }
	}
}

/// Caller-selected freshness policy for pure classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuotaClassificationPolicy {
	maximum_age: ObservationDuration,
}
impl QuotaClassificationPolicy {
	/// Construct a policy from an explicit maximum evidence age.
	pub const fn new(maximum_age: ObservationDuration) -> Self {
		Self { maximum_age }
	}
}

/// Classification facts for one window, retaining its duration identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuotaWindowFact {
	class: QuotaWindowClass,
	state: QuotaWindowState,
}
impl QuotaWindowFact {
	/// Return the duration-owned identity.
	pub const fn class(self) -> QuotaWindowClass {
		self.class
	}

	/// Return the fail-closed window state.
	pub const fn state(self) -> QuotaWindowState {
		self.state
	}
}

/// Separate window and account facts produced by pure classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AccountQuotaFacts {
	five_hour: QuotaWindowFact,
	seven_day: QuotaWindowFact,
	classification: AccountQuotaClassification,
}
impl AccountQuotaFacts {
	/// Return the five-hour fact.
	pub const fn five_hour(self) -> QuotaWindowFact {
		self.five_hour
	}

	/// Return the seven-day fact.
	pub const fn seven_day(self) -> QuotaWindowFact {
		self.seven_day
	}

	/// Return the account-level classification.
	pub const fn classification(self) -> AccountQuotaClassification {
		self.classification
	}
}

/// One account's hypothetical usage-only readiness, retained as a separate fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountReadyAt<AccountId> {
	/// Caller-owned account identity.
	pub account_id: AccountId,
	/// Maximum reset among this account's fresh depleted windows.
	pub ready_at: ObservationInstant,
}

/// Classify one account from an explicit clock without I/O or mutation.
pub fn classify_account_quota(
	observation: AccountQuotaObservation,
	policy: QuotaClassificationPolicy,
	now: ObservationInstant,
) -> AccountQuotaFacts {
	let (five_hour_observation, seven_day_observation, mapping_issue) =
		resolve_window_observations(&observation.windows);
	let (five_hour, seven_day) = match mapping_issue {
		Some(reason) => (
			unknown_window_fact(QuotaWindowClass::FiveHour, reason),
			unknown_window_fact(QuotaWindowClass::SevenDay, reason),
		),
		None => (
			classify_window(
				QuotaWindowClass::FiveHour,
				five_hour_observation.unwrap_or(QuotaWindowValueObservation::Missing),
				policy,
				now,
			),
			classify_window(
				QuotaWindowClass::SevenDay,
				seven_day_observation.unwrap_or(QuotaWindowValueObservation::Missing),
				policy,
				now,
			),
		),
	};
	let classification = classify_account(observation.authentication, five_hour, seven_day, now);

	AccountQuotaFacts { five_hour, seven_day, classification }
}

/// Aggregate only usage-excluded account facts without selecting or mutating an account.
pub fn classify_all_accounts<AccountId>(
	accounts: &[(AccountId, AccountQuotaFacts)],
) -> AllAccountsQuotaFacts<AccountId>
where
	AccountId: Clone + Ord,
{
	if accounts.is_empty() {
		return AllAccountsQuotaFacts::NotAllUsageDepleted;
	}

	let mut account_ids = accounts.iter().map(|(account_id, _)| account_id).collect::<Vec<_>>();

	account_ids.sort();

	if account_ids.windows(2).any(|pair| pair[0] == pair[1]) {
		return AllAccountsQuotaFacts::DuplicateAccount;
	}

	let mut ready = Vec::with_capacity(accounts.len());

	for (account_id, facts) in accounts {
		let AccountQuotaClassification::UsageDepleted { ready_at } = facts.classification else {
			return AllAccountsQuotaFacts::NotAllUsageDepleted;
		};

		ready.push(AccountReadyAt { account_id: account_id.clone(), ready_at });
	}

	ready.sort_by(|left, right| left.account_id.cmp(&right.account_id));

	let Some(earliest_ready_at) = ready.iter().map(|fact| fact.ready_at).min() else {
		return AllAccountsQuotaFacts::NotAllUsageDepleted;
	};

	AllAccountsQuotaFacts::AllUsageDepleted { accounts: ready, earliest_ready_at }
}

fn resolve_window_observations(
	windows: &[QuotaWindowObservation],
) -> (Option<QuotaWindowValueObservation>, Option<QuotaWindowValueObservation>, Option<ProbeReason>)
{
	let mut five_hour = None;
	let mut seven_day = None;
	let mut missing_duration = false;
	let mut malformed_duration = false;
	let mut unknown_duration = false;
	let mut duplicate_window = false;

	for observation in windows {
		let class = match observation.duration {
			WindowDurationObservation::Missing => {
				missing_duration = true;

				continue;
			},
			WindowDurationObservation::Malformed => {
				malformed_duration = true;

				continue;
			},
			WindowDurationObservation::Minutes(duration_minutes) => {
				let Ok(class) = QuotaWindowClass::from_duration_minutes(duration_minutes) else {
					unknown_duration = true;

					continue;
				};

				class
			},
		};
		let target = match class {
			QuotaWindowClass::FiveHour => &mut five_hour,
			QuotaWindowClass::SevenDay => &mut seven_day,
		};

		if target.replace(observation.value).is_some() {
			duplicate_window = true;
		}
	}

	let issue = if malformed_duration {
		Some(ProbeReason::MalformedDuration)
	} else if missing_duration {
		Some(ProbeReason::MissingDuration)
	} else if unknown_duration {
		Some(ProbeReason::UnknownDuration)
	} else if duplicate_window {
		Some(ProbeReason::DuplicateWindow)
	} else {
		None
	};

	(five_hour, seven_day, issue)
}

fn unknown_window_fact(class: QuotaWindowClass, reason: ProbeReason) -> QuotaWindowFact {
	QuotaWindowFact { class, state: QuotaWindowState::UnknownProbeRequired { reason } }
}

fn classify_window(
	class: QuotaWindowClass,
	observation: QuotaWindowValueObservation,
	policy: QuotaClassificationPolicy,
	now: ObservationInstant,
) -> QuotaWindowFact {
	let state = match observation {
		QuotaWindowValueObservation::Missing =>
			QuotaWindowState::UnknownProbeRequired { reason: ProbeReason::Missing },
		QuotaWindowValueObservation::Malformed(reason) =>
			QuotaWindowState::UnknownProbeRequired { reason: ProbeReason::Malformed(reason) },
		QuotaWindowValueObservation::Unknown(reason) =>
			QuotaWindowState::UnknownProbeRequired { reason: ProbeReason::Unknown(reason) },
		QuotaWindowValueObservation::Observed(observed) =>
			classify_observed_window(observed, policy, now),
	};

	QuotaWindowFact { class, state }
}

fn classify_observed_window(
	observed: ObservedQuotaWindow,
	policy: QuotaClassificationPolicy,
	now: ObservationInstant,
) -> QuotaWindowState {
	let Some(age) = now.0.checked_sub(observed.observed_at.0) else {
		return QuotaWindowState::UnknownProbeRequired {
			reason: ProbeReason::ObservationFromFuture,
		};
	};

	if observed.reset_at < observed.observed_at {
		return QuotaWindowState::UnknownProbeRequired {
			reason: ProbeReason::Malformed(MalformedObservation::ResetBeforeObservation),
		};
	}
	if age > policy.maximum_age.0 {
		return QuotaWindowState::UnknownProbeRequired { reason: ProbeReason::Stale };
	}

	match observed.confidence {
		ObservationConfidence::Unknown => {
			return QuotaWindowState::UnknownProbeRequired {
				reason: ProbeReason::UnknownConfidence,
			};
		},
		ObservationConfidence::Low => {
			return QuotaWindowState::UnknownProbeRequired { reason: ProbeReason::LowConfidence };
		},
		ObservationConfidence::High => {},
	}

	if observed.reset_at <= now {
		return QuotaWindowState::UnknownProbeRequired { reason: ProbeReason::ResetElapsed };
	}
	if !observed.remaining.is_depleted() {
		return QuotaWindowState::Available;
	}

	QuotaWindowState::Depleted { reset_at: observed.reset_at }
}

fn classify_account(
	authentication: AuthenticationObservation,
	five_hour: QuotaWindowFact,
	seven_day: QuotaWindowFact,
	now: ObservationInstant,
) -> AccountQuotaClassification {
	match authentication {
		AuthenticationObservation::Failed => {
			return AccountQuotaClassification::AuthenticationExcluded;
		},
		AuthenticationObservation::Unknown => {
			return AccountQuotaClassification::UnknownProbeRequired;
		},
		AuthenticationObservation::Valid => {},
	}

	let states = [five_hour.state, seven_day.state];

	if states.iter().any(|state| matches!(state, QuotaWindowState::UnknownProbeRequired { .. })) {
		return AccountQuotaClassification::UnknownProbeRequired;
	}

	let ready_at = states.iter().filter_map(|state| match state {
		QuotaWindowState::Depleted { reset_at } => Some(*reset_at),
		QuotaWindowState::Available | QuotaWindowState::UnknownProbeRequired { .. } => None,
	});

	match ready_at.max() {
		Some(ready_at) => AccountQuotaClassification::UsageDepleted { ready_at },
		None => AccountQuotaClassification::Available { ready_at: now },
	}
}

#[cfg(test)]
mod tests {
	use std::collections::BTreeMap;

	use serde::Deserialize;

	use crate::quota::{
		self, AccountQuotaClassification, AccountQuotaFacts, AccountQuotaObservation,
		AccountReadyAt, AllAccountsQuotaFacts, AuthenticationObservation, MalformedObservation,
		ObservationConfidence, ObservationDuration, ObservationInstant, ObservedQuotaWindow,
		ProbeReason, QuotaClassificationPolicy, QuotaWindowClass, QuotaWindowObservation,
		QuotaWindowState, QuotaWindowValueObservation, RemainingPercent, TimeOverflow,
		UnknownObservation, UnknownWindowDuration, WindowDurationObservation,
	};

	const ACCEPTED_FIXTURE: &str =
		include_str!("../../../openwiki/evidence/fixtures/xy-1262-quota-matrix.json");

	#[derive(Deserialize)]
	#[serde(deny_unknown_fields)]
	struct Fixture {
		schema: String,
		window_identity: FixtureWindowIdentity,
		rules: FixtureRules,
		cases: Vec<FixtureCase>,
	}

	#[derive(Clone, Copy, Deserialize)]
	#[serde(deny_unknown_fields)]
	struct FixtureWindowIdentity {
		five_hour: FixtureDuration,
		seven_day: FixtureDuration,
	}

	#[derive(Clone, Copy, Deserialize)]
	#[serde(deny_unknown_fields)]
	struct FixtureDuration {
		duration_minutes: u32,
	}

	#[derive(Deserialize)]
	#[serde(deny_unknown_fields)]
	struct FixtureRules {
		positional_fields_are_identity: bool,
		account_ready_at: String,
		all_accounts_ready_at: String,
		elapsed_reset_transition: String,
		stale_transition: String,
	}

	#[derive(Deserialize)]
	#[serde(deny_unknown_fields)]
	struct FixtureCase {
		id: String,
		input: FixtureInput,
		expect: FixtureExpectation,
	}

	#[derive(Deserialize)]
	#[serde(deny_unknown_fields)]
	struct FixtureInput {
		auth: Option<FixtureAuthentication>,
		observed_at: Option<u64>,
		now: Option<u64>,
		stale_after_seconds: Option<u64>,
		#[serde(default)]
		windows: Vec<FixtureWindow>,
		#[serde(default)]
		accounts: Vec<FixtureAccount>,
	}

	#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
	#[serde(rename_all = "snake_case")]
	enum FixtureSourceField {
		Primary,
		Secondary,
	}

	#[derive(Clone, Copy, Deserialize)]
	#[serde(rename_all = "snake_case")]
	enum FixtureConfidence {
		High,
		Low,
		Unknown,
	}

	#[derive(Deserialize)]
	#[serde(deny_unknown_fields)]
	struct FixtureWindow {
		source_field: FixtureSourceField,
		duration_minutes: u32,
		remaining_percent: u16,
		reset_at: u64,
		confidence: Option<FixtureConfidence>,
	}

	#[derive(Deserialize)]
	#[serde(deny_unknown_fields)]
	struct FixtureAccount {
		id: String,
		depleted_resets_at: Vec<u64>,
	}

	#[derive(Clone, Copy, Deserialize)]
	#[serde(rename_all = "snake_case")]
	enum FixtureAuthentication {
		Valid,
		Failed,
	}

	#[derive(Clone, Copy, Deserialize)]
	#[serde(rename_all = "snake_case")]
	enum FixtureState {
		Available,
		Depleted,
		Unknown,
		AuthFailed,
	}

	#[derive(Clone, Copy, Deserialize)]
	#[serde(rename_all = "snake_case")]
	enum FixtureDecision {
		Usable,
		ExcludedUsage,
		ProbeRequired,
		ExcludedAuth,
		WaitingUsage,
	}

	#[derive(Deserialize)]
	#[serde(deny_unknown_fields)]
	struct FixtureClassifiedSources {
		five_hour_source: FixtureSourceField,
		seven_day_source: FixtureSourceField,
	}

	#[derive(Deserialize)]
	#[serde(deny_unknown_fields)]
	struct FixtureExpectation {
		state: FixtureState,
		decision: FixtureDecision,
		ready_at: Option<u64>,
		classified: Option<FixtureClassifiedSources>,
		#[serde(default)]
		account_ready_at: BTreeMap<String, u64>,
		earliest_ready_at: Option<u64>,
	}

	fn instant(seconds: u64) -> ObservationInstant {
		ObservationInstant::from_seconds(seconds)
	}

	fn remaining(percent: u16) -> RemainingPercent {
		let Ok(remaining) = RemainingPercent::new(percent) else {
			panic!("test percentage is valid");
		};

		remaining
	}

	fn observed(
		remaining_percent: u16,
		reset_at: u64,
		observed_at: u64,
		confidence: ObservationConfidence,
	) -> QuotaWindowValueObservation {
		QuotaWindowValueObservation::Observed(ObservedQuotaWindow::new(
			remaining(remaining_percent),
			instant(reset_at),
			instant(observed_at),
			confidence,
		))
	}

	fn windows(
		five_hour: QuotaWindowValueObservation,
		seven_day: QuotaWindowValueObservation,
	) -> Vec<QuotaWindowObservation> {
		vec![
			QuotaWindowObservation::from_duration_minutes(300, five_hour),
			QuotaWindowObservation::from_duration_minutes(10_080, seven_day),
		]
	}

	fn translate_fixture_windows(
		windows: &[FixtureWindow],
		observed_at: u64,
	) -> Vec<QuotaWindowObservation> {
		windows
			.iter()
			.map(|window| {
				QuotaWindowObservation::from_duration_minutes(
					window.duration_minutes,
					observed(
						window.remaining_percent,
						window.reset_at,
						observed_at,
						match window.confidence.unwrap_or(FixtureConfidence::High) {
							FixtureConfidence::High => ObservationConfidence::High,
							FixtureConfidence::Low => ObservationConfidence::Low,
							FixtureConfidence::Unknown => ObservationConfidence::Unknown,
						},
					),
				)
			})
			.collect()
	}

	fn fixture() -> Fixture {
		let Ok(fixture) = serde_json::from_str(ACCEPTED_FIXTURE) else {
			panic!("accepted quota matrix must match its typed test schema");
		};

		fixture
	}

	fn required<T>(value: Option<T>, field: &str, case_id: &str) -> T
	where
		T: Copy,
	{
		let Some(value) = value else {
			panic!("fixture case {case_id} requires {field}");
		};

		value
	}

	fn policy(maximum_age: u64) -> QuotaClassificationPolicy {
		QuotaClassificationPolicy::new(ObservationDuration::from_seconds(maximum_age))
	}

	fn assert_fixture_rules(fixture: &Fixture) {
		assert_eq!(fixture.schema, "decodex/vnext-quota-matrix/1");
		assert!(!fixture.rules.positional_fields_are_identity);
		assert_eq!(
			fixture.rules.account_ready_at,
			"maximum reset_at among that account's fresh depleted windows"
		);
		assert_eq!(
			fixture.rules.all_accounts_ready_at,
			"minimum account_ready_at among accounts excluded only by usage"
		);
		assert_eq!(
			fixture.rules.elapsed_reset_transition,
			"unknown until a fresh observation proves availability"
		);
		assert_eq!(
			fixture.rules.stale_transition,
			"unknown and bounded probe; never depleted or available by inference"
		);
		assert_eq!(
			QuotaWindowClass::from_duration_minutes(
				fixture.window_identity.five_hour.duration_minutes
			),
			Ok(QuotaWindowClass::FiveHour)
		);
		assert_eq!(
			QuotaWindowClass::from_duration_minutes(
				fixture.window_identity.seven_day.duration_minutes
			),
			Ok(QuotaWindowClass::SevenDay)
		);
	}

	fn expected_account_classification(
		case_id: &str,
		expectation: &FixtureExpectation,
	) -> AccountQuotaClassification {
		match (expectation.state, expectation.decision, expectation.ready_at) {
			(FixtureState::Available, FixtureDecision::Usable, Some(ready_at)) =>
				AccountQuotaClassification::Available { ready_at: instant(ready_at) },
			(FixtureState::Depleted, FixtureDecision::ExcludedUsage, Some(ready_at)) =>
				AccountQuotaClassification::UsageDepleted { ready_at: instant(ready_at) },
			(FixtureState::Unknown, FixtureDecision::ProbeRequired, None) =>
				AccountQuotaClassification::UnknownProbeRequired,
			(FixtureState::AuthFailed, FixtureDecision::ExcludedAuth, None) =>
				AccountQuotaClassification::AuthenticationExcluded,
			_ => panic!("fixture case {case_id} has inconsistent account expectations"),
		}
	}

	fn classify_fixture_account_case(case: &FixtureCase) -> AccountQuotaFacts {
		let observed_at = required(case.input.observed_at, "observed_at", &case.id);
		let now = required(case.input.now, "now", &case.id);
		let authentication = match required(case.input.auth, "auth", &case.id) {
			FixtureAuthentication::Valid => AuthenticationObservation::Valid,
			FixtureAuthentication::Failed => AuthenticationObservation::Failed,
		};
		let maximum_age =
			case.input.stale_after_seconds.unwrap_or_else(|| now.saturating_sub(observed_at));

		quota::classify_account_quota(
			AccountQuotaObservation::new(
				authentication,
				translate_fixture_windows(&case.input.windows, observed_at),
			),
			policy(maximum_age),
			instant(now),
		)
	}

	fn assert_fixture_account_case(case: &FixtureCase, identity: FixtureWindowIdentity) {
		assert!(case.input.accounts.is_empty(), "{} mixes account input shapes", case.id);
		assert!(case.expect.account_ready_at.is_empty());
		assert_eq!(case.expect.earliest_ready_at, None);

		let facts = classify_fixture_account_case(case);

		assert_eq!(
			facts.classification(),
			expected_account_classification(&case.id, &case.expect),
			"{}",
			case.id
		);
		assert_eq!(facts.five_hour().class(), QuotaWindowClass::FiveHour);
		assert_eq!(facts.seven_day().class(), QuotaWindowClass::SevenDay);

		if let Some(classified) = &case.expect.classified {
			let five_hour_source = case
				.input
				.windows
				.iter()
				.find(|window| window.duration_minutes == identity.five_hour.duration_minutes)
				.map(|window| window.source_field);
			let seven_day_source = case
				.input
				.windows
				.iter()
				.find(|window| window.duration_minutes == identity.seven_day.duration_minutes)
				.map(|window| window.source_field);

			assert_eq!(five_hour_source, Some(classified.five_hour_source), "{}", case.id);
			assert_eq!(seven_day_source, Some(classified.seven_day_source), "{}", case.id);
		}
	}

	fn fixture_case_by_id<'fixture>(fixture: &'fixture Fixture, id: &str) -> &'fixture FixtureCase {
		let Some(case) = fixture.cases.iter().find(|case| case.id == id) else {
			panic!("accepted quota matrix requires case {id}");
		};

		case
	}

	fn classify_fixture_aggregation_account(
		account: &FixtureAccount,
		identity: FixtureWindowIdentity,
	) -> AccountQuotaFacts {
		assert!((1..=2).contains(&account.depleted_resets_at.len()));

		let durations = [identity.five_hour.duration_minutes, identity.seven_day.duration_minutes];
		let mut observations = account
			.depleted_resets_at
			.iter()
			.enumerate()
			.map(|(index, reset_at)| {
				QuotaWindowObservation::from_duration_minutes(
					durations[index],
					observed(0, *reset_at, 0, ObservationConfidence::High),
				)
			})
			.collect::<Vec<_>>();

		if observations.len() == 1 {
			observations.push(QuotaWindowObservation::from_duration_minutes(
				durations[1],
				observed(100, u64::MAX, 0, ObservationConfidence::High),
			));
		}

		quota::classify_account_quota(
			AccountQuotaObservation::new(AuthenticationObservation::Valid, observations),
			policy(1),
			instant(1),
		)
	}

	fn assert_fixture_aggregation_case(case: &FixtureCase, identity: FixtureWindowIdentity) {
		assert!(case.input.auth.is_none());
		assert!(case.input.observed_at.is_none());
		assert!(case.input.now.is_none());
		assert!(case.input.stale_after_seconds.is_none());
		assert!(case.input.windows.is_empty());
		assert!(matches!(case.expect.state, FixtureState::Depleted));
		assert!(matches!(case.expect.decision, FixtureDecision::WaitingUsage));
		assert_eq!(case.expect.ready_at, None);
		assert!(case.expect.classified.is_none());

		let accounts = case
			.input
			.accounts
			.iter()
			.map(|account| {
				(account.id.clone(), classify_fixture_aggregation_account(account, identity))
			})
			.collect::<Vec<_>>();
		let result = quota::classify_all_accounts(&accounts);
		let AllAccountsQuotaFacts::AllUsageDepleted { accounts, earliest_ready_at } = result else {
			panic!("fixture case {} must classify every account as usage depleted", case.id);
		};
		let actual_ready_at = accounts
			.into_iter()
			.map(|account| (account.account_id, account.ready_at.seconds()))
			.collect::<BTreeMap<_, _>>();

		assert_eq!(actual_ready_at, case.expect.account_ready_at, "{}", case.id);
		assert_eq!(Some(earliest_ready_at.seconds()), case.expect.earliest_ready_at, "{}", case.id);
	}

	#[test]
	fn accepted_matrix_cases_are_deserialized_and_exercised_as_duration_typed_facts() {
		let fixture = fixture();

		assert_fixture_rules(&fixture);

		assert!(!fixture.cases.is_empty());

		for case in &fixture.cases {
			if case.input.accounts.is_empty() {
				assert_fixture_account_case(case, fixture.window_identity);
			} else {
				assert_fixture_aggregation_case(case, fixture.window_identity);
			}
		}
	}

	#[test]
	fn duration_is_the_only_window_identity() {
		assert_eq!(QuotaWindowClass::from_duration_minutes(300), Ok(QuotaWindowClass::FiveHour));
		assert_eq!(QuotaWindowClass::from_duration_minutes(10_080), Ok(QuotaWindowClass::SevenDay));

		for duration in [0, 299, 301, 10_079, 10_081, u32::MAX] {
			assert_eq!(
				QuotaWindowClass::from_duration_minutes(duration),
				Err(UnknownWindowDuration)
			);
		}

		assert_eq!(QuotaWindowClass::FiveHour.duration_minutes(), 300);
		assert_eq!(QuotaWindowClass::SevenDay.duration_minutes(), 10_080);
	}

	#[test]
	fn missing_malformed_unknown_and_duplicate_durations_fail_closed_in_any_order() {
		let valid_five = QuotaWindowObservation::from_duration_minutes(
			300,
			observed(80, 2_000, 1_000, ObservationConfidence::High),
		);
		let valid_seven = QuotaWindowObservation::from_duration_minutes(
			10_080,
			observed(40, 9_000, 1_000, ObservationConfidence::High),
		);
		let invalid = [
			(WindowDurationObservation::Missing, ProbeReason::MissingDuration),
			(WindowDurationObservation::Malformed, ProbeReason::MalformedDuration),
			(WindowDurationObservation::Minutes(42), ProbeReason::UnknownDuration),
		];

		for (duration, reason) in invalid {
			let invalid_window = QuotaWindowObservation::new(
				duration,
				observed(100, 2_000, 1_000, ObservationConfidence::High),
			);

			for observations in [
				vec![invalid_window, valid_five, valid_seven],
				vec![valid_seven, valid_five, invalid_window],
			] {
				let facts = quota::classify_account_quota(
					AccountQuotaObservation::new(AuthenticationObservation::Valid, observations),
					policy(300),
					instant(1_001),
				);

				assert_eq!(
					facts.five_hour().state(),
					QuotaWindowState::UnknownProbeRequired { reason }
				);
				assert_eq!(
					facts.classification(),
					AccountQuotaClassification::UnknownProbeRequired
				);
			}
		}
		for observations in
			[vec![valid_five, valid_seven, valid_five], vec![valid_five, valid_five, valid_seven]]
		{
			let facts = quota::classify_account_quota(
				AccountQuotaObservation::new(AuthenticationObservation::Valid, observations),
				policy(300),
				instant(1_001),
			);

			assert_eq!(
				facts.five_hour().state(),
				QuotaWindowState::UnknownProbeRequired { reason: ProbeReason::DuplicateWindow }
			);
		}
	}

	#[test]
	fn malformed_missing_unknown_stale_future_and_low_confidence_fail_closed() {
		let other = observed(50, 9_000, 1_000, ObservationConfidence::High);
		let cases = [
			(QuotaWindowValueObservation::Missing, ProbeReason::Missing, instant(1_001)),
			(
				QuotaWindowValueObservation::Malformed(MalformedObservation::RemainingOutOfRange),
				ProbeReason::Malformed(MalformedObservation::RemainingOutOfRange),
				instant(1_001),
			),
			(
				QuotaWindowValueObservation::Unknown(UnknownObservation::Unreported),
				ProbeReason::Unknown(UnknownObservation::Unreported),
				instant(1_001),
			),
			(
				observed(50, 2_000, 1_002, ObservationConfidence::High),
				ProbeReason::ObservationFromFuture,
				instant(1_001),
			),
			(
				observed(50, 900, 1_000, ObservationConfidence::High),
				ProbeReason::Malformed(MalformedObservation::ResetBeforeObservation),
				instant(1_001),
			),
			(
				observed(50, 2_000, 1_000, ObservationConfidence::High),
				ProbeReason::Stale,
				instant(1_301),
			),
			(
				observed(50, 2_000, 1_000, ObservationConfidence::Low),
				ProbeReason::LowConfidence,
				instant(1_001),
			),
			(
				observed(50, 2_000, 1_000, ObservationConfidence::Unknown),
				ProbeReason::UnknownConfidence,
				instant(1_001),
			),
		];

		for (five_hour, reason, now) in cases {
			let facts = quota::classify_account_quota(
				AccountQuotaObservation::new(
					AuthenticationObservation::Valid,
					windows(five_hour, other),
				),
				policy(300),
				now,
			);

			assert_eq!(facts.classification(), AccountQuotaClassification::UnknownProbeRequired);
			assert_eq!(
				facts.five_hour().state(),
				QuotaWindowState::UnknownProbeRequired { reason }
			);
		}
	}

	#[test]
	fn freshness_reset_and_percentage_boundaries_are_explicit() {
		assert_eq!(RemainingPercent::new(100).map(RemainingPercent::get), Ok(100));
		assert_eq!(RemainingPercent::new(101), Err(MalformedObservation::RemainingOutOfRange));

		for percent in 1..=100 {
			let facts = quota::classify_account_quota(
				AccountQuotaObservation::new(
					AuthenticationObservation::Valid,
					windows(
						observed(percent, 2_000, 1_000, ObservationConfidence::High),
						observed(percent, 9_000, 1_000, ObservationConfidence::High),
					),
				),
				policy(300),
				instant(1_300),
			);

			assert!(matches!(facts.classification(), AccountQuotaClassification::Available { .. }));
		}

		let elapsed = quota::classify_account_quota(
			AccountQuotaObservation::new(
				AuthenticationObservation::Valid,
				windows(
					observed(0, 1_300, 1_000, ObservationConfidence::High),
					observed(50, 9_000, 1_000, ObservationConfidence::High),
				),
			),
			policy(300),
			instant(1_300),
		);

		assert_eq!(
			elapsed.five_hour().state(),
			QuotaWindowState::UnknownProbeRequired { reason: ProbeReason::ResetElapsed }
		);

		let elapsed_while_previously_available = quota::classify_account_quota(
			AccountQuotaObservation::new(
				AuthenticationObservation::Valid,
				windows(
					observed(80, 1_300, 1_000, ObservationConfidence::High),
					observed(50, 9_000, 1_000, ObservationConfidence::High),
				),
			),
			policy(300),
			instant(1_300),
		);

		assert_eq!(
			elapsed_while_previously_available.five_hour().state(),
			QuotaWindowState::UnknownProbeRequired { reason: ProbeReason::ResetElapsed }
		);
	}

	#[test]
	fn instant_arithmetic_is_checked_and_classification_does_not_wrap() {
		assert_eq!(
			instant(u64::MAX).checked_add(ObservationDuration::from_seconds(1)),
			Err(TimeOverflow)
		);
		assert_eq!(
			instant(u64::MAX - 1).checked_add(ObservationDuration::from_seconds(1)),
			Ok(instant(u64::MAX))
		);
		assert_eq!(ObservationDuration::from_seconds(300).seconds(), 300);
	}

	#[test]
	fn aggregation_is_deterministic_and_never_infers_across_accounts() {
		let fixture = fixture();
		let depleted_a =
			classify_fixture_account_case(fixture_case_by_id(&fixture, "depleted_five_hour"));
		let depleted_b =
			classify_fixture_account_case(fixture_case_by_id(&fixture, "depleted_seven_day"));
		let available =
			classify_fixture_account_case(fixture_case_by_id(&fixture, "available_both_windows"));
		let unknown = classify_fixture_account_case(fixture_case_by_id(
			&fixture,
			"unknown_missing_five_hour",
		));
		let auth = classify_fixture_account_case(fixture_case_by_id(&fixture, "auth_failed"));
		let authentication_unknown = quota::classify_account_quota(
			AccountQuotaObservation::new(
				AuthenticationObservation::Unknown,
				windows(
					observed(0, 2_000, 1_000, ObservationConfidence::High),
					observed(0, 9_000, 1_000, ObservationConfidence::High),
				),
			),
			policy(300),
			instant(1_001),
		);
		let overflow_boundary = quota::classify_account_quota(
			AccountQuotaObservation::new(
				AuthenticationObservation::Valid,
				windows(
					observed(0, u64::MAX, u64::MAX - 1, ObservationConfidence::High),
					observed(0, u64::MAX, u64::MAX - 1, ObservationConfidence::High),
				),
			),
			policy(300),
			instant(u64::MAX),
		);
		let expected = AllAccountsQuotaFacts::AllUsageDepleted {
			accounts: vec![
				AccountReadyAt { account_id: "A", ready_at: instant(2_000) },
				AccountReadyAt { account_id: "B", ready_at: instant(9_000) },
			],
			earliest_ready_at: instant(2_000),
		};

		assert_eq!(quota::classify_all_accounts(&[("A", depleted_a), ("B", depleted_b)]), expected);
		assert_eq!(quota::classify_all_accounts(&[("B", depleted_b), ("A", depleted_a)]), expected);

		for nonusage in [available, unknown, auth, authentication_unknown, overflow_boundary] {
			assert_eq!(
				quota::classify_all_accounts(&[("A", depleted_a), ("B", nonusage)]),
				AllAccountsQuotaFacts::NotAllUsageDepleted
			);
		}

		assert_eq!(
			quota::classify_all_accounts(&[("A", depleted_a), ("A", depleted_b)]),
			AllAccountsQuotaFacts::DuplicateAccount
		);
		assert_eq!(
			quota::classify_all_accounts(&[("A", depleted_a), ("A", available)]),
			AllAccountsQuotaFacts::DuplicateAccount
		);
		assert_eq!(
			quota::classify_all_accounts::<&str>(&[]),
			AllAccountsQuotaFacts::NotAllUsageDepleted
		);
	}
}
