use crate::{
	lane_authority::NoEffectiveDeltaRecovery,
	prelude::Result,
	state::{StateData, sqlite_store::SqliteStateStore},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_no_effective_delta_recoveries(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT payload_json FROM no_effective_delta_recoveries ORDER BY operation_id",
		)?;
		let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
		for payload in rows {
			let recovery = serde_json::from_str::<NoEffectiveDeltaRecovery>(&payload?)?;
			recovery.validate()?;
			state
				.no_effective_delta_recoveries
				.insert(recovery.operation_id().to_owned(), recovery);
		}
		Ok(())
	}
}
