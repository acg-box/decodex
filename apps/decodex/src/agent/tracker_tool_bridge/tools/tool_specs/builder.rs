use crate::agent::tracker_tool_bridge::{DynamicToolSpec, ReviewExecutionMode, TrackerToolBridge};

impl<'a> TrackerToolBridge<'a> {
	pub(in crate::agent::tracker_tool_bridge) fn build_tool_specs(&self) -> Vec<DynamicToolSpec> {
		let mut tool_specs = match self.review_context.as_ref().map(|context| context.mode) {
			Some(ReviewExecutionMode::Repair) => {
				let mut tool_specs = self.comment_tool_specs();

				tool_specs.extend(self.progress_checkpoint_tool_specs());

				tool_specs
			},
			Some(ReviewExecutionMode::Closeout) => self.closeout_base_tool_specs(),
			Some(ReviewExecutionMode::Handoff) => {
				let mut tool_specs = self.base_tool_specs();

				tool_specs.extend(self.review_handoff_tool_specs());

				tool_specs
			},
			None => self.base_tool_specs(),
		};

		if matches!(
			self.review_context.as_ref().map(|context| context.mode),
			Some(ReviewExecutionMode::Repair)
		) {
			tool_specs.extend(self.review_repair_tool_specs());
		}
		if matches!(
			self.review_context.as_ref().map(|context| context.mode),
			Some(ReviewExecutionMode::Closeout)
		) {
			tool_specs.extend(self.closeout_tool_specs());
		}

		tool_specs.push(self.label_add_tool_spec());

		tool_specs
	}
}
