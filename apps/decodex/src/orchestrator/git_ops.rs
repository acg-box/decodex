mod repo_gate_failure {
	use crate::orchestrator::RepoGateFailureDiagnostic;
	use crate::orchestrator::RepoGateTrackedRewriteDecision;
	use std::fmt::Display;
	use std::fmt::Formatter;
	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	pub(super) enum RepoGateFailureDisposition {
		ContinueRepair,
		RetryAfterBackoff,
		NeedsHumanAttention,
	}
	impl RepoGateFailureDisposition {
		pub(super) const fn as_str(self) -> &'static str {
			match self {
				Self::ContinueRepair => "continue_repair",
				Self::RetryAfterBackoff => "retry_after_backoff",
				Self::NeedsHumanAttention => "needs_human_attention",
			}
		}
	}

	#[derive(Clone, Copy, Debug, Eq, PartialEq)]
	pub(super) enum RepoGateFailureKind {
		CanonicalizeCommandFailed,
		VerifyCommandFailed,
		TrackedRewritesLeft,
		ScopeEnvelopeViolation,
		GitLockContention,
		CommandSpawnFailed,
		CleanlinessCheckFailed,
	}
	impl RepoGateFailureKind {
		fn error_class(self) -> &'static str {
			match self {
				Self::CanonicalizeCommandFailed => "repo_gate_canonicalize_failed",
				Self::VerifyCommandFailed => "repo_gate_verify_failed",
				Self::TrackedRewritesLeft => "repo_gate_tracked_rewrites_left",
				Self::ScopeEnvelopeViolation => "repo_gate_scope_envelope_violation",
				Self::GitLockContention => "repo_gate_git_lock_contention",
				Self::CommandSpawnFailed => "repo_gate_command_spawn_failed",
				Self::CleanlinessCheckFailed => "repo_gate_cleanliness_check_failed",
			}
		}

		fn disposition(self) -> RepoGateFailureDisposition {
			match self {
				Self::CanonicalizeCommandFailed | Self::VerifyCommandFailed => {
					RepoGateFailureDisposition::ContinueRepair
				},
				Self::TrackedRewritesLeft | Self::ScopeEnvelopeViolation => {
					RepoGateFailureDisposition::NeedsHumanAttention
				},
				Self::GitLockContention => RepoGateFailureDisposition::RetryAfterBackoff,
				Self::CommandSpawnFailed | Self::CleanlinessCheckFailed => {
					RepoGateFailureDisposition::NeedsHumanAttention
				},
			}
		}

		fn retry_next_action(self) -> &'static str {
			match self {
				Self::CanonicalizeCommandFailed => {
					"additional agent repair is required before repo canonicalization can pass; decodex will retry automatically"
				},
				Self::VerifyCommandFailed => {
					"additional agent repair is required before repo verification can pass; decodex will retry automatically"
				},
				Self::TrackedRewritesLeft => {
					"automatic retry is stopped because the repo gate left tracked rewrites after completing; inspect the retained worktree manually"
				},
				Self::ScopeEnvelopeViolation => {
					"automatic retry is stopped because the repo gate wrote files outside the lane scope envelope"
				},
				Self::GitLockContention => {
					"another Git process appears to hold `.git/index.lock`; decodex will wait briefly, refresh lane state, and retry automatically"
				},
				Self::CommandSpawnFailed => {
					"manual repair is required to restore repo-gate command execution"
				},
				Self::CleanlinessCheckFailed => {
					"manual repair is required to restore repo-gate tracked-file inspection"
				},
			}
		}

		fn terminal_next_action(self, recovery_gate: &str) -> String {
			match self {
				Self::CanonicalizeCommandFailed => format!(
					"inspect the worktree, repair the repo canonicalization failure manually, {recovery_gate}"
				),
				Self::VerifyCommandFailed => format!(
					"inspect the worktree, repair the repo verification failure manually, {recovery_gate}"
				),
				Self::TrackedRewritesLeft => format!(
					"inspect the retained worktree, decide whether the tracked rewrites are in scope, then finish validation and PR handoff or reset the patch manually, {recovery_gate}"
				),
				Self::ScopeEnvelopeViolation => format!(
					"inspect the retained worktree and explicitly decide whether to expand lane scope or isolate repo-wide baseline cleanup before retrying, {recovery_gate}"
				),
				Self::GitLockContention => format!(
					"inspect the worktree for an active or stale `.git/index.lock` holder, clear the Git lock contention manually, {recovery_gate}"
				),
				Self::CommandSpawnFailed => format!(
					"inspect the repo-gate runtime in the worktree, restore command execution manually, {recovery_gate}"
				),
				Self::CleanlinessCheckFailed => format!(
					"inspect the repo-gate runtime in the worktree, restore tracked-file cleanliness inspection manually, {recovery_gate}"
				),
			}
		}
	}

	#[derive(Debug)]
	pub(super) struct RepoGateFailure {
		kind: RepoGateFailureKind,
		message: String,
		diagnostic: Option<RepoGateFailureDiagnostic>,
		tracked_rewrite_decision: Option<RepoGateTrackedRewriteDecision>,
	}
	impl RepoGateFailure {
		pub(super) fn new(kind: RepoGateFailureKind, message: String) -> Self {
			Self { kind, message, diagnostic: None, tracked_rewrite_decision: None }
		}

		pub(super) fn with_diagnostic(mut self, diagnostic: RepoGateFailureDiagnostic) -> Self {
			self.diagnostic = Some(diagnostic);

			self
		}

		pub(super) fn with_tracked_rewrite_decision(
			mut self,
			decision: RepoGateTrackedRewriteDecision,
		) -> Self {
			self.tracked_rewrite_decision = Some(decision);

			self
		}

		pub(super) fn error_class(&self) -> &'static str {
			self.kind.error_class()
		}

		pub(super) fn disposition(&self) -> RepoGateFailureDisposition {
			self.kind.disposition()
		}

		pub(super) fn retry_next_action(&self) -> &'static str {
			self.kind.retry_next_action()
		}

		pub(super) fn retry_schedule_kind(&self) -> Option<&'static str> {
			self.kind.retry_schedule_kind()
		}

		pub(super) fn terminal_next_action(&self, recovery_gate: &str) -> String {
			self.kind.terminal_next_action(recovery_gate)
		}

		pub(super) fn diagnostic(&self) -> Option<&RepoGateFailureDiagnostic> {
			self.diagnostic.as_ref()
		}

		pub(super) fn repair_target_detail(&self) -> String {
			self.diagnostic.as_ref().map_or_else(
				|| format!("Repo gate failed with `{}`.", self.error_class()),
				RepoGateFailureDiagnostic::repair_target_detail,
			)
		}

		pub(super) fn tracked_rewrite_decision(&self) -> Option<&RepoGateTrackedRewriteDecision> {
			self.tracked_rewrite_decision.as_ref()
		}
	}
	impl std::error::Error for RepoGateFailure {}

	impl Display for RepoGateFailure {
		fn fmt(&self, f: &mut Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
			write!(f, "{}", self.message)
		}
	}
}

use std::process::Output;

use crate::workflow::ResolvedRepoGate;
use repo_gate_failure::{RepoGateFailure, RepoGateFailureDisposition, RepoGateFailureKind};

const REPO_GATE_DIAGNOSTIC_EXCERPT_LIMIT: usize = 4_000;
const REPO_GATE_DIAGNOSTIC_LINE_LIMIT: usize = 16;
const REPO_GATE_DIAGNOSTIC_LINE_WIDTH: usize = 320;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RepoGateFailureDiagnostic {
	stage: &'static str,
	failed_command: String,
	exit_status: Option<i32>,
	summary: String,
	problem_lines: Vec<String>,
	output_excerpt: String,
	output_truncated: bool,
}
impl RepoGateFailureDiagnostic {
	fn from_output(
		stage: &'static str,
		failed_command: &str,
		output: &Output,
		output_text: &str,
	) -> Self {
		let (output_excerpt, output_truncated) =
			repo_gate_bounded_output_excerpt(output_text, REPO_GATE_DIAGNOSTIC_EXCERPT_LIMIT);
		let problem_lines = repo_gate_problem_lines(output_text);
		let summary = repo_gate_diagnostic_summary(stage, failed_command, output, &problem_lines);

		Self {
			stage,
			failed_command: failed_command.to_owned(),
			exit_status: output.status.code(),
			summary,
			problem_lines,
			output_excerpt,
			output_truncated,
		}
	}

	fn from_spawn_error(stage: &'static str, failed_command: &str, error: &dyn Display) -> Self {
		let output_text = error.to_string();
		let (output_excerpt, output_truncated) =
			repo_gate_bounded_output_excerpt(&output_text, REPO_GATE_DIAGNOSTIC_EXCERPT_LIMIT);
		let problem_lines = repo_gate_problem_lines(&output_text);
		let summary = format!("Repo gate {stage} command `{failed_command}` failed to spawn.");

		Self {
			stage,
			failed_command: failed_command.to_owned(),
			exit_status: None,
			summary,
			problem_lines,
			output_excerpt,
			output_truncated,
		}
	}

	fn repair_target_detail(&self) -> String {
		let key_lines = if self.problem_lines.is_empty() {
			String::from("none")
		} else {
			self.problem_lines.join(" | ")
		};

		format!(
			"Failed repo-gate command: `{}` during `{}`. Summary: {} Key diagnostic lines: {}.",
			self.failed_command, self.stage, self.summary, key_lines
		)
	}

	fn to_json(&self) -> Value {
		json!({
			"schema": "decodex.repo_gate_failure_diagnostic/1",
			"stage": self.stage,
			"failed_command": &self.failed_command,
			"exit_status": self.exit_status,
			"summary": &self.summary,
			"problem_lines": &self.problem_lines,
			"output_excerpt": &self.output_excerpt,
			"output_truncated": self.output_truncated,
		})
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RepoGateTrackedRewriteDecision {
	files: Vec<String>,
	owned: bool,
	decision: &'static str,
	reason: &'static str,
	source_error_class: Option<&'static str>,
	source_diagnostic: Option<RepoGateFailureDiagnostic>,
}
impl RepoGateTrackedRewriteDecision {
	fn continue_to_commit_capable_phase(files: Vec<String>) -> Self {
		Self {
			files,
			owned: true,
			decision: "continue_to_commit_capable_phase",
			reason: "all rewritten files were already present in the pre-gate implementation diff and the repo gate passed",
			source_error_class: None,
			source_diagnostic: None,
		}
	}

	fn require_attention(files: Vec<String>, owned: bool, reason: &'static str) -> Self {
		Self {
			files,
			owned,
			decision: "repo_gate_tracked_rewrites_left",
			reason,
			source_error_class: None,
			source_diagnostic: None,
		}
	}

	fn scope_envelope_violation(
		files: Vec<String>,
		source_error_class: Option<&'static str>,
		source_diagnostic: Option<RepoGateFailureDiagnostic>,
	) -> Self {
		Self {
			files,
			owned: false,
			decision: "scope_envelope_violation",
			reason: "one or more repo-gate rewrites were not present in the pre-gate lane diff",
			source_error_class,
			source_diagnostic,
		}
	}

	fn files_display(&self) -> String {
		if self.files.is_empty() {
			String::from("(no tracked files reported)")
		} else {
			self.files.join(", ")
		}
	}

	fn to_json(&self) -> Value {
		json!({
			"files": &self.files,
			"owned": self.owned,
			"decision": self.decision,
			"reason": self.reason,
			"sourceErrorClass": self.source_error_class,
			"sourceRepoGateFailure": self.source_diagnostic.as_ref().map(RepoGateFailureDiagnostic::to_json),
		})
	}

	fn is_scope_envelope_violation(&self) -> bool {
		self.decision == "scope_envelope_violation"
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct RepoGateCommandOutcome {
	tracked_rewrite_decision: Option<RepoGateTrackedRewriteDecision>,
}
impl RepoGateCommandOutcome {
	fn clean() -> Self {
		Self::default()
	}

	fn with_tracked_rewrite_decision(decision: RepoGateTrackedRewriteDecision) -> Self {
		Self { tracked_rewrite_decision: Some(decision) }
	}

	fn tracked_rewrite_decision(&self) -> Option<&RepoGateTrackedRewriteDecision> {
		self.tracked_rewrite_decision.as_ref()
	}
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct RepoGateTrackedDiffSnapshot {
	full_diff: String,
	path_diffs: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RepoGateScopeEnvelope {
	authorized_paths: BTreeSet<String>,
}
impl RepoGateScopeEnvelope {
	fn from_pre_gate_diff(snapshot: &RepoGateTrackedDiffSnapshot) -> Self {
		Self { authorized_paths: snapshot.path_diffs.keys().cloned().collect() }
	}

	fn violation_files(&self, rewritten_files: impl IntoIterator<Item = String>) -> Vec<String> {
		rewritten_files.into_iter().filter(|path| !self.authorized_paths.contains(path)).collect()
	}
}

impl RepoGateFailureKind {
	fn retry_schedule_kind(self) -> Option<&'static str> {
		match self {
			Self::GitLockContention => Some("git_lock_contention"),
			_ => None,
		}
	}
}

pub(crate) fn delete_local_branch_if_present(repo_root: &Path, branch_name: &str) -> Result<()> {
	let local_ref = format!("refs/heads/{branch_name}");
	let branch_check = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["show-ref", "--verify", "--quiet", local_ref.as_str()])
		.output()?;

	if !branch_check.status.success() {
		if branch_check.status.code() == Some(1) {
			return Ok(());
		}

		let stderr = String::from_utf8_lossy(&branch_check.stderr);

		eyre::bail!(
			"Failed to inspect retained local branch `{branch_name}` in `{}`: {}",
			repo_root.display(),
			stderr.trim()
		);
	}

	let delete_output = Command::new("git")
		.arg("-C")
		.arg(repo_root)
		.args(["branch", "-D", branch_name])
		.output()?;

	if delete_output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&delete_output.stderr);

	if stderr.contains("not found") || stderr.contains("branch not found") {
		return Ok(());
	}

	eyre::bail!(
		"Failed to delete retained local branch `{branch_name}` from `{}`: {}",
		repo_root.display(),
		stderr.trim()
	);
}

pub(crate) fn detach_worktree_head_from_branch_if_checked_out(
	worktree_path: &Path,
	branch_name: &str,
) -> Result<()> {
	let head_ref = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["symbolic-ref", "--quiet", "--short", "HEAD"])
		.output()?;

	if !head_ref.status.success() {
		if head_ref.status.code() == Some(1) {
			return Ok(());
		}

		let stderr = String::from_utf8_lossy(&head_ref.stderr);

		eyre::bail!(
			"Failed to inspect retained worktree HEAD in `{}` before local branch cleanup: {}",
			worktree_path.display(),
			stderr.trim()
		);
	}

	let current_branch = String::from_utf8(head_ref.stdout)
		.map_err(|error| {
			eyre::eyre!(
				"Retained worktree HEAD in `{}` is not valid UTF-8: {error}",
				worktree_path.display()
			)
		})?
		.trim()
		.to_owned();

	if current_branch != branch_name {
		return Ok(());
	}

	let detach_output = Command::new("git")
		.arg("-C")
		.arg(worktree_path)
		.args(["checkout", "--quiet", "--detach"])
		.output()?;

	if detach_output.status.success() {
		return Ok(());
	}

	let stderr = String::from_utf8_lossy(&detach_output.stderr);

	eyre::bail!(
		"Failed to detach retained worktree `{}` from branch `{branch_name}` before local branch cleanup: {}",
		worktree_path.display(),
		stderr.trim()
	);
}

fn repo_gate_output_text(output: &Output) -> String {
	let stderr = String::from_utf8_lossy(&output.stderr);
	let stdout = String::from_utf8_lossy(&output.stdout);
	let stderr = stderr.trim();
	let stdout = stdout.trim();

	if !stderr.is_empty() {
		return stderr.to_owned();
	}
	if !stdout.is_empty() {
		return stdout.to_owned();
	}

	String::from("(command produced no output)")
}

fn repo_gate_bounded_output_excerpt(output_text: &str, limit: usize) -> (String, bool) {
	let mut excerpt = String::new();
	let mut truncated = false;

	for character in output_text.chars() {
		if excerpt.len() + character.len_utf8() > limit {
			truncated = true;

			break;
		}

		excerpt.push(character);
	}

	(excerpt, truncated)
}

fn repo_gate_diagnostic_summary(
	stage: &str,
	failed_command: &str,
	output: &Output,
	problem_lines: &[String],
) -> String {
	let exit_status =
		output.status.code().map_or_else(|| String::from("unknown"), |code| code.to_string());
	let first_problem = problem_lines
		.first()
		.map_or_else(|| String::from("no diagnostic output"), ToOwned::to_owned);

	format!(
		"repo gate {stage} command `{failed_command}` exited with status {exit_status}: {first_problem}"
	)
}

fn repo_gate_problem_lines(output_text: &str) -> Vec<String> {
	let lines = output_text.lines().collect::<Vec<_>>();
	let mut selected_indexes = BTreeSet::new();

	for (index, line) in lines.iter().enumerate() {
		if repo_gate_line_looks_diagnostic(line) {
			selected_indexes.insert(index);

			if index > 0 {
				selected_indexes.insert(index - 1);
			}

			for follow_index in index.saturating_add(1)..=(index + 4).min(lines.len()) {
				selected_indexes.insert(follow_index);
			}
		}
	}

	if selected_indexes.is_empty() {
		selected_indexes.extend(
			lines
				.iter()
				.enumerate()
				.filter(|(_, line)| !line.trim().is_empty())
				.take(4)
				.map(|(index, _)| index),
		);
	}

	selected_indexes
		.into_iter()
		.filter_map(|index| lines.get(index))
		.map(|line| repo_gate_truncate_diagnostic_line(line.trim()))
		.filter(|line| !line.is_empty())
		.take(REPO_GATE_DIAGNOSTIC_LINE_LIMIT)
		.collect()
}

fn repo_gate_line_looks_diagnostic(line: &str) -> bool {
	let line = line.trim();
	let lower = line.to_ascii_lowercase();

	line.starts_with("-->")
		|| lower.starts_with("error")
		|| lower.starts_with("fatal")
		|| lower.starts_with("failed")
		|| lower.starts_with("warning")
		|| lower.contains(" error:")
		|| lower.contains(" fatal:")
		|| lower.contains(" failed")
		|| lower.contains("panicked at")
		|| lower.contains("too many lines")
		|| lower.contains("clippy")
}

fn repo_gate_truncate_diagnostic_line(line: &str) -> String {
	let (mut line, truncated) =
		repo_gate_bounded_output_excerpt(line, REPO_GATE_DIAGNOSTIC_LINE_WIDTH);

	if truncated {
		line.push_str("...");
	}

	line
}

fn repo_gate_git_output_lines(output: &Output) -> BTreeSet<String> {
	let stdout = String::from_utf8_lossy(&output.stdout);

	stdout.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

fn repo_gate_is_git_lock_contention(output_text: &str) -> bool {
	let output_text = output_text.to_ascii_lowercase();

	output_text.contains("index.lock")
		&& (output_text.contains("file exists")
			|| output_text.contains("already exists")
			|| output_text.contains("another git process seems to be running"))
}

fn repo_gate_failure_kind_for_output(
	default_kind: RepoGateFailureKind,
	output_text: &str,
) -> RepoGateFailureKind {
	if repo_gate_is_git_lock_contention(output_text) {
		RepoGateFailureKind::GitLockContention
	} else {
		default_kind
	}
}

fn repo_gate_shell_from_env(
	shell: Option<std::ffi::OsString>,
) -> (std::ffi::OsString, &'static str) {
	if let Some(shell) = shell
		&& !shell.is_empty()
	{
		let shell_path = Path::new(&shell);
		let shell_name = shell_path.file_name().and_then(std::ffi::OsStr::to_str);

		if shell_name == Some("sh") {
			return (std::ffi::OsString::from("/bin/sh"), "-c");
		}
		if !shell_path.is_absolute() || shell_path.is_file() {
			return (shell, "-lc");
		}
	}

	(std::ffi::OsString::from("/bin/sh"), "-c")
}

fn repo_gate_shell() -> (std::ffi::OsString, &'static str) {
	repo_gate_shell_from_env(env::var_os("SHELL"))
}

fn run_repo_gate_shell_command(command: &str, cwd: &Path) -> Result<Output> {
	let (shell, shell_flag) = repo_gate_shell();

	Command::new(&shell).arg(shell_flag).arg(command).current_dir(cwd).output().map_err(|error| {
		let diagnostic = RepoGateFailureDiagnostic::from_spawn_error("spawn", command, &error);

		Report::new(
			RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				format!(
					"Failed to spawn repo gate command `{}` in `{}` via `{}` `{}`: {}",
					command,
					cwd.display(),
					shell.to_string_lossy(),
					shell_flag,
					error
				),
			)
			.with_diagnostic(diagnostic),
		)
	})
}

fn run_repo_gate_cleanliness_check_with_git(
	git_binary: &std::ffi::OsStr,
	cwd: &Path,
) -> Result<Output> {
	Command::new(git_binary)
		.arg("-C")
		.arg(cwd)
		.args(["status", "--porcelain", "--untracked-files=no"])
		.output()
		.map_err(|error| {
			Report::new(RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				format!(
					"Failed to spawn repo gate tracked-file cleanliness check in `{}` via `{}`: {}",
					cwd.display(),
					git_binary.to_string_lossy(),
					error
				),
			))
		})
}

fn run_repo_gate_git_command(args: &[&str], cwd: &Path) -> Result<Output> {
	Command::new("git").arg("-C").arg(cwd).args(args).output().map_err(|error| {
		eyre::eyre!(
			"Failed to inspect repo-gate changed-file classification in `{}` via `git {}`: {}",
			cwd.display(),
			args.join(" "),
			error
		)
	})
}

fn repo_gate_remote_head_ref(cwd: &Path) -> Result<String> {
	let output = run_repo_gate_git_command(
		&["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"],
		cwd,
	)?;

	if output.status.success() {
		let remote_head = repo_gate_output_text(&output);

		if !remote_head.is_empty() && remote_head != "(command produced no output)" {
			return Ok(remote_head);
		}
	}

	let remote_probe =
		run_repo_gate_git_command(&["ls-remote", "--symref", "origin", "HEAD"], cwd)?;

	if !remote_probe.status.success() {
		eyre::bail!(
			"Failed to resolve `origin/HEAD` for repo-gate changed-file classification in `{}`: {}",
			cwd.display(),
			repo_gate_output_text(&remote_probe)
		);
	}

	let stdout = String::from_utf8_lossy(&remote_probe.stdout);
	let Some(remote_head) = stdout.lines().find_map(|line| {
		let line = line.trim();

		line.strip_prefix("ref: refs/heads/")
			.and_then(|remainder| remainder.strip_suffix("\tHEAD"))
			.map(|branch_name| format!("origin/{branch_name}"))
	}) else {
		eyre::bail!(
			"Remote `origin` did not advertise a default HEAD branch for repo-gate changed-file classification in `{}`.",
			cwd.display()
		);
	};

	Ok(remote_head)
}

fn repo_gate_merge_base(cwd: &Path, base_ref: &str) -> Result<String> {
	let output = run_repo_gate_git_command(&["merge-base", "HEAD", base_ref], cwd)?;

	if !output.status.success() {
		eyre::bail!(
			"Failed to resolve merge-base for repo-gate changed-file classification in `{}` against `{}`: {}",
			cwd.display(),
			base_ref,
			repo_gate_output_text(&output)
		);
	}

	let merge_base = repo_gate_output_text(&output);

	if merge_base.is_empty() || merge_base == "(command produced no output)" {
		eyre::bail!(
			"`git merge-base` returned no revision for repo-gate changed-file classification in `{}` against `{}`.",
			cwd.display(),
			base_ref
		);
	}

	Ok(merge_base)
}

fn repo_gate_changed_files_for_diff_spec(cwd: &Path, diff_spec: &str) -> Result<BTreeSet<String>> {
	let output = run_repo_gate_git_command(
		&["diff", "--name-only", "--diff-filter=ACDMRTUXB", diff_spec],
		cwd,
	)?;

	if !output.status.success() {
		eyre::bail!(
			"Failed to compute repo-gate changed-file classification in `{}` for diff `{}`: {}",
			cwd.display(),
			diff_spec,
			repo_gate_output_text(&output)
		);
	}

	Ok(repo_gate_git_output_lines(&output))
}

fn repo_gate_changed_tracked_files(cwd: &Path) -> Result<BTreeSet<String>> {
	let base_ref = repo_gate_remote_head_ref(cwd)?;
	let merge_base = repo_gate_merge_base(cwd, &base_ref)?;
	let committed_range = format!("{merge_base}..HEAD");
	let mut changed_files = repo_gate_changed_files_for_diff_spec(cwd, &committed_range)?;

	changed_files.extend(repo_gate_changed_files_for_diff_spec(cwd, "HEAD")?);

	Ok(changed_files)
}

fn select_repo_gate_for_worktree<'a>(
	execution: &'a WorkflowExecution,
	cwd: &Path,
) -> ResolvedRepoGate<'a> {
	if execution.gate_profiles().is_empty() {
		return execution.default_repo_gate();
	}

	let changed_files = match repo_gate_changed_tracked_files(cwd) {
		Ok(changed_files) => changed_files,
		Err(error) => {
			tracing::warn!(
				repo_root = %cwd.display(),
				error = %error,
				"Falling back to the default full repo gate because changed-file classification was unavailable."
			);

			return execution.default_repo_gate();
		},
	};
	let selected_gate = execution.select_repo_gate_for_changed_files(&changed_files);

	if let Some(profile_name) = selected_gate.profile_name() {
		tracing::info!(
			repo_root = %cwd.display(),
			profile_name,
			changed_file_count = changed_files.len(),
			"Selected a narrowed repo gate profile from changed tracked files."
		);
	}

	selected_gate
}

fn run_canonicalize_commands(commands: &[String], cwd: &Path) -> Result<()> {
	for command in commands {
		let output = run_repo_gate_shell_command(command, cwd)?;

		if !output.status.success() {
			let output_text = repo_gate_output_text(&output);
			let diagnostic = RepoGateFailureDiagnostic::from_output(
				"canonicalize",
				command,
				&output,
				&output_text,
			);

			return Err(Report::new(
				RepoGateFailure::new(
					repo_gate_failure_kind_for_output(
						RepoGateFailureKind::CanonicalizeCommandFailed,
						&output_text,
					),
					format!(
						"Repo canonicalize command `{}` failed in `{}`: {}",
						command,
						cwd.display(),
						output_text
					),
				)
				.with_diagnostic(diagnostic),
			));
		}
	}

	Ok(())
}

fn run_verify_commands(commands: &[String], cwd: &Path) -> Result<()> {
	for command in commands {
		let output = run_repo_gate_shell_command(command, cwd)?;

		if !output.status.success() {
			let output_text = repo_gate_output_text(&output);
			let diagnostic =
				RepoGateFailureDiagnostic::from_output("verify", command, &output, &output_text);

			return Err(Report::new(
				RepoGateFailure::new(
					repo_gate_failure_kind_for_output(
						RepoGateFailureKind::VerifyCommandFailed,
						&output_text,
					),
					format!(
						"Repo verify command `{}` failed in `{}`: {}",
						command,
						cwd.display(),
						output_text
					),
				)
				.with_diagnostic(diagnostic),
			));
		}
	}

	Ok(())
}

fn read_repo_gate_tracked_diff_paths(cwd: &Path, phase: &str) -> Result<BTreeSet<String>> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(["diff", "--name-only", "--diff-filter=ACDMRTUXB", "HEAD", "--"])
		.output()
		.map_err(|error| {
			Report::new(RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				format!(
					"Failed to spawn tracked-file path check after repo gate {phase} in `{}`: {error}",
					cwd.display()
				),
			))
		})?;

	if !output.status.success() {
		let output_text = repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			repo_gate_failure_kind_for_output(
				RepoGateFailureKind::CleanlinessCheckFailed,
				&output_text,
			),
			format!(
				"Failed to inspect tracked-file paths after repo gate {phase} in `{}`: {}",
				cwd.display(),
				output_text
			),
		)));
	}

	Ok(repo_gate_git_output_lines(&output))
}

fn read_repo_gate_tracked_diff(cwd: &Path, phase: &str) -> Result<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(["diff", "--no-ext-diff", "--binary", "HEAD", "--"])
		.output()
		.map_err(|error| {
			Report::new(RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				format!(
					"Failed to spawn tracked-file diff check after repo gate {phase} in `{}`: {error}",
					cwd.display()
				),
			))
		})?;

	if !output.status.success() {
		let output_text = repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			repo_gate_failure_kind_for_output(
				RepoGateFailureKind::CleanlinessCheckFailed,
				&output_text,
			),
			format!(
				"Failed to inspect tracked-file diff after repo gate {phase} in `{}`: {}",
				cwd.display(),
				output_text
			),
		)));
	}

	Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn read_repo_gate_tracked_diff_for_path(cwd: &Path, phase: &str, path: &str) -> Result<String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(["diff", "--no-ext-diff", "--binary", "HEAD", "--"])
		.arg(path)
		.output()
		.map_err(|error| {
			Report::new(RepoGateFailure::new(
				RepoGateFailureKind::CommandSpawnFailed,
				format!(
					"Failed to spawn tracked-file diff check for `{}` after repo gate {phase} in `{}`: {error}",
					path,
					cwd.display()
				),
			))
		})?;

	if !output.status.success() {
		let output_text = repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			repo_gate_failure_kind_for_output(
				RepoGateFailureKind::CleanlinessCheckFailed,
				&output_text,
			),
			format!(
				"Failed to inspect tracked-file diff for `{}` after repo gate {phase} in `{}`: {}",
				path,
				cwd.display(),
				output_text
			),
		)));
	}

	Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn read_repo_gate_tracked_diff_snapshot(
	cwd: &Path,
	phase: &str,
) -> Result<RepoGateTrackedDiffSnapshot> {
	let full_diff = read_repo_gate_tracked_diff(cwd, phase)?;
	let changed_paths = read_repo_gate_tracked_diff_paths(cwd, phase)?;
	let mut path_diffs = BTreeMap::new();

	for path in changed_paths {
		let path_diff = read_repo_gate_tracked_diff_for_path(cwd, phase, &path)?;

		path_diffs.insert(path, path_diff);
	}

	Ok(RepoGateTrackedDiffSnapshot { full_diff, path_diffs })
}

fn repo_gate_rewritten_files(
	before: &RepoGateTrackedDiffSnapshot,
	after: &RepoGateTrackedDiffSnapshot,
) -> Vec<String> {
	let mut paths = before.path_diffs.keys().cloned().collect::<BTreeSet<_>>();

	paths.extend(after.path_diffs.keys().cloned());

	paths
		.into_iter()
		.filter(|path| before.path_diffs.get(path) != after.path_diffs.get(path))
		.collect()
}

fn repo_gate_tracked_rewrite_decision(
	before: &RepoGateTrackedDiffSnapshot,
	after: &RepoGateTrackedDiffSnapshot,
	allow_owned_rewrites: bool,
) -> Option<RepoGateTrackedRewriteDecision> {
	if before.full_diff == after.full_diff {
		return None;
	}

	let rewritten_files = repo_gate_rewritten_files(before, after);
	let scope_envelope = RepoGateScopeEnvelope::from_pre_gate_diff(before);
	let owned = !rewritten_files.is_empty()
		&& scope_envelope.violation_files(rewritten_files.iter().cloned()).is_empty();

	if allow_owned_rewrites && owned {
		return Some(RepoGateTrackedRewriteDecision::continue_to_commit_capable_phase(
			rewritten_files,
		));
	}

	let reason = if owned {
		"all rewritten files were pre-gate implementation paths, but this lifecycle boundary requires a clean committed worktree"
	} else {
		"one or more rewritten files were not present in the pre-gate implementation diff"
	};

	Some(RepoGateTrackedRewriteDecision::require_attention(rewritten_files, owned, reason))
}

fn repo_gate_diff_rewrite_outcome(
	cwd: &Path,
	phase: &str,
	before: &RepoGateTrackedDiffSnapshot,
	allow_owned_rewrites: bool,
) -> Result<RepoGateCommandOutcome> {
	let after = read_repo_gate_tracked_diff_snapshot(cwd, phase)?;
	let Some(decision) = repo_gate_tracked_rewrite_decision(before, &after, allow_owned_rewrites)
	else {
		return Ok(RepoGateCommandOutcome::clean());
	};

	if decision.decision == "continue_to_commit_capable_phase" {
		return Ok(RepoGateCommandOutcome::with_tracked_rewrite_decision(decision));
	}

	let output = run_repo_gate_cleanliness_check_with_git(std::ffi::OsStr::new("git"), cwd)?;

	if !output.status.success() {
		let output_text = repo_gate_output_text(&output);

		return Err(Report::new(RepoGateFailure::new(
			repo_gate_failure_kind_for_output(
				RepoGateFailureKind::CleanlinessCheckFailed,
				&output_text,
			),
			format!(
				"Failed to inspect tracked-file cleanliness after repo gate {phase} in `{}`: {}",
				cwd.display(),
				output_text
			),
		)));
	}

	let output_text = repo_gate_output_text(&output);
	let dirty_entries = output_text.trim();

	Err(Report::new(RepoGateFailure::new(
		RepoGateFailureKind::TrackedRewritesLeft,
		format!(
			"Repo gate {phase} rewrote tracked files in `{}`; owned={}; decision={}; reason={}; files={}; commit or revert these changes before continuing:\n{}",
			cwd.display(),
			decision.owned,
			decision.decision,
			decision.reason,
			decision.files_display(),
			dirty_entries
		),
	).with_tracked_rewrite_decision(decision)))
}

fn repo_gate_scope_envelope_failure_or_source(
	cwd: &Path,
	phase: &str,
	before: &RepoGateTrackedDiffSnapshot,
	source_error: Report,
) -> Report {
	let source_error_text = source_error.to_string();
	let source_repo_gate_failure = source_error.downcast_ref::<RepoGateFailure>();
	let source_error_class = source_repo_gate_failure.map(RepoGateFailure::error_class);
	let source_diagnostic =
		source_repo_gate_failure.and_then(|failure| failure.diagnostic().cloned());
	let after = match read_repo_gate_tracked_diff_snapshot(cwd, phase) {
		Ok(after) => after,
		Err(error) => {
			tracing::warn!(
				repo_root = %cwd.display(),
				phase,
				error = %error,
				"Could not inspect repo-gate write-set after command failure."
			);

			return source_error;
		},
	};
	let rewritten_files = repo_gate_rewritten_files(before, &after);
	let scope_envelope = RepoGateScopeEnvelope::from_pre_gate_diff(before);
	let scope_violation_files = scope_envelope.violation_files(rewritten_files);

	if scope_violation_files.is_empty() {
		return source_error;
	}

	let decision = RepoGateTrackedRewriteDecision::scope_envelope_violation(
		scope_violation_files,
		source_error_class,
		source_diagnostic,
	);

	Report::new(
		RepoGateFailure::new(
			RepoGateFailureKind::ScopeEnvelopeViolation,
			format!(
				"Repo gate {phase} failed after writing files outside the lane scope envelope in `{}`; files={}; source failure: {}",
				cwd.display(),
				decision.files_display(),
				source_error_text
			),
		)
		.with_tracked_rewrite_decision(decision),
	)
}

fn run_repo_gate_commands(
	canonicalize_commands: &[String],
	verify_commands: &[String],
	cwd: &Path,
) -> Result<()> {
	run_repo_gate_commands_with_rewrite_policy(canonicalize_commands, verify_commands, cwd, false)
		.map(|_| ())
}

fn run_repo_gate_commands_allow_owned_tracked_rewrites(
	canonicalize_commands: &[String],
	verify_commands: &[String],
	cwd: &Path,
) -> Result<RepoGateCommandOutcome> {
	run_repo_gate_commands_with_rewrite_policy(canonicalize_commands, verify_commands, cwd, true)
}

fn run_repo_gate_commands_with_rewrite_policy(
	canonicalize_commands: &[String],
	verify_commands: &[String],
	cwd: &Path,
	allow_owned_rewrites: bool,
) -> Result<RepoGateCommandOutcome> {
	let baseline_tracked_diff = read_repo_gate_tracked_diff_snapshot(cwd, "baseline")?;

	if let Err(error) = run_canonicalize_commands(canonicalize_commands, cwd) {
		return Err(repo_gate_scope_envelope_failure_or_source(
			cwd,
			"canonicalize",
			&baseline_tracked_diff,
			error,
		));
	}
	if let Err(error) = run_verify_commands(verify_commands, cwd) {
		return Err(repo_gate_scope_envelope_failure_or_source(
			cwd,
			"verify",
			&baseline_tracked_diff,
			error,
		));
	}

	repo_gate_diff_rewrite_outcome(
		cwd,
		"verification",
		&baseline_tracked_diff,
		allow_owned_rewrites,
	)
}

fn relative_worktree_path(project: &ServiceConfig, worktree: &WorktreeSpec) -> String {
	relative_worktree_path_for_path(project, &worktree.path)
}

fn relative_worktree_path_for_path(project: &ServiceConfig, worktree_path: &Path) -> String {
	if let Ok(relative_path) = worktree_path.strip_prefix(project.repo_root()) {
		if relative_path.as_os_str().is_empty() {
			return String::from(".");
		}

		return relative_path.display().to_string();
	}
	if let Some(root_name) = project.worktree_root().file_name()
		&& let Ok(relative_path) = worktree_path.strip_prefix(project.worktree_root())
	{
		return Path::new(root_name).join(relative_path).display().to_string();
	}

	worktree_path.file_name().map_or_else(
		|| worktree_path.display().to_string(),
		|path| path.to_string_lossy().into_owned(),
	)
}
