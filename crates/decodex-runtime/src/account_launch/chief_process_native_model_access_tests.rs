use super::*;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native model access metadata"]
async fn installed_native_model_access_metadata_refreshes_after_cold_restart() {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().unwrap();
	let catalog = home.path().join("models.json");

	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let address = listener.local_addr().unwrap();
	let served_catalog = catalog.clone();
	let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let observed = fetches.clone();
	let server = tokio::spawn(async move {
		use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
		while let Ok((stream, _)) = listener.accept().await {
			let mut stream = tokio::io::BufReader::new(stream);
			let mut line = String::new();
			stream.read_line(&mut line).await.unwrap();
			let models = line
				.split_whitespace()
				.nth(1)
				.unwrap_or("")
				.split('?')
				.next()
				.unwrap()
				.ends_with("/models");
			loop {
				line.clear();
				if stream.read_line(&mut line).await.unwrap() == 0 || line == "\r\n" {
					break;
				}
			}
			let (status, body) = if models {
				observed.fetch_add(1, Ordering::AcqRel);
				("200 OK", std::fs::read(&served_catalog).unwrap())
			} else {
				("404 Not Found", Vec::new())
			};
			let header = format!(
				"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nETag: stable-fixture-etag\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
				body.len()
			);
			if stream.get_mut().write_all(header.as_bytes()).await.is_ok() {
				let _ = stream.get_mut().write_all(&body).await;
			}
		}
	});
	std::fs::write(home.path().join("config.toml"),format!("model_provider=\"fixture\"\nchatgpt_base_url=\"http://{address}\"\ncli_auth_credentials_store=\"file\"\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}/v1\"\nwire_api=\"responses\"\nrequires_openai_auth=true\n")).unwrap();
	std::fs::write(home.path().join("auth.json"),serde_json::to_vec(&json!({"auth_mode":"chatgpt","tokens":{
        "id_token":"e30.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOiB7ImNoYXRncHRfYWNjb3VudF9pZCI6ICJhY2Nlc3MtZml4dHVyZSIsICJjaGF0Z3B0X3VzZXJfaWQiOiAiYWNjZXNzLWZpeHR1cmUiLCAiY2hhdGdwdF9wbGFuX3R5cGUiOiAicHJvIn19.signature",
        "access_token":"synthetic-access-fixture","refresh_token":"synthetic-refresh-fixture","account_id":"access-fixture"
    },"last_refresh":String::from_utf8(Command::new("/bin/date").args(["-u", "+%Y-%m-%dT%H:%M:%SZ"]).output().unwrap().stdout).unwrap().trim()})).unwrap()).unwrap();
	for (native_programs, expected) in [
		(
			json!({"cyber":["standard","daybreak_blue","future_program"]}),
			Some(vec!["standard".to_owned(), "daybreakBlue".to_owned()]),
		),
		(json!({"cyber":[]}), Some(vec![])),
		(Value::Null, None),
	] {
		std::fs::write(&catalog,serde_json::to_vec(&json!({"models":[{
            "slug":"access-fixture","display_name":"Access fixture","description":"Synthetic catalog",
            "available_access_programs":native_programs,
            "default_reasoning_level":"high","supported_reasoning_levels":[{"effort":"high","description":"High"}],
            "shell_type":"shell_command","visibility":"list","minimal_client_version":"0.1.0",
            "supported_in_api":true,"priority":0,"support_verbosity":false,"default_verbosity":null,
            "apply_patch_tool_type":null,"truncation_policy":{"mode":"bytes","limit":10000},
            "supports_image_detail_original":false,"multi_agent_version":"v2","context_window":272000,
            "max_context_window":272000,"experimental_supported_tools":[],
            "model_messages":{"instructions_template":"Synthetic catalog fixture","instructions_variables":null}
        }]})).unwrap()).unwrap();
		let before_fetches = fetches.load(Ordering::Acquire);
		let session = NativeSession::start(&binary, home.path());
		// Startup can serve the existing native cache before its online refresh completes.
		// Observe the refreshed value rather than treating first-read cache data as a grant.
		tokio::time::timeout(Duration::from_secs(8), async {
			loop {
				let result = crate::chief_capabilities::read(&session.client).await;
				if let decodex_protocol::ChiefCapabilitiesResult::Available { models, .. } = result
					&& let Some(model) =
						models.iter().find(|model| model.model.as_str() == "access-fixture")
					&& model.available_cyber_programs == expected
					&& fetches.load(Ordering::Acquire) > before_fetches
				{
					break;
				}
				tokio::time::sleep(Duration::from_millis(20)).await;
			}
		})
		.await
		.expect("native catalog must publish refreshed access metadata");
		let saved: Value =
			serde_json::from_slice(&std::fs::read(home.path().join("models_cache.json")).unwrap())
				.unwrap();
		assert_eq!(
			saved["models"][0]["available_access_programs"],
			if native_programs.is_null() {
				Value::Null
			} else if expected.as_ref().unwrap().is_empty() {
				json!({"cyber":[]})
			} else {
				json!({"cyber":["standard","daybreak_blue"]})
			}
		);
	}
	server.abort();
}
