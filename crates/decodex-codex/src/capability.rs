use std::collections::BTreeMap;

use crate::{BuildId, SchemaContract};

/// Adapter capabilities whose live state is tracked independently of schema presence.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Capability {
	/// JSON-RPC initialization handshake.
	Initialize,
	/// Bounded thread listing.
	ThreadList,
	/// Exact-ID thread readback.
	ThreadRead,
	/// Explicit thread archival.
	ThreadArchive,
	/// Paginated rather than legacy thread history.
	PaginatedThreads,
	/// Native run-local collaboration event shape.
	NativeCollaboration,
	/// Global title search, distinct from filtered thread listing.
	GlobalTitleSearch,
}
impl Capability {
	fn schema_method(self) -> Option<&'static str> {
		match self {
			Self::Initialize => Some("initialize"),
			Self::ThreadList => Some("thread/list"),
			Self::ThreadRead => Some("thread/read"),
			Self::ThreadArchive => Some("thread/archive"),
			Self::PaginatedThreads => Some("thread/start"),
			Self::NativeCollaboration => None,
			Self::GlobalTitleSearch => Some("thread/search"),
		}
	}

	fn all() -> &'static [Self] {
		&[
			Self::Initialize,
			Self::ThreadList,
			Self::ThreadRead,
			Self::ThreadArchive,
			Self::PaginatedThreads,
			Self::NativeCollaboration,
			Self::GlobalTitleSearch,
		]
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
	/// The capability is not supported by this exact build.
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

/// One live observation, already stripped of raw protocol data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MethodObservation {
	/// Capability tested by the live method.
	pub capability: Capability,
	/// Sanitized live outcome.
	pub outcome: LiveMethodOutcome,
}

/// Exact schema/live contradiction retained under a Codex build.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityContradiction {
	/// Exact Codex build that produced the contradiction.
	pub build: BuildId,
	/// Schema-advertised capability contradicted by live evidence.
	pub capability: Capability,
	/// Sanitized live result.
	pub live_outcome: LiveMethodOutcome,
}

/// Fully typed capability profile for one exact Codex build.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityProfile {
	build: BuildId,
	schema_fingerprint: String,
	states: BTreeMap<Capability, CapabilityState>,
	contradictions: Vec<CapabilityContradiction>,
}
impl CapabilityProfile {
	/// Negotiate schema evidence against sanitized live outcomes.
	pub(crate) fn negotiate(
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

	/// Exact Codex build owning this profile.
	pub fn build(&self) -> &BuildId {
		&self.build
	}

	/// SHA-256 fingerprint of the schema generated by this exact build.
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

/// Capability-cache insertion failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NegotiationError {
	/// A different profile was already cached for the exact build.
	EvidenceConflict,
}

/// In-memory cache keyed only by exact Codex build identity.
#[derive(Clone, Debug, Default)]
pub struct CapabilityCache {
	profiles: BTreeMap<BuildId, CapabilityProfile>,
}
impl CapabilityCache {
	/// Store an attested profile without replacing contradictory exact-build evidence.
	pub fn insert(&mut self, profile: CapabilityProfile) -> Result<(), NegotiationError> {
		let build = profile.build().clone();

		if self.profiles.get(&build).is_some_and(|cached| cached != &profile) {
			return Err(NegotiationError::EvidenceConflict);
		}

		self.profiles.insert(build, profile);

		Ok(())
	}

	/// Look up only an exact build match; stale nearest-build fallback is forbidden.
	pub fn get(&self, build: &BuildId) -> Option<&CapabilityProfile> {
		self.profiles.get(build)
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
		let build = BuildId::new("codex-cli 0.144.0-alpha.4").unwrap();
		let profile = CapabilityProfile::negotiate(
			build.clone(),
			"schema-a",
			&schema,
			[MethodObservation {
				capability: Capability::PaginatedThreads,
				outcome: LiveMethodOutcome::Unsupported { code: -32_601 },
			}],
		);

		assert_eq!(
			profile.state(Capability::PaginatedThreads),
			&CapabilityState::Unsupported { reason: UnsupportedReason::MethodNotFound }
		);
		assert_eq!(profile.contradictions().len(), 1);
		assert_eq!(profile.contradictions()[0].build, build);
	}

	#[test]
	fn cache_never_reuses_a_profile_for_a_different_build() {
		let schema = SchemaContract::validate(SchemaMarker::accepted()).unwrap();
		let build_a = BuildId::new("codex-cli build-a").unwrap();
		let build_b = BuildId::new("codex-cli build-b").unwrap();
		let profile = CapabilityProfile::negotiate(build_a.clone(), "schema-a", &schema, []);
		let mut cache = CapabilityCache::default();

		cache.insert(profile).unwrap();

		assert!(cache.get(&build_a).is_some());
		assert!(cache.get(&build_b).is_none());
	}

	#[test]
	fn cache_rejects_conflicting_evidence_for_the_same_attested_build() {
		let schema = SchemaContract::validate(SchemaMarker::accepted()).unwrap();
		let build = BuildId::new("codex-cli build-a").unwrap();
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
			BuildId::new("codex-cli build-a").unwrap(),
			"schema-a",
			&schema,
			[],
		);

		assert_eq!(
			profile.state(Capability::ThreadArchive),
			&CapabilityState::Unavailable { reason: UnavailableReason::NotProbed }
		);
	}
}
