//! Supported native Project catalog identity. Local Work ownership remains in Decodex.
use super::{AppServerClient, ClientError};
use serde_json::Value;
impl AppServerClient {
	/// Reuse a unique native catalog Project for the exact registered directory.
	/// Ambiguous or incomplete catalogs do not authorize creation of a duplicate.
	pub async fn project_for_directory(
		&self,
		directory: &str,
	) -> Result<Option<Value>, ClientError> {
		let mut cursor = Value::Null;
		let mut seen = std::collections::HashSet::new();
		let mut selected = None;
		loop {
			let params = serde_json::json!({"cursor":cursor,"limit":100});
			let page = tokio::time::timeout(
				std::time::Duration::from_secs(8),
				self.request("project/list", params),
			)
			.await
			.map_err(|_| ClientError::Io)??;
			let rows = page["data"]
				.as_array()
				.filter(|r| r.len() <= 100)
				.ok_or(ClientError::InvalidFrame)?;
			for project in rows {
				if project["roots"]
					.as_array()
					.is_some_and(|roots| roots.iter().any(|r| r["path"] == directory))
				{
					if selected.is_some() {
						return Err(ClientError::InvalidFrame);
					}
					selected = Some(serde_json::json!({"project":project}));
				}
			}
			if page["nextCursor"].is_null() {
				return Ok(selected);
			}
			let next = page["nextCursor"]
				.as_str()
				.filter(|s| !s.is_empty() && s.len() <= 4096)
				.ok_or(ClientError::InvalidFrame)?;
			if seen.len() >= 128 || !seen.insert(next.to_owned()) {
				return Err(ClientError::CapacityExceeded);
			}
			cursor = serde_json::json!(next);
		}
	}

	/// Save a name on a newly created persistent native context without admitting a turn.
	/// Native metadata persistence materializes its empty history for later resume.
	pub async fn name_thread(&self, thread: &str, name: &str) -> Result<(), ClientError> {
		let params = serde_json::json!({"threadId":thread,"name":name});
		if !is_thread_name_selection(&params) {
			return Err(ClientError::InvalidFrame);
		}
		let response = tokio::time::timeout(
			std::time::Duration::from_secs(8),
			self.request("thread/name/set", params),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		if response.as_object().is_none_or(|r| !r.is_empty()) {
			return Err(ClientError::InvalidFrame);
		}
		Ok(())
	}

	/// Create or recover one catalog entry using the native idempotency key.
	pub async fn create_project(
		&self,
		name: &str,
		directory: &str,
		key: &str,
	) -> Result<Value, ClientError> {
		let params =
			serde_json::json!({"name":name,"roots":[{"path":directory}],"idempotencyKey":key});
		if !is_project_create(&params) {
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(
			std::time::Duration::from_secs(15),
			self.request("project/create", params),
		)
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Read the exact catalog identity; a missing entry is not replaced silently.
	pub async fn read_project(&self, id: &str) -> Result<Value, ClientError> {
		let params = serde_json::json!({"projectId":id});
		if !is_project_read(&params) {
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(
			std::time::Duration::from_secs(8),
			self.request("project/read", params),
		)
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Assign an existing conversation through native metadata without changing execution.
	pub async fn assign_thread_project(
		&self,
		thread: &str,
		project: &str,
	) -> Result<Value, ClientError> {
		let params = serde_json::json!({"threadId":thread,"projectId":project});
		if !is_thread_project_assignment(&params) {
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(
			std::time::Duration::from_secs(8),
			self.request("thread/metadata/update", params),
		)
		.await
		.map_err(|_| ClientError::Io)?
	}
}
/// Validate only the native create contract used by the directory registration owner.
pub fn is_project_create(params: &Value) -> bool {
	params.as_object().is_some_and(|m| m.len() == 3)
		&& valid(&params["name"], 256)
		&& valid(&params["idempotencyKey"], 512)
		&& params["roots"].as_array().is_some_and(|roots| {
			roots.len() == 1
				&& roots[0].as_object().is_some_and(|r| r.len() == 1)
				&& roots[0]["path"]
					.as_str()
					.is_some_and(|p| p.len() <= 4096 && std::path::Path::new(p).is_absolute())
		})
}
/// Validate an exact native read, without a search or wildcard owner.
pub fn is_project_read(params: &Value) -> bool {
	params.as_object().is_some_and(|m| m.len() == 1) && valid(&params["projectId"], 512)
}
fn valid(value: &Value, max: usize) -> bool {
	value
		.as_str()
		.is_some_and(|s| !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control))
}

/// Validate only the bounded native catalog page reader.
pub fn is_project_list(params: &Value) -> bool {
	params.as_object().is_some_and(|m| m.len() == 2)
		&& params["limit"] == 100
		&& (params["cursor"].is_null() || valid(&params["cursor"], 4096))
}

/// Validate the exact name edit used to persist the replacement context.
pub fn is_thread_name_selection(params: &Value) -> bool {
	params.as_object().is_some_and(|m| m.len() == 2)
		&& valid(&params["threadId"], 512)
		&& valid(&params["name"], 1024)
}

/// Validate only an exact Project assignment, without clearing or other metadata edits.
pub fn is_thread_project_assignment(params: &Value) -> bool {
	params.as_object().is_some_and(|m| m.len() == 2)
		&& valid(&params["threadId"], 512)
		&& valid(&params["projectId"], 512)
}
