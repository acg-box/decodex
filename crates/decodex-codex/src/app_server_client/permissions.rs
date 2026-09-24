//! Native permission catalogs and task-local selections. A queue ACK is not confirmation.
use super::{AppServerClient, ClientError, HistoryGuard};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path, time::Duration};

fn bounded(value: &str, limit: usize) -> bool {
	!value.trim().is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

/// One profile resolved by the server for the selected working directory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NativePermissionProfile {
	/// Opaque profile ID, including native builtin IDs.
	pub id: String,
	/// Effective requirements allow selecting this profile.
	pub allowed: bool,
	/// Optional native display text; never interpreted as instructions.
	pub description: Option<String>,
}

/// A saved-task profile selection, without unrelated model or policy overrides.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ThreadPermissionSelection {
	thread_id: String,
	permissions: String,
}
impl ThreadPermissionSelection {
	/// The runtime must authorize the exact task and revalidate the allowed catalog entry.
	pub fn new(thread: &str, profile: &str) -> Result<Self, ClientError> {
		let value = Self { thread_id: thread.into(), permissions: profile.into() };
		if value.valid() { Ok(value) } else { Err(ClientError::InvalidFrame) }
	}

	fn valid(&self) -> bool {
		bounded(&self.thread_id, 512) && bounded(&self.permissions, 256)
	}
}

/// Validate the narrow shape accepted by the retained process bridge.
pub fn is_thread_permission_selection(value: &Value) -> bool {
	serde_json::from_value::<ThreadPermissionSelection>(value.clone())
		.is_ok_and(|selection| selection.valid())
}

/// Native accepted the request for processing. Observe settings before claiming application.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThreadPermissionSelectionQueued;

impl AppServerClient {
	/// Read every profile page for one native cwd. Errors never become an empty catalog.
	/// A 128 KiB aggregate budget and 1,024-row/16-page limits bound untrusted responses.
	pub async fn permission_profiles(
		&self,
		cwd: &str,
	) -> Result<Vec<NativePermissionProfile>, ClientError> {
		if !bounded(cwd, 16384) || !Path::new(cwd).is_absolute() {
			return Err(ClientError::InvalidFrame);
		}
		tokio::time::timeout(Duration::from_secs(15), async {
			let mut cursor: Option<String> = None;
			let mut cursors = HashSet::new();
			let mut ids = HashSet::new();
			let mut profiles = Vec::new();
			let mut budget = 128 * 1024_usize;
			for _ in 0..16 {
				let page = self
					.request(
						"permissionProfile/list",
						json!({
							"cwd":cwd,"limit":100,"cursor":cursor
						}),
					)
					.await?;
				budget = budget
					.checked_sub(page.to_string().len())
					.ok_or(ClientError::CapacityExceeded)?;
				let data = page["data"].as_array().ok_or(ClientError::InvalidFrame)?;
				if data.len() > 100 || profiles.len() + data.len() > 1024 {
					return Err(ClientError::CapacityExceeded);
				}
				for value in data {
					let profile: NativePermissionProfile = serde_json::from_value(value.clone())
						.map_err(|_| ClientError::InvalidFrame)?;
					if !bounded(&profile.id, 256)
						|| !ids.insert(profile.id.clone())
						|| profile.description.as_ref().is_some_and(|text| text.len() > 4096)
					{
						return Err(ClientError::InvalidFrame);
					}
					profiles.push(profile);
				}
				match page.get("nextCursor") {
					None | Some(Value::Null) => return Ok(profiles),
					Some(Value::String(next))
						if bounded(next, 4096) && cursors.insert(next.clone()) =>
					{
						cursor = Some(next.clone());
					},
					_ => return Err(ClientError::InvalidFrame),
				}
			}
			Err(ClientError::CapacityExceeded)
		})
		.await
		.map_err(|_| ClientError::Io)?
	}

	/// Submit once after the runtime records its durable dispatch fence. No automatic retry.
	pub async fn queue_thread_permission_selection(
		&self,
		selection: &ThreadPermissionSelection,
		guard: HistoryGuard,
	) -> Result<ThreadPermissionSelectionQueued, ClientError> {
		if !selection.valid() {
			return Err(ClientError::InvalidFrame);
		}
		let params = serde_json::to_value(selection).map_err(|_| ClientError::InvalidFrame)?;
		let response = tokio::time::timeout(
			Duration::from_secs(8),
			self.request_with_history("thread/settings/update", params, guard),
		)
		.await
		.map_err(|_| ClientError::Io)??;
		if response.as_object().is_some_and(|value| value.is_empty()) {
			Ok(ThreadPermissionSelectionQueued)
		} else {
			Err(ClientError::InvalidFrame)
		}
	}
}

#[cfg(test)]
#[path = "permissions_tests.rs"]
mod tests;

/// Native saved permission facts. These observations do not authorize a new operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NativeTaskPermissions {
	/// Native cwd used to resolve the profile catalog.
	pub cwd: String,
	/// Absent for native policies without a named or builtin profile identity.
	pub profile_id: Option<String>,
	/// Native approval policy, including structured policies.
	pub approval_policy: Value,
	/// Native reviewer name; no local default is substituted.
	pub approvals_reviewer: String,
	/// Native sandbox projection; not the complete filesystem rules of a named profile.
	pub sandbox_policy: Value,
}
impl NativeTaskPermissions {
	/// Parse a complete native settings publication.
	pub fn from_notification(value: &Value) -> Option<Self> {
		Self::from_fields(value, "sandboxPolicy")
	}

	/// Parse a native thread start/resume reply.
	pub fn from_thread_response(value: &Value) -> Option<Self> {
		Self::from_fields(value, "sandbox")
	}

	fn from_fields(value: &Value, sandbox_key: &str) -> Option<Self> {
		let cwd = value.get("cwd")?.as_str()?;
		let reviewer = value.get("approvalsReviewer")?.as_str()?;
		let approval = value.get("approvalPolicy")?;
		let sandbox = value.get(sandbox_key)?;
		if !bounded(cwd, 16384)
			|| !Path::new(cwd).is_absolute()
			|| !bounded(reviewer, 128)
			|| !(approval.is_string() || approval.is_object())
			|| approval.to_string().len() > 4096
			|| !sandbox.is_object()
			|| !sandbox["type"].as_str().is_some_and(|kind| bounded(kind, 128))
			|| sandbox.to_string().len() > 32768
		{
			return None;
		}
		let profile_id = match value.get("activePermissionProfile") {
			None | Some(Value::Null) => None,
			Some(profile) =>
				Some(profile.get("id")?.as_str().filter(|id| bounded(id, 256))?.to_owned()),
		};
		Some(Self {
			cwd: cwd.into(),
			profile_id,
			approval_policy: approval.clone(),
			approvals_reviewer: reviewer.into(),
			sandbox_policy: sandbox.clone(),
		})
	}
}
