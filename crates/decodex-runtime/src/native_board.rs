//! Read the locked Codex local Board. Only Codex publishes posts, recovers trees and deletes data.
//! Code Mode can complete a native post without emitting a public timeline item.
//! The read contract is the v1 posts table in Codex 475e13ab1a, message-board/src/local/pool.rs.
use std::path::Path;

use decodex_codex::app_server_client::AppServerClient;
use decodex_protocol::{NativeBoardPage, NativeBoardPost};
use rusqlite::{Connection, OpenFlags, params};

pub(crate) async fn read(
	client: &AppServerClient,
	root: &str,
	after: i64,
) -> Option<NativeBoardPage> {
	let home = client.native_home()?;
	let thread = client.thread_read(serde_json::json!({"threadId":root})).await.ok()?;
	if thread["thread"]["id"] != root {
		return None;
	}
	let cwd = thread["thread"]["cwd"].as_str()?;
	let response = client
		.request("config/read", serde_json::json!({"cwd":cwd,"includeLayers":false}))
		.await
		.ok()?;
	let config = response.get("config")?;
	if config
		.pointer("/features/multi_agent_v2/message_board_remote")
		.is_some_and(|remote| !remote.is_null())
	{
		return None;
	}
	// The supervised process has a clean environment. Codex uses its own home unless
	// sqlite_home is configured. Never create, migrate or search for a native database.
	let directory = match config.get("sqlite_home") {
		None | Some(serde_json::Value::Null) => home,
		Some(value) => Path::new(value.as_str()?),
	};
	if !directory.is_absolute() {
		return None;
	}
	let database = directory.join("agent_message_board_1.sqlite");
	let root = root.to_owned();
	tokio::task::spawn_blocking(move || {
		let db = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
		page(&db, &root, after).ok()
	})
	.await
	.ok()
	.flatten()
}

fn page(db: &Connection, root: &str, after: i64) -> rusqlite::Result<NativeBoardPage> {
	const PAGE_SIZE: usize = 12;
	// Query within one tree. Use the native append sequence, so a reply to an old
	// topic arrives on the next page even when nothing changed in the main chat.
	let mut query = db.prepare("SELECT seq, id, root, channel, author,
        json_extract(payload, '$.metadata.created_at'), substr(json_extract(payload, '$.text'), 1, 512),
        length(json_extract(payload, '$.text')) > 512, request_id
        FROM posts WHERE board = ?1 AND seq > ?2 ORDER BY seq LIMIT ?3")?;
	let mut rows = query.query(params![root, after, (PAGE_SIZE + 1) as i64])?;
	let mut result = NativeBoardPage { root: root.into(), after, more: false, posts: Vec::new() };
	while let Some(row) = rows.next()? {
		if result.posts.len() == PAGE_SIZE {
			result.more = true;
			break;
		}
		let request: String = row.get(8)?;
		// Native post request keys are scoped by the publishing Codex thread.
		let posted_by_thread = request.split_once(':').map(|(thread, _)| thread.to_owned());
		result.posts.push(NativeBoardPost {
			message_id: row.get(1)?,
			thread_id: row.get(2)?,
			channel_name: row.get(3)?,
			author: row.get(4)?,
			created_at: row.get(5)?,
			text: row.get(6)?,
			truncated: row.get(7)?,
			posted_by_thread,
		});
		result.after = row.get(0)?;
	}
	Ok(result)
}

#[cfg(test)]
mod tests {
	use super::*;
	#[test]
	fn board_cursor_survives_a_reopen_and_excludes_other_trees() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("board.sqlite");
		let db = Connection::open(&path).unwrap();
		db.execute_batch("CREATE TABLE posts (seq INTEGER PRIMARY KEY, board TEXT, id TEXT, root TEXT, channel TEXT, author TEXT, payload TEXT, request_id TEXT)").unwrap();
		for i in 1..=15 {
			let opener = i == 1;
			let tree = if i == 2 { "other" } else { "tree" };
			let writer = if opener { "asker" } else { "responder" };
			let text = if opener { "How many?".into() } else { "午".repeat(513) };
			db.execute("INSERT INTO posts VALUES (?1, ?2, ?3, '1', 'lunch', ?4, ?5, ?6)",
                params![i, tree, i.to_string(), writer, serde_json::json!({"metadata":{"created_at":"2026-01-01T00:00:00Z"},"text":text}).to_string(), format!("{writer}:turn:tool")]).unwrap();
		}
		let first = page(&db, "tree", 0).unwrap();
		assert!(first.more);
		assert_eq!(first.after, 13);
		assert_eq!(first.posts[0].text, "How many?");
		assert!(!first.posts.iter().any(|p| p.message_id == "2"));
		drop(db);
		let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
		let next = page(&db, "tree", first.after).unwrap();
		assert!(!next.more);
		assert_eq!(next.posts.len(), 2);
		let reply = &next.posts[0];
		assert_eq!(reply.thread_id, first.posts[0].message_id);
		assert_eq!(reply.posted_by_thread.as_deref(), Some("responder"));
		assert_eq!(reply.text.chars().count(), 512);
		assert!(reply.truncated);
		assert!(page(&db, "tree", next.after).unwrap().posts.is_empty());
	}
}
