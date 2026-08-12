use std::{
	cell::{Cell, RefCell},
	collections::VecDeque,
};

use super::*;

struct FakeProvider {
	pr_begin: RefCell<Result<GitHubSnapshot, GitHubReadFailure>>,
	pr_end: RefCell<Result<GitHubSnapshot, GitHubReadFailure>>,
	pr_pages:
		RefCell<VecDeque<Result<GitHubPage<GitHubPullRequestObservation>, GitHubReadFailure>>>,
	pr_mutation: Cell<GitHubPullRequestMutationOutcome>,
	pr_mutations: Cell<usize>,
	check_begin: RefCell<Result<GitHubSnapshot, GitHubReadFailure>>,
	check_end: RefCell<Result<GitHubSnapshot, GitHubReadFailure>>,
	check_pages: RefCell<VecDeque<Result<GitHubPage<GitHubCheckObservation>, GitHubReadFailure>>>,
	check_mutation: Cell<GitHubCheckMutationOutcome>,
	check_mutations: Cell<usize>,
}

impl FakeProvider {
	fn new() -> Self {
		Self {
			pr_begin: RefCell::new(Ok(snapshot("snapshot-1"))),
			pr_end: RefCell::new(Ok(snapshot("snapshot-1"))),
			pr_pages: RefCell::new(VecDeque::new()),
			pr_mutation: Cell::new(GitHubPullRequestMutationOutcome::DefinitelyNotSent),
			pr_mutations: Cell::new(0),
			check_begin: RefCell::new(Ok(snapshot("snapshot-1"))),
			check_end: RefCell::new(Ok(snapshot("snapshot-1"))),
			check_pages: RefCell::new(VecDeque::new()),
			check_mutation: Cell::new(GitHubCheckMutationOutcome::DefinitelyNotSent),
			check_mutations: Cell::new(0),
		}
	}
}

impl GitHubEffectProvider for FakeProvider {
	fn begin_pull_request_snapshot(
		&self,
		_authority: &GitHubPullRequestAuthority,
	) -> Result<GitHubSnapshot, GitHubReadFailure> {
		self.pr_begin.borrow().clone()
	}

	fn pull_request_page(
		&self,
		_authority: &GitHubPullRequestAuthority,
		_snapshot: &GitHubSnapshot,
		_cursor: Option<&GitHubCursor>,
	) -> Result<GitHubPage<GitHubPullRequestObservation>, GitHubReadFailure> {
		self.pr_pages.borrow_mut().pop_front().expect("pull-request fixture has the requested page")
	}

	fn end_pull_request_snapshot(
		&self,
		_authority: &GitHubPullRequestAuthority,
		_start: &GitHubSnapshot,
	) -> Result<GitHubSnapshot, GitHubReadFailure> {
		self.pr_end.borrow().clone()
	}

	fn apply_pull_request(
		&self,
		mutation: GitHubPullRequestMutation<'_>,
	) -> GitHubPullRequestMutationOutcome {
		assert_eq!(mutation.authority().repository.owner(), "acg-box");
		assert_eq!(mutation.title(), "Acceptance pull request");
		self.pr_mutations.set(self.pr_mutations.get() + 1);
		self.pr_mutation.get()
	}

	fn begin_check_snapshot(
		&self,
		_authority: &GitHubCheckAuthority,
	) -> Result<GitHubSnapshot, GitHubReadFailure> {
		self.check_begin.borrow().clone()
	}

	fn check_page(
		&self,
		_authority: &GitHubCheckAuthority,
		_snapshot: &GitHubSnapshot,
		_cursor: Option<&GitHubCursor>,
	) -> Result<GitHubPage<GitHubCheckObservation>, GitHubReadFailure> {
		self.check_pages.borrow_mut().pop_front().expect("check fixture has the requested page")
	}

	fn end_check_snapshot(
		&self,
		_authority: &GitHubCheckAuthority,
		_start: &GitHubSnapshot,
	) -> Result<GitHubSnapshot, GitHubReadFailure> {
		self.check_end.borrow().clone()
	}

	fn apply_check(&self, mutation: GitHubCheckMutation<'_>) -> GitHubCheckMutationOutcome {
		assert_eq!(mutation.authority().repository.name(), "decodex");
		assert_eq!(mutation.name(), "build");
		self.check_mutations.set(self.check_mutations.get() + 1);
		self.check_mutation.get()
	}
}

fn snapshot(value: &str) -> GitHubSnapshot {
	GitHubSnapshot::new(value).expect("snapshot is canonical")
}

fn binding() -> GitHubRepositoryBinding {
	GitHubRepositoryBinding::new(
		GitHubProviderIdentity::GitHubDotCom,
		GitHubRepositoryId::new(1).expect("repository ID is positive"),
		GitHubRepositoryOwner::new("acg-box").expect("owner is canonical"),
		GitHubRepositoryName::new("decodex").expect("repository name is canonical"),
		GitHubInstallationId::new(2).expect("installation ID is positive"),
		GitHubAccountId::new(3).expect("account ID is positive"),
	)
}

fn revisions() -> GitHubRevisionAuthority {
	GitHubRevisionAuthority::new(
		GitHubBranchName::new("main").expect("base branch is canonical"),
		GitHubRevision::new("1111111111111111111111111111111111111111")
			.expect("base revision is canonical"),
		GitHubBranchName::new("xv/xy-1353-vnext-integration").expect("head branch is canonical"),
		GitHubRevision::new("2222222222222222222222222222222222222222")
			.expect("head revision is canonical"),
	)
	.expect("revision authority is distinct")
}

fn marker(suffix: u8) -> GitHubOperationMarker {
	GitHubOperationMarker::new(format!(
		"decodex/github-effect/1/33000000-0000-4000-8000-{suffix:012}"
	))
	.expect("marker is canonical")
}

fn pull_request_identity() -> GitHubPullRequestIdentity {
	GitHubPullRequestIdentity::new(
		GitHubPullRequestId::new(41).expect("pull-request ID is positive"),
		17,
	)
	.expect("pull-request identity is canonical")
}

fn pull_request_authority() -> GitHubPullRequestAuthority {
	GitHubPullRequestAuthority::new(
		binding(),
		revisions(),
		GitHubPullRequestTarget::Unassigned,
		marker(1),
	)
}

fn pull_request_spec() -> GitHubPullRequestSpec {
	GitHubPullRequestSpec::new(
		"Acceptance pull request",
		"Durable marker and exact revision fixture.",
		false,
	)
	.expect("pull-request spec is canonical")
}

fn pagination(
	page_number: u16,
	requested: Option<&str>,
	next: Option<&str>,
) -> GitHubPaginationMetadata {
	GitHubPaginationMetadata::new(
		snapshot("snapshot-1"),
		GitHubPageIdentity::new(format!("page-{page_number}")).expect("page identity is canonical"),
		page_number,
		requested.map(|value| GitHubCursor::new(value).expect("cursor is canonical")),
		next.is_some(),
		next.map(|value| GitHubCursor::new(value).expect("cursor is canonical")),
	)
	.expect("pagination is canonical")
}

fn pr_page(
	page_number: u16,
	requested: Option<&str>,
	next: Option<&str>,
	objects: Vec<GitHubPullRequestObservation>,
) -> GitHubPage<GitHubPullRequestObservation> {
	GitHubPage::new(binding(), pagination(page_number, requested, next), objects)
		.expect("pull-request page is canonical")
}

fn check_page(
	page_number: u16,
	requested: Option<&str>,
	next: Option<&str>,
	objects: Vec<GitHubCheckObservation>,
) -> GitHubPage<GitHubCheckObservation> {
	GitHubPage::new(binding(), pagination(page_number, requested, next), objects)
		.expect("check page is canonical")
}

fn pull_request_observation(spec: GitHubPullRequestSpec) -> GitHubPullRequestObservation {
	GitHubPullRequestObservation::new(
		pull_request_identity(),
		revisions(),
		GitHubProviderField::Visible(Some(marker(1))),
		GitHubPullRequestState::Open,
		GitHubProviderField::Visible(spec),
	)
}

fn completed_state() -> GitHubCheckState {
	GitHubCheckState::new(GitHubCheckStatus::Completed, Some(GitHubCheckConclusion::Success))
		.expect("completed state has a conclusion")
}

fn check_observation(
	name: &str,
	run: u64,
	marker_value: Option<GitHubOperationMarker>,
) -> GitHubCheckObservation {
	GitHubCheckObservation::new(
		GitHubCheckIdentity::new(
			GitHubCheckSuiteId::new(50).expect("suite ID is positive"),
			GitHubCheckRunId::new(run).expect("run ID is positive"),
		),
		pull_request_identity(),
		revisions(),
		GitHubProviderField::Visible(marker_value),
		GitHubProviderField::Visible(
			GitHubCheckSpec::new(name, completed_state()).expect("check spec is canonical"),
		),
	)
}

#[test]
fn explicit_identity_and_public_text_contracts_reject_ambient_or_secret_inference() {
	assert!(GitHubRepositoryOwner::new("Acg-Box").is_err());
	assert!(GitHubBranchName::new("../main").is_err());
	assert!(GitHubRevision::new("HEAD").is_err());
	assert!(GitHubOperationMarker::new("marker-from-cwd").is_err());
	assert!(GitHubPullRequestSpec::new("Bearer abcdefghijklmnop", "safe", false).is_err());
	assert!(GitHubCheckState::new(GitHubCheckStatus::Completed, None).is_err());
	assert!(
		GitHubCheckSuiteContract::new(vec![
			GitHubRequiredCheckRun::new("build", None).expect("run is canonical"),
			GitHubRequiredCheckRun::new("build", None).expect("run is canonical"),
		])
		.is_err()
	);
	let authority = pull_request_authority();
	assert_eq!(authority.repository.provider(), GitHubProviderIdentity::GitHubDotCom);
	assert_eq!(authority.repository.repository_id().get(), 1);
	assert_eq!(authority.revisions.head_revision(), "2222222222222222222222222222222222222222");
}

#[test]
fn lost_pull_request_response_reconciles_from_a_complete_multi_page_snapshot() {
	let provider = FakeProvider::new();
	provider.pr_pages.borrow_mut().push_back(Ok(pr_page(1, None, None, Vec::new())));
	provider.pr_mutation.set(GitHubPullRequestMutationOutcome::LostResponse);
	let dispatch = reconcile_pull_request_dispatch(
		&provider,
		GitHubPullRequestDispatchReceipt {
			authority: pull_request_authority(),
			spec: pull_request_spec(),
		},
	);
	let GitHubPullRequestDispatchResolution::ReadbackRequired(continuation) = dispatch else {
		panic!("lost response did not require readback");
	};
	assert_eq!(provider.pr_mutations.get(), 1);

	provider.pr_pages.borrow_mut().extend([
		Ok(pr_page(1, None, Some("cursor-2"), Vec::new())),
		Ok(pr_page(2, Some("cursor-2"), None, vec![pull_request_observation(pull_request_spec())])),
	]);
	let readback = reconcile_pull_request_readback(&provider, continuation);
	let GitHubPullRequestReadbackResolution::Completed(completion) = readback else {
		panic!("complete readback did not reconcile");
	};
	assert_eq!(completion.observation().identity, pull_request_identity());
	assert_eq!(completion.summary(), GitHubObservationSummary { pages: 2, objects: 1 });
	assert_eq!(provider.pr_mutations.get(), 1);
}

#[test]
fn absent_stale_external_and_provider_fault_outcomes_never_become_success() {
	let provider = FakeProvider::new();
	provider.pr_pages.borrow_mut().push_back(Ok(pr_page(1, None, None, Vec::new())));
	let absent = reconcile_pull_request_readback(
		&provider,
		GitHubPullRequestContinuation {
			authority: pull_request_authority(),
			spec: pull_request_spec(),
			response_identity: None,
			reason: GitHubReadbackReason::LostResponse,
		},
	);
	assert!(matches!(
		absent,
		GitHubPullRequestReadbackResolution::NoEffect(GitHubNoEffect {
			reason: GitHubNoEffectReason::CompletelyObservedAbsent,
			..
		})
	));

	let mut stale_revisions = revisions();
	stale_revisions.head_revision = GitHubRevision::new("3333333333333333333333333333333333333333")
		.expect("stale revision is canonical");
	provider.pr_pages.borrow_mut().push_back(Ok(pr_page(
		1,
		None,
		None,
		vec![GitHubPullRequestObservation::new(
			pull_request_identity(),
			stale_revisions,
			GitHubProviderField::Visible(Some(marker(1))),
			GitHubPullRequestState::Open,
			GitHubProviderField::Visible(pull_request_spec()),
		)],
	)));
	let stale = reconcile_pull_request_readback(
		&provider,
		GitHubPullRequestContinuation {
			authority: pull_request_authority(),
			spec: pull_request_spec(),
			response_identity: None,
			reason: GitHubReadbackReason::DuplicateOrAlreadyExists,
		},
	);
	assert!(matches!(
		stale,
		GitHubPullRequestReadbackResolution::Stale(GitHubStale {
			reason: GitHubStaleReason::HeadRevisionChanged,
			..
		})
	));

	provider.pr_pages.borrow_mut().push_back(Ok(pr_page(
		1,
		None,
		None,
		vec![pull_request_observation(
			GitHubPullRequestSpec::new("Externally changed", "Changed body", false)
				.expect("changed spec is canonical"),
		)],
	)));
	let changed = reconcile_pull_request_readback(
		&provider,
		GitHubPullRequestContinuation {
			authority: pull_request_authority(),
			spec: pull_request_spec(),
			response_identity: None,
			reason: GitHubReadbackReason::DuplicateOrAlreadyExists,
		},
	);
	assert!(matches!(
		changed,
		GitHubPullRequestReadbackResolution::Ambiguous(GitHubTerminalAmbiguity {
			reason: GitHubAmbiguity::ExternallyChangedFields,
			..
		})
	));

	*provider.pr_begin.borrow_mut() = Err(GitHubReadFailure::Unauthorized);
	let fault = reconcile_pull_request_dispatch(
		&provider,
		GitHubPullRequestDispatchReceipt {
			authority: pull_request_authority(),
			spec: pull_request_spec(),
		},
	);
	assert!(matches!(
		fault,
		GitHubPullRequestDispatchResolution::Ambiguous(GitHubTerminalAmbiguity {
			reason: GitHubAmbiguity::Unauthorized,
			..
		})
	));
}

#[test]
fn pagination_detects_cursor_snapshot_page_and_duplicate_drift() {
	let repository = binding();
	let start = snapshot("snapshot-1");
	let cases = [
		(
			vec![
				GitHubPage::new(
					repository.clone(),
					GitHubPaginationMetadata::new(
						start.clone(),
						GitHubPageIdentity::new("page-1").expect("page identity is canonical"),
						1,
						None,
						true,
						None,
					)
					.expect("metadata is representable"),
					Vec::<u64>::new(),
				)
				.expect("page is canonical"),
			],
			GitHubAmbiguity::MissingNextCursor,
		),
		(
			vec![
				GitHubPage::new(
					repository.clone(),
					GitHubPaginationMetadata::new(
						snapshot("snapshot-other"),
						GitHubPageIdentity::new("page-1").expect("page identity is canonical"),
						1,
						None,
						false,
						None,
					)
					.expect("metadata is representable"),
					Vec::<u64>::new(),
				)
				.expect("page is canonical"),
			],
			GitHubAmbiguity::PageSnapshotChanged,
		),
		(
			vec![
				GitHubPage::new(
					repository.clone(),
					GitHubPaginationMetadata::new(
						start.clone(),
						GitHubPageIdentity::new("page-2").expect("page identity is canonical"),
						2,
						None,
						false,
						None,
					)
					.expect("metadata is representable"),
					Vec::<u64>::new(),
				)
				.expect("page is canonical"),
			],
			GitHubAmbiguity::PageCycle,
		),
		(
			vec![
				GitHubPage::new(repository.clone(), pagination(1, None, None), vec![7_u64, 7_u64])
					.expect("page is canonical"),
			],
			GitHubAmbiguity::DuplicateObjectIdentity,
		),
	];

	for (pages, expected) in cases {
		let mut pages = VecDeque::from(pages);
		let result = collect_pages(
			&repository,
			&start,
			|_| Ok(pages.pop_front().expect("fixture page is available")),
			|value| (*value, None),
		);
		assert!(matches!(result, Err(CollectionFailure::Ambiguous(reason)) if reason == expected));
	}
}

#[test]
fn check_completion_requires_the_complete_exact_suite_inventory() {
	let provider = FakeProvider::new();
	let desired_identity = GitHubCheckIdentity::new(
		GitHubCheckSuiteId::new(50).expect("suite ID is positive"),
		GitHubCheckRunId::new(51).expect("run ID is positive"),
	);
	let authority = GitHubCheckAuthority::new(
		binding(),
		revisions(),
		pull_request_identity(),
		GitHubCheckTarget::Exact(desired_identity),
		marker(2),
	);
	let spec = GitHubCheckSpec::new("build", completed_state()).expect("spec is canonical");
	let suite = GitHubCheckSuiteContract::new(vec![
		GitHubRequiredCheckRun::new(
			"build",
			Some(GitHubCheckRunId::new(51).expect("run ID is positive")),
		)
		.expect("required run is canonical"),
		GitHubRequiredCheckRun::new(
			"test",
			Some(GitHubCheckRunId::new(52).expect("run ID is positive")),
		)
		.expect("required run is canonical"),
	])
	.expect("suite contract is canonical");
	provider.check_pages.borrow_mut().extend([
		Ok(check_page(
			1,
			None,
			Some("cursor-2"),
			vec![check_observation("build", 51, Some(marker(2)))],
		)),
		Ok(check_page(2, Some("cursor-2"), None, vec![check_observation("test", 52, None)])),
	]);
	let resolution = reconcile_check_dispatch(
		&provider,
		GitHubCheckDispatchReceipt {
			authority: authority.clone(),
			spec: spec.clone(),
			suite_contract: suite.clone(),
		},
	);
	let GitHubCheckDispatchResolution::Completed(completion) = resolution else {
		panic!("complete suite did not reconcile");
	};
	assert_eq!(completion.required_runs().len(), 2);
	assert_eq!(completion.summary(), GitHubObservationSummary { pages: 2, objects: 2 });
	assert_eq!(provider.check_mutations.get(), 0);

	provider.check_pages.borrow_mut().push_back(Ok(check_page(
		1,
		None,
		None,
		vec![check_observation("build", 51, Some(marker(2)))],
	)));
	let incomplete = reconcile_check_dispatch(
		&provider,
		GitHubCheckDispatchReceipt { authority, spec, suite_contract: suite },
	);
	assert!(matches!(
		incomplete,
		GitHubCheckDispatchResolution::Ambiguous(GitHubTerminalAmbiguity {
			reason: GitHubAmbiguity::IncompleteChecks,
			..
		})
	));
}
