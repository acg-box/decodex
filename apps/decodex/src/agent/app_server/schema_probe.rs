//! App-server generated schema compatibility probe.

mod constants;
mod dynamic_tools;
mod evidence;
mod generation;
mod markers;
mod method_unions;
mod output;
mod validation;

#[cfg(test)]
pub(super) use constants::{
	APP_SERVER_REQUIRED_CLIENT_NOTIFICATIONS, APP_SERVER_REQUIRED_CLIENT_REQUESTS,
	APP_SERVER_REQUIRED_SERVER_NOTIFICATIONS, APP_SERVER_REQUIRED_SERVER_REQUESTS,
	APP_SERVER_SCHEMA_REQUIRED_MARKERS,
};
pub(super) use generation::probe_app_server_schema;
#[cfg(test)] pub(super) use validation::validate_generated_app_server_schema;
