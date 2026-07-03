use crate::orchestrator::operator_http::{
	self, Arc, Mutex, OperatorRunStatus, OperatorStatusSnapshot, PublishedOperatorSnapshot, Result,
	Value, eyre,
};

pub(super) fn snapshot_json_with_live_account_control(snapshot_json: &[u8]) -> Vec<u8> {
	let Ok(mut snapshot) = serde_json::from_slice::<Value>(snapshot_json) else {
		return snapshot_json.to_vec();
	};
	let Some(snapshot_object) = snapshot.as_object_mut() else {
		return snapshot_json.to_vec();
	};

	if !snapshot_object.contains_key("account_control") {
		return snapshot_json.to_vec();
	}

	let account_control = operator_http::global_codex_account_control_status();

	snapshot_object.insert(
		String::from("account_control"),
		operator_http::json!({
			"mode": account_control.mode,
			"account_selector": account_control.account_selector,
		}),
	);

	match serde_json::to_vec(&snapshot) {
		Ok(output) => output,
		Err(_) => snapshot_json.to_vec(),
	}
}

pub(super) fn build_operator_app_snapshot_http_response(
	snapshot: &Arc<Mutex<PublishedOperatorSnapshot>>,
) -> Vec<u8> {
	let snapshot = match snapshot.lock() {
		Ok(snapshot) => snapshot,
		Err(error) => {
			return operator_http::http_response_bytes(
				"500 Internal Server Error",
				"text/plain; charset=utf-8",
				format!("operator snapshot lock poisoned: {error}").as_bytes(),
			);
		},
	};
	let Some(snapshot_json) = snapshot.snapshot_json.as_deref() else {
		return operator_http::http_response_bytes_with_headers(
			"200 OK",
			"application/json",
			&[("Cache-Control", String::from("no-store"))],
			b"{}",
		);
	};
	let body = snapshot_json_with_live_account_control(snapshot_json);
	let mut headers = vec![("Cache-Control", String::from("no-store"))];

	if let Some(published_at) = snapshot.last_publish_unix_epoch {
		headers.push(("X-Decodex-Snapshot-Unix-Epoch", published_at.to_string()));
	}

	operator_http::http_response_bytes_with_headers("200 OK", "application/json", &headers, &body)
}

pub(super) fn attach_operator_snapshot_presentation(
	snapshot: &mut Value,
	current_lanes: &[OperatorRunStatus],
) -> Result<()> {
	if let Some(object) = snapshot.as_object_mut() {
		object.insert(
			String::from("presentation"),
			operator_http::operator_snapshot_presentation_value(current_lanes)?,
		);
	}

	Ok(())
}

pub(super) fn dashboard_current_snapshot_event_payload(
	snapshot: &Arc<Mutex<PublishedOperatorSnapshot>>,
) -> Result<Option<Value>> {
	let published_snapshot = snapshot
		.lock()
		.map_err(|error| eyre::eyre!("Operator state snapshot lock poisoned: {error}"))?
		.clone();
	let Some(snapshot_json) = published_snapshot.snapshot_json.as_ref() else {
		return Ok(None);
	};
	let snapshot_json = snapshot_json_with_live_account_control(snapshot_json);
	let snapshot = serde_json::from_slice::<Value>(&snapshot_json)?;

	Ok(Some(operator_http::json!({
		"snapshotPublishedAtUnixEpoch": published_snapshot.last_publish_unix_epoch,
		"snapshot": snapshot,
	})))
}

pub(crate) fn operator_snapshot_json_value(snapshot: &OperatorStatusSnapshot) -> Result<Value> {
	let mut value = serde_json::to_value(snapshot)?;

	attach_operator_snapshot_presentation(&mut value, &snapshot.current_lanes)?;

	Ok(value)
}
