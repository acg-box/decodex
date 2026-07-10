mod blockers;
mod comments;
mod lookup;
mod relations;
mod search;

pub(in crate::tracker::linear) use self::{
	blockers::ISSUE_BLOCKERS_QUERY,
	comments::ISSUE_COMMENTS_QUERY,
	lookup::{ISSUE_BY_IDENTIFIER_QUERY, ISSUES_BY_IDS_QUERY},
	relations::{ISSUE_INVERSE_RELATIONS_QUERY, ISSUE_RELATIONS_QUERY},
	search::ISSUES_WITH_LABEL_QUERY,
};
