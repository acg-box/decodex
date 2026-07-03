use color_eyre::Report;

pub(crate) fn issue_lookup_missing_error_for_candidate(error: &Report, candidate: &str) -> bool {
	let message = error.to_string();

	message.contains("Linear GraphQL request failed: Entity not found: Issue")
		|| (message.contains("Linear GraphQL request failed: Argument Validation Error")
			&& !looks_like_linear_server_issue_id(candidate))
}

pub(crate) fn label_not_on_issue_error(error: &Report) -> bool {
	error
		.chain()
		.any(|source| source.to_string().to_ascii_lowercase().contains("label not on issue"))
}

fn looks_like_linear_server_issue_id(candidate: &str) -> bool {
	let candidate = candidate.trim();
	let bytes = candidate.as_bytes();

	bytes.len() == 36
		&& [8, 13, 18, 23].into_iter().all(|index| bytes[index] == b'-')
		&& bytes
			.iter()
			.enumerate()
			.all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
}
