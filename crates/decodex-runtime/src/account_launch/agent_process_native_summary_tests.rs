//! Summary history remains display-only across native process restart.
use std::{env, fs, sync::atomic::AtomicUsize};

use tokio::{net::TcpListener, time};

use crate::account_launch::agent_process::native_tests::{
	self, Arc, Duration, NativeSession, ServerEvent,
};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native summary history"]
async fn installed_native_summary_history_is_read_only_and_survives_restart() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native binary");
	let home = tempfile::tempdir().expect("home");
	let listener = TcpListener::bind("127.0.0.1:0").await.expect("backend");
	let address = listener.local_addr().expect("address");
	let calls = Arc::new(AtomicUsize::new(0));
	let backend = tokio::spawn(native_tests::serve(listener, calls.clone()));

	fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("config");

	let mut session = NativeSession::start(&binary, home.path());
	let thread=time::timeout(Duration::from_secs(30),async {
        let started=session.client.thread_start(serde_json::json!({"cwd":home.path(),"historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.expect("start");
        let thread=started["thread"]["id"].as_str().expect("thread").to_owned();

        for text in ["First prompt","Second prompt"] {
            let turn=session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":text}]})).await.expect("turn");
            let turn=turn["turn"]["id"].as_str().expect("turn ID");

            loop {
                if let ServerEvent::Notification {method,params}=session.events.recv().await.expect("event")
                    && method=="turn/completed" && params["threadId"]==thread && params["turn"]["id"]==turn {break;}
            }
        }

        thread
    }).await.expect("native turns deadline");
	let summary = session.client.thread_history_summary(&thread, 100).await.expect("summary");
	let turns = summary["turns"].as_array().expect("turns");

	assert_eq!(turns.len(), 2);
	assert!(turns[0].to_string().contains("First prompt"));
	assert!(turns[1].to_string().contains("Second prompt"));
	assert!(turns.iter().all(|t| t.to_string().contains("Native bridge answer")));
	assert!(summary.get("nextCursor").is_none());

	drop(session);

	let cold = NativeSession::start(&binary, home.path());

	assert_eq!(cold.client.thread_history_summary(&thread, 100).await.expect("cold read"), summary);

	let latest = cold.client.thread_history_summary(&thread, 1).await.expect("bounded read");

	assert_eq!(latest["turns"], serde_json::json!([turns[1]]));
	assert_eq!(calls.load(std::sync::atomic::Ordering::Acquire), 2);

	drop(cold);

	backend.abort();
}
