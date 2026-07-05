use std::path::Path;

use rusqlite::Connection;

use crate::state::{
	ReviewHandoffMarker, ReviewOrchestrationMarker, ReviewPolicyCheckpointInput, StateStore,
};

const DROPPED_REVIEW_MARKER_TABLES_FIXTURE: &str = r#"
CREATE TABLE review_handoffs (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	branch_name TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	pr_url TEXT NOT NULL,
	target_base_ref_name TEXT,
	pr_head_ref_name TEXT NOT NULL,
	pr_head_oid TEXT NOT NULL,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, branch_name)
);
CREATE TABLE review_orchestrations (
	project_id TEXT NOT NULL,
	issue_id TEXT NOT NULL,
	branch_name TEXT NOT NULL,
	run_id TEXT NOT NULL,
	attempt_number INTEGER NOT NULL,
	pr_url TEXT NOT NULL,
	head_sha TEXT NOT NULL,
	phase TEXT NOT NULL,
	request_comment_database_id INTEGER,
	request_created_at_unix_epoch INTEGER,
	request_description_thumbs_up_count INTEGER,
	request_retry_count INTEGER NOT NULL,
	external_round_count INTEGER NOT NULL,
	auto_merge_enabled_at_unix_epoch INTEGER,
	updated_at TEXT NOT NULL,
	updated_at_unix INTEGER NOT NULL,
	PRIMARY KEY (project_id, issue_id, branch_name, run_id, attempt_number)
);
INSERT INTO review_handoffs (
	project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
	target_base_ref_name, pr_head_ref_name, pr_head_oid, updated_at, updated_at_unix
) VALUES (
	'pubfi', 'PUB-101', 'x/decodex-pub-101', 'run-1', 2,
	'https://github.com/hack-ink/decodex/pull/101', 'main', 'x/decodex-pub-101',
	'08a20f7dfb9526e7421a5f095b1c6adec84e52d6', '2026-06-17T01:00:00Z',
	1771290000
);
INSERT INTO review_orchestrations (
	project_id, issue_id, branch_name, run_id, attempt_number, pr_url, head_sha,
	phase, request_comment_database_id, request_created_at_unix_epoch,
	request_description_thumbs_up_count, request_retry_count, external_round_count,
	auto_merge_enabled_at_unix_epoch, updated_at, updated_at_unix
) VALUES (
	'pubfi', 'PUB-101', 'x/decodex-pub-101', 'run-1', 2,
	'https://github.com/hack-ink/decodex/pull/101',
	'19b20f7dfb9526e7421a5f095b1c6adec84e52d7', 'waiting_for_ack', 1234,
	1771290030, 4, 1, 3, 1771290060, '2026-06-17T01:01:00Z', 1771290060
);
INSERT INTO review_orchestrations (
	project_id, issue_id, branch_name, run_id, attempt_number, pr_url, head_sha,
	phase, request_comment_database_id, request_created_at_unix_epoch,
	request_description_thumbs_up_count, request_retry_count, external_round_count,
	auto_merge_enabled_at_unix_epoch, updated_at, updated_at_unix
) VALUES (
	'pubfi', 'PUB-202', 'x/decodex-pub-202', 'run-2', 1,
	'https://github.com/hack-ink/decodex/pull/202',
	'28c20f7dfb9526e7421a5f095b1c6adec84e52d8', 'request_pending', NULL,
	NULL, NULL, 0, 1, NULL, '2026-06-17T01:02:00Z', 1771290120
);
INSERT INTO review_handoffs (
	project_id, issue_id, branch_name, run_id, attempt_number, pr_url,
	target_base_ref_name, pr_head_ref_name, pr_head_oid, updated_at, updated_at_unix
) VALUES (
	'pubfi', 'PUB-303', 'x/decodex-pub-303', 'run-2', 1,
	'https://github.com/hack-ink/decodex/pull/303', 'main', 'x/decodex-pub-303',
	'38c20f7dfb9526e7421a5f095b1c6adec84e52d8', '2026-06-17T01:03:00Z',
	1771290180
);
INSERT INTO review_orchestrations (
	project_id, issue_id, branch_name, run_id, attempt_number, pr_url, head_sha,
	phase, request_comment_database_id, request_created_at_unix_epoch,
	request_description_thumbs_up_count, request_retry_count, external_round_count,
	auto_merge_enabled_at_unix_epoch, updated_at, updated_at_unix
) VALUES (
	'pubfi', 'PUB-303', 'x/decodex-pub-303', 'run-1', 1,
	'https://github.com/hack-ink/decodex/pull/303',
	'39c20f7dfb9526e7421a5f095b1c6adec84e52d9', 'waiting_for_ack', 4321,
	1771290240, 5, 2, 4, 1771290300, '2026-06-17T01:04:00Z', 1771290240
);
"#;
pub(crate) fn sample_pub_101_review_handoff() -> ReviewHandoffMarker {
	ReviewHandoffMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"main",
		"x/decodex-pub-101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
	)
}

pub(crate) fn sample_pub_101_review_orchestration() -> ReviewOrchestrationMarker {
	ReviewOrchestrationMarker::new(
		"run-1",
		1,
		"x/decodex-pub-101",
		"https://github.com/hack-ink/decodex/pull/101",
		"08a20f7dfb9526e7421a5f095b1c6adec84e52d6",
		"request_pending",
		None,
		None,
		None,
		0,
		0,
		None,
	)
}
pub(crate) fn seed_dropped_review_marker_tables(state_path: &Path) {
	let connection = Connection::open(state_path).expect("fixture db should open");

	connection
		.execute_batch(DROPPED_REVIEW_MARKER_TABLES_FIXTURE)
		.expect("dropped review marker tables should seed");
}

pub(crate) fn upsert_handoff_review_policy_checkpoint(
	store: &StateStore,
	issue_id: &str,
	run_id: &str,
	status: &str,
	head_sha: &str,
	nonclean_rounds: i64,
) {
	store
		.upsert_review_policy_checkpoint(ReviewPolicyCheckpointInput {
			project_id: "pubfi",
			issue_id,
			run_id,
			attempt_number: 1,
			phase: "handoff",
			review_level: "standard",
			status,
			head_sha,
			nonclean_rounds,
			details_json: "{}",
		})
		.expect("review policy checkpoint should persist");
}
