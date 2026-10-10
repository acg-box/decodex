//! Read receipts through the retained native bridge, including cold persistence.
use crate::account_launch::agent_process::native_tests::{
	self, Arc, Duration, NativeSession, Ordering, ServerEvent,
};
use decodex_codex::app_server_client::{ClientError, NativeUnreadPosition};
use std::{env, fs, sync::atomic::AtomicUsize};
use tokio::{net::TcpListener, time};
#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated native read receipts"]
async fn installed_native_read_state_preserves_new_results_and_cold_marks() {
	time::timeout(Duration::from_secs(45),async{
        let binary=env::var_os("DECODEX_TEST_CODEX_BINARY").expect("native receipt fixture");
        let home=tempfile::tempdir().expect("native receipt fixture");
        let listener=TcpListener::bind("127.0.0.1:0").await.expect("native receipt fixture");
        let address=listener.local_addr().expect("native receipt fixture");
        let calls=Arc::new(AtomicUsize::new(0));
        let backend=tokio::spawn(native_tests::serve(listener,calls.clone()));
        fs::write(home.path().join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).expect("native receipt fixture");
        let mut session=NativeSession::start(&binary,home.path());
        let started=session.client.thread_start(serde_json::json!({"cwd":home.path(),"threadSource":"user","historyMode":"paginated","approvalPolicy":"never","sandbox":"read-only"})).await.expect("native receipt fixture");
        let thread=started["thread"]["id"].as_str().expect("native receipt fixture").to_owned();
        // Persist a visible result before expecting a durable receipt.
        finish_turn(&mut session,&thread).await;
        let first=session.client.thread_read_state(&thread).await.expect("native receipt fixture").expect("native receipt fixture");
        assert!(matches!(first.first_unread,Some(NativeUnreadPosition::Turn{..})));
        assert_eq!(session.client.thread_read_state(&thread).await.expect("native receipt fixture"),Some(first.clone()),"reading must not acknowledge");
        finish_turn(&mut session,&thread).await;
        let second=session.client.thread_read_state(&thread).await.expect("native receipt fixture").expect("native receipt fixture");
        assert_eq!(first.first_unread,second.first_unread);
        assert_ne!(first.revision,second.revision);
        let guard=session.client.history_guard(session.client.history_revision()).expect("native receipt fixture");
        assert!(matches!(session.client.update_thread_read_state(&thread,&first.revision,true,guard).await,Err(ClientError::Remote(error)) if error.code == -32600 && error.data.as_ref().is_some_and(|d|d["reason"]=="readStateConflict")));
        let guard=session.client.history_guard(session.client.history_revision()).expect("native receipt fixture");
        let read=session.client.update_thread_read_state(&thread,&second.revision,true,guard).await.expect("native receipt fixture");
        assert!(read.first_unread.is_none());
        let guard=session.client.history_guard(session.client.history_revision()).expect("native receipt fixture");
        let unread=session.client.update_thread_read_state(&thread,&read.revision,false,guard).await.expect("native receipt fixture");
        assert_eq!(unread.first_unread,Some(NativeUnreadPosition::ThreadStart));
        assert_eq!(calls.load(Ordering::SeqCst),2,"metadata never invokes inference");
        drop(session);
        let reopened=NativeSession::start(&binary,home.path());
        assert_eq!(reopened.client.thread_read_state(&thread).await.expect("native receipt fixture"),Some(unread));
        let ephemeral=reopened.client.thread_start(serde_json::json!({"cwd":home.path(),"ephemeral":true,"approvalPolicy":"never","sandbox":"read-only"})).await.expect("native receipt fixture");
        assert!(reopened.client.thread_read_state(ephemeral["thread"]["id"].as_str().expect("native receipt fixture")).await.expect("native receipt fixture").is_none());
        assert_eq!(calls.load(Ordering::SeqCst),2);
        backend.abort();
    }).await.expect("native receipt deadline");
}
async fn finish_turn(session: &mut NativeSession, thread: &str) {
	session.client.turn_start(serde_json::json!({"threadId":thread,"input":[{"type":"text","text":"Read receipt fixture.","text_elements":[]}]})).await.expect("native receipt fixture");
	let mut receipt_changed = false;
	loop {
		if let ServerEvent::Notification { method, params } =
			session.events.recv().await.expect("native receipt fixture")
		{
			if params["threadId"] != thread {
				continue;
			}
			if method == "thread/readState/changed" {
				receipt_changed = true;
			}
			assert_ne!(method, "error", "{params}");
			if method == "turn/completed" {
				assert_eq!(params["turn"]["status"], "completed");
				assert!(receipt_changed, "receipt must precede completion");
				return;
			}
		}
	}
}
