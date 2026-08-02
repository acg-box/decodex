//! Cross-file validation for durable social outcomes.

use std::{
	collections::{BTreeMap, BTreeSet, btree_map::Entry},
	path::{Path, PathBuf},
};

use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
	SOCIAL_OUTCOME_SCHEMA, SOCIAL_POST_SCHEMA,
	prelude::{Result, eyre},
};

struct PublishedPostBinding {
	canonical_url: String,
	posted_at: OffsetDateTime,
	publication_lineage_sha256: String,
	publisher: String,
	slug: String,
	target_account: String,
	xurl_app: String,
	xurl_version: String,
}

pub(crate) fn validated_observed_windows(
	root: &Path,
	outcomes_dir: &Path,
	posts_dir: &Path,
) -> Result<BTreeSet<(String, String)>> {
	let outcomes_dir = crate::resolve_against(root, outcomes_dir);
	let posts_dir = crate::resolve_against(root, posts_dir);
	let available_posts = crate::collect_json_files(std::slice::from_ref(&posts_dir))?
		.into_iter()
		.collect::<BTreeSet<_>>();
	let outcome_paths = crate::collect_json_files(&[outcomes_dir])?;
	let mut post_bindings = BTreeMap::new();
	let mut observed = BTreeSet::new();

	for outcome_path in outcome_paths {
		let result =
			validate_outcome(root, &posts_dir, &available_posts, &mut post_bindings, &outcome_path);
		let key = result
			.map_err(|error| eyre::eyre!("{}: {error}", crate::path_arg(root, &outcome_path)))?;
		if !observed.insert(key.clone()) {
			return Err(eyre::eyre!(
				"{}: duplicate social outcome for post {:?} and window {:?}",
				crate::path_arg(root, &outcome_path),
				key.0,
				key.1
			));
		}
	}

	Ok(observed)
}

fn validate_outcome(
	root: &Path,
	posts_dir: &Path,
	available_posts: &BTreeSet<PathBuf>,
	post_bindings: &mut BTreeMap<PathBuf, PublishedPostBinding>,
	outcome_path: &Path,
) -> Result<(String, String)> {
	let outcome = crate::load_json(outcome_path)?;
	crate::validate_generated_social_artifact(&outcome)
		.map_err(|error| eyre::eyre!("social outcome failed validation: {error}"))?;
	if outcome.get("schema").and_then(Value::as_str) != Some(SOCIAL_OUTCOME_SCHEMA) {
		return Err(eyre::eyre!("outcomes directory contains a non-outcome artifact"));
	}

	let post_ref = required_string(&outcome, "/social_post_ref", "social_post_ref")?;
	let requested_post_path = crate::resolve_against(root, Path::new(post_ref));
	crate::require_contained_regular_file(&requested_post_path, posts_dir)
		.map_err(|error| eyre::eyre!("social_post_ref is invalid: {error}"))?;
	let post_path = available_posts
		.get(&requested_post_path)
		.ok_or_else(|| eyre::eyre!("social_post_ref is not a configured JSON post"))?;
	let canonical_ref = crate::path_arg(root, post_path);
	if post_ref != canonical_ref {
		return Err(eyre::eyre!("social_post_ref must equal canonical post ref {canonical_ref:?}"));
	}

	let post = match post_bindings.entry(post_path.clone()) {
		Entry::Occupied(entry) => entry.into_mut(),
		Entry::Vacant(entry) => entry.insert(load_published_post(post_path)?),
	};
	let window = validate_outcome_binding(&outcome, post)?;

	Ok((canonical_ref, window.to_owned()))
}

fn load_published_post(path: &Path) -> Result<PublishedPostBinding> {
	let post = crate::load_json(path)?;
	crate::validate_generated_social_artifact(&post)
		.map_err(|error| eyre::eyre!("referenced social post failed validation: {error}"))?;
	if post.get("schema").and_then(Value::as_str) != Some(SOCIAL_POST_SCHEMA)
		|| post.get("status").and_then(Value::as_str) != Some("published")
	{
		return Err(eyre::eyre!("social outcome must reference a published social_post"));
	}
	crate::social_evidence::validate_source_evidence(&post).map_err(|error| {
		eyre::eyre!("referenced social post evidence failed validation: {error}")
	})?;

	let slug = required_string(&post, "/slug", "social post slug")?;
	let target_account = required_string(&post, "/target_account", "social post target_account")?;
	let post_id = required_string(&post, "/publication/post_id", "publication.post_id")?;
	let published_url =
		required_string(&post, "/publication/published_urls/0", "publication.published_urls[0]")?;
	let canonical_url = format!("https://x.com/{target_account}/status/{post_id}");
	if published_url != canonical_url {
		return Err(eyre::eyre!("published social post URL does not match its post ID"));
	}
	let publication_lineage_sha256 = required_string(
		&post,
		"/publication/publication_lineage_sha256",
		"publication.publication_lineage_sha256",
	)?;
	let idempotency_key =
		required_string(&post, "/decision/idempotency_key", "decision.idempotency_key")?;
	if idempotency_key != format!("content-publication:{publication_lineage_sha256}") {
		return Err(eyre::eyre!(
			"published social post idempotency key does not match its publication lineage"
		));
	}
	let verified_account =
		required_string(&post, "/publication/verified_account", "publication.verified_account")?;
	if verified_account != target_account {
		return Err(eyre::eyre!(
			"published social post verified account does not match its target account"
		));
	}
	let posted_at = parse_timestamp(
		required_string(&post, "/publication/posted_at", "publication.posted_at")?,
		"publication.posted_at",
	)?;

	Ok(PublishedPostBinding {
		canonical_url,
		posted_at,
		publication_lineage_sha256: publication_lineage_sha256.to_owned(),
		publisher: required_string(&post, "/publication/publisher", "publication.publisher")?
			.to_owned(),
		slug: slug.to_owned(),
		target_account: target_account.to_owned(),
		xurl_app: required_string(&post, "/publication/xurl_app", "publication.xurl_app")?
			.to_owned(),
		xurl_version: required_string(
			&post,
			"/publication/xurl_version",
			"publication.xurl_version",
		)?
		.to_owned(),
	})
}

fn validate_outcome_binding<'a>(
	outcome: &'a Value,
	post: &PublishedPostBinding,
) -> Result<&'a str> {
	let window = required_string(outcome, "/window", "window")?;
	let expected_slug = format!("{}-{window}", post.slug);
	if required_string(outcome, "/slug", "slug")? != expected_slug {
		return Err(eyre::eyre!("social outcome slug does not match its post and window"));
	}
	if required_string(outcome, "/target_account", "target_account")? != post.target_account
		|| required_string(
			outcome,
			"/observation/verified_account",
			"observation.verified_account",
		)? != post.target_account
	{
		return Err(eyre::eyre!("social outcome account does not match its published post"));
	}
	if required_string(outcome, "/published_url", "published_url")? != post.canonical_url {
		return Err(eyre::eyre!("social outcome published_url does not match its published post"));
	}
	if required_string(
		outcome,
		"/observation/publication_lineage_sha256",
		"observation.publication_lineage_sha256",
	)? != post.publication_lineage_sha256
	{
		return Err(eyre::eyre!("social outcome publication lineage does not match its post"));
	}
	if required_string(outcome, "/observation/xurl_version", "observation.xurl_version")?
		!= post.xurl_version
		|| required_string(outcome, "/observation/xurl_app", "observation.xurl_app")?
			!= post.xurl_app
		|| required_string(outcome, "/observation/reader", "observation.reader")? != post.publisher
	{
		return Err(eyre::eyre!("social outcome xurl identity does not match its post"));
	}

	let observed_at =
		parse_timestamp(required_string(outcome, "/observed_at", "observed_at")?, "observed_at")?;
	let minimum_hours = match window {
		"24h" => 23,
		"7d" => 167,
		_ => return Err(eyre::eyre!("social outcome window must be 24h or 7d")),
	};
	let elapsed_hours = (observed_at - post.posted_at).whole_hours();
	if elapsed_hours < minimum_hours {
		return Err(eyre::eyre!(
			"{window} social outcome is before its earliest window: elapsed_hours={elapsed_hours}"
		));
	}

	Ok(window)
}

fn required_string<'a>(value: &'a Value, pointer: &str, label: &str) -> Result<&'a str> {
	value
		.pointer(pointer)
		.and_then(Value::as_str)
		.filter(|value| !value.trim().is_empty())
		.ok_or_else(|| eyre::eyre!("{label} is required"))
}

fn parse_timestamp(value: &str, label: &str) -> Result<OffsetDateTime> {
	OffsetDateTime::parse(value, &Rfc3339)
		.map_err(|_| eyre::eyre!("{label} must be an RFC3339 timestamp"))
}

#[cfg(test)]
mod tests {
	use std::{
		fs,
		os::unix::fs::symlink,
		path::{Path, PathBuf},
	};

	use serde_json::{Value, json};

	use super::validated_observed_windows;
	use crate::{SocialObserveDueRequest, repo_local_test_directory};

	const POST_TEXT: &str = "Codex app-server exposes a typed capability check before experimental calls, so operators can detect unsupported protocol surfaces before a workflow starts.";
	const RUN_ID: &str = "019fa400-0000-7000-8000-000000000001";
	const SECOND_RUN_ID: &str = "019fa400-0000-7000-8000-000000000002";

	struct Store {
		root: std::path::PathBuf,
		posts: std::path::PathBuf,
		outcomes: std::path::PathBuf,
	}

	impl Store {
		fn direct(root: &Path) -> Self {
			Self {
				root: root.to_path_buf(),
				posts: root.join("posts"),
				outcomes: root.join("outcomes"),
			}
		}

		fn default(root: &Path) -> Self {
			Self {
				root: root.to_path_buf(),
				posts: root.join(crate::DEFAULT_SOCIAL_POSTS_DIR),
				outcomes: root.join(crate::DEFAULT_SOCIAL_OUTCOMES_DIR),
			}
		}
	}

	#[test]
	fn valid_outcome_is_accepted_by_default_store_validation() {
		let temp = repo_local_test_directory("publisher-outcome-valid-");
		let store = Store::default(temp.path());
		let post = write_post(&store, RUN_ID, "post-a", "1001", &"a".repeat(64));
		let post_ref = crate::path_arg(&store.root, &post);
		write_outcome(&store, RUN_ID, &valid_outcome("post-a", &post_ref, "1001", &"a".repeat(64)));

		let report = crate::validate_social_at(&store.root, &[]).expect("valid social store");
		assert_eq!(report.checked_files, 2);
		let observed = validated_observed_windows(&store.root, &store.outcomes, &store.posts)
			.expect("validated observed windows");
		assert!(observed.contains(&(post_ref, "24h".into())));
	}

	#[test]
	fn copied_outcome_retargeted_to_another_post_fails_validation_and_observed_windows() {
		let temp = repo_local_test_directory("publisher-outcome-copied-");
		let store = Store::default(temp.path());
		let post_a = write_post(&store, RUN_ID, "post-a", "1001", &"a".repeat(64));
		let post_b = write_post(&store, SECOND_RUN_ID, "post-b", "2002", &"b".repeat(64));
		let mut copied = valid_outcome(
			"post-b",
			&crate::path_arg(&store.root, &post_b),
			"2002",
			&"b".repeat(64),
		);
		copied["social_post_ref"] = json!(crate::path_arg(&store.root, &post_a));
		write_outcome(&store, RUN_ID, &copied);

		let validation_error = crate::validate_social_at(&store.root, &[])
			.expect_err("copied outcome must fail default validation")
			.to_string();
		assert!(validation_error.contains("does not match"), "{validation_error}");
		let observed_error = validated_observed_windows(&store.root, &store.outcomes, &store.posts)
			.expect_err("copied outcome must not produce an observed window")
			.to_string();
		assert!(observed_error.contains("does not match"), "{observed_error}");
	}

	#[test]
	fn observe_due_rejects_a_tampered_outcome_before_any_xurl_attempt() {
		let temp = repo_local_test_directory("publisher-outcome-observe-due-");
		let store = Store::direct(temp.path());
		let post_a = write_post(&store, RUN_ID, "post-a", "1001", &"a".repeat(64));
		let post_b = write_post(&store, SECOND_RUN_ID, "post-b", "2002", &"b".repeat(64));
		let mut copied = valid_outcome(
			"post-b",
			&crate::path_arg(&crate::repo_root().expect("repo root"), &post_b),
			"2002",
			&"b".repeat(64),
		);
		copied["social_post_ref"] =
			json!(crate::path_arg(&crate::repo_root().expect("repo root"), &post_a));
		write_outcome(&store, RUN_ID, &copied);

		let error = crate::social_workflow::observe_due_with_test_binary(
			&SocialObserveDueRequest {
				run_id: SECOND_RUN_ID.into(),
				observed_at: "2026-08-03T12:02:00Z".into(),
			},
			temp.path(),
			&temp.path().join("xurl-must-not-run"),
		)
		.expect_err("tampered outcome must stop observe-due")
		.to_string();
		assert!(error.contains("does not match"), "{error}");
		assert!(!temp.path().join("attempts").exists());
	}

	#[test]
	fn mismatched_url_lineage_and_timing_are_rejected() {
		for case in ["url", "lineage", "timing"] {
			let temp = repo_local_test_directory("publisher-outcome-mismatch-");
			let store = Store::direct(temp.path());
			let lineage = "a".repeat(64);
			let post = write_post(&store, RUN_ID, "post-a", "1001", &lineage);
			let post_ref = crate::path_arg(&store.root, &post);
			let mut outcome = valid_outcome("post-a", &post_ref, "1001", &lineage);
			match case {
				"url" => outcome["published_url"] = json!("https://x.com/decodexspace/status/9999"),
				"lineage" =>
					outcome["observation"]["publication_lineage_sha256"] = json!("b".repeat(64)),
				"timing" => outcome["observed_at"] = json!("2026-07-28T10:01:00Z"),
				_ => unreachable!(),
			}
			write_outcome(&store, RUN_ID, &outcome);

			let error = validated_observed_windows(&store.root, &store.outcomes, &store.posts)
				.expect_err("mismatched outcome must fail")
				.to_string();
			assert!(
				error.contains("does not match") || error.contains("earliest window"),
				"{case}: {error}"
			);
		}
	}

	#[test]
	fn path_traversal_and_symlink_post_refs_are_rejected() {
		let traversal = repo_local_test_directory("publisher-outcome-traversal-");
		let traversal_store = Store::direct(traversal.path());
		let lineage = "a".repeat(64);
		write_post(&traversal_store, RUN_ID, "post-a", "1001", &lineage);
		let traversal_ref = "posts/../posts/019fa400-0000-7000-8000-000000000001.json";
		write_outcome(
			&traversal_store,
			RUN_ID,
			&valid_outcome("post-a", traversal_ref, "1001", &lineage),
		);
		assert!(
			validated_observed_windows(
				&traversal_store.root,
				&traversal_store.outcomes,
				&traversal_store.posts,
			)
			.is_err()
		);

		let linked = repo_local_test_directory("publisher-outcome-symlink-");
		let linked_store = Store::direct(linked.path());
		let target = write_post(&linked_store, RUN_ID, "post-a", "1001", &lineage);
		let link = linked_store.posts.join("linked.json");
		symlink(&target, &link).expect("post symlink fixture");
		write_outcome(
			&linked_store,
			RUN_ID,
			&valid_outcome("post-a", &crate::path_arg(&linked_store.root, &link), "1001", &lineage),
		);
		assert!(
			validated_observed_windows(
				&linked_store.root,
				&linked_store.outcomes,
				&linked_store.posts,
			)
			.is_err()
		);
		fs::remove_file(link).expect("remove post symlink fixture");
	}

	fn write_post(
		store: &Store,
		run_id: &str,
		slug: &str,
		post_id: &str,
		lineage: &str,
	) -> PathBuf {
		let path = store.posts.join(format!("{run_id}.json"));
		crate::write_new_json(&path, &valid_post(run_id, slug, post_id, lineage))
			.expect("published post fixture");
		path
	}

	fn write_outcome(store: &Store, run_id: &str, outcome: &Value) {
		crate::write_new_json(&store.outcomes.join(format!("{run_id}.json")), outcome)
			.expect("social outcome fixture");
	}

	fn valid_post(run_id: &str, slug: &str, post_id: &str, lineage: &str) -> Value {
		json!({
			"schema": "social_post/v1",
			"slug": slug,
			"channel": "x",
			"target_account": "decodexspace",
			"owner": {"automation_id": "decodex-xurl-publisher", "run_id": run_id},
			"mode": "operator_impact",
			"status": "published",
			"audience": "Codex operators",
			"text": [POST_TEXT],
			"source_refs": {
				"reservations": ["reservations/source.json"],
				"social_candidates": ["candidates/source.json"],
				"urls": ["https://github.com/openai/codex/pull/22414"]
			},
			"evidence_digests": {},
			"evidence_notes": ["The source documents an operator-visible protocol change."],
			"claims": [{
				"text": "The app-server exposes a typed capability check.",
				"evidence": "https://github.com/openai/codex/pull/22414",
				"confidence": "confirmed"
			}],
			"decision": {
				"worthiness": "publish",
				"priority": "high",
				"idempotency_key": format!("content-publication:{lineage}"),
				"reason": "The change alters an operator-visible protocol workflow.",
				"daily_limit": 1,
				"daily_count_before": 0,
				"daily_count_after": 1,
				"day": "2026-07-27",
				"timezone": "UTC"
			},
			"publication": {
				"posted_at": "2026-07-27T12:02:00Z",
				"published_urls": [format!("https://x.com/decodexspace/status/{post_id}")],
				"post_id": post_id,
				"publisher": "xurl",
				"xurl_version": "1.3.1",
				"xurl_app": "default",
				"verified_account": "decodexspace",
				"verified_user_id": "42",
				"account_verified": true,
				"made_with_ai": true,
				"identity_response_sha256": "d".repeat(64),
				"create_response_sha256": "a".repeat(64),
				"read_response_sha256": "b".repeat(64),
				"publication_lineage_sha256": lineage,
				"recorded_cost_ceiling_microusd": 30000
			}
		})
	}

	fn valid_outcome(slug: &str, post_ref: &str, post_id: &str, lineage: &str) -> Value {
		json!({
			"schema": "social_outcome/v1",
			"slug": format!("{slug}-24h"),
			"target_account": "decodexspace",
			"owner": {"automation_id": "decodex-xurl-publisher", "run_id": RUN_ID},
			"social_post_ref": post_ref,
			"published_url": format!("https://x.com/decodexspace/status/{post_id}"),
			"observed_at": "2026-07-28T12:02:00Z",
			"window": "24h",
			"metrics": {"views": 125, "likes": 4, "replies": 1, "reposts": 2},
			"observation": {
				"reader": "xurl",
				"xurl_version": "1.3.1",
				"xurl_app": "default",
				"verified_account": "decodexspace",
				"publication_lineage_sha256": lineage,
				"response_sha256": "c".repeat(64),
				"recorded_cost_ceiling_microusd": 5000
			},
			"notes": ["Metrics were read through the bounded xurl post lookup."]
		})
	}
}
