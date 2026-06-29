use std::collections::BTreeSet;

#[derive(Default)]
pub(in crate::mcp) struct McpHttpSessions {
	active: BTreeSet<String>,
	next_id: u64,
}
impl McpHttpSessions {
	pub(in crate::mcp::http::handler) fn create(&mut self) -> String {
		self.next_id = self.next_id.saturating_add(1);

		let session_id = format!("decodex-mcp-session-{:016x}", self.next_id);

		self.active.insert(session_id.clone());

		session_id
	}

	pub(in crate::mcp::http::handler) fn contains(&self, session_id: &str) -> bool {
		self.active.contains(session_id)
	}

	pub(in crate::mcp::http::handler) fn remove(&mut self, session_id: &str) -> bool {
		self.active.remove(session_id)
	}
}
