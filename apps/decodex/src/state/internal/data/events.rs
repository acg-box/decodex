use crate::{
	prelude::{Result, eyre},
	state::{
		internal::data::StateData, runtime_records::ProtocolEventSummaryRecord, runtime_row_parsers,
	},
};

impl StateData {
	pub(in crate::state) fn protocol_event_summary(
		&self,
		run_id: &str,
	) -> ProtocolEventSummaryRecord {
		self.event_summaries
			.get(run_id)
			.cloned()
			.or_else(|| {
				self.events
					.get(run_id)
					.map(|events| runtime_row_parsers::protocol_event_summary_from_events(events))
			})
			.unwrap_or_default()
	}

	pub(in crate::state) fn next_private_execution_event_id(&self) -> Result<i64> {
		self.private_execution_events
			.iter()
			.map(|record| record.record_id)
			.max()
			.unwrap_or(0)
			.checked_add(1)
			.ok_or_else(|| eyre::eyre!("Private execution event row id overflowed i64."))
	}
}
