mod build;
mod model;
mod normalize;
mod parse;

pub(crate) use self::{
	build::{
		build_commit_message, build_landed_merge_commit_message, build_landing_commit_message,
		validate_commit_message_subject,
	},
	model::MANUAL_AUTHORITY,
	normalize::{looks_like_issue_identifier, normalize_issue_identifier},
};

#[cfg(test)] mod tests;
