use crate::{
	agent::app_server::{RunRecorder, markers},
	prelude::Result,
	state::ProtocolActivityMarker,
};

impl RunRecorder<'_> {
	pub(crate) fn record(&mut self, event_type: &str, payload: &str) -> Result<()> {
		self.state_store.append_event(self.run_id, self.next_sequence, event_type, payload)?;

		let child_activity = self.child_activity.record(event_type, payload);
		let protocol_activity = self.protocol_activity.record(event_type, payload, &child_activity);

		self.state_store.record_run_activity_summary(
			self.run_id,
			self.attempt_number,
			Some(&child_activity),
			Some(&protocol_activity),
		)?;

		if let Some(marker_path) = self.activity_marker_path {
			let activity = ProtocolActivityMarker {
				run_id: self.run_id,
				attempt_number: self.attempt_number,
				thread_id: self.thread_id.as_deref(),
				turn_id: self.turn_id.as_deref(),
				event_count: self.next_sequence,
				last_event_type: event_type,
				child_agent_activity: Some(&child_activity),
				protocol_activity: Some(&protocol_activity),
			};

			markers::write_protocol_activity_marker_best_effort(marker_path, &activity);
		}

		self.next_sequence += 1;

		Ok(())
	}
}
