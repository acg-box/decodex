use serde_json::Value;

use crate::{
	ATTENTION_RULES, HIGH_VALUE_SURFACES, SURFACE_RULES,
	review_queue::{RecentCommit, bundles::SourceBundle},
};

pub(super) fn subject_from_bundle(
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

pub(super) fn append_commit_sha(subject: &mut Value, sha: &str) {
	let Some(shas) = subject.get_mut("commit_shas").and_then(Value::as_array_mut) else {
		return;
	};

	if !shas.iter().any(|value| value.as_str() == Some(sha)) {
		shas.push(Value::String(sha.to_owned()));
	}
}

pub(crate) fn sort_queue_subjects(mut subjects: Vec<Value>) -> Vec<Value> {
	subjects.sort_by_key(queue_sort_key);

	subjects
}

fn commit_shas(bundle: &SourceBundle, seed_commit: &RecentCommit) -> Vec<String> {
	let shas = bundle.commits.iter().map(|commit| commit.sha.clone()).collect::<Vec<_>>();

	if shas.is_empty() { vec![seed_commit.sha.clone()] } else { shas }
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
