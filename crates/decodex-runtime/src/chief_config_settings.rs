//! Shared native file recovery, independent of the initiating settings panel or request lifetime.
use crate::chief_usage_estimate::Source;
use decodex_database::{ChiefConfigOwner, ChiefConfigReceipt as Receipt, SqliteStore};
use decodex_protocol::ChiefConfigEditReceipt;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

pub(super) fn digest(value: &str) -> String {
	Sha256::digest(value.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}
pub(super) fn owner(source: &Source) -> ChiefConfigOwner {
	let k = &source.key;
	ChiefConfigOwner {
		work: k.work.clone(),
		thread: k.thread.clone(),
		generation: k.generation.as_str().into(),
		account: k.account.as_str().into(),
	}
}
pub(super) fn project(receipt: &Receipt) -> ChiefConfigEditReceipt {
	let (state, owner, target, version) = match receipt {
		Receipt::Hook(r) => (
			&r.state,
			&r.attempt.owner,
			format!("Hook {} · {}", r.attempt.hook, r.attempt.field),
			None,
		),
		Receipt::App(r) => (
			&r.state,
			&r.attempt.owner,
			if r.attempt.field == "omit_tools_from" {
				format!("App {} · tool visibility", r.attempt.connector)
			} else {
				format!(
					"App {} · connection {} · {}",
					r.attempt.connector, r.attempt.link, r.attempt.field
				)
			},
			r.saved_version.clone(),
		),
	};
	ChiefConfigEditReceipt {
		outcome: state.clone(),
		target,
		work_id: owner.work.clone(),
		account_id: owner.account.clone(),
		saved_version: version,
	}
}
pub(super) fn pending(receipt: &Receipt) -> bool {
	let state = match receipt {
		Receipt::Hook(r) => &r.state,
		Receipt::App(r) => &r.state,
	};
	matches!(state.as_str(), "reserved" | "unknown")
}
pub(super) fn raw_app(
	native: &decodex_codex::app_server_client::AppLinkSettings,
	field: &str,
) -> Option<Value> {
	if field == "approvals_reviewer" { &native.user_reviewer } else { &native.user_mode }
		.as_ref()
		.map(|v| Value::String(v.clone()))
}
/// Read only the uncertain target, then let the durable owner decide whether this observation
/// settles it.
pub(super) async fn reconcile(
	store: &SqliteStore,
	source: &Source,
	cwd: &str,
	scope: &str,
) -> Option<()> {
	let Some(receipt) = store.chief_config_receipt(scope.into()).await.ok()? else {
		return Some(());
	};
	if !pending(&receipt) {
		return Some(());
	}
	match receipt {
		Receipt::Hook(r) => {
			let native = source.client.hook_settings(cwd).await.ok()?;
			if digest(native.config_file()) != scope {
				return None;
			}
			store
				.observe_chief_hook_setting(
					r.id,
					decodex_database::ChiefHookObservation {
						owner: owner(source),
						scope: scope.into(),
						hook: r.attempt.hook.clone(),
						field: r.attempt.field.clone(),
						value: native
							.saved_hook(&r.attempt.hook)
							.and_then(|v| v.get(&r.attempt.field))
							.cloned(),
						config_version: native.config_version().into(),
					},
				)
				.await
				.ok()?;
		},
		Receipt::App(r) => {
			let (value, version) = if r.attempt.field == "omit_tools_from" {
				let native =
					source.client.app_tool_exposure(cwd, &r.attempt.connector).await.ok()?;
				if digest(native.config_file()) != scope {
					return None;
				}
				(
					native.preference.as_ref().map(|v| serde_json::json!(v)),
					native.config_version().to_owned(),
				)
			} else {
				let native = source
					.client
					.app_link_settings(cwd, &r.attempt.connector, &r.attempt.link)
					.await
					.ok()?;
				if digest(native.config_file()) != scope {
					return None;
				}
				(raw_app(&native, &r.attempt.field), native.config_version().to_owned())
			};
			store
				.observe_chief_app_settings(
					r.id,
					decodex_database::ChiefAppSettingsObservation {
						owner: owner(source),
						scope: scope.into(),
						connector: r.attempt.connector,
						link: r.attempt.link,
						value,
						field: r.attempt.field,
						config_version: version,
					},
				)
				.await
				.ok()?;
		},
	}
	Some(())
}
