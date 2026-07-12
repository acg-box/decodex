use crate::{
	lane_authority::{AuthorityEvent, AuthorityEventDraft},
	prelude::{Result, eyre},
	state::StateStore,
};

impl StateStore {
	pub fn initialize_authority_generation(
		&self,
		generation: u64,
		genesis_hash: &[u8],
	) -> Result<()> {
		self.sqlite
			.as_ref()
			.ok_or_else(|| eyre::eyre!("Authority events require a persistent StateStore."))?
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
			.initialize_authority_generation(generation, genesis_hash)
	}

	pub fn append_authority_event(&self, draft: AuthorityEventDraft) -> Result<AuthorityEvent> {
		self.sqlite
			.as_ref()
			.ok_or_else(|| eyre::eyre!("Authority events require a persistent StateStore."))?
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
			.append_authority_event(draft)
	}

	pub fn verify_authority_events(&self) -> Result<Vec<AuthorityEvent>> {
		self.sqlite
			.as_ref()
			.ok_or_else(|| eyre::eyre!("Authority events require a persistent StateStore."))?
			.lock()
			.map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?
			.verify_authority_events()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::lane_authority::{AuthorityDecision, AuthorityEventType, AuthorityReasonCode};

	#[test]
	fn authority_operator_readback_detects_persisted_rewrite_and_delete() {
		for mutation in ["rewrite", "delete", "indexed_hash", "fork"] {
			let temp = tempfile::tempdir().expect("tempdir");
			let database = temp.path().join("state.sqlite");
			let store = StateStore::open(&database).expect("store");
			store.initialize_authority_generation(1, &[3_u8; 32]).expect("genesis");
			store.append_authority_event(draft("event-1", "transition-1")).expect("first event");
			store.append_authority_event(draft("event-2", "transition-2")).expect("second event");
			drop(store);
			let connection = rusqlite::Connection::open(&database).expect("tamper connection");
			match mutation {
				"rewrite" => {
					connection
						.execute(
							"UPDATE authority_events SET event_cbor = x'00' WHERE sequence = 1",
							[],
						)
						.expect("rewrite row");
				},
				"delete" => {
					connection
						.execute("DELETE FROM authority_events WHERE sequence = 1", [])
						.expect("delete row");
				},
				"indexed_hash" => {
					connection
						.execute(
							"UPDATE authority_events SET event_hash = zeroblob(32) WHERE sequence = 1",
							[],
						)
						.expect("rewrite indexed hash");
				},
				"fork" => {
					connection
						.execute(
							"INSERT INTO authority_events
							 (generation, sequence, event_id, previous_event_hash, event_hash,
							  event_cbor, recorded_at_unix_micros)
							 SELECT 2, 1, 'fork-event', previous_event_hash, event_hash,
							        event_cbor, recorded_at_unix_micros
							 FROM authority_events WHERE generation = 1 AND sequence = 1",
							[],
						)
						.expect("insert fork row");
				},
				_ => unreachable!(),
			}
			drop(connection);
			assert!(
				StateStore::open_with_invocation(
					&database,
					crate::authority_broker::test_invocation_identity(),
				)
				.is_err(),
				"production open must fail closed for {mutation}",
			);
			let reopened = StateStore::open(&database).expect("reopen");
			assert!(reopened.verify_authority_events().is_err(), "{mutation} must be detected");
		}
	}

	#[test]
	fn lane_authority_v2_c5_persistent_chain_reopens_at_exact_head() {
		let temp = tempfile::tempdir().expect("tempdir");
		let database = temp.path().join("state.sqlite");
		let store = StateStore::open(&database).expect("store");
		store.initialize_authority_generation(4, &[9_u8; 32]).expect("genesis");
		let first =
			store.append_authority_event(draft("event-1", "transition-1")).expect("first event");
		let second =
			store.append_authority_event(draft("event-2", "transition-2")).expect("second event");
		assert_eq!(first.sequence, 1);
		assert_eq!(second.sequence, 2);
		drop(store);
		let reopened = StateStore::open(&database).expect("reopen");
		assert_eq!(reopened.verify_authority_events().expect("verify"), vec![first, second]);
	}

	fn draft(event_id: &str, transition_id: &str) -> AuthorityEventDraft {
		AuthorityEventDraft {
			event_id: event_id.to_owned(),
			event_type: AuthorityEventType::TransitionCommitted,
			transition_id: transition_id.to_owned(),
			correlation_id: String::from("correlation-1"),
			causation_id: String::from("cause-1"),
			project_key: Some(String::from("pubfi")),
			tracker_issue_id: Some(String::from("PUB-1711")),
			project_binding_fingerprint: Some(String::from("binding-1")),
			invocation_identity_fingerprint: String::from("invocation-1"),
			observed_facts_fingerprint: String::from("facts-1"),
			decision: AuthorityDecision::Committed,
			reason_codes: vec![AuthorityReasonCode::BindingMatched],
			operation_id: Some(String::from("operation-1")),
			effect_id: None,
			receipt_ref: None,
			runtime_version: String::from("0.2.0"),
			recorded_at_unix_micros: 1,
			boot_id_fingerprint: String::from("boot-1"),
			monotonic_nanos: 1,
		}
	}
}
