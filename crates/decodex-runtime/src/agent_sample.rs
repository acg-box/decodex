//! Bounded in-memory sample results; native execution owns the private child and deadline.
use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex},
};

use decodex_codex::app_server_client::{ClientError, HistoryGuard};
use decodex_database::AgentVoiceHistoryRevision;
use decodex_protocol::{
	AgentSampleAnswer, AgentSamplePhase, AgentSampleStatus, EntityId, WireText,
};

use crate::{agent_recap::same_owner, agent_usage_estimate::Source};

#[derive(Clone, Default)]
pub(crate) struct Samples(Arc<Mutex<BTreeMap<String, Record>>>);

struct Record {
	source: Source,
	guard: HistoryGuard,
	voice: AgentVoiceHistoryRevision,
	status: AgentSampleStatus,
}

impl Samples {
	pub(crate) fn start(
		&self,
		source: Source,
		key: &str,
		prompt: &str,
		voice: AgentVoiceHistoryRevision,
	) -> Result<(), &'static str> {
		let guard = source
			.client
			.history_guard(source.key.history_revision)
			.and_then(|guard| source.client.with_thread_settings_guard(&source.key.thread, guard))
			.ok_or("The conversation changed. Refresh before sampling.")?;
		let mut state = self.0.lock().map_err(|_| "Sampling is unavailable")?;
		if state.get(&source.key.work).is_some_and(|r| r.status.phase == AgentSamplePhase::Pending)
		{
			return Err("A sample is already running for this task");
		}
		if state.len() >= 32 && !state.contains_key(&source.key.work) {
			let oldest = state
				.iter()
				.find(|(_, r)| r.status.phase != AgentSamplePhase::Pending)
				.map(|(k, _)| k.clone())
				.ok_or("Sampling capacity is full")?;
			state.remove(&oldest);
		}
		let status = AgentSampleStatus {
			work_id: EntityId::new(source.key.work.clone()).map_err(|_| "Invalid task")?,
			thread_id: Some(
				WireText::new(source.key.thread.clone()).map_err(|_| "Invalid native thread")?,
			),
			request_id: Some(WireText::new(key).map_err(|_| "Invalid request")?),
			phase: AgentSamplePhase::Pending,
			prompt: Some(WireText::new(prompt).map_err(|_| "Question too long")?),
			answer: None,
		};
		state.insert(source.key.work.clone(), Record { source, guard, voice, status });
		Ok(())
	}

	pub(crate) fn status(
		&self,
		work: EntityId,
		source: Option<&Source>,
		voice: Option<&AgentVoiceHistoryRevision>,
	) -> AgentSampleStatus {
		let Ok(state) = self.0.lock() else { return AgentSampleStatus::idle(work) };
		let Some(record) = state.get(work.as_str()) else { return AgentSampleStatus::idle(work) };
		let mut status = record.status.clone();
		if voice != Some(&record.voice)
			|| !record.guard.is_live()
			|| source.is_none_or(|source| !same_owner(&record.source, source))
		{
			status.phase = AgentSamplePhase::Obsolete;
			status.answer = None;
		}
		status
	}

	pub(crate) fn finish(
		&self,
		source: &Source,
		key: &str,
		result: Result<AgentSampleAnswer, ClientError>,
	) {
		let Ok(mut state) = self.0.lock() else { return };
		let Some(record) = state.get_mut(&source.key.work) else { return };
		if record.status.request_id.as_ref().is_none_or(|id| id.as_str() != key) {
			return;
		}
		if !record.guard.is_live() {
			record.status.phase = AgentSamplePhase::Obsolete;
		} else {
			match result {
				Ok(answer) if answer.is_valid() => {
					record.status.phase = if answer.text.is_some() {
						AgentSamplePhase::Ready
					} else {
						AgentSamplePhase::NoText
					};
					record.status.answer = Some(answer);
				},
				Err(ClientError::StaleHistory | ClientError::Closed) =>
					record.status.phase = AgentSamplePhase::Obsolete,
				Err(ClientError::Io) => record.status.phase = AgentSamplePhase::Unconfirmed,
				Err(ClientError::Remote(error)) if error.code == -32_603 =>
					record.status.phase = AgentSamplePhase::Unconfirmed,
				Err(ClientError::Remote(error)) if error.code == -32_601 =>
					record.status.phase = AgentSamplePhase::Unsupported,
				_ => record.status.phase = AgentSamplePhase::Failed,
			}
		}
	}
}

#[cfg(test)]
#[path = "agent_sample_tests.rs"]
mod tests;
