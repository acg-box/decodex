//! Installed native skill discovery and exact skill input, against a local provider only.
use super::*;
use std::sync::{Mutex, atomic::AtomicUsize};

#[tokio::test]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY; isolated skills inventory and injection"]
async fn installed_skill_picker_inventory_and_exact_input_use_native_skill_owner() {
	tokio::time::timeout(Duration::from_secs(45), async {
		let binary=std::env::var_os("DECODEX_TEST_CODEX_BINARY").expect("explicit binary");
		let temp=tempfile::tempdir().unwrap();let home=temp.path().canonicalize().unwrap();
		let roots=home.join("skills (fixture)");let skill=roots.join("explicit-skill");
		std::fs::create_dir_all(&skill).unwrap();
		std::fs::write(skill.join("SKILL.md"),"---\nname: explicit-fixture\ndescription: Verify native skill invocation.\n---\nUse the exact marker DECODEX_EXPLICIT_SKILL_BODY_91 when replying.\n").unwrap();
		let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();
		let bodies=Arc::new(Mutex::new(Vec::new()));let calls=Arc::new(AtomicUsize::new(0));
		let backend=tokio::spawn(serve_fixture(listener,calls.clone(),None,Some(bodies.clone()),None,|_|json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done"}]})));
		std::fs::write(home.join("config.toml"),format!("model=\"gpt-5.6-sol\"\nmodel_provider=\"fixture\"\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname=\"fixture\"\nbase_url=\"http://{address}\"\nwire_api=\"responses\"\nrequires_openai_auth=false\nsupports_websockets=false\n")).unwrap();
		let mut session=NativeSession::start(&binary,&home);
		session.client.request("skills/extraRoots/set",json!({"extraRoots":[roots]})).await.unwrap();
		let inventory=session.client.request("skills/list",json!({"cwds":[home],"forceReload":true})).await.unwrap();
		let page=crate::agent_skills::project(&inventory,home.to_str().unwrap(),"explicit-fixture").unwrap();
		assert_eq!(page.skills.len(),1,"native extra-root skill must be discoverable before creating a thread");
		assert_eq!(calls.load(Ordering::SeqCst),0,"discovery must not start inference");
		let selected=&page.skills[0];
		assert_eq!(selected.path.as_str(),skill.join("SKILL.md").to_str().unwrap());
		let thread=session.client.thread_start(json!({"cwd":home,"approvalPolicy":"never","sandbox":"read-only"})).await.unwrap()["thread"]["id"].as_str().unwrap().to_owned();
		let mut params=json!({"threadId":thread,"input":[{"type":"text","text":"Use the selected skill."}]});
		crate::agent::apply_message_options(&mut params,&json!({"options":{"attachments":[{"path":selected.path.as_str(),"image":false,"skill_name":selected.name.as_str()}]}}).to_string()).unwrap();
		session.client.turn_start(params).await.unwrap();
		loop {
			if let Some(ServerEvent::Notification {method,params})=session.events.recv().await
				&& method=="turn/completed" {
				assert_eq!(params["turn"]["status"],"completed");break;
			}
		}
		let request=bodies.lock().unwrap()[0].to_string();
		assert!(request.contains("DECODEX_EXPLICIT_SKILL_BODY_91"),"native must inject the selected skill body");
		assert_eq!(calls.load(Ordering::SeqCst),1);
		drop(session);backend.abort();
	}).await.unwrap();
}
