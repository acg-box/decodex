mod account;
mod catalog;
mod command;
mod lifecycle;
mod thread;
mod turn;

use crate::agent::json_rpc::JsonRpcConnection;

pub(in crate::agent::app_server) struct AppServerClient {
	pub(in crate::agent::app_server) connection: JsonRpcConnection,
}
