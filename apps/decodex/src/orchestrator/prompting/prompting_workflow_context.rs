use std::{fs, io::ErrorKind};

use crate::{
	config::ServiceConfig,
	prelude::{Result, eyre},
	tracker::TrackerIssue,
	workflow::WorkflowDocument,
};

pub(crate) fn review_pull_request_title(issue: &TrackerIssue) -> String {
	let title = issue.title.trim();
	let prefix = format!("{}:", issue.identifier);

	if let Some(candidate_prefix) = title.get(..prefix.len())
		&& candidate_prefix.eq_ignore_ascii_case(&prefix)
	{
		let summary = title.get(prefix.len()..).unwrap_or_default().trim();

		if summary.is_empty() {
			return issue.identifier.clone();
		}

		return format!("{prefix} {summary}");
	}

	format!("{prefix} {title}")
}

pub(crate) fn validate_workflow_read_first_files(
	project: &ServiceConfig,
	workflow: &WorkflowDocument,
) -> Result<()> {
	for relative_path in workflow.frontmatter().context().read_first() {
		read_workflow_read_first_file(project, relative_path)?;
	}

	Ok(())
}

pub(super) fn read_workflow_read_first_file(
	project: &ServiceConfig,
	relative_path: &str,
) -> Result<String> {
	let absolute_path = project.repo_root().join(relative_path);

	fs::read_to_string(&absolute_path).map_err(|error| {
		if error.kind() == ErrorKind::NotFound {
			return eyre::eyre!(
				"Project `{}` workflow `{}` references missing `context.read_first` file `{}` at `{}`. Update the path or restore the file before dispatch.",
				project.service_id(),
				project.workflow_path().display(),
				relative_path,
				absolute_path.display()
			);
		}

		eyre::eyre!(
			"Failed to read project `{}` workflow `{}` `context.read_first` file `{}` at `{}`: {error}",
			project.service_id(),
			project.workflow_path().display(),
			relative_path,
			absolute_path.display()
		)
	})
}
