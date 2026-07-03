use serde_json::{self, Value};

use crate::{
	loop_contract::DecisionContract,
	prelude::Result,
	research_design::{
		ResearchDesignOutcome, ResearchDesignRunInput,
		normalized::NormalizedResearchDesignInput,
		reports::{ResearchDesignCompilation, ResearchDesignRunReport},
	},
};

pub(crate) fn dry_run_research_design_compile(
	input: ResearchDesignRunInput,
	project_id: &str,
) -> Result<ResearchDesignRunReport> {
	Ok(compile_research_design_run(input, project_id)?.report)
}

pub(super) fn compile_research_design_run(
	input: ResearchDesignRunInput,
	project_id: &str,
) -> Result<ResearchDesignCompilation> {
	let normalized = NormalizedResearchDesignInput::new(input)?;

	normalized.validate_outcome()?;

	let contract = build_decision_contract(&normalized, project_id)?;
	let report = ResearchDesignRunReport::from_compilation(&normalized, &contract);

	Ok(ResearchDesignCompilation { contract, report })
}

fn build_decision_contract(
	input: &NormalizedResearchDesignInput,
	project_id: &str,
) -> Result<DecisionContract> {
	let payload = serde_json::json!({
		"schema": crate::loop_contract::DECISION_CONTRACT_SCHEMA,
		"record_version": crate::loop_contract::DECISION_CONTRACT_RECORD_VERSION,
		"contract_id": input.contract_id,
		"status": input.outcome.contract_status(),
		"source_intent": {
			"summary": input.intent,
			"user_utterance": input.intent,
			"source_issue_identifier": input.source_issue_identifier,
		},
		"research_provenance": research_provenance_json(input),
		"research_evidence": research_evidence_json(input),
		"research_options": research_options_json(input),
		"accepted_authority": {
			"accepted_objectives": input.objectives,
			"non_goals": input.non_goals,
			"constraints": input.constraints,
			"assumptions": input.assumptions,
			"objections": input.objections,
			"stop_conditions": stop_conditions(input),
		},
		"execution_readiness": {
			"summary": input.readiness_summary,
			"ready_for_issue_shaping": input.ready_for_issue_shaping(),
			"missing_decisions": input.missing_decisions(),
			"validation_expectations": input.validation_expectations,
			"risk_notes": risk_notes(input),
			"proposed_issues": input.proposed_issues,
			"promotion_targets": input.promotion_targets,
			"conflict_domains": input.conflict_domains,
		},
		"links": {
			"generated_issue_ids": [],
			"generated_issue_identifiers": [],
			"execution_program_node_ids": [],
		},
		"evidence_boundary": {
			"private_evidence_refs": private_evidence_refs_json(input, project_id),
			"public_projection_refs": public_projection_refs_json(input),
			"public_summary": input.public_summary,
		},
	});
	let contract = serde_json::from_value::<DecisionContract>(payload)?;

	contract.validate()?;

	Ok(contract)
}

fn research_provenance_json(input: &NormalizedResearchDesignInput) -> Vec<Value> {
	let mut provenance = input
		.provenance
		.iter()
		.map(|item| {
			serde_json::json!({
				"kind": item.kind,
				"reference": item.reference,
				"summary": item.summary,
			})
		})
		.collect::<Vec<_>>();

	for subwork in &input.ai_subwork {
		provenance.push(serde_json::json!({
			"kind": format!("ai_subwork_{}", subwork.worker_kind),
			"reference": subwork.objective,
			"summary": subwork.summary(),
		}));
	}

	provenance
}

fn research_evidence_json(input: &NormalizedResearchDesignInput) -> Vec<Value> {
	input
		.evidence
		.iter()
		.map(|item| {
			serde_json::json!({
				"kind": item.kind,
				"claim": item.claim,
				"support": item.support,
				"source_ref": item.source_ref,
			})
		})
		.collect()
}

fn research_options_json(input: &NormalizedResearchDesignInput) -> Vec<Value> {
	input
		.options
		.iter()
		.map(|item| {
			serde_json::json!({
				"option": item.option,
				"tradeoffs": item.tradeoffs,
				"decision": item.decision,
				"rejected_reason": item.rejected_reason,
			})
		})
		.collect()
}

fn private_evidence_refs_json(
	input: &NormalizedResearchDesignInput,
	project_id: &str,
) -> Vec<Value> {
	input
		.private_evidence_refs
		.iter()
		.map(|item| {
			serde_json::json!({
				"project_id": item.project_id.as_deref().unwrap_or(project_id),
				"issue_id": item.issue_id,
				"run_id": item.run_id,
				"attempt_number": item.attempt_number,
				"record_id": item.record_id,
				"event_type": item.event_type,
			})
		})
		.collect()
}

fn public_projection_refs_json(input: &NormalizedResearchDesignInput) -> Vec<Value> {
	input
		.public_projection_refs
		.iter()
		.map(|item| {
			serde_json::json!({
				"surface": item.surface,
				"reference": item.reference,
				"summary": item.summary,
			})
		})
		.collect()
}

fn stop_conditions(input: &NormalizedResearchDesignInput) -> Vec<String> {
	let mut stop_conditions = input.stop_conditions.clone();

	for blocker in &input.blockers {
		stop_conditions.push(format!("Stop research promotion until blocker resolves: {blocker}"));
	}

	stop_conditions
}

fn risk_notes(input: &NormalizedResearchDesignInput) -> Vec<String> {
	let mut risk_notes = input.risk_notes.clone();

	if input.outcome == ResearchDesignOutcome::NotDecisionReady {
		risk_notes.push(String::from(
			"Research is not decision-ready and must not become implementation work.",
		));
	}

	for blocker in &input.blockers {
		risk_notes.push(format!("Research/design blocker: {blocker}"));
	}

	risk_notes
}
