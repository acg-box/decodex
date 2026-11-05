//! Own one local RAGFlow nightly stack. Docker owns its process lifecycle.
//!
//! Configuration follows infiniflow/ragflow at 2c4bbbc720fe24bd7ac509ab989325d4c1e8e532.
//! Only the API is published, on loopback. Uninstall retains the data volumes.
use std::{
	fs,
	io::{ErrorKind, Read as _, Write as _},
	net::TcpListener,
	os::unix::fs::PermissionsExt as _,
	path::{Path, PathBuf},
	process::Command,
	thread,
	time::{Duration, Instant},
};

use reqwest::{Url, blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::{MemorySetup, response};
use crate::{ComponentStore, Result};

const PROVIDER: &str = "OpenAI-API-Compatible";

const CONTAINER_REQUIRED: &str =
	"RAGFlow requires a running Docker-compatible container environment with Docker Compose.";

/// Independent extraction and embedding APIs. Kept in private storage.
#[derive(Clone, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct Models {
	/// Model API used to extract facts from messages.
	pub extraction: ModelEndpoint,
	/// Model API used to embed saved memories.
	pub embedding: ModelEndpoint,
}

/// One OpenAI-compatible model API and its optional private credential.
#[derive(Clone, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct ModelEndpoint {
	/// Base URL of this model API.
	pub base_url: String,
	/// Optional private API key for this endpoint.
	pub api_key: String,
	/// Provider's model identifier, retained verbatim.
	pub model: String,
}

impl Models {
	/// Check both endpoints and their required model names.
	pub fn validate(&self) -> Result<()> {
		self.extraction.validate("extraction")?;
		self.embedding.validate("embedding")
	}
}

impl ModelEndpoint {
	fn validate(&self, role: &str) -> Result<()> {
		let url = Url::parse(self.base_url.trim())
			.map_err(|_| format!("Enter a valid {role} model Base URL."))?;
		if !matches!(url.scheme(), "http" | "https")
			|| url.host_str().is_none()
			|| !url.username().is_empty()
			|| url.password().is_some()
			|| url.query().is_some()
			|| url.fragment().is_some()
		{
			return Err(format!(
				"Use an HTTP or HTTPS {role} model Base URL without credentials or a query."
			));
		}
		if self.model.trim().is_empty() {
			return Err(format!("Enter the {role} model name."));
		}
		Ok(())
	}

	fn service_url(&self) -> String {
		let mut url = Url::parse(self.base_url.trim()).expect("validated model URL");
		if matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")) {
			url.set_host(Some("host.docker.internal")).expect("Docker host name");
		}
		url.to_string().trim_end_matches('/').into()
	}
}

/// An operation on the owned RAGFlow stack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
	/// Prepare the managed stack.
	Install,
	/// Update the stack and retain its saved data.
	Update,
	/// Stop the stack and retain its data volumes.
	Uninstall,
}

/// Private local receipt. Credentials never enter service settings or model tools.
#[derive(Clone, Deserialize, Serialize)]
pub struct Installation {
	/// Loopback endpoint of the owned RAGFlow API.
	pub url: String,
	/// Version reported by the installed service.
	pub version: String,
	/// Identity of the owned RAGFlow image.
	pub image: String,
	/// Dedicated Memory setup, when configuration is complete.
	pub memory: Option<MemorySetup>,
	/// Saved private model configuration, when supplied.
	#[serde(default)]
	pub models: Option<Models>,
	project: String,
	password: String,
	/// Whether the owned stack is installed.
	pub installed: bool,
}

impl Installation {
	/// Check the installed service's public API without sending credentials or model work.
	pub fn is_running(&self) -> bool {
		self.installed
			&& Client::builder()
				.timeout(Duration::from_secs(3))
				.redirect(Policy::none())
				.build()
				.is_ok_and(|client| {
					response(client.get(format!("{}/api/v1/system/version", self.url))).is_ok()
				})
	}
}

impl ComponentStore {
	/// Verify and save models, then create or recover the scoped Memory.
	pub fn configure_ragflow_models(
		&self,
		models: Models,
		namespace: &str,
	) -> Result<Installation> {
		models.validate()?;
		let directory = self.root.join("ragflow/service");
		let mut config = read(&directory)?
			.filter(|i| i.installed)
			.ok_or("Install RAGFlow before configuring its models.")?;
		let client = service_client()?;
		let token = account_token(&client, &config)?;
		// Native Memory owns whether an existing embedding choice can change.
		if let Some(memory) = &config.memory {
			let messages = response(
				client
					.get(format!("{}/api/v1/memories/{}?page_size=1", config.url, memory.memory_id))
					.header("Authorization", &token),
			)?;
			if messages["messages"]["total_count"]
				.as_u64()
				.ok_or("RAGFlow returned an invalid Memory message count.")?
				> 0
				&& config.models.as_ref().is_none_or(|old| {
					old.embedding.model != models.embedding.model
						|| old.embedding.base_url != models.embedding.base_url
				}) {
				return Err("This Memory already contains messages. Keep its embedding model and Base URL to preserve the saved index.".into());
			}
		}
		configure_models(&client, &config, &token, &models)?;
		let key = api_key(&client, &config.url, &token)?;
		let memory = self.setup_ragflow(&config.url, &key, namespace)?;
		response(
			client
				.put(format!("{}/api/v1/memories/{}", config.url, memory.memory_id))
				.header("Authorization", &token)
				.json(&json!({
				 "llm_id":model_id(&models.extraction.model, "decodex-extraction"),
				 "embd_id":model_id(&models.embedding.model, "decodex-embedding")
				})),
		)?;
		config.memory = Some(memory);
		config.models = Some(models);
		write(&directory, &config)?;
		Ok(config)
	}

	/// Read the private receipt for the owned service.
	pub fn ragflow_installation(&self) -> Result<Option<Installation>> {
		read(&self.root.join("ragflow/service"))
	}

	/// Install or update the owned service and reuse its saved model configuration.
	pub fn apply_ragflow(
		&self,
		action: Action,
		namespace: &str,
		mut progress: impl FnMut(&str),
	) -> Result<Option<Installation>> {
		let docker = [
			"/usr/local/bin/docker",
			"/opt/homebrew/bin/docker",
			"/Applications/Docker.app/Contents/Resources/bin/docker",
		]
		.into_iter()
		.map(PathBuf::from)
		.find(|p| p.is_file())
		.ok_or(CONTAINER_REQUIRED)?;
		self.apply_ragflow_with(&docker, action, namespace, &mut progress)
	}

	fn apply_ragflow_with(
		&self,
		docker: &Path,
		action: Action,
		namespace: &str,
		progress: &mut impl FnMut(&str),
	) -> Result<Option<Installation>> {
		let directory = self.root.join("ragflow/service");
		let saved = read(&directory)?;
		if action == Action::Uninstall && saved.is_none() {
			return Ok(None);
		}
		progress("Checking the container environment…");
		for args in [&["info", "--format", "{{.ServerVersion}}"][..], &["compose", "version"][..]] {
			if !Command::new(docker).args(args).output().is_ok_and(|o| o.status.success()) {
				return Err(CONTAINER_REQUIRED.into());
			}
		}
		let mut config = if let Some(saved) = saved {
			saved
		} else {
			let port = TcpListener::bind("127.0.0.1:0")
				.and_then(|listener| listener.local_addr())
				.map_err(|_| "Could not select a local RAGFlow port.")?
				.port();
			let hash = Sha256::digest(self.root.to_string_lossy().as_bytes())
				.iter()
				.map(|b| format!("{b:02x}"))
				.collect::<String>();
			let mut bytes = [0; 32];
			fs::File::open("/dev/urandom")
				.and_then(|mut f| f.read_exact(&mut bytes))
				.map_err(|_| "Could not create the private RAGFlow credentials.")?;
			Installation {
				url: format!("http://127.0.0.1:{port}"),
				project: format!("decodex-ragflow-{}", &hash[..12]),
				password: format!(
					"dcx-{}",
					bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
				),
				image: String::new(),
				version: "nightly".into(),
				memory: None,
				models: None,
				installed: false,
			}
		};
		let run = |args: &[&str]| {
			let mut command = Command::new(docker);
			command
				.args(["compose", "--project-name", &config.project, "--file"])
				.arg(directory.join("compose.json"))
				.args(args);
			run_command(&mut command, &config.password)
		};
		if action == Action::Uninstall {
			progress("Uninstalling RAGFlow…");
			run(&["down", "--remove-orphans"])?;
			config.installed = false;
			write(&directory, &config)?;
			return Ok(Some(config));
		}
		progress("Preparing RAGFlow…");
		fs::create_dir_all(&directory).map_err(|_| "Could not prepare RAGFlow storage.")?;
		fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
			.map_err(|_| "Could not protect RAGFlow storage.")?;
		prepare(&directory, &config)?;
		// Keep the owned identity before effects so retries and Uninstall use the same stack.
		config.installed = true;
		write(&directory, &config)?;
		progress("Downloading RAGFlow nightly…");
		run(&["pull", "--ignore-buildable"])?;
		progress("Starting RAGFlow and Infinity…");
		run(&[
			"up",
			"--detach",
			"--build",
			"--force-recreate",
			"--remove-orphans",
			"--wait",
			"--wait-timeout",
			"600",
		])?;

		progress("Configuring Assistant Memory…");
		let client = service_client()?;
		let deadline = Instant::now() + Duration::from_secs(600);
		loop {
			if client
				.get(format!("{}/api/v1/system/healthz", config.url))
				.send()
				.is_ok_and(|r| r.status().is_success())
			{
				break;
			}
			if Instant::now() >= deadline {
				return Err("RAGFlow did not become ready. Check Docker, then retry Update.".into());
			}
			thread::sleep(Duration::from_secs(1));
		}

		let version = response(client.get(format!("{}/api/v1/system/version", config.url)))?;
		let reported_version = version
			.as_str()
			.or_else(|| version["version"].as_str())
			.filter(|s| !s.is_empty())
			.ok_or("RAGFlow did not report its installed version.")?
			.to_owned();
		config.version = reported_version;
		config.image = run(&["images", "--quiet", "ragflow"])?.trim().into();
		write(&directory, &config)?;
		account_token(&client, &config)?;
		if let Some(models) = config.models.clone() {
			return self.configure_ragflow_models(models, namespace).map(Some);
		}
		Ok(Some(config))
	}
}

fn read(directory: &Path) -> Result<Option<Installation>> {
	match fs::read(directory.join("installation.json")) {
		Ok(bytes) => serde_json::from_slice(&bytes)
			.map(Some)
			.map_err(|_| "Could not read the RAGFlow installation.".into()),
		Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
		Err(_) => Err("Could not read the RAGFlow installation.".into()),
	}
}

fn private_file(path: &Path, bytes: &[u8]) -> Result<()> {
	let mut file =
		tempfile::NamedTempFile::new_in(path.parent().ok_or("Invalid component directory.")?)
			.map_err(|_| "Could not save the RAGFlow installation.")?;
	file.write_all(bytes).map_err(|_| "Could not save the RAGFlow installation.")?;
	file.persist(path).map_err(|_| "Could not save the RAGFlow installation.")?;
	Ok(())
}

fn write(directory: &Path, config: &Installation) -> Result<()> {
	private_file(
		&directory.join("installation.json"),
		&serde_json::to_vec(config).map_err(|_| "Could not save the RAGFlow installation.")?,
	)
}

fn run_command(command: &mut Command, password: &str) -> Result<String> {
	let output = command.output().map_err(|_| CONTAINER_REQUIRED)?;
	if !output.status.success() {
		let detail = String::from_utf8_lossy(&output.stderr).replace(password, "[private]");
		let lines = detail.lines().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>();
		let detail = lines[lines.len().saturating_sub(4)..].join("\n");
		return Err(format!(
			"RAGFlow setup failed. {}",
			detail.chars().take(1500).collect::<String>()
		));
	}
	Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn service_client() -> Result<Client> {
	Client::builder()
		.timeout(Duration::from_secs(30))
		.redirect(Policy::none())
		.build()
		.map_err(|_| "Could not prepare the RAGFlow connection.".into())
}

fn account_token(client: &Client, config: &Installation) -> Result<String> {
	let url = &config.url;
	let email = format!("{}@decodex.local", config.project);
	let login = || {
		client
			.post(format!("{url}/api/v1/auth/login"))
			.json(&json!({"email":email,"password":config.password}))
			.send()
	};
	let mut reply = login().map_err(|_| "Could not initialize the local RAGFlow account.")?;
	let token = if let Some(token) = reply.headers().get("Authorization") {
		token.to_str().map_err(|_| "Invalid RAGFlow account response.")?.to_owned()
	} else {
		reply = client
			.post(format!("{url}/api/v1/users"))
			.json(&json!({"email":email,"password":config.password,"nickname":"Decodex Assistant"}))
			.send()
			.map_err(|_| "Could not initialize the local RAGFlow account.")?;
		reply
			.headers()
			.get("Authorization")
			.and_then(|s| s.to_str().ok())
			.ok_or("Could not initialize the local RAGFlow account. Retry Update.")?
			.to_owned()
	};
	Ok(token)
}

fn model_id(name: &str, instance: &str) -> String {
	format!("{name}@{instance}@{PROVIDER}")
}

fn configure_models(
	client: &Client,
	config: &Installation,
	token: &str,
	models: &Models,
) -> Result<()> {
	let url = &config.url;
	let auth = |r: reqwest::blocking::RequestBuilder| r.header("Authorization", token);
	response(
		auth(client.put(format!("{url}/api/v1/providers")))
			.json(&json!({"provider_name":PROVIDER})),
	)?;
	let instances =
		response(auth(client.get(format!("{url}/api/v1/providers/{PROVIDER}/instances"))))?;
	let instances = instances.as_array().ok_or("Invalid RAGFlow model configuration.")?;
	// Each model owns its endpoint and credential under a stable role name.
	let endpoints = [
		("decodex-extraction", "chat", "extraction", &models.extraction),
		("decodex-embedding", "embedding", "embedding", &models.embedding),
	];
	// Verify both choices before changing either saved provider instance.
	for (_, kind, role, model) in endpoints {
		response(
			auth(client.post(format!("{url}/api/v1/providers/{PROVIDER}/connection")))
				.json(&json!({"api_key":model.api_key,"base_url":model.service_url(),
			 "model_info":[{"model_name":model.model,"model_type":[kind]}]})),
		)
		.map_err(|_| {
			format!(
				"Could not verify the {role} model. Check its Base URL, API key and model name."
			)
		})?;
	}
	for (instance, kind, _, model) in endpoints {
		let exists = instances.iter().any(|i| i["instance_name"] == instance);
		let path = format!("{url}/api/v1/providers/{PROVIDER}/instances");
		let request =
			if exists { client.put(format!("{path}/{instance}")) } else { client.post(path) };
		response(auth(request).json(&json!({
		 "instance_name":instance,"api_key":model.api_key,"clear_api_key":model.api_key.is_empty(),
		 "base_url":model.service_url(),"model_info":[{"model_name":model.model,"model_type":[kind]}]
		})))?;
	}
	let defaults = response(auth(client.get(format!("{url}/api/v1/users/me/models"))))?;
	response(auth(client.patch(format!("{url}/api/v1/users/me/models"))).json(&json!({
	 "tenant_id":defaults["tenant_id"],"asr_id":defaults["asr_id"],"img2txt_id":defaults["img2txt_id"],
	 "llm_id":model_id(&models.extraction.model, "decodex-extraction"),
	 "embd_id":model_id(&models.embedding.model, "decodex-embedding")
	})))?;
	Ok(())
}

fn api_key(client: &Client, url: &str, token: &str) -> Result<String> {
	let auth = |r: reqwest::blocking::RequestBuilder| r.header("Authorization", token);
	let keys = response(auth(client.get(format!("{url}/api/v1/system/tokens"))))?;
	if let Some(key) =
		keys.as_array().and_then(|keys| keys.first()).and_then(|key| key["token"].as_str())
	{
		return Ok(key.into());
	}
	let created = response(
		auth(client.post(format!("{url}/api/v1/system/tokens")))
			.json(&json!({"name":"Decodex Assistant"})),
	)?;
	created["token"]
		.as_str()
		.map(str::to_owned)
		.ok_or("Could not configure the RAGFlow API credential.".into())
}

fn prepare(directory: &Path, config: &Installation) -> Result<()> {
	let runtime = directory.join("infinity-runtime");
	fs::create_dir_all(&runtime).map_err(|_| "Could not prepare the Infinity runtime.")?;
	private_file(
		&runtime.join("Dockerfile"),
		include_bytes!("../resources/ragflow-infinity.Dockerfile"),
	)?;
	let secret = &config.password;
	let service = format!(
		"general:\n  heartbeat_interval: 3s\napi:\n  host: 0.0.0.0\n  http_port: 9380\nmysql:\n  name: rag_flow\n  user: root\n  password: '{secret}'\n  host: mysql\n  port: 3306\nminio:\n  user: decodex\n  password: '{secret}'\n  host: 'minio:9000'\ninfinity:\n  uri: 'infinity:23817'\n  postgres_port: 5432\n  db_name: default_db\nkvrocks:\n  db: 1\n  password: '{secret}'\n  host: 'kvrocks:6379'\nnats:\n  host: nats\n  port: 4222\ningestor:\n  mq_type: nats\n  concurrent_workers: 1\n  page_concurrency: 2\notel:\n  enable: false\n"
	);
	private_file(&directory.join("service_conf.yaml.template"), service.as_bytes())?;
	private_file(&directory.join("init.sql"), b"CREATE DATABASE IF NOT EXISTS rag_flow;\n")?;
	private_file(
		&directory.join("kvrocks.conf"),
		format!("bind 0.0.0.0\nport 6379\ndir /var/lib/kvrocks\nrequirepass {secret}\n").as_bytes(),
	)?;
	private_file(
		&directory.join("infinity.toml"),
		include_bytes!("../resources/ragflow-infinity.toml"),
	)?;
	private_file(
		&directory.join("compose.json"),
		&serde_json::to_vec_pretty(&compose(config))
			.map_err(|_| "Could not prepare RAGFlow services.")?,
	)
}

fn compose(config: &Installation) -> Value {
	let secret = &config.password;
	let port = config.url.rsplit(':').next().expect("generated local URL");
	let health = |test: Value| json!({"test":test,"interval":"5s","timeout":"5s","retries":120});
	let mut value = json!({"services":{
		"ragflow":{
			"image":"infiniflow/ragflow:nightly", "command":["--disable-syncer"],
			"ports":[format!("127.0.0.1:{port}:9380")],
			"volumes":["./service_conf.yaml.template:/ragflow/conf/service_conf.yaml.template:ro"],
			"environment":{"DOC_ENGINE":"infinity","DB_TYPE":"mysql","ENABLE_REGISTER":"1","REGISTER_ENABLED":"1","KVROCKS_HOST":"kvrocks","KVROCKS_PORT":"6379"},
			"depends_on":{"mysql":{"condition":"service_healthy"},"infinity":{"condition":"service_healthy"},"kvrocks":{"condition":"service_healthy"},"nats":{"condition":"service_healthy"},"minio":{"condition":"service_healthy"}}
		},
		"mysql":{"image":"mysql:8.0.40","environment":{"MYSQL_ROOT_PASSWORD":secret},"command":["--max-allowed-packet=1073741824","--character-set-server=utf8mb4","--collation-server=utf8mb4_unicode_ci","--init-file=/data/init.sql"],"volumes":["mysql:/var/lib/mysql","./init.sql:/data/init.sql:ro"],"healthcheck":health(json!(["CMD","mysqladmin","ping","-uroot",format!("-p{secret}")]))},
		"infinity":{"image":if cfg!(target_arch="aarch64"){"infiniflow/infinity:v0.7.4-arm64"}else{"infiniflow/infinity:v0.7.4-x64-v3"},"command":["-f","/infinity.toml"],"volumes":["infinity:/var/infinity","./infinity.toml:/infinity.toml:ro"],"healthcheck":health(json!(["CMD","curl","--fail","http://localhost:23820/admin/node/current"]))},
		"minio":{"image":"pgsty/silo:RELEASE.2026-08-06T00-00-00Z","command":["server","/data"],"environment":{"MINIO_ROOT_USER":"decodex","MINIO_ROOT_PASSWORD":secret},"volumes":["minio:/data"],"healthcheck":health(json!(["CMD","curl","--fail","http://localhost:9000/minio/health/live"]))},
		"kvrocks":{"image":"apache/kvrocks:2.16.0","user":"0:0","entrypoint":["kvrocks","-c","/etc/kvrocks/decodex.conf"],"volumes":["kvrocks:/var/lib/kvrocks","./kvrocks.conf:/etc/kvrocks/decodex.conf:ro"],"healthcheck":health(json!(["CMD","redis-cli","-a",secret,"ping"]))},
		"nats":{"image":"nats:2.14.2","command":["-js","-sd","/data"],"volumes":["nats:/data"],"healthcheck":health(json!(["CMD","nats-server","--health-check"]))}
	},"volumes":{"mysql":{},"infinity":{},"minio":{},"kvrocks":{},"nats":{}}});
	for service in value["services"].as_object_mut().expect("service template").values_mut() {
		service["restart"] = json!("unless-stopped");
		service["logging"] =
			json!({"driver":"json-file","options":{"max-size":"10m","max-file":"2"}});
	}
	if cfg!(target_arch = "aarch64") {
		value["services"]["infinity"]["image"] = json!("decodex-ragflow-infinity:0.7.4-arm64");
		value["services"]["infinity"]["build"] = json!({"context":"./infinity-runtime"});
	}
	value
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn unavailable_container_prerequisites_leave_installation_untouched() {
		for fail_at in ["info", "compose"] {
			let root = tempfile::tempdir().unwrap();
			let store = ComponentStore::new(root.path().join("components"));
			let docker = root.path().join("docker");
			fs::write(
				&docker,
				format!(
					"#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.args\"\nif [ \"$1\" = '{fail_at}' ]; then exit 1; fi\n"
				),
			)
			.unwrap();
			fs::set_permissions(&docker, fs::Permissions::from_mode(0o700)).unwrap();
			let error = store
				.apply_ragflow_with(&docker, Action::Install, "fixture", &mut |_| {})
				.err()
				.expect("unavailable prerequisite");
			assert_eq!(error, CONTAINER_REQUIRED);
			assert!(!root.path().join("components").exists());
			let calls = fs::read_to_string(root.path().join("docker.args")).unwrap();
			assert_eq!(calls.lines().count(), if fail_at == "info" { 1 } else { 2 });
			assert!(!calls.contains("pull") && !calls.contains("up"));
		}
	}

	#[test]
	fn managed_stack_uses_ragflow_nightly_with_infinity_and_only_publishes_its_loopback_api() {
		let config = Installation {
			url: "http://127.0.0.1:12345".into(),
			project: "decodex-ragflow-fixture".into(),
			password: "private-fixture".into(),
			image: String::new(),
			version: "nightly".into(),
			memory: None,
			models: None,
			installed: true,
		};
		let services = compose(&config);
		assert_eq!(services["services"].as_object().unwrap().len(), 6);
		assert!(services["volumes"].get("models").is_none());
		assert_eq!(services["services"]["ragflow"]["image"], "infiniflow/ragflow:nightly");
		assert_eq!(services["services"]["ragflow"]["environment"]["DOC_ENGINE"], "infinity");
		for (name, service) in services["services"].as_object().unwrap() {
			if name == "ragflow" {
				assert_eq!(service["ports"], json!(["127.0.0.1:12345:9380"]));
			} else {
				assert!(service.get("ports").is_none());
			}
		}
	}

	#[test]
	fn model_endpoints_keep_custom_paths_and_names_and_route_loopback_to_the_host() {
		for url in ["http://localhost:1234/v1", "http://127.0.0.1:1234/v1", "http://[::1]:1234/v1"]
		{
			let model = ModelEndpoint {
				base_url: url.into(),
				model: "my/custom-model:preview".into(),
				..Default::default()
			};
			model.validate("extraction").unwrap();
			assert_eq!(model.service_url(), "http://host.docker.internal:1234/v1");
			assert_eq!(
				model_id(&model.model, "decodex-extraction"),
				"my/custom-model:preview@decodex-extraction@OpenAI-API-Compatible"
			);
		}
		let model = ModelEndpoint {
			base_url: "https://models.example/api/v1".into(),
			api_key: "fixture-secret".into(),
			model: "custom-model".into(),
		};
		assert_eq!(model.service_url(), model.base_url);
		assert!(
			ModelEndpoint { base_url: "https://user:password@models.example/v1".into(), ..model }
				.validate("embedding")
				.is_err()
		);
	}

	fn model_fixture(url: String) -> Installation {
		Installation {
			url: url.clone(),
			project: "decodex-fixture".into(),
			password: "fixture-password".into(),
			version: "fixture".into(),
			image: "fixture".into(),
			installed: true,
			memory: Some(MemorySetup {
				url,
				credential_id: "fixture".into(),
				user_id: "fixture".into(),
				memory_id: "memory".into(),
			}),
			models: Some(Models {
				extraction: ModelEndpoint {
					base_url: "https://old.example/v1".into(),
					api_key: "old-key".into(),
					model: "old-chat".into(),
				},
				embedding: ModelEndpoint {
					base_url: "https://embedding.example/v1".into(),
					api_key: "embedding-key".into(),
					model: "same-name@q8".into(),
				},
			}),
		}
	}

	fn model_reply(index: usize, header: &str, body: Value, fail_embedding: bool) -> Option<Value> {
		match index {
			0 => {
				assert!(header.starts_with("POST /api/v1/auth/login "));
			},
			1 => {
				assert!(header.starts_with("GET /api/v1/memories/memory?"));
				return Some(json!({"messages":{"total_count":1}}));
			},
			2 => {
				assert!(header.starts_with("PUT /api/v1/providers "));
			},
			3 => return Some(json!([{"instance_name":"decodex-embedding"}])),
			4..=7 => {
				let extraction = index == 4 || index == 6;
				assert_eq!(
					body["base_url"],
					if extraction {
						"https://extraction.example/v1"
					} else {
						"https://embedding.example/v1"
					}
				);
				assert_eq!(
					body["api_key"],
					if extraction { "extraction-key" } else { "embedding-key" }
				);
				assert_eq!(
					body["model_info"],
					json!([{"model_name":"same-name@q8", "model_type":[if extraction {"chat"} else {"embedding"}]}])
				);
				if index <= 5 {
					assert!(
						header.starts_with(
							"POST /api/v1/providers/OpenAI-API-Compatible/connection "
						)
					);
					if index == 5 && fail_embedding {
						return None;
					}
				} else {
					assert_eq!(
						body["instance_name"],
						if extraction { "decodex-extraction" } else { "decodex-embedding" }
					);
					assert!(header.starts_with(if extraction {
						"POST /api/v1/providers/OpenAI-API-Compatible/instances "
					} else {
						"PUT /api/v1/providers/OpenAI-API-Compatible/instances/decodex-embedding "
					}));
				}
			},
			8 => return Some(json!({"tenant_id":"tenant","asr_id":"asr","img2txt_id":"vision"})),
			9 | 12 => {
				assert_eq!(body["llm_id"], "same-name@q8@decodex-extraction@OpenAI-API-Compatible");
				assert_eq!(body["embd_id"], "same-name@q8@decodex-embedding@OpenAI-API-Compatible");
				if index == 9 {
					assert_eq!(body["asr_id"], "asr");
					assert_eq!(body["img2txt_id"], "vision");
				}
			},
			10 => return Some(json!([{"token":"fixture-key"}])),
			11 => {
				let path = header.lines().next().unwrap().split_whitespace().nth(1).unwrap();
				let query = Url::parse(&format!("http://fixture{path}")).unwrap();
				let name =
					query.query_pairs().find(|(key, _)| key == "keywords").unwrap().1.into_owned();
				return Some(json!({"memory_list":[{"name":name,"id":"memory"}]}));
			},
			_ => unreachable!(),
		}
		Some(Value::Null)
	}

	#[test]
	fn independent_models_use_separate_credentials_and_preserve_the_indexed_embedding_identity() {
		for fail_embedding in [false, true] {
			let count = if fail_embedding { 6 } else { 13 };
			let (url, worker) = super::super::tests::server(count, move |index, header, body| {
				model_reply(index, header, body, fail_embedding)
			});
			let root = tempfile::tempdir().unwrap();
			let store = ComponentStore::new(root.path().into());
			let directory = root.path().join("ragflow/service");
			fs::create_dir_all(&directory).unwrap();
			let config = model_fixture(url);
			write(&directory, &config).unwrap();
			let before = fs::read(directory.join("installation.json")).unwrap();
			let mut models = config.models.unwrap();
			models.extraction = ModelEndpoint {
				base_url: "https://extraction.example/v1".into(),
				api_key: "extraction-key".into(),
				model: "same-name@q8".into(),
			};
			let result = store.configure_ragflow_models(models.clone(), "service:assistant");
			if fail_embedding {
				assert_eq!(
					result.err().unwrap(),
					"Could not verify the embedding model. Check its Base URL, API key and model name."
				);
				assert_eq!(fs::read(directory.join("installation.json")).unwrap(), before);
			} else {
				assert!(result.unwrap().models.unwrap() == models);
				assert!(store.ragflow_installation().unwrap().unwrap().models.unwrap() == models);
			}
			worker.join().unwrap();
		}
	}

	#[test]
	fn service_light_checks_the_public_api_without_credentials() {
		let (url, worker) = super::super::tests::server(1, |_, header, _| {
			assert!(header.starts_with("GET /api/v1/system/version "));
			assert!(!header.to_ascii_lowercase().contains("authorization:"));
			Some(json!({"version":"fixture"}))
		});
		let config = model_fixture(url);
		assert!(config.is_running());
		worker.join().unwrap();
		assert!(
			!config.is_running(),
			"an installed receipt alone does not mean the service is running"
		);
	}

	#[test]
	fn changing_an_indexed_embedding_endpoint_or_model_is_rejected_before_provider_changes() {
		for change_url in [false, true] {
			let (url, worker) = super::super::tests::server(2, |index, _, _| {
				Some(if index == 0 { Value::Null } else { json!({"messages":{"total_count":1}}) })
			});
			let root = tempfile::tempdir().unwrap();
			let store = ComponentStore::new(root.path().into());
			let directory = root.path().join("ragflow/service");
			fs::create_dir_all(&directory).unwrap();
			let config = model_fixture(url);
			write(&directory, &config).unwrap();
			let before = fs::read(directory.join("installation.json")).unwrap();
			let mut models = config.models.unwrap();
			if change_url {
				models.embedding.base_url = "https://different.example/v1".into();
			} else {
				models.embedding.model = "different-model".into();
			}
			assert!(
				store
					.configure_ragflow_models(models, "service:assistant")
					.err()
					.unwrap()
					.contains("preserve the saved index")
			);
			assert_eq!(fs::read(directory.join("installation.json")).unwrap(), before);
			worker.join().unwrap();
		}
	}

	#[test]
	fn uninstall_stops_only_the_owned_project_and_keeps_saved_memory() {
		let root = tempfile::tempdir().unwrap();
		let store = ComponentStore::new(root.path().into());
		let directory = root.path().join("ragflow/service");
		fs::create_dir_all(&directory).unwrap();
		let config = Installation {
			url: "http://127.0.0.1:12345".into(),
			project: "decodex-ragflow-fixture".into(),
			password: "private-fixture".into(),
			image: String::new(),
			version: "nightly".into(),
			memory: None,
			models: None,
			installed: true,
		};
		write(&directory, &config).unwrap();
		fs::write(directory.join("memory-data"), "Keep this").unwrap();
		let docker = root.path().join("docker");
		fs::write(&docker, "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"$0.args\"\n").unwrap();
		fs::set_permissions(&docker, fs::Permissions::from_mode(0o700)).unwrap();
		store.apply_ragflow_with(&docker, Action::Uninstall, "fixture", &mut |_| {}).unwrap();
		assert!(!store.ragflow_installation().unwrap().unwrap().installed);
		assert!(directory.join("memory-data").is_file());
		let args = fs::read_to_string(root.path().join("docker.args")).unwrap();
		assert!(args.contains("--project-name decodex-ragflow-fixture"));
		assert!(args.ends_with("down --remove-orphans\n"));
		assert!(!args.contains("--volumes"));
		assert_eq!(
			fs::metadata(directory.join("installation.json")).unwrap().permissions().mode() & 0o777,
			0o600
		);
	}
}
