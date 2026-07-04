mod lifecycle;
mod request;
mod transport;

use std::{
	collections::VecDeque,
	process::{Child, ChildStdin},
	sync::{Arc, Mutex, mpsc::Receiver},
	time::Duration,
};

use color_eyre::eyre;
use serde::{Serialize, de::DeserializeOwned};

use crate::{agent::json_rpc::wire::WireMessage, prelude::Result};

pub(crate) struct JsonRpcConnection {
	pub(super) child: Child,
	pub(super) stdin: ChildStdin,
	pub(super) stdout_rx: Receiver<String>,
	pub(super) stderr_tail: Arc<Mutex<VecDeque<String>>>,
	pub(super) pending_messages: VecDeque<WireMessage>,
	pub(super) next_request_id: i64,
}
impl JsonRpcConnection {
	#[allow(dead_code)]
	pub(crate) fn request<P, T>(&mut self, method: &str, params: &P, timeout: Duration) -> Result<T>
	where
		P: Serialize,
		T: DeserializeOwned,
	{
		self.request_with_handler(method, params, timeout, |_connection, _message, request| {
			eyre::bail!(
				"Unexpected inbound JSON-RPC request `{}` while waiting for `{method}`.",
				request.method
			);
		})
	}
}
