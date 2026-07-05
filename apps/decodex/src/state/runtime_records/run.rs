use std::path::PathBuf;

use serde_json::Value;

use crate::{
	state::{
		ChildAgentActivitySummary, PrivateExecutionEvent, ProtocolActivitySummary, RunAttempt,
		RunControlChannel,
	},
	tracker::records::LinearExecutionEventRecord,
};

pub(in crate::state) struct TimestampParts {
	pub(in crate::state) text: String,
	pub(in crate::state) unix: i64,
}

#[derive(Clone, Debug)]
pub(in crate::state) struct RunAttemptRecord {
	pub(in crate::state) run_id: String,
	pub(in crate::state) project_id: Option<String>,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) status: String,
	pub(in crate::state) thread_id: Option<String>,
	pub(in crate::state) turn_id: Option<String>,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl RunAttemptRecord {
	pub(in crate::state) fn as_public(&self) -> RunAttempt {
		RunAttempt {
			run_id: self.run_id.clone(),
			issue_id: self.issue_id.clone(),
			attempt_number: self.attempt_number,
			status: self.status.clone(),
			thread_id: self.thread_id.clone(),
			turn_id: self.turn_id.clone(),
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct RunControlChannelRecord {
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) transport: String,
	pub(in crate::state) channel_path: PathBuf,
	pub(in crate::state) status: String,
	pub(in crate::state) published_at: String,
	pub(in crate::state) published_at_unix: i64,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}
impl RunControlChannelRecord {
	pub(in crate::state) fn as_public(&self) -> RunControlChannel {
		RunControlChannel {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			transport: self.transport.clone(),
			channel_path: self.channel_path.clone(),
			status: self.status.clone(),
			published_at: self.published_at.clone(),
			published_at_unix: self.published_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct ProtocolEventRecord {
	pub(in crate::state) sequence_number: i64,
	pub(in crate::state) event_type: String,
	pub(in crate::state) payload_sha256: String,
	pub(in crate::state) created_at: String,
	pub(in crate::state) created_at_unix: i64,
}
impl ProtocolEventRecord {
	pub(in crate::state) fn is_idempotent_replay_of(&self, candidate: &Self) -> bool {
		self.event_type == candidate.event_type && self.payload_sha256 == candidate.payload_sha256
	}
}

#[derive(Clone, Debug, Default)]
pub(in crate::state) struct ProtocolEventSummaryRecord {
	pub(in crate::state) event_count: i64,
	pub(in crate::state) last_sequence_number: Option<i64>,
	pub(in crate::state) last_event_type: Option<String>,
	pub(in crate::state) last_event_at: Option<String>,
	pub(in crate::state) last_event_at_unix: Option<i64>,
}
impl ProtocolEventSummaryRecord {
	pub(in crate::state) fn record_event(&mut self, event: &ProtocolEventRecord) {
		self.event_count += 1;

		if self
			.last_sequence_number
			.is_none_or(|sequence_number| event.sequence_number >= sequence_number)
		{
			self.last_sequence_number = Some(event.sequence_number);
			self.last_event_type = Some(event.event_type.clone());
			self.last_event_at = Some(event.created_at.clone());
			self.last_event_at_unix = Some(event.created_at_unix);
		}
	}
}

#[derive(Clone, Debug)]
pub(in crate::state) struct RunActivitySummaryRecord {
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) child_agent_activity: Option<ChildAgentActivitySummary>,
	pub(in crate::state) protocol_activity: Option<ProtocolActivitySummary>,
	pub(in crate::state) updated_at: String,
	pub(in crate::state) updated_at_unix: i64,
}

#[derive(Clone, Debug)]
pub(in crate::state) struct LinearExecutionEventRuntimeRecord {
	pub(in crate::state) record: LinearExecutionEventRecord,
	pub(in crate::state) event_unix: Option<i64>,
	pub(in crate::state) recorded_at: String,
	pub(in crate::state) recorded_at_unix: i64,
}

#[derive(Clone, Debug)]
pub(in crate::state) struct PrivateExecutionEventRuntimeRecord {
	pub(in crate::state) record_id: i64,
	pub(in crate::state) project_id: String,
	pub(in crate::state) issue_id: String,
	pub(in crate::state) run_id: String,
	pub(in crate::state) attempt_number: i64,
	pub(in crate::state) event_type: String,
	pub(in crate::state) payload: Value,
	pub(in crate::state) recorded_at: String,
	pub(in crate::state) recorded_at_unix: i64,
}
impl PrivateExecutionEventRuntimeRecord {
	pub(in crate::state) fn as_public(&self) -> PrivateExecutionEvent {
		PrivateExecutionEvent {
			record_id: self.record_id,
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			event_type: self.event_type.clone(),
			payload: self.payload.clone(),
			recorded_at: self.recorded_at.clone(),
			recorded_at_unix: self.recorded_at_unix,
		}
	}
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::state) enum GuardRetention {
	Local,
	ParentAfterHandoff,
	AdoptingChild,
}
