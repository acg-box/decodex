use serde_json::Value;

use crate::autonomy_proposal::{AutonomyProposal, AutonomyProposalChallengeEvidence};

pub(super) fn autonomy_decision_research_evidence(proposal: &AutonomyProposal) -> Vec<Value> {
	let mut evidence = proposal
		.source_signals
		.iter()
		.map(|signal| {
			let mut support = vec![
				format!("freshness={}", signal.freshness),
				format!("evidence_class={}", signal.evidence_class),
				format!("confidence={}", signal.confidence),
			];

			if !signal.gaps.is_empty() {
				support.push(format!("gaps={}", signal.gaps.join("; ")));
			}
			if !signal.contradictions.is_empty() {
				support.push(format!("contradictions={}", signal.contradictions.join("; ")));
			}

			serde_json::json!({
				"kind": format!("autonomy_signal:{}", signal.kind),
				"claim": format!("Autonomy signal `{}` contributed to accepted proposal `{}`.", signal.signal_id, proposal.id),
				"support": support.join("; "),
				"source_ref": signal.signal_id.clone(),
			})
		})
		.collect::<Vec<_>>();

	if !proposal.gaps.is_empty() {
		evidence.push(serde_json::json!({
			"kind": "autonomy_proposal_gap",
			"claim": "Accepted proposal retained evidence gaps for downstream review.",
			"support": proposal.gaps.join("; "),
			"source_ref": proposal.id.clone(),
		}));
	}
	if !proposal.contradictions.is_empty() {
		evidence.push(serde_json::json!({
			"kind": "autonomy_proposal_contradiction",
			"claim": "Accepted proposal retained contradictions for downstream authority checks.",
			"support": proposal.contradictions.join("; "),
			"source_ref": proposal.id.clone(),
		}));
	}

	for challenge in &proposal.challenge_evidence {
		evidence.push(serde_json::json!({
			"kind": "autonomy_proposal_challenge",
			"claim": challenge.summary.clone(),
			"support": challenge_support(challenge),
			"source_ref": format!("challenge:{}", challenge.actor),
		}));
	}

	evidence
}

fn challenge_support(challenge: &AutonomyProposalChallengeEvidence) -> String {
	if !challenge.objections.is_empty() {
		challenge.objections.join("; ")
	} else if !challenge.evidence_refs.is_empty() {
		format!("evidence_refs={}", challenge.evidence_refs.join("; "))
	} else {
		String::from("Challenge recorded no objections.")
	}
}
