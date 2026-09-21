use super::*;

#[tokio::test]
async fn recovery_retries_closing_thread_and_reconciles_without_replaying_input() {
	for terminal in [false, true] {
		let history = json!({
			"_resume_failures":1,"_resume_closing":true,
			"opaque thread/1":{"thread":{"id":"opaque thread/1",
				"status":{"type":if terminal {"idle"} else {"active"}},
				"turns":[{"id":"opaque turn/1",
					"status":if terminal {"completed"} else {"inProgress"},
					"items":[]}]}}
		});
		let (mut original, mut sent, _directory) = fixture_with_history(history).await;
		original.start_chief("chief", "Coordinate").await.unwrap();
		while sent.try_recv().is_ok() {}
		let mut recovered = ChiefCoordinator::new(
			original.store.clone(),
			original.client.clone(),
			original.config.clone(),
		)
		.unwrap();
		drop(original);
		recovered.recover_persisted().await.unwrap();
		let work = recovered.store.get_chief_work_item("chief".into()).await.unwrap();
		assert_eq!(work.codex_thread_id.as_deref(), Some("opaque thread/1"));
		assert_eq!(
			work.dispatch_state,
			if terminal {
				decodex_database::ChiefDispatchState::Idle
			} else {
				decodex_database::ChiefDispatchState::Running
			}
		);
		let mut resumes = Vec::new();
		while let Ok(request) = sent.try_recv() {
			assert!(
				["thread/resume", "thread/read"].contains(&request["method"].as_str().unwrap())
			);
			if request["method"] == "thread/resume" {
				resumes.push(request["params"].clone());
			}
		}
		assert_eq!(resumes.len(), 2);
		assert_eq!(resumes[0], resumes[1]);
		assert_eq!(resumes[0]["threadId"], "opaque thread/1");
		assert_eq!(resumes[0]["excludeTurns"], true);
	}
}
