//! Opt-in interactive signed desktop acceptance against the isolated real service.
use super::*;

pub(super) async fn check(
	client: &ChiefClient,
	home: &std::path::Path,
	account: &AccountId,
	requests: &std::sync::atomic::AtomicUsize,
) {
	let binary = std::env::var_os("DECODEX_TEST_DESKTOP_APP").expect("explicit desktop executable");
	assert!(std::path::Path::new(&binary).is_absolute());
	let workspace = home.join("workspace");
	std::fs::create_dir(&workspace).unwrap();
	accepted(
		client,
		Action::Start(ChiefStartDto {
			root_id: EntityId::new("recap-root").unwrap(),
			prompt: HistoryText::new("Isolated desktop acceptance. Reply briefly.").unwrap(),
			model: ConversationModel::new("cold-native-model").unwrap(),
			effort: Some(ConversationReasoningEffort::new("provider-effort").unwrap()),
			cwd: ConversationWorkingDirectory::new(workspace.to_str().unwrap()).unwrap(),
			account_id: Some(EntityId::new(account.as_str()).unwrap()),
			sandbox: ChiefSandboxDto::ReadOnly,
		}),
		"desktop-start",
	)
	.await;
	let thread = settled(client).await;
	let launch = || {
		let log = std::fs::OpenOptions::new()
			.create(true)
			.append(true)
			.open(home.join("desktop.log"))
			.unwrap();
		let error = log.try_clone().unwrap();
		tokio::process::Command::new(&binary)
			.current_dir(home)
			.stdout(log)
			.stderr(error)
			.kill_on_drop(true)
			.spawn()
			.unwrap()
	};
	let mut child = launch();
	let mut launches = 1;
	let mut exits = 0;
	let mut exited = false;
	let write = |pid, launches, exits| {
		std::fs::write(home.join("desktop-ready.json"), serde_json::to_vec_pretty(&json!({"pid":pid,"launches":launches,"exits":exits,"thread":thread,"root":home.join(".decodex"),"model_requests":requests.load(Ordering::Acquire)})).unwrap()).unwrap();
	};
	write(child.id(), launches, exits);
	loop {
		if !exited && let Some(status) = child.try_wait().unwrap() {
			assert!(status.success(), "signed desktop failed; inspect desktop.log");
			exited = true;
			exits += 1;
			write(None, launches, exits);
		}
		if home.join("desktop-relaunch").exists() {
			assert!(exited, "quit the exact desktop before relaunch");
			std::fs::remove_file(home.join("desktop-relaunch")).unwrap();
			child = launch();
			launches += 1;
			exited = false;
			write(child.id(), launches, exits);
		}
		if home.join("desktop-finish").exists() {
			assert!(exited, "quit the desktop before completing acceptance");
			assert_eq!(launches, exits);
			break;
		}
		tokio::time::sleep(Duration::from_millis(100)).await;
	}
}
