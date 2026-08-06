use std::collections::BTreeMap;

use crate::{BuildId, SchemaContract};

pub use decodex_core::CodexCapability as Capability;

trait CapabilityExt {
	fn schema_method(self) -> Option<&'static str>;
	fn all() -> &'static [Self]
	where
		Self: Sized;
}
impl CapabilityExt for Capability {
	fn schema_method(self) -> Option<&'static str> {
		match self {
			Self::Initialize => Some("initialize"),
			Self::AccountRead => Some("account/read"),
			Self::ThreadList => Some("thread/list"),
			Self::ThreadRead => Some("thread/read"),
			Self::ThreadArchive => Some("thread/archive"),
			Self::PaginatedHistory => None,
			Self::NativeCollaboration => None,
			Self::ThreadSearch => Some("thread/search"),
		}
	}

	fn all() -> &'static [Self] {
		&Self::ALL
	}
}

/// Why a capability is unsupported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedReason {
	/// The generated schema does not contain the relevant marker.
	SchemaMissing,
	/// The live server returned JSON-RPC method-not-found.
	MethodNotFound,
	/// The live server rejected the advertised operation for another stable reason.
	CodexRejected,
}

/// Closed reasons why live capability evidence is unavailable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnavailableReason {
	/// The optional method was not exercised by this read-only probe.
	NotProbed,
	/// The bounded probe could not establish a live result.
	ProbeFailed,
}

/// Closed limitations retained for a supported capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DegradedReason {
	/// Only legacy, non-paginated history is available.
	LegacyHistoryOnly,
}

/// Typed negotiated capability state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapabilityState {
	/// Schema and live evidence support the capability.
	Supported,
	/// The capability is not supported by this observed Codex executable.
	Unsupported {
		/// Classified unsupported reason.
		reason: UnsupportedReason,
	},
	/// The capability cannot currently be evaluated or used.
	Unavailable {
		/// Classified unavailable reason.
		reason: UnavailableReason,
	},
	/// The capability is usable only with an explicit limitation.
	Degraded {
		/// Classified degradation reason.
		reason: DegradedReason,
	},
}

/// Sanitized result of one live method probe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveMethodOutcome {
	/// The live method completed with a typed result.
	Supported,
	/// The live method returned a stable protocol rejection.
	Unsupported {
		/// JSON-RPC error code; no raw message is retained.
		code: i64,
	},
	/// The method could not be probed.
	Unavailable {
		/// Classified unavailable reason.
		reason: UnavailableReason,
	},
	/// The method completed with a known limitation.
	Degraded {
		/// Classified degradation reason.
		reason: DegradedReason,
	},
}

/// Capability-cache insertion failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NegotiationError {
	/// A different profile was already cached for the same observed executable.
	EvidenceConflict,
}

/// One live observation, already stripped of raw protocol data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MethodObservation {
	/// Capability tested by the live method.
	pub capability: Capability,
	/// Sanitized live outcome.
	pub outcome: LiveMethodOutcome,
}

/// Schema/live contradiction retained under one observed Codex executable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityContradiction {
	/// Exact Codex build that produced the contradiction.
	pub build: BuildId,
	/// Schema-advertised capability contradicted by live evidence.
	pub capability: Capability,
	/// Sanitized live result.
	pub live_outcome: LiveMethodOutcome,
}

/// Fully typed capability profile for one observed Codex executable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityProfile {
	build: BuildId,
	schema_fingerprint: String,
	states: BTreeMap<Capability, CapabilityState>,
	contradictions: Vec<CapabilityContradiction>,
}
impl CapabilityProfile {
	/// Negotiate schema evidence against sanitized live outcomes.
	#[doc(hidden)]
	pub fn negotiate(
		build: BuildId,
		schema_fingerprint: impl Into<String>,
		schema: &SchemaContract,
		observations: impl IntoIterator<Item = MethodObservation>,
	) -> Self {
		let observations = observations
			.into_iter()
			.map(|item| (item.capability, item.outcome))
			.collect::<BTreeMap<_, _>>();
		let mut states = BTreeMap::new();
		let mut contradictions = Vec::new();

		for capability in Capability::all() {
			let advertised = match capability {
				Capability::NativeCollaboration => schema.advertises_collaboration(),
				Capability::PaginatedHistory => schema.advertises_paginated_history(),
				_ => capability
					.schema_method()
					.is_some_and(|method| schema.advertises_request(method)),
			};
			let outcome = observations.get(capability);
			let state = if !advertised {
				CapabilityState::Unsupported { reason: UnsupportedReason::SchemaMissing }
			} else {
				match outcome {
					Some(LiveMethodOutcome::Supported) => CapabilityState::Supported,
					Some(LiveMethodOutcome::Unsupported { code }) => {
						let reason = if *code == -32_601 {
							UnsupportedReason::MethodNotFound
						} else {
							UnsupportedReason::CodexRejected
						};

						CapabilityState::Unsupported { reason }
					},
					Some(LiveMethodOutcome::Unavailable { reason }) =>
						CapabilityState::Unavailable { reason: *reason },
					Some(LiveMethodOutcome::Degraded { reason }) =>
						CapabilityState::Degraded { reason: *reason },
					None => CapabilityState::Unavailable { reason: UnavailableReason::NotProbed },
				}
			};

			if advertised
				&& matches!(
					outcome,
					Some(LiveMethodOutcome::Unsupported { .. })
						| Some(LiveMethodOutcome::Unavailable { .. })
				) {
				contradictions.push(CapabilityContradiction {
					build: build.clone(),
					capability: *capability,
					live_outcome: outcome.cloned().expect("matched outcome"),
				});
			}

			states.insert(*capability, state);
		}

		Self { build, schema_fingerprint: schema_fingerprint.into(), states, contradictions }
	}

	fn merge(self, incoming: Self) -> Result<Self, NegotiationError> {
		if self.build != incoming.build || self.schema_fingerprint != incoming.schema_fingerprint {
			return Err(NegotiationError::EvidenceConflict);
		}

		let mut states = BTreeMap::new();

		for capability in Capability::all() {
			let current = self.state(*capability);
			let next = incoming.state(*capability);
			let merged = merge_state(current, next)?;

			states.insert(*capability, merged);
		}

		let mut contradictions = self.contradictions;

		for contradiction in incoming.contradictions {
			if !contradictions.contains(&contradiction) {
				contradictions.push(contradiction);
			}
		}

		Ok(Self {
			build: incoming.build,
			schema_fingerprint: incoming.schema_fingerprint,
			states,
			contradictions,
		})
	}

	/// Exact Codex build owning this profile.
	pub fn build(&self) -> &BuildId {
		&self.build
	}

	/// SHA-256 fingerprint of the schema generated by this observed executable.
	pub fn schema_fingerprint(&self) -> &str {
		&self.schema_fingerprint
	}

	/// Typed state for a capability.
	pub fn state(&self, capability: Capability) -> &CapabilityState {
		self.states.get(&capability).expect("all capabilities are profiled")
	}

	/// Recorded schema/live contradictions.
	pub fn contradictions(&self) -> &[CapabilityContradiction] {
		&self.contradictions
	}
}

/// In-memory cache keyed only by exact Codex build identity.
#[derive(Clone, Debug, Default)]
pub struct CapabilityCache {
	profiles: BTreeMap<BuildId, CapabilityProfile>,
}
impl CapabilityCache {
	/// Merge monotonic evidence without replacing contradictory executable outcomes.
	pub fn insert(
		&mut self,
		profile: CapabilityProfile,
	) -> Result<CapabilityProfile, NegotiationError> {
		let build = profile.build().clone();
		let profile = match self.profiles.get(&build).cloned() {
			Some(cached) => cached.merge(profile)?,
			None => profile,
		};

		self.profiles.insert(build, profile.clone());

		Ok(profile)
	}

	/// Look up only the same observed executable; stale nearest-build fallback is forbidden.
	pub fn get(&self, build: &BuildId) -> Option<&CapabilityProfile> {
		self.profiles.get(build)
	}

	/// Number of observed-executable profiles retained by this cache.
	pub fn len(&self) -> usize {
		self.profiles.len()
	}

	/// Return whether no executable evidence has been retained.
	pub fn is_empty(&self) -> bool {
		self.profiles.is_empty()
	}
}

fn merge_state(
	current: &CapabilityState,
	incoming: &CapabilityState,
) -> Result<CapabilityState, NegotiationError> {
	if current == incoming {
		return Ok(current.clone());
	}

	match (current, incoming) {
		(CapabilityState::Unavailable { reason: UnavailableReason::NotProbed }, other) =>
			Ok(other.clone()),
		(other, CapabilityState::Unavailable { reason: UnavailableReason::NotProbed }) =>
			Ok(other.clone()),
		(CapabilityState::Unavailable { reason: UnavailableReason::ProbeFailed }, other) =>
			Ok(other.clone()),
		(other, CapabilityState::Unavailable { reason: UnavailableReason::ProbeFailed }) =>
			Ok(other.clone()),
		_ => Err(NegotiationError::EvidenceConflict),
	}
}

#[cfg(test)]
mod tests {
	use crate::{
		CapabilityState, SchemaMarker, UnavailableReason, UnsupportedReason,
		capability::{
			BuildId, Capability, CapabilityCache, CapabilityProfile, LiveMethodOutcome,
			MethodObservation, SchemaContract,
		},
	};

	#[test]
	fn schema_advertised_live_rejected_is_recorded_by_exact_build() {
		let schema = SchemaContract::validate(SchemaMarker::accepted()).unwrap();
		let build = BuildId::for_test("codex-cli 0.144.0-alpha.4");
		let profile = CapabilityProfile::negotiate(
			build.clone(),
			"schema-a",
			&schema,
			[MethodObservation {
				capability: Capability::PaginatedHistory,
				outcome: LiveMethodOutcome::Unsupported { code: -32_601 },
			}],
		);

		assert_eq!(
			profile.state(Capability::PaginatedHistory),
			&CapabilityState::Unsupported { reason: UnsupportedReason::MethodNotFound }
		);
		assert_eq!(profile.contradictions().len(), 1);
		assert_eq!(profile.contradictions()[0].build, build);
	}

	#[test]
	fn cache_never_reuses_a_profile_for_a_different_build() {
		let schema = SchemaContract::validate(SchemaMarker::accepted()).unwrap();
		let build_a = BuildId::for_test("codex-cli build-a");
		let build_b = BuildId::for_test("codex-cli build-b");
		let profile = CapabilityProfile::negotiate(build_a.clone(), "schema-a", &schema, []);
		let mut cache = CapabilityCache::default();

		cache.insert(profile).unwrap();

		assert!(cache.get(&build_a).is_some());
		assert!(cache.get(&build_b).is_none());
	}

	#[test]
	fn cache_rejects_conflicting_evidence_for_the_same_attested_build() {
		let schema = SchemaContract::validate(SchemaMarker::accepted()).unwrap();
		let build = BuildId::for_test("codex-cli build-a");
		let first = CapabilityProfile::negotiate(build.clone(), "schema-a", &schema, []);
		let second = CapabilityProfile::negotiate(build, "schema-b", &schema, []);
		let mut cache = CapabilityCache::default();

		cache.insert(first).unwrap();

		assert_eq!(cache.insert(second), Err(super::NegotiationError::EvidenceConflict));
	}

	#[test]
	fn unprobed_optional_methods_are_explicitly_unavailable() {
		let schema = SchemaContract::validate(SchemaMarker::accepted()).unwrap();
		let profile = CapabilityProfile::negotiate(
			BuildId::for_test("codex-cli build-a"),
			"schema-a",
			&schema,
			[],
		);

		assert_eq!(
			profile.state(Capability::ThreadArchive),
			&CapabilityState::Unavailable { reason: UnavailableReason::NotProbed }
		);
	}

	#[test]
	fn cache_upgrades_not_probed_evidence_without_losing_exact_build_authority() {
		let schema = SchemaContract::validate(SchemaMarker::accepted()).unwrap();
		let build = BuildId::for_test("codex-cli build-a");
		let first = CapabilityProfile::negotiate(build.clone(), "schema-a", &schema, []);
		let second = CapabilityProfile::negotiate(
			build.clone(),
			"schema-a",
			&schema,
			[MethodObservation {
				capability: Capability::ThreadRead,
				outcome: LiveMethodOutcome::Supported,
			}],
		);
		let mut cache = CapabilityCache::default();

		cache.insert(first).unwrap();

		let merged = cache.insert(second).unwrap();

		assert_eq!(merged.state(Capability::ThreadRead), &CapabilityState::Supported);
		assert_eq!(cache.get(&build), Some(&merged));
	}
}
