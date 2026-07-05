mod external;
mod non_github;
mod pre;

pub(crate) use self::{
	external::apply_review_orchestration_phase_classification,
	non_github::apply_non_github_review_post_review_classification,
	pre::apply_pre_orchestration_post_review_classification,
};
