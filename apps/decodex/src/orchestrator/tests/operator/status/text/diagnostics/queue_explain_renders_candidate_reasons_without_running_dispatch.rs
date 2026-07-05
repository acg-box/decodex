use crate::orchestrator::tests::operator::status::{self, orchestrator};

#[test]
fn queue_explain_renders_candidate_reasons_without_running_dispatch() {
	let (_temp_dir, config, _workflow) = status::temp_project_layout();
	let candidates = status::operator_status_text_queued_candidates();
	let rendered = orchestrator::render_queue_explain(&config, &candidates);

	assert!(rendered.contains("Mode: dry-run queue explain"));
	assert!(rendered.contains("Queued candidates: 3"));
	assert!(rendered.contains("Ready: 1"));
	assert!(rendered.contains("Claimed: 1"));
	assert!(rendered.contains("Closed: 1"));
	assert!(rendered.contains("issue: PUB-102"));
	assert!(rendered.contains("classification: ready"));
	assert!(rendered.contains("reason: eligible_for_dispatch"));
}
