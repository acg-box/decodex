//! Operator HTTP endpoint, dashboard, and control API handling.

mod api;
mod assets;
mod dashboard;
mod http;
mod routes;
mod server;
mod snapshot;
mod types;

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use self::api::{
	build_operator_lane_inspect_http_response, build_operator_lane_interrupt_http_response,
	build_operator_lane_steer_http_response, build_operator_state_http_response,
	build_operator_state_http_response_with_control_requests,
};
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use self::{
	assets::DASHBOARD_MAX_WEBSOCKET_CLIENTS,
	dashboard::{
		build_operator_run_activity_event, dashboard_websocket_message,
		strip_dashboard_run_activity_volatile_fields,
	},
	server::handle_operator_state_endpoint_connection,
	types::DashboardClientSubscription,
};
pub(crate) use self::{
	dashboard::run_operator_run_activity_websocket_broadcasts, server::run_operator_state_endpoint,
	snapshot::operator_snapshot_json_value, types::DashboardEventHub,
};

use std::{
	io::{ErrorKind, Read, Write},
	net::{TcpListener, TcpStream},
	sync::{Arc, Mutex, mpsc::Receiver},
	thread,
	time::Duration,
};

use base64::engine::general_purpose::STANDARD;
use color_eyre::Report;
use serde_json::{Value, json};
use sha1::Sha1;

use self::{
	api::{
		build_operator_account_http_response, build_operator_linear_scan_http_response,
		operator_request_route_is_account_api,
	},
	assets::{DASHBOARD_RUN_ACTIVITY_FINGERPRINT_VOLATILE_FIELDS, OPERATOR_HTTP_READ_TIMEOUT},
	dashboard::{
		handle_operator_dashboard_websocket_connection, websocket_upgrade_required_response,
	},
	http::{
		http_response_bytes, http_response_bytes_with_headers, operator_http_header_contains_token,
		operator_http_header_value, operator_http_query_value, operator_http_query_value_alias,
		operator_http_request_body, read_operator_state_request_headers,
	},
	routes::{build_operator_state_http_response_for_route, parse_operator_state_request_route},
	snapshot::build_operator_app_snapshot_http_response,
	types::{
		DashboardBroadcastEvent, DashboardClientFrame, DashboardClientMessage, DashboardControlAck,
		DashboardRunActivityEvent, DashboardWebSocketSession, OperatorAccountRequest,
		OperatorLaneInterruptHttpRequest, OperatorLaneSteerHttpRequest,
		OperatorLinearScanHttpRequest, OperatorRequestRoute,
	},
};
use crate::{
	accounts::AccountUseRequest,
	config::ServiceConfig,
	orchestrator::{
		DEFAULT_STEER_RESULT_WAIT_TIMEOUT, LaneSteerReport, LaneSteerRequest,
		OPERATOR_ACCOUNTS_ENDPOINT_PATH, OPERATOR_APP_SNAPSHOT_ENDPOINT_PATH,
		OPERATOR_DASHBOARD_ALIAS_ENDPOINT_PATH, OPERATOR_DASHBOARD_ENDPOINT_PATH,
		OPERATOR_DASHBOARD_WS_CLIENT_MESSAGE_MAX_BYTES, OPERATOR_DASHBOARD_WS_ENDPOINT_PATH,
		OPERATOR_LANE_INSPECT_ENDPOINT_PATH, OPERATOR_LANE_INTERRUPT_ENDPOINT_PATH,
		OPERATOR_LANE_STEER_ALIAS_ENDPOINT_PATH, OPERATOR_LANE_STEER_ENDPOINT_PATH,
		OPERATOR_LINEAR_SCAN_ENDPOINT_PATH, OPERATOR_LIVE_ENDPOINT_PATH,
		OPERATOR_RUN_ACTIVITY_STREAM_INTERVAL, OPERATOR_STATE_HEADER_TERMINATOR,
		OPERATOR_STATE_MAX_REQUEST_BYTES, OperatorCodexAccountControlStatus,
		OperatorControlRequests, OperatorRunStatus, OperatorStatusSnapshot,
		PublishedOperatorSnapshot, global_codex_account_control_status, lane_control,
		operator_snapshot_presentation_value,
	},
	prelude::{Result, eyre},
	state::StateStore,
	workflow::WorkflowDocument,
};
