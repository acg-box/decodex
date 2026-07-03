use crate::{
	loop_contract::{DecisionContractStatus, DecisionPromotion},
	prelude::{Result, eyre},
	research_design::{ResearchDesignRunInput, compiler, reports::ResearchDesignRunReport},
	state::{DecisionContractRecord, StateStore},
};

pub(crate) fn persist_research_design_run(
	store: &StateStore,
	project_id: &str,
	input: ResearchDesignRunInput,
) -> Result<ResearchDesignRunReport> {
	let source_issue_id = input.source_issue_identifier().map(str::to_owned);
	let compilation = compiler::compile_research_design_run(input, project_id)?;
	let record = store.upsert_decision_contract(
		project_id,
		source_issue_id.as_deref(),
		compilation.contract,
	)?;

	Ok(ResearchDesignRunReport { source_issue_id, ..compilation.report.with_record(&record) })
}

pub(crate) fn promote_research_design_contract(
	store: &StateStore,
	project_id: &str,
	contract_id: &str,
	promotion: DecisionPromotion,
) -> Result<DecisionContractRecord> {
	let record = store.promote_decision_contract(project_id, contract_id, promotion)?;

	ensure_contract_authorizes_execution(&record)?;

	Ok(record)
}

#[allow(dead_code)]
pub(super) fn ensure_contract_authorizes_execution(record: &DecisionContractRecord) -> Result<()> {
	if record.status() != DecisionContractStatus::AcceptedPromoted {
		eyre::bail!(
			"Research/design contract `{}` is not accepted; refusing to create execution work from unaccepted research.",
			record.contract_id()
		);
	}
	if !record.contract().execution_readiness().ready_for_issue_shaping() {
		eyre::bail!(
			"Accepted research/design contract `{}` is not ready for issue shaping.",
			record.contract_id()
		);
	}
	if !record.contract().execution_readiness().missing_decisions().is_empty() {
		eyre::bail!(
			"Accepted research/design contract `{}` still has unresolved decisions.",
			record.contract_id()
		);
	}
	if record.contract().execution_readiness().proposed_issues().is_empty() {
		eyre::bail!(
			"Accepted research/design contract `{}` has no structured proposed issues.",
			record.contract_id()
		);
	}

	Ok(())
}
