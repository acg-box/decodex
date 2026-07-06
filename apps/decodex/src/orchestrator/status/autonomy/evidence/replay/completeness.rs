pub(crate) fn operator_autonomy_evidence_completeness_rank(value: &str) -> u8 {
	match value {
		"complete" => 1,
		_ => 0,
	}
}
