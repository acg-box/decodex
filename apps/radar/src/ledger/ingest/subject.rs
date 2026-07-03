use crate::{
	ledger::{self, Value, eyre},
	prelude::Result,
};

pub(super) fn subject_for_bundle(bundle: &Value) -> Result<(String, String, String)> {
	let bundle = ledger::object_value(bundle, "bundle")?;
	let repo = ledger::required_string(bundle, "repo", "repo")?.to_owned();

	if let Some(number) = bundle
		.get("primary_pr")
		.and_then(Value::as_object)
		.and_then(|primary_pr| primary_pr.get("number"))
		.and_then(Value::as_u64)
	{
		return Ok((repo, "pr".into(), number.to_string()));
	}

	let commits = ledger::non_empty_array(bundle.get("commits"))
		.ok_or_else(|| eyre::eyre!("commits must be a non-empty list"))?;
	let first_commit = ledger::object_value(&commits[0], "commits[0]")?;
	let sha = ledger::required_string(first_commit, "sha", "commits[0].sha")?;

	Ok((repo, "commit".into(), sha.to_owned()))
}
