use crate::{agent::tracker_tool_bridge::TrackerToolBridge, tracker};

impl<'a> TrackerToolBridge<'a> {
	pub(super) fn apply_manual_attention_label(&self) -> Result<(), String> {
		let label = self.workflow.frontmatter().tracker().needs_attention_label();
		let current_issue = match self.refreshed_issue_snapshot() {
			Ok(Some(issue)) => issue,
			Ok(None) => {
				return Err(format!(
					"Failed to refresh issue `{}` before applying manual-attention label `{label}`: tracker returned no current snapshot.",
					self.issue.identifier
				));
			},
			Err(error) => {
				return Err(format!(
					"Failed to refresh issue `{}` before applying manual-attention label `{label}`: {error}",
					self.issue.identifier
				));
			},
		};

		tracker::set_issue_label_presence(self.tracker, &current_issue, label, true).map_err(
			|error| {
				format!(
					"Failed to add label `{label}` to issue `{}`: {error}",
					self.issue.identifier
				)
			},
		)?;

		Ok(())
	}
}
