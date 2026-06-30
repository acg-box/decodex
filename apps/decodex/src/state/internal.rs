use std::env;

use libc::{F_GETFD, F_SETFD, FD_CLOEXEC};
use rusqlite::{self, Row};

const REVIEW_LIFECYCLE_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS review_lifecycle_records (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	branch_name TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	pr_url TEXT NOT NULL,
	target_base_ref_name TEXT,
	pr_head_ref_name TEXT NOT NULL,
	pr_head_oid TEXT NOT NULL,
	head_sha TEXT NOT NULL,
	phase TEXT NOT NULL,
	request_comment_database_id INTEGER,
	request_created_at_unix_epoch INTEGER,
	request_description_thumbs_up_count INTEGER,
	request_retry_count INTEGER NOT NULL,
	external_round_count INTEGER NOT NULL,
	auto_merge_enabled_at_unix_epoch INTEGER,
	landing_state TEXT NOT NULL DEFAULT 'not_started',
	closeout_state TEXT NOT NULL DEFAULT 'not_started',
	repair_attempt_count INTEGER NOT NULL DEFAULT 0,
	evidence_json TEXT NOT NULL DEFAULT '{}',
	next_action TEXT NOT NULL DEFAULT '',
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, branch_name)
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
"#;
const EVIDENCE_ARTIFACT_SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS evidence_artifacts (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	artifact_kind TEXT NOT NULL,
	key_hash TEXT NOT NULL,
	phase TEXT NOT NULL,
	status TEXT NOT NULL,
	head_sha TEXT,
	key_json TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	source_run_id TEXT NOT NULL,
	source_attempt_number INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, artifact_kind, key_hash)
);
CREATE INDEX IF NOT EXISTS evidence_artifacts_lookup_idx
ON evidence_artifacts (project_id, issue_id, artifact_kind, phase, head_sha, status);
"#;
const DROP_LEGACY_REVIEW_MARKER_TABLES_SQL: &str = r#"
DROP TABLE IF EXISTS review_handoffs;
DROP TABLE IF EXISTS review_orchestrations;
"#;
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
				let Self { project_id: _, slot_index: _, lock_path, lock_file, retention: _ } =
					self;

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
	run_activity_summaries: HashMap<String, RunActivitySummaryRecord>,
	worktrees: HashMap<String, WorktreeMappingRecord>,
	linear_execution_events: HashMap<String, LinearExecutionEventRuntimeRecord>,
	private_execution_events: Vec<PrivateExecutionEventRuntimeRecord>,
	decision_contracts: HashMap<DecisionContractKey, DecisionContractRuntimeRecord>,
	autonomy_objectives: HashMap<AutonomyObjectiveKey, AutonomyObjectiveRuntimeRecord>,
	autonomy_signals: HashMap<AutonomySignalKey, AutonomySignalRuntimeRecord>,
	autonomy_proposals: HashMap<AutonomyProposalKey, AutonomyProposalRuntimeRecord>,
	execution_programs: HashMap<ExecutionProgramKey, ExecutionProgramRuntimeRecord>,
	program_intake_plans: HashMap<ProgramIntakePlanKey, ProgramIntakePlanRecord>,
	program_issue_mappings: HashMap<ProgramIssueMappingKey, ProgramIssueMappingRecord>,
	review_lifecycle_records: HashMap<ReviewLifecycleKey, ReviewLifecycleRuntimeRecord>,
	review_policy_checkpoints: HashMap<ReviewPolicyKey, ReviewPolicyRuntimeRecord>,
	evidence_artifacts: HashMap<EvidenceArtifactKey, EvidenceArtifactRuntimeRecord>,
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
		self.run_activity_summaries = loaded.run_activity_summaries;
		self.worktrees = loaded.worktrees;
		self.linear_execution_events = loaded.linear_execution_events;
		self.private_execution_events = loaded.private_execution_events;
		self.decision_contracts = loaded.decision_contracts;
		self.autonomy_objectives = loaded.autonomy_objectives;
		self.autonomy_signals = loaded.autonomy_signals;
		self.autonomy_proposals = loaded.autonomy_proposals;
		self.execution_programs = loaded.execution_programs;
		self.program_intake_plans = loaded.program_intake_plans;
		self.program_issue_mappings = loaded.program_issue_mappings;
		self.review_lifecycle_records = loaded.review_lifecycle_records;
		self.review_policy_checkpoints = loaded.review_policy_checkpoints;
		self.evidence_artifacts = loaded.evidence_artifacts;
		self.loop_guardrail_checkpoints = loaded.loop_guardrail_checkpoints;
		self.connector_backoffs = loaded.connector_backoffs;
	}

	fn replace_project_run_metadata_state(&mut self, loaded: Self) {
		self.leases = loaded.leases;
		self.run_attempts = loaded.run_attempts;
		self.control_channels = loaded.control_channels;
		self.run_activity_summaries = loaded.run_activity_summaries;
		self.worktrees = loaded.worktrees;
	}

	fn replace_project_loop_evidence_state(&mut self, project_id: &str, loaded: Self) {
		self.private_execution_events.retain(|record| record.project_id != project_id);
		self.private_execution_events.extend(loaded.private_execution_events);
		self.review_lifecycle_records.retain(|key, _record| key.project_id != project_id);
		self.review_lifecycle_records.extend(loaded.review_lifecycle_records);
		self.review_policy_checkpoints.retain(|key, _record| key.project_id != project_id);
		self.review_policy_checkpoints.extend(loaded.review_policy_checkpoints);
		self.evidence_artifacts.retain(|key, _record| key.project_id != project_id);
		self.evidence_artifacts.extend(loaded.evidence_artifacts);
		self.autonomy_signals.retain(|key, _record| key.project_id != project_id);
		self.autonomy_signals.extend(loaded.autonomy_signals);
		self.autonomy_proposals.retain(|key, _record| key.project_id != project_id);
		self.autonomy_proposals.extend(loaded.autonomy_proposals);
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
		let run_lease = self
			.leases
			.get(&attempt.issue_id)
			.is_some_and(|lease| lease.project_id == project_id && lease.run_id == attempt.run_id);
		let remembered_project = attempt.project_id.as_deref() == Some(project_id);
		let in_project = remembered_project
			|| worktree.is_some_and(|mapping| mapping.project_id == project_id)
			|| run_lease;

		if !in_project {
			return None;
		}

		let event_summary = self.protocol_event_summary(&attempt.run_id);
		let run_activity_summary = self.run_activity_summaries.get(&attempt.run_id);
		let control_channel = self
			.control_channels
			.get(&attempt.run_id)
			.filter(|channel| {
				channel.project_id == project_id
					&& channel.issue_id == attempt.issue_id
					&& channel.attempt_number == attempt.attempt_number
			})
			.map(RunControlChannelRecord::as_public);
		let mut recovery_evidence = vec![String::from("run_attempt")];

		if run_lease {
			recovery_evidence.push(String::from("active_lease"));
		}
		if control_channel.is_some() {
			recovery_evidence.push(String::from("run_control_channel"));
		}
		if event_summary.event_count > 0 {
			recovery_evidence.push(format!("protocol_events:{}", event_summary.event_count));
		}
		if run_activity_summary.and_then(|summary| summary.child_agent_activity.as_ref()).is_some()
		{
			recovery_evidence.push(String::from("child_agent_activity_summary"));
		}
		if run_activity_summary.and_then(|summary| summary.protocol_activity.as_ref()).is_some() {
			recovery_evidence.push(String::from("protocol_activity_summary"));
		}

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
			run_lease,
			event_count: event_summary.event_count,
			last_event_type: event_summary.last_event_type,
			last_event_at: event_summary.last_event_at,
			last_event_at_unix: event_summary.last_event_at_unix,
			control_channel,
			child_agent_activity: run_activity_summary
				.and_then(|summary| summary.child_agent_activity.clone()),
			protocol_activity: run_activity_summary
				.and_then(|summary| summary.protocol_activity.clone()),
			recovery_source: String::from("recorded"),
			recovery_evidence,
			recovery_gaps: Vec::new(),
		})
	}

	fn protocol_event_summary(&self, run_id: &str) -> ProtocolEventSummaryRecord {
		self.event_summaries
			.get(run_id)
			.cloned()
			.or_else(|| {
				self.events.get(run_id).map(|events| protocol_event_summary_from_events(events))
			})
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
	payload_sha256 TEXT NOT NULL,
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
CREATE TABLE IF NOT EXISTS run_activity_summaries (
	run_id TEXT PRIMARY KEY NOT NULL,
	attempt_number INTEGER NOT NULL,
	child_agent_activity_json TEXT,
	protocol_activity_json TEXT,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL
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
		self.bootstrap_evidence_artifact_schema()?;
		self.bootstrap_run_control_channels_schema()?;
		self.bootstrap_connector_backoffs_schema()?;
		self.bootstrap_private_execution_events_schema()?;
		self.bootstrap_decision_contracts_schema()?;
		self.bootstrap_autonomy_objectives_schema()?;
		self.bootstrap_autonomy_signals_schema()?;
		self.bootstrap_autonomy_proposals_schema()?;
		self.bootstrap_execution_programs_schema()?;
		self.bootstrap_program_intake_state_schema()?;
		self.bootstrap_loop_guardrail_schema()?;
		self.run_schema_migrations()?;
		self.record_schema_version()?;
		self.seal_run_activity_summary_records()?;
		self.connection.execute_batch("PRAGMA optimize=0x10002;")?;

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
		self.connection.execute_batch(DROP_LEGACY_REVIEW_MARKER_TABLES_SQL)?;
		self.connection.execute_batch(REVIEW_LIFECYCLE_SCHEMA_SQL)?;
		self.ensure_column(
			"review_policy_checkpoints",
			"details_json",
			"ALTER TABLE review_policy_checkpoints ADD COLUMN details_json TEXT NOT NULL DEFAULT '{}'",
		)?;

		Ok(())
	}

	fn bootstrap_evidence_artifact_schema(&self) -> Result<()> {
		self.connection.execute_batch(EVIDENCE_ARTIFACT_SCHEMA_SQL)?;

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

	fn bootstrap_autonomy_objectives_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS autonomy_objectives (
	project_id TEXT NOT NULL,
	objective_id TEXT NOT NULL,
	version INTEGER NOT NULL,
	state TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, objective_id, version)
);
CREATE INDEX IF NOT EXISTS autonomy_objectives_project_state_idx
ON autonomy_objectives (project_id, state, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_objectives_history_idx
ON autonomy_objectives (project_id, objective_id, version);
"#,
		)?;

		Ok(())
	}

	fn bootstrap_autonomy_signals_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS autonomy_signals (
	project_id TEXT NOT NULL,
	signal_id TEXT NOT NULL,
	objective_id TEXT NOT NULL,
	objective_version INTEGER NOT NULL,
	kind TEXT NOT NULL,
	fingerprint TEXT NOT NULL,
	freshness TEXT NOT NULL,
	evidence_class TEXT NOT NULL,
	confidence TEXT NOT NULL,
	privacy TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, signal_id)
);
CREATE INDEX IF NOT EXISTS autonomy_signals_objective_idx
ON autonomy_signals (project_id, objective_id, objective_version, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_signals_recent_idx
ON autonomy_signals (project_id, updated_at_unix);
"#,
		)?;

		Ok(())
	}

	fn bootstrap_autonomy_proposals_schema(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS autonomy_proposals (
	project_id TEXT NOT NULL,
	proposal_id TEXT NOT NULL,
	objective_id TEXT NOT NULL,
	objective_version INTEGER NOT NULL,
	state TEXT NOT NULL,
	fingerprint TEXT NOT NULL,
	source_family TEXT NOT NULL,
	intended_surface TEXT NOT NULL,
	payload_json TEXT NOT NULL,
	created_at TEXT NOT NULL,
	created_at_unix INTEGER NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, proposal_id)
);
CREATE INDEX IF NOT EXISTS autonomy_proposals_objective_idx
ON autonomy_proposals (project_id, objective_id, objective_version, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_proposals_state_idx
ON autonomy_proposals (project_id, state, updated_at_unix);
CREATE INDEX IF NOT EXISTS autonomy_proposals_recent_idx
ON autonomy_proposals (project_id, updated_at_unix);
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
		let columns =
			statement.query_map([], |row| Ok((row.get::<_, String>(1)?, row.get::<_, i64>(3)?)))?;
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
DROP TABLE IF EXISTS program_issue_mappings;
DROP TABLE IF EXISTS program_queue_label_ownership;
CREATE TABLE IF NOT EXISTS program_issue_mappings (
	project_id TEXT NOT NULL,
	program_id TEXT NOT NULL,
	node_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	issue_identifier TEXT NOT NULL,
	issue_state TEXT NOT NULL,
	queue_intent TEXT NOT NULL,
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

	fn schema_version(&self) -> Result<Option<i64>> {
		self.ensure_schema_meta_table()?;
		let version = self
			.connection
			.query_row(
				"SELECT value FROM schema_meta WHERE key = 'schema_version'",
				[],
				|row| row.get::<_, String>(0),
			)
			.optional()?
			.and_then(|value| value.parse::<i64>().ok());

		Ok(version)
	}

	fn ensure_schema_meta_table(&self) -> Result<()> {
		self.connection.execute_batch(
			r#"
CREATE TABLE IF NOT EXISTS schema_meta (
	key TEXT PRIMARY KEY NOT NULL,
	value TEXT NOT NULL
);
"#,
		)?;

		Ok(())
	}

	fn schema_migration_completed(&self, key: &str) -> Result<bool> {
		self.ensure_schema_meta_table()?;
		let value = self
			.connection
			.query_row("SELECT value FROM schema_meta WHERE key = ?1", params![key], |row| {
				row.get::<_, String>(0)
			})
			.optional()?;

		Ok(value.as_deref() == Some("completed"))
	}

	fn record_schema_migration_completed(&self, key: &str) -> Result<()> {
		self.ensure_schema_meta_table()?;
		self.connection.execute(
			"INSERT INTO schema_meta (key, value)
			 VALUES (?1, 'completed')
			 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
			params![key],
		)?;

		Ok(())
	}

	fn run_schema_migrations(&self) -> Result<()> {
		let version = self.schema_version()?.unwrap_or(0);

		if version < 12 {
			if !self.schema_migration_completed(
				"migration:protocol_event_summaries_from_events:v12",
			)? {
				self.backfill_protocol_event_summaries_from_events()?;
				self.record_schema_migration_completed(
					"migration:protocol_event_summaries_from_events:v12",
				)?;
			}
			self.migrate_legacy_decision_contract_issue_summaries()?;
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
VALUES ('schema_version', '12')
ON CONFLICT(key) DO UPDATE SET value =
	CASE
		WHEN CAST(schema_meta.value AS INTEGER) < CAST(excluded.value AS INTEGER)
		THEN excluded.value
		ELSE schema_meta.value
	END;
"#,
		)?;

		Ok(())
	}

	fn backfill_protocol_event_summaries_from_events(&self) -> Result<()> {
		let now = timestamp_parts();

		self.connection.execute(
			"INSERT INTO protocol_event_summaries (
					run_id, event_count, last_sequence_number, last_event_type, last_event_at,
					last_event_at_unix, compacted_at, compacted_at_unix
				)
			 SELECT totals.run_id, totals.event_count, totals.last_sequence_number,
					last.event_type, last.created_at, last.created_at_unix, ?1, ?2
			 FROM (
				 SELECT run_id, COUNT(*) AS event_count, MAX(sequence_number) AS last_sequence_number
				 FROM protocol_events
				 GROUP BY run_id
			 ) totals
			 JOIN protocol_events last
			 ON last.run_id = totals.run_id
			 AND last.sequence_number = totals.last_sequence_number
			 ON CONFLICT(run_id) DO UPDATE SET
				 event_count = excluded.event_count,
				 last_sequence_number = excluded.last_sequence_number,
				 last_event_type = excluded.last_event_type,
				 last_event_at = excluded.last_event_at,
				 last_event_at_unix = excluded.last_event_at_unix,
				 compacted_at = excluded.compacted_at,
				 compacted_at_unix = excluded.compacted_at_unix",
			params![now.text, now.unix],
		)?;

		Ok(())
	}

	fn migrate_legacy_decision_contract_issue_summaries(&self) -> Result<()> {
		let updates = {
			let mut statement = self.connection.prepare(
				"SELECT project_id, contract_id, payload_json
				 FROM decision_contracts
				 WHERE json_type(payload_json, '$.execution_readiness.proposed_issue_summaries') IS NOT NULL
				 OR json_type(payload_json, '$.execution_readiness.queue_intent') IS NOT NULL
				 ORDER BY project_id ASC, contract_id ASC",
			)?;
			let rows = statement.query_map([], |row| {
				Ok((
					row.get::<_, String>(0)?,
					row.get::<_, String>(1)?,
					row.get::<_, String>(2)?,
				))
			})?;
			let mut updates = Vec::new();

			for row in rows {
				let (project_id, contract_id, payload_json) = row?;
				let migrated_payload = migrate_legacy_decision_contract_payload(&payload_json)
					.map_err(|error| {
						eyre::eyre!(
							"Decision Contract `{project_id}/{contract_id}` legacy payload migration failed: {error}"
						)
					})?;

				if migrated_payload != payload_json {
					updates.push((project_id, contract_id, migrated_payload));
				}
			}

			updates
		};

		for (project_id, contract_id, payload_json) in updates {
			self.connection.execute(
				"UPDATE decision_contracts
				 SET payload_json = ?3
				 WHERE project_id = ?1 AND contract_id = ?2",
				params![project_id, contract_id, payload_json],
			)?;
		}

		Ok(())
	}

	fn seal_run_activity_summary_records(&self) -> Result<()> {
		let updates = {
			let mut statement = self.connection.prepare(
				"SELECT run_id, child_agent_activity_json FROM run_activity_summaries \
				 WHERE child_agent_activity_json IS NOT NULL",
			)?;
			let rows = statement
				.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
			let mut updates = Vec::new();

			for row in rows {
				let (run_id, child_agent_activity_json) = row?;
				let sealed_json = serde_json::to_string(
					&serde_json::from_str::<ChildAgentActivitySummary>(&child_agent_activity_json)?
						.sealed_durable(),
				)?;

				if sealed_json != child_agent_activity_json {
					updates.push((run_id, sealed_json));
				}
			}

			updates
		};

		for (run_id, child_agent_activity_json) in updates {
			self.connection.execute(
				"UPDATE run_activity_summaries SET child_agent_activity_json = ?2 WHERE run_id = ?1",
				params![run_id, child_agent_activity_json],
			)?;
		}

		Ok(())
	}

	fn load_state(&self) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_projects(&mut state)?;
		self.load_leases(&mut state)?;
		self.load_run_attempts(&mut state)?;
		self.load_run_control_channels(&mut state)?;
		self.load_protocol_event_summaries(&mut state)?;
		self.load_run_activity_summaries(&mut state)?;
		self.load_worktrees(&mut state)?;
		self.load_linear_execution_events(&mut state)?;
		self.load_private_execution_events(&mut state)?;
		self.load_decision_contracts(&mut state)?;
		self.load_autonomy_objectives(&mut state)?;
		self.load_autonomy_signals(&mut state)?;
		self.load_autonomy_proposals(&mut state)?;
		self.load_execution_programs(&mut state)?;
		self.load_program_intake_state(&mut state)?;
		self.load_review_lifecycle_records(&mut state)?;
		self.load_review_policy_checkpoints(&mut state)?;
		self.load_evidence_artifacts(&mut state)?;
		self.load_loop_guardrail_checkpoints(&mut state)?;
		self.load_connector_backoffs(&mut state)?;

		Ok(state)
	}

	fn load_project_run_metadata_for_project(&self, project_id: &str) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_leases(&mut state)?;
		self.load_run_attempts_for_project(&mut state, project_id)?;
		self.load_run_activity_summaries_for_loaded_runs(&mut state)?;
		self.load_worktrees(&mut state)?;
		self.load_run_control_channels_for_project(&mut state, project_id)?;

		Ok(state)
	}

	fn load_project_loop_evidence_for_project(&self, project_id: &str) -> Result<StateData> {
		let mut state = StateData::default();

		self.load_private_execution_events_for_project(&mut state, project_id)?;
		self.load_review_lifecycle_records_for_project(&mut state, project_id)?;
		self.load_review_policy_checkpoints_for_project(&mut state, project_id)?;
		self.load_evidence_artifacts_for_project(&mut state, project_id)?;
		self.load_autonomy_signals_for_project(&mut state, project_id)?;
		self.load_autonomy_proposals_for_project(&mut state, project_id)?;

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
		persist_run_activity_summaries(&transaction, state)?;
		persist_worktrees(&transaction, state)?;
		persist_linear_execution_events(&transaction, state)?;
		persist_private_execution_events(&transaction, state)?;
		persist_decision_contracts(&transaction, state)?;
		persist_autonomy_objectives(&transaction, state)?;
		persist_autonomy_signals(&transaction, state)?;
		persist_autonomy_proposals(&transaction, state)?;
		persist_execution_programs(&transaction, state)?;
		persist_program_intake_state(&transaction, state)?;
		persist_review_lifecycle_records(&transaction, state)?;
		persist_review_policy_checkpoints(&transaction, state)?;
		persist_evidence_artifacts(&transaction, state)?;
		persist_loop_guardrail_checkpoints(&transaction, state)?;
		persist_connector_backoffs(&transaction, state)?;

		transaction.commit()?;

		Ok(())
	}

	fn delete_project(&mut self, service_id: &str) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute("DELETE FROM projects WHERE service_id = ?1", params![service_id])?;
		transaction
			.execute("DELETE FROM connector_backoffs WHERE project_id = ?1", params![service_id])?;
		transaction.execute(
			"DELETE FROM run_control_channels WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction
			.execute("DELETE FROM decision_contracts WHERE project_id = ?1", params![service_id])?;
		transaction.execute(
			"DELETE FROM autonomy_objectives WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction
			.execute("DELETE FROM autonomy_signals WHERE project_id = ?1", params![service_id])?;
		transaction
			.execute("DELETE FROM autonomy_proposals WHERE project_id = ?1", params![service_id])?;
		transaction
			.execute("DELETE FROM execution_programs WHERE project_id = ?1", params![service_id])?;
		transaction.execute(
			"DELETE FROM program_intake_plans WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction.execute(
			"DELETE FROM program_issue_mappings WHERE project_id = ?1",
			params![service_id],
		)?;
		transaction
			.execute("DELETE FROM evidence_artifacts WHERE project_id = ?1", params![service_id])?;
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

	fn upsert_run_activity_summary(&self, summary: &RunActivitySummaryRecord) -> Result<()> {
		let child_agent_activity_json = summary
			.child_agent_activity
			.as_ref()
			.cloned()
			.map(ChildAgentActivitySummary::sealed_durable)
			.map(|summary| serde_json::to_string(&summary))
			.transpose()?;
		let protocol_activity_json =
			summary.protocol_activity.as_ref().map(serde_json::to_string).transpose()?;

		self.connection.execute(
			"INSERT OR REPLACE INTO run_activity_summaries (
					run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			params![
				&summary.run_id,
				summary.attempt_number,
				child_agent_activity_json.as_deref(),
				protocol_activity_json.as_deref(),
				&summary.updated_at,
				summary.updated_at_unix,
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

		update_run_attempt_project(
			&transaction,
			lease.project_id(),
			lease.issue_id(),
			Some(lease.run_id()),
		)?;

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
					run_id, sequence_number, event_type, payload_sha256, created_at, created_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			params![
				run_id,
				event.sequence_number,
				&event.event_type,
				&event.payload_sha256,
				&event.created_at,
				event.created_at_unix,
			],
		)?;

		Ok(changed == 1)
	}

	fn protocol_event(
		&self,
		run_id: &str,
		sequence_number: i64,
	) -> Result<Option<ProtocolEventRecord>> {
		Ok(self
			.connection
			.query_row(
				"SELECT sequence_number, event_type, payload_sha256, created_at, created_at_unix \
				 FROM protocol_events WHERE run_id = ?1 AND sequence_number = ?2",
				params![run_id, sequence_number],
				protocol_event_record_from_row,
			)
			.optional()?)
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
	fn upsert_autonomy_objective(&self, record: &AutonomyObjectiveRuntimeRecord) -> Result<()> {
		let payload_json = serde_json::to_string(&record.objective)?;
		let version = i64::try_from(record.objective.version())
			.map_err(|_| eyre::eyre!("Autonomy objective version exceeds SQLite integer range."))?;

		self.connection.execute(
			"INSERT INTO autonomy_objectives (
					project_id, objective_id, version, state, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
			 ON CONFLICT(project_id, objective_id, version) DO UPDATE SET
				 state = excluded.state,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.objective.id(),
				version,
				record.state.as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}

	fn upsert_autonomy_signal(&self, record: &AutonomySignalRuntimeRecord) -> Result<()> {
		let payload_json = serde_json::to_string(&record.signal)?;
		let version = i64::try_from(record.signal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy signal objective_version exceeds SQLite integer range.")
		})?;

		self.connection.execute(
			"INSERT INTO autonomy_signals (
					project_id, signal_id, objective_id, objective_version, kind, fingerprint,
					freshness, evidence_class, confidence, privacy, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
			 ON CONFLICT(project_id, signal_id) DO UPDATE SET
				 objective_id = excluded.objective_id,
				 objective_version = excluded.objective_version,
				 kind = excluded.kind,
				 fingerprint = excluded.fingerprint,
				 freshness = excluded.freshness,
				 evidence_class = excluded.evidence_class,
				 confidence = excluded.confidence,
				 privacy = excluded.privacy,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.signal.id(),
				record.signal.objective_id(),
				version,
				record.signal.kind().as_str(),
				record.signal.fingerprint(),
				record.signal.freshness().as_str(),
				record.signal.evidence_class().as_str(),
				record.signal.confidence().as_str(),
				record.signal.privacy().as_str(),
				payload_json,
				&record.created_at,
				record.created_at_unix,
				&record.updated_at,
				record.updated_at_unix,
			],
		)?;

		Ok(())
	}

	fn upsert_autonomy_proposal(&self, record: &AutonomyProposalRuntimeRecord) -> Result<()> {
		let payload_json = serde_json::to_string(&record.proposal)?;
		let version = i64::try_from(record.proposal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy proposal objective_version exceeds SQLite integer range.")
		})?;

		self.connection.execute(
			"INSERT INTO autonomy_proposals (
					project_id, proposal_id, objective_id, objective_version, state, fingerprint,
					source_family, intended_surface, payload_json, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
			 ON CONFLICT(project_id, proposal_id) DO UPDATE SET
				 objective_id = excluded.objective_id,
				 objective_version = excluded.objective_version,
				 state = excluded.state,
				 fingerprint = excluded.fingerprint,
				 source_family = excluded.source_family,
				 intended_surface = excluded.intended_surface,
				 payload_json = excluded.payload_json,
				 updated_at = excluded.updated_at,
				 updated_at_unix = excluded.updated_at_unix",
			params![
				&record.project_id,
				record.proposal.id(),
				record.proposal.objective_id(),
				version,
				record.state.as_str(),
				record.proposal.fingerprint(),
				record.proposal.source_family(),
				record.proposal.intended_surface(),
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

		insert_program_intake_state(&self.connection, record)
	}

	fn delete_lease(&mut self, issue_id: &str) -> Result<()> {
		self.connection.execute("DELETE FROM leases WHERE issue_id = ?1", params![issue_id])?;

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
		transaction
			.execute("DELETE FROM leases WHERE issue_id = ?1", params![previous_issue_id])?;
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
		transaction
			.execute("DELETE FROM worktrees WHERE issue_id = ?1", params![previous_issue_id])?;
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
			"INSERT OR IGNORE INTO evidence_artifacts (
					project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha,
					key_json, payload_json, source_run_id, source_attempt_number, updated_at,
					updated_at_unix
				)
			 SELECT project_id, ?2, artifact_kind, key_hash, phase, status, head_sha,
					key_json, payload_json, source_run_id, source_attempt_number, updated_at,
					updated_at_unix
			 FROM evidence_artifacts WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM evidence_artifacts WHERE issue_id = ?1",
			params![previous_issue_id],
		)?;
		transaction.execute(
			"INSERT OR IGNORE INTO review_lifecycle_records (
					project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase,
					request_comment_database_id, request_created_at_unix_epoch,
					request_description_thumbs_up_count, request_retry_count, external_round_count,
					auto_merge_enabled_at_unix_epoch, landing_state, closeout_state,
					repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix
				)
			 SELECT project_id, ?2, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase,
					request_comment_database_id, request_created_at_unix_epoch,
					request_description_thumbs_up_count, request_retry_count, external_round_count,
					auto_merge_enabled_at_unix_epoch, landing_state, closeout_state,
					repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix
			 FROM review_lifecycle_records WHERE issue_id = ?1",
			params![previous_issue_id, canonical_issue_id],
		)?;
		transaction.execute(
			"DELETE FROM review_lifecycle_records WHERE issue_id = ?1",
			params![previous_issue_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

	fn delete_worktree_and_review_lifecycle(&mut self, issue_id: &str) -> Result<()> {
		let transaction = self.connection.transaction()?;

		transaction.execute("DELETE FROM worktrees WHERE issue_id = ?1", params![issue_id])?;
		transaction.execute(
			"DELETE FROM review_lifecycle_records WHERE issue_id = ?1",
			params![issue_id],
		)?;
		transaction.execute(
			"DELETE FROM review_policy_checkpoints WHERE issue_id = ?1",
			params![issue_id],
		)?;
		transaction
			.execute("DELETE FROM evidence_artifacts WHERE issue_id = ?1", params![issue_id])?;
		transaction.execute(
			"DELETE FROM loop_guardrail_checkpoints WHERE issue_id = ?1",
			params![issue_id],
		)?;
		transaction.commit()?;

		Ok(())
	}

	fn delete_worktree_mapping(&mut self, issue_id: &str) -> Result<()> {
		self.connection.execute("DELETE FROM worktrees WHERE issue_id = ?1", params![issue_id])?;

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
			"DELETE FROM review_lifecycle_records
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
		self.load_compacted_protocol_event_summaries(state)
	}

	fn load_protocol_event_summaries_for_runs(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		for run_id in run_ids {
			state.event_summaries.remove(run_id);
			if !self.load_compacted_protocol_event_summary_for_run(state, run_id)? {
				self.load_protocol_event_summary_for_run(state, run_id)?;
			}
		}

		Ok(())
	}

	fn rebuild_protocol_event_summaries_for_runs(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		for run_id in run_ids {
			state.event_summaries.remove(run_id);
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
			"SELECT totals.event_count, totals.last_sequence_number, last.event_type, \
			 last.created_at, last.created_at_unix \
			 FROM (
			 SELECT COUNT(*) AS event_count, MAX(sequence_number) AS last_sequence_number \
			 FROM protocol_events WHERE run_id = ?1
			 ) totals \
			 JOIN protocol_events last \
			 ON last.run_id = ?1 \
			 AND last.sequence_number = totals.last_sequence_number",
		)?;
		let summary = statement
			.query_row(params![run_id], |row| {
				Ok(ProtocolEventSummaryRecord {
					event_count: row.get(0)?,
					last_sequence_number: Some(row.get(1)?),
					last_event_type: Some(row.get(2)?),
					last_event_at: Some(row.get(3)?),
					last_event_at_unix: Some(row.get(4)?),
				})
			})
			.optional()?;

		if let Some(summary) = summary {
			self.upsert_protocol_event_summary(run_id, &summary)?;
			state.event_summaries.insert(run_id.to_owned(), summary);
		}

		Ok(())
	}

	fn load_run_activity_summaries(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
			 updated_at, updated_at_unix FROM run_activity_summaries ORDER BY run_id",
		)?;
		let rows = statement.query_map([], run_activity_summary_record_from_row)?;

		for row in rows {
			let summary = row?;

			state.run_activity_summaries.insert(summary.run_id.clone(), summary);
		}

		Ok(())
	}

	fn load_run_activity_summaries_for_loaded_runs(&self, state: &mut StateData) -> Result<()> {
		let run_ids = state.run_attempts.keys().cloned().collect::<Vec<_>>();

		self.load_run_activity_summaries_for_runs(state, &run_ids)
	}

	fn load_run_activity_summaries_for_runs(
		&self,
		state: &mut StateData,
		run_ids: &[String],
	) -> Result<()> {
		for run_id in run_ids {
			self.load_run_activity_summary_for_run(state, run_id)?;
		}

		Ok(())
	}

	fn load_run_activity_summary_for_run(&self, state: &mut StateData, run_id: &str) -> Result<()> {
		state.run_activity_summaries.remove(run_id);

		let mut statement = self.connection.prepare(
			"SELECT run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
			 updated_at, updated_at_unix FROM run_activity_summaries WHERE run_id = ?1",
		)?;
		let summary = statement
			.query_row(params![run_id], run_activity_summary_record_from_row)
			.optional()?;

		if let Some(summary) = summary {
			state.run_activity_summaries.insert(run_id.to_owned(), summary);
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
	) -> Result<bool> {
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

			return Ok(true);
		}

		Ok(false)
	}

	fn upsert_protocol_event_summary(
		&self,
		run_id: &str,
		summary: &ProtocolEventSummaryRecord,
	) -> Result<()> {
		let now = timestamp_parts();

		self.connection.execute(
			"INSERT OR REPLACE INTO protocol_event_summaries (
					run_id, event_count, last_sequence_number, last_event_type, last_event_at,
					last_event_at_unix, compacted_at, compacted_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
			params![
				run_id,
				summary.event_count,
				summary.last_sequence_number,
				summary.last_event_type.as_deref(),
				summary.last_event_at.as_deref(),
				summary.last_event_at_unix,
				now.text,
				now.unix,
			],
		)?;

		Ok(())
	}

	fn load_worktrees(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
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

			state.linear_execution_events.insert(record.record.idempotency_key.clone(), record);
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

		rows.next()?
			.map(decision_contract_runtime_row_parts)
			.transpose()?
			.map(decision_contract_record_from_row_parts)
			.transpose()
	}

	fn decision_contract_for_readback(
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
		let Some(parts) = rows.next()?.map(decision_contract_runtime_row_parts).transpose()? else {
			return Ok(None);
		};

		decision_contract_record_from_row_parts(parts).map(Some)
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
		let rows = statement
			.query_map(params![project_id, source_issue_id], decision_contract_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(decision_contract_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	fn list_decision_contracts_for_project(
		&self,
		project_id: &str,
	) -> Result<Vec<DecisionContractRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, contract_id, source_issue_id, status, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM decision_contracts \
			 WHERE project_id = ?1 \
			 ORDER BY created_at_unix ASC, contract_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], decision_contract_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(decision_contract_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	fn load_autonomy_objectives(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 ORDER BY project_id ASC, objective_id ASC, version ASC",
		)?;
		let rows = statement.query_map([], autonomy_objective_runtime_row_parts)?;

		for row in rows {
			let record = autonomy_objective_record_from_row_parts(row?)?;

			state.autonomy_objectives.insert(record.key(), record);
		}

		Ok(())
	}

	fn autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		version: u64,
	) -> Result<Option<AutonomyObjectiveRuntimeRecord>> {
		let version = i64::try_from(version)
			.map_err(|_| eyre::eyre!("Autonomy objective version exceeds SQLite integer range."))?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 AND objective_id = ?2 AND version = ?3 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, objective_id, version])?;

		rows.next()?
			.map(autonomy_objective_runtime_row_parts)
			.transpose()?
			.map(autonomy_objective_record_from_row_parts)
			.transpose()
	}

	fn current_accepted_autonomy_objective(
		&self,
		project_id: &str,
		objective_id: &str,
	) -> Result<Option<AutonomyObjectiveRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 AND objective_id = ?2 AND state = 'accepted' \
			 ORDER BY version DESC \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, objective_id])?;

		rows.next()?
			.map(autonomy_objective_runtime_row_parts)
			.transpose()?
			.map(autonomy_objective_record_from_row_parts)
			.transpose()
	}

	fn list_autonomy_objective_history(
		&self,
		project_id: &str,
		objective_id: &str,
	) -> Result<Vec<AutonomyObjectiveRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 AND objective_id = ?2 \
			 ORDER BY version ASC",
		)?;
		let rows = statement
			.query_map(params![project_id, objective_id], autonomy_objective_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_objective_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	fn recent_autonomy_objectives_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyObjectiveRuntimeRecord>> {
		let limit = i64::try_from(limit).unwrap_or(i64::MAX);
		let mut statement = self.connection.prepare(
			"SELECT project_id, objective_id, version, state, payload_json, created_at, created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_objectives \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, objective_id ASC, version ASC \
			 LIMIT ?2",
		)?;
		let rows = statement
			.query_map(params![project_id, limit], autonomy_objective_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_objective_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	fn load_autonomy_signals(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 ORDER BY project_id ASC, objective_id ASC, objective_version ASC, updated_at_unix ASC",
		)?;
		let rows = statement.query_map([], autonomy_signal_runtime_row_parts)?;

		for row in rows {
			let record = autonomy_signal_record_from_row_parts(row?)?;

			state.autonomy_signals.insert(record.key(), record);
		}

		Ok(())
	}

	fn load_autonomy_signals_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, signal_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], autonomy_signal_runtime_row_parts)?;

		for row in rows {
			let record = autonomy_signal_record_from_row_parts(row?)?;

			state.autonomy_signals.insert(record.key(), record);
		}

		Ok(())
	}

	fn autonomy_signal(
		&self,
		project_id: &str,
		signal_id: &str,
	) -> Result<Option<AutonomySignalRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 AND signal_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, signal_id])?;

		rows.next()?
			.map(autonomy_signal_runtime_row_parts)
			.transpose()?
			.map(autonomy_signal_record_from_row_parts)
			.transpose()
	}

	fn list_autonomy_signals_for_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		objective_version: u64,
	) -> Result<Vec<AutonomySignalRuntimeRecord>> {
		let version = i64::try_from(objective_version).map_err(|_| {
			eyre::eyre!("Autonomy signal objective_version exceeds SQLite integer range.")
		})?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 AND objective_id = ?2 AND objective_version = ?3 \
			 ORDER BY updated_at_unix ASC, signal_id ASC",
		)?;
		let rows = statement.query_map(
			params![project_id, objective_id, version],
			autonomy_signal_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_signal_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	fn recent_autonomy_signals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomySignalRuntimeRecord>> {
		let limit = i64::try_from(limit).map_err(|_| {
			eyre::eyre!("Autonomy signal readback limit exceeds SQLite integer range.")
		})?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, signal_id, objective_id, objective_version, kind, fingerprint, \
			 freshness, evidence_class, confidence, privacy, payload_json, created_at, \
			 created_at_unix, updated_at, updated_at_unix \
			 FROM autonomy_signals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, signal_id ASC \
			 LIMIT ?2",
		)?;
		let rows =
			statement.query_map(params![project_id, limit], autonomy_signal_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_signal_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	fn load_autonomy_proposals(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 ORDER BY project_id ASC, objective_id ASC, objective_version ASC, updated_at_unix ASC",
		)?;
		let rows = statement.query_map([], autonomy_proposal_runtime_row_parts)?;

		for row in rows {
			let record = autonomy_proposal_record_from_row_parts(row?)?;

			state.autonomy_proposals.insert(record.key(), record);
		}

		Ok(())
	}

	fn load_autonomy_proposals_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, proposal_id ASC",
		)?;
		let rows = statement.query_map(params![project_id], autonomy_proposal_runtime_row_parts)?;

		for row in rows {
			let record = autonomy_proposal_record_from_row_parts(row?)?;

			state.autonomy_proposals.insert(record.key(), record);
		}

		Ok(())
	}

	fn autonomy_proposal(
		&self,
		project_id: &str,
		proposal_id: &str,
	) -> Result<Option<AutonomyProposalRuntimeRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 AND proposal_id = ?2 \
			 LIMIT 1",
		)?;
		let mut rows = statement.query(params![project_id, proposal_id])?;

		rows.next()?
			.map(autonomy_proposal_runtime_row_parts)
			.transpose()?
			.map(autonomy_proposal_record_from_row_parts)
			.transpose()
	}

	fn list_autonomy_proposals_for_objective(
		&self,
		project_id: &str,
		objective_id: &str,
		objective_version: u64,
	) -> Result<Vec<AutonomyProposalRuntimeRecord>> {
		let version = i64::try_from(objective_version).map_err(|_| {
			eyre::eyre!("Autonomy proposal objective_version exceeds SQLite integer range.")
		})?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 AND objective_id = ?2 AND objective_version = ?3 \
			 ORDER BY updated_at_unix ASC, proposal_id ASC",
		)?;
		let rows = statement.query_map(
			params![project_id, objective_id, version],
			autonomy_proposal_runtime_row_parts,
		)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_proposal_record_from_row_parts(row?)?);
		}

		Ok(records)
	}

	fn recent_autonomy_proposals_for_project(
		&self,
		project_id: &str,
		limit: usize,
	) -> Result<Vec<AutonomyProposalRuntimeRecord>> {
		let limit = i64::try_from(limit).map_err(|_| {
			eyre::eyre!("Autonomy proposal readback limit exceeds SQLite integer range.")
		})?;
		let mut statement = self.connection.prepare(
			"SELECT project_id, proposal_id, objective_id, objective_version, state, fingerprint, \
			 source_family, intended_surface, payload_json, created_at, created_at_unix, \
			 updated_at, updated_at_unix \
			 FROM autonomy_proposals \
			 WHERE project_id = ?1 \
			 ORDER BY updated_at_unix DESC, proposal_id ASC \
			 LIMIT ?2",
		)?;
		let rows =
			statement.query_map(params![project_id, limit], autonomy_proposal_runtime_row_parts)?;
		let mut records = Vec::new();

		for row in rows {
			records.push(autonomy_proposal_record_from_row_parts(row?)?);
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

		rows.next()?
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

	fn list_program_intake_plans(&self, project_id: &str) -> Result<Vec<ProgramIntakePlanRecord>> {
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
			 queue_intent, has_active_label, has_opt_out_label, has_needs_attention_label, \
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
			 queue_intent, has_active_label, has_opt_out_label, has_needs_attention_label, \
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

	fn load_review_lifecycle_records(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, branch_name, run_id, attempt_number, pr_url, \
			 target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase, \
			 request_comment_database_id, request_created_at_unix_epoch, \
			 request_description_thumbs_up_count, request_retry_count, external_round_count, \
			 auto_merge_enabled_at_unix_epoch, landing_state, closeout_state, \
			 repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix \
			 FROM review_lifecycle_records",
		)?;
		let rows = statement.query_map([], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let branch_name: String = row.get(2)?;
			let run_id: String = row.get(3)?;
			let attempt_number: i64 = row.get(4)?;
			let request_description_thumbs_up_count =
				row.get::<_, Option<i64>>(13)?.and_then(|count| usize::try_from(count).ok());

			Ok((
				ReviewLifecycleKey::new(&project_id, &issue_id, &branch_name),
				ReviewLifecycleRuntimeRecord {
					project_id,
					issue_id,
					branch_name,
					run_id,
					attempt_number,
					pr_url: row.get(5)?,
					target_base_ref_name: row.get(6)?,
					pr_head_ref_name: row.get(7)?,
					pr_head_oid: row.get(8)?,
					head_sha: row.get(9)?,
					phase: row.get(10)?,
					request_comment_database_id: row.get(11)?,
					request_created_at_unix_epoch: row.get(12)?,
					request_description_thumbs_up_count,
					request_retry_count: row.get(14)?,
					external_round_count: row.get(15)?,
					auto_merge_enabled_at_unix_epoch: row.get(16)?,
					landing_state: row.get(17)?,
					closeout_state: row.get(18)?,
					repair_attempt_count: row.get(19)?,
					evidence_json: row.get(20)?,
					next_action: row.get(21)?,
					updated_at: row.get(22)?,
					updated_at_unix: row.get(23)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_lifecycle_records.insert(key, record);
		}

		Ok(())
	}

	fn load_review_lifecycle_records_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, branch_name, run_id, attempt_number, pr_url, \
			 target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha, phase, \
			 request_comment_database_id, request_created_at_unix_epoch, \
			 request_description_thumbs_up_count, request_retry_count, external_round_count, \
			 auto_merge_enabled_at_unix_epoch, landing_state, closeout_state, \
			 repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix \
			 FROM review_lifecycle_records WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(params![project_id], |row| {
			let project_id: String = row.get(0)?;
			let issue_id: String = row.get(1)?;
			let branch_name: String = row.get(2)?;
			let run_id: String = row.get(3)?;
			let attempt_number: i64 = row.get(4)?;
			let request_description_thumbs_up_count =
				row.get::<_, Option<i64>>(13)?.and_then(|count| usize::try_from(count).ok());

			Ok((
				ReviewLifecycleKey::new(&project_id, &issue_id, &branch_name),
				ReviewLifecycleRuntimeRecord {
					project_id,
					issue_id,
					branch_name,
					run_id,
					attempt_number,
					pr_url: row.get(5)?,
					target_base_ref_name: row.get(6)?,
					pr_head_ref_name: row.get(7)?,
					pr_head_oid: row.get(8)?,
					head_sha: row.get(9)?,
					phase: row.get(10)?,
					request_comment_database_id: row.get(11)?,
					request_created_at_unix_epoch: row.get(12)?,
					request_description_thumbs_up_count,
					request_retry_count: row.get(14)?,
					external_round_count: row.get(15)?,
					auto_merge_enabled_at_unix_epoch: row.get(16)?,
					landing_state: row.get(17)?,
					closeout_state: row.get(18)?,
					repair_attempt_count: row.get(19)?,
					evidence_json: row.get(20)?,
					next_action: row.get(21)?,
					updated_at: row.get(22)?,
					updated_at_unix: row.get(23)?,
				},
			))
		})?;

		for row in rows {
			let (key, record) = row?;

			state.review_lifecycle_records.insert(key, record);
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

	fn load_evidence_artifacts(&self, state: &mut StateData) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha, \
			 key_json, payload_json, source_run_id, source_attempt_number, updated_at, \
			 updated_at_unix FROM evidence_artifacts",
		)?;
		let rows = statement.query_map([], Self::evidence_artifact_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.evidence_artifacts.insert(key, record);
		}

		Ok(())
	}

	fn load_evidence_artifacts_for_project(
		&self,
		state: &mut StateData,
		project_id: &str,
	) -> Result<()> {
		let mut statement = self.connection.prepare(
			"SELECT project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha, \
			 key_json, payload_json, source_run_id, source_attempt_number, updated_at, \
			 updated_at_unix FROM evidence_artifacts WHERE project_id = ?1",
		)?;
		let rows = statement.query_map(params![project_id], Self::evidence_artifact_from_row)?;

		for row in rows {
			let (key, record) = row?;

			state.evidence_artifacts.insert(key, record);
		}

		Ok(())
	}

	fn evidence_artifact_from_row(
		row: &Row<'_>,
	) -> rusqlite::Result<(EvidenceArtifactKey, EvidenceArtifactRuntimeRecord)> {
		let project_id: String = row.get(0)?;
		let issue_id: String = row.get(1)?;
		let artifact_kind: String = row.get(2)?;
		let key_hash: String = row.get(3)?;

		Ok((
			EvidenceArtifactKey::new(&project_id, &issue_id, &artifact_kind, &key_hash),
			EvidenceArtifactRuntimeRecord {
				project_id,
				issue_id,
				artifact_kind,
				key_hash,
				phase: row.get(4)?,
				status: row.get(5)?,
				head_sha: row.get(6)?,
				key_json: row.get(7)?,
				payload_json: row.get(8)?,
				source_run_id: row.get(9)?,
				source_attempt_number: row.get(10)?,
				updated_at: row.get(11)?,
				updated_at_unix: row.get(12)?,
			},
		))
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

fn persist_protocol_events(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for (run_id, events) in &state.events {
		for event in events {
			transaction.execute(
				"INSERT OR REPLACE INTO protocol_events (
						run_id, sequence_number, event_type, payload_sha256, created_at,
						created_at_unix
					) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
				params![
					run_id,
					event.sequence_number,
					&event.event_type,
					&event.payload_sha256,
					&event.created_at,
					event.created_at_unix,
				],
			)?;
		}
	}

	Ok(())
}

fn persist_run_activity_summaries(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for summary in state.run_activity_summaries.values() {
		let child_agent_activity_json = summary
			.child_agent_activity
			.as_ref()
			.cloned()
			.map(ChildAgentActivitySummary::sealed_durable)
			.map(|summary| serde_json::to_string(&summary))
			.transpose()?;
		let protocol_activity_json =
			summary.protocol_activity.as_ref().map(serde_json::to_string).transpose()?;

		transaction.execute(
			"INSERT OR REPLACE INTO run_activity_summaries (
					run_id, attempt_number, child_agent_activity_json, protocol_activity_json,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
			params![
				&summary.run_id,
				summary.attempt_number,
				child_agent_activity_json.as_deref(),
				protocol_activity_json.as_deref(),
				&summary.updated_at,
				summary.updated_at_unix,
			],
		)?;
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

fn persist_linear_execution_events(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
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

fn persist_decision_contracts(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
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

fn persist_autonomy_objectives(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for record in state.autonomy_objectives.values() {
		let payload_json = serde_json::to_string(&record.objective)?;
		let version = i64::try_from(record.objective.version())
			.map_err(|_| eyre::eyre!("Autonomy objective version exceeds SQLite integer range."))?;

		transaction.execute(
			"INSERT OR REPLACE INTO autonomy_objectives (
					project_id, objective_id, version, state, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
			params![
				&record.project_id,
				record.objective.id(),
				version,
				record.state.as_str(),
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

fn persist_autonomy_signals(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for record in state.autonomy_signals.values() {
		let payload_json = serde_json::to_string(&record.signal)?;
		let version = i64::try_from(record.signal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy signal objective_version exceeds SQLite integer range.")
		})?;

		transaction.execute(
			"INSERT OR REPLACE INTO autonomy_signals (
					project_id, signal_id, objective_id, objective_version, kind, fingerprint,
					freshness, evidence_class, confidence, privacy, payload_json, created_at,
					created_at_unix, updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
			params![
				&record.project_id,
				record.signal.id(),
				record.signal.objective_id(),
				version,
				record.signal.kind().as_str(),
				record.signal.fingerprint(),
				record.signal.freshness().as_str(),
				record.signal.evidence_class().as_str(),
				record.signal.confidence().as_str(),
				record.signal.privacy().as_str(),
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

fn persist_autonomy_proposals(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for record in state.autonomy_proposals.values() {
		let payload_json = serde_json::to_string(&record.proposal)?;
		let version = i64::try_from(record.proposal.objective_version()).map_err(|_| {
			eyre::eyre!("Autonomy proposal objective_version exceeds SQLite integer range.")
		})?;

		transaction.execute(
			"INSERT OR REPLACE INTO autonomy_proposals (
					project_id, proposal_id, objective_id, objective_version, state, fingerprint,
					source_family, intended_surface, payload_json, created_at, created_at_unix,
					updated_at, updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
			params![
				&record.project_id,
				record.proposal.id(),
				record.proposal.objective_id(),
				version,
				record.state.as_str(),
				record.proposal.fingerprint(),
				record.proposal.source_family(),
				record.proposal.intended_surface(),
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

fn persist_execution_programs(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
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
					queue_intent, has_active_label, has_opt_out_label, has_needs_attention_label,
					has_generic_dispatch_briefing, created_at, created_at_unix, updated_at,
					updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
			params![
				&record.project_id,
				&record.program_id,
				&record.node_id,
				&record.issue_id,
				&record.issue_identifier,
				&record.issue_state,
				&record.queue_intent,
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
					queue_intent, has_active_label, has_opt_out_label, has_needs_attention_label,
					has_generic_dispatch_briefing, created_at, created_at_unix, updated_at,
					updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
			params![
				&mapping.project_id,
				&mapping.program_id,
				&mapping.node_id,
				&mapping.issue_id,
				&mapping.issue_identifier,
				&mapping.issue_state,
				&mapping.queue_intent,
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

	Ok(())
}

fn persist_review_lifecycle_records(
	transaction: &Transaction<'_>,
	state: &StateData,
) -> Result<()> {
	for record in state.review_lifecycle_records.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO review_lifecycle_records (
					project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
					target_base_ref_name, pr_head_ref_name, pr_head_oid, head_sha,
					phase, request_comment_database_id, request_created_at_unix_epoch,
					request_description_thumbs_up_count, request_retry_count, external_round_count,
					auto_merge_enabled_at_unix_epoch, landing_state, closeout_state,
					repair_attempt_count, evidence_json, next_action, updated_at, updated_at_unix
				) VALUES (
					?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
					?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24
				)",
			params![
				record.project_id,
				record.issue_id,
				record.branch_name,
				record.run_id,
				record.attempt_number,
				record.pr_url,
				record.target_base_ref_name,
				record.pr_head_ref_name,
				record.pr_head_oid,
				record.head_sha,
				record.phase,
				record.request_comment_database_id,
				record.request_created_at_unix_epoch,
				record
					.request_description_thumbs_up_count
					.and_then(|count| i64::try_from(count).ok()),
				record.request_retry_count,
				record.external_round_count,
				record.auto_merge_enabled_at_unix_epoch,
				record.landing_state,
				record.closeout_state,
				record.repair_attempt_count,
				record.evidence_json,
				record.next_action,
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

fn persist_evidence_artifacts(transaction: &Transaction<'_>, state: &StateData) -> Result<()> {
	for record in state.evidence_artifacts.values() {
		transaction.execute(
			"INSERT OR REPLACE INTO evidence_artifacts (
					project_id, issue_id, artifact_kind, key_hash, phase, status, head_sha,
					key_json, payload_json, source_run_id, source_attempt_number, updated_at,
					updated_at_unix
				) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
			params![
				record.project_id,
				record.issue_id,
				record.artifact_kind,
				record.key_hash,
				record.phase,
				record.status,
				record.head_sha,
				record.key_json,
				record.payload_json,
				record.source_run_id,
				record.source_attempt_number,
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

	env::temp_dir().join("decodex-shared-lock-coordinators").join(format!("{hash:016x}.lock"))
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

fn remove_derived_program_intake_state(state: &mut StateData, project_id: &str, program_id: &str) {
	state
		.program_intake_plans
		.retain(|key, _record| key.project_id != project_id || key.program_id != program_id);
	state
		.program_issue_mappings
		.retain(|key, _record| key.project_id != project_id || key.program_id != program_id);
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
			ProgramIssueMappingKey::new(&mapping.project_id, &mapping.program_id, &mapping.node_id),
			mapping,
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

fn compare_project_run_status(left: &ProjectRunStatus, right: &ProjectRunStatus) -> cmp::Ordering {
	right
		.run_lease
		.cmp(&left.run_lease)
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
