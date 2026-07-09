use clap::{Args, Subcommand, ValueEnum};
use color_eyre::eyre;

use crate::{
	cli::ProjectConfigArgs,
	config::ServiceConfig,
	github::{self, CommitStatusPublishRequest, CommitStatusState},
	prelude::Result,
};

#[derive(Debug, Args)]
pub(super) struct VerifyCommand {
	#[command(subcommand)]
	command: VerifySubcommand,
}
impl VerifyCommand {
	pub(super) fn run(&self) -> Result<()> {
		match &self.command {
			VerifySubcommand::PublishStatus(args) => args.run(),
		}
	}
}

#[derive(Debug, Subcommand)]
enum VerifySubcommand {
	/// Publish a GitHub commit status for the current PR head.
	PublishStatus(PublishStatusCommand),
}

#[derive(Debug, Args)]
struct PublishStatusCommand {
	#[command(flatten)]
	project_config: ProjectConfigArgs,
	/// Pull request URL whose current head receives the status.
	#[arg(long, value_name = "URL")]
	pr: String,
	/// Commit status context to publish.
	#[arg(long, default_value = "decodex/local-full-check")]
	context: String,
	/// Commit status state to publish.
	#[arg(long, value_enum)]
	state: PublishStatusState,
	/// Expected PR head SHA. Required when publishing success.
	#[arg(long, value_name = "SHA", required_if_eq("state", "success"))]
	expected_head: Option<String>,
	/// Expected PR base branch name. Required when publishing success.
	#[arg(long, value_name = "BRANCH", required_if_eq("state", "success"))]
	expected_base_ref: Option<String>,
	/// Expected current PR base tip SHA. Required when publishing success.
	#[arg(long, value_name = "SHA", required_if_eq("state", "success"))]
	expected_base_oid: Option<String>,
	/// Short GitHub status description.
	#[arg(long)]
	description: Option<String>,
	/// Detail URL for the status.
	#[arg(long)]
	target_url: Option<String>,
}
impl PublishStatusCommand {
	fn run(&self) -> Result<()> {
		let config_path = self
			.project_config
			.as_path()
			.ok_or_else(|| eyre::eyre!("`decodex verify publish-status` requires `--config`."))?;
		let config = ServiceConfig::from_path(config_path)?;
		let github_token = config.github().resolve_token()?;
		let landing_state = github::inspect_pull_request_landing_state(
			config.repo_root(),
			&self.pr,
			&github_token,
			config.github().command_path(),
			&[],
			&[],
		)?;

		if self.state == PublishStatusState::Success && self.expected_head.is_none() {
			color_eyre::eyre::bail!(
				"`decodex verify publish-status --state success` requires `--expected-head`."
			);
		}
		if self.state == PublishStatusState::Success && self.expected_base_ref.is_none() {
			color_eyre::eyre::bail!(
				"`decodex verify publish-status --state success` requires `--expected-base-ref`."
			);
		}
		if self.state == PublishStatusState::Success && self.expected_base_oid.is_none() {
			color_eyre::eyre::bail!(
				"`decodex verify publish-status --state success` requires `--expected-base-oid`."
			);
		}

		if let Some(expected_head) = self.expected_head.as_deref()
			&& landing_state.head_ref_oid != expected_head
		{
			color_eyre::eyre::bail!(
				"PR head changed before status publish: expected `{}`, found `{}`.",
				expected_head,
				landing_state.head_ref_oid
			);
		}
		if let Some(expected_base_ref) = self.expected_base_ref.as_deref()
			&& landing_state.base_ref_name != expected_base_ref
		{
			color_eyre::eyre::bail!(
				"PR base changed before status publish: expected `{}`, found `{}`.",
				expected_base_ref,
				landing_state.base_ref_name
			);
		}
		if let Some(expected_base_oid) = self.expected_base_oid.as_deref()
			&& landing_state.base_ref_oid.as_deref() != Some(expected_base_oid)
		{
			color_eyre::eyre::bail!(
				"PR base changed before status publish: expected `{}`, found `{}`.",
				expected_base_oid,
				landing_state.base_ref_oid.as_deref().unwrap_or("<unknown>")
			);
		}

		let locator = github::parse_pull_request_url(&self.pr)?;
		let description = if self.state == PublishStatusState::Success {
			Some(github::commit_status_description_with_base_ref_oid(
				self.description.as_deref(),
				landing_state.base_ref_oid.as_deref().ok_or_else(|| {
					eyre::eyre!("GitHub did not return a PR base SHA; refusing to publish success.")
				})?,
			))
		} else {
			self.description.clone()
		};

		github::publish_commit_status(CommitStatusPublishRequest {
			cwd: config.repo_root(),
			owner: &locator.owner,
			repo: &locator.repo,
			sha: &landing_state.head_ref_oid,
			context: &self.context,
			state: self.state.into(),
			description: description.as_deref(),
			target_url: self.target_url.as_deref(),
			github_token: &github_token,
			gh_command_path: config.github().command_path(),
		})?;

		println!(
			"status ok: pr={} head={} context={} state={}",
			self.pr,
			landing_state.head_ref_oid,
			self.context,
			CommitStatusState::from(self.state).as_str()
		);

		Ok(())
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum PublishStatusState {
	Error,
	Failure,
	Pending,
	Success,
}
impl From<PublishStatusState> for CommitStatusState {
	fn from(value: PublishStatusState) -> Self {
		match value {
			PublishStatusState::Error => Self::Error,
			PublishStatusState::Failure => Self::Failure,
			PublishStatusState::Pending => Self::Pending,
			PublishStatusState::Success => Self::Success,
		}
	}
}
