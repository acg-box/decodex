use crate::{
	prelude::{Result, eyre},
	state::{
		runtime_records::{
			AutonomyObjectiveRuntimeRecord, AutonomyProposalRuntimeRecord,
			AutonomyRuntimePolicyRuntimeRecord, AutonomySignalRuntimeRecord,
			DecisionContractRuntimeRecord, ExecutionProgramRuntimeRecord,
			LinearExecutionEventRuntimeRecord, PrivateExecutionEventRuntimeRecord,
		},
		store::StateStore,
	},
};

impl StateStore {
	pub(in crate::state) fn insert_linear_execution_event_if_absent_locked(
		&self,
		record: &LinearExecutionEventRuntimeRecord,
	) -> Result<bool> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(true);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.insert_linear_execution_event_if_absent(record)
	}

	pub(in crate::state) fn delete_linear_execution_event_locked(
		&self,
		idempotency_key: &str,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.delete_linear_execution_event(idempotency_key)
	}

	pub(in crate::state) fn list_persisted_linear_execution_events(
		&self,
		service_id: &str,
		issue_id: &str,
	) -> Result<Option<Vec<LinearExecutionEventRuntimeRecord>>> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(None);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.list_linear_execution_events(service_id, issue_id).map(Some)
	}

	pub(in crate::state) fn insert_private_execution_event_locked(
		&self,
		record: &PrivateExecutionEventRuntimeRecord,
	) -> Result<Option<i64>> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(None);
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.insert_private_execution_event(record).map(Some)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_decision_contract_locked(
		&self,
		record: &DecisionContractRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_decision_contract(record)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_autonomy_objective_locked(
		&self,
		record: &AutonomyObjectiveRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_autonomy_objective(record)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_autonomy_runtime_policy_locked(
		&self,
		record: &AutonomyRuntimePolicyRuntimeRecord,
	) -> Result<AutonomyRuntimePolicyRuntimeRecord> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(record.clone());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_autonomy_runtime_policy(record)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_autonomy_signal_locked(
		&self,
		record: &AutonomySignalRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_autonomy_signal(record)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_autonomy_proposal_locked(
		&self,
		record: &AutonomyProposalRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_autonomy_proposal(record)
	}

	#[allow(dead_code)]
	pub(in crate::state) fn upsert_execution_program_locked(
		&self,
		record: &ExecutionProgramRuntimeRecord,
	) -> Result<()> {
		let Some(sqlite) = self.sqlite.as_ref() else {
			return Ok(());
		};
		let sqlite =
			sqlite.lock().map_err(|_| eyre::eyre!("StateStore SQLite mutex is poisoned."))?;

		sqlite.upsert_execution_program(record)
	}
}
