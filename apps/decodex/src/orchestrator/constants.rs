use std::time::Duration;
pub(crate) const DEFAULT_STATUS_RUN_LIMIT: usize = 10;
pub(crate) const DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT: usize = 25;
pub(crate) const DEFAULT_OPERATOR_LISTEN_ADDRESS: &str = "127.0.0.1:8192";
pub(crate) const EXTERNAL_REVIEW_ACTOR_LOGIN: &str = "codex";
pub(crate) const EXTERNAL_REVIEW_REQUEST_BODY: &str = "@codex review";
pub(crate) const EXTERNAL_REVIEW_PASS_PHRASE: &str = "Didn't find any major issues.";
pub(crate) const EXTERNAL_REVIEW_ACK_TIMEOUT_SECS: i64 = 60;
pub(crate) const EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS: i64 = 15 * 60;
pub(crate) const CONTINUATION_RETRY_DELAY_MS: u64 = 1_000;
pub(crate) const FAILURE_RETRY_BASE_DELAY_MS: u64 = 10_000;
pub(crate) const RECOVERABLE_WORKTREE_SKIP_TTL: Duration = Duration::from_secs(10 * 60);
pub(crate) const CONTINUATION_PENDING_RUN_STATUS: &str = "continuation_pending";
pub(crate) const TERMINAL_GUARDED_RUN_STATUS: &str = "terminal_guarded";
pub(crate) const TERMINAL_GUARD_MARKER_FILE: &str = ".decodex-terminal-guarded";
pub(crate) const TRACKER_RATE_LIMIT_BACKOFF_SECS: u64 = 15 * 60;
pub(crate) const TRACKER_RATE_LIMIT_WARNING: &str = "tracker_rate_limited";
pub(crate) const TRACKER_TRANSIENT_TIMEOUT_BACKOFF_SECS: u64 = 60;
pub(crate) const TRACKER_TRANSIENT_TIMEOUT_WARNING: &str = "tracker_transient_timeout";
pub(crate) const OPERATOR_DASHBOARD_ENDPOINT_PATH: &str = "/";
pub(crate) const OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH: &str = "/dashboard";
pub(crate) const OPERATOR_DASHBOARD_WS_ENDPOINT_PATH: &str = "/dashboard/control";
pub(crate) const OPERATOR_LIVE_ENDPOINT_PATH: &str = "/livez";
pub(crate) const OPERATOR_ACCOUNTS_ENDPOINT_PATH: &str = "/api/accounts";
pub(crate) const OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH: &str = "/api/operator-snapshot";
pub(crate) const OPERATOR_LINEAR_SCAN_ENDPOINT_PATH: &str = "/api/linear-scan";
pub(crate) const OPERATOR_LANE_INSPECT_ENDPOINT_PATH: &str = "/api/lane/inspect";
pub(crate) const OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH: &str = "/api/lane/interrupt";
pub(crate) const OPERATOR_LANE_STEER_ENDPOINT_PATH: &str = "/api/lane/steer";
pub(crate) const OPERATOR_LANE_STEER_ALIAS_ENDPOINT_PATH: &str = "/api/lane-steer";
pub(crate) const OPERATOR_STATE_MAX_REQUEST_BYTES: usize = 256 * 1_024;
pub(crate) const OPERATOR_DASHBOARD_WS_CLIENT_MESSAGE_MAX_BYTES: usize = 64 * 1_024;
pub(crate) const OPERATOR_STATE_HEADER_TERMINATOR: &[u8] = b"\r\n\r\n";
pub(crate) const STATUS_OPERATOR_SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(60);
pub(crate) const STATUS_OPERATOR_SNAPSHOT_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
pub(crate) const STATUS_OPERATOR_SNAPSHOT_IO_TIMEOUT: Duration = Duration::from_millis(500);
pub(crate) const STATUS_OPERATOR_SNAPSHOT_WARNING: &str = "status_cached_snapshot_unavailable";
pub(crate) const OPERATOR_DASHBOARD_WS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
pub(crate) const OPERATOR_RUN_ACTIVITY_STREAM_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const OPERATOR_DEV_SNAPSHOT_STREAM_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const DEFAULT_CONTROL_PLANE_POLL_INTERVAL: Duration = Duration::from_secs(15);
pub(crate) const LINEAR_CONTROL_PLANE_POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);
pub(crate) const PULL_REQUEST_REVIEW_STATE_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!, $reviewThreadsAfter: String) {
  repository(owner: $owner, name: $name) {
    mergeCommitAllowed
    pullRequest(number: $number) {
      url
      state
      isDraft
      reviewDecision
      baseRefOid
      mergeable
      mergeStateStatus
      headRefName
      headRefOid
      mergeCommit {
        oid
      }
      headRepository {
        name
      }
      headRepositoryOwner {
        login
      }
      reactionGroups {
        content
        users(first: 100) {
          totalCount
          nodes {
            login
          }
        }
      }
      comments(first: 100) {
        nodes {
          databaseId
          body
          createdAt
          author {
            login
          }
          reactionGroups {
            content
            users(first: 100) {
              totalCount
              nodes {
                login
              }
            }
          }
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
      reviews(last: 100) {
        nodes {
          body
          state
          submittedAt
          author {
            login
          }
        }
      }
      reviewRequests(first: 1) {
        totalCount
      }
      reviewThreads(first: 100, after: $reviewThreadsAfter) {
        nodes {
          isResolved
          isOutdated
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
      commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              state
            }
          }
        }
      }
    }
  }
}
"#;
pub(crate) const PULL_REQUEST_ISSUE_COMMENTS_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!, $commentsAfter: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      url
      comments(first: 100, after: $commentsAfter) {
        nodes {
          databaseId
          body
          createdAt
          author {
            login
          }
          reactionGroups {
            content
            users(first: 100) {
              totalCount
              nodes {
                login
              }
            }
          }
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
  }
}
"#;
