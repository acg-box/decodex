#[cfg(target_os = "macos")]
use std::mem::{self, MaybeUninit};
use std::sync::atomic::AtomicU64;
use std::env;

use libc::FD_CLOEXEC;
use libc::F_GETFD;
use libc::F_SETFD;
#[cfg(target_os = "macos")]
use libc::{
	c_void,
	proc_bsdinfo,
	PROC_PIDTBSDINFO,
};
#[cfg(target_os = "macos")]
use process::Command;
use rusqlite::{self, Row};

use crate::tracker;

static RUN_ACTIVITY_MARKER_WRITE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
	slot_limit: DispatchSlotLimit,
}

struct IssueClaimGuard {
	lock_path: PathBuf,
	lock_file: File,
	retention: GuardRetention,
}
impl IssueClaimGuard {
	fn lock_root(&self) -> Result<&Path> {
		lock_root_from_lock_path(&self.lock_path)
	}

	fn unlock(self) -> Result<()> {
		let Self { lock_path, lock_file, retention: _ } = self;

		lock_file.unlock()?;

		drop(lock_file);
		remove_lock_file_if_exists(&lock_path)?;

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
	lock_path: PathBuf,
	lock_file: File,
	retention: GuardRetention,
}
impl DispatchSlotGuard {
	fn lock_root(&self) -> Result<&Path> {
		lock_root_from_lock_path(&self.lock_path)
	}

	fn release_for_clear(self) -> Result<()> {
		match self.retention {
			GuardRetention::ParentAfterHandoff => Ok(()),
			GuardRetention::Local | GuardRetention::AdoptingChild => {
				let Self {
					project_id: _,
					slot_index: _,
					lock_path,
					lock_file,
					retention: _,
				} = self;

				lock_file.unlock()?;

				drop(lock_file);
				remove_lock_file_if_exists(&lock_path)?;

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
	control_channels: HashMap<String, RunControlChannelRecord>,
	events: HashMap<String, Vec<ProtocolEventRecord>>,
	event_summaries: HashMap<String, ProtocolEventSummaryRecord>,
	worktrees: HashMap<String, WorktreeMappingRecord>,
	linear_execution_events: HashMap<String, LinearExecutionEventRuntimeRecord>,
	private_execution_events: Vec<PrivateExecutionEventRuntimeRecord>,
	decision_contracts: HashMap<DecisionContractKey, DecisionContractRuntimeRecord>,
	execution_programs: HashMap<ExecutionProgramKey, ExecutionProgramRuntimeRecord>,
	program_intake_plans: HashMap<ProgramIntakePlanKey, ProgramIntakePlanRecord>,
	program_issue_mappings: HashMap<ProgramIssueMappingKey, ProgramIssueMappingRecord>,
	program_queue_label_ownership:
		HashMap<ProgramQueueLabelOwnershipKey, ProgramQueueLabelOwnershipRecord>,
	review_handoffs: HashMap<ReviewMarkerKey, ReviewHandoffRuntimeRecord>,
	review_orchestrations: HashMap<ReviewOrchestrationKey, ReviewOrchestrationRuntimeRecord>,
	review_policy_checkpoints: HashMap<ReviewPolicyKey, ReviewPolicyRuntimeRecord>,
	loop_guardrail_checkpoints: HashMap<LoopGuardrailKey, LoopGuardrailRuntimeRecord>,
	connector_backoffs: HashMap<(String, String), ConnectorBackoff>,
	dispatch_slot_configs: HashMap<String, DispatchSlotConfig>,
	issue_claim_guards: HashMap<String, IssueClaimGuard>,
	dispatch_slot_guards: HashMap<String, DispatchSlotGuard>,
}
impl StateData {
	fn replace_durable_state(&mut self, loaded: Self) {
		self.projects = loaded.projects;
		self.leases = loaded.leases;
		self.run_attempts = loaded.run_attempts;
		self.control_channels = loaded.control_channels;
		self.events = loaded.events;
		self.event_summaries = loaded.event_summaries;
		self.worktrees = loaded.worktrees;
		self.linear_execution_events = loaded.linear_execution_events;
		self.private_execution_events = loaded.private_execution_events;
		self.decision_contracts = loaded.decision_contracts;
		self.execution_programs = loaded.execution_programs;
		self.program_intake_plans = loaded.program_intake_plans;
		self.program_issue_mappings = loaded.program_issue_mappings;
		self.program_queue_label_ownership = loaded.program_queue_label_ownership;
		self.review_handoffs = loaded.review_handoffs;
		self.review_orchestrations = loaded.review_orchestrations;
		self.review_policy_checkpoints = loaded.review_policy_checkpoints;
		self.loop_guardrail_checkpoints = loaded.loop_guardrail_checkpoints;
		self.connector_backoffs = loaded.connector_backoffs;
	}

	fn replace_project_run_metadata_state(&mut self, loaded: Self) {
		self.leases = loaded.leases;
		self.run_attempts = loaded.run_attempts;
		self.control_channels = loaded.control_channels;
		self.worktrees = loaded.worktrees;
	}

	fn replace_project_loop_evidence_state(&mut self, project_id: &str, loaded: Self) {
		self.private_execution_events.retain(|record| record.project_id != project_id);
		self.private_execution_events.extend(loaded.private_execution_events);
		self.review_policy_checkpoints.retain(|key, _record| key.project_id != project_id);
		self.review_policy_checkpoints.extend(loaded.review_policy_checkpoints);
	}

	fn replace_project_registry_state(&mut self, loaded: Self) {
		self.projects = loaded.projects;
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
		let control_channel = self
			.control_channels
			.get(&attempt.run_id)
			.filter(|channel| {
				channel.project_id == project_id
					&& channel.issue_id == attempt.issue_id
					&& channel.attempt_number == attempt.attempt_number
			})
			.map(RunControlChannelRecord::as_public);

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
			control_channel,
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

	fn next_private_execution_event_id(&self) -> Result<i64> {
		self.private_execution_events
			.iter()
			.map(|record| record.record_id)
			.max()
			.unwrap_or(0)
			.checked_add(1)
			.ok_or_else(|| eyre::eyre!("Private execution event row id overflowed i64."))
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
CREATE INDEX IF NOT EXISTS run_attempts_issue_attempt_idx
ON run_attempts (issue_id, attempt_number, updated_at_unix, run_id);
CREATE TABLE IF NOT EXISTS protocol_events (
	run_id TEXT NOT NULL,
	sequence_number INTEGER NOT NULL,
	event_type TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	PRIMARY KEY (run_id, sequence_number)
);
CREATE TABLE IF NOT EXISTS protocol_event_summaries (
	run_id TEXT PRIMARY KEY NOT NULL,
	event_count INTEGER NOT NULL,
	last_sequence_number INTEGER,
	last_event_type TEXT,
	last_event_at TEXT,
	last_event_at_unix INTEGER,
	compacted_at TEXT NOT NULL,
	compacted_at_unix INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS worktrees (
	issue_id TEXT PRIMARY KEY NOT NULL,
	project_id TEXT NOT NULL,
	branch_name TEXT NOT NULL,
	worktree_path TEXT NOT NULL,
	provenance_source TEXT NOT NULL DEFAULT 'runtime_recorded',
	created_at_unix INTEGER,
	updated_at_unix INTEGER
);
CREATE INDEX IF NOT EXISTS worktrees_project_issue_idx
ON worktrees (project_id, issue_id);
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
"#,
		)?;
		self.bootstrap_worktree_schema()?;
		self.bootstrap_review_schema()?;
		self.bootstrap_run_control_channels_schema()?;
		self.bootstrap_connector_backoffs_schema()?;
		self.bootstrap_private_execution_events_schema()?;
		self.bootstrap_decision_contracts_schema()?;
		self.bootstrap_execution_programs_schema()?;
		self.bootstrap_program_intake_state_schema()?;
		self.bootstrap_loop_guardrail_schema()?;
		self.record_schema_version()?;

		Ok(())
	}

	fn bootstrap_worktree_schema(&self) -> Result<()> {
		self.ensure_column(
			"worktrees",
			"provenance_source",
			"ALTER TABLE worktrees ADD COLUMN provenance_source TEXT NOT NULL DEFAULT 'legacy_unknown'",
		)?;
		self.ensure_column(
			"worktrees",
			"created_at_unix",
			"ALTER TABLE worktrees ADD COLUMN created_at_unix INTEGER",
		)?;
		self.ensure_column(
			"worktrees",
			"updated_at_unix",
			"ALTER TABLE worktrees ADD COLUMN updated_at_unix INTEGER",
		)?;

		Ok(())
	}

	fn bootstrap_review_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
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
CREATE TABLE IF NOT EXISTS review_policy_checkpoints (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	phase TEXT NOT NULL,
	status TEXT NOT NULL,
	head_sha TEXT NOT NULL,
	nonclean_rounds INTEGER NOT NULL,
	details_json TEXT NOT NULL DEFAULT '{}',
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, run_id, attempt_number, phase)
);
"#,
		)?;
		self.ensure_column(
			"review_policy_checkpoints",
			"details_json",
			"ALTER TABLE review_policy_checkpoints ADD COLUMN details_json TEXT NOT NULL DEFAULT '{}'",
		)?;

		Ok(())
	}

	fn ensure_column(&self, table: &str, column: &str, add_column_sql: &str) -> Result<()> {
		let mut statement = self.connection.prepare(&format!("PRAGMA table_info({table})"))?;
		let column_names = statement.query_map([], |row| row.get::<_, String>(1))?;

		for column_name in column_names {
			if column_name? == column {
				return Ok(());
			}
		}

		self.connection.execute_batch(add_column_sql)?;

		Ok(())
	}

	fn bootstrap_run_control_channels_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS run_control_channels (
	run_id TEXT PRIMARY KEY NOT NULL,
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	transport TEXT NOT NULL,
	channel_path TEXT NOT NULL,
	status TEXT NOT NULL,
	published_at TEXT NOT NULL,
	published_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS run_control_channels_project_issue_idx
ON run_control_channels (project_id, issue_id, attempt_number);
"#,
		)?;

		Ok(())
	}

	fn bootstrap_loop_guardrail_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS loop_guardrail_checkpoints (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	reason TEXT NOT NULL,
	fingerprint TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	consecutive_count INTEGER NOT NULL,
	details_json TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, reason)
);
"#,
		)?;

		Ok(())
	}

	fn bootstrap_connector_backoffs_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS connector_backoffs (
	project_id TEXT NOT NULL,
	connector TEXT NOT NULL,
	sync_phase TEXT NOT NULL,
	quota_class TEXT NOT NULL,
	reset_unix_epoch INTEGER NOT NULL,
	reset_source TEXT NOT NULL,
	warning TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, connector)
);
"#,
		)?;

		Ok(())
	}

	fn bootstrap_private_execution_events_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS private_execution_events (
	record_id INTEGER PRIMARY KEY AUTOINCREMENT,
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	event_type TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	recorded_at TEXT NOT NULL,
	recorded_at_unix INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS private_execution_events_attempt_idx
ON private_execution_events (
	project_id, issue_id, run_id, attempt_number, record_id
);
"#,
		)?;

		Ok(())
	}

	fn bootstrap_decision_contracts_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS decision_contracts (
	project_id TEXT NOT NULL,
	contract_id TEXT NOT NULL,
	source_issue_id TEXT,
	status TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, contract_id)
);
CREATE INDEX IF NOT EXISTS decision_contracts_source_issue_idx
ON decision_contracts (project_id, source_issue_id, updated_at_unix);
CREATE INDEX IF NOT EXISTS decision_contracts_status_idx
ON decision_contracts (project_id, status, updated_at_unix);
"#,
		)?;

		Ok(())
	}

	fn bootstrap_execution_programs_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS execution_programs (
	project_id TEXT NOT NULL,
	program_id TEXT NOT NULL,
	source_contract_id TEXT,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, program_id)
);
CREATE INDEX IF NOT EXISTS execution_programs_source_contract_idx
ON execution_programs (project_id, source_contract_id, updated_at_unix);
"#,
		)?;
		self.ensure_execution_program_source_contract_nullable()?;

		Ok(())
	}

	fn ensure_execution_program_source_contract_nullable(&self) -> Result<()> {
		let mut statement = self.connection.prepare("PRAGMA table_info(execution_programs)")?;
		let columns = statement.query_map([], |row| {
			Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)?))
		})?;
		let mut source_contract_not_null = false;

		for column in columns {
			let (name, not_null) = column?;

			if name == "source_contract_id" {
				source_contract_not_null = not_null != 0;

				break;
			}
		}

		if !source_contract_not_null {
			return Ok(());
		}

		self.connection.execute_batch(
			r#"
ALTER TABLE execution_programs RENAME TO execution_programs_legacy_contract_required;
CREATE TABLE execution_programs (
	project_id TEXT NOT NULL,
	program_id TEXT NOT NULL,
	source_contract_id TEXT,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, program_id)
);
INSERT INTO execution_programs (
	project_id, program_id, source_contract_id, payload_json, created_at, created_at_unix,
	updated_at, updated_at_unix
)
SELECT project_id, program_id, source_contract_id, payload_json, created_at, created_at_unix,
	updated_at, updated_at_unix
FROM execution_programs_legacy_contract_required;
DROP TABLE execution_programs_legacy_contract_required;
CREATE INDEX IF NOT EXISTS execution_programs_source_contract_idx
ON execution_programs (project_id, source_contract_id, updated_at_unix);
"#,
		)?;

		Ok(())
	}

	fn bootstrap_program_intake_state_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS program_intake_plans (
	project_id TEXT NOT NULL,
	program_id TEXT NOT NULL,
	plan_id TEXT NOT NULL,
	intake_kind TEXT NOT NULL,
	source_contract_id TEXT,
	accepted_contract_fingerprint TEXT NOT NULL,
	public_summary TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, program_id, plan_id)
);
CREATE INDEX IF NOT EXISTS program_intake_plans_project_idx
ON program_intake_plans (project_id, intake_kind, updated_at_unix);
CREATE TABLE IF NOT EXISTS program_issue_mappings (
	project_id TEXT NOT NULL,
	program_id TEXT NOT NULL,
	node_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	issue_identifier TEXT NOT NULL,
	issue_state TEXT NOT NULL,
	queue_intent TEXT NOT NULL,
	has_queue_label INTEGER NOT NULL,
	queue_label_owned_by_program_reconciler INTEGER NOT NULL,
	has_active_label INTEGER NOT NULL,
	has_opt_out_label INTEGER NOT NULL,
	has_needs_attention_label INTEGER NOT NULL,
	has_generic_dispatch_briefing INTEGER NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, program_id, node_id)
);
CREATE INDEX IF NOT EXISTS program_issue_mappings_issue_idx
ON program_issue_mappings (project_id, issue_id, updated_at_unix);
CREATE TABLE IF NOT EXISTS program_queue_label_ownership (
	project_id TEXT NOT NULL,
	program_id TEXT NOT NULL,
	node_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	issue_identifier TEXT NOT NULL,
	label_name TEXT NOT NULL,
	service_id TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, program_id, node_id, label_name)
);
CREATE INDEX IF NOT EXISTS program_queue_label_ownership_issue_idx
ON program_queue_label_ownership (project_id, issue_id, label_name, updated_at_unix);
"#,
		)?;
		self.backfill_program_intake_state_from_execution_programs()?;

		Ok(())
	}

	fn backfill_program_intake_state_from_execution_programs(&self) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 ORDER BY project_id ASC, program_id ASC",
		)?;
		let rows = statement.query_map([], execution_program_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(execution_program_record_from_row_parts(row?)?);
		}

		drop(statement);

		for record in records {
			self.replace_program_intake_state(&record)?;
		}

		Ok(())
	}

	fn record_schema_version(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS schema_meta (
	key TEXT PRIMARY KEY NOT NULL,
	value TEXT NOT NULL
);
INSERT INTO schema_meta (key, value)
VALUES ('schema_version', '11')
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
		self.load_run_control_channels(&mut state)?;
		self.load_protocol_event_summaries(&mut state)?;
		self.load_worktrees(&mut state)?;
		self.load_linear_execution_events(&mut state)?;
		self.load_private_execution_events(&mut state)?;
		self.load_decision_contracts(&mut state)?;
		self.load_execution_programs(&mut state)?;
		self.load_program_intake_state(&mut state)?;
		self.load_review_handoffs(&mut state)?;
		self.load_review_orchestrations(&mut state)?;
		self.load_review_policy_checkpoints(&mut state)?;
		self.load_loop_guardrail_checkpoints(&mut state)?;
		self.load_connector_backoffs(&mut state)?;

		Ok(state)
	}

	fn load_project_run_metadata_for_project(&self, project_id: &str) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_leases(&mut state)?;
		self.load_run_attempts_for_project(&mut state, project_id)?;
		self.load_worktrees(&mut state)?;
		self.load_run_control_channels_for_project(&mut state, project_id)?;

		Ok(state)
	}

	fn load_project_loop_evidence_for_project(&self, project_id: &str) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_private_execution_events_for_project(&mut state, project_id)?;
		self.load_review_policy_checkpoints_for_project(&mut state, project_id)?;

		Ok(state)
	}

	fn load_project_registry_state(&self) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_projects(&mut state)?;

		Ok(state)
	}

	fn persist_runtime_state(&mut self, state: &StateData) -> Result<()> {
		let transaction = self.connection.transaction()?;

		persist_projects(&transaction, state)?;
		persist_leases(&transaction, state)?;
		persist_run_attempts(&transaction, state)?;
		persist_run_control_channels(&transaction, state)?;
		persist_protocol_events(&transaction, state)?;
		persist_worktrees(&transaction, state)?;
		persist_linear_execution_events(&transaction, state)?;
		persist_private_execution_events(&transaction, state)?;
		persist_decision_contracts(&transaction, state)?;
		persist_execution_programs(&transaction, state)?;
		persist_program_intake_state(&transaction, state)?;
		persist_review_handoffs(&transaction, state)?;
		persist_review_orchestrations(&transaction, state)?;
		persist_review_policy_checkpoints(&transaction, state)?;
		persist_loop_guardrail_checkpoints(&transaction, state)?;
		persist_connector_backoffs(&transaction, state)?;

		transaction.commit()?;

		Ok(())
	}

	fn delete_project(&mut self, service_id: &str) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute("DELETE FROM projects WHERE service_id = ?1", params![service_id])?;
		transaction.execute(
			"DELETE FROM connector_backoffs WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM run_control_channels WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM decision_contracts WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM execution_programs WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM program_intake_plans WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM program_issue_mappings WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM program_queue_label_ownership WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

	fn upsert_project(&self, project: &ProjectRegistration) -> Result<()> {
		self.connection.execute(
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

		Ok(())
	}

	fn delete_connector_backoff(&self, project_id: &str, connector: &str) -> Result<()> {
		self.connection.execute(
			"DELETE FROM connector_backoffs WHERE project_id = ?1 AND connector = ?2",
			params![project_id, connector],
		)?;

		Ok(())
	}

	fn connector_backoff(
		&self,
		project_id: &str,
		connector: &str,
	) -> Result<Option<ConnectorBackoff>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, connector, sync_phase, quota_class, reset_unix_epoch,
			 reset_source, warning, updated_at, updated_at_unix
			 FROM connector_backoffs
			 WHERE project_id = ?1 AND connector = ?2
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, connector])?;

		Ok(rows.next()?.map(connector_backoff_from_row).transpose()?)
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

	fn upsert_run_control_channel(&self, channel: &RunControlChannelRecord) -> Result<()> {
		self.connection.execute(
			"INSERT OR REPLACE INTO run_control_channels (
					run_id, project_id, issue_id, attempt_number, transport, channel_path, status,
					published_at, published_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&channel.run_id,
				&channel.project_id,
				&channel.issue_id,
				channel.attempt_number,
				&channel.transport,
				channel.channel_path.to_string_lossy().as_ref(),
				&channel.status,
				&channel.published_at,
				channel.published_at_unix,
				&channel.updated_at,
				channel.updated_at_unix,
			],
		)?;

		Ok(())
	}

	fn upsert_lease_and_remember_run_project(&mut self, lease: &IssueLease) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute(
			"INSERT OR REPLACE INTO leases (issue_id, project_id, run_id, issue_state)
			 VALUES (?1, ?2, ?3, ?4)",
			params![lease.issue_id(), lease.project_id(), lease.run_id(), lease.issue_state()],
		)?;

		update_run_attempt_project(&transaction, lease.project_id(), lease.issue_id(), Some(lease.run_id()))?;

		transaction.commit()?;

		Ok(())
	}

	fn upsert_worktree_and_remember_run_project(
		&mut self,
		mapping: &WorktreeMappingRecord,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute(
			"INSERT OR REPLACE INTO worktrees (
				issue_id, project_id, branch_name, worktree_path,
				provenance_source, created_at_unix, updated_at_unix
			 )
			 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
			params![
				&mapping.issue_id,
				&mapping.project_id,
				&mapping.branch_name,
				mapping.worktree_path.to_string_lossy().as_ref(),
				&mapping.provenance_source,
				mapping.created_at_unix,
				mapping.updated_at_unix,
			],
		)?;

		update_run_attempt_project(&transaction, &mapping.project_id, &mapping.issue_id, None)?;

		transaction.commit()?;

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

	fn insert_private_execution_event(
		&self,
		record: &PrivateExecutionEventRuntimeRecord,
	) -> Result<i64> {
		let payload_json = serde_json::to_string(&record.payload)?;

		self.connection.execute(
			"INSERT INTO private_execution_events (
					project_id, issue_id, run_id, attempt_number, event_type, payload_json,
					recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
			params![
				&record.project_id,
				&record.issue_id,
				&record.run_id,
				record.attempt_number,
				&record.event_type,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;

		Ok(self.connection.last_insert_rowid())
	}

	#[allow(dead_code)]
	fn upsert_decision_contract(&self, record: &DecisionContractRuntimeRecord) -> Result<()> {
		let payload_json = serde_json::to_string(&record.contract)?;

		self.connection.execute(
			"INSERT INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
			 ON CONFLICT(project_id, contract_id) DO UPDATE SET
				 source_issue_id = excluded.source_issue_id,
				 status = excluded.status,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.contract.contract_id(),
				record.source_issue_id.as_deref(),
				record.status.as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}

	#[allow(dead_code)]
	fn upsert_execution_program(&self, record: &ExecutionProgramRuntimeRecord) -> Result<()> {
		let payload_json = serde_json::to_string(&record.program)?;

		self.connection.execute(
			"INSERT INTO execution_programs (
					project_id, program_id, source_contract_id, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
			 ON CONFLICT(project_id, program_id) DO UPDATE SET
				 source_contract_id = excluded.source_contract_id,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.program.program_id(),
				record.source_contract_id.as_deref(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
		self.replace_program_intake_state(record)?;

		Ok(())
	}

	fn replace_program_intake_state(&self, record: &ExecutionProgramRuntimeRecord) -> Result<()> {
		self.connection.execute(
			"DELETE FROM program_intake_plans WHERE project_id = ?1 AND program_id = ?2",
			params![&record.project_id, record.program.program_id()],
		)?;
		self.connection.execute(
			"DELETE FROM program_issue_mappings WHERE project_id = ?1 AND program_id = ?2",
			params![&record.project_id, record.program.program_id()],
		)?;
		self.connection.execute(
			"DELETE FROM program_queue_label_ownership WHERE project_id = ?1 AND program_id = ?2",
			params![&record.project_id, record.program.program_id()],
		)?;

		insert_program_intake_state(&self.connection, record)
	}

	fn delete_lease(&mut self, issue_id: &str) -> Result<()> {
		self.connection
			.execute("DELETE FROM leases WHERE issue_id = ?1", params![issue_id])?;

		Ok(())
	}

	fn retarget_issue_identity(
		&mut self,
		previous_issue_id: &str,
		canonical_issue_id: &str,
	) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute(
			"INSERT OR IGNORE INTO leases (issue_id, project_id, run_id, issue_state)
			 SELECT ?2, project_id, run_id, issue_state FROM leases WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute("DELETE FROM leases WHERE issue_id = ?1", params![previous_issue_id])?;
		transaction.execute(
			"INSERT OR IGNORE INTO worktrees (
				issue_id, project_id, branch_name, worktree_path,
				provenance_source, created_at_unix, updated_at_unix
			 )
			 SELECT ?2, project_id, branch_name, worktree_path,
				provenance_source, created_at_unix, updated_at_unix
			 FROM worktrees WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute("DELETE FROM worktrees WHERE issue_id = ?1", params![previous_issue_id])?;
		transaction.execute(
			"UPDATE run_attempts SET issue_id = ?2 WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE run_control_channels SET issue_id = ?2 WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE private_execution_events SET issue_id = ?2 WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE decision_contracts SET source_issue_id = ?2 WHERE source_issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE program_issue_mappings SET issue_id = ?2 WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"UPDATE program_queue_label_ownership SET issue_id = ?2 WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO loop_guardrail_checkpoints (
					project_id, issue_id, reason, fingerprint, run_id, attempt_number,
					consecutive_count, details_json, updated_at, updated_at_unix
				)
			 SELECT project_id, ?2, reason, fingerprint, run_id, attempt_number,
					consecutive_count, details_json, updated_at, updated_at_unix
			 FROM loop_guardrail_checkpoints WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE issue_id = ?1",
			params![previous_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO review_policy_checkpoints (
					project_id, issue_id, run_id, attempt_number, phase, status, head_sha,
					nonclean_rounds, details_json, updated_at, updated_at_unix
				)
			 SELECT project_id, ?2, run_id, attempt_number, phase, status, head_sha,
					nonclean_rounds, details_json, updated_at, updated_at_unix
			 FROM review_policy_checkpoints WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM review_policy_checkpoints WHERE issue_id = ?1",
			params![previous_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO review_handoffs (
					project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, updated_at, updated_at_unix
				)
			 SELECT project_id, ?2, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, updated_at, updated_at_unix
			 FROM review_handoffs WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM review_handoffs WHERE issue_id = ?1",
			params![previous_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO review_orchestrations (
					project_id, issue_id, branch_name, run_id, attempt_number, pr_url, head_sha,
					phase, request_comment_database_id, request_created_at_unix_epoch,
					request_description_thumbs_up_count, request_retry_count, external_round_count,
					auto_merge_enabled_at_unix_epoch, updated_at, updated_at_unix
				)
			 SELECT project_id, ?2, branch_name, run_id, attempt_number, pr_url, head_sha,
					phase, request_comment_database_id, request_created_at_unix_epoch,
					request_description_thumbs_up_count, request_retry_count, external_round_count,
					auto_merge_enabled_at_unix_epoch, updated_at, updated_at_unix
			 FROM review_orchestrations WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
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
		transaction.execute(
			"DELETE FROM review_policy_checkpoints WHERE issue_id = ?1",
			params![issue_id],
		)?;
		transaction.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE issue_id = ?1",
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
		transaction.execute(
			"DELETE FROM review_policy_checkpoints
			 WHERE project_id = ?1 AND issue_id = ?2 AND run_id = ?3 AND attempt_number = ?4",
			params![project_id, issue_id, run_id, attempt_number],
		)?;
		transaction.commit()?;

		Ok(())
	}

	fn delete_loop_guardrail_checkpoints_for_issue(
		&mut self,
		project_id: &str,
		issue_id: &str,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE project_id = ?1 AND issue_id = ?2",
			params![project_id, issue_id],
		)?;

		Ok(())
	}

	fn delete_loop_guardrail_checkpoint(
		&mut self,
		project_id: &str,
		issue_id: &str,
		reason: &str,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM loop_guardrail_checkpoints \
			 WHERE project_id = ?1 AND issue_id = ?2 AND reason = ?3",
			params![project_id, issue_id, reason],
		)?;

		Ok(())
	}

	fn delete_review_policy_checkpoints_for_run_attempt(
		&mut self,
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
	) -> Result<()> {
		self.connection.execute(
			"DELETE FROM review_policy_checkpoints
			 WHERE project_id = ?1 AND issue_id = ?2 AND run_id = ?3 AND attempt_number = ?4",
			params![project_id, issue_id, run_id, attempt_number],
		)?;

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

	fn load_run_attempts_for_project(&self, state: &mut StateData, project_id: &str) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(params![project_id], run_attempt_record_from_row)?;

		for row in rows {
			let attempt = row?;

			state.run_attempts.insert(attempt.run_id.clone(), attempt);
		}

		Ok(())
	}

	fn load_run_control_channels(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, transport, channel_path, status, \
			 published_at, published_at_unix, updated_at, updated_at_unix \
			 FROM run_control_channels",
		)?;
		let rows = statement.query_map([], |row| {
			Ok(RunControlChannelRecord {
				run_id: row.get(0)?,
				project_id: row.get(1)?,
				issue_id: row.get(2)?,
				attempt_number: row.get(3)?,
				transport: row.get(4)?,
				channel_path: PathBuf::from(row.get::<_, String>(5)?),
				status: row.get(6)?,
				published_at: row.get(7)?,
				published_at_unix: row.get(8)?,
				updated_at: row.get(9)?,
				updated_at_unix: row.get(10)?,
			})
		})?;

		for row in rows {
			let channel = row?;

			state.control_channels.insert(channel.run_id.clone(), channel);
		}

		Ok(())
	}

	fn load_run_control_channels_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, transport, channel_path, status, \
			 published_at, published_at_unix, updated_at, updated_at_unix \
			 FROM run_control_channels WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(params![project_id], |row| {
			Ok(RunControlChannelRecord {
				run_id: row.get(0)?,
				project_id: row.get(1)?,
				issue_id: row.get(2)?,
				attempt_number: row.get(3)?,
				transport: row.get(4)?,
				channel_path: PathBuf::from(row.get::<_, String>(5)?),
				status: row.get(6)?,
				published_at: row.get(7)?,
				published_at_unix: row.get(8)?,
				updated_at: row.get(9)?,
				updated_at_unix: row.get(10)?,
			})
		})?;

		for row in rows {
			let channel = row?;

			state.control_channels.insert(channel.run_id.clone(), channel);
		}

		Ok(())
	}

	fn retry_budget_attempt_count(&self, issue_id: &str) -> Result<i64> {
		self.connection
			.query_row(
				"SELECT COUNT(*) FROM run_attempts \
				 WHERE issue_id = ?1 AND status IN ('failed', 'interrupted', 'terminal_guarded')",
				params![issue_id],
				|row| row.get(0),
			)
			.map_err(Into::into)
	}

	fn issue_has_retry_budget_attempt_after(
		&self,
		issue_id: &str,
		attempt_number: i64,
	) -> Result<bool> {
		let count = self.connection.query_row(
			"SELECT COUNT(*) FROM run_attempts \
			 WHERE issue_id = ?1 \
			 AND attempt_number > ?2 \
			 AND status IN ('failed', 'interrupted', 'terminal_guarded') \
			 LIMIT 1",
			params![issue_id, attempt_number],
			|row| row.get::<_, i64>(0),
		)?;

		Ok(count > 0)
	}

	fn run_attempt_for_issue_attempt(
		&self,
		issue_id: &str,
		attempt_number: i64,
	) -> Result<Option<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE issue_id = ?1 AND attempt_number = ?2 \
			 ORDER BY updated_at_unix DESC, run_id DESC \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![issue_id, attempt_number])?;

		Ok(rows.next()?.map(run_attempt_record_from_row).transpose()?)
	}

	fn latest_run_attempt_for_issue(&self, issue_id: &str) -> Result<Option<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE issue_id = ?1 \
			 ORDER BY attempt_number DESC, updated_at_unix DESC, run_id DESC \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![issue_id])?;

		Ok(rows.next()?.map(run_attempt_record_from_row).transpose()?)
	}

	fn list_run_attempts_for_issue(&self, issue_id: &str) -> Result<Vec<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE issue_id = ?1 \
			 ORDER BY attempt_number ASC, run_id ASC",
		)?;
		let rows = statement.query_map(params![issue_id], run_attempt_record_from_row)?;

		rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
	}

	fn list_run_attempts_for_project(&self, project_id: &str) -> Result<Vec<RunAttemptRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, project_id, issue_id, attempt_number, status, thread_id, turn_id, \
			 updated_at, updated_at_unix FROM run_attempts \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, run_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], run_attempt_record_from_row)?;

		rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
	}

	fn run_has_protocol_event(&self, run_id: &str, event_type: &str) -> Result<bool> {
		let exists = self.connection.query_row(
			"SELECT EXISTS(
			 SELECT 1 FROM protocol_events
			 WHERE run_id = ?1 AND event_type = ?2
			 LIMIT 1
			 )",
			params![run_id, event_type],
			|row| row.get::<_, i64>(0),
		)?;

		Ok(exists != 0)
	}

	fn load_protocol_event_summaries(&self, state: &mut StateData) -> Result<()> {
		self.load_compacted_protocol_event_summaries(state)?;

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

	fn load_protocol_event_summaries_for_runs(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		for run_id in run_ids {
			state.event_summaries.remove(run_id);
			self.load_compacted_protocol_event_summary_for_run(state, run_id)?;
			self.load_protocol_event_summary_for_run(state, run_id)?;
		}

		Ok(())
	}

	fn load_protocol_event_summary_for_run(
		&self,
		state: &mut StateData,
		run_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT sequence_number, event_type, created_at, created_at_unix \
			 FROM protocol_events \
			 WHERE run_id = ?1 \
			 ORDER BY sequence_number DESC \
			 LIMIT 1",
		)?;
		let summary = statement
			.query_row(params![run_id], |row| {
				let last_sequence_number = row.get(0)?;

				Ok(ProtocolEventSummaryRecord {
					event_count: last_sequence_number,
					last_sequence_number: Some(last_sequence_number),
					last_event_type: Some(row.get(1)?),
					last_event_at: Some(row.get(2)?),
					last_event_at_unix: Some(row.get(3)?),
				})
			})
			.optional()?;

		if let Some(summary) = summary {
			state.event_summaries.insert(run_id.to_owned(), summary);
		}

		Ok(())
	}

	fn load_compacted_protocol_event_summaries(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, event_count, last_sequence_number, last_event_type, last_event_at, \
			 last_event_at_unix FROM protocol_event_summaries ORDER BY run_id",
		)?;
		let rows = statement.query_map([], |row| {
			Ok((
				row.get::<_, String>(0)?,
				ProtocolEventSummaryRecord {
					event_count: row.get(1)?,
					last_sequence_number: row.get(2)?,
					last_event_type: row.get(3)?,
					last_event_at: row.get(4)?,
					last_event_at_unix: row.get(5)?,
				},
			))
		})?;

		for row in rows {
			let (run_id, summary) = row?;

			state.event_summaries.insert(run_id, summary);
		}

		Ok(())
	}

	fn load_compacted_protocol_event_summary_for_run(
		&self,
		state: &mut StateData,
		run_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT event_count, last_sequence_number, last_event_type, last_event_at, \
			 last_event_at_unix FROM protocol_event_summaries WHERE run_id = ?1",
		)?;
		let summary = statement
			.query_row(params![run_id], |row| {
				Ok(ProtocolEventSummaryRecord {
					event_count: row.get(0)?,
					last_sequence_number: row.get(1)?,
					last_event_type: row.get(2)?,
					last_event_at: row.get(3)?,
					last_event_at_unix: row.get(4)?,
				})
			})
			.optional()?;

		if let Some(summary) = summary {
			state.event_summaries.insert(run_id.to_owned(), summary);
		}

		Ok(())
	}

	fn load_worktrees(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self
			.connection
			.prepare(
				"SELECT issue_id, project_id, branch_name, worktree_path,
					provenance_source, created_at_unix, updated_at_unix
				 FROM worktrees",
			)?;
		let rows = statement.query_map([], |row| {
			let mapping = worktree_mapping_record_from_row(row)?;

			Ok((mapping.issue_id.clone(), mapping))
		})?;

		for row in rows {
			let (issue_id, mapping) = row?;

			state.worktrees.insert(issue_id, mapping);
		}

		Ok(())
	}

	fn worktree_for_issue(&self, issue_id: &str) -> Result<Option<WorktreeMappingRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT issue_id, project_id, branch_name, worktree_path,
			 provenance_source, created_at_unix, updated_at_unix
			 FROM worktrees
			 WHERE issue_id = ?1
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![issue_id])?;

		Ok(rows.next()?.map(worktree_mapping_record_from_row).transpose()?)
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

	fn list_linear_execution_events(
		&self,
		service_id: &str,
		issue_id: &str,
	) -> Result<Vec<LinearExecutionEventRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT payload_json, event_unix, recorded_at, recorded_at_unix \
			 FROM linear_execution_events \
			 WHERE service_id = ?1 AND issue_id = ?2",
		)?;
		let rows = statement.query_map(params![service_id, issue_id], |row| {
			Ok((
				row.get::<_, String>(0)?,
				row.get::<_, Option<i64>>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, i64>(3)?,
			))
		})?;
		let mut records = Vec::new();

		for row in rows {
			let (payload_json, event_unix, recorded_at, recorded_at_unix) = row?;
			let record = serde_json::from_str::<LinearExecutionEventRecord>(&payload_json)?;

			records.push(LinearExecutionEventRuntimeRecord {
				record,
				event_unix,
				recorded_at,
				recorded_at_unix,
			});
		}

		Ok(records)
	}

	fn load_private_execution_events(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT record_id, project_id, issue_id, run_id, attempt_number, event_type, \
			 payload_json, recorded_at, recorded_at_unix \
			 FROM private_execution_events \
			 ORDER BY record_id ASC",
		)?;
		let rows = statement.query_map([], |row| {
			Ok((
				row.get::<_, i64>(0)?,
				row.get::<_, String>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, String>(3)?,
				row.get::<_, i64>(4)?,
				row.get::<_, String>(5)?,
				row.get::<_, String>(6)?,
				row.get::<_, String>(7)?,
				row.get::<_, i64>(8)?,
			))
		})?;

		for row in rows {
			let (
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload_json,
				recorded_at,
				recorded_at_unix,
			) = row?;
			let payload = serde_json::from_str::<Value>(&payload_json)?;

			state.private_execution_events.push(PrivateExecutionEventRuntimeRecord {
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload,
				recorded_at,
				recorded_at_unix,
			});
		}

		Ok(())
	}

	fn load_private_execution_events_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT record_id, project_id, issue_id, run_id, attempt_number, event_type, \
			 payload_json, recorded_at, recorded_at_unix \
			 FROM private_execution_events \
			 WHERE project_id = ?1 \
			 ORDER BY record_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], |row| {
			Ok((
				row.get::<_, i64>(0)?,
				row.get::<_, String>(1)?,
				row.get::<_, String>(2)?,
				row.get::<_, String>(3)?,
				row.get::<_, i64>(4)?,
				row.get::<_, String>(5)?,
				row.get::<_, String>(6)?,
				row.get::<_, String>(7)?,
				row.get::<_, i64>(8)?,
			))
		})?;

		for row in rows {
			let (
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload_json,
				recorded_at,
				recorded_at_unix,
			) = row?;
			let payload = serde_json::from_str::<Value>(&payload_json)?;

			state.private_execution_events.push(PrivateExecutionEventRuntimeRecord {
				record_id,
				project_id,
				issue_id,
				run_id,
				attempt_number,
				event_type,
				payload,
				recorded_at,
				recorded_at_unix,
			});
		}

		Ok(())
	}

	fn load_decision_contracts(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 ORDER BY project_id ASC, contract_id ASC",
		)?;
		let rows = statement.query_map([], decision_contract_runtime_row_parts)?;

		for row in rows {
			let record = decision_contract_record_from_row_parts(row?)?;

			state.decision_contracts.insert(record.key(), record);
		}

		Ok(())
	}

	fn decision_contract(
		&self,
		project_id: &str,
		contract_id: &str,
	) -> Result<Option<DecisionContractRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 WHERE project_id = ?1 AND contract_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, contract_id])?;

		rows
			.next()?
			.map(decision_contract_runtime_row_parts)
			.transpose()?
			.map(decision_contract_record_from_row_parts)
			.transpose()
	}

	fn list_decision_contracts_for_issue(
		&self,
		project_id: &str,
		source_issue_id: &str,
	) -> Result<Vec<DecisionContractRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 WHERE project_id = ?1 AND source_issue_id = ?2 \
			 ORDER BY created_at_unix ASC, contract_id ASC",
		)?;
		let rows = statement.query_map(
			params![project_id, source_issue_id],
			decision_contract_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(decision_contract_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	fn load_execution_programs(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 ORDER BY project_id ASC, program_id ASC",
		)?;
		let rows = statement.query_map([], execution_program_runtime_row_parts)?;

		for row in rows {
			let record = execution_program_record_from_row_parts(row?)?;

			state.execution_programs.insert(record.key(), record);
		}

		Ok(())
	}

	fn execution_program(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<Option<ExecutionProgramRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 WHERE project_id = ?1 AND program_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, program_id])?;

		rows
			.next()?
			.map(execution_program_runtime_row_parts)
			.transpose()?
			.map(execution_program_record_from_row_parts)
			.transpose()
	}

	fn list_execution_programs_for_contract(
		&self,
		project_id: &str,
		source_contract_id: &str,
	) -> Result<Vec<ExecutionProgramRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 WHERE project_id = ?1 AND source_contract_id = ?2 \
			 ORDER BY created_at_unix ASC, program_id ASC",
		)?;
		let rows = statement.query_map(
			params![project_id, source_contract_id],
			execution_program_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(execution_program_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	fn list_execution_programs(
		&self,
		project_id: &str,
	) -> Result<Vec<ExecutionProgramRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, source_contract_id, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM execution_programs \
			 WHERE project_id = ?1 \
			 ORDER BY created_at_unix ASC, program_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], execution_program_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(execution_program_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	fn load_program_intake_state(&self, state: &mut StateData) -> Result<()> {
		for record in self.list_all_program_intake_plans()? {
			state.program_intake_plans.insert(
				ProgramIntakePlanKey::new(&record.project_id, &record.program_id, &record.plan_id),
				record,
			);
		}
		for record in self.list_all_program_issue_mappings()? {
			state.program_issue_mappings.insert(
				ProgramIssueMappingKey::new(
					&record.project_id,
					&record.program_id,
					&record.node_id,
				),
				record,
			);
		}
		for record in self.list_all_program_queue_label_ownership()? {
			state.program_queue_label_ownership.insert(
				ProgramQueueLabelOwnershipKey::new(
					&record.project_id,
					&record.program_id,
					&record.node_id,
					&record.label_name,
				),
				record,
			);
		}

		Ok(())
	}

	fn list_all_program_intake_plans(&self) -> Result<Vec<ProgramIntakePlanRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, plan_id, intake_kind, source_contract_id, \
			 accepted_contract_fingerprint, public_summary, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM program_intake_plans \
			 ORDER BY project_id ASC, program_id ASC, plan_id ASC",
		)?;
		let rows = statement.query_map([], program_intake_plan_row)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(row?);
		}

		Ok(records)
	}

	fn list_program_intake_plans(
		&self,
		project_id: &str,
	) -> Result<Vec<ProgramIntakePlanRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, plan_id, intake_kind, source_contract_id, \
			 accepted_contract_fingerprint, public_summary, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM program_intake_plans \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix ASC, program_id ASC, plan_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], program_intake_plan_row)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(row?);
		}

		Ok(records)
	}

	fn list_all_program_issue_mappings(&self) -> Result<Vec<ProgramIssueMappingRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, node_id, issue_id, issue_identifier, issue_state, \
			 queue_intent, has_queue_label, queue_label_owned_by_program_reconciler, \
			 has_active_label, has_opt_out_label, has_needs_attention_label, \
			 has_generic_dispatch_briefing, created_at, created_at_unix, updated_at, \
			 updated_at_unix \
			 FROM program_issue_mappings \
			 ORDER BY project_id ASC, program_id ASC, node_id ASC",
		)?;
		let rows = statement.query_map([], program_issue_mapping_row)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(row?);
		}

		Ok(records)
	}

	fn list_program_issue_mappings(
		&self,
		project_id: &str,
		program_id: &str,
	) -> Result<Vec<ProgramIssueMappingRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, node_id, issue_id, issue_identifier, issue_state, \
			 queue_intent, has_queue_label, queue_label_owned_by_program_reconciler, \
			 has_active_label, has_opt_out_label, has_needs_attention_label, \
			 has_generic_dispatch_briefing, created_at, created_at_unix, updated_at, \
			 updated_at_unix \
			 FROM program_issue_mappings \
			 WHERE project_id = ?1 AND program_id = ?2 \
			 ORDER BY updated_at_unix ASC, node_id ASC",
		)?;
		let rows =
			statement.query_map(params![project_id, program_id], program_issue_mapping_row)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(row?);
		}

		Ok(records)
	}

	fn list_all_program_queue_label_ownership(
		&self,
	) -> Result<Vec<ProgramQueueLabelOwnershipRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, node_id, issue_id, issue_identifier, label_name, \
			 service_id, created_at, created_at_unix, updated_at, updated_at_unix \
			 FROM program_queue_label_ownership \
			 ORDER BY project_id ASC, program_id ASC, node_id ASC, label_name ASC",
		)?;
		let rows = statement.query_map([], program_queue_label_ownership_row)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(row?);
		}

		Ok(records)
	}

	fn program_queue_label_ownership_for_issue(
		&self,
		project_id: &str,
		issue_id: &str,
		label_name: &str,
	) -> Result<Vec<ProgramQueueLabelOwnershipRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, program_id, node_id, issue_id, issue_identifier, label_name, \
			 service_id, created_at, created_at_unix, updated_at, updated_at_unix \
			 FROM program_queue_label_ownership \
			 WHERE project_id = ?1 AND issue_id = ?2 AND label_name = ?3 \
			 ORDER BY updated_at_unix ASC, program_id ASC, node_id ASC",
		)?;
		let rows = statement.query_map(
			params![project_id, issue_id, label_name],
			program_queue_label_ownership_row,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(row?);
		}

		Ok(records)
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

	fn load_review_policy_checkpoints(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, run_id, attempt_number, phase, status, head_sha, \
			 nonclean_rounds, details_json, updated_at, updated_at_unix FROM review_policy_checkpoints",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let run_id: String = row.get(2)?;
			let attempt_number: i64 = row.get(3)?;
			let phase: String = row.get(4)?;

			Ok((
				ReviewPolicyKey::new(&project_id, &issue_id, &run_id, attempt_number, &phase),
				ReviewPolicyRuntimeRecord {
					project_id,
					issue_id,
					run_id,
					attempt_number,
					phase,
					status: row.get(5)?,
					head_sha: row.get(6)?,
					nonclean_rounds: row.get(7)?,
					details_json: row.get(8)?,
					updated_at: row.get(9)?,
					updated_at_unix: row.get(10)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_policy_checkpoints.insert(key, record);
		}

		Ok(())
	}

	fn load_review_policy_checkpoints_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, run_id, attempt_number, phase, status, head_sha, \
			 nonclean_rounds, details_json, updated_at, updated_at_unix FROM review_policy_checkpoints \
			 WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(params![project_id], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let run_id: String = row.get(2)?;
			let attempt_number: i64 = row.get(3)?;
			let phase: String = row.get(4)?;

			Ok((
				ReviewPolicyKey::new(&project_id, &issue_id, &run_id, attempt_number, &phase),
				ReviewPolicyRuntimeRecord {
					project_id,
					issue_id,
					run_id,
					attempt_number,
					phase,
					status: row.get(5)?,
					head_sha: row.get(6)?,
					nonclean_rounds: row.get(7)?,
					details_json: row.get(8)?,
					updated_at: row.get(9)?,
					updated_at_unix: row.get(10)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_policy_checkpoints.insert(key, record);
		}

		Ok(())
	}

	fn load_loop_guardrail_checkpoints(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, reason, fingerprint, run_id, attempt_number, \
			 consecutive_count, details_json, updated_at, updated_at_unix \
			 FROM loop_guardrail_checkpoints",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let reason: String = row.get(2)?;

			Ok((
				LoopGuardrailKey::new(&project_id, &issue_id, &reason),
				LoopGuardrailRuntimeRecord {
					project_id,
					issue_id,
					reason,
					fingerprint: row.get(3)?,
					run_id: row.get(4)?,
					attempt_number: row.get(5)?,
					consecutive_count: row.get(6)?,
					details_json: row.get(7)?,
					updated_at: row.get(8)?,
					updated_at_unix: row.get(9)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.loop_guardrail_checkpoints.insert(key, record);
		}

		Ok(())
	}

	fn load_connector_backoffs(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, connector, sync_phase, quota_class, reset_unix_epoch, \
			 reset_source, warning, updated_at, updated_at_unix FROM connector_backoffs",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let connector: String = row.get(1)?;

			Ok((
				(project_id.clone(), connector.clone()),
				ConnectorBackoff {
					project_id,
					connector,
					sync_phase: row.get(2)?,
					quota_class: row.get(3)?,
					reset_unix_epoch: row.get(4)?,
					reset_source: row.get(5)?,
					warning: row.get(6)?,
					updated_at: row.get(7)?,
					updated_at_unix: row.get(8)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.connector_backoffs.insert(key, record);
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
struct RunControlChannelRecord {
	project_id: String,
	issue_id: String,
	run_id: String,
	attempt_number: i64,
	transport: String,
	channel_path: PathBuf,
	status: String,
	published_at: String,
	published_at_unix: i64,
	updated_at: String,
	updated_at_unix: i64,
}
impl RunControlChannelRecord {
	fn as_public(&self) -> RunControlChannel {
		RunControlChannel {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			transport: self.transport.clone(),
			channel_path: self.channel_path.clone(),
			status: self.status.clone(),
			published_at: self.published_at.clone(),
			published_at_unix: self.published_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
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
struct PrivateExecutionEventRuntimeRecord {
	record_id: i64,
	project_id: String,
	issue_id: String,
	run_id: String,
	attempt_number: i64,
	event_type: String,
	payload: Value,
	recorded_at: String,
	recorded_at_unix: i64,
}
impl PrivateExecutionEventRuntimeRecord {
	fn as_public(&self) -> PrivateExecutionEvent {
		PrivateExecutionEvent {
			record_id: self.record_id,
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			event_type: self.event_type.clone(),
			payload: self.payload.clone(),
			recorded_at: self.recorded_at.clone(),
			recorded_at_unix: self.recorded_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DecisionContractKey {
	project_id: String,
	contract_id: String,
}
impl DecisionContractKey {
	fn new(project_id: &str, contract_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), contract_id: contract_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
struct DecisionContractRuntimeRecord {
	project_id: String,
	source_issue_id: Option<String>,
	contract: DecisionContract,
	status: DecisionContractStatus,
	created_at: String,
	created_at_unix: i64,
	updated_at: String,
	updated_at_unix: i64,
}
impl DecisionContractRuntimeRecord {
	#[allow(dead_code)]
	fn key(&self) -> DecisionContractKey {
		DecisionContractKey::new(&self.project_id, self.contract.contract_id())
	}

	#[allow(dead_code)]
	fn as_public(&self) -> DecisionContractRecord {
		DecisionContractRecord {
			project_id: self.project_id.clone(),
			source_issue_id: self.source_issue_id.clone(),
			contract: self.contract.clone(),
			status: self.status,
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ExecutionProgramKey {
	project_id: String,
	program_id: String,
}
impl ExecutionProgramKey {
	fn new(project_id: &str, program_id: &str) -> Self {
		Self { project_id: project_id.to_owned(), program_id: program_id.to_owned() }
	}
}

#[derive(Clone, Debug)]
struct ExecutionProgramRuntimeRecord {
	project_id: String,
	source_contract_id: Option<String>,
	program: ExecutionProgram,
	created_at: String,
	created_at_unix: i64,
	updated_at: String,
	updated_at_unix: i64,
}
impl ExecutionProgramRuntimeRecord {
	#[allow(dead_code)]
	fn key(&self) -> ExecutionProgramKey {
		ExecutionProgramKey::new(&self.project_id, self.program.program_id())
	}

	#[allow(dead_code)]
	fn as_public(&self) -> ExecutionProgramRecord {
		ExecutionProgramRecord {
			project_id: self.project_id.clone(),
			program: self.program.clone(),
			source_contract_id: self.source_contract_id.clone(),
			created_at: self.created_at.clone(),
			created_at_unix: self.created_at_unix,
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ProgramIntakePlanKey {
	project_id: String,
	program_id: String,
	plan_id: String,
}
impl ProgramIntakePlanKey {
	fn new(project_id: &str, program_id: &str, plan_id: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			program_id: program_id.to_owned(),
			plan_id: plan_id.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ProgramIssueMappingKey {
	project_id: String,
	program_id: String,
	node_id: String,
}
impl ProgramIssueMappingKey {
	fn new(project_id: &str, program_id: &str, node_id: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			program_id: program_id.to_owned(),
			node_id: node_id.to_owned(),
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ProgramQueueLabelOwnershipKey {
	project_id: String,
	program_id: String,
	node_id: String,
	label_name: String,
}
impl ProgramQueueLabelOwnershipKey {
	fn new(project_id: &str, program_id: &str, node_id: &str, label_name: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			program_id: program_id.to_owned(),
			node_id: node_id.to_owned(),
			label_name: label_name.to_owned(),
		}
	}
}

#[derive(Clone, Debug)]
struct WorktreeMappingRecord {
	project_id: String,
	issue_id: String,
	branch_name: String,
	worktree_path: PathBuf,
	provenance_source: String,
	created_at_unix: Option<i64>,
	updated_at_unix: Option<i64>,
}
impl WorktreeMappingRecord {
	fn as_public(&self) -> WorktreeMapping {
		WorktreeMapping {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			branch_name: self.branch_name.clone(),
			worktree_path: self.worktree_path.clone(),
			provenance: worktree_provenance(
				self.provenance_source.clone(),
				self.created_at_unix,
				self.updated_at_unix,
			),
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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ReviewPolicyKey {
	project_id: String,
	issue_id: String,
	run_id: String,
	attempt_number: i64,
	phase: String,
}
impl ReviewPolicyKey {
	fn new(
		project_id: &str,
		issue_id: &str,
		run_id: &str,
		attempt_number: i64,
		phase: &str,
	) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			run_id: run_id.to_owned(),
			attempt_number,
			phase: phase.to_owned(),
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

#[derive(Clone, Debug)]
struct ReviewPolicyRuntimeRecord {
	project_id: String,
	issue_id: String,
	run_id: String,
	attempt_number: i64,
	phase: String,
	status: String,
	head_sha: String,
	nonclean_rounds: i64,
	details_json: String,
	updated_at: String,
	updated_at_unix: i64,
}
impl ReviewPolicyRuntimeRecord {
	fn as_public(&self) -> ReviewPolicyCheckpoint {
		ReviewPolicyCheckpoint {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			phase: self.phase.clone(),
			status: self.status.clone(),
			head_sha: self.head_sha.clone(),
			nonclean_rounds: self.nonclean_rounds,
			details_json: self.details_json.clone(),
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct LoopGuardrailKey {
	project_id: String,
	issue_id: String,
	reason: String,
}
impl LoopGuardrailKey {
	fn new(project_id: &str, issue_id: &str, reason: &str) -> Self {
		Self {
			project_id: project_id.to_owned(),
			issue_id: issue_id.to_owned(),
			reason: reason.to_owned(),
		}
	}
}

#[derive(Clone, Debug)]
struct LoopGuardrailRuntimeRecord {
	project_id: String,
	issue_id: String,
	reason: String,
	fingerprint: String,
	run_id: String,
	attempt_number: i64,
	consecutive_count: i64,
	details_json: String,
	updated_at: String,
	updated_at_unix: i64,
}
impl LoopGuardrailRuntimeRecord {
	fn as_public(&self) -> LoopGuardrailCheckpoint {
		LoopGuardrailCheckpoint {
			project_id: self.project_id.clone(),
			issue_id: self.issue_id.clone(),
			reason: self.reason.clone(),
			fingerprint: self.fingerprint.clone(),
			run_id: self.run_id.clone(),
			attempt_number: self.attempt_number,
			consecutive_count: self.consecutive_count,
			details_json: self.details_json.clone(),
			updated_at: self.updated_at.clone(),
			updated_at_unix: self.updated_at_unix,
		}
	}
}

#[derive(Clone, Default)]
struct RunActivityMarkerRecord {
	run_id: Option<String>,
	attempt_number: Option<i64>,
	process_id: Option<u32>,
	host_boot_id: Option<String>,
	process_start_identity: Option<String>,
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

struct DecisionContractRuntimeRowParts {
	project_id: String,
	contract_id: String,
	source_issue_id: Option<String>,
	status: String,
	payload_json: String,
	created_at: String,
	created_at_unix: i64,
	updated_at: String,
	updated_at_unix: i64,
}

struct ExecutionProgramRuntimeRowParts {
	project_id: String,
	program_id: String,
	source_contract_id: Option<String>,
	payload_json: String,
	created_at: String,
	created_at_unix: i64,
	updated_at: String,
	updated_at_unix: i64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum GuardRetention {
	Local,
	ParentAfterHandoff,
	AdoptingChild,
}

pub(crate) fn protocol_event_counts_as_work_progress(event_type: &str) -> bool {
	let normalized = event_type.to_ascii_lowercase();

	if protocol_event_is_non_work_activity(&normalized) {
		return false;
	}

	normalized.starts_with("turn/")
		|| normalized.starts_with("item/")
		|| normalized == "thread/archive"
		|| normalized.contains("plan")
		|| normalized.contains("diff")
		|| normalized.contains("filechange")
		|| normalized.contains("patch")
		|| normalized.contains("command")
		|| normalized.contains("validation")
		|| normalized.contains("review")
		|| normalized.contains("pull_request")
		|| normalized == "model/response"
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

	set_run_activity_marker_process_identity(&mut marker, process_id);

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

	ensure_run_activity_marker_current_process_identity(&mut marker);

	marker.last_activity_unix_epoch = Some(now);
	marker.last_protocol_activity_unix_epoch = Some(now);

	if protocol_event_counts_as_work_progress(activity.last_event_type) {
		marker.last_progress_unix_epoch = Some(now);
	}

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

	ensure_run_activity_marker_current_process_identity(&mut marker);

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

	ensure_run_activity_marker_current_process_identity(&mut marker);

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

	ensure_run_activity_marker_current_process_identity(&mut marker);

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

	ensure_run_activity_marker_current_process_identity(&mut marker);

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

	ensure_run_activity_marker_current_process_identity(&mut marker);

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

	ensure_run_activity_marker_current_process_identity(&mut marker);

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

#[cfg_attr(not(test), allow(dead_code))]
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

	ensure_run_activity_marker_current_process_identity(&mut marker);

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
				host_boot_id: marker.host_boot_id,
				process_start_identity: marker.process_start_identity,
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

pub(crate) fn current_host_boot_id() -> Option<String> {
	static CURRENT_HOST_BOOT_ID: OnceLock<Option<String>> = OnceLock::new();

	CURRENT_HOST_BOOT_ID.get_or_init(read_current_host_boot_id).clone()
}

pub(crate) fn current_process_start_identity() -> Option<String> {
	static CURRENT_PROCESS_START_IDENTITY: OnceLock<Option<String>> = OnceLock::new();

	CURRENT_PROCESS_START_IDENTITY
		.get_or_init(|| process_start_identity(process::id()))
		.clone()
}

pub(crate) fn process_start_identity(process_id: u32) -> Option<String> {
	read_platform_process_start_identity(process_id)
		.and_then(|identity| normalized_process_start_identity(&identity))
}

fn protocol_event_is_non_work_activity(normalized_event_type: &str) -> bool {
	normalized_event_type.starts_with("account/")
		|| normalized_event_type.starts_with("skills/")
		|| normalized_event_type.starts_with("thread/goal/")
		|| normalized_event_type.contains("ratelimit")
		|| normalized_event_type.contains("rate_limit")
		|| normalized_event_type == "thread/status/changed"
		|| normalized_event_type.contains("tokenusage")
		|| matches!(
			normalized_event_type,
			"deprecationnotice"
				| "warning"
				| "configwarning"
				| "guardianwarning"
				| "model/rerouted"
				| "model/verification"
		)
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

fn set_run_activity_marker_process_identity(
	marker: &mut RunActivityMarkerRecord,
	process_id: u32,
) {
	marker.process_id = Some(process_id);
	marker.host_boot_id = current_host_boot_id();
	marker.process_start_identity = if process_id == process::id() {
		current_process_start_identity()
	} else {
		process_start_identity(process_id)
	};
}

fn ensure_run_activity_marker_current_process_identity(marker: &mut RunActivityMarkerRecord) {
	let current_process_id = process::id();

	match marker.process_id {
		None => set_run_activity_marker_process_identity(marker, current_process_id),
		Some(process_id)
			if process_id == current_process_id
				&& (marker.host_boot_id.is_none() || marker.process_start_identity.is_none()) =>
		{
			if marker.host_boot_id.is_none() {
				marker.host_boot_id = current_host_boot_id();
			}
			if marker.process_start_identity.is_none() {
				marker.process_start_identity = current_process_start_identity();
			}
		},
		Some(_) => {},
	}
}

fn read_current_host_boot_id() -> Option<String> {
	read_platform_host_boot_id().and_then(|boot_id| normalized_host_boot_id(&boot_id))
}

#[cfg(target_os = "linux")]
fn read_platform_host_boot_id() -> Option<String> {
	fs::read_to_string("/proc/sys/kernel/random/boot_id")
		.ok()
		.map(|boot_id| format!("linux:{boot_id}"))
}

#[cfg(target_os = "macos")]
fn read_platform_host_boot_id() -> Option<String> {
	let output = Command::new("/usr/sbin/sysctl")
		.args(["-n", "kern.boottime"])
		.output()
		.ok()?;

	if !output.status.success() {
		return None;
	}

	String::from_utf8(output.stdout)
		.ok()
		.map(|boot_id| format!("macos:{boot_id}"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_platform_host_boot_id() -> Option<String> {
	None
}

fn normalized_host_boot_id(boot_id: &str) -> Option<String> {
	let normalized = boot_id.split_whitespace().collect::<Vec<_>>().join(" ");

	(!normalized.is_empty()).then_some(normalized)
}

#[cfg(target_os = "linux")]
fn read_platform_process_start_identity(process_id: u32) -> Option<String> {
	let stat = fs::read_to_string(format!("/proc/{process_id}/stat")).ok()?;
	let comm_end = stat.rfind(')')?;
	let after_comm = stat.get(comm_end + 2..)?;
	let start_time = after_comm.split_whitespace().nth(19)?;

	Some(format!("linux_starttime:{start_time}"))
}

#[cfg(target_os = "macos")]
fn read_platform_process_start_identity(process_id: u32) -> Option<String> {
	let Ok(pid) = i32::try_from(process_id) else {
		return None;
	};

	if pid <= 0 {
		return None;
	}

	let mut info = MaybeUninit::<proc_bsdinfo>::zeroed();
	let Ok(info_size) = i32::try_from(mem::size_of::<proc_bsdinfo>()) else {
		return None;
	};
	let read_size = unsafe {
		libc::proc_pidinfo(
			pid,
			PROC_PIDTBSDINFO,
			0,
			info.as_mut_ptr().cast::<c_void>(),
			info_size,
		)
	};

	if read_size != info_size {
		return None;
	}

	let info = unsafe { info.assume_init() };

	Some(format!("macos_starttime:{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_platform_process_start_identity(_process_id: u32) -> Option<String> {
	None
}

fn normalized_process_start_identity(identity: &str) -> Option<String> {
	let normalized = identity.split_whitespace().collect::<Vec<_>>().join(" ");

	(!normalized.is_empty()).then_some(normalized)
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

fn update_run_attempt_project(
	transaction: &Transaction<'_>,
	project_id: &str,
	issue_id: &str,
	run_id: Option<&str>,
) -> Result<()> {
	match run_id {
		Some(run_id) => {
			transaction.execute(
				"UPDATE run_attempts SET project_id = ?1 WHERE issue_id = ?2 AND run_id = ?3",
				params![project_id, issue_id, run_id],
			)?;
		},
		None => {
			transaction.execute(
				"UPDATE run_attempts SET project_id = ?1 WHERE issue_id = ?2",
				params![project_id, issue_id],
			)?;
		},
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

fn persist_run_control_channels(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for channel in state.control_channels.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO run_control_channels (
					run_id, project_id, issue_id, attempt_number, transport, channel_path, status,
					published_at, published_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&channel.run_id,
				&channel.project_id,
				&channel.issue_id,
				channel.attempt_number,
				&channel.transport,
				channel.channel_path.to_string_lossy().as_ref(),
				&channel.status,
				&channel.published_at,
				channel.published_at_unix,
				&channel.updated_at,
				channel.updated_at_unix,
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
			"INSERT OR REPLACE INTO worktrees (
				issue_id, project_id, branch_name, worktree_path,
				provenance_source, created_at_unix, updated_at_unix
			 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
			params![
				&mapping.issue_id,
				&mapping.project_id,
				&mapping.branch_name,
				mapping.worktree_path.to_string_lossy().as_ref(),
				&mapping.provenance_source,
				mapping.created_at_unix,
				mapping.updated_at_unix,
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

fn persist_private_execution_events(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in &state.private_execution_events {
		let payload_json = serde_json::to_string(&record.payload)?;

		transaction.execute(
			"INSERT OR REPLACE INTO private_execution_events (
					record_id, project_id, issue_id, run_id, attempt_number, event_type,
					payload_json, recorded_at, recorded_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				record.record_id,
				&record.project_id,
				&record.issue_id,
				&record.run_id,
				record.attempt_number,
				&record.event_type,
				payload_json,
				&record.recorded_at,
				record.recorded_at_unix,
			],
		)?;
	}

	Ok(())
}

fn persist_decision_contracts(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.decision_contracts.values() {
		let payload_json = serde_json::to_string(&record.contract)?;

		transaction.execute(
			"INSERT OR REPLACE INTO decision_contracts (
					project_id, contract_id, source_issue_id, status, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&record.project_id,
				record.contract.contract_id(),
				record.source_issue_id.as_deref(),
				record.status.as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

fn persist_execution_programs(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.execution_programs.values() {
		let payload_json = serde_json::to_string(&record.program)?;

		transaction.execute(
			"INSERT OR REPLACE INTO execution_programs (
					project_id, program_id, source_contract_id, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
			params![
				&record.project_id,
				record.program.program_id(),
				record.source_contract_id.as_deref(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

fn persist_program_intake_state(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for record in state.program_intake_plans.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO program_intake_plans (
					project_id, program_id, plan_id, intake_kind, source_contract_id,
					accepted_contract_fingerprint, public_summary, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&record.project_id,
				&record.program_id,
				&record.plan_id,
				&record.intake_kind,
				record.source_contract_id.as_deref(),
				&record.accepted_contract_fingerprint,
				&record.public_summary,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}
	for record in state.program_issue_mappings.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO program_issue_mappings (
					project_id, program_id, node_id, issue_id, issue_identifier, issue_state,
					queue_intent, has_queue_label, queue_label_owned_by_program_reconciler,
					has_active_label, has_opt_out_label, has_needs_attention_label,
					has_generic_dispatch_briefing, created_at, created_at_unix, updated_at,
					updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
			params![
				&record.project_id,
				&record.program_id,
				&record.node_id,
				&record.issue_id,
				&record.issue_identifier,
				&record.issue_state,
				&record.queue_intent,
				sqlite_bool_value(record.has_queue_label),
				sqlite_bool_value(record.queue_label_owned_by_program_reconciler),
				sqlite_bool_value(record.has_active_label),
				sqlite_bool_value(record.has_opt_out_label),
				sqlite_bool_value(record.has_needs_attention_label),
				sqlite_bool_value(record.has_generic_dispatch_briefing),
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}
	for record in state.program_queue_label_ownership.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO program_queue_label_ownership (
					project_id, program_id, node_id, issue_id, issue_identifier, label_name,
					service_id, created_at, created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&record.project_id,
				&record.program_id,
				&record.node_id,
				&record.issue_id,
				&record.issue_identifier,
				&record.label_name,
				&record.service_id,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

fn insert_program_intake_state(
	connection: &Connection,
	record: &ExecutionProgramRuntimeRecord,
) -> Result<()> {
	for plan in derived_program_intake_plan_records(record) {
		connection.execute(
			"INSERT OR REPLACE INTO program_intake_plans (
					project_id, program_id, plan_id, intake_kind, source_contract_id,
					accepted_contract_fingerprint, public_summary, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&plan.project_id,
				&plan.program_id,
				&plan.plan_id,
				&plan.intake_kind,
				plan.source_contract_id.as_deref(),
				&plan.accepted_contract_fingerprint,
				&plan.public_summary,
				&plan.created_at,
				plan.created_at_unix,
				&plan.updated_at,
				plan.updated_at_unix,
			],
		)?;
	}
	for mapping in derived_program_issue_mapping_records(record) {
		connection.execute(
			"INSERT OR REPLACE INTO program_issue_mappings (
					project_id, program_id, node_id, issue_id, issue_identifier, issue_state,
					queue_intent, has_queue_label, queue_label_owned_by_program_reconciler,
					has_active_label, has_opt_out_label, has_needs_attention_label,
					has_generic_dispatch_briefing, created_at, created_at_unix, updated_at,
					updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
			params![
				&mapping.project_id,
				&mapping.program_id,
				&mapping.node_id,
				&mapping.issue_id,
				&mapping.issue_identifier,
				&mapping.issue_state,
				&mapping.queue_intent,
				sqlite_bool_value(mapping.has_queue_label),
				sqlite_bool_value(mapping.queue_label_owned_by_program_reconciler),
				sqlite_bool_value(mapping.has_active_label),
				sqlite_bool_value(mapping.has_opt_out_label),
				sqlite_bool_value(mapping.has_needs_attention_label),
				sqlite_bool_value(mapping.has_generic_dispatch_briefing),
				&mapping.created_at,
				mapping.created_at_unix,
				&mapping.updated_at,
				mapping.updated_at_unix,
			],
		)?;
	}
	for ownership in derived_program_queue_label_ownership_records(record) {
		connection.execute(
			"INSERT OR REPLACE INTO program_queue_label_ownership (
					project_id, program_id, node_id, issue_id, issue_identifier, label_name,
					service_id, created_at, created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				&ownership.project_id,
				&ownership.program_id,
				&ownership.node_id,
				&ownership.issue_id,
				&ownership.issue_identifier,
				&ownership.label_name,
				&ownership.service_id,
				&ownership.created_at,
				ownership.created_at_unix,
				&ownership.updated_at,
				ownership.updated_at_unix,
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

fn persist_review_policy_checkpoints(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.review_policy_checkpoints.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO review_policy_checkpoints (
					project_id, issue_id, run_id, attempt_number, phase, status, head_sha,
					nonclean_rounds, details_json, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
			params![
				record.project_id,
				record.issue_id,
				record.run_id,
				record.attempt_number,
				record.phase,
				record.status,
				record.head_sha,
				record.nonclean_rounds,
				record.details_json,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

fn persist_loop_guardrail_checkpoints(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.loop_guardrail_checkpoints.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO loop_guardrail_checkpoints (
					project_id, issue_id, reason, fingerprint, run_id, attempt_number,
					consecutive_count, details_json, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
			params![
				record.project_id,
				record.issue_id,
				record.reason,
				record.fingerprint,
				record.run_id,
				record.attempt_number,
				record.consecutive_count,
				record.details_json,
				record.updated_at,
				record.updated_at_unix,
			],
		)?;
	}

	Ok(())
}

fn persist_connector_backoffs(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for record in state.connector_backoffs.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO connector_backoffs (
					project_id, connector, sync_phase, quota_class, reset_unix_epoch,
					reset_source, warning, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				record.project_id,
				record.connector,
				record.sync_phase,
				record.quota_class,
				record.reset_unix_epoch,
				record.reset_source,
				record.warning,
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

fn shared_lock_coordinator_path(root: &Path) -> PathBuf {
	let mut hash = 0xcbf2_9ce4_8422_2325_u64;

	for byte in root.as_os_str().as_bytes() {
		hash ^= u64::from(*byte);
		hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
	}

	env::temp_dir()
		.join("decodex-shared-lock-coordinators")
		.join(format!("{hash:016x}.lock"))
}

fn acquire_shared_lock_coordinator(root: &Path) -> Result<File> {
	fs::create_dir_all(root)?;

	let coordinator_path = shared_lock_coordinator_path(root);

	if let Some(parent) = coordinator_path.parent() {
		fs::create_dir_all(parent)?;
	}

	let coordinator = OpenOptions::new()
		.read(true)
		.write(true)
		.create(true)
		.truncate(false)
		.open(coordinator_path)?;

	coordinator.lock()?;

	Ok(coordinator)
}

fn lock_root_from_lock_path(lock_path: &Path) -> Result<&Path> {
	lock_path
		.parent()
		.ok_or_else(|| eyre::eyre!("shared lock path `{}` has no parent root", lock_path.display()))
}

fn remove_lock_file_if_exists(path: &Path) -> Result<()> {
	match fs::remove_file(path) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

fn shared_lock_file_is_cleanup_candidate(path: &Path) -> bool {
	let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
		return false;
	};

	file_name.starts_with(&format!("{ISSUE_CLAIM_LOCK_FILE_PREFIX}."))
		|| file_name.starts_with(&format!("{DISPATCH_SLOT_LOCK_FILE_PREFIX}."))
}

fn prune_unlocked_shared_lock_files(root: &Path) -> Result<()> {
	let _coordinator = acquire_shared_lock_coordinator(root)?;
	let read_dir = match fs::read_dir(root) {
		Ok(read_dir) => read_dir,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
		Err(error) => return Err(error.into()),
	};

	for entry in read_dir {
		let path = entry?.path();

		if !shared_lock_file_is_cleanup_candidate(&path) {
			continue;
		}

		let lock_file = match OpenOptions::new()
			.read(true)
			.write(true)
			.create(false)
			.truncate(false)
			.open(&path)
		{
			Ok(file) => file,
			Err(error) if error.kind() == ErrorKind::NotFound => continue,
			Err(error) => return Err(error.into()),
		};

		match lock_file.try_lock() {
			Ok(()) => {
				lock_file.unlock()?;

				drop(lock_file);
				remove_lock_file_if_exists(&path)?;
			},
			Err(TryLockError::WouldBlock) => {},
			Err(TryLockError::Error(error)) => return Err(error.into()),
		}
	}

	Ok(())
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
	let existing_marker = read_run_activity_marker_record(worktree_path)?;
	let same_run_marker = existing_marker
		.as_ref()
		.filter(|marker| marker.run_id.as_deref() == Some(run_id) && marker.attempt_number == Some(attempt_number));
	let mut marker = run_activity_marker_record_for_attempt(existing_marker.as_ref(), run_id, attempt_number);

	set_run_activity_marker_process_identity(&mut marker, process_id);

	marker.last_activity_unix_epoch = Some(last_activity_unix_epoch);
	marker.last_protocol_activity_unix_epoch = last_protocol_activity_unix_epoch
		.or_else(|| same_run_marker.and_then(|marker| marker.last_protocol_activity_unix_epoch));

	if let Some(same_run_marker) = same_run_marker {
		marker.retry_kind = same_run_marker.retry_kind.clone();
		marker.retry_ready_at_unix_epoch = same_run_marker.retry_ready_at_unix_epoch;
	}

	write_run_activity_marker_record(worktree_path, &marker)?;

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
			host_boot_id: same_run_marker.and_then(|marker| marker.host_boot_id.clone()),
			process_start_identity: same_run_marker
				.and_then(|marker| marker.process_start_identity.clone()),
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
				"host_boot_id" => marker.host_boot_id = Some(value.to_owned()),
				"process_start_identity" => marker.process_start_identity = Some(value.to_owned()),
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
	let marker_path = worktree_path.join(RUN_ACTIVITY_MARKER_FILE);
	let mut marker = marker.clone();

	if let Some(current_marker) = read_run_activity_marker_record(worktree_path)? {
		preserve_current_run_account_marker_fields(&current_marker, &mut marker);
	}

	write_run_activity_marker_body_atomic(&marker_path, &serialize_run_activity_marker_record(&marker))?;

	Ok(())
}

fn preserve_current_run_account_marker_fields(
	current: &RunActivityMarkerRecord,
	next: &mut RunActivityMarkerRecord,
) {
	if current.run_id != next.run_id || current.attempt_number != next.attempt_number {
		return;
	}

	let Some(current_account) = selected_marker_account(current).cloned() else {
		return;
	};
	let keep_current_account = match next.account.as_ref() {
		Some(next_account) =>
			account_marker_observed_unix_epoch(&current_account)
				> account_marker_observed_unix_epoch(next_account),
		None => true,
	};

	if keep_current_account {
		next.account = Some(current_account.clone());
		next.accounts = if current.accounts.is_empty() {
			vec![current_account]
		} else {
			current.accounts.clone()
		};
	} else if next.accounts.is_empty() && !current.accounts.is_empty() {
		next.accounts = current.accounts.clone();
	}
}

fn selected_marker_account(
	marker: &RunActivityMarkerRecord,
) -> Option<&CodexAccountActivitySummary> {
	marker
		.account
		.as_ref()
		.or_else(|| {
			marker.accounts.iter().find(|account| {
				account.status.eq_ignore_ascii_case("selected")
			})
		})
		.or_else(|| marker.accounts.first())
}

fn account_marker_observed_unix_epoch(account: &CodexAccountActivitySummary) -> i64 {
	[account.selected_at_unix_epoch, account.checked_at_unix_epoch]
		.into_iter()
		.flatten()
		.max()
		.unwrap_or(0)
}

fn write_run_activity_marker_body_atomic(marker_path: &Path, body: &str) -> Result<()> {
	let parent = marker_path.parent().ok_or_else(|| {
		eyre::eyre!("activity marker path `{}` has no parent directory", marker_path.display())
	})?;
	let sequence = RUN_ACTIVITY_MARKER_WRITE_SEQUENCE.fetch_add(
		1,
		std::sync::atomic::Ordering::Relaxed,
	);
	let temp_path = parent.join(format!(
		".{RUN_ACTIVITY_MARKER_FILE}.{}.{}.tmp",
		process::id(),
		sequence,
	));
	let result = (|| -> Result<()> {
		let mut temp_file = OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&temp_path)?;

		temp_file.write_all(body.as_bytes())?;
		temp_file.flush()?;

		drop(temp_file);

		fs::rename(&temp_path, marker_path)?;

		Ok(())
	})();

	if result.is_err() {
		let _ = fs::remove_file(&temp_path);
	}

	result
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
	if let Some(host_boot_id) = &marker.host_boot_id {
		body.push_str(&format!("host_boot_id={host_boot_id}\n"));
	}
	if let Some(process_start_identity) = &marker.process_start_identity {
		body.push_str(&format!("process_start_identity={process_start_identity}\n"));
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

	append_run_activity_marker_account_fields(&mut body, marker);
	append_run_activity_marker_retry_fields(&mut body, marker);
	append_run_activity_marker_review_policy_fields(&mut body, marker);

	body
}

fn append_run_activity_marker_account_fields(
	body: &mut String,
	marker: &RunActivityMarkerRecord,
) {
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
}

fn append_run_activity_marker_retry_fields(body: &mut String, marker: &RunActivityMarkerRecord) {
	if let Some(retry_budget_attempt_count) = marker.retry_budget_attempt_count {
		body.push_str(&format!("retry_budget_attempt_count={retry_budget_attempt_count}\n"));
	}
	if let Some(retry_kind) = &marker.retry_kind {
		body.push_str(&format!("retry_kind={retry_kind}\n"));
	}
	if let Some(retry_ready_at_unix_epoch) = marker.retry_ready_at_unix_epoch {
		body.push_str(&format!("retry_ready_at_unix_epoch={retry_ready_at_unix_epoch}\n"));
	}
}

fn append_run_activity_marker_review_policy_fields(
	body: &mut String,
	marker: &RunActivityMarkerRecord,
) {
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

fn validate_private_execution_event_inputs(
	project_id: &str,
	issue_id: &str,
	run_id: &str,
	attempt_number: i64,
	event_type: &str,
) -> Result<()> {
	if project_id.trim().is_empty() {
		eyre::bail!("Private execution event project_id must not be empty.");
	}
	if issue_id.trim().is_empty() {
		eyre::bail!("Private execution event issue_id must not be empty.");
	}
	if run_id.trim().is_empty() {
		eyre::bail!("Private execution event run_id must not be empty.");
	}
	if attempt_number < 1 {
		eyre::bail!("Private execution event attempt_number must be greater than zero.");
	}
	if event_type.trim().is_empty() {
		eyre::bail!("Private execution event event_type must not be empty.");
	}

	Ok(())
}

fn protocol_event_summary_from_events(events: &[ProtocolEventRecord]) -> ProtocolEventSummaryRecord {
	let mut summary = ProtocolEventSummaryRecord::default();

	for event in events {
		summary.record_event(event);
	}

	summary
}

fn compare_attempt_records(left: &RunAttemptRecord, right: &RunAttemptRecord) -> cmp::Ordering {
	left.attempt_number
		.cmp(&right.attempt_number)
		.then_with(|| left.updated_at_unix.cmp(&right.updated_at_unix))
		.then_with(|| left.run_id.cmp(&right.run_id))
}

fn run_attempt_record_from_row(
	row: &Row<'_>,
) -> std::result::Result<RunAttemptRecord, rusqlite::Error> {
	Ok(RunAttemptRecord {
		run_id: row.get(0)?,
		project_id: row.get(1)?,
		issue_id: row.get(2)?,
		attempt_number: row.get(3)?,
		status: row.get(4)?,
		thread_id: row.get(5)?,
		turn_id: row.get(6)?,
		updated_at: row.get(7)?,
		updated_at_unix: row.get(8)?,
	})
}

fn worktree_mapping_record_from_row(
	row: &Row<'_>,
) -> std::result::Result<WorktreeMappingRecord, rusqlite::Error> {
	Ok(WorktreeMappingRecord {
		issue_id: row.get(0)?,
		project_id: row.get(1)?,
		branch_name: row.get(2)?,
		worktree_path: PathBuf::from(row.get::<_, String>(3)?),
		provenance_source: row.get(4)?,
		created_at_unix: row.get(5)?,
		updated_at_unix: row.get(6)?,
	})
}

fn decision_contract_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<DecisionContractRuntimeRowParts, rusqlite::Error> {
	Ok(DecisionContractRuntimeRowParts {
		project_id: row.get(0)?,
		contract_id: row.get(1)?,
		source_issue_id: row.get(2)?,
		status: row.get(3)?,
		payload_json: row.get(4)?,
		created_at: row.get(5)?,
		created_at_unix: row.get(6)?,
		updated_at: row.get(7)?,
		updated_at_unix: row.get(8)?,
	})
}

fn decision_contract_record_from_row_parts(
	parts: DecisionContractRuntimeRowParts,
) -> Result<DecisionContractRuntimeRecord> {
	let contract = serde_json::from_str::<DecisionContract>(&parts.payload_json)?;
	let contract_status = contract.status();

	contract.validate()?;

	if parts.contract_id != contract.contract_id() {
		eyre::bail!(
			"Decision contract row `{}` contained payload `{}`.",
			parts.contract_id,
			contract.contract_id()
		);
	}
	if parts.status != contract_status.as_str() {
		tracing::warn!(
			project_id = %parts.project_id,
			contract_id = %parts.contract_id,
			"decision contract status column differed from payload status"
		);
	}

	Ok(DecisionContractRuntimeRecord {
		project_id: parts.project_id,
		source_issue_id: parts.source_issue_id,
		status: contract_status,
		contract,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}

fn execution_program_runtime_row_parts(
	row: &Row<'_>,
) -> std::result::Result<ExecutionProgramRuntimeRowParts, rusqlite::Error> {
	Ok(ExecutionProgramRuntimeRowParts {
		project_id: row.get(0)?,
		program_id: row.get(1)?,
		source_contract_id: row.get(2)?,
		payload_json: row.get(3)?,
		created_at: row.get(4)?,
		created_at_unix: row.get(5)?,
		updated_at: row.get(6)?,
		updated_at_unix: row.get(7)?,
	})
}

fn execution_program_record_from_row_parts(
	parts: ExecutionProgramRuntimeRowParts,
) -> Result<ExecutionProgramRuntimeRecord> {
	let program = serde_json::from_str::<ExecutionProgram>(&parts.payload_json)?;

	program.validate()?;

	if parts.program_id != program.program_id() {
		eyre::bail!(
			"Execution program row `{}` contained payload `{}`.",
			parts.program_id,
			program.program_id()
		);
	}
	if parts.source_contract_id.as_deref() != program.source_contract_id() {
		eyre::bail!(
			"Execution program row `{}` carried source contract `{}` but payload references `{}`.",
			parts.program_id,
			parts.source_contract_id.as_deref().unwrap_or("none"),
			program.source_contract_id().unwrap_or("none")
		);
	}

	Ok(ExecutionProgramRuntimeRecord {
		project_id: parts.project_id,
		source_contract_id: parts.source_contract_id,
		program,
		created_at: parts.created_at,
		created_at_unix: parts.created_at_unix,
		updated_at: parts.updated_at,
		updated_at_unix: parts.updated_at_unix,
	})
}

fn program_intake_plan_row(
	row: &Row<'_>,
) -> std::result::Result<ProgramIntakePlanRecord, rusqlite::Error> {
	Ok(ProgramIntakePlanRecord {
		project_id: row.get(0)?,
		program_id: row.get(1)?,
		plan_id: row.get(2)?,
		intake_kind: row.get(3)?,
		source_contract_id: row.get(4)?,
		accepted_contract_fingerprint: row.get(5)?,
		public_summary: row.get(6)?,
		created_at: row.get(7)?,
		created_at_unix: row.get(8)?,
		updated_at: row.get(9)?,
		updated_at_unix: row.get(10)?,
	})
}

fn program_issue_mapping_row(
	row: &Row<'_>,
) -> std::result::Result<ProgramIssueMappingRecord, rusqlite::Error> {
	Ok(ProgramIssueMappingRecord {
		project_id: row.get(0)?,
		program_id: row.get(1)?,
		node_id: row.get(2)?,
		issue_id: row.get(3)?,
		issue_identifier: row.get(4)?,
		issue_state: row.get(5)?,
		queue_intent: row.get(6)?,
		has_queue_label: sqlite_bool(row, 7)?,
		queue_label_owned_by_program_reconciler: sqlite_bool(row, 8)?,
		has_active_label: sqlite_bool(row, 9)?,
		has_opt_out_label: sqlite_bool(row, 10)?,
		has_needs_attention_label: sqlite_bool(row, 11)?,
		has_generic_dispatch_briefing: sqlite_bool(row, 12)?,
		created_at: row.get(13)?,
		created_at_unix: row.get(14)?,
		updated_at: row.get(15)?,
		updated_at_unix: row.get(16)?,
	})
}

fn program_queue_label_ownership_row(
	row: &Row<'_>,
) -> std::result::Result<ProgramQueueLabelOwnershipRecord, rusqlite::Error> {
	Ok(ProgramQueueLabelOwnershipRecord {
		project_id: row.get(0)?,
		program_id: row.get(1)?,
		node_id: row.get(2)?,
		issue_id: row.get(3)?,
		issue_identifier: row.get(4)?,
		label_name: row.get(5)?,
		service_id: row.get(6)?,
		created_at: row.get(7)?,
		created_at_unix: row.get(8)?,
		updated_at: row.get(9)?,
		updated_at_unix: row.get(10)?,
	})
}

fn sqlite_bool(row: &Row<'_>, index: usize) -> std::result::Result<bool, rusqlite::Error> {
	Ok(row.get::<_, i64>(index)? != 0)
}

fn sqlite_bool_value(value: bool) -> i64 {
	if value { 1 } else { 0 }
}

fn connector_backoff_from_row(row: &Row<'_>) -> std::result::Result<ConnectorBackoff, rusqlite::Error> {
	Ok(ConnectorBackoff {
		project_id: row.get(0)?,
		connector: row.get(1)?,
		sync_phase: row.get(2)?,
		quota_class: row.get(3)?,
		reset_unix_epoch: row.get(4)?,
		reset_source: row.get(5)?,
		warning: row.get(6)?,
		updated_at: row.get(7)?,
		updated_at_unix: row.get(8)?,
	})
}

fn compare_linear_execution_event_runtime_records(
	left: &LinearExecutionEventRuntimeRecord,
	right: &LinearExecutionEventRuntimeRecord,
) -> cmp::Ordering {
	left.event_unix
		.cmp(&right.event_unix)
		.then_with(|| left.recorded_at_unix.cmp(&right.recorded_at_unix))
		.then_with(|| left.record.idempotency_key.cmp(&right.record.idempotency_key))
}

fn compare_private_execution_event_runtime_records(
	left: &PrivateExecutionEventRuntimeRecord,
	right: &PrivateExecutionEventRuntimeRecord,
) -> cmp::Ordering {
	left.record_id.cmp(&right.record_id)
}

#[allow(dead_code)]
fn compare_decision_contract_runtime_records(
	left: &DecisionContractRuntimeRecord,
	right: &DecisionContractRuntimeRecord,
) -> cmp::Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.contract.contract_id().cmp(right.contract.contract_id()))
}

#[allow(dead_code)]
fn compare_execution_program_runtime_records(
	left: &ExecutionProgramRuntimeRecord,
	right: &ExecutionProgramRuntimeRecord,
) -> cmp::Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.program.program_id().cmp(right.program.program_id()))
}

fn compare_program_intake_plan_records(
	left: &ProgramIntakePlanRecord,
	right: &ProgramIntakePlanRecord,
) -> cmp::Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.program_id.cmp(&right.program_id))
		.then_with(|| left.plan_id.cmp(&right.plan_id))
}

fn compare_program_issue_mapping_records(
	left: &ProgramIssueMappingRecord,
	right: &ProgramIssueMappingRecord,
) -> cmp::Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.program_id.cmp(&right.program_id))
		.then_with(|| left.node_id.cmp(&right.node_id))
}

fn compare_program_queue_label_ownership_records(
	left: &ProgramQueueLabelOwnershipRecord,
	right: &ProgramQueueLabelOwnershipRecord,
) -> cmp::Ordering {
	left.updated_at_unix
		.cmp(&right.updated_at_unix)
		.then_with(|| left.program_id.cmp(&right.program_id))
		.then_with(|| left.node_id.cmp(&right.node_id))
		.then_with(|| left.label_name.cmp(&right.label_name))
}

fn remove_derived_program_intake_state(
	state: &mut StateData,
	project_id: &str,
	program_id: &str,
) {
	state
		.program_intake_plans
		.retain(|key, _record| key.project_id != project_id || key.program_id != program_id);
	state
		.program_issue_mappings
		.retain(|key, _record| key.project_id != project_id || key.program_id != program_id);
	state.program_queue_label_ownership.retain(|key, _record| {
		key.project_id != project_id || key.program_id != program_id
	});
}

fn apply_derived_program_intake_state(
	state: &mut StateData,
	record: &ExecutionProgramRuntimeRecord,
) {
	remove_derived_program_intake_state(state, &record.project_id, record.program.program_id());

	for plan in derived_program_intake_plan_records(record) {
		state.program_intake_plans.insert(
			ProgramIntakePlanKey::new(&plan.project_id, &plan.program_id, &plan.plan_id),
			plan,
		);
	}
	for mapping in derived_program_issue_mapping_records(record) {
		state.program_issue_mappings.insert(
			ProgramIssueMappingKey::new(
				&mapping.project_id,
				&mapping.program_id,
				&mapping.node_id,
			),
			mapping,
		);
	}
	for ownership in derived_program_queue_label_ownership_records(record) {
		state.program_queue_label_ownership.insert(
			ProgramQueueLabelOwnershipKey::new(
				&ownership.project_id,
				&ownership.program_id,
				&ownership.node_id,
				&ownership.label_name,
			),
			ownership,
		);
	}
}

fn derived_program_intake_plan_records(
	record: &ExecutionProgramRuntimeRecord,
) -> Vec<ProgramIntakePlanRecord> {
	record
		.program
		.program_intake_plan()
		.map(|plan| {
			vec![ProgramIntakePlanRecord {
				project_id: record.project_id.clone(),
				program_id: record.program.program_id().to_owned(),
				plan_id: plan.plan_id().to_owned(),
				intake_kind: plan.intake_kind().as_str().to_owned(),
				source_contract_id: plan.source_contract_id().map(str::to_owned),
				accepted_contract_fingerprint: plan.accepted_contract_fingerprint().to_owned(),
				public_summary: plan.public_summary().to_owned(),
				created_at: record.created_at.clone(),
				created_at_unix: record.created_at_unix,
				updated_at: record.updated_at.clone(),
				updated_at_unix: record.updated_at_unix,
			}]
		})
		.unwrap_or_default()
}

fn derived_program_issue_mapping_records(
	record: &ExecutionProgramRuntimeRecord,
) -> Vec<ProgramIssueMappingRecord> {
	record
		.program
		.nodes()
		.iter()
		.filter_map(|node| {
			let issue = node.linear_issue()?;

			Some(ProgramIssueMappingRecord {
				project_id: record.project_id.clone(),
				program_id: record.program.program_id().to_owned(),
				node_id: node.node_id().to_owned(),
				issue_id: issue.issue_id().to_owned(),
				issue_identifier: issue.issue_identifier().to_owned(),
				issue_state: issue.issue_state().to_owned(),
				queue_intent: node.queue_intent().as_str().to_owned(),
				has_queue_label: issue.has_queue_label(),
				queue_label_owned_by_program_reconciler: issue
					.queue_label_owned_by_program_reconciler(),
				has_active_label: issue.has_active_label(),
				has_opt_out_label: issue.has_opt_out_label(),
				has_needs_attention_label: issue.has_needs_attention_label(),
				has_generic_dispatch_briefing: issue.has_generic_dispatch_briefing(),
				created_at: record.created_at.clone(),
				created_at_unix: record.created_at_unix,
				updated_at: record.updated_at.clone(),
				updated_at_unix: record.updated_at_unix,
			})
		})
		.collect()
}

fn derived_program_queue_label_ownership_records(
	record: &ExecutionProgramRuntimeRecord,
) -> Vec<ProgramQueueLabelOwnershipRecord> {
	let label_name = tracker::automation_queue_label(record.program.service_id());

	record
		.program
		.nodes()
		.iter()
		.filter_map(|node| {
			let issue = node.linear_issue()?;

			if !issue.queue_label_owned_by_program_reconciler() {
				return None;
			}

			Some(ProgramQueueLabelOwnershipRecord {
				project_id: record.project_id.clone(),
				program_id: record.program.program_id().to_owned(),
				node_id: node.node_id().to_owned(),
				issue_id: issue.issue_id().to_owned(),
				issue_identifier: issue.issue_identifier().to_owned(),
				label_name: label_name.clone(),
				service_id: record.program.service_id().to_owned(),
				created_at: record.created_at.clone(),
				created_at_unix: record.created_at_unix,
				updated_at: record.updated_at.clone(),
				updated_at_unix: record.updated_at_unix,
			})
		})
		.collect()
}

fn compare_project_run_status(left: &ProjectRunStatus, right: &ProjectRunStatus) -> cmp::Ordering {
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
		return Err(std::io::Error::last_os_error().into());
	}

	let new_flags = existing_flags & !FD_CLOEXEC;

	if new_flags != existing_flags {
		let result = unsafe { libc::fcntl(fd, F_SETFD, new_flags) };

		if result == -1 {
			return Err(std::io::Error::last_os_error().into());
		}
	}

	Ok(())
}

#[cfg(unix)]
fn set_close_on_exec(file: &File) -> Result<()> {
	let fd = file.as_raw_fd();
	let existing_flags = unsafe { libc::fcntl(fd, F_GETFD) };

	if existing_flags == -1 {
		return Err(std::io::Error::last_os_error().into());
	}

	let new_flags = existing_flags | FD_CLOEXEC;

	if new_flags != existing_flags {
		let result = unsafe { libc::fcntl(fd, F_SETFD, new_flags) };

		if result == -1 {
			return Err(std::io::Error::last_os_error().into());
		}
	}

	Ok(())
}
