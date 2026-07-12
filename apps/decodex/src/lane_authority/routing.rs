//! Deterministic global ProjectBinding resolution before Lane identity exists.

use super::ProjectBinding;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RoutingPredicateResult {
	Matched,
	TrackerScopeMismatch,
	RoutingLabelMismatch,
	RepositoryMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RoutingCandidate {
	pub(crate) binding: ProjectBinding,
	pub(crate) result: RoutingPredicateResult,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RoutingResolution {
	Selected { binding: ProjectBinding, candidates: Vec<RoutingCandidate> },
	NoMatch { candidates: Vec<RoutingCandidate> },
	Ambiguous { candidates: Vec<RoutingCandidate> },
	InvalidSelector,
}

pub(crate) fn resolve_project_binding(
	bindings: impl IntoIterator<Item = ProjectBinding>,
	tracker_team_id: &str,
	requested_routing_label: &str,
	issue_labels: &[String],
) -> RoutingResolution {
	let mut repository_selectors =
		issue_labels.iter().filter_map(|label| label.strip_prefix("repo:")).collect::<Vec<_>>();
	repository_selectors.sort_unstable();
	repository_selectors.dedup();
	if repository_selectors.iter().any(|selector| selector.is_empty())
		|| repository_selectors.len() > 1
	{
		return RoutingResolution::InvalidSelector;
	}
	let mut routing_selectors = issue_labels
		.iter()
		.filter(|label| label.starts_with("decodex:queued:"))
		.map(String::as_str)
		.collect::<Vec<_>>();
	routing_selectors.sort_unstable();
	routing_selectors.dedup();
	if routing_selectors.len() > 1 {
		return RoutingResolution::InvalidSelector;
	}
	let routing_selector = routing_selectors.first().copied().unwrap_or(requested_routing_label);
	let repository_selector = repository_selectors.first().copied();
	let mut candidates = bindings
		.into_iter()
		.map(|binding| {
			let result = if binding.tracker_team_id() != tracker_team_id {
				RoutingPredicateResult::TrackerScopeMismatch
			} else if binding.routing_label() != routing_selector {
				RoutingPredicateResult::RoutingLabelMismatch
			} else if repository_selector
				.is_some_and(|repository| repository != binding.github_repository())
			{
				RoutingPredicateResult::RepositoryMismatch
			} else {
				RoutingPredicateResult::Matched
			};
			RoutingCandidate { binding, result }
		})
		.collect::<Vec<_>>();
	candidates.sort_by(|left, right| left.binding.project_key().cmp(right.binding.project_key()));
	let matches = candidates
		.iter()
		.filter(|candidate| candidate.result == RoutingPredicateResult::Matched)
		.collect::<Vec<_>>();
	match matches.as_slice() {
		[matched] => RoutingResolution::Selected { binding: matched.binding.clone(), candidates },
		[] => RoutingResolution::NoMatch { candidates },
		_ => RoutingResolution::Ambiguous { candidates },
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn resolver_rejects_pub_1711_wrong_repository_without_selecting_a_lane() {
		let resolution = resolve_project_binding(
			vec![binding("pubfi", "pubfi-mono"), binding("pubfi-insight", "pubfi-insight")],
			"team-pubfi",
			"decodex:queued:pubfi-insight",
			&[String::from("decodex:queued:pubfi-insight"), String::from("repo:pubfi-mono")],
		);
		assert!(matches!(resolution, RoutingResolution::NoMatch { .. }));
	}

	fn binding(project: &str, repository: &str) -> ProjectBinding {
		ProjectBinding::new(
			project,
			"helixbox",
			repository,
			"team-pubfi",
			&format!("decodex:queued:{project}"),
			&format!("binding:{project}"),
		)
		.expect("binding")
	}
}
