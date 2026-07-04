use crate::orchestrator::agent_evidence::PrivateEvidencePayloadSummary;

pub(in crate::orchestrator::agent_evidence::private_readback::render) fn render_private_evidence_payload_summary(
	summary: &PrivateEvidencePayloadSummary,
) -> String {
	let keys = if summary.keys.is_empty() { String::from("none") } else { summary.keys.join(",") };
	let preview =
		if summary.preview.is_empty() { String::from("none") } else { summary.preview.join("; ") };
	let redacted = if summary.redacted_default_keys.is_empty() {
		String::from("none")
	} else {
		summary.redacted_default_keys.join(",")
	};

	format!(
		"kind={} bytes={} keys={} preview={} redacted_default_keys={}",
		summary.kind, summary.byte_count, keys, preview, redacted
	)
}
