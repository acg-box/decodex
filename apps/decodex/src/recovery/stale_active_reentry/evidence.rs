pub(in crate::recovery) fn evidence_contains(evidence: &[String], expected: &str) -> bool {
	evidence.iter().any(|entry| entry == expected)
}
