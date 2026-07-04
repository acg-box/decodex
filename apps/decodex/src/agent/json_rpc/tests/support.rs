use std::{
	collections::VecDeque,
	process::{Command, Stdio},
	sync::{Arc, Mutex, mpsc},
};

use crate::agent::json_rpc::JsonRpcConnection;

pub(crate) fn test_connection_with_messages<const N: usize>(
	messages: [&str; N],
) -> JsonRpcConnection {
	let mut child = Command::new("sh")
		.args(["-c", "cat >/dev/null"])
		.stdin(Stdio::piped())
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.spawn()
		.expect("child process should spawn");
	let stdin = child.stdin.take().expect("child stdin should be captured");
	let (stdout_tx, stdout_rx) = mpsc::channel();

	for message in messages {
		stdout_tx.send(message.to_owned()).expect("test message should send");
	}

	JsonRpcConnection {
		child,
		stdin,
		stdout_rx,
		stderr_tail: Arc::new(Mutex::new(VecDeque::new())),
		pending_messages: VecDeque::new(),
		next_request_id: 1,
	}
}
