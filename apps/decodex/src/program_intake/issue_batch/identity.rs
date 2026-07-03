use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::{
	prelude::{Result, eyre},
	tracker::TrackerIssue,
};

pub(in crate::program_intake) fn normalize_issue_identifiers(
	issue_identifiers: Vec<String>,
) -> Result<Vec<String>> {
	let mut normalized = issue_identifiers
		.into_iter()
		.map(|identifier| identifier.trim().to_owned())
		.filter(|identifier| !identifier.is_empty())
		.collect::<Vec<_>>();

	normalized.sort();
	normalized.dedup();

	if normalized.is_empty() {
		eyre::bail!("Issue-batch intake requires at least one issue identifier.");
	}

	Ok(normalized)
}

pub(in crate::program_intake) fn issue_batch_fingerprint(
	service_id: &str,
	issue_identifiers: &[String],
	resolved: &BTreeMap<String, TrackerIssue>,
) -> String {
	let mut digest = Sha256::new();

	digest.update(service_id.as_bytes());

	for identifier in issue_identifiers {
		digest.update(b"\0identifier:");
		digest.update(identifier.as_bytes());

		if let Some(issue) = resolved.get(identifier) {
			digest.update(b"\0issue:");
			digest.update(issue.id.as_bytes());
			digest.update(b"\0state:");
			digest.update(issue.state.name.as_bytes());
			digest.update(b"\0updated:");
			digest.update(issue.updated_at.as_bytes());
		}
	}

	digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(in crate::program_intake) fn issue_batch_program_id(
	service_id: &str,
	fingerprint: &str,
) -> String {
	format!("issue-batch-{service_id}-{}", &fingerprint[..16])
}

pub(in crate::program_intake) fn node_id_for_issue(identifier: &str) -> String {
	format!("issue:{identifier}")
}
