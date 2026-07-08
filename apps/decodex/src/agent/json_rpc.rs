mod connection;
mod environment;
mod errors;
mod wire;

#[cfg(test)] pub(crate) use self::wire::JsonRpcErrorPayload;
pub(crate) use self::{
	connection::JsonRpcConnection,
	environment::{AppServerProcessEnv, ResolvedAppServerCodexHomeEnv, app_server_command_program},
	errors::{AppServerHomePreflightFailure, AppServerOutputTimeout, AppServerTransportFailure},
	wire::{JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, WireMessage},
};

#[cfg(test)] mod tests;
