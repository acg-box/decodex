use serde_json::{self, Value};

use crate::agent::tracker_tool_bridge::{
	DynamicToolCallResponse, ProgressCheckpointArgs, TrackerToolBridge,
};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge::tools) fn handle_progress_checkpoint(
		&self,
		arguments: Value,
	) -> DynamicToolCallResponse {
		let parsed = match serde_json::from_value::<ProgressCheckpointArgs>(arguments) {
			Ok(parsed) => parsed,
			Err(error) => {
				return DynamicToolCallResponse::failure(format!(
					"Invalid `issue.progress_checkpoint` arguments: {error}"
				));
			},
		};

		if let Err(error) = self.ensure_issue_scope(&parsed.scope) {
			return DynamicToolCallResponse::failure(error);
		}

		let checkpoint = match self.normalize_progress_checkpoint(parsed) {
			Ok(checkpoint) => checkpoint,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};
		let (review_context, state_store) = match self.progress_checkpoint_context() {
			Ok(context) => context,
			Err(error) => return DynamicToolCallResponse::failure(error),
		};

		if let Err(error) =
			self.append_private_progress_checkpoint(review_context, state_store, &checkpoint)
		{
			return DynamicToolCallResponse::failure(error);
		}

		let public_projection =
			self.render_progress_checkpoint_projection(review_context, &checkpoint);

		match self.publish_progress_checkpoint_projection(state_store, &public_projection) {
			Ok(true) => DynamicToolCallResponse::success(format!(
				"Recorded private `{}` execution state for issue `{}` and published the public Linear projection.",
				checkpoint.phase.as_str(),
				self.issue.identifier
			)),
			Ok(false) => DynamicToolCallResponse::success(format!(
				"Recorded private `{}` execution state for issue `{}`; public Linear projection is unchanged.",
				checkpoint.phase.as_str(),
				self.issue.identifier
			)),
			Err(error) => DynamicToolCallResponse::failure(error),
		}
	}
}
