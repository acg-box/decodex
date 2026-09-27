//! Commit a second connection's update between the real projection queries.
use super::{CreateConversationRecord, OrdinaryTaskConversationProjection};
use crate::{CommandIdentity, SqliteStore};
use decodex_core::ConversationId;
use std::ffi::{CStr, c_int, c_uint, c_void};

struct Writer {
	connection: rusqlite::Connection,
	fired: bool,
	error: Option<String>,
}

struct ProjectionProbe {
	store: SqliteStore,
	writer: Box<Writer>,
}

impl ProjectionProbe {
	fn install(store: &SqliteStore, path: &std::path::Path) -> Self {
		let mut probe = Self {
			store: store.clone(),
			writer: Box::new(Writer {
				connection: rusqlite::Connection::open(path).expect("second database connection"),
				fired: false,
				error: None,
			}),
		};
		probe
			.store
			.with_connection(|connection| {
				// SAFETY: the boxed writer stays at this address until Drop unregisters the
				// callback. The test accesses it only before or after the awaited store read.
				let status = unsafe {
					rusqlite::ffi::sqlite3_trace_v2(
						connection.handle(),
						rusqlite::ffi::SQLITE_TRACE_PROFILE as c_uint,
						Some(commit_after_metadata),
						std::ptr::from_mut(probe.writer.as_mut()).cast(),
					)
				};
				assert_eq!(status, rusqlite::ffi::SQLITE_OK);
				Ok(())
			})
			.expect("install deterministic projection probe");
		probe
	}
}

impl Drop for ProjectionProbe {
	fn drop(&mut self) {
		self.store
			.with_connection(|connection| {
				// SAFETY: the retained store keeps this handle alive. Disable the callback
				// before its boxed context is dropped, including during assertion unwinding.
				unsafe {
					rusqlite::ffi::sqlite3_trace_v2(
						connection.handle(),
						0,
						None,
						std::ptr::null_mut(),
					);
				}
				Ok(())
			})
			.expect("remove projection probe");
	}
}

unsafe extern "C" fn commit_after_metadata(
	_event: c_uint,
	context: *mut c_void,
	statement: *mut c_void,
	_elapsed: *mut c_void,
) -> c_int {
	// SAFETY: SQLite supplies the installed context and active statement for a
	// PROFILE callback. ProjectionProbe keeps the context alive until unregistering.
	let writer = unsafe { &mut *context.cast::<Writer>() };
	let sql = unsafe { rusqlite::ffi::sqlite3_sql(statement.cast()) };
	if writer.fired || sql.is_null() {
		return 0;
	}
	let sql = unsafe { CStr::from_ptr(sql) }.to_string_lossy();
	if sql.starts_with("SELECT conversation_id, state, revision, updated_at_micros") {
		writer.fired = true;
		writer.error = writer
			.connection
			.execute(
				"UPDATE conversations SET title='After concurrent commit', revision=revision+1",
				[],
			)
			.err()
			.map(|error| error.to_string());
	}
	0
}

#[tokio::test]
async fn conversation_projection_keeps_one_snapshot_across_concurrent_commit() {
	for exact in [false, true] {
		let directory = tempfile::tempdir().expect("snapshot fixture");
		let path = directory.path().join("snapshot.sqlite3");
		let store = SqliteStore::open_test(&path).expect("fixture store");
		let id = ConversationId::new("30000000-0000-4000-8000-000000000001").expect("fixture ID");
		store
			.create_conversation(
				&CommandIdentity::new("snapshot-create", b"snapshot input").expect("command"),
				&CreateConversationRecord {
					conversation_id: id.clone(),
					title: "Before concurrent commit".into(),
					message: "Keep this input".into(),
					working_directory: "/tmp".into(),
					model: "fixture-model".into(),
					reasoning_effort: None,
					fast: false,
					service_tier: None,
					initial_model_source: None,
				},
			)
			.await
			.expect("create conversation");
		let before = store
			.read_ordinary_task_conversations(Some(&id), None, 1)
			.await
			.expect("initial projection");
		let probe = ProjectionProbe::install(&store, &path);
		let during = store.read_ordinary_task_conversations(exact.then_some(&id), None, 1).await;
		let fired = probe.writer.fired;
		let error = probe.writer.error.clone();
		drop(probe);
		assert!(fired, "commit must occur between the production queries");
		assert_eq!(error, None, "WAL writer must commit while the reader is active");
		let during = during.expect("concurrent projection");
		assert_eq!(during, before, "one read cannot combine old revision and new title");
		let after = store
			.read_ordinary_task_conversations(Some(&id), None, 1)
			.await
			.expect("fresh projection");
		let [OrdinaryTaskConversationProjection::Current(after)] = after.as_slice() else {
			panic!("current conversation")
		};
		let [OrdinaryTaskConversationProjection::Current(before)] = before.as_slice() else {
			panic!("initial conversation")
		};
		assert_eq!(after.title, "After concurrent commit");
		assert_eq!(after.conversation_revision, before.conversation_revision + 1);
	}
}
