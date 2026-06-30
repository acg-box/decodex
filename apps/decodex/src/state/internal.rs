#[cfg(unix)] use std::os::{fd::AsRawFd, unix::ffi::OsStrExt};
use std::{
	cmp,
	collections::HashMap,
	env, fs,
	fs::{File, OpenOptions, TryLockError},
	io::{ErrorKind, Read, Seek, SeekFrom, Write},
	path::{Path, PathBuf},
};

use libc::{F_GETFD, F_SETFD, FD_CLOEXEC};

use crate::prelude::{Result, eyre};

use super::{
	ChildAgentActivitySummary, CodexAccountActivitySummary, ConnectorBackoff,
	DISPATCH_SLOT_LOCK_FILE_PREFIX, ISSUE_CLAIM_LOCK_FILE_PREFIX, IssueLease,
	ProgramIntakePlanRecord, ProgramIssueMappingRecord, ProjectRegistration, ProjectRunStatus,
	ProtocolActivitySummary, protocol_event_summary_from_events,
	runtime_records::{
		AutonomyObjectiveKey, AutonomyObjectiveRuntimeRecord, AutonomyProposalKey,
		AutonomyProposalRuntimeRecord, AutonomySignalKey, AutonomySignalRuntimeRecord,
		DecisionContractKey, DecisionContractRuntimeRecord, EvidenceArtifactKey,
		EvidenceArtifactRuntimeRecord, ExecutionProgramKey, ExecutionProgramRuntimeRecord,
		GuardRetention, LinearExecutionEventRuntimeRecord, LoopGuardrailKey,
		LoopGuardrailRuntimeRecord, PrivateExecutionEventRuntimeRecord, ProgramIntakePlanKey,
		ProgramIssueMappingKey, ProtocolEventRecord, ProtocolEventSummaryRecord,
		ReviewLifecycleKey, ReviewLifecycleRuntimeRecord, ReviewPolicyKey,
		ReviewPolicyRuntimeRecord, RunActivitySummaryRecord, RunAttemptRecord,
		RunControlChannelRecord, WorktreeMappingRecord,
	},
};

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
pub(super) struct DispatchSlotConfig {
	pub(super) root: PathBuf,
}

pub(super) struct IssueClaimGuard {
	pub(super) lock_path: PathBuf,
	pub(super) lock_file: File,
	pub(super) retention: GuardRetention,
}
impl IssueClaimGuard {
	pub(super) fn lock_root(&self) -> Result<&Path> {
		lock_root_from_lock_path(&self.lock_path)
	}

	pub(super) fn unlock(self) -> Result<()> {
		let Self { lock_path, lock_file, retention: _ } = self;

		lock_file.unlock()?;

		drop(lock_file);
		remove_lock_file_if_exists(&lock_path)?;

		Ok(())
	}

	pub(super) fn release_for_clear(self) -> Result<()> {
		match self.retention {
			GuardRetention::ParentAfterHandoff => Ok(()),
			GuardRetention::Local | GuardRetention::AdoptingChild => self.unlock(),
		}
	}
}

pub(super) struct DispatchSlotGuard {
	pub(super) project_id: String,
	pub(super) slot_index: usize,
	pub(super) lock_path: PathBuf,
	pub(super) lock_file: File,
	pub(super) retention: GuardRetention,
}
impl DispatchSlotGuard {
	pub(super) fn lock_root(&self) -> Result<&Path> {
		lock_root_from_lock_path(&self.lock_path)
	}

	pub(super) fn release_for_clear(self) -> Result<()> {
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
pub(super) struct StateData {
	pub(super) projects: HashMap<String, ProjectRegistration>,
	pub(super) leases: HashMap<String, IssueLease>,
	pub(super) run_attempts: HashMap<String, RunAttemptRecord>,
	pub(super) control_channels: HashMap<String, RunControlChannelRecord>,
	pub(super) events: HashMap<String, Vec<ProtocolEventRecord>>,
	pub(super) event_summaries: HashMap<String, ProtocolEventSummaryRecord>,
	pub(super) run_activity_summaries: HashMap<String, RunActivitySummaryRecord>,
	pub(super) worktrees: HashMap<String, WorktreeMappingRecord>,
	pub(super) linear_execution_events: HashMap<String, LinearExecutionEventRuntimeRecord>,
	pub(super) private_execution_events: Vec<PrivateExecutionEventRuntimeRecord>,
	pub(super) decision_contracts: HashMap<DecisionContractKey, DecisionContractRuntimeRecord>,
	pub(super) autonomy_objectives: HashMap<AutonomyObjectiveKey, AutonomyObjectiveRuntimeRecord>,
	pub(super) autonomy_signals: HashMap<AutonomySignalKey, AutonomySignalRuntimeRecord>,
	pub(super) autonomy_proposals: HashMap<AutonomyProposalKey, AutonomyProposalRuntimeRecord>,
	pub(super) execution_programs: HashMap<ExecutionProgramKey, ExecutionProgramRuntimeRecord>,
	pub(super) program_intake_plans: HashMap<ProgramIntakePlanKey, ProgramIntakePlanRecord>,
	pub(super) program_issue_mappings: HashMap<ProgramIssueMappingKey, ProgramIssueMappingRecord>,
	pub(super) review_lifecycle_records: HashMap<ReviewLifecycleKey, ReviewLifecycleRuntimeRecord>,
	pub(super) review_policy_checkpoints: HashMap<ReviewPolicyKey, ReviewPolicyRuntimeRecord>,
	pub(super) evidence_artifacts: HashMap<EvidenceArtifactKey, EvidenceArtifactRuntimeRecord>,
	pub(super) loop_guardrail_checkpoints: HashMap<LoopGuardrailKey, LoopGuardrailRuntimeRecord>,
	pub(super) connector_backoffs: HashMap<(String, String), ConnectorBackoff>,
	pub(super) dispatch_slot_configs: HashMap<String, DispatchSlotConfig>,
	pub(super) issue_claim_guards: HashMap<String, IssueClaimGuard>,
	pub(super) dispatch_slot_guards: HashMap<String, DispatchSlotGuard>,
}
impl StateData {
	pub(super) fn replace_durable_state(&mut self, loaded: Self) {
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

	pub(super) fn replace_project_run_metadata_state(&mut self, loaded: Self) {
		self.leases = loaded.leases;
		self.run_attempts = loaded.run_attempts;
		self.control_channels = loaded.control_channels;
		self.run_activity_summaries = loaded.run_activity_summaries;
		self.worktrees = loaded.worktrees;
	}

	pub(super) fn replace_project_loop_evidence_state(&mut self, project_id: &str, loaded: Self) {
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

	pub(super) fn replace_project_registry_state(&mut self, loaded: Self) {
		self.projects = loaded.projects;
	}

	pub(super) fn project_run_status(
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

	pub(super) fn protocol_event_summary(&self, run_id: &str) -> ProtocolEventSummaryRecord {
		self.event_summaries
			.get(run_id)
			.cloned()
			.or_else(|| {
				self.events.get(run_id).map(|events| protocol_event_summary_from_events(events))
			})
			.unwrap_or_default()
	}

	pub(super) fn project_id_for_run(&self, issue_id: &str, run_id: &str) -> Option<String> {
		self.leases
			.get(issue_id)
			.filter(|lease| lease.run_id == run_id)
			.map(|lease| lease.project_id.clone())
			.or_else(|| self.worktrees.get(issue_id).map(|mapping| mapping.project_id.clone()))
	}

	pub(super) fn remember_run_project(
		&mut self,
		project_id: &str,
		issue_id: &str,
		run_id: Option<&str>,
	) {
		for attempt in self
			.run_attempts
			.values_mut()
			.filter(|attempt| attempt.issue_id == issue_id)
			.filter(|attempt| run_id.is_none_or(|run_id| attempt.run_id == run_id))
		{
			attempt.project_id = Some(project_id.to_owned());
		}
	}

	pub(super) fn next_private_execution_event_id(&self) -> Result<i64> {
		self.private_execution_events
			.iter()
			.map(|record| record.record_id)
			.max()
			.unwrap_or(0)
			.checked_add(1)
			.ok_or_else(|| eyre::eyre!("Private execution event row id overflowed i64."))
	}
}

pub(super) fn dispatch_slot_lock_path(root: &Path, slot_index: usize) -> PathBuf {
	root.join(format!("{DISPATCH_SLOT_LOCK_FILE_PREFIX}.{slot_index}.lock"))
}

pub(super) fn issue_claim_lock_path(root: &Path, issue_id: &str) -> PathBuf {
	root.join(format!("{ISSUE_CLAIM_LOCK_FILE_PREFIX}.{issue_id}.lock"))
}

pub(super) fn issue_claim_id_from_path(path: &Path) -> Option<String> {
	let file_name = path.file_name()?.to_str()?;

	file_name
		.strip_prefix(&format!("{ISSUE_CLAIM_LOCK_FILE_PREFIX}."))
		.and_then(|suffix| suffix.strip_suffix(".lock"))
		.map(str::to_owned)
}

pub(super) fn shared_lock_coordinator_path(root: &Path) -> PathBuf {
	let mut hash = 0xcbf2_9ce4_8422_2325_u64;

	for byte in root.as_os_str().as_bytes() {
		hash ^= u64::from(*byte);
		hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
	}

	env::temp_dir().join("decodex-shared-lock-coordinators").join(format!("{hash:016x}.lock"))
}

pub(super) fn acquire_shared_lock_coordinator(root: &Path) -> Result<File> {
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

pub(super) fn lock_root_from_lock_path(lock_path: &Path) -> Result<&Path> {
	lock_path
		.parent()
		.ok_or_else(|| eyre::eyre!("shared lock path `{}` has no parent root", lock_path.display()))
}

pub(super) fn remove_lock_file_if_exists(path: &Path) -> Result<()> {
	match fs::remove_file(path) {
		Ok(()) => Ok(()),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error.into()),
	}
}

pub(super) fn shared_lock_file_is_cleanup_candidate(path: &Path) -> bool {
	let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
		return false;
	};

	file_name.starts_with(&format!("{ISSUE_CLAIM_LOCK_FILE_PREFIX}."))
		|| file_name.starts_with(&format!("{DISPATCH_SLOT_LOCK_FILE_PREFIX}."))
}

pub(super) fn prune_unlocked_shared_lock_files(root: &Path) -> Result<()> {
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

pub(super) fn write_issue_claim_record(
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

pub(super) fn read_issue_claim_record(path: &Path) -> Result<Option<IssueLease>> {
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

pub(super) fn remove_derived_program_intake_state(
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
}

pub(super) fn apply_derived_program_intake_state(
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

pub(super) fn derived_program_intake_plan_records(
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

pub(super) fn derived_program_issue_mapping_records(
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

pub(super) fn compare_project_run_status(
	left: &ProjectRunStatus,
	right: &ProjectRunStatus,
) -> cmp::Ordering {
	right
		.run_lease
		.cmp(&left.run_lease)
		.then_with(|| right.updated_at.cmp(&left.updated_at))
		.then_with(|| right.attempt_number.cmp(&left.attempt_number))
		.then_with(|| right.run_id.cmp(&left.run_id))
}

#[cfg(unix)]
pub(super) fn clear_close_on_exec(file: &File) -> Result<()> {
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
pub(super) fn set_close_on_exec(file: &File) -> Result<()> {
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
