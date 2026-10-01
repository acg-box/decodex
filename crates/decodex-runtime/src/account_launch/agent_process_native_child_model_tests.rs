//! Native children inherit the model captured after a runtime-owned active-turn edit.
use super::*;
use std::path::Path;

fn catalog(home: &Path) -> String {
	let models: Vec<_> = ["gpt-5.6-sol", "gpt-5.6-terra"].into_iter().map(|slug| json!({
		"slug":slug,"display_name":slug,"description":"Synthetic model",
		"default_reasoning_level":"low","supported_reasoning_levels":[{"effort":"low","description":"Low"},{"effort":"high","description":"High"}],
		"shell_type":"shell_command","visibility":"list","minimal_client_version":"0.1.0",
		"supported_in_api":true,"priority":0,"support_verbosity":false,"default_verbosity":null,
		"apply_patch_tool_type":null,"truncation_policy":{"mode":"bytes","limit":10_000},
		"supports_image_detail_original":false,"multi_agent_version":"v2","context_window":272_000,
		"max_context_window":272_000,"experimental_supported_tools":[],
		"model_messages":{"instructions_template":"Synthetic fixture","instructions_variables":null,
			"tools":{"multi_agent":{"spawn_agent":{"description":format!("Catalog spawn for {slug}.")}}}}
	})).collect();
	let path = home.join("models.json");

	std::fs::write(&path, serde_json::to_vec(&json!({"models":models})).expect("catalog JSON"))
		.expect("write fixture catalog");

	format!("model_catalog_json={}\n", serde_json::to_string(&path).expect("catalog path"))
}

fn spawn_spec(body: &Value) -> &Value {
	body["tools"]
		.as_array()
		.expect("outbound tools")
		.iter()
		.flat_map(|tool| tool["tools"].as_array().into_iter().flatten())
		.find(|tool| tool["name"] == "spawn_agent")
		.expect("native spawn schema")
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated captured child settings"]
async fn installed_native_child_inherits_updated_step_model() {
	qualify_child_model(false).await;
}

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native model-owned spawn descriptions"]
async fn installed_native_spawn_description_follows_updated_step_model() {
	qualify_child_model(true).await;
}

async fn qualify_child_model(check_catalog: bool) {
	let binary = std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit native binary");
	let home = tempfile::tempdir().expect("native fixture");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("native fixture");
	let address = listener.local_addr().expect("native fixture");
	let calls = Arc::new(AtomicUsize::new(0));
	let bodies = Arc::new(std::sync::Mutex::new(Vec::new()));
	let backend = tokio::spawn(serve_fixture(
		listener,
		calls.clone(),
		None,
		Some(bodies.clone()),
		Some(json!({"input_tokens":1,"output_tokens":1,"total_tokens":2})),
		|serial| {
			if serial == 0 {
				json!({"type":"function_call","name":"pause_fixture","arguments":"{}","call_id":"pause"})
			} else if serial == 1 {
				json!({"type":"function_call","namespace":"collaboration","name":"spawn_agent","call_id":"spawn-updated","arguments":json!({"task_name":"settings_child","fork_turns":"none","message":"CHILD_CAPTURED_SETTINGS"}).to_string()})
			} else {
				json!({"type":"message","role":"assistant","id":format!("done-{serial}"),"content":[{"type":"output_text","text":"Done"}]})
			}
		},
	));
	let catalog_config = if check_catalog { catalog(home.path()) } else { String::new() };

	std::fs::write(home.path().join("config.toml"),format!("{catalog_config}model=\"gpt-5.6-sol\"\nmodel_reasoning_effort=\"low\"\nmodel_provider=\"fixture\"\n[features]\nmulti_agent=true\nmulti_agent_v2=true\nstep_model_switching=true\nenable_request_compression=false\n[model_providers.fixture]\nname=\"OpenAI\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("native fixture");

	tokio::time::timeout(Duration::from_secs(60), async {
        let mut session = NativeSession::start(&binary, home.path());
        let start = session.client.thread_start(json!({"cwd":home.path(),"model":"gpt-5.6-sol","approvalPolicy":"never","sandbox":"read-only","dynamicTools":[{"name":"pause_fixture","description":"Pause the isolated test","inputSchema":{"type":"object","properties":{}}}]})).await.expect("native fixture");
        let thread = start["thread"]["id"].as_str().expect("native fixture").to_owned();
        let turn = session.client.turn_start(json!({"threadId":thread,"effort":"low","input":[{"type":"text","text":"Pause, then create the isolated child."}]})).await.expect("native fixture");
        let turn = turn["turn"]["id"].as_str().expect("native fixture");
        let (id, method, params) = super::super::super::next_request(&mut session.events).await;

        assert_eq!(method, "item/tool/call");

        let guard = session.client.server_request_guard(&id, &method, &params).expect("native fixture");
        let owned = OwnedReviewer::new(home.path(), &session.client, &thread, turn).await;
        let state = read_options(&owned.store, true, || async {Some(owned.source(&owned.key))}).await;
        let AgentLiveReviewerState::Available {review_token, ..} = state else {panic!("live settings unavailable")};

        write(&owned.store, || async {Some(owned.source(&owned.key))}, turn, review_token.as_str(), LiveEdit::Model {model:ConversationModel::new("gpt-5.6-terra").expect("native fixture"), effort:ConversationReasoningEffort::High}, "child-step-edit").await.expect("native fixture");

        assert_eq!(calls.load(Ordering::Acquire), 1);

        session.client.respond_guarded(id, json!({"contentItems":[{"type":"inputText","text":"Continue"}],"success":true}), guard).await.expect("native fixture");

        let mut completed = std::collections::HashSet::new();

        while completed.len() < 2 {
            match session.events.recv().await.expect("native child event") {
                ServerEvent::Request {method, ..} => panic!("unexpected child request: {method}"),
                ServerEvent::Notification {method, params} => {
                    assert_ne!(method, "error", "native error: {params}");

                    if method == "turn/completed" {
                        assert_eq!(params["turn"]["status"], "completed");

                        completed.insert(params["threadId"].as_str().expect("native fixture").to_owned());
                    }
                },
                _ => {},
            }
        }

        assert!(completed.contains(&thread));

        let captured = bodies.lock().expect("native fixture");
        let children: Vec<_> = captured.iter().filter(|body|
            body["client_metadata"]["x-codex-parent-thread-id"] == thread
            && body["client_metadata"]["x-openai-subagent"] == "collab_spawn"
        ).collect();

        assert_eq!(children.len(), 1, "exactly one isolated child inference");
        assert!(completed.contains(children[0]["client_metadata"]["thread_id"].as_str().expect("native fixture")));
        assert_eq!(captured[0]["model"], "gpt-5.6-sol");
        assert_eq!(children[0]["model"], "gpt-5.6-terra");

        let metadata: Value = serde_json::from_str(children[0]["client_metadata"]["x-codex-turn-metadata"].as_str().expect("native fixture")).expect("native fixture");

        assert_eq!(metadata["reasoning_effort"], "high");

        if check_catalog {
        for (body, model) in [(&captured[0], "gpt-5.6-sol"), (&captured[1], "gpt-5.6-terra")] {
            let description = spawn_spec(body)["description"].as_str().expect("native fixture");

            assert!(description.contains(&format!("Catalog spawn for {model}.")), "model={model}, actual={description}");
            assert!(!description.contains("Spawns an agent to work on the specified task."));
        }

        assert_eq!(spawn_spec(&captured[0])["parameters"], spawn_spec(&captured[1])["parameters"]);
        }
    }).await.expect("native child model deadline");

	backend.abort();
}
