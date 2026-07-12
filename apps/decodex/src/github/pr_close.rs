use std::{path::Path, process::Command};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	github,
	lane_authority::EffectReceipt,
	prelude::{Result, eyre},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PullRequestCloseReadback {
	AlreadyClosed(EffectReceipt),
	ConditionalMutationUnsupported { facts_fingerprint: String },
	PrerequisiteDrift { facts_fingerprint: String },
}

#[derive(Debug, Deserialize)]
struct PullRequestResponse {
	id: i64,
	number: u64,
	state: String,
	updated_at: String,
	head: PullRequestHead,
	base: PullRequestBase,
}

#[derive(Debug, Deserialize)]
struct PullRequestHead {
	sha: String,
}

#[derive(Debug, Deserialize)]
struct PullRequestBase {
	#[serde(rename = "ref")]
	ref_name: String,
}

#[allow(dead_code)]
pub(crate) fn reconcile_pull_request_close(
	cwd: &Path,
	pr_url: &str,
	expected_head_oid: &str,
	expected_base_ref: &str,
	request_digest: &str,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<PullRequestCloseReadback> {
	let locator = github::parse_pull_request_url(pr_url)?;
	let endpoint = format!("repos/{}/{}/pulls/{}", locator.owner, locator.repo, locator.number);
	let mut command = github::gh_command_with_config(gh_command_path);
	configure_pull_request_readback_command(&mut command, &endpoint);
	command.current_dir(cwd);
	github::configure_gh_command(&mut command, github_token);
	let output = command.output()?;
	if !output.status.success() {
		eyre::bail!("GitHub pull request close readback failed.");
	}
	let response = serde_json::from_slice::<PullRequestResponse>(&output.stdout)?;
	if response.number != locator.number {
		eyre::bail!("GitHub pull request close readback returned the wrong pull request.");
	}
	let facts_fingerprint = close_facts_fingerprint(&response);
	if response.head.sha != expected_head_oid || response.base.ref_name != expected_base_ref {
		return Ok(PullRequestCloseReadback::PrerequisiteDrift { facts_fingerprint });
	}
	if response.state.eq_ignore_ascii_case("closed") {
		let observed_at = OffsetDateTime::parse(&response.updated_at, &Rfc3339)?;
		return Ok(PullRequestCloseReadback::AlreadyClosed(EffectReceipt::new(
			&format!("github-pr-close:{}:{}", response.id, response.updated_at),
			request_digest,
			&facts_fingerprint,
			Some(&response.id.to_string()),
			Some(&facts_fingerprint),
			&response.updated_at,
			observed_at.unix_timestamp(),
		)?));
	}
	if response.state.eq_ignore_ascii_case("open") {
		return Ok(PullRequestCloseReadback::ConditionalMutationUnsupported { facts_fingerprint });
	}
	Ok(PullRequestCloseReadback::PrerequisiteDrift { facts_fingerprint })
}

pub(in crate::github) fn configure_pull_request_readback_command(
	command: &mut Command,
	endpoint: &str,
) {
	command.args(["api", "--method", "GET", endpoint]);
}

fn close_facts_fingerprint(response: &PullRequestResponse) -> String {
	let digest = Sha256::digest(
		serde_json::to_vec(&serde_json::json!({
			"id": response.id,
			"number": response.number,
			"state": response.state.to_ascii_lowercase(),
			"head_oid": response.head.sha,
			"base_ref": response.base.ref_name,
			"updated_at": response.updated_at,
		}))
		.expect("pull request facts serialize"),
	);
	format!("sha256:{}", digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
}
