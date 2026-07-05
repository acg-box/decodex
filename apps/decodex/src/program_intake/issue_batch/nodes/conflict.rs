use std::collections::BTreeSet;

use crate::{
	execution_program::{ExecutionConflictDomain, ExecutionConflictDomainKind},
	prelude::Result,
	tracker::TrackerIssue,
};

pub(in crate::program_intake) fn issue_conflict_domains(
	issue: &TrackerIssue,
) -> Result<Vec<ExecutionConflictDomain>> {
	let mut domains = vec![ExecutionConflictDomain::new(
		ExecutionConflictDomainKind::TrackerOwnership,
		issue.identifier.clone(),
	)?];
	let mut seen = BTreeSet::from([format!(
		"{}:{}",
		ExecutionConflictDomainKind::TrackerOwnership.as_str(),
		issue.identifier
	)]);

	for label in &issue.labels {
		if let Some(module) = label.name.strip_prefix("repo:")
			&& !module.trim().is_empty()
		{
			insert_repo_module_domain(module, &mut seen, &mut domains)?;
		}
	}

	domains.sort_by(|left, right| {
		left.kind().as_str().cmp(right.kind().as_str()).then_with(|| left.key().cmp(right.key()))
	});

	Ok(domains)
}

fn insert_repo_module_domain(
	module: &str,
	seen: &mut BTreeSet<String>,
	domains: &mut Vec<ExecutionConflictDomain>,
) -> Result<()> {
	let key = module.trim().to_owned();
	let seen_key = format!("{}:{key}", ExecutionConflictDomainKind::Module.as_str());

	if seen.insert(seen_key) {
		domains.push(ExecutionConflictDomain::new(ExecutionConflictDomainKind::Module, key)?);
	}

	Ok(())
}
