//! Request DTOs for explicit operator recovery commands.

/// Read-only retained review handoff diagnostic request.
#[derive(Debug)]
pub(crate) struct ReviewHandoffDiagnoseRequest {
	/// Optional issue identifier to inspect.
	pub(crate) issue: Option<String>,
	/// Emit JSON instead of text.
	pub(crate) json: bool,
}

/// Explicit retained review handoff rebind request.
#[derive(Debug)]
pub(crate) struct ReviewHandoffRebindRequest {
	/// Issue identifier to repair.
	pub(crate) issue: String,
	/// Pull request URL to bind.
	pub(crate) pr_url: String,
	/// Validate without writing a lifecycle record or tracker audit comments.
	pub(crate) dry_run: bool,
}

/// Explicit manual PR takeover into retained review handoff state.
#[derive(Debug)]
pub(crate) struct ReviewHandoffAdoptRequest {
	/// Issue identifier to adopt.
	pub(crate) issue: String,
	/// Pull request URL to adopt.
	pub(crate) pr_url: String,
	/// Validate without writing runtime lifecycle state or tracker audit comments.
	pub(crate) dry_run: bool,
}

/// Read-only ghost-lane diagnostic request.
#[derive(Debug)]
pub(crate) struct GhostLaneDiagnoseRequest {
	/// Optional issue identifier or local issue id to inspect.
	pub(crate) issue: Option<String>,
	/// Emit JSON instead of text.
	pub(crate) json: bool,
}

/// Explicit missing-issue ghost-lane cleanup request.
#[derive(Debug)]
pub(crate) struct GhostLaneCleanupRequest {
	/// Issue identifier or local issue id to terminalize.
	pub(crate) issue: String,
	/// Validate without writing runtime state.
	pub(crate) dry_run: bool,
}

/// Read-only tracker-present stale active ownership diagnostic request.
#[derive(Debug)]
pub(crate) struct StaleActiveDiagnoseRequest {
	/// Optional issue identifier or tracker issue id to inspect.
	pub(crate) issue: Option<String>,
	/// Emit JSON instead of text.
	pub(crate) json: bool,
}

/// Explicit tracker-present stale active ownership release request.
#[derive(Debug)]
pub(crate) struct StaleActiveReleaseRequest {
	/// Issue identifier or tracker issue id to release.
	pub(crate) issue: String,
	/// Validate without mutating tracker labels or runtime state.
	pub(crate) dry_run: bool,
}

/// Explicit legacy closeout audit request.
#[derive(Debug)]
pub(crate) struct LegacyCloseoutRecoveryRequest {
	/// Issue identifier to audit.
	pub(crate) issue: String,
	/// Merged pull request URL that proves terminal code lineage.
	pub(crate) pr_url: String,
	/// Validate without writing a tracker audit comment.
	pub(crate) dry_run: bool,
	/// Required for non-dry-run mutation.
	pub(crate) manual_authority: bool,
}

/// Explicit merged PR closeout reconciliation for stale retained attention.
#[derive(Debug)]
pub(crate) struct MergedCloseoutRecoveryRequest {
	/// Issue identifier to reconcile.
	pub(crate) issue: String,
	/// Merged pull request URL that proves terminal code lineage.
	pub(crate) pr_url: String,
	/// Validate without writing runtime or tracker ledger events.
	pub(crate) dry_run: bool,
	/// Required for non-dry-run mutation.
	pub(crate) manual_authority: bool,
}

/// Explicit superseded retained PR closeout after a successor PR landed the repair.
#[derive(Debug)]
pub(crate) struct SupersededCloseoutRecoveryRequest {
	/// Superseded issue identifier to terminalize.
	pub(crate) issue: String,
	/// Obsolete retained pull request URL to close.
	pub(crate) pr_url: String,
	/// Successor issue identifier that carried the landed repair.
	pub(crate) successor_issue: String,
	/// Merged successor pull request URL that proves terminal code lineage.
	pub(crate) successor_pr_url: String,
	/// Validate without writing runtime, tracker, or GitHub state.
	pub(crate) dry_run: bool,
	/// Required for non-dry-run mutation.
	pub(crate) manual_authority: bool,
}
