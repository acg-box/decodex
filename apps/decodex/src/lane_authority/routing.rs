//! Deterministic global ProjectBinding resolution before Lane identity exists.

use super::ProjectBinding;
use sha2::{Digest as _, Sha256};

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
impl RoutingResolution {
	pub(crate) fn quarantine(
		&self,
		tracker_issue_id: &str,
		selector_fingerprint: &str,
	) -> Option<RoutingQuarantine> {
		let (reason, candidates): (_, &[_]) = match self {
			Self::NoMatch { candidates } => (RoutingQuarantineReason::NoMatch, candidates),
			Self::Ambiguous { candidates } => (RoutingQuarantineReason::Ambiguous, candidates),
			Self::InvalidSelector => (RoutingQuarantineReason::InvalidSelector, &[]),
			Self::Selected { .. } => return None,
		};
		Some(RoutingQuarantine::new(tracker_issue_id, reason, selector_fingerprint, candidates))
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RoutingQuarantineReason {
	NoMatch,
	Ambiguous,
	InvalidSelector,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub(crate) struct RoutingQuarantine {
	pub(crate) tracker_issue_id: String,
	pub(crate) epoch: u64,
	pub(crate) reason: RoutingQuarantineReason,
	pub(crate) selector_fingerprint: String,
	pub(crate) candidate_fingerprints: Vec<String>,
}
impl RoutingQuarantine {
	fn new(
		tracker_issue_id: &str,
		reason: RoutingQuarantineReason,
		selector_fingerprint: &str,
		candidates: &[RoutingCandidate],
	) -> Self {
		let candidate_fingerprints = candidates
			.iter()
			.map(|candidate| {
				let mut digest = Sha256::new();
				digest.update(candidate.binding.project_key().as_bytes());
				digest.update(candidate.binding.config_fingerprint().as_bytes());
				digest.update(format!("{:?}", candidate.result).as_bytes());
				digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
			})
			.collect();
		Self {
			tracker_issue_id: tracker_issue_id.to_owned(),
			epoch: 1,
			reason,
			selector_fingerprint: selector_fingerprint.to_owned(),
			candidate_fingerprints,
		}
	}
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

	#[test]
	fn lane_authority_v2_c2_adm_02() {
		let temp = tempfile::tempdir().expect("tempdir");
		let store =
			crate::state::StateStore::open(temp.path().join("runtime.sqlite3")).expect("store");
		let cases = [
			(
				"issue-zero",
				resolve_project_binding(Vec::new(), "team-pubfi", "decodex:queued:pubfi", &[]),
			),
			(
				"issue-multiple",
				resolve_project_binding(
					vec![binding_with_route("one", "one"), binding_with_route("two", "two")],
					"team-pubfi",
					"decodex:queued:shared",
					&[],
				),
			),
		];
		for (issue, resolution) in cases {
			assert!(matches!(
				resolution,
				RoutingResolution::NoMatch { .. } | RoutingResolution::Ambiguous { .. }
			));
			let quarantine = resolution.quarantine(issue, &"b".repeat(64)).expect("quarantine");
			store.record_routing_quarantine(quarantine.clone()).expect("persist");
			assert_eq!(store.routing_quarantine(issue).expect("read"), Some(quarantine));
		}
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

	fn binding_with_route(project: &str, repository: &str) -> ProjectBinding {
		ProjectBinding::new(
			project,
			"helixbox",
			repository,
			"team-pubfi",
			"decodex:queued:shared",
			&format!("binding:{project}"),
		)
		.expect("binding")
	}
}
