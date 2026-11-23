//! Register a persisted conversation from the account-visible native catalog.
use std::collections::HashSet;

use serde_json::{Value, json};

use super::{AgentCoordinator, AgentError, AgentWorkItem};
use decodex_core::work::ThreadRegistration;
use decodex_protocol::{NativeThreadCandidate, NativeThreadPage};

pub(crate) fn list_params(cursor: Option<&str>) -> Value {
	json!({"cursor":cursor,"limit":100,"currentAccountOnly":true,"archived":false,
        "sourceKinds":["cli","vscode","appServer","exec"],"modelProviders":[],
        "sortKey":"recency_at","sortDirection":"desc","useStateDbOnly":true})
}

pub(crate) fn candidate(row: &Value) -> Option<NativeThreadCandidate> {
	if row["ephemeral"] != false
		|| !row["parentThreadId"].is_null()
		|| row["canAcceptDirectInput"] == false
		|| !matches!(row["source"].as_str(), Some("cli" | "vscode" | "appServer" | "exec"))
		|| !matches!(row["threadSource"].as_str(), None | Some("user"))
	{
		return None;
	}
	let thread_id = row["id"].as_str()?;
	if thread_id.trim().is_empty()
		|| thread_id.len() > 512
		|| thread_id.chars().any(char::is_control)
	{
		return None;
	}
	let preview = catalog_text(&row["preview"]);
	let title = catalog_text(&row["name"])
		.or_else(|| preview.clone())
		.unwrap_or_else(|| thread_id.to_owned());
	Some(NativeThreadCandidate {
		thread_id: thread_id.into(),
		preview: preview.filter(|text| text != &title),
		title,
	})
}

fn catalog_text(field: &Value) -> Option<String> {
	let text = field.as_str()?.trim();
	if text.is_empty() || decodex_core::contains_credential_material(text) {
		return None;
	}
	let bounded = text
		.chars()
		.filter(|c| !c.is_control())
		.scan(0, |len, c| {
			*len += c.len_utf8();
			(*len <= 1024).then_some(c)
		})
		.collect::<String>();
	(!bounded.is_empty()).then_some(bounded)
}

pub(crate) fn page(raw: &Value) -> Option<NativeThreadPage> {
	let rows = raw["data"].as_array()?;
	if rows.len() > 100 {
		return None;
	}
	let next_cursor = match raw.get("nextCursor")? {
		Value::Null => None,
		Value::String(s) if !s.is_empty() && s.len() <= 4096 => Some(s.clone()),
		_ => return None,
	};
	Some(NativeThreadPage { threads: rows.iter().filter_map(candidate).collect(), next_cursor })
}

pub(crate) async fn find_visible<F, Fut>(
	thread: &str,
	mut fetch: F,
) -> Option<NativeThreadCandidate>
where
	F: FnMut(Option<String>) -> Fut,
	Fut: Future<Output = Option<NativeThreadPage>>,
{
	let mut cursor = None;
	let mut seen = HashSet::new();
	loop {
		let page = fetch(cursor).await?;
		if let Some(row) = page.threads.into_iter().find(|c| c.thread_id == thread) {
			return Some(row);
		}
		let next = page.next_cursor?;
		if !seen.insert(next.clone()) {
			return None;
		}
		cursor = Some(next);
	}
}

impl AgentCoordinator {
	pub(super) async fn admit_native_thread(
		&self,
		thread: &str,
		relation: &ThreadRegistration,
	) -> Result<NativeThreadCandidate, AgentError> {
		let client = &self.client;
		let candidate = find_visible(thread, |cursor| async move {
			page(&client.request("thread/list", list_params(cursor.as_deref())).await.ok()?)
		})
		.await
		.ok_or_else(|| {
			AgentError::Rejected("This conversation is unavailable in the current account.".into())
		})?;
		let raw = self.client.thread_read(json!({"threadId":thread})).await?;
		validate(&raw["thread"], &candidate, relation)?;
		Ok(candidate)
	}

	pub(crate) async fn register_native_thread(
		&mut self,
		thread: &str,
		relation: ThreadRegistration,
	) -> Result<AgentWorkItem, AgentError> {
		let selected = self.admit_native_thread(thread, &relation).await?;
		let (record, promoted) = self
			.store
			.register_native_thread_with_role_change(
				selected.thread_id,
				selected.title,
				relation,
				None,
			)
			.await?;
		if promoted {
			self.release_promoted_thread(thread).await;
		}
		Ok(record)
	}
}

fn validate(
	row: &Value,
	selected: &NativeThreadCandidate,
	relation: &ThreadRegistration,
) -> Result<(), AgentError> {
	if row["id"] != selected.thread_id || candidate(row).is_none() {
		return Err(AgentError::Rejected("This conversation cannot accept direct input.".into()));
	}
	let changes_role = matches!(
		relation,
		ThreadRegistration::Promote { .. }
			| ThreadRegistration::Release { .. }
			| ThreadRegistration::Assign { expected_owner: Some(_), .. }
	);
	if changes_role && row["status"]["type"] == "active" {
		return Err(AgentError::Rejected(
			"Finish the current turn before changing responsibility.".into(),
		));
	}
	Ok(())
}

pub(crate) async fn bind(
	store: &decodex_database::SqliteStore,
	row: &Value,
	selected: NativeThreadCandidate,
	relation: ThreadRegistration,
	root_settings: Option<String>,
) -> Result<AgentWorkItem, AgentError> {
	validate(row, &selected, &relation)?;
	Ok(store
		.register_native_thread_with_role_change(
			selected.thread_id,
			selected.title,
			relation,
			root_settings,
		)
		.await?
		.0)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn discovery_uses_account_visibility_and_only_lists_interactive_persisted_threads() {
		let query = list_params(Some("next"));
		assert_eq!(query["currentAccountOnly"], true);
		assert_eq!(query["archived"], false);
		assert_eq!(query["cursor"], "next");

		let direct = json!({"id":"top","source":"appServer","threadSource":"user",
            "ephemeral":false, "preview":"Review the design"});
		let hidden = [
			("ephemeral", json!(true)),
			("source", json!({"subAgent":{"thread_spawn":{"parent_thread_id":"parent"}}})),
			("parentThreadId", json!("parent")),
			("canAcceptDirectInput", json!(false)),
			("threadSource", json!("decodex_assistant")),
		];
		let mut rows = vec![direct.clone()];
		for (field, value) in hidden {
			let mut row = direct.clone();
			row[field] = value;
			rows.push(row);
		}
		let page = page(&json!({"data":rows, "nextCursor":"older"})).unwrap();
		assert_eq!(
			page.threads,
			[NativeThreadCandidate {
				thread_id: "top".into(),
				title: "Review the design".into(),
				preview: None
			}]
		);
		assert_eq!(page.next_cursor.as_deref(), Some("older"));

		let mut named = direct;
		named["name"] = json!("Release review");
		assert_eq!(candidate(&named).unwrap().preview.as_deref(), Some("Review the design"));
		named["preview"] =
			json!("Authorization: Bearer sk-proj-abcdefghijklmnopqrstuvwxyz0123456789");
		assert_eq!(candidate(&named).unwrap().preview, None);
		named["name"] = Value::Null;
		assert_eq!(candidate(&named).unwrap().title, "top");
	}
}
