use crate::orchestrator::execution_architecture_recovery::{
	self, ArchitectureRecoveryBoundary, AuthorityBoundaryChangedSurface,
	AuthorityBoundaryDisposition, AuthorityBoundaryPolicyDecision, AuthorityBoundarySurface, Path,
	surface::path_classification,
};

pub(in crate::orchestrator::execution_architecture_recovery) fn architecture_recovery_changed_surfaces(
	boundary: &ArchitectureRecoveryBoundary,
	worktree_path: &Path,
) -> Vec<AuthorityBoundaryChangedSurface<'static>> {
	let mut surfaces = Vec::new();

	push_architecture_recovery_changed_surface(
		&mut surfaces,
		boundary.boundary_type,
		"Replace the non-converging guardrail repair strategy with a materially different architecture recovery strategy.",
		boundary.policy_decision,
		boundary.disposition,
	);

	if let Ok(Some(diff_paths)) = execution_architecture_recovery::git_guardrail_output(
		worktree_path,
		&["diff", "--name-only", "HEAD", "--"],
	) {
		for relative_path in diff_paths.lines().filter(|path| !path.trim().is_empty()) {
			for surface in
				path_classification::architecture_recovery_surfaces_for_path(relative_path)
			{
				push_architecture_recovery_changed_surface(
					&mut surfaces,
					surface,
					path_classification::architecture_recovery_surface_summary(surface),
					surface.policy_decision(),
					surface.policy_decision().disposition(),
				);
			}
		}
	}

	surfaces
}

fn push_architecture_recovery_changed_surface(
	surfaces: &mut Vec<AuthorityBoundaryChangedSurface<'static>>,
	surface: AuthorityBoundarySurface,
	change_summary: &'static str,
	policy_decision: AuthorityBoundaryPolicyDecision,
	legacy_disposition: AuthorityBoundaryDisposition,
) {
	if surfaces.iter().any(|existing| existing.surface == surface) {
		return;
	}

	surfaces.push(AuthorityBoundaryChangedSurface {
		surface,
		change_summary,
		policy_decision,
		legacy_disposition,
	});
}
