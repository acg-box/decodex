use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::branch::{gh_delete_ref_missing_branch, github_api_ref_path};
use crate::{
	github,
	lane_authority::EffectReceipt,
	prelude::{Result, eyre},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RemoteRefDeleteReadback {
	AlreadyAbsent(EffectReceipt),
	ConditionalMutationUnsupported { facts_fingerprint: String },
	PrerequisiteDrift { facts_fingerprint: String },
}

#[derive(Debug, Deserialize)]
struct GitReferenceResponse {
	#[serde(rename = "ref")]
	ref_name: String,
	object: GitReferenceObject,
}

#[derive(Debug, Deserialize)]
struct GitReferenceObject {
	sha: String,
	#[serde(rename = "type")]
	object_type: String,
}

#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn reconcile_remote_ref_delete(
	cwd: &Path,
	owner: &str,
	repository: &str,
	branch_name: &str,
	expected_oid: &str,
	request_digest: &str,
	observed_at: &str,
	observed_at_unix: i64,
	github_token: &str,
	gh_command_path: Option<&Path>,
) -> Result<RemoteRefDeleteReadback> {
	if [owner, repository, branch_name, expected_oid, request_digest]
		.iter()
		.any(|value| value.trim().is_empty())
	{
		eyre::bail!("Remote ref cleanup requires complete immutable identity.");
	}
	let expected_ref = format!("refs/heads/{branch_name}");
	let endpoint =
		format!("repos/{owner}/{repository}/git/ref/heads/{}", github_api_ref_path(branch_name));
	let mut command = github::gh_command_with_config(gh_command_path);
	command.args(["api", "--method", "GET", endpoint.as_str()]);
	command.current_dir(cwd);
	github::configure_gh_command(&mut command, github_token);
	let output = command.output()?;
	if !output.status.success() {
		if gh_delete_ref_missing_branch(&output) {
			let facts = ref_fingerprint(&expected_ref, expected_oid, "absent");
			return Ok(RemoteRefDeleteReadback::AlreadyAbsent(EffectReceipt::new(
				&format!("github-ref-absent:{owner}/{repository}:{branch_name}"),
				request_digest,
				&facts,
				None,
				Some(&facts),
				observed_at,
				observed_at_unix,
			)?));
		}
		eyre::bail!("GitHub remote ref readback failed.");
	}
	let response = serde_json::from_slice::<GitReferenceResponse>(&output.stdout)?;
	let facts =
		ref_fingerprint(&response.ref_name, &response.object.sha, &response.object.object_type);
	if response.ref_name != expected_ref
		|| response.object.sha != expected_oid
		|| response.object.object_type != "commit"
	{
		return Ok(RemoteRefDeleteReadback::PrerequisiteDrift { facts_fingerprint: facts });
	}
	Ok(RemoteRefDeleteReadback::ConditionalMutationUnsupported { facts_fingerprint: facts })
}

fn ref_fingerprint(ref_name: &str, oid: &str, object_type: &str) -> String {
	let digest = Sha256::digest(
		serde_json::to_vec(&serde_json::json!({
			"ref": ref_name,
			"oid": oid,
			"object_type": object_type,
		}))
		.expect("ref facts serialize"),
	);
	format!("sha256:{}", digest.iter().map(|byte| format!("{byte:02x}")).collect::<String>())
}
