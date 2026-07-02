use crate::orchestrator::*;

pub(in crate::orchestrator) struct PullRequestReviewStatePageQuery<'a> {
	pub(in crate::orchestrator) cwd: &'a Path,
	pub(in crate::orchestrator) owner: &'a str,
	pub(in crate::orchestrator) repo: &'a str,
	pub(in crate::orchestrator) number: u64,
	pub(in crate::orchestrator) review_threads_after: Option<&'a str>,
	pub(in crate::orchestrator) pr_url: &'a str,
	pub(in crate::orchestrator) github_token: &'a str,
	pub(in crate::orchestrator) gh_command_path: Option<&'a Path>,
}

pub(in crate::orchestrator) struct PullRequestIssueCommentsPageQuery<'a> {
	pub(in crate::orchestrator) cwd: &'a Path,
	pub(in crate::orchestrator) owner: &'a str,
	pub(in crate::orchestrator) repo: &'a str,
	pub(in crate::orchestrator) number: u64,
	pub(in crate::orchestrator) comments_after: &'a str,
	pub(in crate::orchestrator) pr_url: &'a str,
	pub(in crate::orchestrator) github_token: &'a str,
	pub(in crate::orchestrator) gh_command_path: Option<&'a Path>,
}

pub(in crate::orchestrator) fn query_pull_request_review_state_page(
	query: PullRequestReviewStatePageQuery<'_>,
) -> Result<PullRequestReviewStateRepository> {
	let mut command = github::gh_command_with_config(query.gh_command_path);

	command.args(["api", "graphql", "-f", &format!("query={PULL_REQUEST_REVIEW_STATE_QUERY}")]);
	command.args(["-F", &format!("owner={}", query.owner)]);
	command.args(["-F", &format!("name={}", query.repo)]);
	command.args(["-F", &format!("number={}", query.number)]);

	if let Some(review_threads_after) = query.review_threads_after {
		command.args(["-F", &format!("reviewThreadsAfter={review_threads_after}")]);
	}

	command.current_dir(query.cwd);

	github::configure_gh_command(&mut command, query.github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect pull request review state `{}`: {}",
			query.pr_url,
			stderr.trim()
		);
	}

	let response = serde_json::from_slice::<PullRequestReviewStateResponse>(&output.stdout)?;
	let Some(repository) = response.data.repository else {
		eyre::bail!("GitHub GraphQL response for `{}` did not include a repository.", query.pr_url);
	};

	if repository.pull_request.is_none() {
		eyre::bail!(
			"GitHub GraphQL response for `{}` did not include a pull request.",
			query.pr_url
		);
	}

	Ok(repository)
}

pub(in crate::orchestrator) fn query_pull_request_issue_comments_page(
	query: PullRequestIssueCommentsPageQuery<'_>,
) -> Result<PullRequestIssueCommentsNode> {
	let mut command = github::gh_command_with_config(query.gh_command_path);

	command.args(["api", "graphql", "-f", &format!("query={PULL_REQUEST_ISSUE_COMMENTS_QUERY}")]);
	command.args(["-F", &format!("owner={}", query.owner)]);
	command.args(["-F", &format!("name={}", query.repo)]);
	command.args(["-F", &format!("number={}", query.number)]);
	command.args(["-F", &format!("commentsAfter={}", query.comments_after)]);
	command.current_dir(query.cwd);

	github::configure_gh_command(&mut command, query.github_token);

	let output = command.output()?;

	if !output.status.success() {
		let stderr = String::from_utf8_lossy(&output.stderr);

		eyre::bail!(
			"Failed to inspect pull request issue comments for `{}`: {}",
			query.pr_url,
			stderr.trim()
		);
	}

	let response = serde_json::from_slice::<PullRequestIssueCommentsResponse>(&output.stdout)?;
	let Some(repository) = response.data.repository else {
		eyre::bail!("GitHub GraphQL response for `{}` did not include a repository.", query.pr_url);
	};
	let Some(pull_request) = repository.pull_request else {
		eyre::bail!(
			"GitHub GraphQL response for `{}` did not include a pull request.",
			query.pr_url
		);
	};

	Ok(pull_request)
}

pub(in crate::orchestrator) fn pull_request_review_state_from_page(
	repository: &PullRequestReviewStateRepository,
	pull_request: &PullRequestReviewStateNode,
) -> Result<PullRequestReviewState> {
	Ok(PullRequestReviewState {
		url: pull_request.url.clone(),
		state: pull_request.state.clone(),
		is_draft: pull_request.is_draft,
		review_decision: pull_request.review_decision.clone(),
		merge_commit_allowed: repository.merge_commit_allowed,
		pending_review_requests: pull_request.review_requests.total_count,
		mergeable: pull_request.mergeable.clone(),
		merge_state_status: pull_request.merge_state_status.clone(),
		head_ref_name: pull_request.head_ref_name.clone(),
		head_ref_oid: pull_request.head_ref_oid.clone(),
		merge_commit_oid: pull_request.merge_commit.as_ref().map(|commit| commit.oid.clone()),
		head_repository_name: pull_request
			.head_repository
			.as_ref()
			.map(|repository| repository.name.clone()),
		head_repository_owner: pull_request
			.head_repository_owner
			.as_ref()
			.map(|owner| owner.login.clone()),
		status_check_rollup_state: pull_request_status_check_rollup_state(pull_request),
		unresolved_review_threads: count_unresolved_review_threads(&pull_request.review_threads),
		issue_description_external_review_thumbs_up_count: reaction_group_actor_count(
			&pull_request.reaction_groups,
			"THUMBS_UP",
			EXTERNAL_REVIEW_ACTOR_LOGIN,
		),
		issue_comments: pull_request
			.comments
			.nodes
			.iter()
			.map(issue_comment_state_from_node)
			.collect::<Result<Vec<_>>>()?,
		reviews: pull_request
			.reviews
			.nodes
			.iter()
			.filter_map(review_summary_state_from_node)
			.collect(),
	})
}

pub(in crate::orchestrator) fn merge_pull_request_review_state_page(
	review_state: &mut PullRequestReviewState,
	repository: &PullRequestReviewStateRepository,
	pull_request: &PullRequestReviewStateNode,
) -> Result<Option<String>> {
	if review_state.url != pull_request.url
		|| review_state.state != pull_request.state
		|| review_state.is_draft != pull_request.is_draft
		|| review_state.review_decision != pull_request.review_decision
		|| review_state.merge_commit_allowed != repository.merge_commit_allowed
		|| review_state.pending_review_requests != pull_request.review_requests.total_count
		|| review_state.mergeable != pull_request.mergeable
		|| review_state.merge_state_status != pull_request.merge_state_status
		|| review_state.head_ref_name != pull_request.head_ref_name
		|| review_state.head_ref_oid != pull_request.head_ref_oid
		|| review_state.merge_commit_oid
			!= pull_request.merge_commit.as_ref().map(|commit| commit.oid.clone())
		|| review_state.head_repository_name
			!= pull_request.head_repository.as_ref().map(|repository| repository.name.clone())
		|| review_state.head_repository_owner
			!= pull_request.head_repository_owner.as_ref().map(|owner| owner.login.clone())
		|| review_state.status_check_rollup_state
			!= pull_request_status_check_rollup_state(pull_request)
		|| review_state.issue_description_external_review_thumbs_up_count
			!= reaction_group_actor_count(
				&pull_request.reaction_groups,
				"THUMBS_UP",
				EXTERNAL_REVIEW_ACTOR_LOGIN,
			) || review_state.reviews
		!= pull_request
			.reviews
			.nodes
			.iter()
			.filter_map(review_summary_state_from_node)
			.collect::<Vec<_>>()
	{
		eyre::bail!("Pull request review state changed while paginating `{}`.", review_state.url);
	}

	review_state.unresolved_review_threads +=
		count_unresolved_review_threads(&pull_request.review_threads);

	next_pull_request_review_threads_cursor(pull_request)
}

pub(in crate::orchestrator) fn merge_pull_request_issue_comment_page(
	review_state: &mut PullRequestReviewState,
	pull_request: &PullRequestIssueCommentsNode,
) -> Result<Option<String>> {
	if review_state.url != pull_request.url {
		eyre::bail!(
			"Pull request issue comment state changed while paginating `{}`.",
			review_state.url
		);
	}

	let mut comment_ids = review_state
		.issue_comments
		.iter()
		.map(|comment| comment.database_id)
		.collect::<HashSet<_>>();

	for comment in pull_request.comments.nodes.iter().map(issue_comment_state_from_node) {
		let comment = comment?;

		if !comment_ids.insert(comment.database_id) {
			eyre::bail!(
				"Pull request issue comments repeated while paginating `{}`.",
				review_state.url
			);
		}

		review_state.issue_comments.push(comment);
	}

	next_pull_request_issue_comments_cursor(&pull_request.comments, pull_request.url.as_str())
}

pub(in crate::orchestrator) fn count_unresolved_review_threads(
	review_threads: &PullRequestReviewThreadConnection,
) -> usize {
	review_threads.nodes.iter().filter(|thread| !thread.is_resolved && !thread.is_outdated).count()
}

pub(in crate::orchestrator) fn pull_request_status_check_rollup_state(
	pull_request: &PullRequestReviewStateNode,
) -> Option<String> {
	pull_request
		.commits
		.nodes
		.first()
		.and_then(|node| node.commit.status_check_rollup.as_ref())
		.map(|rollup| rollup.state.clone())
}

pub(in crate::orchestrator) fn issue_comment_state_from_node(
	comment: &PullRequestIssueCommentNode,
) -> Result<PullRequestIssueCommentState> {
	Ok(PullRequestIssueCommentState {
		database_id: comment.database_id,
		author_login: comment.author.as_ref().map(|author| author.login.clone()),
		body: comment.body.clone(),
		created_at_unix_epoch: parse_github_timestamp_to_unix_epoch(&comment.created_at)?,
		external_review_eyes_reaction_count: reaction_group_actor_count(
			&comment.reaction_groups,
			"EYES",
			EXTERNAL_REVIEW_ACTOR_LOGIN,
		),
	})
}

pub(in crate::orchestrator) fn review_summary_state_from_node(
	review: &PullRequestReviewNode,
) -> Option<PullRequestReviewSummaryState> {
	let submitted_at_unix_epoch =
		parse_github_timestamp_to_unix_epoch(review.submitted_at.as_deref()?).ok()?;

	Some(PullRequestReviewSummaryState {
		author_login: review.author.as_ref().map(|author| author.login.clone()),
		body: review.body.clone(),
		state: review.state.clone(),
		submitted_at_unix_epoch,
	})
}

pub(in crate::orchestrator) fn reaction_group_actor_count(
	groups: &[PullRequestReactionGroup],
	content: &str,
	actor_login: &str,
) -> usize {
	groups.iter().find(|group| group.content == content).map_or(0, |group| {
		group
			.users
			.nodes
			.iter()
			.filter(|actor| actor.login.eq_ignore_ascii_case(actor_login))
			.count()
	})
}

pub(in crate::orchestrator) fn parse_github_timestamp_to_unix_epoch(
	timestamp: &str,
) -> Result<i64> {
	Ok(OffsetDateTime::parse(timestamp, &Rfc3339)
		.map_err(|error| eyre::eyre!("Failed to parse GitHub timestamp `{timestamp}`: {error}"))?
		.unix_timestamp())
}

pub(in crate::orchestrator) fn next_pull_request_review_threads_cursor(
	pull_request: &PullRequestReviewStateNode,
) -> Result<Option<String>> {
	if !pull_request.review_threads.page_info.has_next_page {
		return Ok(None);
	}

	pull_request.review_threads.page_info.end_cursor.clone().map(Some).ok_or_else(|| {
		eyre::eyre!(
			"GitHub GraphQL response for `{}` reported additional review thread pages without an end cursor.",
			pull_request.url
		)
	})
}

pub(in crate::orchestrator) fn next_pull_request_issue_comments_cursor(
	comments: &PullRequestIssueCommentConnection,
	pr_url: &str,
) -> Result<Option<String>> {
	if !comments.page_info.has_next_page {
		return Ok(None);
	}

	comments.page_info.end_cursor.clone().map(Some).ok_or_else(|| {
		eyre::eyre!(
			"GitHub GraphQL response for `{pr_url}` reported additional issue comment pages without an end cursor.",
		)
	})
}

pub(in crate::orchestrator) fn format_run_once_summary(
	summary: &RunSummary,
	dry_run: bool,
) -> String {
	if dry_run {
		return format!(
			"dry run: project={} issue={} branch={} worktree={} attempt={}",
			summary.project_id,
			summary.issue_identifier,
			summary.branch_name,
			summary.worktree_path.display(),
			summary.attempt_number
		);
	}
	if summary.continuation_pending {
		return format!(
			"run paused at continuation boundary: project={} issue={} run_id={} worktree={} next_action=rerun_or_use_daemon",
			summary.project_id,
			summary.issue_identifier,
			summary.run_id,
			summary.worktree_path.display()
		);
	}

	format!(
		"run complete: project={} issue={} run_id={} worktree={}",
		summary.project_id,
		summary.issue_identifier,
		summary.run_id,
		summary.worktree_path.display()
	)
}
