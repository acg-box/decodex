use sha2::{Digest as _, Sha256};

use crate::{
	lane_authority::{
		AuthorityDecision, AuthorityEventDraft, AuthorityEventType, AuthorityReasonCode,
		RoutingQuarantine, RoutingQuarantineReason,
	},
	prelude::{Result, eyre},
	state::StateStore,
};

impl StateStore {
	pub(crate) fn record_routing_quarantine(&self, quarantine: RoutingQuarantine) -> Result<()> {
		if quarantine.tracker_issue_id.trim().is_empty()
			|| quarantine.epoch == 0
			|| quarantine.selector_fingerprint.len() != 64
		{
			eyre::bail!("routing_quarantine_invalid");
		}
		let event = self
			.invocation_identity
			.as_ref()
			.map(|invocation| -> Result<AuthorityEventDraft> {
				let reason = match quarantine.reason {
					RoutingQuarantineReason::NoMatch => AuthorityReasonCode::BindingMismatch,
					RoutingQuarantineReason::Ambiguous
					| RoutingQuarantineReason::InvalidSelector => AuthorityReasonCode::AmbiguousRouting,
				};
				let now = crate::state::timestamp_parts().unix;
				Ok(AuthorityEventDraft {
					event_id: format!(
						"routing-quarantine:{}:{}",
						quarantine.tracker_issue_id,
						&quarantine.selector_fingerprint[..16],
					),
					event_type: AuthorityEventType::LaneQuarantined,
					transition_id: format!("routing-quarantine:{}", quarantine.tracker_issue_id),
					correlation_id: format!("routing:{}", quarantine.tracker_issue_id),
					causation_id: invocation.invocation_id().to_owned(),
					project_key: None,
					tracker_issue_id: Some(quarantine.tracker_issue_id.clone()),
					project_binding_fingerprint: None,
					invocation_identity_fingerprint: hex(&invocation.fingerprint()?),
					observed_facts_fingerprint: quarantine.selector_fingerprint.clone(),
					decision: AuthorityDecision::Rejected,
					reason_codes: vec![reason],
					operation_id: None,
					effect_id: None,
					receipt_ref: None,
					runtime_version: env!("CARGO_PKG_VERSION").to_owned(),
					recorded_at_unix_micros: now.saturating_mul(1_000_000),
					boot_id_fingerprint: hex(&Sha256::digest(
						crate::state::current_host_boot_id()
							.unwrap_or_else(|| String::from("unavailable")),
					)),
					monotonic_nanos: 0,
				})
			})
			.transpose()?;
		if let Some(sqlite) = &self.sqlite {
			sqlite
				.lock()
				.map_err(|_| eyre::eyre!("SQLite state lock poisoned."))?
				.record_routing_quarantine(&quarantine, event)?;
		}
		let mut state = self.inner.lock().map_err(|_| eyre::eyre!("State lock poisoned."))?;
		if let Some(existing) = state.routing_quarantines.get(&quarantine.tracker_issue_id)
			&& existing != &quarantine
		{
			eyre::bail!("routing_quarantine_authority_collision");
		}
		state.routing_quarantines.insert(quarantine.tracker_issue_id.clone(), quarantine);
		drop(state);
		self.advance_authority_anchor()?;
		Ok(())
	}

	pub(crate) fn routing_quarantine(
		&self,
		tracker_issue_id: &str,
	) -> Result<Option<RoutingQuarantine>> {
		if let Some(sqlite) = &self.sqlite {
			return sqlite
				.lock()
				.map_err(|_| eyre::eyre!("SQLite state lock poisoned."))?
				.routing_quarantine(tracker_issue_id);
		}
		Ok(self
			.inner
			.lock()
			.map_err(|_| eyre::eyre!("State lock poisoned."))?
			.routing_quarantines
			.get(tracker_issue_id)
			.cloned())
	}
}

fn hex(bytes: &[u8]) -> String {
	bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn brokered_routing_quarantine_commits_reservation_and_authority_event_atomically() {
		let temp = tempfile::tempdir().expect("tempdir");
		let database = temp.path().join("runtime.sqlite3");
		let store = StateStore::open(&database).expect("prepare store");
		store.initialize_authority_generation(1, &[7_u8; 32]).expect("generation");
		crate::lane_authority::protected_head::AuthorityAnchor::initialize_for_test(
			temp.path(),
			1,
			&[7_u8; 32],
			&[],
		)
		.expect("anchor");
		drop(store);
		let mut store = StateStore::open_with_invocation(
			&database,
			crate::authority_broker::test_invocation_identity(),
		)
		.expect("brokered store");
		store.attach_authority_anchor(temp.path()).expect("attach anchor");
		let binding = crate::lane_authority::ProjectBinding::new(
			"pubfi",
			"helixbox",
			"pubfi-mono",
			"team-pubfi",
			"decodex:queued:pubfi",
			"binding-1",
		)
		.expect("binding");
		let resolution = crate::lane_authority::resolve_project_binding(
			vec![binding],
			"team-pubfi",
			"decodex:queued:pubfi",
			&[String::from("repo:another-repository")],
		);
		let quarantine = resolution.quarantine("issue-1", &"a".repeat(64)).expect("quarantine");
		store.record_routing_quarantine(quarantine.clone()).expect("record");
		assert_eq!(store.routing_quarantine("issue-1").expect("read"), Some(quarantine));
		let events = store.verify_authority_events().expect("events");
		assert_eq!(events.len(), 1);
		assert_eq!(events[0].draft.event_type, AuthorityEventType::LaneQuarantined);
		assert_eq!(
			crate::lane_authority::protected_head::protected_head_sequence_for_test(temp.path())
				.expect("head"),
			1
		);
	}
}
