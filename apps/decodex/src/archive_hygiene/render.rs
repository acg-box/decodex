//! Archive hygiene dry-run and execution output.

use super::ArchivePlan;

pub(super) fn print_archive_plan(
	plan: &ArchivePlan,
	repo_labels: &[String],
	updated_before: &str,
	execute: bool,
) {
	let mode = if execute { "execute" } else { "dry run" };

	println!("Linear tracker archive hygiene ({mode})");
	println!("Repo labels: {}", repo_labels.join(", "));
	println!("Updated before: {updated_before}");
	println!("Archive candidates: {}", plan.candidates.len());

	for candidate in &plan.candidates {
		println!(
			"- {} [{}] updated={} labels={} title={}",
			candidate.identifier,
			candidate.state,
			candidate.updated_at,
			candidate.repo_labels.join(","),
			candidate.title
		);
	}

	if !plan.skipped.is_empty() {
		println!("Skipped: {}", plan.skipped.len());

		for skipped in &plan.skipped {
			println!("- {}: {}", skipped.identifier, skipped.reason);
		}
	}
}
