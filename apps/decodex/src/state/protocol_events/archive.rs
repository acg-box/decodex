use sha2::{Digest as _, Sha256};

use crate::{
	prelude::{Result, eyre},
	state::runtime_records::ProtocolEventRecord,
};

pub(super) const TERMINAL_THREAD_ARCHIVE_EVENT_TYPES: [&str; 2] =
	["thread/archive", "thread/archive/discarded"];

const DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE: &str = "protocol/post_archive_event/discarded";

pub(super) fn protocol_event_is_terminal_thread_archive(event_type: &str) -> bool {
	TERMINAL_THREAD_ARCHIVE_EVENT_TYPES.contains(&event_type)
}

pub(super) fn protocol_event_can_be_discarded_after_archive(event: &ProtocolEventRecord) -> bool {
	!protocol_event_is_terminal_thread_archive(&event.event_type)
		&& event.event_type != DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE
}

pub(super) fn protocol_event_conflict_should_be_discarded_after_archive(
	existing: &ProtocolEventRecord,
	candidate: &ProtocolEventRecord,
) -> bool {
	protocol_event_is_terminal_thread_archive(&existing.event_type)
		&& protocol_event_can_be_discarded_after_archive(candidate)
}

pub(super) fn protocol_event_is_discarded_post_archive_collision(
	existing: &ProtocolEventRecord,
	candidate: &ProtocolEventRecord,
) -> bool {
	existing.event_type == DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE
		&& candidate.event_type == DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE
		&& !existing.is_idempotent_replay_of(candidate)
}

pub(super) fn discarded_post_archive_protocol_event_with_log(
	run_id: &str,
	event: ProtocolEventRecord,
) -> ProtocolEventRecord {
	let original_sequence_number = event.sequence_number;
	let original_event_type = event.event_type.clone();
	let discarded = discarded_post_archive_protocol_event(event);

	tracing::info!(
		run_id,
		original_sequence_number,
		original_event_type,
		discarded_sequence_number = discarded.sequence_number,
		discarded_event_type = discarded.event_type.as_str(),
		"Discarded late app-server protocol event after terminal thread archive barrier; child protocol activity is isolated from parent journal and closeout state."
	);

	discarded
}

pub(super) fn next_discarded_post_archive_sequence_after_collision(
	sequence_number: i64,
) -> Result<i64> {
	if sequence_number == i64::MIN {
		eyre::bail!("Post-archive discarded protocol event sequence space is exhausted.");
	}

	Ok(sequence_number - 1)
}

pub(super) fn ensure_protocol_event_replay_matches(
	run_id: &str,
	existing: &ProtocolEventRecord,
	candidate: &ProtocolEventRecord,
) -> Result<()> {
	if existing.is_idempotent_replay_of(candidate) {
		return Ok(());
	}

	eyre::bail!(
		"Protocol event `{run_id}` sequence `{}` conflicts with an existing runtime journal event: \
		 existing event_type=`{}` payload_sha256=`{}`, candidate event_type=`{}` payload_sha256=`{}`.",
		candidate.sequence_number,
		existing.event_type,
		existing.payload_sha256,
		candidate.event_type,
		candidate.payload_sha256,
	);
}

fn discarded_post_archive_protocol_event(mut event: ProtocolEventRecord) -> ProtocolEventRecord {
	if event.event_type == DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE {
		return event;
	}

	event.sequence_number = discarded_post_archive_protocol_sequence(&event);
	event.event_type = DISCARDED_POST_ARCHIVE_PROTOCOL_EVENT_TYPE.to_owned();

	event
}

fn discarded_post_archive_protocol_sequence(event: &ProtocolEventRecord) -> i64 {
	let payload =
		format!("{}\n{}\n{}", event.sequence_number, event.event_type, event.payload_sha256);
	let digest = Sha256::digest(payload.as_bytes());
	let mut bytes = [0_u8; 8];

	bytes.copy_from_slice(&digest[..8]);

	let slot = i64::from_be_bytes(bytes) & i64::MAX;

	if slot == i64::MAX { i64::MIN } else { -1 - slot }
}
