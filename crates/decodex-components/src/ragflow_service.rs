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

use reqwest::{blocking::Client, redirect::Policy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use super::{MemorySetup, response};
use crate::{ComponentStore, Result};

const CHAT: &str = "qwen3:1.7b";
const EMBEDDING: &str = "bge-m3";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
	Install,
	Update,
	Uninstall,
}

/// Private local receipt. Credentials never enter service settings or model tools.
#[derive(Clone, Deserialize, Serialize)]
pub struct Installation {
	pub url: String,
	pub version: String,
	pub image: String,
	pub memory: Option<MemorySetup>,
	project: String,
	password: String,
	pub installed: bool,
}

impl ComponentStore {
	pub fn ragflow_installation(&self) -> Result<Option<Installation>> {
		read(&self.root.join("ragflow/service"))
	}

	/// Install or update the owned stack and configure usable Memory in the same action.
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
		.ok_or("Install and start Docker or OrbStack, then retry RAGFlow installation.")?;
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
		run_command(
			Command::new(docker).args(["info", "--format", "{{.ServerVersion}}"]),
			&config.password,
		)
		.map_err(|_| "Start Docker or OrbStack, then retry RAGFlow installation.")?;
		fs::create_dir_all(&directory).map_err(|_| "Could not prepare RAGFlow storage.")?;
		fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
			.map_err(|_| "Could not protect RAGFlow storage.")?;
		prepare(&directory, &config)?;
		// Keep the owned identity before effects so retries and Uninstall use the same stack.
		config.installed = true;
		write(&directory, &config)?;
		progress("Downloading RAGFlow nightly and local models…");
		run(&["pull", "--ignore-buildable"])?;
		progress("Starting RAGFlow and Infinity…");
		run(&["up", "--detach", "--build", "--force-recreate", "--wait", "--wait-timeout", "600"])?;

		progress("Configuring Assistant Memory…");
		let client = Client::builder()
			.timeout(Duration::from_secs(10))
			.redirect(Policy::none())
			.build()
			.map_err(|_| "Could not prepare the local RAGFlow connection.")?;
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
		config.version = format!("nightly ({reported_version})");
		config.image = run(&["images", "--quiet", "ragflow"])?.trim().into();
		write(&directory, &config)?;
		let key = bootstrap(&client, &config)?;
		for model in [CHAT, EMBEDDING] {
			progress(&format!("Preparing {model}…"));
			run(&["exec", "-T", "ollama", "ollama", "pull", model]).map_err(|_| {
				format!(
					"Could not download the local model {model}. Check Docker and the network, then retry Update."
				)
			})?;
		}
		config.memory = Some(self.setup_ragflow(&config.url, &key, namespace)?);
		write(&directory, &config)?;
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
	let output = command
		.output()
		.map_err(|_| "Could not run Docker. Start Docker or OrbStack and retry.")?;
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

fn bootstrap(client: &Client, config: &Installation) -> Result<String> {
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
	let auth = |r: reqwest::blocking::RequestBuilder| r.header("Authorization", &token);
	response(
		auth(client.put(format!("{url}/api/v1/providers")))
			.json(&json!({"provider_name":"Ollama"})),
	)?;
	let instances = response(auth(client.get(format!("{url}/api/v1/providers/Ollama/instances"))))?;
	if !instances
		.as_array()
		.ok_or("Invalid RAGFlow model configuration.")?
		.iter()
		.any(|i| i["instance_name"] == "decodex")
	{
		response(auth(client.post(format!("{url}/api/v1/providers/Ollama/instances"))).json(
			&json!({
				"instance_name":"decodex","base_url":"http://ollama:11434","model_info":[
					{"model_name":CHAT,"model_type":["chat"],"max_tokens":8192},
					{"model_name":EMBEDDING,"model_type":["embedding"],"max_tokens":8192}
				]
			}),
		))?;
	}
	let defaults = response(auth(client.get(format!("{url}/api/v1/users/me/models"))))?;
	response(auth(client.patch(format!("{url}/api/v1/users/me/models"))).json(&json!({"tenant_id":defaults["tenant_id"],"asr_id":defaults["asr_id"],"img2txt_id":defaults["img2txt_id"],"llm_id":format!("{CHAT}@decodex@Ollama"),"embd_id":format!("{EMBEDDING}@decodex@Ollama")})))?;
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
		"nats":{"image":"nats:2.14.2","command":["-js","-sd","/data"],"volumes":["nats:/data"],"healthcheck":health(json!(["CMD","nats-server","--health-check"]))},
		"ollama":{"image":"ollama/ollama:latest","volumes":["models:/root/.ollama"],"healthcheck":health(json!(["CMD","ollama","list"]))}
	},"volumes":{"mysql":{},"infinity":{},"minio":{},"kvrocks":{},"nats":{},"models":{}}});
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
	fn managed_stack_uses_ragflow_nightly_with_infinity_and_only_publishes_its_loopback_api() {
		let config = Installation {
			url: "http://127.0.0.1:12345".into(),
			project: "decodex-ragflow-fixture".into(),
			password: "private-fixture".into(),
			image: String::new(),
			version: "nightly".into(),
			memory: None,
			installed: true,
		};
		let services = compose(&config);
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
