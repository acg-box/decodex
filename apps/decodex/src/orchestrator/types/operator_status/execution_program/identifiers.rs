use crate::state::ExecutionProgramRecord;

pub(crate) fn operator_execution_program_mapped_issue_identifiers(
	record: &ExecutionProgramRecord,
) -> Vec<String> {
	let mut identifiers = record
		.program()
		.nodes()
		.iter()
		.filter_map(|node| node.linear_issue().map(|issue| issue.issue_identifier().to_owned()))
		.collect::<Vec<_>>();

	identifiers.sort();
	identifiers.dedup();

	identifiers
}
