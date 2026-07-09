//! Service configuration for Decodex.

pub(in crate::config) mod path_resolution;

mod autonomy;
mod codex;
mod document;
mod git_paths;
mod github;
mod paths;
mod privacy;
mod review;
mod service;
mod tracker;
mod validation;

pub use self::{
	autonomy::{ProjectAutonomyConfig, ProjectAutonomyRuntimePolicyConfig},
	codex::{ProjectCodexAccountsConfig, ProjectCodexConfig},
	github::{FAST_LANDING_STATUS_CONTEXT, ProjectGitHubConfig, ProjectGitHubLandingMode},
	paths::ProjectPathsConfig,
	privacy::ProjectPrivacyClassifierConfig,
	review::ReviewLevel,
	service::ServiceConfig,
	tracker::ProjectTrackerConfig,
};
pub use git_paths::{
	canonical_repo_root_for_checkout, checkouts_share_repository, git_dir_for_checkout,
};

#[cfg(test)] use git_paths::path_buf_from_git_line_output;

#[cfg(test)] mod tests;
