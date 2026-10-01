//! Isolated Responses fixture shared by native lifecycle tests.
use std::sync::{
	Arc,
	atomic::{AtomicUsize, Ordering},
};

use tokio::{io::AsyncWriteExt as _, net::TcpListener, sync::Notify};

use crate::agent::tests::native_task_references;

pub(super) async fn serve(listener: TcpListener, calls: Arc<AtomicUsize>) {
	serve_with_gate(listener, calls, Vec::new()).await;
}

pub(super) async fn serve_with_gate(
	listener: TcpListener,
	calls: Arc<AtomicUsize>,
	gates: Vec<(usize, Arc<Notify>)>,
) {
	while let Ok((mut socket, _)) = listener.accept().await {
		let _body = native_task_references::read_http_body(&mut socket).await;
		let serial = calls.fetch_add(1, Ordering::AcqRel);

		if let Some((_, gate)) = gates.iter().find(|(blocked, _)| serial == *blocked) {
			gate.notified().await;
		}

		let id = format!("permissions-{serial}");
		let frames = [
			serde_json::json!({"type":"response.created","response":{"id":id}}),
			serde_json::json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":format!("message-{serial}"),"content":[{"type":"output_text","text":"Done."}]}}),
			serde_json::json!({"type":"response.completed","response":{"id":id,"usage":{"input_tokens":0,"output_tokens":5,"total_tokens":5}}}),
		];
		let data = frames
			.iter()
			.map(|v| format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap()))
			.collect::<String>();
		let response = format!(
			"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}",
			data.len()
		);

		socket.write_all(response.as_bytes()).await.unwrap();
	}
}
