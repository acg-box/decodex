#[cfg(unix)] use std::os::fd::AsRawFd;
use std::{
	cmp::Ordering,
	collections::{HashMap, HashSet},
	env,
	error::Error,
	fmt::{self, Display, Formatter},
	fs::{self, File, OpenOptions},
	io::{ErrorKind, Read, Write},
	net::{SocketAddr, TcpListener, TcpStream},
	path::{Path, PathBuf},
	process::{self, Child, Command, ExitStatus, Stdio},
	slice,
	sync::{
		Arc, Mutex,
		mpsc::{self, Receiver, RecvTimeoutError, Sender},
	},
	thread::{self, JoinHandle},
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use color_eyre::Report;
use libc::pid_t;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{agent, default_branch_sync, git_credentials, maintenance, state};
#[rustfmt::skip]
use crate::{agent::{ACTIVE_RUN_IDLE_TIMEOUT, AppServerCapabilityPreflightFailure, AppServerDynamicToolFailure, AppServerHomePreflightFailure, AppServerProcessEnv, AppServerRunRequest, AppServerRunResult, AppServerTransportFailure, AppServerTurnFailure, ISSUE_DELIVERY_CLOSEOUT_COMPLETE_TOOL_NAME, ISSUE_LABEL_ADD_TOOL_NAME, ISSUE_PROGRESS_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_CHECKPOINT_TOOL_NAME, ISSUE_REVIEW_HANDOFF_TOOL_NAME, ISSUE_REVIEW_REPAIR_COMPLETE_TOOL_NAME, ISSUE_TERMINAL_FINALIZE_TOOL_NAME, ISSUE_TRANSITION_TOOL_NAME, DecodexRunContext, DecodexToolBridge, ReviewExecutionMode, ReviewHandoffContext, ReviewHandoffWritebackFailed, ReviewPolicyStopReason, ReviewPolicyStopRequested, RunCompletionDisposition, TrackerToolBridge, TurnContinuationGuard}, config::{InternalReviewMode, ServiceConfig}, git_credentials::GitCredentialSource, github, prelude::{Result, eyre}, state::{ChildAgentActivityBucket, ChildAgentActivitySummary, CodexAccountActivitySummary, ProjectRegistration, ProjectRunStatus, ProtocolActivitySummary, RUN_OPERATION_AGENT_RUN, RUN_OPERATION_APP_SERVER_PREFLIGHT, RUN_OPERATION_GIT_CREDENTIALS, RUN_OPERATION_IDLE, RUN_OPERATION_RECONCILIATION, RUN_OPERATION_REPO_GATE, RUN_OPERATION_REVIEW_WRITEBACK, RUN_OPERATION_WAITING_EXTERNAL, ReviewHandoffMarker, ReviewOrchestrationMarker, RunActivityMarker, RunAttempt, StateStore, WorktreeMapping}, tracker::{IssueTracker, TrackerComment, TrackerIssue, linear::LinearClient, records}, workflow::{WorkflowDocument, WorkflowExecution}, worktree::{WorktreeManager, WorktreeSpec}};

include!("orchestrator/types.rs");

include!("orchestrator/entrypoints.rs");

include!("orchestrator/operator_http.rs");

include!("orchestrator/pull_request_review.rs");

include!("orchestrator/daemon.rs");

include!("orchestrator/reconciliation.rs");

include!("orchestrator/run_cycle.rs");

include!("orchestrator/runtime_validation.rs");

include!("orchestrator/execution.rs");

include!("orchestrator/dispatch_policy.rs");

include!("orchestrator/prompting.rs");

include!("orchestrator/git_ops.rs");

include!("orchestrator/status.rs");

include!("orchestrator/selection.rs");

include!("orchestrator/agent_evidence.rs");

pub(crate) const DEFAULT_STATUS_RUN_LIMIT: usize = 10;
pub(crate) const DEFAULT_OPERATOR_DASHBOARD_RUN_LIMIT: usize = 25;
pub(crate) const EXTERNAL_REVIEW_ACTOR_LOGIN: &str = "codex";
pub(crate) const EXTERNAL_REVIEW_REQUEST_BODY: &str = "@codex review";
pub(crate) const EXTERNAL_REVIEW_PASS_PHRASE: &str = "Didn't find any major issues.";
pub(crate) const EXTERNAL_REVIEW_ACK_TIMEOUT_SECS: i64 = 60;
pub(crate) const EXTERNAL_REVIEW_MERGE_VISIBILITY_TIMEOUT_SECS: i64 = 15 * 60;

const CONTINUATION_RETRY_DELAY_MS: u64 = 1_000;
const FAILURE_RETRY_BASE_DELAY_MS: u64 = 10_000;
const AGENT_GIT_ASKPASS_PREFIX: &str = ".decodex-git-askpass-";
const CONTINUATION_PENDING_RUN_STATUS: &str = "continuation_pending";
const TERMINAL_GUARDED_RUN_STATUS: &str = "terminal_guarded";
const TERMINAL_GUARD_MARKER_FILE: &str = ".decodex-terminal-guarded";
const TRACKER_RATE_LIMIT_BACKOFF_SECS: u64 = 15 * 60;
const TRACKER_RATE_LIMIT_WARNING: &str = "tracker_rate_limited";
const OPERATOR_DASHBOARD_ENDPOINT_PATH: &str = "/";
const OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH: &str = "/dashboard";
const OPERATOR_DASHBOARD_WS_ENDPOINT_PATH: &str = "/dashboard/control";
const OPERATOR_LIVE_ENDPOINT_PATH: &str = "/livez";
const OPERATOR_ACCOUNTS_ENDPOINT_PATH: &str = "/api/accounts";
const OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH: &str = "/api/operator-snapshot";
const OPERATOR_STATE_MAX_REQUEST_BYTES: usize = 8_192;
const OPERATOR_DASHBOARD_WS_CLIENT_MESSAGE_MAX_BYTES: usize = 64 * 1_024;
const OPERATOR_STATE_HEADER_TERMINATOR: &[u8] = b"\r\n\r\n";
const OPERATOR_DASHBOARD_WS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(20);
const OPERATOR_RUN_ACTIVITY_STREAM_INTERVAL: Duration = Duration::from_secs(1);
const OPERATOR_DEV_SNAPSHOT_STREAM_INTERVAL: Duration = Duration::from_secs(1);
const PULL_REQUEST_REVIEW_STATE_QUERY: &str = r#"
query($owner: String!, $name: String!, $number: Int!, $reviewThreadsAfter: String) {
  repository(owner: $owner, name: $name) {
    mergeCommitAllowed
    pullRequest(number: $number) {
      url
      state
      isDraft
      reviewDecision
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
const PULL_REQUEST_ISSUE_COMMENTS_QUERY: &str = r#"
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

#[cfg(test)] mod tests;
