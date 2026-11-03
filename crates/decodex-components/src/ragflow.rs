//! Configure an existing RAGFlow service as an optional memory component.
use std::{
	fs,
	io::{Read as _, Write as _},
	os::unix::fs::PermissionsExt as _,
	time::Duration,
};

use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::{ComponentStore, Result};

/// A configured private Memory. It contains no API key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemorySetup {
	/// Normalized service origin.
	pub url: String,
	/// Component-owned credential reference.
	pub credential_id: String,
	/// Dedicated namespace for this local Assistant.
	pub user_id: String,
	/// Memory created or recovered by its stable name.
	pub memory_id: String,
}

#[derive(Deserialize, Serialize)]
struct Credential {
	url: String,
	api_key: String,
}

/// Accept HTTPS services and loopback HTTP. Credentials stay on the selected origin.
pub fn origin(value: &str) -> Result<Url> {
	let url = Url::parse(value.trim()).map_err(|_| "Enter a RAGFlow service address.")?;
	let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
	if !(url.scheme() == "https" || (url.scheme() == "http" && local))
		|| url.host_str().is_none()
		|| !url.username().is_empty()
		|| url.password().is_some()
		|| url.query().is_some()
		|| url.fragment().is_some()
		|| url.path() != "/"
	{
		return Err("Use an HTTPS service address or local HTTP address.".into());
	}
	Ok(url)
}

impl ComponentStore {
	/// Read one component credential, bound to the same origin used during setup.
	pub fn ragflow_key(&self, id: &str, url: &str) -> Result<String> {
		if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
			return Err("Reconnect the RAGFlow component.".into());
		}
		let bytes = fs::read(self.root.join("ragflow").join(id))
			.map_err(|_| "Reconnect the RAGFlow component.")?;
		let saved: Credential =
			serde_json::from_slice(&bytes).map_err(|_| "Reconnect the RAGFlow component.")?;
		if origin(&saved.url)? != origin(url)? {
			return Err("Reconnect the RAGFlow component.".into());
		}
		Ok(saved.api_key)
	}

	/// Create or recover a dedicated Memory using the service's configured default models.
	/// No chat content is sent. The caller must separately enable any chat import.
	pub fn setup_ragflow(&self, url: &str, key: &str, namespace: &str) -> Result<MemorySetup> {
		let base = origin(url)?;
		if key.trim().is_empty() {
			return Err("Enter the RAGFlow API key.".into());
		}
		let client = Client::builder()
			.timeout(Duration::from_secs(10))
			.redirect(Policy::none())
			.build()
			.map_err(|_| "Could not connect to RAGFlow.")?;
		let id = Sha256::digest(format!("{base}\n{namespace}"))
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect::<String>();
		let name = format!("Decodex {}", &id[..24]);
		let mut list_url = base.join("api/v1/memories").map_err(|_| "Invalid service address.")?;
		list_url.query_pairs_mut().append_pair("keywords", &name).append_pair("page_size", "10");
		let listed = response(client.get(list_url).bearer_auth(key))?;
		let memories =
			listed["memory_list"].as_array().ok_or("RAGFlow returned an invalid Memory list.")?;
		let existing = memories.iter().find(|m| m["name"] == name);
		let memory_id = if let Some(memory) = existing {
			memory["id"]
				.as_str()
				.filter(|s| !s.is_empty())
				.ok_or("RAGFlow returned an invalid Memory.")?
				.to_owned()
		} else {
			let models = response(
				client
					.get(
						base.join("api/v1/users/me/models")
							.map_err(|_| "Invalid service address.")?,
					)
					.bearer_auth(key),
			)?;
			let embedding = models["embd_id"].as_str().filter(|s| !s.trim().is_empty());
			let chat = models["llm_id"].as_str().filter(|s| !s.trim().is_empty());
			let (Some(embedding), Some(chat)) = (embedding, chat) else {
				return Err(
					"Select the default chat and embedding models in RAGFlow, then retry setup."
						.into(),
				);
			};
			let created = response(
				client
					.post(base.join("api/v1/memories").map_err(|_| "Invalid service address.")?)
					.bearer_auth(key)
					.json(&json!({
						"name":name,"memory_type":["raw","semantic"],"embd_id":embedding,"llm_id":chat
					})),
			)?;
			created["id"]
				.as_str()
				.filter(|s| !s.is_empty())
				.ok_or("Memory creation was not confirmed. Retry setup to check its saved name.")?
				.to_owned()
		};
		let directory = self.root.join("ragflow");
		fs::create_dir_all(&directory).map_err(|_| "Could not save the RAGFlow connection.")?;
		fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
			.map_err(|_| "Could not protect the RAGFlow connection.")?;
		let mut file = tempfile::NamedTempFile::new_in(&directory)
			.map_err(|_| "Could not save the RAGFlow connection.")?;
		serde_json::to_writer(
			&mut file,
			&Credential { url: base.to_string(), api_key: key.into() },
		)
		.map_err(|_| "Could not save the RAGFlow connection.")?;
		file.flush().map_err(|_| "Could not save the RAGFlow connection.")?;
		file.persist(directory.join(&id)).map_err(|_| "Could not save the RAGFlow connection.")?;
		Ok(MemorySetup {
			url: base.to_string(),
			credential_id: id.clone(),
			user_id: format!("decodex-{id}"),
			memory_id,
		})
	}
}

fn response(request: reqwest::blocking::RequestBuilder) -> Result<Value> {
	let reply =
		request.send().map_err(|_| "Could not reach RAGFlow. Check its service address.")?;
	if !reply.status().is_success() {
		return Err(format!(
			"RAGFlow connection failed (HTTP {}). Check the service and API key.",
			reply.status().as_u16()
		));
	}
	let mut bytes = Vec::new();
	reply
		.take(65_537)
		.read_to_end(&mut bytes)
		.map_err(|_| "Could not read the RAGFlow response.")?;
	if bytes.len() > 65_536 {
		return Err("The RAGFlow response is too large.".into());
	}
	let reply: Value =
		serde_json::from_slice(&bytes).map_err(|_| "RAGFlow returned an invalid response.")?;
	if reply["code"] != 0 {
		return Err(
			"RAGFlow rejected the request. Check the API key, Memory support and default models."
				.into(),
		);
	}
	Ok(reply["data"].clone())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{net::TcpListener, thread};

	fn server(
		count: usize,
		mut reply: impl FnMut(usize, &str, Value) -> Option<Value> + Send + 'static,
	) -> (String, thread::JoinHandle<()>) {
		let listener = TcpListener::bind("127.0.0.1:0").unwrap();
		let address = format!("http://{}", listener.local_addr().unwrap());
		let worker = thread::spawn(move || {
			for index in 0..count {
				let (mut stream, _) = listener.accept().unwrap();
				stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
				let mut bytes = Vec::new();
				let mut part = [0; 4096];
				let (header, body) = loop {
					let n = stream.read(&mut part).unwrap();
					assert!(n > 0);
					bytes.extend_from_slice(&part[..n]);
					if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
						let header = String::from_utf8(bytes[..end].to_vec()).unwrap();
						let length = header
							.lines()
							.find_map(|line| {
								line.to_ascii_lowercase()
									.strip_prefix("content-length: ")
									.map(|v| v.parse::<usize>().unwrap())
							})
							.unwrap_or(0);
						if bytes.len() >= end + 4 + length {
							break (
								header,
								if length == 0 {
									Value::Null
								} else {
									serde_json::from_slice(&bytes[end + 4..end + 4 + length])
										.unwrap()
								},
							);
						}
					}
				};
				assert!(header.to_ascii_lowercase().contains("authorization: bearer fixture-key"));
				if let Some(data) = reply(index, &header, body) {
					let body = json!({"code":0,"data":data}).to_string();
					stream
						.write_all(
							format!(
								"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
								body.len()
							)
							.as_bytes(),
						)
						.unwrap();
				}
			}
		});
		(address, worker)
	}

	#[test]
	fn setup_recovers_a_lost_creation_reply_and_saves_an_origin_bound_private_credential() {
		let mut saved_name = String::new();
		let (address, worker) = server(4, move |index, header, body| match index {
			0 => {
				assert!(header.starts_with("GET /api/v1/memories?"));
				Some(json!({"memory_list":[]}))
			},
			1 => {
				assert!(header.starts_with("GET /api/v1/users/me/models "));
				Some(json!({"embd_id":"embed@provider","llm_id":"chat@provider"}))
			},
			2 => {
				assert!(header.starts_with("POST /api/v1/memories "));
				assert_eq!(body["embd_id"], "embed@provider");
				assert_eq!(body["llm_id"], "chat@provider");
				saved_name = body["name"].as_str().unwrap().into();
				None
			},
			3 => {
				assert!(header.starts_with("GET /api/v1/memories?"));
				Some(
					json!({"memory_list":[{"id":"unrelated","name":"Another Memory"},{"id":"recovered","name":saved_name}]}),
				)
			},
			_ => unreachable!(),
		});
		let directory = tempfile::tempdir().unwrap();
		let store = ComponentStore::new(directory.path().into());
		assert!(store.setup_ragflow(&address, "fixture-key", "service:assistant").is_err());
		assert!(!directory.path().join("ragflow").exists());
		let setup = store.setup_ragflow(&address, "fixture-key", "service:assistant").unwrap();
		assert_eq!(setup.memory_id, "recovered");
		assert_eq!(store.ragflow_key(&setup.credential_id, &address).unwrap(), "fixture-key");
		assert!(store.ragflow_key(&setup.credential_id, "https://another.example").is_err());
		assert_eq!(
			fs::metadata(directory.path().join("ragflow")).unwrap().permissions().mode() & 0o777,
			0o700
		);
		assert_eq!(
			fs::metadata(directory.path().join("ragflow").join(&setup.credential_id))
				.unwrap()
				.permissions()
				.mode()
				& 0o777,
			0o600
		);
		worker.join().unwrap();
	}

	#[test]
	fn setup_with_no_default_models_does_not_create_memory_or_save_credentials() {
		let (address, worker) = server(2, |index, _, _| {
			Some(if index == 0 {
				json!({"memory_list":[]})
			} else {
				json!({"embd_id":"","llm_id":""})
			})
		});
		let directory = tempfile::tempdir().unwrap();
		let store = ComponentStore::new(directory.path().into());
		let error = store.setup_ragflow(&address, "fixture-key", "service:assistant").unwrap_err();
		assert!(error.contains("default chat and embedding models"));
		assert!(!error.contains("fixture-key"));
		assert!(!directory.path().join("ragflow").exists());
		worker.join().unwrap();
	}
}
