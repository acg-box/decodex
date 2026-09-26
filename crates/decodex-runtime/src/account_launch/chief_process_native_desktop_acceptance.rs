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
	std::fs::create_dir(&workspace).expect("create isolated workspace");
	accepted(
		client,
		Action::Start(ChiefStartDto {
			root_id: EntityId::new("recap-root").expect("valid fixture root"),
			prompt: HistoryText::new("Isolated desktop acceptance. Reply briefly.")
				.expect("valid fixture prompt"),
			model: ConversationModel::new("cold-native-model").expect("valid fixture model"),
			effort: Some(
				ConversationReasoningEffort::new("provider-effort").expect("valid fixture effort"),
			),
			cwd: ConversationWorkingDirectory::new(
				workspace.to_str().expect("fixture workspace uses UTF-8"),
			)
			.expect("valid fixture directory"),
			account_id: Some(EntityId::new(account.as_str()).expect("valid fixture account")),
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
			.expect("open desktop log");
		let error = log.try_clone().expect("clone desktop log");
		tokio::process::Command::new(&binary)
			.current_dir(home)
			.stdout(log)
			.stderr(error)
			.kill_on_drop(true)
			.spawn()
			.expect("launch explicit desktop executable")
	};
	let mut child = launch();
	let mut launches = 1;
	let mut exits = 0;
	let mut exited = false;
	let write = |pid, launches, exits| {
		std::fs::write(home.join("desktop-ready.json"), serde_json::to_vec_pretty(&json!({"pid":pid,"launches":launches,"exits":exits,"thread":thread,"root":home.join(".decodex"),"model_requests":requests.load(Ordering::Acquire)})).expect("serialize desktop process evidence")).expect("write desktop process evidence");
	};
	write(child.id(), launches, exits);
	loop {
		if !exited && let Some(status) = child.try_wait().expect("read desktop exit status") {
			assert!(status.success(), "signed desktop failed; inspect desktop.log");
			exited = true;
			exits += 1;
			write(None, launches, exits);
		}
		if home.join("desktop-relaunch").exists() {
			assert!(exited, "quit the exact desktop before relaunch");
			std::fs::remove_file(home.join("desktop-relaunch"))
				.expect("consume desktop relaunch marker");
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
