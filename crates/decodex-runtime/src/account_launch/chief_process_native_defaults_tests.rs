//! Native project defaults are read at the requested directory, without inference.
use super::*;

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native creation defaults"]
async fn installed_creation_defaults_use_the_requested_trusted_directory() {
	tokio::time::timeout(Duration::from_secs(45), async {
        let binary=std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit binary");
        let home=tempfile::tempdir_in("/tmp").expect("fixture home");
        let root=home.path().canonicalize().expect("canonical home");
        let workspace=root.join("saved-workspace");
        std::fs::create_dir_all(workspace.join(".codex")).expect("project config directory");
        std::fs::create_dir(workspace.join(".git")).expect("project root");
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address=listener.local_addr().expect("address");
        let calls=Arc::new(Mutex::new(Vec::new()));
        let server=tokio::spawn(serve(listener,calls.clone()));
        std::fs::write(root.join("config.toml"),format!("model=\"global-model\"\nmodel_reasoning_effort=\"low\"\nmodel_provider=\"fixture\"\nchatgpt_base_url=\"http://{address}/backend-api\"\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nrequires_openai_auth=true\nsupports_websockets=false\n[projects.{}]\ntrust_level=\"trusted\"\n",json!(workspace))).expect("global config");
        std::fs::write(workspace.join(".codex/config.toml"),"model=\"project-model\"\nmodel_reasoning_effort=\"high\"\nservice_tier=\"flex\"\n").expect("project config");
        let mut session=AuthSession::start(&binary,&root).await;
        let global=session.client.initial_model_defaults(root.to_str().expect("root")).await.expect("global defaults");
        let project=session.client.initial_model_defaults(workspace.to_str().expect("workspace")).await.expect("project defaults");
        assert_eq!(global.configured.model.as_deref(),Some("global-model"));
        assert_eq!(global.configured.reasoning_effort.as_deref(),Some("low"));
        assert_eq!(project.configured.model.as_deref(),Some("project-model"));
        assert_eq!(project.configured.reasoning_effort.as_deref(),Some("high"));
        assert_eq!(project.configured.service_tier.as_deref(),Some("flex"));
        assert_eq!(project.managed,decodex_codex::app_server_client::NativeExecutionDefaults::default());
        assert!(calls.lock().expect("calls").is_empty(),"defaults do not require model discovery or inference");
        session.child.kill().await.expect("stop fixture");session.child.wait().await.expect("reap fixture");
        assert!(!server.is_finished());server.abort();
    }).await.expect("bounded defaults fixture");
}
