use crate::orchestrator::OperatorRunStatus;

pub(crate) fn operator_run_group_key(run: &OperatorRunStatus) -> String {
	let issue_id = run.issue_id.trim();

	if !issue_id.is_empty() && !issue_id.eq_ignore_ascii_case("unknown") {
		return issue_id.to_ascii_uppercase();
	}

	operator_run_issue_key(run)
}

pub(crate) fn operator_run_issue_key(run: &OperatorRunStatus) -> String {
	if let Some(issue_identifier) = run
		.issue_identifier
		.as_ref()
		.filter(|value| !value.trim().is_empty() && !value.eq_ignore_ascii_case("unknown"))
	{
		return issue_identifier.clone();
	}
	if let Some(issue_identifier) = operator_run_issue_identifier_from_fields(
		&run.run_id,
		run.branch_name.as_deref(),
		run.worktree_path.as_deref(),
	) {
		return issue_identifier;
	}

	let issue_id = run.issue_id.trim();

	if issue_id.is_empty() { String::from("unknown") } else { issue_id.to_owned() }
}

pub(crate) fn operator_run_issue_identifier_from_fields(
	run_id: &str,
	branch_name: Option<&str>,
	worktree_path: Option<&str>,
) -> Option<String> {
	if let Some(issue_identifier) = issue_identifier_from_run_id(run_id) {
		return Some(issue_identifier);
	}

	for value in [branch_name, worktree_path] {
		if let Some(issue_identifier) = value.and_then(issue_identifier_in_text) {
			return Some(issue_identifier);
		}
	}

	None
}

pub(crate) fn issue_identifier_from_run_id(run_id: &str) -> Option<String> {
	if let Some((candidate, _attempt_suffix)) = run_id.split_once("-attempt-") {
		return issue_identifier_in_text(candidate);
	}
	if let Some(candidate) = run_id.strip_prefix("recovered-") {
		return issue_identifier_in_text(candidate);
	}

	None
}

pub(crate) fn issue_identifier_in_text(value: &str) -> Option<String> {
	let bytes = value.as_bytes();

	for index in 0..bytes.len() {
		if !bytes[index].is_ascii_alphabetic() {
			continue;
		}

		let mut prefix_end = index + 1;

		while prefix_end < bytes.len() && bytes[prefix_end].is_ascii_alphanumeric() {
			prefix_end += 1;
		}

		if prefix_end >= bytes.len() || bytes[prefix_end] != b'-' {
			continue;
		}

		let mut digit_end = prefix_end + 1;

		while digit_end < bytes.len() && bytes[digit_end].is_ascii_digit() {
			digit_end += 1;
		}

		if digit_end > prefix_end + 1 {
			return Some(value[index..digit_end].to_ascii_uppercase());
		}
	}

	None
}
