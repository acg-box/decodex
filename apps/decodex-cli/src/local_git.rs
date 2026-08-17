use std::{
	env,
	path::{Path, PathBuf},
	process::{Command, Output},
	thread,
	time::Duration,
};

use clap::Args;
use serde::{Deserialize, Serialize};

const DEFAULT_BRANCH: &str = "main";
const MERGE_VISIBILITY_ATTEMPTS: usize = 20;
const MERGE_VISIBILITY_DELAY: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct CommitCommand {
	/// Tree-change summary for the signed commit record.
	#[arg(value_name = "SUMMARY")]
	pub(crate) summary: String,
	/// Use the reserved local manual authority.
	#[arg(long)]
	pub(crate) manual_authority: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Args)]
pub struct LandCommand {
	/// Change summary for the signed landing record.
	#[arg(value_name = "SUMMARY")]
	pub(crate) summary: String,
	/// Use the reserved local manual authority.
	#[arg(long, requires = "pr")]
	pub(crate) manual_authority: bool,
	/// Exact pull request URL to land.
	#[arg(long, value_name = "URL")]
	pub(crate) pr: String,
	/// Exact remote default-branch object ID accepted by the landing CAS.
	#[arg(long, value_name = "OID")]
	pub(crate) expected_base_oid: String,
	/// Exact reviewed pull-request head object ID.
	#[arg(long, value_name = "OID")]
	pub(crate) expected_head_oid: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullRequest {
	url: String,
	state: String,
	is_draft: bool,
	is_cross_repository: bool,
	base_ref_name: String,
	base_ref_oid: Option<String>,
	head_ref_name: String,
	head_ref_oid: String,
	merge_commit: Option<MergeCommit>,
}

#[derive(Debug, Deserialize)]
struct MergeCommit {
	oid: String,
}

#[derive(Serialize)]
struct CommitRecord<'a> {
	schema: &'static str,
	change: &'a str,
	authority: &'static str,
	impact: &'static str,
}

struct RepositoryLayout {
	checkout: PathBuf,
	primary: PathBuf,
	is_primary: bool,
}

struct WorktreeEntry {
	path: PathBuf,
	branch: Option<String>,
	bare: bool,
	prunable: bool,
}

pub(crate) fn execute_commit(command: &CommitCommand) -> Result<String, String> {
	require_manual_authority(command.manual_authority)?;
	let summary = normalized_summary(&command.summary)?;
	let cwd =
		env::current_dir().map_err(|error| format!("current directory unavailable: {error}"))?;
	let layout = repository_layout(&cwd)?;

	if layout.is_primary {
		return Err(String::from("`decodex commit` must run from an isolated task worktree."));
	}
	require_staged_only_changes(&layout.checkout)?;
	let record = commit_record(summary, false)?;

	git_checked(&layout.checkout, &["commit", "-S", "-m", &record])?;
	let commit = git_stdout(&layout.checkout, &["rev-parse", "HEAD"])?;

	validate_oid(&commit)?;
	git_checked(&layout.checkout, &["verify-commit", "--raw", &commit])?;

	if git_stdout(&layout.checkout, &["show", "-s", "--format=%s", &commit])? != record {
		return Err(String::from("signed commit record readback mismatch"));
	}

	Ok(format!("commit ok: commit={commit}"))
}

pub(crate) fn execute_land(command: &LandCommand) -> Result<String, String> {
	require_manual_authority(command.manual_authority)?;
	let summary = normalized_summary(&command.summary)?;

	validate_oid(&command.expected_base_oid)?;
	validate_oid(&command.expected_head_oid)?;

	let cwd =
		env::current_dir().map_err(|error| format!("current directory unavailable: {error}"))?;
	let layout = repository_layout(&cwd)?;
	require_pull_request_repository(&layout.primary, &command.pr)?;
	let mut pull_request = read_pull_request(&layout.primary, &command.pr)?;
	git_checked(&layout.primary, &["check-ref-format", "--branch", &pull_request.head_ref_name])
		.map_err(|_| String::from("pull request head branch is not a valid local branch name"))?;

	validate_pull_request_identity(&pull_request, command)?;

	if pull_request.state == "OPEN" {
		if layout.is_primary {
			return Err(String::from(
				"an open pull request must be landed from its isolated task worktree",
			));
		}
		validate_open_lane(&layout, &pull_request, command)?;
		require_primary_at_base(&layout.primary, &command.expected_base_oid)?;

		let record = commit_record(summary, true)?;
		let merge_commit = create_and_push_exact_merge(
			&layout,
			&command.expected_base_oid,
			&command.expected_head_oid,
			&record,
		)?;

		pull_request = wait_for_exact_merge(&layout.primary, &command.pr, &merge_commit)?;
	} else if pull_request.state != "MERGED" {
		return Err(format!(
			"pull request `{}` is `{}` and cannot be landed",
			command.pr, pull_request.state
		));
	}

	let merge_commit = pull_request
		.merge_commit
		.as_ref()
		.map(|merge_commit| merge_commit.oid.as_str())
		.ok_or_else(|| String::from("merged pull request has no merge commit object ID"))?;
	let record = commit_record(summary, true)?;

	fetch_default_branch(&layout.primary)?;
	verify_exact_merge(
		&layout.primary,
		merge_commit,
		&command.expected_base_oid,
		&command.expected_head_oid,
		&record,
		true,
	)?;
	sync_primary(&layout.primary, &command.expected_base_oid, merge_commit)?;
	cleanup_lane(&layout, &pull_request.head_ref_name, &command.expected_head_oid)?;
	require_primary_synced(&layout.primary, merge_commit)?;

	Ok(format!(
		"land ok: pr={} merge_commit={} default_branch={} local_default_branch_synced=true",
		command.pr, merge_commit, DEFAULT_BRANCH
	))
}

fn require_manual_authority(enabled: bool) -> Result<(), String> {
	if enabled {
		Ok(())
	} else {
		Err(String::from("local `decodex commit` and `decodex land` require `--manual-authority`"))
	}
}

fn normalized_summary(value: &str) -> Result<&str, String> {
	let value = value.trim();

	if value.is_empty()
		|| value.len() > 256
		|| value.chars().any(char::is_control)
		|| value.contains('\n')
	{
		return Err(String::from("change summary must be one nonempty line of at most 256 bytes"));
	}

	Ok(value)
}

fn commit_record(summary: &str, landed: bool) -> Result<String, String> {
	let change = if landed {
		format!("Land {}", summary.strip_prefix("Land ").unwrap_or(summary))
	} else {
		summary.to_owned()
	};

	serde_json::to_string(&CommitRecord {
		schema: "decodex/commit/2",
		change: &change,
		authority: "manual",
		impact: "compatible",
	})
	.map_err(|error| format!("commit record serialization failed: {error}"))
}

fn repository_layout(cwd: &Path) -> Result<RepositoryLayout, String> {
	let checkout = canonical_git_path(cwd, &["rev-parse", "--show-toplevel"], "checkout")?;
	let common_dir = canonical_git_path(
		&checkout,
		&["rev-parse", "--path-format=absolute", "--git-common-dir"],
		"Git common directory",
	)?;
	let inventory = worktree_inventory(&checkout)?;
	let primary_entry =
		inventory.first().ok_or_else(|| String::from("Git worktree inventory is empty"))?;

	if primary_entry.bare || primary_entry.prunable {
		return Err(String::from("Git main worktree is not an available checkout"));
	}

	let primary = primary_entry
		.path
		.canonicalize()
		.map_err(|error| format!("primary path cannot be canonicalized: {error}"))?;
	let primary_top_level =
		canonical_git_path(&primary, &["rev-parse", "--show-toplevel"], "primary checkout")?;
	let primary_common_dir = canonical_git_path(
		&primary,
		&["rev-parse", "--path-format=absolute", "--git-common-dir"],
		"primary Git common directory",
	)?;

	if primary_top_level != primary || primary_common_dir != common_dir {
		return Err(String::from(
			"Git main worktree and checkout do not share one exact repository",
		));
	}

	let matching_entries = inventory
		.iter()
		.enumerate()
		.filter_map(|(index, entry)| {
			entry.path.canonicalize().ok().filter(|path| path == &checkout).map(|_| (index, entry))
		})
		.collect::<Vec<_>>();

	if matching_entries.len() != 1 {
		return Err(String::from(
			"checkout is not one exact registered Git worktree of this repository",
		));
	}

	let (checkout_index, checkout_entry) = matching_entries[0];

	if checkout_entry.bare || checkout_entry.prunable {
		return Err(String::from("registered Git worktree is not an available checkout"));
	}

	let is_primary = checkout_index == 0;

	if is_primary != (checkout == primary) {
		return Err(String::from("Git worktree inventory primary identity mismatch"));
	}

	Ok(RepositoryLayout { checkout, primary, is_primary })
}

fn canonical_git_path(cwd: &Path, args: &[&str], label: &str) -> Result<PathBuf, String> {
	PathBuf::from(git_stdout(cwd, args)?)
		.canonicalize()
		.map_err(|error| format!("{label} path cannot be canonicalized: {error}"))
}

fn worktree_inventory(cwd: &Path) -> Result<Vec<WorktreeEntry>, String> {
	let output = command_output("git", cwd, &["worktree", "list", "--porcelain", "-z"])?;
	let stdout = String::from_utf8(output.stdout)
		.map_err(|error| format!("Git worktree inventory is not UTF-8: {error}"))?;
	let mut inventory = Vec::new();
	let mut current: Option<WorktreeEntry> = None;

	for field in stdout.split('\0') {
		if field.is_empty() {
			if let Some(entry) = current.take() {
				inventory.push(entry);
			}
			continue;
		}

		if let Some(path) = field.strip_prefix("worktree ") {
			if current.is_some() || path.is_empty() || !Path::new(path).is_absolute() {
				return Err(String::from("Git worktree inventory is malformed"));
			}
			current = Some(WorktreeEntry {
				path: PathBuf::from(path),
				branch: None,
				bare: false,
				prunable: false,
			});
			continue;
		}

		let entry =
			current.as_mut().ok_or_else(|| String::from("Git worktree inventory is malformed"))?;

		if let Some(branch) = field.strip_prefix("branch ") {
			if entry.branch.replace(branch.to_owned()).is_some() {
				return Err(String::from("Git worktree inventory branch is duplicated"));
			}
		} else if field == "bare" {
			entry.bare = true;
		} else if field == "prunable" || field.starts_with("prunable ") {
			entry.prunable = true;
		}
	}

	if current.is_some() {
		return Err(String::from("Git worktree inventory record is unterminated"));
	}

	Ok(inventory)
}

fn require_staged_only_changes(checkout: &Path) -> Result<(), String> {
	let status = git_stdout(checkout, &["status", "--porcelain=v1", "--untracked-files=all"])?;
	let mut staged = false;

	for line in status.lines() {
		let bytes = line.as_bytes();

		if bytes.len() < 3 || line.starts_with("??") || bytes[1] != b' ' || bytes[0] == b' ' {
			return Err(String::from(
				"`decodex commit` requires only tracked staged changes and no unstaged or untracked files",
			));
		}
		staged = true;
	}
	if !staged {
		return Err(String::from("`decodex commit` requires a nonempty staged change"));
	}

	Ok(())
}

fn read_pull_request(cwd: &Path, pr_url: &str) -> Result<PullRequest, String> {
	github_repository_from_pull_request_url(pr_url)?;

	let output = command_output(
		"gh",
		cwd,
		&[
			"pr",
			"view",
			pr_url,
			"--json",
			"url,state,isDraft,isCrossRepository,baseRefName,baseRefOid,headRefName,headRefOid,mergeCommit",
		],
	)?;

	serde_json::from_slice(&output.stdout)
		.map_err(|error| format!("pull request readback is malformed: {error}"))
}

fn require_pull_request_repository(primary: &Path, pr_url: &str) -> Result<(), String> {
	let pull_request_repository = github_repository_from_pull_request_url(pr_url)?;
	let origin = git_stdout(primary, &["config", "--get", "remote.origin.url"])?;
	let origin_repository = github_repository_from_remote(&origin)?;

	if pull_request_repository != origin_repository {
		return Err(String::from("pull request repository does not match the checkout `origin`"));
	}

	Ok(())
}

fn github_repository_from_pull_request_url(value: &str) -> Result<String, String> {
	let path = value
		.strip_prefix("https://github.com/")
		.ok_or_else(|| String::from("`--pr` must be a canonical GitHub pull request URL"))?;
	let segments = path.split('/').collect::<Vec<_>>();

	if segments.len() != 4
		|| segments[2] != "pull"
		|| !valid_github_component(segments[0])
		|| !valid_github_component(segments[1])
		|| segments[3].is_empty()
		|| !segments[3].bytes().all(|byte| byte.is_ascii_digit())
		|| segments[3].starts_with('0')
	{
		return Err(String::from("`--pr` must be a canonical GitHub pull request URL"));
	}

	Ok(format!("{}/{}", segments[0], segments[1]))
}

fn github_repository_from_remote(value: &str) -> Result<String, String> {
	let path = value
		.strip_prefix("git@github.com:")
		.or_else(|| value.strip_prefix("ssh://git@github.com/"))
		.or_else(|| value.strip_prefix("https://github.com/"))
		.ok_or_else(|| String::from("`origin` must be a canonical GitHub repository URL"))?;
	let path = path.strip_suffix(".git").unwrap_or(path);
	let segments = path.split('/').collect::<Vec<_>>();

	if segments.len() != 2
		|| !valid_github_component(segments[0])
		|| !valid_github_component(segments[1])
	{
		return Err(String::from("`origin` must be a canonical GitHub repository URL"));
	}

	Ok(format!("{}/{}", segments[0], segments[1]))
}

fn valid_github_component(value: &str) -> bool {
	!value.is_empty()
		&& value.len() <= 100
		&& value != "."
		&& value != ".."
		&& value
			.bytes()
			.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_pull_request_identity(
	pull_request: &PullRequest,
	command: &LandCommand,
) -> Result<(), String> {
	if pull_request.url != command.pr
		|| pull_request.is_cross_repository
		|| pull_request.base_ref_name != DEFAULT_BRANCH
		|| pull_request.head_ref_oid != command.expected_head_oid
		|| pull_request.head_ref_name == DEFAULT_BRANCH
	{
		return Err(String::from("pull request identity does not match the landing intent"));
	}
	if pull_request.state == "OPEN"
		&& (pull_request.is_draft
			|| pull_request.base_ref_oid.as_deref() != Some(&command.expected_base_oid))
	{
		return Err(String::from(
			"open pull request state or base object ID does not match the landing intent",
		));
	}

	Ok(())
}

fn validate_open_lane(
	layout: &RepositoryLayout,
	pull_request: &PullRequest,
	command: &LandCommand,
) -> Result<(), String> {
	let branch = git_stdout(&layout.checkout, &["branch", "--show-current"])?;
	let head = git_stdout(&layout.checkout, &["rev-parse", "HEAD"])?;
	let status =
		git_stdout(&layout.checkout, &["status", "--porcelain=v1", "--untracked-files=all"])?;

	if branch != pull_request.head_ref_name
		|| head != command.expected_head_oid
		|| !status.is_empty()
	{
		return Err(String::from(
			"task worktree is not the exact clean reviewed pull request head",
		));
	}

	Ok(())
}

fn require_primary_at_base(primary: &Path, expected_base_oid: &str) -> Result<(), String> {
	require_primary_branch_and_clean(primary)?;
	fetch_default_branch(primary)?;
	let local = git_stdout(primary, &["rev-parse", "HEAD"])?;
	let remote = git_stdout(primary, &["rev-parse", "refs/remotes/origin/main"])?;

	if local != expected_base_oid || remote != expected_base_oid {
		return Err(String::from(
			"primary and remote default branches do not equal the exact landing base",
		));
	}

	Ok(())
}

fn require_primary_branch_and_clean(primary: &Path) -> Result<(), String> {
	let branch = git_stdout(primary, &["branch", "--show-current"])?;
	let status = git_stdout(primary, &["status", "--porcelain=v1", "--untracked-files=all"])?;

	if branch != DEFAULT_BRANCH || !status.is_empty() {
		return Err(String::from("primary checkout must be clean and on `main`"));
	}

	Ok(())
}

fn create_and_push_exact_merge(
	layout: &RepositoryLayout,
	expected_base_oid: &str,
	expected_head_oid: &str,
	record: &str,
) -> Result<String, String> {
	let tree =
		git_stdout(&layout.checkout, &["rev-parse", &format!("{expected_head_oid}^{{tree}}")])?;
	let merge_commit = git_stdout(
		&layout.checkout,
		&[
			"commit-tree",
			&tree,
			"-p",
			expected_base_oid,
			"-p",
			expected_head_oid,
			"-S",
			"-m",
			record,
		],
	)?;

	verify_exact_merge(
		&layout.primary,
		&merge_commit,
		expected_base_oid,
		expected_head_oid,
		record,
		false,
	)?;

	let remote_before = remote_branch_oid(&layout.primary, DEFAULT_BRANCH)?;

	if remote_before.as_deref() != Some(expected_base_oid) {
		return Err(String::from(
			"remote default branch changed before the landing compare-and-swap",
		));
	}

	let lease = format!("--force-with-lease=refs/heads/main:{expected_base_oid}");
	let refspec = format!("{merge_commit}:refs/heads/main");
	let push = Command::new("git")
		.arg("-C")
		.arg(&layout.primary)
		.args(["push", &lease, "origin", &refspec])
		.env("GIT_TERMINAL_PROMPT", "0")
		.output()
		.map_err(|error| format!("failed to start exact landing push: {error}"))?;
	let remote_after = remote_branch_oid(&layout.primary, DEFAULT_BRANCH)?;

	if remote_after.as_deref() == Some(merge_commit.as_str()) {
		return Ok(merge_commit);
	}

	Err(format!(
		"exact landing compare-and-swap failed and remote `main` remains at `{}`: {}",
		remote_after.as_deref().unwrap_or("absent"),
		output_detail(&push)
	))
}

fn wait_for_exact_merge(
	cwd: &Path,
	pr_url: &str,
	expected_merge: &str,
) -> Result<PullRequest, String> {
	for attempt in 1..=MERGE_VISIBILITY_ATTEMPTS {
		let pull_request = read_pull_request(cwd, pr_url)?;
		let observed =
			pull_request.merge_commit.as_ref().map(|merge_commit| merge_commit.oid.as_str());

		if pull_request.state == "MERGED" {
			if observed == Some(expected_merge) {
				return Ok(pull_request);
			}

			return Err(String::from(
				"pull request merged with an unexpected merge commit object ID",
			));
		}
		if pull_request.state != "OPEN" {
			return Err(format!(
				"pull request entered unexpected state `{}` after landing",
				pull_request.state
			));
		}
		if attempt < MERGE_VISIBILITY_ATTEMPTS {
			thread::sleep(MERGE_VISIBILITY_DELAY);
		}
	}

	Err(String::from(
		"exact merge is on remote `main`, but pull request merge visibility is pending",
	))
}

fn verify_exact_merge(
	primary: &Path,
	merge_commit: &str,
	expected_base_oid: &str,
	expected_head_oid: &str,
	record: &str,
	require_remote_ancestry: bool,
) -> Result<(), String> {
	validate_oid(merge_commit)?;
	let parents = git_stdout(primary, &["show", "-s", "--format=%P", merge_commit])?;

	if parents != format!("{expected_base_oid} {expected_head_oid}") {
		return Err(String::from("landing merge parent readback mismatch"));
	}

	let merge_tree = git_stdout(primary, &["rev-parse", &format!("{merge_commit}^{{tree}}")])?;
	let head_tree = git_stdout(primary, &["rev-parse", &format!("{expected_head_oid}^{{tree}}")])?;

	if merge_tree != head_tree {
		return Err(String::from("landing merge tree differs from the reviewed head tree"));
	}

	git_checked(primary, &["verify-commit", "--raw", merge_commit])?;

	if git_stdout(primary, &["show", "-s", "--format=%s", merge_commit])? != record {
		return Err(String::from("landing merge record readback mismatch"));
	}
	if require_remote_ancestry {
		let fetched_remote = git_stdout(primary, &["rev-parse", "refs/remotes/origin/main"])?;
		let observed_remote = remote_branch_oid(primary, DEFAULT_BRANCH)?;

		if observed_remote.as_deref() != Some(&fetched_remote)
			|| !git_is_ancestor(primary, merge_commit, &fetched_remote)?
		{
			return Err(String::from(
				"landing merge is not in the exact current remote `main` lineage",
			));
		}
	}

	Ok(())
}

fn sync_primary(primary: &Path, expected_base_oid: &str, merge_commit: &str) -> Result<(), String> {
	require_primary_branch_and_clean(primary)?;
	let local = git_stdout(primary, &["rev-parse", "HEAD"])?;
	let remote = git_stdout(primary, &["rev-parse", "refs/remotes/origin/main"])?;
	let local_contains_merge =
		local == merge_commit || git_is_ancestor(primary, merge_commit, &local)?;

	if (local != expected_base_oid && !local_contains_merge)
		|| !git_is_ancestor(primary, &local, &remote)?
		|| !git_is_ancestor(primary, merge_commit, &remote)?
	{
		return Err(String::from(
			"primary default branch cannot fast-forward from the exact landing lineage",
		));
	}

	git_checked(primary, &["merge", "--ff-only", "refs/remotes/origin/main"])
}

fn cleanup_lane(
	layout: &RepositoryLayout,
	branch: &str,
	expected_head_oid: &str,
) -> Result<(), String> {
	preflight_lane_cleanup(layout, branch, expected_head_oid)?;
	delete_remote_branch_exact(&layout.primary, branch, expected_head_oid)?;

	if !layout.is_primary {
		git_checked(
			&layout.primary,
			&[
				"worktree",
				"remove",
				layout
					.checkout
					.to_str()
					.ok_or_else(|| String::from("task worktree path is not UTF-8"))?,
			],
		)?;
	}

	let local_head = git_stdout_allow_missing(
		&layout.primary,
		&["show-ref", "--verify", "--hash", &format!("refs/heads/{branch}")],
	)?;

	if let Some(local_head) = local_head {
		if local_head != expected_head_oid {
			return Err(String::from("local task branch advanced before post-land cleanup"));
		}
		git_checked(&layout.primary, &["branch", "-D", "--", branch])?;
	}

	Ok(())
}

fn preflight_lane_cleanup(
	layout: &RepositoryLayout,
	branch: &str,
	expected_head_oid: &str,
) -> Result<(), String> {
	let branch_ref = format!("refs/heads/{branch}");

	for entry in worktree_inventory(&layout.primary)? {
		if entry.branch.as_deref() == Some(&branch_ref) {
			let observed = entry
				.path
				.canonicalize()
				.map_err(|_| String::from("task branch worktree cannot be canonicalized"))?;

			if layout.is_primary || observed != layout.checkout {
				return Err(String::from("task branch is checked out in an unexpected worktree"));
			}
		}
	}

	if !layout.is_primary {
		let branch_now = git_stdout(&layout.checkout, &["branch", "--show-current"])?;
		let head_now = git_stdout(&layout.checkout, &["rev-parse", "HEAD"])?;
		let status =
			git_stdout(&layout.checkout, &["status", "--porcelain=v1", "--untracked-files=all"])?;

		if branch_now != branch || head_now != expected_head_oid || !status.is_empty() {
			return Err(String::from("task worktree changed before exact post-land cleanup"));
		}
	}

	let local_head = git_stdout_allow_missing(
		&layout.primary,
		&["show-ref", "--verify", "--hash", &branch_ref],
	)?;

	if local_head.as_deref().is_some_and(|head| head != expected_head_oid) {
		return Err(String::from("local task branch advanced before post-land cleanup"));
	}

	Ok(())
}

fn delete_remote_branch_exact(
	primary: &Path,
	branch: &str,
	expected_head_oid: &str,
) -> Result<(), String> {
	let remote = remote_branch_oid(primary, branch)?;

	if remote.is_none() {
		return Ok(());
	}
	if remote.as_deref() != Some(expected_head_oid) {
		return Err(String::from("remote task branch advanced before post-land cleanup"));
	}

	let branch_ref = format!("refs/heads/{branch}");
	let lease = format!("--force-with-lease={branch_ref}:{expected_head_oid}");
	let delete_refspec = format!(":{branch_ref}");
	let output = Command::new("git")
		.arg("-C")
		.arg(primary)
		.args(["push", &lease, "origin", &delete_refspec])
		.env("GIT_TERMINAL_PROMPT", "0")
		.output()
		.map_err(|error| format!("failed to start exact branch cleanup: {error}"))?;

	if remote_branch_oid(primary, branch)?.is_none() {
		return Ok(());
	}

	Err(format!("exact remote task branch cleanup failed: {}", output_detail(&output)))
}

fn require_primary_synced(primary: &Path, merge_commit: &str) -> Result<(), String> {
	require_primary_branch_and_clean(primary)?;
	let local = git_stdout(primary, &["rev-parse", "HEAD"])?;
	let remote = remote_branch_oid(primary, DEFAULT_BRANCH)?;

	if remote.as_deref() != Some(&local) || !git_is_ancestor(primary, merge_commit, &local)? {
		return Err(String::from("post-land primary and remote default-branch readback mismatch"));
	}

	Ok(())
}

fn fetch_default_branch(primary: &Path) -> Result<(), String> {
	git_checked(
		primary,
		&["fetch", "--quiet", "origin", "refs/heads/main:refs/remotes/origin/main"],
	)
}

fn git_is_ancestor(primary: &Path, ancestor: &str, descendant: &str) -> Result<bool, String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(primary)
		.args(["merge-base", "--is-ancestor", ancestor, descendant])
		.env("GIT_TERMINAL_PROMPT", "0")
		.output()
		.map_err(|error| format!("failed to start Git ancestry readback: {error}"))?;

	if output.status.success() {
		return Ok(true);
	}
	if output.status.code() == Some(1) {
		return Ok(false);
	}

	Err(format!("Git ancestry readback failed: {}", output_detail(&output)))
}

fn remote_branch_oid(primary: &Path, branch: &str) -> Result<Option<String>, String> {
	let branch_ref = format!("refs/heads/{branch}");
	let output = command_output("git", primary, &["ls-remote", "--heads", "origin", &branch_ref])?;
	let stdout = String::from_utf8(output.stdout)
		.map_err(|error| format!("remote branch readback is not UTF-8: {error}"))?;
	let lines = stdout.lines().collect::<Vec<_>>();

	if lines.is_empty() {
		return Ok(None);
	}
	if lines.len() != 1 {
		return Err(String::from("remote branch readback is ambiguous"));
	}

	let fields = lines[0].split_ascii_whitespace().collect::<Vec<_>>();

	if fields.len() != 2 || fields[1] != branch_ref {
		return Err(String::from("remote branch readback is malformed"));
	}
	validate_oid(fields[0])?;

	Ok(Some(fields[0].to_owned()))
}

fn validate_oid(value: &str) -> Result<(), String> {
	if matches!(value.len(), 40 | 64)
		&& value.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
	{
		return Ok(());
	}

	Err(String::from("expected a full lowercase Git object ID"))
}

fn git_stdout(cwd: &Path, args: &[&str]) -> Result<String, String> {
	let output = command_output("git", cwd, args)?;

	String::from_utf8(output.stdout)
		.map(|value| value.trim().to_owned())
		.map_err(|error| format!("Git output is not UTF-8: {error}"))
}

fn git_stdout_allow_missing(cwd: &Path, args: &[&str]) -> Result<Option<String>, String> {
	let output = Command::new("git")
		.arg("-C")
		.arg(cwd)
		.args(args)
		.env("GIT_TERMINAL_PROMPT", "0")
		.output()
		.map_err(|error| format!("failed to start Git: {error}"))?;

	if output.status.success() {
		return String::from_utf8(output.stdout)
			.map(|value| Some(value.trim().to_owned()))
			.map_err(|error| format!("Git output is not UTF-8: {error}"));
	}
	if output.status.code() == Some(1) {
		return Ok(None);
	}

	Err(format!("Git readback failed: {}", output_detail(&output)))
}

fn git_checked(cwd: &Path, args: &[&str]) -> Result<(), String> {
	command_output("git", cwd, args).map(|_| ())
}

fn command_output(program: &str, cwd: &Path, args: &[&str]) -> Result<Output, String> {
	let output = Command::new(program)
		.current_dir(cwd)
		.args(args)
		.env("GH_PROMPT_DISABLED", "1")
		.env("GIT_TERMINAL_PROMPT", "0")
		.output()
		.map_err(|error| format!("failed to start `{program}`: {error}"))?;

	if output.status.success() {
		return Ok(output);
	}

	Err(format!("`{program}` failed: {}", output_detail(&output)))
}

fn output_detail(output: &Output) -> String {
	let stderr = String::from_utf8_lossy(&output.stderr);
	let stdout = String::from_utf8_lossy(&output.stdout);
	let detail = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
	let mut bounded = detail
		.chars()
		.filter(|character| !character.is_control() || *character == ' ')
		.take(512)
		.collect::<String>();

	if bounded.is_empty() {
		bounded = format!("exit status {}", output.status);
	}

	bounded
}

#[cfg(all(test, unix))]
mod tests {
	use std::{fs, os::unix::fs::PermissionsExt as _, process::Command};

	use tempfile::TempDir;

	use super::{
		RepositoryLayout, create_and_push_exact_merge, git_checked, git_stdout,
		github_repository_from_pull_request_url, github_repository_from_remote, remote_branch_oid,
		verify_exact_merge,
	};

	const RECORD: &str = r#"{"schema":"decodex/commit/2","change":"Land exact candidate","authority":"manual","impact":"compatible"}"#;

	#[test]
	fn canonical_github_repository_identities_match_supported_urls() {
		assert_eq!(
			github_repository_from_pull_request_url("https://github.com/acg-box/decodex/pull/123")
				.unwrap(),
			"acg-box/decodex"
		);
		for remote in [
			"git@github.com:acg-box/decodex.git",
			"ssh://git@github.com/acg-box/decodex.git",
			"https://github.com/acg-box/decodex.git",
			"https://github.com/acg-box/decodex",
		] {
			assert_eq!(github_repository_from_remote(remote).unwrap(), "acg-box/decodex");
		}
	}

	#[test]
	fn noncanonical_github_repository_identities_fail_closed() {
		for pull_request in [
			"http://github.com/acg-box/decodex/pull/1",
			"https://github.com/acg-box/decodex/pull/0",
			"https://github.com/acg-box/decodex/pull/01",
			"https://github.com/acg-box/decodex/pull/1/",
			"https://github.com/acg-box/decodex/issues/1",
			"https://example.com/acg-box/decodex/pull/1",
		] {
			assert!(github_repository_from_pull_request_url(pull_request).is_err());
		}
		for remote in [
			"git@example.com:acg-box/decodex.git",
			"https://github.com/acg-box/decodex/extra",
			"/tmp/decodex.git",
		] {
			assert!(github_repository_from_remote(remote).is_err());
		}
	}

	#[test]
	fn exact_signed_merge_compare_and_swap_updates_only_the_expected_base() {
		let fixture = Fixture::new();
		let merge_commit =
			create_and_push_exact_merge(&fixture.layout, &fixture.base, &fixture.head, RECORD)
				.expect("exact merge should land");

		assert_eq!(
			remote_branch_oid(&fixture.layout.primary, "main")
				.expect("remote main should read")
				.as_deref(),
			Some(merge_commit.as_str())
		);
		verify_exact_merge(
			&fixture.layout.primary,
			&merge_commit,
			&fixture.base,
			&fixture.head,
			RECORD,
			true,
		)
		.expect("exact merge evidence should verify");
	}

	#[test]
	fn exact_signed_merge_compare_and_swap_rejects_head_as_racing_base() {
		let fixture = Fixture::new();
		let hooks = fixture.temp.path().join("hooks");
		let hook = hooks.join("pre-push");

		fs::create_dir_all(&hooks).expect("hooks directory should create");
		fs::write(
			&hook,
			format!(
				"#!/bin/sh\ngit --git-dir='{}' update-ref refs/heads/main '{}'\n",
				fixture.origin.display(),
				fixture.head
			),
		)
		.expect("pre-push hook should write");
		let mut permissions = fs::metadata(&hook).expect("hook metadata should read").permissions();

		permissions.set_mode(0o700);
		fs::set_permissions(&hook, permissions).expect("hook should be executable");
		git_checked(
			&fixture.layout.primary,
			&["config", "core.hooksPath", hooks.to_str().expect("hook path is UTF-8")],
		)
		.expect("hook path should configure");

		let error =
			create_and_push_exact_merge(&fixture.layout, &fixture.base, &fixture.head, RECORD)
				.expect_err("a raced base must reject");

		assert!(error.contains("compare-and-swap failed"));
		assert_eq!(
			remote_branch_oid(&fixture.layout.primary, "main")
				.expect("remote main should read")
				.as_deref(),
			Some(fixture.head.as_str())
		);
		assert!(fixture.layout.checkout.exists());
	}

	struct Fixture {
		temp: TempDir,
		origin: std::path::PathBuf,
		layout: RepositoryLayout,
		base: String,
		head: String,
	}
	impl Fixture {
		fn new() -> Self {
			let temp = TempDir::new().expect("temp directory should create");
			let origin = temp.path().join("origin.git");
			let primary = temp.path().join("repo");
			let checkout = primary.join(".worktrees/exact");
			let empty_hooks = temp.path().join("empty-hooks");

			fs::create_dir_all(&empty_hooks).expect("empty hooks directory should create");
			run(Command::new("git").args([
				"init",
				"--bare",
				"--initial-branch=main",
				origin.to_str().expect("origin path is UTF-8"),
			]));
			run(Command::new("git").args([
				"clone",
				origin.to_str().expect("origin path is UTF-8"),
				primary.to_str().expect("primary path is UTF-8"),
			]));
			git_checked(&primary, &["config", "user.name", "Decodex Tests"])
				.expect("user name should configure");
			git_checked(&primary, &["config", "user.email", "decodex-tests@example.com"])
				.expect("user email should configure");
			git_checked(
				&primary,
				&["config", "core.hooksPath", empty_hooks.to_str().expect("hooks path is UTF-8")],
			)
			.expect("empty hooks should configure");
			fs::write(primary.join("README.md"), "base\n").expect("readme should write");
			git_checked(&primary, &["add", "README.md"]).expect("readme should stage");
			git_checked(
				&primary,
				&[
					"commit",
					"-m",
					r#"{"schema":"decodex/commit/2","change":"base","authority":"manual","impact":"compatible"}"#,
				],
			)
			.expect("base should commit");
			git_checked(&primary, &["push", "-u", "origin", "main"]).expect("base should push");
			let base = git_stdout(&primary, &["rev-parse", "HEAD"]).expect("base should read");

			git_checked(
				&primary,
				&[
					"worktree",
					"add",
					"-b",
					"xv/exact",
					checkout.to_str().expect("checkout path is UTF-8"),
				],
			)
			.expect("task worktree should create");
			fs::write(checkout.join("feature.txt"), "feature\n").expect("feature should write");
			git_checked(&checkout, &["add", "feature.txt"]).expect("feature should stage");
			git_checked(
				&checkout,
				&[
					"commit",
					"-m",
					r#"{"schema":"decodex/commit/2","change":"feature","authority":"manual","impact":"compatible"}"#,
				],
			)
				.expect("feature should commit");
			git_checked(&checkout, &["push", "-u", "origin", "xv/exact"])
				.expect("feature branch should push");
			let head = git_stdout(&checkout, &["rev-parse", "HEAD"]).expect("head should read");

			configure_signing(&temp, &primary);

			Self {
				temp,
				origin,
				layout: RepositoryLayout { checkout, primary, is_primary: false },
				base,
				head,
			}
		}
	}

	fn configure_signing(temp: &TempDir, primary: &std::path::Path) {
		let key = temp.path().join("signing-key");

		run(Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-f"]).arg(&key));
		let public_key =
			fs::read_to_string(key.with_extension("pub")).expect("public key should read");
		let allowed_signers = temp.path().join("allowed-signers");

		fs::write(&allowed_signers, format!("decodex-tests@example.com {}", public_key.trim()))
			.expect("allowed signers should write");
		git_checked(primary, &["config", "gpg.format", "ssh"])
			.expect("SSH signing should configure");
		git_checked(
			primary,
			&["config", "user.signingkey", key.to_str().expect("key path is UTF-8")],
		)
		.expect("signing key should configure");
		git_checked(
			primary,
			&[
				"config",
				"gpg.ssh.allowedSignersFile",
				allowed_signers.to_str().expect("allowed signer path is UTF-8"),
			],
		)
		.expect("allowed signers should configure");
	}

	fn run(command: &mut Command) {
		assert!(
			command.status().expect("test command should start").success(),
			"test command should succeed"
		);
	}
}
