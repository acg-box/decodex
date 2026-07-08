mod parse;
mod public_projection;
mod render;
mod types;
mod validation;

#[cfg(test)]
pub(crate) use self::types::{
	CLOSEOUT_RECORD_TYPE, CloseoutRecord, REVIEW_HANDOFF_RECORD_TYPE, ReviewHandoffRecord,
};
pub(crate) use self::{
	parse::{has_linear_execution_event_record, parse_linear_execution_event_record},
	public_projection::{
		PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_ACTION, PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_COMMENT_BODY,
		PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_DETAIL, PRIVACY_CLASSIFIER_WITHHELD_PUBLIC_SUMMARY,
		linear_execution_event_public_projection,
	},
	render::{
		append_structured_comment_record, render_linear_execution_event_comment_body,
		render_progress_checkpoint_public_projection, stable_event_anchor,
	},
	types::{
		LINEAR_EXECUTION_EVENT_RECORD_TYPE, LINEAR_EXECUTION_EVENT_RECORD_VERSION,
		LinearExecutionEventIdentity, LinearExecutionEventPublicProjection,
		LinearExecutionEventRecord,
	},
	validation::validate_linear_execution_event_record,
};

#[cfg(test)] mod tests;
