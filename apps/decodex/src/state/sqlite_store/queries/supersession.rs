use crate::{
	lane_authority::{
		RepairHandoffAuthority, RepairHandoffState, SupersededCloseoutOperation, SupersessionEdge,
	},
	prelude::Result,
	state::{StateData, sqlite_store::SqliteStateStore},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_supersession_authority(
		&self,
		state: &mut StateData,
	) -> Result<()> {
		let mut handoffs = self
			.connection
			.prepare("SELECT payload_json, state FROM repair_handoffs ORDER BY handoff_id")?;
		for row in handoffs
			.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
		{
			let (payload, state_text) = row?;
			let handoff = serde_json::from_str::<RepairHandoffAuthority>(&payload)?;
			handoff.validate()?;
			let handoff_state = RepairHandoffState::from_str(&state_text)
				.ok_or_else(|| crate::prelude::eyre::eyre!("Invalid repair handoff state."))?;
			state.repair_handoff_states.insert(handoff.handoff_id().to_owned(), handoff_state);
			state.repair_handoffs.insert(handoff.handoff_id().to_owned(), handoff);
		}
		let mut edges = self
			.connection
			.prepare("SELECT payload_json FROM supersession_edges ORDER BY edge_id")?;
		for payload in edges.query_map([], |row| row.get::<_, String>(0))? {
			let edge = serde_json::from_str::<SupersessionEdge>(&payload?)?;
			edge.validate()?;
			state.supersession_edges.insert(edge.predecessor_lane_id().clone(), edge);
		}
		let mut operations = self.connection.prepare(
			"SELECT payload_json FROM superseded_closeout_operations ORDER BY operation_id",
		)?;
		for payload in operations.query_map([], |row| row.get::<_, String>(0))? {
			let operation = serde_json::from_str::<SupersededCloseoutOperation>(&payload?)?;
			operation.validate()?;
			state
				.superseded_closeout_operations
				.insert(operation.operation_id().to_owned(), operation);
		}
		Ok(())
	}
}
