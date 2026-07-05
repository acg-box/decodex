use crate::agent::app_server::ChildActivityAccumulator;

impl ChildActivityAccumulator {
	pub(in crate::agent::app_server::activity::child::accumulator) fn set_current(
		&mut self,
		bucket: Option<String>,
		detail: Option<String>,
		started_unix_epoch: Option<i64>,
	) {
		if self.current_bucket == bucket && self.current_detail == detail {
			return;
		}

		self.current_bucket = bucket.clone();
		self.current_detail = detail.clone();
		self.summary.current_bucket = bucket;
		self.summary.current_detail = detail;
		self.summary.current_started_unix_epoch = started_unix_epoch;
		self.summary.current_elapsed_seconds = None;
	}
}
