use std::collections::BTreeSet;

use crate::{
	loop_contract::DecisionProposedIssue,
	prelude::{Result, eyre},
};

pub(super) fn default_research_evidence_kind() -> String {
	String::from("unspecified")
}

pub(super) fn validate_required(name: &str, value: &str) -> Result<()> {
	if value.trim().is_empty() {
		eyre::bail!("{name} must not be empty.");
	}

	Ok(())
}

pub(super) fn validate_optional(name: &str, value: Option<&str>) -> Result<()> {
	if let Some(value) = value {
		validate_required(name, value)?;
	}

	Ok(())
}

pub(super) fn validate_string_list(name: &str, values: &[String]) -> Result<()> {
	for value in values {
		validate_required(name, value)?;
	}

	Ok(())
}

pub(super) fn validate_proposed_issues(issues: &[DecisionProposedIssue]) -> Result<()> {
	let mut keys = BTreeSet::new();

	for issue in issues {
		issue.validate()?;

		if !keys.insert(issue.key().to_owned()) {
			eyre::bail!("Decision Contract proposed issue key `{}` is duplicated.", issue.key());
		}
	}
	for issue in issues {
		for dependency in issue.dependencies() {
			if dependency == issue.key() {
				eyre::bail!(
					"Decision Contract proposed issue `{}` must not depend on itself.",
					issue.key()
				);
			}
			if !keys.contains(dependency) {
				eyre::bail!(
					"Decision Contract proposed issue `{}` depends on unknown issue `{dependency}`.",
					issue.key()
				);
			}
		}
	}

	let mut visiting = BTreeSet::new();
	let mut visited = BTreeSet::new();

	for issue in issues {
		validate_proposed_issue_acyclic(issue.key(), issues, &mut visiting, &mut visited)?;
	}

	Ok(())
}

pub(super) fn validate_proposed_issue_stage(key: &str, stage: &str) -> Result<()> {
	match stage {
		"research" | "design" | "spec" | "schema" | "runtime" | "plugin" | "eval" | "handoff" =>
			Ok(()),
		_ => {
			eyre::bail!("Decision Contract proposed issue `{key}` has unsupported stage `{stage}`.")
		},
	}
}

pub(super) fn validate_proposed_issue_queue_intent(key: &str, queue_intent: &str) -> Result<()> {
	match queue_intent {
		"not_ready" | "ready_to_queue" | "queued" | "active" | "paused" | "done" | "canceled" =>
			Ok(()),
		_ => eyre::bail!(
			"Decision Contract proposed issue `{key}` has unsupported queue_intent `{queue_intent}`."
		),
	}
}

pub(super) fn normalized_link_values(
	values: impl IntoIterator<Item = impl Into<String>>,
) -> Result<Vec<String>> {
	let mut normalized = Vec::new();

	for value in values {
		let value = value.into();
		let value = value.trim();

		validate_required("decision contract generated link", value)?;

		if !normalized.iter().any(|existing| existing == value) {
			normalized.push(value.to_owned());
		}
	}

	Ok(normalized)
}

fn validate_proposed_issue_acyclic(
	key: &str,
	issues: &[DecisionProposedIssue],
	visiting: &mut BTreeSet<String>,
	visited: &mut BTreeSet<String>,
) -> Result<()> {
	if visited.contains(key) {
		return Ok(());
	}
	if !visiting.insert(key.to_owned()) {
		eyre::bail!("Decision Contract proposed issue dependency cycle includes `{key}`.");
	}

	let issue = issues
		.iter()
		.find(|issue| issue.key() == key)
		.ok_or_else(|| eyre::eyre!("Decision Contract proposed issue `{key}` does not exist."))?;

	for dependency in issue.dependencies() {
		validate_proposed_issue_acyclic(dependency, issues, visiting, visited)?;
	}

	visiting.remove(key);
	visited.insert(key.to_owned());

	Ok(())
}
