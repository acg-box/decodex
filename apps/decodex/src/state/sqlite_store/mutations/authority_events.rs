use rusqlite::{OptionalExtension, Transaction};

use crate::{
	lane_authority::{AuthorityEvent, AuthorityEventDraft, verify_authority_event_chain},
	prelude::{Result, eyre},
	state::sqlite_store::{SqliteStateStore, mutations::params},
};

pub(in crate::state) struct AuthorityChainSnapshot {
	pub(in crate::state) generation: u64,
	pub(in crate::state) genesis_hash: Vec<u8>,
	pub(in crate::state) events: Vec<AuthorityEvent>,
}

impl SqliteStateStore {
	pub(in crate::state) fn initialize_authority_generation(
		&self,
		generation: u64,
		genesis_hash: &[u8],
	) -> Result<()> {
		if generation == 0 || genesis_hash.len() != 32 {
			eyre::bail!("Authority generation genesis is invalid.");
		}
		let inserted = self.connection.execute(
			"INSERT OR IGNORE INTO authority_event_chain_head
			 (singleton, generation, sequence, genesis_hash, event_hash)
			 VALUES (1, ?1, 0, ?2, ?2)",
			params![i64::try_from(generation)?, genesis_hash],
		)?;
		if inserted == 1 {
			return Ok(());
		}
		let existing = self.connection.query_row(
			"SELECT generation, genesis_hash FROM authority_event_chain_head WHERE singleton = 1",
			[],
			|row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
		)?;
		if existing != (i64::try_from(generation)?, genesis_hash.to_vec()) {
			eyre::bail!("Authority generation cannot be replaced.");
		}
		Ok(())
	}

	pub(in crate::state) fn append_authority_event(
		&mut self,
		draft: AuthorityEventDraft,
	) -> Result<AuthorityEvent> {
		let transaction = self.connection.transaction()?;
		let event = append_authority_event_in_transaction(&transaction, draft)?;
		transaction.commit()?;
		Ok(event)
	}

	pub(in crate::state) fn verify_authority_events(&self) -> Result<Vec<AuthorityEvent>> {
		Ok(self.authority_chain_snapshot()?.events)
	}

	pub(in crate::state) fn authority_chain_snapshot(&self) -> Result<AuthorityChainSnapshot> {
		let (generation, head_sequence, genesis_hash, head_hash) = self.connection.query_row(
			"SELECT generation, sequence, genesis_hash, event_hash
			 FROM authority_event_chain_head WHERE singleton = 1",
			[],
			|row| {
				Ok((
					row.get::<_, i64>(0)?,
					row.get::<_, i64>(1)?,
					row.get::<_, Vec<u8>>(2)?,
					row.get::<_, Vec<u8>>(3)?,
				))
			},
		)?;
		let mut statement = self.connection.prepare(
			"SELECT generation, sequence, event_id, previous_event_hash, event_hash,
			        event_cbor, recorded_at_unix_micros
			 FROM authority_events ORDER BY generation ASC, sequence ASC",
		)?;
		let events = statement
			.query_map([], |row| {
				Ok((
					row.get::<_, i64>(0)?,
					row.get::<_, i64>(1)?,
					row.get::<_, String>(2)?,
					row.get::<_, Vec<u8>>(3)?,
					row.get::<_, Vec<u8>>(4)?,
					row.get::<_, Vec<u8>>(5)?,
					row.get::<_, i64>(6)?,
				))
			})?
			.map(|row| {
				let (
					row_generation,
					row_sequence,
					event_id,
					previous_hash,
					event_hash,
					bytes,
					recorded_at,
				) = row?;
				let event = minicbor::decode::<AuthorityEvent>(&bytes)?;
				if i64::try_from(event.generation)? != row_generation
					|| i64::try_from(event.sequence)? != row_sequence
					|| event.draft.event_id != event_id
					|| event.previous_event_hash != previous_hash
					|| event.event_hash != event_hash
					|| event.draft.recorded_at_unix_micros != recorded_at
				{
					eyre::bail!("Authority event indexed columns do not match canonical bytes.");
				}
				Ok(event)
			})
			.collect::<Result<Vec<_>>>()?;
		verify_authority_event_chain(&events, u64::try_from(generation)?, &genesis_hash)?;
		if i64::try_from(events.len())? != head_sequence
			|| events.last().map_or(genesis_hash.as_slice(), |event| event.event_hash.as_slice())
				!= head_hash
		{
			eyre::bail!("Authority event chain head does not match persisted events.");
		}
		Ok(AuthorityChainSnapshot { generation: u64::try_from(generation)?, genesis_hash, events })
	}
}

pub(in crate::state::sqlite_store::mutations) fn append_authority_event_in_transaction(
	transaction: &Transaction<'_>,
	draft: AuthorityEventDraft,
) -> Result<AuthorityEvent> {
	let (generation, sequence, previous_hash) = transaction
		.query_row(
			"SELECT generation, sequence, event_hash
				 FROM authority_event_chain_head WHERE singleton = 1",
			[],
			|row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, Vec<u8>>(2)?)),
		)
		.optional()?
		.ok_or_else(|| eyre::eyre!("Authority generation is not initialized."))?;
	let next_sequence =
		sequence.checked_add(1).ok_or_else(|| eyre::eyre!("Authority event sequence overflow."))?;
	let event = AuthorityEvent::append(
		u64::try_from(generation)?,
		u64::try_from(next_sequence)?,
		&previous_hash,
		draft,
	)?;
	transaction.execute(
		"INSERT INTO authority_events
			 (generation, sequence, event_id, previous_event_hash, event_hash,
			  event_cbor, recorded_at_unix_micros)
			 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
		params![
			generation,
			next_sequence,
			&event.draft.event_id,
			&event.previous_event_hash,
			&event.event_hash,
			event.canonical_bytes()?,
			event.draft.recorded_at_unix_micros,
		],
	)?;
	let advanced = transaction.execute(
		"UPDATE authority_event_chain_head SET sequence = ?1, event_hash = ?2
			 WHERE singleton = 1 AND generation = ?3 AND sequence = ?4 AND event_hash = ?5",
		params![next_sequence, &event.event_hash, generation, sequence, &previous_hash],
	)?;
	if advanced != 1 {
		eyre::bail!("Authority event chain head CAS rejected a stale writer.");
	}
	Ok(event)
}
