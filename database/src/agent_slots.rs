//! The service owns reusable display slots; identity and routing never use them.
use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension as _, TransactionBehavior};

use crate::{SqliteStore, StoreError, error};

/// A live display binding, independent of work history and execution capacity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSlot {
	/// Work ID for a managed agent, or a native child key.
	pub key: String,
	/// Managed owner of this binding.
	pub work_id: String,
	/// Exact native thread, when bound.
	pub thread_id: Option<String>,
	/// Reusable display number. The personal root always owns zero.
	pub number: u32,
}

impl SqliteStore {
	/// Read the current bindings without assigning or releasing a slot.
	pub async fn agent_slots(&self) -> Result<Vec<AgentSlot>, StoreError> {
		self.run(|c| read(c)).await
	}

	/// Apply positively observed membership changes in one serialized transaction.
	/// Missing or partial inventories must not be passed as a release instruction.
	pub async fn reconcile_agent_slots(
		&self,
		active: Vec<(String, String, Option<String>)>,
		retired: Vec<String>,
	) -> Result<Vec<AgentSlot>, StoreError> {
		self.run(move |c| {
			let tx = c.transaction_with_behavior(TransactionBehavior::Immediate).map_err(error::sqlite_error)?;

			for key in retired {
				tx.execute("DELETE FROM agent_slots WHERE agent_key=?1", [key]).map_err(error::sqlite_error)?;
			}

			let root: Option<String> = tx.query_row(
				"SELECT id FROM agent_work_items WHERE parent_goal_id IS NULL ORDER BY created_at_micros,id LIMIT 1",
				[], |row| row.get(0),
			).optional().map_err(error::sqlite_error)?;

			if let Some(root) = &root.filter(|id| active.iter().any(|(_, work, _)| work == id)) {
				tx.execute("INSERT OR IGNORE INTO agent_slots(agent_key,work_id,thread_id,slot) SELECT id,id,codex_thread_id,0 FROM agent_work_items WHERE id=?1", [root]).map_err(error::sqlite_error)?;
			}

			let eligible = tx.prepare("SELECT id FROM agent_work_items WHERE parent_goal_id IS NULL OR kind<>'task' OR status<>'resolved' OR dispatch_state<>'idle'").map_err(error::sqlite_error)?.query_map([], |row| row.get::<_,String>(0)).map_err(error::sqlite_error)?.collect::<Result<BTreeSet<_>,_>>().map_err(error::sqlite_error)?;
            let mut current = read(&tx)?;

            for slot in &current {
                if slot.number != 0 && !eligible.contains(&slot.work_id) {
                    tx.execute("DELETE FROM agent_slots WHERE agent_key=?1", [&slot.key]).map_err(error::sqlite_error)?;
                }
            }

            current.retain(|slot| slot.number == 0 || eligible.contains(&slot.work_id));

            let mut used: BTreeSet<_> = current.iter().map(|slot| slot.number).collect();

			for (key, work, thread) in active {
				if !eligible.contains(&work) { continue; }
				if current.iter().any(|slot| slot.key == key) {
					tx.execute("UPDATE agent_slots SET thread_id=?2 WHERE agent_key=?1 AND thread_id IS NOT ?2", rusqlite::params![key,thread]).map_err(error::sqlite_error)?;

					continue;
				}

				let number = (1..).find(|n| !used.contains(n)).ok_or(StoreError::InvalidInput("no display slot available"))?;

				if tx.execute("INSERT INTO agent_slots SELECT ?1,?2,?3,?4 WHERE NOT EXISTS(SELECT 1 FROM agent_slots WHERE agent_key=?1) AND NOT EXISTS(SELECT 1 FROM agent_closed_native_threads WHERE thread_id=?3)", rusqlite::params![key,work,thread,number]).map_err(error::sqlite_error)? > 0 {
					used.insert(number);
				}
			}

			let result = read(&tx)?;

			tx.commit().map_err(error::sqlite_error)?;

			Ok(result)
		}).await
	}

	/// Release a positively closed native child. An idle or unloaded child is not closed.
	pub async fn release_native_agent_slots(&self, threads: Vec<String>) -> Result<(), StoreError> {
		self.run(move |c| {
			let tx = c
				.transaction_with_behavior(TransactionBehavior::Immediate)
				.map_err(error::sqlite_error)?;

			for thread in threads {
				tx.execute(
					"INSERT OR IGNORE INTO agent_closed_native_threads SELECT ?1 WHERE NOT EXISTS(SELECT 1 FROM agent_slots WHERE thread_id=?1 AND agent_key=work_id) AND NOT EXISTS(SELECT 1 FROM agent_work_items WHERE codex_thread_id=?1)",
					[&thread],
				)
				.map_err(error::sqlite_error)?;
				tx.execute(
					"DELETE FROM agent_slots WHERE thread_id=?1 AND agent_key<>work_id",
					[thread],
				)
				.map_err(error::sqlite_error)?;
			}

			tx.commit().map_err(error::sqlite_error)?;

			Ok(())
		})
		.await
	}

	/// A new native turn confirms that a closed child participates again.
	pub async fn resume_native_agent_slot(&self, thread: String) -> Result<(), StoreError> {
		self.run(move |c| {
			c.execute("DELETE FROM agent_closed_native_threads WHERE thread_id=?1", [thread])
				.map_err(error::sqlite_error)?;

			Ok(())
		})
		.await
	}
}

fn read(c: &Connection) -> Result<Vec<AgentSlot>, StoreError> {
	c.prepare("SELECT agent_key,work_id,thread_id,slot FROM agent_slots ORDER BY slot")
		.map_err(error::sqlite_error)?
		.query_map([], |row| {
			Ok(AgentSlot {
				key: row.get(0)?,
				work_id: row.get(1)?,
				thread_id: row.get(2)?,
				number: row.get(3)?,
			})
		})
		.map_err(error::sqlite_error)?
		.collect::<Result<Vec<_>, _>>()
		.map_err(|error| error::sqlite_error(error).into())
}

#[cfg(test)]
mod tests {
	use crate::{agent_slots::SqliteStore, error};

	#[tokio::test]
	async fn slots_reuse_holes_without_changing_survivors_and_persist_across_restart() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join("slots.sqlite3");
		let store = SqliteStore::open_test(&path).unwrap();

		store.with_connection(|c| {
			c.execute("INSERT INTO agent_work_items(id,kind,title,instructions,status,created_at_micros,updated_at_micros) VALUES('main','goal','Main','Keep context','open',1,1)", []).map_err(error::sqlite_error)?;

			for id in ["a", "b", "c"] {
				c.execute("INSERT INTO agent_work_items(id,parent_goal_id,kind,title,instructions,status,created_at_micros,updated_at_micros) VALUES(?1,'main',?2,?1,'Keep context','open',2,2)", rusqlite::params![id, if id=="c" {"task"} else {"goal"}]).map_err(error::sqlite_error)?;
			}

			Ok(())
		}).unwrap();

		let target = |id: &str| (id.to_owned(), id.to_owned(), Some(format!("thread-{id}")));
		let slots = store
			.reconcile_agent_slots(vec![target("main"), target("a"), target("b")], vec![])
			.await
			.unwrap();

		assert_eq!(
			slots.iter().map(|s| (s.key.as_str(), s.number)).collect::<Vec<_>>(),
			[("main", 0), ("a", 1), ("b", 2)]
		);
		// Partial observations cannot release bindings.
		assert_eq!(store.reconcile_agent_slots(vec![target("b")], vec![]).await.unwrap(), slots);

		store.reconcile_agent_slots(vec![target("c")], vec!["a".into()]).await.unwrap();

		drop(store);

		let store = SqliteStore::open_test(&path).unwrap();
		let restored = store.reconcile_agent_slots(vec![target("a")], vec![]).await.unwrap();

		assert_eq!(
			restored.iter().map(|s| (s.key.as_str(), s.number)).collect::<Vec<_>>(),
			[("main", 0), ("c", 1), ("b", 2), ("a", 3)]
		);
		assert_eq!(
			restored.iter().find(|s| s.key == "a").unwrap().thread_id.as_deref(),
			Some("thread-a")
		);

		let native = ("native:a:child".into(), "a".into(), Some("child".into()));

		store.release_native_agent_slots(vec!["child".into()]).await.unwrap();

		assert_eq!(
			store.reconcile_agent_slots(vec![native.clone()], vec![]).await.unwrap(),
			restored
		);

		store.resume_native_agent_slot("child".into()).await.unwrap();
		store.reconcile_agent_slots(vec![native.clone()], vec![]).await.unwrap();
		store.release_native_agent_slots(vec!["child".into(), "thread-a".into()]).await.unwrap();

		assert!(store.agent_slots().await.unwrap().iter().any(|s| s.key == "a"));
		assert!(store.agent_slots().await.unwrap().iter().all(|s| s.key != "native:a:child"));
		assert!(
			store
				.reconcile_agent_slots(vec![native.clone()], vec![])
				.await
				.unwrap()
				.iter()
				.all(|s| s.key != "native:a:child")
		);

		store.resume_native_agent_slot("child".into()).await.unwrap();

		let resumed = store.reconcile_agent_slots(vec![native], vec![]).await.unwrap();

		assert_eq!(resumed.last().unwrap().number, 4);

		store
			.with_connection(|c| {
				c.execute("UPDATE agent_work_items SET status='resolved' WHERE id='c'", [])
					.map_err(error::sqlite_error)?;
				c.execute("UPDATE agent_work_items SET status='resolved' WHERE id='b'", [])
					.map_err(error::sqlite_error)?;

				Ok(())
			})
			.unwrap();

		let ended =
			store.reconcile_agent_slots(vec![target("b"), target("c")], vec![]).await.unwrap();

		assert!(ended.iter().all(|s| s.key != "c"));
		assert_eq!(ended.iter().find(|s| s.key == "b").unwrap().number, 2);
		assert_eq!(ended.iter().find(|s| s.key == "a").unwrap().number, 3);
	}

	#[tokio::test]
	async fn archived_root_slot_stays_released_until_explicit_restoration() {
		let directory = tempfile::tempdir().unwrap();
		let store = SqliteStore::open_test(&directory.path().join("slots.sqlite3")).unwrap();
		store.with_connection(|c| {
			c.execute("INSERT INTO agent_work_items(id,kind,title,instructions,status,created_at_micros,updated_at_micros) VALUES('main','goal','Main','Keep context','open',1,1)", []).map_err(error::sqlite_error)?;
			Ok(())
		}).unwrap();
		let root = ("main".into(), "main".into(), Some("thread-main".into()));
		let active = store.reconcile_agent_slots(vec![root.clone()], vec![]).await.unwrap();
		assert_eq!(active[0].number, 0);
		assert!(store.reconcile_agent_slots(vec![], vec!["main".into()]).await.unwrap().is_empty());
		assert!(store.reconcile_agent_slots(vec![], vec![]).await.unwrap().is_empty());
		assert_eq!(store.reconcile_agent_slots(vec![root], vec![]).await.unwrap(), active);
	}
}
