//! Explicit offline development reset. Native Codex owns deletion of its contexts.
//! No model turn is submitted. Run --plan before --apply and stop Decodex first.
#![allow(unused_crate_dependencies)]

#[cfg(target_os = "macos")]
mod operation {
	use decodex_codex::app_server_client::{AppServerClient, ClientError};
	use decodex_components::ComponentStore;
	use decodex_core::DecodexRoot;
	use decodex_database::SqliteStore;
	use serde_json::{Value, json};
	use std::{
		collections::{BTreeMap, BTreeSet},
		env,
		error::Error,
		fs,
		io::{BufRead as _, BufReader},
		os::unix::net::UnixStream,
		path::{Path, PathBuf},
	};
	use tokio::process::Command;

	type Result<T> = std::result::Result<T, Box<dyn Error>>;

	fn uuid(id: &str) -> bool {
		id.len() == 36
			&& id.bytes().enumerate().all(|(i, b)| {
				if [8, 13, 18, 23].contains(&i) { b == b'-' } else { b.is_ascii_hexdigit() }
			})
	}

	fn inspect_sessions(
		path: &Path,
		threads: &mut BTreeSet<String>,
		parents: &mut BTreeMap<String, String>,
	) -> Result<()> {
		if !path.exists() {
			return Ok(());
		}
		for entry in fs::read_dir(path)? {
			let entry = entry?;
			let kind = entry.file_type()?;
			if kind.is_dir() {
				inspect_sessions(&entry.path(), threads, parents)?;
			} else if kind.is_file() && entry.path().extension().is_some_and(|s| s == "jsonl") {
				let mut line = String::new();
				BufReader::new(fs::File::open(entry.path())?).read_line(&mut line)?;
				let Ok(meta) = serde_json::from_str::<Value>(&line) else { continue };
				let p = &meta["payload"];
				let Some(id) = p["id"].as_str().filter(|id| uuid(id)) else { continue };
				if p["originator"] == "decodex" {
					threads.insert(id.into());
				}
				if let Some(parent) = p
					.pointer("/history_base/thread_id")
					.or_else(|| p.get("forked_from_id"))
					.and_then(Value::as_str)
				{
					parents.insert(id.into(), parent.into());
				}
			}
		}
		Ok(())
	}

	fn deletion_order(
		threads: &BTreeSet<String>,
		parents: &BTreeMap<String, String>,
	) -> Vec<String> {
		let depth = |id: &String| {
			let mut current = id;
			let mut seen = BTreeSet::new();
			while let Some(parent) = parents.get(current).filter(|p| threads.contains(*p)) {
				if !seen.insert(current) {
					break;
				}
				current = parent;
			}
			seen.len()
		};
		let mut result: Vec<_> = threads.iter().cloned().collect();
		result.sort_by_key(|id| std::cmp::Reverse(depth(id)));
		result
	}

	pub async fn run() -> Result<()> {
		let args: Vec<_> = env::args_os().skip(1).collect();
		if args.len() != 4 || (args[3] != "--plan" && args[3] != "--apply") {
			return Err("Usage: clear_development_data ROOT CODEX_BINARY ASSISTANT_NAMESPACE --plan|--apply".into());
		}
		let apply = args[3] == "--apply";
		let paths = DecodexRoot::new(PathBuf::from(&args[0]))?.paths();
		let namespace = args[2].to_str().ok_or("Invalid namespace")?.to_owned();
		if apply && UnixStream::connect(paths.local_transport_socket()).is_ok() {
			return Err("Stop Decodex before clearing development data.".into());
		}
		let store = SqliteStore::open(&paths)?;
		let registered = store.development_native_threads().await?;
		let mut threads: BTreeSet<_> = registered.iter().filter(|id| uuid(id)).cloned().collect();
		let mut parents = BTreeMap::new();
		let home = env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or(
			PathBuf::from(env::var_os("HOME").ok_or("Home unavailable")?).join(".codex"),
		);
		for directory in ["sessions", "archived_sessions"] {
			inspect_sessions(&home.join(directory), &mut threads, &mut parents)?;
		}
		let component = ComponentStore::for_user()?;
		let installation =
			component.ragflow_installation()?.ok_or("RAGFlow installation not found")?;
		let memory = installation.memory.as_ref().ok_or("Managed Memory is not configured")?;
		if !apply {
			println!(
				"{}",
				json!({"mode":"plan","native_thread_ids":threads,"registered_references":registered.len(),"registered_non_native_references":registered.iter().filter(|id|!uuid(id)).count(),"old_memory_id":memory.memory_id,"preserved":["accounts","credentials","account policy","model defaults","project folders"]})
			);
			return Ok(());
		}
		let mut command = Command::new(&args[1]);
		command
			.arg("app-server")
			.arg("-c")
			.arg("analytics.enabled=false")
			.arg("-c")
			.arg("features.apps=false")
			.arg("-c")
			.arg("features.remote_plugins=false");
		let (client, mut events, mut process) = AppServerClient::spawn(&mut command)?;
		let drain = tokio::spawn(async move { while events.recv().await.is_some() {} });
		let deletion = async {
			client.initialize(json!({"clientInfo":{"name":"decodex-development-reset","version":"0.1.0"},"capabilities":{"experimentalApi":true}})).await?;
			let mut deleted=0;
			let mut absent=0;
			for thread in deletion_order(&threads, &parents) {
				eprintln!("Checking native context: {thread}");
				let read = tokio::time::timeout(std::time::Duration::from_secs(60), client.thread_read(json!({"threadId":thread,"includeTurns":false}))).await?;
				match read {
					Ok(_) => {},
					Err(ClientError::Remote(error)) if error.code == -32600 && (error.message == format!("thread not found: {thread}") || error.message == format!("no rollout found for thread id {thread}") || error.message == format!("thread not loaded: {thread}")) => {},
					Err(ClientError::Remote(error)) => return Err(format!("Native read {thread}: {}", error.message).into()),
					Err(error) => return Err(Box::<dyn Error>::from(error)),
				}
				// Read before a mutation, including after an earlier unknown result.
				// Offline deletion can outlast the eight-second interactive helper.
				let response = match tokio::time::timeout(std::time::Duration::from_secs(60), client.request("thread/delete",json!({"threadId":thread}))).await? {
					Ok(response) => response,
					Err(ClientError::Remote(error)) if error.code == -32600 && (error.message == format!("thread not found: {thread}") || error.message == format!("no rollout found for thread id {thread}")) => {absent+=1;continue;},
					Err(ClientError::Remote(error)) => return Err(format!("Native delete {thread}: {}",error.message).into()),
					Err(error) => return Err(error.into()),
				};
				if !response.as_object().is_some_and(serde_json::Map::is_empty) { return Err("Invalid native deletion receipt".into()); }
				deleted+=1;
				eprintln!("Native contexts processed: {}/{}", deleted+absent, threads.len());
			}
			Ok((deleted,absent))
		}.await;
		process.shutdown().await?;
		drain.abort();
		let (deleted, absent) = deletion?;
		let expected = memory.clone();
		let reset = tokio::task::spawn_blocking(move || {
			ComponentStore::for_user()?.reset_ragflow_memory(&expected, &namespace)
		})
		.await??;
		let removed = store.clear_development_data().await?;
		for directory in [
			paths.blobs_dir(),
			paths.cache_dir(),
			paths.root().as_path().join("client-drafts"),
			paths.root().as_path().join("backups"),
			paths.logs_dir(),
		] {
			if directory.exists() {
				fs::remove_dir_all(directory)?;
			}
		}
		paths.ensure_layout()?;
		println!(
			"{}",
			json!({"mode":"applied","native_contexts_deleted":deleted,"native_contexts_already_absent":absent,"removed_rows":removed,"memory":reset.memory,"model_calls":0})
		);
		Ok(())
	}

	#[cfg(test)]
	mod tests {
		use super::*;
		#[test]
		fn dependent_contexts_are_deleted_before_their_sources() {
			let threads = ["parent", "child", "grandchild"].map(String::from).into_iter().collect();
			let parents =
				[("child".into(), "parent".into()), ("grandchild".into(), "child".into())]
					.into_iter()
					.collect();
			assert_eq!(deletion_order(&threads, &parents), ["grandchild", "child", "parent"]);
		}
	}
}

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	operation::run().await
}

#[cfg(not(target_os = "macos"))]
fn main() {
	panic!("This development reset requires macOS component storage.");
}
