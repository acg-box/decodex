use crate::{
	lane_authority::LaneEffect,
	prelude::Result,
	state::sqlite_store::{SqliteStateStore, queries::StateData},
};

impl SqliteStateStore {
	pub(in crate::state) fn load_lane_effects(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self
			.connection
			.prepare("SELECT payload_json FROM lane_effects ORDER BY operation_id, ordinal")?;
		let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
		for payload in rows {
			let effect = serde_json::from_str::<LaneEffect>(&payload?)?;
			effect.validate()?;
			state.lane_effects.insert(effect.effect_id().to_owned(), effect);
		}
		Ok(())
	}
}
