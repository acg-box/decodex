use std::collections::BTreeSet;

pub(super) fn reason_codes(reasons: &[String]) -> Vec<String> {
	let mut seen = BTreeSet::new();

	for reason in reasons {
		seen.insert(reason_code(reason).to_owned());
	}

	seen.into_iter().collect()
}

pub(super) fn public_reason(reason: &str) -> String {
	if reason.starts_with("conflict domain `") {
		String::from("another active or retained program node occupies this conflict domain")
	} else if reason.contains(" is owned by the retained post-review lifecycle") {
		String::from(
			"Review & Landing owns this issue until post-review landing or closeout finishes",
		)
	} else if reason.starts_with("dependency `") {
		String::from("a dependency has not reached a required terminal state")
	} else {
		reason.to_owned()
	}
}

fn reason_code(reason: &str) -> &'static str {
	if reason == "node no longer matches the accepted Decision Contract" {
		"accepted_contract_mismatch"
	} else if reason == "node dispatch intent is not-ready" {
		"dispatch_intent_not_ready"
	} else if reason == "node dispatch intent is paused" {
		"dispatch_intent_paused"
	} else if reason == "node already has a current lane" {
		"current_lane_present"
	} else if reason == "node dispatch intent is terminal" {
		"dispatch_intent_terminal"
	} else if reason == "node is ready for normal Linear issue execution" {
		"ready_for_linear_execution"
	} else if reason == "node has no acceptance expectations" {
		"acceptance_expectations_missing"
	} else if reason == "node has no validation expectations" {
		"validation_expectations_missing"
	} else if reason.starts_with("dependency `") {
		"dependency_not_terminal"
	} else if reason.starts_with("conflict domain `") {
		"conflict_domain_occupied"
	} else if reason == "node has no normal Linear issue mapping" {
		"linear_issue_mapping_missing"
	} else if reason.contains(" is already terminal in `") {
		"mapped_issue_terminal"
	} else if reason.contains(" is not in a startable state") {
		"mapped_issue_not_startable"
	} else if reason.contains(" already carries `") {
		"mapped_issue_active_label_present"
	} else if reason.contains(" is owned by the retained post-review lifecycle") {
		"mapped_issue_post_review_owner"
	} else if reason.contains(" carries `decodex:manual-only`") {
		"mapped_issue_manual_only"
	} else if reason.contains(" carries `decodex:needs-attention`") {
		"mapped_issue_needs_attention"
	} else if reason.contains(" has open tracker dependency blockers") {
		"mapped_issue_open_blockers"
	} else if reason.contains(" is missing a generic dispatch briefing") {
		"mapped_issue_dispatch_briefing_missing"
	} else {
		"program_readiness_blocked"
	}
}
