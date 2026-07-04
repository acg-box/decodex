use crate::state::{ChildAgentActivityBucket, ChildAgentActivitySummary};

impl ChildAgentActivitySummary {
	pub(crate) fn sealed_durable(mut self) -> Self {
		self.seal_open_interval();

		self
	}

	pub(crate) fn live_projection(mut self, now_unix_epoch: i64) -> Self {
		let observed_elapsed_seconds =
			self.current_elapsed_seconds.filter(|elapsed| *elapsed >= 0).unwrap_or(0);
		let current_elapsed_seconds = self.current_started_unix_epoch.and_then(|started_at| {
			now_unix_epoch.checked_sub(started_at).filter(|elapsed| *elapsed >= 0)
		});
		let open_delta_seconds = current_elapsed_seconds.and_then(|elapsed| {
			elapsed.checked_sub(observed_elapsed_seconds).filter(|delta| *delta > 0)
		});

		self.current_elapsed_seconds = current_elapsed_seconds;

		let current_bucket = self.current_bucket.clone();

		if let (Some(current_bucket), Some(open_delta_seconds)) =
			(current_bucket, open_delta_seconds)
		{
			let bucket = self.bucket_mut(&current_bucket);

			bucket.wall_seconds = bucket.wall_seconds.saturating_add(open_delta_seconds);
		}

		self
	}

	fn seal_open_interval(&mut self) {
		self.current_bucket = None;
		self.current_detail = None;
		self.current_started_unix_epoch = None;
		self.current_elapsed_seconds = None;
	}

	fn bucket_mut(&mut self, name: &str) -> &mut ChildAgentActivityBucket {
		if let Some(index) = self.buckets.iter().position(|bucket| bucket.name == name) {
			return &mut self.buckets[index];
		}

		self.buckets.push(ChildAgentActivityBucket {
			name: name.to_owned(),
			..ChildAgentActivityBucket::default()
		});

		let last_index = self.buckets.len().saturating_sub(1);

		&mut self.buckets[last_index]
	}
}
