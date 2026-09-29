//! Optional host-owned skill directories, captured once for the service lifetime.
//! DECODEX_SKILL_ROOTS uses the platform path-list separator (colon on macOS).
//! These directories apply to each fresh retained Agent process, across its tasks.
use decodex_codex::app_server_client::AppServerClient;
use std::{ffi::OsStr, time::Duration};

#[derive(Clone)]
pub(crate) struct RuntimeSkillRoots(Option<Vec<String>>);

impl RuntimeSkillRoots {
	pub(crate) fn from_environment() -> Result<Self, &'static str> {
		Self::parse(std::env::var_os("DECODEX_SKILL_ROOTS").as_deref())
	}

	fn parse(value: Option<&OsStr>) -> Result<Self, &'static str> {
		let Some(value) = value else {
			return Ok(Self(None));
		};
		let mut roots = Vec::new();
		if !value.is_empty() {
			for path in std::env::split_paths(value) {
				let Some(path) = path.to_str().filter(|_| path.is_absolute()) else {
					return Err("DECODEX_SKILL_ROOTS must contain absolute UTF-8 paths");
				};
				if !roots.iter().any(|root| root == path) {
					roots.push(path.to_owned());
				}
			}
		}
		Ok(Self(Some(roots)))
	}

	pub(crate) fn values(&self) -> Option<&[String]> {
		self.0.as_deref()
	}

	pub(crate) async fn apply(&self, client: &AppServerClient) -> Result<(), &'static str> {
		let Some(roots) = &self.0 else {
			return Ok(());
		};
		// Native owns discovery, watching, and replacement. Never edit plugin configuration.
		match tokio::time::timeout(
			Duration::from_secs(15),
			client.request("skills/extraRoots/set", serde_json::json!({"extraRoots":roots})),
		)
		.await
		{
			Ok(Ok(_)) => Ok(()),
			_ => Err(
				"Native skill directory setup was not confirmed; check DECODEX_SKILL_ROOTS and native support",
			),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde_json::{Value, json};
	use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

	#[tokio::test]
	async fn runtime_skill_roots_are_explicit_and_reapplied_to_each_connection() {
		let paths =
			std::env::join_paths(["/runtime/shared skills", "/runtime/team", "/runtime/team"])
				.unwrap();
		let roots = RuntimeSkillRoots::parse(Some(&paths)).unwrap();
		assert!(RuntimeSkillRoots::parse(Some(OsStr::new("relative"))).is_err());
		for refused in [false, true] {
			let (local, remote) = tokio::io::duplex(4096);
			let (r, w) = tokio::io::split(local);
			let (client, _events) = AppServerClient::from_io(r, w);
			let server = tokio::spawn(async move {
				let (r, mut w) = tokio::io::split(remote);
				let mut lines = BufReader::new(r).lines();
				let request: Value =
					serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
				assert_eq!(request["method"], "skills/extraRoots/set");
				assert_eq!(
					request["params"],
					json!({"extraRoots":["/runtime/shared skills","/runtime/team"]})
				);
				let response = if refused {
					json!({"id":request["id"],"error":{"code":-32601,"message":"unsupported"}})
				} else {
					json!({"id":request["id"],"result":{}})
				};
				w.write_all(format!("{response}\n").as_bytes()).await.unwrap();
			});
			// An absent setting makes no native request, so the first request is the explicit
			// setup.
			RuntimeSkillRoots::parse(None).unwrap().apply(&client).await.unwrap();
			assert_eq!(roots.apply(&client).await.is_err(), refused);
			server.await.unwrap();
		}
		assert_eq!(RuntimeSkillRoots::parse(Some(OsStr::new(""))).unwrap().0, Some(vec![]));
	}
}
