use super::*;

#[tokio::test]
async fn setup_recovery_keeps_identity_and_never_replays_claimed_work() {
	let directory = tempdir().unwrap();
	let store = SqliteStore::open_test(&directory.path().join("setup.sqlite3")).unwrap();
	for id in ["setup", "claimed", "running", "modern", "no-error", "foreign"] {
		store.create_chief_work_item(item(id, None)).await.unwrap();
		store.bind_chief_thread(id.into(), format!("{id}-thread")).await.unwrap();
		if id != "modern" {
			store
				.with_connection(|c| {
					c.execute("DELETE FROM chief_tool_versions WHERE work_id=?1", [id])
						.map_err(sqlite_error)?;
					Ok(())
				})
				.unwrap();
		}
		if id == "claimed" || id == "running" {
			store
				.begin_chief_dispatch_with_input(
					id.into(),
					vec![],
					Some("actual instruction".into()),
				)
				.await
				.unwrap();
			if id == "running" {
				store.acknowledge_chief_dispatch(id.into(), "actual-turn".into()).await.unwrap();
			}
		} else {
			store.begin_chief_tool_upgrade(id.into(), format!("{id}-thread")).await.unwrap();
		}
		store.mark_chief_dispatch_unknown(id.into()).await.unwrap();
		if id != "no-error" {
			store
				.record_chief_delivery_failure(id.into(), "Chief: Transport(InvalidFrame)".into())
				.await
				.unwrap();
		}
	}
	for id in ["claimed", "running", "modern", "no-error"] {
		assert!(
			!store
				.recover_legacy_chief_setup(id.into(), format!("{id}-thread"), None)
				.await
				.unwrap(),
			"{id}"
		);
		assert_eq!(
			store.get_chief_work_item(id.into()).await.unwrap().dispatch_state,
			ChiefDispatchState::Unknown
		);
	}
	assert!(
		!store
			.recover_legacy_chief_setup("foreign".into(), "wrong-thread".into(), None)
			.await
			.unwrap()
	);
	assert!(
		store
			.recover_legacy_chief_setup("setup".into(), "setup-thread".into(), None)
			.await
			.unwrap()
	);
	let recovered = store.get_chief_work_item("setup".into()).await.unwrap();
	assert_eq!(recovered.dispatch_state, ChiefDispatchState::Idle);
	assert_eq!(recovered.codex_thread_id.as_deref(), Some("setup-thread"));
	assert!(
		!store
			.recover_legacy_chief_setup("setup".into(), "setup-thread".into(), None)
			.await
			.unwrap()
	);
}
