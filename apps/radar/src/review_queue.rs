//! Upstream review queue source discovery and subject classification.

use super::{
	ATTENTION_RULES, BTreeMap, GitHubApi, HIGH_VALUE_SURFACES, HashSet, Path, RadarLedger,
	RadarRefreshQueueRequest, SURFACE_RULES, UPSTREAM_REVIEW_QUEUE_SCHEMA, Value,
	absolute_repo_path, extract_commit_sha_from_url, extract_pr_number_from_url, eyre, first_line,
	ledger_path, load_json, optional_value_string, percent_encode, repo_default_branch,
	required_value_string, required_value_u64, serde_json, sorted_json_files, string_array,
	truncate_patch_excerpt, utc_now_iso, validate_signal_file,
};

pub(super) struct RecentCommit {
	pub(super) sha: String,
	pub(super) title: String,
	pub(super) url: String,
	pub(super) committed_at: Option<String>,
}

#[derive(Clone, Debug)]
struct BundleFile {
	path: String,
	patch_excerpt: Option<String>,
}

#[derive(Clone, Debug)]
struct BundleCommit {
	sha: String,
	message: String,
}

#[derive(Clone, Debug)]
struct BundlePr {
	number: u64,
	title: String,
	body: String,
	state: String,
	url: String,
}

#[derive(Clone, Debug)]
struct SourceBundle {
	primary_pr: Option<BundlePr>,
	commits: Vec<BundleCommit>,
	files: Vec<BundleFile>,
}

#[derive(Debug)]
pub(super) struct QueueBuild {
	pub(super) queue: Value,
	pub(super) ledger_enabled: bool,
}

pub(super) fn build_review_queue(
	request: &RadarRefreshQueueRequest,
	root: &Path,
	api: &GitHubApi,
) -> crate::prelude::Result<QueueBuild> {
	let (default_branch, commits) = recent_commits(api, &request.repo, request.search_limit)?;
	let recent_commits_scanned = commits.len();
	let (published_prs, published_shas) =
		published_subjects(&absolute_repo_path(root, &request.signals_dir))?;
	let ledger_path = ledger_path(root, request);
	let mut ledger = ledger_path.as_deref().map(RadarLedger::open).transpose()?;
	let mut subjects = BTreeMap::<(String, String), Value>::new();
	let mut published_seen = 0_usize;

	for commit in commits {
		let pr_number = maybe_promote_commit_to_pr(api, &request.repo, &commit.sha)?;
		let subject_kind = if pr_number.is_some() { "pr" } else { "commit" };
		let subject_id = pr_number.map_or_else(|| commit.sha.clone(), |number| number.to_string());

		if let Some(ledger) = &mut ledger {
			ledger.record_commit(&request.repo, &commit, pr_number)?;
		}

		if published_shas.contains(&commit.sha)
			|| pr_number.is_some_and(|number| published_prs.contains(&number))
		{
			published_seen += 1;

			if let Some(ledger) = &mut ledger {
				ledger.record_review(
					&request.repo,
					subject_kind,
					&subject_id,
					"signal",
					"Already present in published signal collection.",
					Some("confirmed"),
				)?;
			}

			continue;
		}

		let key = (subject_kind.to_owned(), subject_id.clone());

		if let Some(current) = subjects.get_mut(&key) {
			append_commit_sha(current, &commit.sha);

			continue;
		}

		let bundle = match pr_number {
			Some(number) => build_pr_bundle(api, &request.repo, number)?,
			None => build_commit_bundle(api, &request.repo, &commit.sha)?,
		};

		subjects.insert(key, subject_from_bundle(&bundle, subject_kind, &subject_id, &commit));

		if let Some(ledger) = &mut ledger {
			ledger.record_review(
				&request.repo,
				subject_kind,
				&subject_id,
				"watch",
				"Queued for AI upstream review by deterministic Radar sync.",
				Some("likely"),
			)?;
		}
	}

	if let Some(ledger) = &mut ledger {
		ledger.commit()?;
	}

	let ordered_subjects = sort_queue_subjects(subjects.into_values().collect());
	let queue = review_queue_payload(
		request,
		&default_branch,
		recent_commits_scanned,
		published_seen,
		ordered_subjects,
	)?;

	Ok(QueueBuild { queue, ledger_enabled: !request.no_ledger })
}

fn review_queue_payload(
	request: &RadarRefreshQueueRequest,
	default_branch: &str,
	recent_commits_scanned: usize,
	published_seen: usize,
	subjects: Vec<Value>,
) -> crate::prelude::Result<Value> {
	let critical = count_priority(&subjects, "critical");
	let high = count_priority(&subjects, "high");
	let normal = count_priority(&subjects, "normal");
	let low = count_priority(&subjects, "low");

	Ok(serde_json::json!({
		"schema": UPSTREAM_REVIEW_QUEUE_SCHEMA,
		"repo": request.repo,
		"generated_at": utc_now_iso()?,
		"source": {
			"default_branch": default_branch,
			"search_limit": request.search_limit,
			"signals_dir": request.signals_dir.to_string_lossy(),
		},
		"subjects": subjects,
		"counts": {
			"recent_commits_scanned": recent_commits_scanned,
			"published_subjects_seen": published_seen,
			"subjects_queued": critical + high + normal + low,
			"critical": critical,
			"high": high,
			"normal": normal,
			"low": low,
		},
	}))
}

fn count_priority(subjects: &[Value], priority: &str) -> usize {
	subjects
		.iter()
		.filter(|subject| {
			subject
				.get("review_priority")
				.and_then(Value::as_str)
				.is_some_and(|value| value == priority)
		})
		.count()
}

fn recent_commits(
	api: &GitHubApi,
	repo: &str,
	search_limit: usize,
) -> crate::prelude::Result<(String, Vec<RecentCommit>)> {
	let default_branch = repo_default_branch(api, repo)?;
	let url = format!(
		"https://api.github.com/repos/{repo}/commits?sha={}&per_page={search_limit}",
		percent_encode(&default_branch)
	);
	let payload = api.get(&url)?.payload;
	let Some(items) = payload.as_array() else {
		eyre::bail!("Expected commits list payload from GitHub API");
	};
	let commits = items.iter().filter_map(recent_commit_from_value).collect::<Vec<_>>();

	Ok((default_branch, commits))
}

fn recent_commit_from_value(item: &Value) -> Option<RecentCommit> {
	let commit = item.get("commit")?.as_object()?;
	let sha = item.get("sha")?.as_str()?.to_owned();
	let url = item.get("html_url")?.as_str()?.to_owned();
	let message = commit.get("message")?.as_str()?;

	if message.is_empty() {
		return None;
	}

	Some(RecentCommit {
		sha,
		title: first_line(message),
		url,
		committed_at: commit
			.get("committer")
			.and_then(Value::as_object)
			.and_then(|committer| committer.get("date"))
			.and_then(Value::as_str)
			.map(str::to_owned),
	})
}

fn published_subjects(
	signals_dir: &Path,
) -> crate::prelude::Result<(HashSet<u64>, HashSet<String>)> {
	let mut published_prs = HashSet::new();
	let mut published_shas = HashSet::new();

	for path in sorted_json_files(signals_dir)? {
		let payload = load_json(&path)?;

		validate_signal_file(&path, &payload)?;

		if let Some(pr_number) = payload
			.get("source_refs")
			.and_then(|refs| refs.get("pr_url"))
			.and_then(Value::as_str)
			.and_then(extract_pr_number_from_url)
		{
			published_prs.insert(pr_number);
		}

		for url in string_array(payload.pointer("/source_refs/commit_urls")) {
			if let Some(sha) = extract_commit_sha_from_url(&url) {
				published_shas.insert(sha);
			}
		}
	}

	Ok((published_prs, published_shas))
}

fn maybe_promote_commit_to_pr(
	api: &GitHubApi,
	repo: &str,
	commit_sha: &str,
) -> crate::prelude::Result<Option<u64>> {
	let url = format!("https://api.github.com/repos/{repo}/commits/{commit_sha}/pulls");
	let pulls = match api.get_paginated(&url) {
		Ok(pulls) => pulls,
		Err(_) => return Ok(None),
	};

	Ok(pulls.first().and_then(|first| first.get("number")).and_then(Value::as_u64))
}

fn build_pr_bundle(
	api: &GitHubApi,
	repo: &str,
	pr_number: u64,
) -> crate::prelude::Result<SourceBundle> {
	let pr = api.get(&format!("https://api.github.com/repos/{repo}/pulls/{pr_number}"))?.payload;
	let commits = api.get_paginated(&format!(
		"https://api.github.com/repos/{repo}/pulls/{pr_number}/commits?per_page=100"
	))?;
	let files = api.get_paginated(&format!(
		"https://api.github.com/repos/{repo}/pulls/{pr_number}/files?per_page=100"
	))?;

	Ok(SourceBundle {
		primary_pr: Some(BundlePr {
			number: required_value_u64(&pr, "number")?,
			title: required_value_string(&pr, "title")?,
			body: optional_value_string(&pr, "body").unwrap_or_default(),
			state: if optional_value_string(&pr, "merged_at").is_some() {
				"merged".to_owned()
			} else {
				required_value_string(&pr, "state")?
			},
			url: required_value_string(&pr, "html_url")?,
		}),
		commits: commits.iter().filter_map(bundle_commit_from_pr_commit).collect(),
		files: files.iter().filter_map(bundle_file_from_value).collect(),
	})
}

fn build_commit_bundle(
	api: &GitHubApi,
	repo: &str,
	commit_sha: &str,
) -> crate::prelude::Result<SourceBundle> {
	let commit =
		api.get(&format!("https://api.github.com/repos/{repo}/commits/{commit_sha}"))?.payload;
	let files = commit.get("files").and_then(Value::as_array).cloned().unwrap_or_default();
	let message = commit.pointer("/commit/message").and_then(Value::as_str).unwrap_or_default();

	Ok(SourceBundle {
		primary_pr: None,
		commits: vec![BundleCommit {
			sha: required_value_string(&commit, "sha")?,
			message: first_line(message),
		}],
		files: files.iter().filter_map(bundle_file_from_value).collect(),
	})
}

fn bundle_commit_from_pr_commit(item: &Value) -> Option<BundleCommit> {
	Some(BundleCommit {
		sha: item.get("sha")?.as_str()?.to_owned(),
		message: first_line(item.pointer("/commit/message")?.as_str()?),
	})
}

fn bundle_file_from_value(item: &Value) -> Option<BundleFile> {
	Some(BundleFile {
		path: item.get("filename")?.as_str()?.to_owned(),
		patch_excerpt: item.get("patch").and_then(Value::as_str).map(truncate_patch_excerpt),
	})
}

fn subject_from_bundle(
	bundle: &SourceBundle,
	subject_kind: &str,
	subject_id: &str,
	seed_commit: &RecentCommit,
) -> Value {
	let surface_hints = detect_surface_hints(bundle);
	let attention_flags = detect_attention_flags(bundle);
	let mut subject = serde_json::json!({
		"subject_kind": subject_kind,
		"subject_id": subject_id,
		"title": seed_commit.title.clone(),
		"url": seed_commit.url.clone(),
		"source_state": "commit_only",
		"commit_shas": commit_shas(bundle, seed_commit),
		"committed_at": seed_commit.committed_at.clone(),
		"changed_file_count": bundle.files.len(),
		"sample_paths": bundle.files.iter().take(12).map(|file| file.path.clone()).collect::<Vec<_>>(),
		"surface_hints": surface_hints,
		"attention_flags": attention_flags,
		"review_priority": priority_for(&surface_hints, &attention_flags),
		"review_reason": review_reason(&surface_hints, &attention_flags),
		"next_step": "ai_review_required",
	});

	if let Some(primary_pr) = &bundle.primary_pr
		&& let Some(subject) = subject.as_object_mut()
	{
		subject.insert("title".to_owned(), Value::String(primary_pr.title.clone()));
		subject.insert("url".to_owned(), Value::String(primary_pr.url.clone()));
		subject.insert("source_state".to_owned(), Value::String(primary_pr.state.clone()));
		subject.insert("pr_number".to_owned(), Value::from(primary_pr.number));
		subject.insert("pr_url".to_owned(), Value::String(primary_pr.url.clone()));
	}

	subject
}

fn commit_shas(bundle: &SourceBundle, seed_commit: &RecentCommit) -> Vec<String> {
	let shas = bundle.commits.iter().map(|commit| commit.sha.clone()).collect::<Vec<_>>();

	if shas.is_empty() { vec![seed_commit.sha.clone()] } else { shas }
}

fn append_commit_sha(subject: &mut Value, sha: &str) {
	let Some(shas) = subject.get_mut("commit_shas").and_then(Value::as_array_mut) else {
		return;
	};

	if !shas.iter().any(|value| value.as_str() == Some(sha)) {
		shas.push(Value::String(sha.to_owned()));
	}
}

fn sort_queue_subjects(mut subjects: Vec<Value>) -> Vec<Value> {
	subjects.sort_by_key(queue_sort_key);

	subjects
}

fn queue_sort_key(subject: &Value) -> (u8, String, String, String) {
	(
		match subject.get("review_priority").and_then(Value::as_str) {
			Some("critical") => 0,
			Some("high") => 1,
			Some("normal") => 2,
			Some("low") => 3,
			_ => 9,
		},
		subject.get("committed_at").and_then(Value::as_str).unwrap_or_default().to_owned(),
		subject.get("subject_kind").and_then(Value::as_str).unwrap_or_default().to_owned(),
		subject.get("subject_id").and_then(Value::as_str).unwrap_or_default().to_owned(),
	)
}

fn detect_surface_hints(bundle: &SourceBundle) -> Vec<String> {
	let haystack =
		bundle.files.iter().map(|file| file.path.to_lowercase()).collect::<Vec<_>>().join("\n");
	let mut hints = SURFACE_RULES
		.iter()
		.filter(|(_, terms)| terms.iter().any(|term| haystack.contains(term)))
		.map(|(surface, _)| (*surface).to_owned())
		.collect::<Vec<_>>();

	if hints.is_empty() {
		hints.push("internal_churn".to_owned());
	}

	hints.sort();

	hints
}

fn detect_attention_flags(bundle: &SourceBundle) -> Vec<String> {
	let haystack = text_blob(bundle);
	let mut flags = ATTENTION_RULES
		.iter()
		.filter(|(_, terms)| terms.iter().any(|term| haystack.contains(term)))
		.map(|(flag, _)| (*flag).to_owned())
		.collect::<Vec<_>>();

	flags.sort();

	flags
}

fn text_blob(bundle: &SourceBundle) -> String {
	let mut parts = Vec::new();

	if let Some(primary_pr) = &bundle.primary_pr {
		parts.push(primary_pr.title.clone());
		parts.push(primary_pr.body.clone());
	}

	parts.extend(bundle.commits.iter().map(|commit| commit.message.clone()));
	parts.extend(
		bundle
			.files
			.iter()
			.flat_map(|file| [file.path.clone(), file.patch_excerpt.clone().unwrap_or_default()]),
	);

	parts.join("\n").to_lowercase()
}

fn priority_for(surface_hints: &[String], attention_flags: &[String]) -> &'static str {
	let has_high_surface =
		surface_hints.iter().any(|surface| HIGH_VALUE_SURFACES.contains(&surface.as_str()));
	let breaking_or_removed = attention_flags
		.iter()
		.any(|flag| matches!(flag.as_str(), "breaking_change" | "deprecated_removed"));

	if breaking_or_removed && has_high_surface {
		"critical"
	} else if has_high_surface {
		"high"
	} else if attention_flags.iter().any(|flag| {
		matches!(flag.as_str(), "new_feature" | "protocol_change" | "release_packaging")
	}) {
		"normal"
	} else {
		"low"
	}
}

fn review_reason(surface_hints: &[String], attention_flags: &[String]) -> String {
	if surface_hints.iter().any(|hint| hint == "internal_churn") && attention_flags.is_empty() {
		return "Needs AI review because every recent upstream commit is tracked, but deterministic hints found only internal churn.".to_owned();
	}
	if !attention_flags.is_empty() {
		return format!("Needs AI review for {}.", attention_flags.join(", "));
	}

	format!("Needs AI review for surface hints: {}.", surface_hints.join(", "))
}
