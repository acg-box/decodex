use libc::FD_CLOEXEC;
use libc::F_GETFD;
use libc::F_SETFD;

pub(crate) struct EffectiveRuntimeMarker<'a> {
	pub(crate) thread_id: Option<&'a str>,
	pub(crate) turn_id: Option<&'a str>,
	pub(crate) effective_model: &'a str,
	pub(crate) effective_model_provider: &'a str,
	pub(crate) effective_cwd: &'a str,
	pub(crate) effective_approval_policy: &'a str,
	pub(crate) effective_approvals_reviewer: &'a str,
	pub(crate) effective_sandbox_mode: &'a str,
}

pub(crate) struct ProtocolActivityMarker<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) thread_id: Option<&'a str>,
	pub(crate) turn_id: Option<&'a str>,
	pub(crate) event_count: i64,
	pub(crate) last_event_type: &'a str,
	pub(crate) child_agent_activity: Option<&'a ChildAgentActivitySummary>,
	pub(crate) protocol_activity: Option<&'a ProtocolActivitySummary>,
}

pub(crate) struct CodexAccountMarker<'a> {
	pub(crate) run_id: &'a str,
	pub(crate) attempt_number: i64,
	pub(crate) account: &'a CodexAccountActivitySummary,
	pub(crate) accounts: &'a [CodexAccountActivitySummary],
}

#[derive(Clone)]
struct DispatchSlotConfig {
	root: PathBuf,
	slot_limit: usize,
}

struct IssueClaimGuard {
	lock_file: File,
	retention: GuardRetention,
}
impl IssueClaimGuard {
	fn unlock(self) -> Result<()> {
		self.lock_file.unlock()?;

		Ok(())
	}

	fn release_for_clear(self) -> Result<()> {
		match self.retention {
			GuardRetention::ParentAfterHandoff => Ok(()),
			GuardRetention::Local | GuardRetention::AdoptingChild => self.unlock(),
		}
	}
}

struct DispatchSlotGuard {
	project_id: String,
	slot_index: usize,
	lock_file: File,
	retention: GuardRetention,
}
impl DispatchSlotGuard {
	fn release_for_clear(self) -> Result<()> {
		match self.retention {
			GuardRetention::ParentAfterHandoff => Ok(()),
			GuardRetention::Local | GuardRetention::AdoptingChild => {
				self.lock_file.unlock()?;

				Ok(())
			},
		}
	}
}

#[derive(Default)]
struct StateData {
	projects: HashMap<String, ProjectRegistration>,
	leases: HashMap<String, IssueLease>,
	run_attempts: HashMap<String, RunAttemptRecord>,
	events: HashMap<String, Vec<ProtocolEventRecord>>,
	event_summaries: HashMap<String, ProtocolEventSummaryRecord>,
	worktrees: HashMap<String, WorktreeMappingRecord>,
	linear_execution_events: HashMap<String, LinearExecutionEventRuntimeRecord>,
	review_handoffs: HashMap<ReviewMarkerKey, ReviewHandoffRuntimeRecord>,
	review_orchestrations: HashMap<ReviewOrchestrationKey, ReviewOrchestrationRuntimeRecord>,
	dispatch_slot_configs: HashMap<String, DispatchSlotConfig>,
	issue_claim_guards: HashMap<String, IssueClaimGuard>,
	dispatch_slot_guards: HashMap<String, DispatchSlotGuard>,
}
impl StateData {
	fn replace_durable_state(&mut self, loaded: Self) {
		self.projects = loaded.projects;
		self.leases = loaded.leases;
		self.run_attempts = loaded.run_attempts;
		self.events = loaded.events;
		self.event_summaries = loaded.event_summaries;
		self.worktrees = loaded.worktrees;
		self.linear_execution_events = loaded.linear_execution_events;
		self.review_handoffs = loaded.review_handoffs;
		self.review_orchestrations = loaded.review_orchestrations;
	}

	fn project_run_status(
		&self,
		project_id: &str,
		attempt: &RunAttemptRecord,
	) -> Option<ProjectRunStatus> {
		let worktree = self.worktrees.get(&attempt.issue_id);
		let active_lease = self
			.leases
			.get(&attempt.issue_id)
			.is_some_and(|lease| lease.project_id == project_id && lease.run_id == attempt.run_id);
		let remembered_project = attempt.project_id.as_deref() == Some(project_id);
		let in_project =
			remembered_project
				|| worktree.is_some_and(|mapping| mapping.project_id == project_id)
				|| active_lease;

		if !in_project {
			return None;
		}

		let event_summary = self.protocol_event_summary(&attempt.run_id);

		Some(ProjectRunStatus {
			run_id: attempt.run_id.clone(),
			issue_id: attempt.issue_id.clone(),
			attempt_number: attempt.attempt_number,
			status: attempt.status.clone(),
			thread_id: attempt.thread_id.clone(),
			turn_id: attempt.turn_id.clone(),
			updated_at: attempt.updated_at.clone(),
			updated_at_unix: attempt.updated_at_unix,
			branch_name: worktree.map(|mapping| mapping.branch_name.clone()),
			worktree_path: worktree.map(|mapping| mapping.worktree_path.clone()),
			active_lease,
			event_count: event_summary.event_count,
			last_event_type: event_summary.last_event_type,
			last_event_at: event_summary.last_event_at,
			last_event_at_unix: event_summary.last_event_at_unix,
		})
	}

	fn protocol_event_summary(&self, run_id: &str) -> ProtocolEventSummaryRecord {
		self.event_summaries
			.get(run_id)
			.cloned()
			.or_else(|| self.events.get(run_id).map(|events| protocol_event_summary_from_events(events)))
			.unwrap_or_default()
	}

	fn project_id_for_run(&self, issue_id: &str, run_id: &str) -> Option<String> {
		self.leases
			.get(issue_id)
			.filter(|lease| lease.run_id == run_id)
			.map(|lease| lease.project_id.clone())
			.or_else(|| self.worktrees.get(issue_id).map(|mapping| mapping.project_id.clone()))
	}

	fn remember_run_project(&mut self, project_id: &str, issue_id: &str, run_id: Option<&str>) {
		for attempt in self
			.run_attempts
			.values_mut()
			.filter(|attempt| attempt.issue_id == issue_id)
			.filter(|attempt| run_id.is_none_or(|run_id| attempt.run_id == run_id))
		{
			attempt.project_id = Some(project_id.to_owned());
		}
	}
}

struct SqliteStateStore {
	connection: Connection,
}
impl SqliteStateStore {
	fn open(path: &Path) -> Result<Self> {
		if let Some(parent) = path.parent() {
			fs::create_dir_all(parent)?;
		}

		let connection = Connection::open(path)?;

		connection.busy_timeout(Duration::from_secs(5))?;

		let store = Self { connection };

		store.bootstrap_schema()?;

		Ok(store)
	}

	fn bootstrap_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS projects (
	service_id TEXT PRIMARY KEY NOT NULL,
	config_path TEXT NOT NULL,
	repo_root TEXT NOT NULL,
	worktree_root TEXT NOT NULL,
	workflow_path TEXT NOT NULL,
	tracker_api_key_env_var TEXT NOT NULL,
	github_token_env_var TEXT NOT NULL,
	enabled INTEGER NOT NULL,
	config_fingerprint TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS leases (
	issue_id TEXT PRIMARY KEY NOT NULL,
	project_id TEXT NOT NULL,
	run_id TEXT NOT NULL,
	issue_state TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS run_attempts (
	run_id TEXT PRIMARY KEY NOT NULL,
	project_id TEXT,
	issue_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	status TEXT NOT NULL,
	thread_id TEXT,
	turn_id TEXT,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS protocol_events (
	run_id TEXT NOT NULL,
	sequence_number INTEGER NOT NULL,
	event_type TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	PRIMARY KEY (run_id, sequence_number)
);
CREATE TABLE IF NOT EXISTS worktrees (
	issue_id TEXT PRIMARY KEY NOT NULL,
	project_id TEXT NOT NULL,
	branch_name TEXT NOT NULL,
	worktree_path TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS linear_execution_events (
	idempotency_key TEXT PRIMARY KEY NOT NULL,
	service_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	event_type TEXT NOT NULL,
	event_timestamp TEXT NOT NULL,
	event_unix INTEGER,
	payload_json TEXT NOT NULL,
	recorded_at TEXT NOT NULL,
	recorded_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS linear_execution_events_issue_idx
ON linear_execution_events (service_id, issue_id, event_unix, recorded_at_unix);
CREATE TABLE IF NOT EXISTS review_handoffs (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	branch_name TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	pr_url TEXT NOT NULL,
	target_base_ref_name TEXT,
	pr_head_ref_name TEXT NOT NULL,
	pr_head_oid TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, branch_name)
);
CREATE TABLE IF NOT EXISTS review_orchestrations (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	branch_name TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	pr_url TEXT NOT NULL,
	head_sha TEXT NOT NULL,
	phase TEXT NOT NULL,
	request_comment_database_id INTEGER,
	request_created_at_unix_epoch INTEGER,
	request_description_thumbs_up_count INTEGER,
	request_retry_count INTEGER NOT NULL,
	external_round_count INTEGER NOT NULL,
	auto_merge_enabled_at_unix_epoch INTEGER,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, branch_name, run_id, attempt_number)
);
CREATE TABLE IF NOT EXISTS schema_meta (
	key TEXT PRIMARY KEY NOT NULL,
	value TEXT NOT NULL
);
INSERT INTO schema_meta (key, value)
VALUES ('schema_version', '3')
ON CONFLICT(key) DO UPDATE SET value = excluded.value;
"#,
		)?;

		Ok(())
	}

	fn load_state(&self) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_projects(&mut state)?;
		self.load_leases(&mut state)?;
		self.load_run_attempts(&mut state)?;
		self.load_protocol_event_summaries(&mut state)?;
		self.load_worktrees(&mut state)?;
		self.load_linear_execution_events(&mut state)?;
		self.load_review_handoffs(&mut state)?;
		self.load_review_orchestrations(&mut state)?;

		Ok(state)
	}

	fn persist_runtime_state(&mut self, state: &StateData) -> Result<()> {
		let transaction = self.connection.transaction()?;

		persist_projects(&transaction, state)?;
		persist_leases(&transaction, state)?;
		persist_run_attempts(&transaction, state)?;
		persist_protocol_events(&transaction, state)?;
		persist_worktrees(&transaction, state)?;
		persist_linear_execution_events(&transaction, state)?;
		persist_review_handoffs(&transaction, state)?;
		persist_review_orchestrations(&transaction, state)?;

		transaction.commit()?;

		Ok(())
	}

	fn upsert_run_attempt(&self, attempt: &RunAttemptRecord) -> Result<()> {
		self.connection.execute(
			"INSERT OR REPLACE INTO run_attempts (
					run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&attempt.run_id,
				attempt.project_id.as_deref(),
				&attempt.issue_id,
				attempt.attempt_number,
				&attempt.status,
				attempt.thread_id.as_deref(),
				attempt.turn_id.as_deref(),
				&attempt.updated_at,
				attempt.updated_at_unix,
			],
		)?;

		Ok(())
	}

	fn append_protocol_event(&self, run_id: &str, event: &ProtocolEventRecord) -> Result<bool> {
		let changed = self.connection.execute(
			"INSERT OR IGNORE INTO protocol_events (
					run_id, sequence_number, event_type, created_at, created_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5)",
			params![
				run_id,
				event.sequence_number,
				&event.event_type,
				&event.created_at,
				event.created_at_unix,
			],
		)?;

		Ok(changed == 1)
	}

	fn insert_linear_execution_event_if_absent(
		&self,
		record: &LinearExecutionEventRuntimeRecord,
	) -> Result<bool> {
		let payload_json = serde_json::to_string(&record.record)?;
		let changed = self.connection.execute(
			"INSERT OR IGNORE INTO linear_execution_events (
					idempotency_key, service_id, issue_id, event_type, event_timestamp,
					event_unix, payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&record.record.idempotency_key,
				&record.record.service_id,
				&record.record.issue_id,
				&record.record.event_type,
				&record.record.event_timestamp,
				record.event_unix,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;

		Ok(changed == 1)
	}

	fn delete_linear_execution_event(&self, idempotency_key: &str) -> Result<()> {
		self.connection.execute(
			"DELETE FROM linear_execution_events WHERE idempotency_key = ?1",
			params![idempotency_key],
		)?;

		Ok(())
	}

	fn delete_lease(&mut self, issue_id: &str) -> Result<()> {
		self.connection
			.execute("DELETE FROM leases WHERE issue_id = ?1", params![issue_id])?;

		Ok(())
	}

	fn delete_previous_issue_identity(&mut self, previous_issue_id: &str) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute("DELETE FROM leases WHERE issue_id = ?1", params![previous_issue_id])?;
		transaction.execute("DELETE FROM worktrees WHERE issue_id = ?1", params![previous_issue_id])?;
		transaction.execute(
			"DELETE FROM review_handoffs WHERE issue_id = ?1",
			params![previous_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM review_orchestrations WHERE issue_id = ?1",
			params![previous_issue_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

	fn delete_worktree_and_review_markers(&mut self, issue_id: &str) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute("DELETE FROM worktrees WHERE issue_id = ?1", params![issue_id])?;
		transaction.execute("DELETE FROM review_handoffs WHERE issue_id = ?1", params![issue_id])?;
		transaction.execute(
			"DELETE FROM review_orchestrations WHERE issue_id = ?1",
			params![issue_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

	fn delete_review_markers(&mut self, issue_id: &str) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute("DELETE FROM review_handoffs WHERE issue_id = ?1", params![issue_id])?;
		transaction.execute(
			"DELETE FROM review_orchestrations WHERE issue_id = ?1",
			params![issue_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

	fn delete_review_marker_identity(
		&mut self,
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute(
			"DELETE FROM review_handoffs
			 WHERE project_id = ?1 AND issue_id = ?2 AND branch_name = ?3",
			params![project_id, issue_id, branch_name],
		)?;
		transaction.execute(
			"DELETE FROM review_orchestrations
			 WHERE project_id = ?1 AND issue_id = ?2 AND branch_name = ?3
			   AND run_id = ?4 AND attempt_number = ?5",
			params![project_id, issue_id, branch_name, run_id, attempt_number],
		)?;
		transaction.commit()?;

		Ok(())
	}

	fn load_projects(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT service_id, config_path, repo_root, worktree_root, workflow_path, \
			 tracker_api_key_env_var, github_token_env_var, enabled, config_fingerprint, \
			 updated_at, updated_at_unix FROM projects",
		)?;
		let rows = statement.query_map([], |row| {
			let service_id: String = row.get(0)?;

			Ok((
				service_id.clone(),
				ProjectRegistration {
					service_id,
					config_path: PathBuf::from(row.get::<_, String>(1)?),
					repo_root: PathBuf::from(row.get::<_, String>(2)?),
					worktree_root: PathBuf::from(row.get::<_, String>(3)?),
					workflow_path: PathBuf::from(row.get::<_, String>(4)?),
					tracker_api_key_env_var: row.get(5)?,
					github_token_env_var: row.get(6)?,
					enabled: row.get::<_, i64>(7)? != 0,
					config_fingerprint: row.get(8)?,
					updated_at: row.get(9)?,
					updated_at_unix: row.get(10)?,
				},
			))
		})?;

		for row in rows {
			let (service_id, project) = row?;

			state.projects.insert(service_id, project);
		}

		Ok(())
	}

	fn load_leases(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self
			.connection
			.prepare("SELECT issue_id, project_id, run_id, issue_state FROM leases")?;
		let rows = statement.query_map([], |row| {
			let issue_id: String = row.get(0)?;

			Ok((
				issue_id.clone(),
				IssueLease {
					issue_id,
					project_id: row.get(1)?,
					run_id: row.get(2)?,
					issue_state: row.get(3)?,
				},
			))
		})?;

		for row in rows {
			let (issue_id, lease) = row?;

			state.leases.insert(issue_id, lease);
		}

		Ok(())
	}

	fn load_run_attempts(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts",
		)?;
		let rows = statement.query_map([], |row| {
			let run_id: String = row.get(0)?;

			Ok((
				run_id.clone(),
				RunAttemptRecord {
					run_id,
					project_id: row.get(1)?,
					issue_id: row.get(2)?,
					attempt_number: row.get(3)?,
					status: row.get(4)?,
					thread_id: row.get(5)?,
					turn_id: row.get(6)?,
					updated_at: row.get(7)?,
					updated_at_unix: row.get(8)?,
				},
			))
		})?;

		for row in rows {
			let (run_id, attempt) = row?;

			state.run_attempts.insert(run_id, attempt);
		}

		Ok(())
	}

	fn load_protocol_event_summaries(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT totals.run_id, totals.event_count, totals.last_sequence_number, \
			 last.event_type, last.created_at, last.created_at_unix \
			 FROM (
			 SELECT run_id, COUNT(*) AS event_count, MAX(sequence_number) AS last_sequence_number \
			 FROM protocol_events GROUP BY run_id
			 ) totals \
			 JOIN protocol_events last \
			 ON last.run_id = totals.run_id \
			 AND last.sequence_number = totals.last_sequence_number \
			 ORDER BY totals.run_id",
		)?;
		let rows = statement.query_map([], |row| {
			Ok((
				row.get::<_, String>(0)?,
				ProtocolEventSummaryRecord {
					event_count: row.get(1)?,
					last_sequence_number: Some(row.get(2)?),
					last_event_type: Some(row.get(3)?),
					last_event_at: Some(row.get(4)?),
					last_event_at_unix: Some(row.get(5)?),
				},
			))
		})?;

		for row in rows {
			let (run_id, summary) = row?;

			state.event_summaries.insert(run_id, summary);
		}

		Ok(())
	}

	fn load_worktrees(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self
			.connection
			.prepare("SELECT issue_id, project_id, branch_name, worktree_path FROM worktrees")?;
		let rows = statement.query_map([], |row| {
			let issue_id: String = row.get(0)?;

			Ok((
				issue_id.clone(),
				WorktreeMappingRecord {
					issue_id,
					project_id: row.get(1)?,
					branch_name: row.get(2)?,
					worktree_path: PathBuf::from(row.get::<_, String>(3)?),
				},
			))
		})?;

		for row in rows {
			let (issue_id, mapping) = row?;

			state.worktrees.insert(issue_id, mapping);
		}

		Ok(())
	}

	fn load_linear_execution_events(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT payload_json, event_unix, recorded_at, recorded_at_unix \
			 FROM linear_execution_events",
		)?;
		let rows = statement.query_map([], |row| {
			Ok((
				row.get::<_, String>(0)?,
				row.get::<_, Option<i64>>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, i64>(3)?,
			))
		})?;

		for row in rows {
			let (payload_json, event_unix, recorded_at, recorded_at_unix) = row?;
			let record = serde_json::from_str::<LinearExecutionEventRecord>(&payload_json)?;
			let record = LinearExecutionEventRuntimeRecord {
				record,
				event_unix,
				recorded_at,
				recorded_at_unix,
			};

			state
				.linear_execution_events
				.insert(record.record.idempotency_key.clone(), record);
		}

		Ok(())
	}

	fn load_review_handoffs(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, branch_name, run_id, attempt_number, pr_url, \
			 target_base_ref_name, pr_head_ref_name, pr_head_oid, updated_at, updated_at_unix \
			 FROM review_handoffs",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let branch_name: String = row.get(2)?;
			let marker = ReviewHandoffMarker {
				run_id: row.get(3)?,
				attempt_number: row.get(4)?,
				branch_name: branch_name.clone(),
				pr_url: row.get(5)?,
				target_base_ref_name: row.get(6)?,
				pr_head_ref_name: row.get(7)?,
				pr_head_oid: row.get(8)?,
			};

			Ok((
				ReviewMarkerKey::new(&project_id, &issue_id, &branch_name),
				ReviewHandoffRuntimeRecord {
					project_id,
					issue_id,
					branch_name,
					marker,
					updated_at: row.get(9)?,
					updated_at_unix: row.get(10)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_handoffs.insert(key, record);
		}

		Ok(())
	}

	fn load_review_orchestrations(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, branch_name, run_id, attempt_number, pr_url, head_sha, \
			 phase, request_comment_database_id, request_created_at_unix_epoch, \
			 request_description_thumbs_up_count, request_retry_count, external_round_count, \
			 auto_merge_enabled_at_unix_epoch, updated_at, updated_at_unix \
			 FROM review_orchestrations",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let branch_name: String = row.get(2)?;
			let run_id: String = row.get(3)?;
			let attempt_number: i64 = row.get(4)?;
			let request_description_thumbs_up_count = row
				.get::<_, Option<i64>>(10)?
				.and_then(|count| usize::try_from(count).ok());
			let marker = ReviewOrchestrationMarker::new(
				run_id.clone(),
				attempt_number,
				branch_name.clone(),
				row.get::<_, String>(5)?,
				row.get::<_, String>(6)?,
				row.get::<_, String>(7)?,
				row.get(8)?,
				row.get(9)?,
				request_description_thumbs_up_count,
				row.get(11)?,
				row.get(12)?,
				row.get(13)?,
			);

			Ok((
				ReviewOrchestrationKey::new(
					&project_id,
					&issue_id,
					&branch_name,
					&run_id,
					attempt_number,
				),
				ReviewOrchestrationRuntimeRecord {
					project_id,
					issue_id,
					branch_name,
					run_id,
					attempt_number,
					marker,
					updated_at: row.get(14)?,
					updated_at_unix: row.get(15)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_orchestrations.insert(key, record);
		}

		Ok(())
	}
}

struct TimestampParts {
	text: String,
	unix: i64,
}

#[derive(Clone, Debug)]
struct RunAttemptRecord {
	run_id: String,
	project_id: Option<String>,
	issue_id: String,
	attempt_number: i64,
	status: String,
	thread_id: Option<String>,
	turn_id: Option<String>,
	updated_at: String,
	updated_at_unix: i64,
}
impl RunAttemptRecord {
	fn as_public(&self) -> RunAttempt {
		RunAttempt {
			run_id: self.run_id.clone(),
			issue_id: self.issue_id.clone(),
			attempt_number: self.attempt_number,
			status: self.status.clone(),
			thread_id: self.thread_id.clone(),
			turn_id: self.turn_id.clone(),
		}
	}
}

#[derive(Clone, Debug)]
struct ProtocolEventRecord {
	sequence_number: i64,
	event_type: String,
	created_at: String,
	created_at_unix: i64,
}

#[derive(Clone, Debug, Default)]
struct ProtocolEventSummaryRecord {
	event_count: i64,
	last_sequence_number: Option<i64>,
	last_event_type: Option<String>,
	last_event_at: Option<String>,
	last_event_at_unix: Option<i64>,
}
impl ProtocolEventSummaryRecord {
	fn record_event(&mut self, event: &ProtocolEventRecord) {
		self.event_count += 1;

		if self
			.last_sequence_number
			.is_none_or(|sequence_number| event.sequence_number >= sequence_number)
		{
			self.last_sequence_number = Some(event.sequence_number);
			self.last_event_type = Some(event.event_type.clone());
			self.last_event_at = Some(event.created_at.clone());
			self.last_event_at_unix = Some(event.created_at_unix);
		}
	}
}

#[derive(Clone, Debug)]
struct LinearExecutionEventRuntimeRecord {
	record: LinearExecutionEventRecord,
	event_unix: Option<i64>,
	recorded_at: String,
	recorded_at_unix: i64,
}

#[derive(Clone, Debug)]
struct WorktreeMappingRecord {
	project_id: String,
	issue_id: String,
	branch_name: String,
	worktree_path: PathBuf,
}
impl WorktreeMappingRecord {
	fn as_public(&self) -> WorktreeMapping {
		WorktreeMapping {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			branch_name: self.branch_name.clone(),
			worktree_path: self.worktree_path.clone(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ReviewMarkerKey {
	project_id: String,
	issue_id: String,
	branch_name: String,
}
impl ReviewMarkerKey {
	fn new(project_id: &str, issue_id: &str, branch_name: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			branch_name: branch_name.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ReviewOrchestrationKey {
	project_id: String,
	issue_id: String,
	branch_name: String,
	run_id: String,
	attempt_number: i64,
}
impl ReviewOrchestrationKey {
	fn new(
		project_id: &str,
		issue_id: &str,
		branch_name: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			branch_name: branch_name.to_owned(),
			run_id: run_id.to_owned(),
			attempt_number,
		}
	}
}

#[derive(Clone, Debug)]
struct ReviewHandoffRuntimeRecord {
	project_id: String,
	issue_id: String,
	branch_name: String,
	marker: ReviewHandoffMarker,
	updated_at: String,
	updated_at_unix: i64,
}

#[derive(Clone, Debug)]
struct ReviewOrchestrationRuntimeRecord {
	project_id: String,
	issue_id: String,
	branch_name: String,
	run_id: String,
	attempt_number: i64,
	marker: ReviewOrchestrationMarker,
	updated_at: String,
	updated_at_unix: i64,
}

#[derive(Clone, Default)]
struct RunActivityMarkerRecord {
	run_id: Option<String>,
	attempt_number: Option<i64>,
	process_id: Option<u32>,
	last_activity_unix_epoch: Option<i64>,
	last_protocol_activity_unix_epoch: Option<i64>,
	last_progress_unix_epoch: Option<i64>,
	current_operation: Option<String>,
	thread_id: Option<String>,
	turn_id: Option<String>,
	thread_status: Option<String>,
	thread_active_flags: Vec<String>,
	event_count: Option<i64>,
	last_event_type: Option<String>,
	effective_model: Option<String>,
	effective_model_provider: Option<String>,
	effective_cwd: Option<String>,
	effective_approval_policy: Option<String>,
	effective_approvals_reviewer: Option<String>,
	effective_sandbox_mode: Option<String>,
	child_agent_activity: Option<ChildAgentActivitySummary>,
	protocol_activity: Option<ProtocolActivitySummary>,
	account: Option<CodexAccountActivitySummary>,
	accounts: Vec<CodexAccountActivitySummary>,
	retry_budget_attempt_count: Option<i64>,
	retry_kind: Option<String>,
	retry_ready_at_unix_epoch: Option<i64>,
	review_policy_phase: Option<String>,
	review_policy_status: Option<String>,
	review_policy_head_sha: Option<String>,
	review_policy_nonclean_rounds: Option<i64>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum GuardRetention {
	Local,
	ParentAfterHandoff,
	AdoptingChild,
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<()> {
	write_run_activity_marker_for_process(worktree_path, run_id, attempt_number, process::id())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn write_run_activity_marker_for_process(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	process_id: u32,
) -> Result<()> {
	write_run_activity_marker_at(
		worktree_path,
		run_id,
		attempt_number,
		process_id,
		OffsetDateTime::now_utc().unix_timestamp(),
		None,
	)
}

pub(crate) fn write_run_operation_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	current_operation: &str,
) -> Result<()> {
	write_run_operation_marker_for_process(
		worktree_path,
		run_id,
		attempt_number,
		process::id(),
		current_operation,
	)
}

pub(crate) fn write_run_operation_marker_for_process(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	process_id: u32,
	current_operation: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let now = OffsetDateTime::now_utc().unix_timestamp();
	let existing_marker = read_run_activity_marker_record(worktree_path)?;
	let mut marker = run_activity_marker_record_for_attempt(existing_marker.as_ref(), run_id, attempt_number);

	marker.process_id = Some(process_id);
	marker.last_activity_unix_epoch = Some(now);
	marker.last_progress_unix_epoch = Some(now);
	marker.current_operation = Some(current_operation.to_owned());

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_operation_marker_preserving_activity(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	current_operation: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let existing_marker = read_run_activity_marker_record(worktree_path)?;
	let mut marker = run_activity_marker_record_for_attempt(existing_marker.as_ref(), run_id, attempt_number);

	marker.current_operation = Some(current_operation.to_owned());

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_protocol_activity_marker(
	worktree_path: &Path,
	activity: &ProtocolActivityMarker<'_>,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let now = OffsetDateTime::now_utc().unix_timestamp();
	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(activity.run_id.to_owned());
	marker.attempt_number = Some(activity.attempt_number);

	marker.process_id.get_or_insert_with(process::id);

	marker.last_activity_unix_epoch = Some(now);
	marker.last_protocol_activity_unix_epoch = Some(now);
	marker.last_progress_unix_epoch = Some(now);
	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = activity.thread_id.map(str::to_owned).or(marker.thread_id);
	marker.turn_id = activity.turn_id.map(str::to_owned).or(marker.turn_id);
	marker.event_count = Some(activity.event_count);
	marker.last_event_type = Some(activity.last_event_type.to_owned());
	marker.child_agent_activity = activity.child_agent_activity.cloned();
	marker.protocol_activity = activity.protocol_activity.cloned();

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_account_marker(
	worktree_path: &Path,
	account: &CodexAccountMarker<'_>,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(account.run_id.to_owned());
	marker.attempt_number = Some(account.attempt_number);

	marker.process_id.get_or_insert_with(process::id);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.account = Some(account.account.clone());
	marker.accounts = normalize_accounts(account.account, account.accounts);

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_thread_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	marker.process_id.get_or_insert_with(process::id);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = Some(thread_id.to_owned());

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_turn_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	turn_id: &str,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	marker.process_id.get_or_insert_with(process::id);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.turn_id = Some(turn_id.to_owned());

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_thread_status_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	thread_id: Option<&str>,
	turn_id: Option<&str>,
	thread_status: &str,
	thread_active_flags: &[String],
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	marker.process_id.get_or_insert_with(process::id);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = thread_id.map(str::to_owned).or(marker.thread_id);
	marker.turn_id = turn_id.map(str::to_owned).or(marker.turn_id);
	marker.thread_status = Some(thread_status.to_owned());
	marker.thread_active_flags = thread_active_flags.to_vec();

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_effective_runtime_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	runtime: &EffectiveRuntimeMarker<'_>,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	marker.process_id.get_or_insert_with(process::id);

	marker.current_operation = Some(RUN_OPERATION_AGENT_RUN.to_owned());
	marker.thread_id = runtime.thread_id.map(str::to_owned).or(marker.thread_id);
	marker.turn_id = runtime.turn_id.map(str::to_owned).or(marker.turn_id);
	marker.effective_model = Some(runtime.effective_model.to_owned());
	marker.effective_model_provider = Some(runtime.effective_model_provider.to_owned());
	marker.effective_cwd = Some(runtime.effective_cwd.to_owned());
	marker.effective_approval_policy = Some(runtime.effective_approval_policy.to_owned());
	marker.effective_approvals_reviewer = Some(runtime.effective_approvals_reviewer.to_owned());
	marker.effective_sandbox_mode = Some(runtime.effective_sandbox_mode.to_owned());

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn read_run_activity_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<Option<i64>> {
	let marker = read_run_activity_marker_record(worktree_path)?.filter(|marker| {
		marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number)
	});

	Ok(marker.and_then(|marker| marker.last_activity_unix_epoch))
}

pub(crate) fn read_run_protocol_activity_marker(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
) -> Result<Option<i64>> {
	let marker = read_run_activity_marker_record(worktree_path)?.filter(|marker| {
		marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number)
	});

	Ok(marker.and_then(|marker| marker.last_protocol_activity_unix_epoch))
}

pub(crate) fn write_run_retry_budget_attempt_count(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	retry_budget_attempt_count: i64,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	marker.process_id.get_or_insert_with(process::id);

	marker.retry_budget_attempt_count = Some(retry_budget_attempt_count);

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_retry_schedule(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	retry_kind: &str,
	retry_ready_at_unix_epoch: i64,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);
	marker.retry_kind = Some(retry_kind.to_owned());
	marker.retry_ready_at_unix_epoch = Some(retry_ready_at_unix_epoch);

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn clear_run_retry_schedule(worktree_path: &Path) -> Result<()> {
	let Some(mut marker) = read_run_activity_marker_record(worktree_path)? else {
		return Ok(());
	};

	marker.retry_kind = None;
	marker.retry_ready_at_unix_epoch = None;

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn write_run_review_policy_state(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	review_policy_phase: &str,
	review_policy_status: &str,
	review_policy_head_sha: &str,
	review_policy_nonclean_rounds: i64,
) -> Result<()> {
	fs::create_dir_all(worktree_path)?;

	let mut marker = read_run_activity_marker_record(worktree_path)?.unwrap_or_default();

	marker.run_id = Some(run_id.to_owned());
	marker.attempt_number = Some(attempt_number);

	marker.process_id.get_or_insert_with(process::id);

	marker.review_policy_phase = Some(review_policy_phase.to_owned());
	marker.review_policy_status = Some(review_policy_status.to_owned());
	marker.review_policy_head_sha = Some(review_policy_head_sha.to_owned());
	marker.review_policy_nonclean_rounds = Some(review_policy_nonclean_rounds);

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn clear_run_review_policy_state(worktree_path: &Path) -> Result<()> {
	let Some(mut marker) = read_run_activity_marker_record(worktree_path)? else {
		return Ok(());
	};

	marker.review_policy_phase = None;
	marker.review_policy_status = None;
	marker.review_policy_head_sha = None;
	marker.review_policy_nonclean_rounds = None;

	write_run_activity_marker_record(worktree_path, &marker)?;

	Ok(())
}

pub(crate) fn read_run_retry_budget_attempt_count(worktree_path: &Path) -> Result<Option<i64>> {
	Ok(read_run_activity_marker_record(worktree_path)?
		.and_then(|marker| marker.retry_budget_attempt_count))
}

pub(crate) fn read_run_activity_marker_snapshot(
	worktree_path: &Path,
) -> Result<Option<RunActivityMarker>> {
	Ok(read_run_activity_marker_record(worktree_path)?.and_then(|marker| {
		let accounts = accounts_from_marker_record(&marker);

		Some(RunActivityMarker {
			run_id: marker.run_id?,
			attempt_number: marker.attempt_number?,
			process_id: marker.process_id,
			last_activity_unix_epoch: marker.last_activity_unix_epoch,
			last_protocol_activity_unix_epoch: marker.last_protocol_activity_unix_epoch,
			last_progress_unix_epoch: marker.last_progress_unix_epoch,
			current_operation: marker.current_operation,
			thread_id: marker.thread_id,
			turn_id: marker.turn_id,
			thread_status: marker.thread_status,
			thread_active_flags: marker.thread_active_flags,
			event_count: marker.event_count,
			last_event_type: marker.last_event_type,
			effective_model: marker.effective_model,
			effective_model_provider: marker.effective_model_provider,
			effective_cwd: marker.effective_cwd,
			effective_approval_policy: marker.effective_approval_policy,
			effective_approvals_reviewer: marker.effective_approvals_reviewer,
			effective_sandbox_mode: marker.effective_sandbox_mode,
			child_agent_activity: marker.child_agent_activity,
			protocol_activity: marker.protocol_activity,
			account: marker.account,
			accounts,
			retry_budget_attempt_count: marker.retry_budget_attempt_count,
			retry_kind: marker.retry_kind,
			retry_ready_at_unix_epoch: marker.retry_ready_at_unix_epoch,
			review_policy_phase: marker.review_policy_phase,
			review_policy_status: marker.review_policy_status,
			review_policy_head_sha: marker.review_policy_head_sha,
			review_policy_nonclean_rounds: marker.review_policy_nonclean_rounds,
		})
	}))
}

fn normalize_accounts(
	selected: &CodexAccountActivitySummary,
	accounts: &[CodexAccountActivitySummary],
) -> Vec<CodexAccountActivitySummary> {
	let mut normalized = if accounts.is_empty() {
		vec![selected.clone()]
	} else {
		accounts.to_vec()
	};

	if !normalized.iter().any(|account| {
		account.account_fingerprint == selected.account_fingerprint
	}) {
		normalized.insert(0, selected.clone());
	}

	normalized
}

fn accounts_from_marker_record(
	marker: &RunActivityMarkerRecord,
) -> Vec<CodexAccountActivitySummary> {
	if marker.accounts.is_empty() {
		marker.account.iter().cloned().collect()
	} else {
		marker.accounts.clone()
	}
}

fn persist_projects(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for project in state.projects.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO projects (
					service_id, config_path, repo_root, worktree_root, workflow_path,
					tracker_api_key_env_var, github_token_env_var, enabled, config_fingerprint,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				project.service_id(),
				project.config_path().to_string_lossy().as_ref(),
				project.repo_root().to_string_lossy().as_ref(),
				project.worktree_root().to_string_lossy().as_ref(),
				project.workflow_path().to_string_lossy().as_ref(),
				project.tracker_api_key_env_var(),
				project.github_token_env_var(),
				if project.enabled() { 1_i64 } else { 0_i64 },
				project.config_fingerprint(),
				project.updated_at(),
				project.updated_at_unix(),
			],
		)?;
	}

	Ok(())
}

fn persist_leases(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for lease in state.leases.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO leases (issue_id, project_id, run_id, issue_state) \
				 VALUES (?1, ?2, ?3, ?4)",
			params![lease.issue_id(), lease.project_id(), lease.run_id(), lease.issue_state()],
		)?;
	}

	Ok(())
}

fn persist_run_attempts(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for attempt in state.run_attempts.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO run_attempts (
					run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&attempt.run_id,
				attempt.project_id.as_deref(),
				&attempt.issue_id,
				attempt.attempt_number,
				&attempt.status,
				attempt.thread_id.as_deref(),
				attempt.turn_id.as_deref(),
				&attempt.updated_at,
				attempt.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

fn persist_protocol_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for (run_id, events) in &state.events {
		for event in events {
			transaction.execute(
				"INSERT OR REPLACE INTO protocol_events (
						run_id, sequence_number, event_type, created_at, created_at_unix
					) VALUES (?1, ?2, ?3, ?4, ?5)",
				params![
					run_id,
					event.sequence_number,
					&event.event_type,
					&event.created_at,
					event.created_at_unix,
				],
			)?;
		}
	}

	Ok(())
}

fn persist_worktrees(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for mapping in state.worktrees.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO worktrees (issue_id, project_id, branch_name, worktree_path) \
				 VALUES (?1, ?2, ?3, ?4)",
			params![
				&mapping.issue_id,
				&mapping.project_id,
				&mapping.branch_name,
				mapping.worktree_path.to_string_lossy().as_ref(),
			],
		)?;
	}

	Ok(())
}

fn persist_linear_execution_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.linear_execution_events.values() {
		let payload_json = serde_json::to_string(&record.record)?;

		transaction.execute(
			"INSERT OR REPLACE INTO linear_execution_events (
					idempotency_key, service_id, issue_id, event_type, event_timestamp,
					event_unix, payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&record.record.idempotency_key,
				&record.record.service_id,
				&record.record.issue_id,
				&record.record.event_type,
				&record.record.event_timestamp,
				record.event_unix,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;
	}

	Ok(())
}

fn persist_review_handoffs(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for record in state.review_handoffs.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO review_handoffs (
					project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				record.project_id,
				record.issue_id,
				record.branch_name,
				record.marker.run_id,
				record.marker.attempt_number,
				record.marker.pr_url,
				record.marker.target_base_ref_name,
				record.marker.pr_head_ref_name,
				record.marker.pr_head_oid,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

fn persist_review_orchestrations(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for record in state.review_orchestrations.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO review_orchestrations (
					project_id, issue_id, branch_name, run_id, attempt_number, pr_url, head_sha,
					phase, request_comment_database_id, request_created_at_unix_epoch,
					request_description_thumbs_up_count, request_retry_count, external_round_count,
					auto_merge_enabled_at_unix_epoch, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
			params![
				record.project_id,
				record.issue_id,
				record.branch_name,
				record.run_id,
				record.attempt_number,
				record.marker.pr_url,
				record.marker.head_sha,
				record.marker.phase,
				record.marker.request_comment_database_id,
				record.marker.request_created_at_unix_epoch,
				record
					.marker
					.request_description_thumbs_up_count
					.and_then(|count| i64::try_from(count).ok()),
				record.marker.request_retry_count,
				record.marker.external_round_count,
				record.marker.auto_merge_enabled_at_unix_epoch,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

fn dispatch_slot_lock_path(root: &Path, slot_index: usize) -> PathBuf {
	root.join(format!("{DISPATCH_SLOT_LOCK_FILE_PREFIX}.{slot_index}.lock"))
}

fn issue_claim_lock_path(root: &Path, issue_id: &str) -> PathBuf {
	root.join(format!("{ISSUE_CLAIM_LOCK_FILE_PREFIX}.{issue_id}.lock"))
}

fn issue_claim_id_from_path(path: &Path) -> Option<String> {
	let file_name = path.file_name()?.to_str()?;

	file_name
		.strip_prefix(&format!("{ISSUE_CLAIM_LOCK_FILE_PREFIX}."))
		.and_then(|suffix| suffix.strip_suffix(".lock"))
		.map(str::to_owned)
}

fn write_issue_claim_record(
	lock_file: &mut File,
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	issue_state: &str,
) -> Result<()> {
	lock_file.set_len(0)?;
	lock_file.seek(SeekFrom::Start(0))?;

	write!(
		lock_file,
		"project_id={project_id}\nissue_id={issue_id}\nrun_id={run_id}\nissue_state={issue_state}\n"
	)?;

	lock_file.flush()?;

	Ok(())
}

fn read_issue_claim_record(path: &Path) -> Result<Option<IssueLease>> {
	let mut body = String::new();
	let mut file = File::open(path)?;

	file.read_to_string(&mut body)?;

	if body.trim().is_empty() {
		return Ok(None);
	}

	let mut project_id = None;
	let mut issue_id = None;
	let mut run_id = None;
	let mut issue_state = None;

	for line in body.lines().filter(|line| !line.trim().is_empty()) {
		let (key, value) = line
			.split_once('=')
			.ok_or_else(|| eyre::eyre!("issue claim record `{}` is malformed", path.display()))?;

		match key {
			"project_id" => project_id = Some(value.to_owned()),
			"issue_id" => issue_id = Some(value.to_owned()),
			"run_id" => run_id = Some(value.to_owned()),
			"issue_state" => issue_state = Some(value.to_owned()),
			_ => {},
		}
	}

	let Some(project_id) = project_id else {
		return Err(eyre::eyre!("issue claim record `{}` is missing project_id", path.display()));
	};
	let Some(issue_id) = issue_id else {
		return Err(eyre::eyre!("issue claim record `{}` is missing issue_id", path.display()));
	};
	let Some(run_id) = run_id else {
		return Err(eyre::eyre!("issue claim record `{}` is missing run_id", path.display()));
	};
	let Some(issue_state) = issue_state else {
		return Err(eyre::eyre!("issue claim record `{}` is missing issue_state", path.display()));
	};

	Ok(Some(IssueLease { project_id, issue_id, run_id, issue_state }))
}

#[cfg_attr(not(test), allow(dead_code))]
fn write_run_activity_marker_at(
	worktree_path: &Path,
	run_id: &str,
	attempt_number: i64,
	process_id: u32,
	last_activity_unix_epoch: i64,
	last_protocol_activity_unix_epoch: Option<i64>,
) -> Result<()> {
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let existing_marker = read_run_activity_marker_record(worktree_path)?;
	let same_run_marker = existing_marker
		.as_ref()
		.filter(|marker| marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number));
	let mut marker = run_activity_marker_record_for_attempt(existing_marker.as_ref(), run_id, attempt_number);

	marker.process_id = Some(process_id);
	marker.last_activity_unix_epoch = Some(last_activity_unix_epoch);
	marker.last_protocol_activity_unix_epoch = last_protocol_activity_unix_epoch
		.or_else(|| same_run_marker.and_then(|marker| marker.last_protocol_activity_unix_epoch));

	if let Some(same_run_marker) = same_run_marker {
		marker.retry_kind = same_run_marker.retry_kind.clone();
		marker.retry_ready_at_unix_epoch = same_run_marker.retry_ready_at_unix_epoch;
	}

	fs::write(marker_path, serialize_run_activity_marker_record(&marker))?;

	Ok(())
}

fn run_activity_marker_record_for_attempt(
	existing_marker: Option<&RunActivityMarkerRecord>,
	run_id: &str,
	attempt_number: i64,
) -> RunActivityMarkerRecord {
	let same_run_marker = existing_marker
		.filter(|marker| marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number));

	RunActivityMarkerRecord {
		run_id: Some(run_id.to_owned()),
		attempt_number: Some(attempt_number),
		process_id: same_run_marker.and_then(|marker| marker.process_id),
		last_activity_unix_epoch: same_run_marker.and_then(|marker| marker.last_activity_unix_epoch),
		last_protocol_activity_unix_epoch: same_run_marker
			.and_then(|marker| marker.last_protocol_activity_unix_epoch),
		last_progress_unix_epoch: same_run_marker.and_then(|marker| marker.last_progress_unix_epoch),
		current_operation: same_run_marker.and_then(|marker| marker.current_operation.clone()),
		thread_id: same_run_marker.and_then(|marker| marker.thread_id.clone()),
		turn_id: same_run_marker.and_then(|marker| marker.turn_id.clone()),
		thread_status: same_run_marker.and_then(|marker| marker.thread_status.clone()),
		thread_active_flags: same_run_marker
			.map(|marker| marker.thread_active_flags.clone())
			.unwrap_or_default(),
		event_count: same_run_marker.and_then(|marker| marker.event_count),
		last_event_type: same_run_marker.and_then(|marker| marker.last_event_type.clone()),
		effective_model: same_run_marker.and_then(|marker| marker.effective_model.clone()),
		effective_model_provider: same_run_marker
			.and_then(|marker| marker.effective_model_provider.clone()),
		effective_cwd: same_run_marker.and_then(|marker| marker.effective_cwd.clone()),
		effective_approval_policy: same_run_marker
			.and_then(|marker| marker.effective_approval_policy.clone()),
		effective_approvals_reviewer: same_run_marker
			.and_then(|marker| marker.effective_approvals_reviewer.clone()),
		effective_sandbox_mode: same_run_marker
			.and_then(|marker| marker.effective_sandbox_mode.clone()),
		child_agent_activity: same_run_marker
			.and_then(|marker| marker.child_agent_activity.clone()),
		protocol_activity: same_run_marker.and_then(|marker| marker.protocol_activity.clone()),
		account: same_run_marker.and_then(|marker| marker.account.clone()),
		accounts: same_run_marker.map(|marker| marker.accounts.clone()).unwrap_or_default(),
		retry_budget_attempt_count: existing_marker
			.and_then(|marker| marker.retry_budget_attempt_count),
		retry_kind: same_run_marker.and_then(|marker| marker.retry_kind.clone()),
		retry_ready_at_unix_epoch: same_run_marker.and_then(|marker| marker.retry_ready_at_unix_epoch),
		review_policy_phase: existing_marker.and_then(|marker| marker.review_policy_phase.clone()),
		review_policy_status: existing_marker.and_then(|marker| marker.review_policy_status.clone()),
		review_policy_head_sha: existing_marker.and_then(|marker| marker.review_policy_head_sha.clone()),
		review_policy_nonclean_rounds: existing_marker.and_then(|marker| marker.review_policy_nonclean_rounds),
	}
}

fn read_run_activity_marker_record(
	worktree_path: &Path,
) -> Result<Option<RunActivityMarkerRecord>> {
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let marker_body = match fs::read_to_string(&marker_path) {
		Ok(body) => body,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
		Err(error) => return Err(error.into()),
	};
	let mut marker = RunActivityMarkerRecord::default();

	for line in marker_body.lines() {
		let Some((key, value)) = line.split_once('=') else {
			continue;
		};

		match key {
			"run_id" => marker.run_id = Some(value.to_owned()),
			"attempt_number" => marker.attempt_number = value.parse::<i64>().ok(),
			"process_id" => marker.process_id = value.parse::<u32>().ok(),
			"last_activity_unix_epoch" =>
				marker.last_activity_unix_epoch = value.parse::<i64>().ok(),
			"last_protocol_activity_unix_epoch" =>
				marker.last_protocol_activity_unix_epoch = value.parse::<i64>().ok(),
			"last_progress_unix_epoch" =>
				marker.last_progress_unix_epoch = value.parse::<i64>().ok(),
			"current_operation" => marker.current_operation = Some(value.to_owned()),
			"thread_id" => marker.thread_id = Some(value.to_owned()),
			"turn_id" => marker.turn_id = Some(value.to_owned()),
			"thread_status" => marker.thread_status = Some(value.to_owned()),
			"thread_active_flags" => marker.thread_active_flags = parse_marker_list(value),
			"event_count" => marker.event_count = value.parse::<i64>().ok(),
			"last_event_type" => marker.last_event_type = Some(value.to_owned()),
			"effective_model" => marker.effective_model = Some(value.to_owned()),
			"effective_model_provider" =>
				marker.effective_model_provider = Some(value.to_owned()),
			"effective_cwd" => marker.effective_cwd = Some(value.to_owned()),
			"effective_approval_policy" =>
				marker.effective_approval_policy = Some(value.to_owned()),
			"effective_approvals_reviewer" =>
				marker.effective_approvals_reviewer = Some(value.to_owned()),
			"effective_sandbox_mode" => marker.effective_sandbox_mode = Some(value.to_owned()),
			"child_agent_activity" =>
				marker.child_agent_activity = serde_json::from_str(value).ok(),
			"protocol_activity" => marker.protocol_activity = serde_json::from_str(value).ok(),
			"account" => marker.account = serde_json::from_str(value).ok(),
			"accounts" => {
				if let Ok(accounts) = serde_json::from_str(value) {
					marker.accounts = accounts;
				}
			},
			"retry_budget_attempt_count" =>
				marker.retry_budget_attempt_count = value.parse::<i64>().ok(),
			"retry_kind" => marker.retry_kind = Some(value.to_owned()),
			"retry_ready_at_unix_epoch" =>
				marker.retry_ready_at_unix_epoch = value.parse::<i64>().ok(),
			"review_policy_phase" => marker.review_policy_phase = Some(value.to_owned()),
			"review_policy_status" => marker.review_policy_status = Some(value.to_owned()),
			"review_policy_head_sha" => marker.review_policy_head_sha = Some(value.to_owned()),
			"review_policy_nonclean_rounds" =>
				marker.review_policy_nonclean_rounds = value.parse::<i64>().ok(),
			_ => {},
		}
	}

	Ok(Some(marker))
}

fn write_run_activity_marker_record(
	worktree_path: &Path,
	marker: &RunActivityMarkerRecord,
) -> Result<()> {
	fs::write(
		worktree_path.join(RUN_ACTIVITY_MARKER_FILE),
		serialize_run_activity_marker_record(marker),
	)?;

	Ok(())
}

fn serialize_run_activity_marker_record(marker: &RunActivityMarkerRecord) -> String {
	let mut body = String::new();

	if let Some(run_id) = &marker.run_id {
		body.push_str(&format!("run_id={run_id}\n"));
	}
	if let Some(attempt_number) = marker.attempt_number {
		body.push_str(&format!("attempt_number={attempt_number}\n"));
	}
	if let Some(process_id) = marker.process_id {
		body.push_str(&format!("process_id={process_id}\n"));
	}
	if let Some(last_activity_unix_epoch) = marker.last_activity_unix_epoch {
		body.push_str(&format!("last_activity_unix_epoch={last_activity_unix_epoch}\n"));
	}
	if let Some(last_protocol_activity_unix_epoch) = marker.last_protocol_activity_unix_epoch {
		body.push_str(&format!(
			"last_protocol_activity_unix_epoch={last_protocol_activity_unix_epoch}\n"
		));
	}
	if let Some(last_progress_unix_epoch) = marker.last_progress_unix_epoch {
		body.push_str(&format!("last_progress_unix_epoch={last_progress_unix_epoch}\n"));
	}
	if let Some(current_operation) = &marker.current_operation {
		body.push_str(&format!("current_operation={current_operation}\n"));
	}
	if let Some(thread_id) = &marker.thread_id {
		body.push_str(&format!("thread_id={thread_id}\n"));
	}
	if let Some(turn_id) = &marker.turn_id {
		body.push_str(&format!("turn_id={turn_id}\n"));
	}
	if let Some(thread_status) = &marker.thread_status {
		body.push_str(&format!("thread_status={thread_status}\n"));
	}

	if !marker.thread_active_flags.is_empty() {
		body.push_str(&format!(
			"thread_active_flags={}\n",
			marker.thread_active_flags.join(",")
		));
	}

	if let Some(event_count) = marker.event_count {
		body.push_str(&format!("event_count={event_count}\n"));
	}
	if let Some(last_event_type) = &marker.last_event_type {
		body.push_str(&format!("last_event_type={last_event_type}\n"));
	}
	if let Some(effective_model) = &marker.effective_model {
		body.push_str(&format!("effective_model={effective_model}\n"));
	}
	if let Some(effective_model_provider) = &marker.effective_model_provider {
		body.push_str(&format!("effective_model_provider={effective_model_provider}\n"));
	}
	if let Some(effective_cwd) = &marker.effective_cwd {
		body.push_str(&format!("effective_cwd={effective_cwd}\n"));
	}
	if let Some(effective_approval_policy) = &marker.effective_approval_policy {
		body.push_str(&format!(
			"effective_approval_policy={effective_approval_policy}\n"
		));
	}
	if let Some(effective_approvals_reviewer) = &marker.effective_approvals_reviewer {
		body.push_str(&format!(
			"effective_approvals_reviewer={effective_approvals_reviewer}\n"
		));
	}
	if let Some(effective_sandbox_mode) = &marker.effective_sandbox_mode {
		body.push_str(&format!("effective_sandbox_mode={effective_sandbox_mode}\n"));
	}
	if let Some(child_agent_activity) = &marker.child_agent_activity
		&& let Ok(summary_json) = serde_json::to_string(child_agent_activity)
	{
		body.push_str(&format!("child_agent_activity={summary_json}\n"));
	}
	if let Some(protocol_activity) = &marker.protocol_activity
		&& let Ok(summary_json) = serde_json::to_string(protocol_activity)
	{
		body.push_str(&format!("protocol_activity={summary_json}\n"));
	}
	if let Some(account) = &marker.account
		&& let Ok(summary_json) = serde_json::to_string(account)
		{
			body.push_str(&format!("account={summary_json}\n"));
		}

		if !marker.accounts.is_empty()
			&& let Ok(accounts_json) = serde_json::to_string(&marker.accounts)
		{
			body.push_str(&format!("accounts={accounts_json}\n"));
		}

		if let Some(retry_budget_attempt_count) = marker.retry_budget_attempt_count {
			body.push_str(&format!("retry_budget_attempt_count={retry_budget_attempt_count}\n"));
		}
	if let Some(retry_kind) = &marker.retry_kind {
		body.push_str(&format!("retry_kind={retry_kind}\n"));
	}
	if let Some(retry_ready_at_unix_epoch) = marker.retry_ready_at_unix_epoch {
		body.push_str(&format!("retry_ready_at_unix_epoch={retry_ready_at_unix_epoch}\n"));
	}
	if let Some(review_policy_phase) = &marker.review_policy_phase {
		body.push_str(&format!("review_policy_phase={review_policy_phase}\n"));
	}
	if let Some(review_policy_status) = &marker.review_policy_status {
		body.push_str(&format!("review_policy_status={review_policy_status}\n"));
	}
	if let Some(review_policy_head_sha) = &marker.review_policy_head_sha {
		body.push_str(&format!("review_policy_head_sha={review_policy_head_sha}\n"));
	}
	if let Some(review_policy_nonclean_rounds) = marker.review_policy_nonclean_rounds {
		body.push_str(&format!("review_policy_nonclean_rounds={review_policy_nonclean_rounds}\n"));
	}

	body
}

fn parse_marker_list(value: &str) -> Vec<String> {
	value
		.split(',')
		.filter(|part| !part.is_empty())
		.map(str::to_owned)
		.collect()
}

fn timestamp_parts() -> TimestampParts {
	let now = OffsetDateTime::now_utc();

	TimestampParts {
		text: now.format(&Rfc3339).expect("timestamp formatting should succeed"),
		unix: now.unix_timestamp(),
	}
}

fn parse_linear_execution_event_unix(record: &LinearExecutionEventRecord) -> Option<i64> {
	OffsetDateTime::parse(&record.event_timestamp, &Rfc3339)
		.ok()
		.map(|timestamp| timestamp.unix_timestamp())
}

fn protocol_event_summary_from_events(events: &[ProtocolEventRecord]) -> ProtocolEventSummaryRecord {
	let mut summary = ProtocolEventSummaryRecord::default();

	for event in events {
		summary.record_event(event);
	}

	summary
}

fn compare_attempt_records(left: &RunAttemptRecord, right: &RunAttemptRecord) -> Ordering {
	left.attempt_number
		.cmp(&right.attempt_number)
		.then_with(|| left.updated_at_unix.cmp(&right.updated_at_unix))
		.then_with(|| left.run_id.cmp(&right.run_id))
}

fn compare_linear_execution_event_runtime_records(
	left: &LinearExecutionEventRuntimeRecord,
	right: &LinearExecutionEventRuntimeRecord,
) -> Ordering {
	left.event_unix
		.cmp(&right.event_unix)
		.then_with(|| left.recorded_at_unix.cmp(&right.recorded_at_unix))
		.then_with(|| left.record.idempotency_key.cmp(&right.record.idempotency_key))
}

fn compare_project_run_status(left: &ProjectRunStatus, right: &ProjectRunStatus) -> Ordering {
	right
		.active_lease
		.cmp(&left.active_lease)
		.then_with(|| right.updated_at.cmp(&left.updated_at))
		.then_with(|| right.attempt_number.cmp(&left.attempt_number))
		.then_with(|| right.run_id.cmp(&left.run_id))
}

#[cfg(unix)]
fn clear_close_on_exec(file: &File) -> Result<()> {
	let fd = file.as_raw_fd();
	let existing_flags = unsafe { libc::fcntl(fd, F_GETFD) };

	if existing_flags == -1 {
		return Err(Error::last_os_error().into());
	}

	let new_flags = existing_flags & !FD_CLOEXEC;

	if new_flags != existing_flags {
		let result = unsafe { libc::fcntl(fd, F_SETFD, new_flags) };

		if result == -1 {
			return Err(Error::last_os_error().into());
		}
	}

	Ok(())
}

#[cfg(unix)]
fn set_close_on_exec(file: &File) -> Result<()> {
	let fd = file.as_raw_fd();
	let existing_flags = unsafe { libc::fcntl(fd, F_GETFD) };

	if existing_flags == -1 {
		return Err(Error::last_os_error().into());
	}

	let new_flags = existing_flags | FD_CLOEXEC;

	if new_flags != existing_flags {
		let result = unsafe { libc::fcntl(fd, F_SETFD, new_flags) };

		if result == -1 {
			return Err(Error::last_os_error().into());
		}
	}

	Ok(())
}
