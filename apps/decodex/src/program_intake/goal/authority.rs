use crate::{
	loop_contract::{DecisionContract, DecisionContractStatus},
	prelude::{Result, eyre},
};

pub(in crate::program_intake) fn ensure_goal_intake_authority(
	contract: &DecisionContract,
) -> Result<()> {
	if contract.status() != DecisionContractStatus::AcceptedPromoted {
		eyre::bail!(
			"Decision Contract `{}` is `{}`; goal intake requires accepted execution authority.",
			contract.contract_id(),
			contract.status().as_str()
		);
	}
	if !contract.execution_readiness().ready_for_issue_shaping() {
		eyre::bail!(
			"Decision Contract `{}` is not ready for issue shaping.",
			contract.contract_id()
		);
	}
	if !contract.execution_readiness().missing_decisions().is_empty() {
		eyre::bail!(
			"Decision Contract `{}` still has unresolved decisions.",
			contract.contract_id()
		);
	}
	if contract.execution_readiness().proposed_issues().is_empty() {
		eyre::bail!(
			"Decision Contract `{}` has no structured proposed issues to materialize.",
			contract.contract_id()
		);
	}

	Ok(())
}
