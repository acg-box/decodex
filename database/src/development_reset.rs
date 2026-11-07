//! Explicit offline cleanup for a development workspace. Account configuration stays intact.
use std::collections::BTreeMap;

use rusqlite::TransactionBehavior;

use crate::{SqliteStore, StoreError, error, schema};

fn retained(table: &str) -> bool {
	table.starts_with("account_")
		|| matches!(
			table,
			"accounts"
				| "codex_account_capability"
				| "desktop_settings"
				| "guardian_settings"
				| "role_profiles"
				| "projects"
				| "reset_card_operations"
		)
}

impl SqliteStore {
	/// Read the exact native contexts registered by this product database.
	pub async fn development_native_threads(&self) -> Result<Vec<String>, StoreError> {
		self.run(|c| {
			c.prepare("SELECT codex_thread_id FROM agent_work_items WHERE codex_thread_id IS NOT NULL UNION SELECT old_thread_id FROM agent_thread_revisions UNION SELECT new_thread_id FROM agent_thread_revisions UNION SELECT thread_id FROM agent_closed_native_threads UNION SELECT codex_thread_id FROM runtime_sessions WHERE codex_thread_id IS NOT NULL")
				.map_err(error::sqlite_error)?
				.query_map([], |r| r.get(0)).map_err(error::sqlite_error)?
				.collect::<Result<Vec<_>, _>>().map_err(|e| error::sqlite_error(e).into())
		}).await
	}

	/// Clear development conversations, Work, teams, duties and execution state.
	/// The operator must stop the service and remove its native contexts first.
	/// Preserve account credentials, account policy, model defaults and project folders.
	pub async fn clear_development_data(&self) -> Result<BTreeMap<String, usize>, StoreError> {
		self.run(|c| {
			let tx = c
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(error::sqlite_error)?;
			if tx
				.query_row(
					"SELECT EXISTS(SELECT 1 FROM agent_work_items WHERE active_turn_id IS NOT NULL)",
					[],
					|r| r.get::<_, bool>(0),
				)
				.map_err(error::sqlite_error)?
			{
				return Err(StoreError::InvalidInput(
					"stop active development turns before clearing data",
				));
			}
			let tables = tx
				.prepare(
					"SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
				)
				.map_err(error::sqlite_error)?
				.query_map([], |r| r.get::<_, String>(0))
				.map_err(error::sqlite_error)?
				.collect::<Result<Vec<_>, _>>()
				.map_err(error::sqlite_error)?;
			tx.execute_batch(
				"PRAGMA defer_foreign_keys=ON; UPDATE projects SET lead_agent_id=NULL;",
			)
			.map_err(error::sqlite_error)?;
			let mut removed = BTreeMap::new();
			for table in tables.into_iter().filter(|t| !retained(t)) {
				let count = tx
					.execute(&format!("DELETE FROM \"{}\"", table.replace('"', "\"\"")), [])
					.map_err(error::sqlite_error)?;
				removed.insert(table, count);
			}
			tx.execute("DELETE FROM sqlite_sequence", []).map_err(error::sqlite_error)?;
			tx.commit().map_err(error::sqlite_error)?;
			c.execute_batch(
				"PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);",
			)
			.map_err(error::sqlite_error)?;
			schema::verify(c)?;
			Ok(removed)
		})
		.await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{AgentDispatchState, AgentWorkItem, AgentWorkKind, AgentWorkStatus};

	#[tokio::test]
	async fn development_reset_clears_conversations_and_keeps_configuration() {
		let dir = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&dir.path().join("reset.sqlite3")).unwrap();
		store
			.create_agent_manager(AgentWorkItem {
				id: "old".into(),
				parent_goal_id: None,
				kind: AgentWorkKind::Goal,
				title: "Old conversation".into(),
				instructions: "Old instructions".into(),
				codex_thread_id: None,
				status: AgentWorkStatus::Open,
				dispatch_state: AgentDispatchState::Idle,
				active_turn_id: None,
				next_check_at_micros: None,
				created_at_micros: 1,
				updated_at_micros: 1,
			})
			.await
			.unwrap();
		store
			.register_project("project".into(), "Workspace".into(), "/tmp/workspace".into())
			.await
			.unwrap();
		store.set_project_lead("project".into(), "old".into()).await.unwrap();
		store.run(|c| {
			c.execute_batch("INSERT INTO account_identities VALUES('00000000-0000-0000-0000-000000000001',1); UPDATE role_profiles SET model='retained-model'; INSERT INTO project_agents(agent_id,project_id) VALUES('old','project');").map_err(error::sqlite_error)?;
			Ok(())
		}).await.unwrap();
		let removed = store.clear_development_data().await.unwrap();
		assert_eq!(removed["agent_work_items"], 1);
		assert!(store.agents().await.unwrap().is_empty());
		let projects = store.projects().await.unwrap();
		assert_eq!(projects[0].directory, "/tmp/workspace");
		assert!(projects[0].lead_agent_id.is_none() && projects[0].agent_ids.is_empty());
		store
			.run(|c| {
				assert_eq!(
					c.query_row("SELECT count(*) FROM account_identities", [], |r| r
						.get::<_, i64>(0))
						.unwrap(),
					1
				);
				assert_eq!(
					c.query_row("SELECT model FROM role_profiles WHERE role='task'", [], |r| {
						r.get::<_, String>(0)
					})
					.unwrap(),
					"retained-model"
				);
				Ok(())
			})
			.await
			.unwrap();
		assert!(store.clear_development_data().await.unwrap().values().all(|n| *n == 0));
	}
}
