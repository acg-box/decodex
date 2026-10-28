//! Opt-in original EGM qualification with process-scoped subscription authentication.

use std::{
	env,
	ffi::OsStr,
	fs,
	path::{Path, PathBuf},
	time::Duration,
};

use serde_json::{Value, json};
use tokio::{sync::mpsc::Receiver, time};

use super::{
	AccountBinding, AccountId, AccountIdentity, AttestedAppServerLaunch, AttestedProcessChild,
	CredentialProjection, CredentialVault, CredentialVaultError, ProcessGenerationAccountBinding,
	RunnerCapacity, native_control_tests,
};
use crate::account_import::{self, CredentialSource, ImportedCredential};
use decodex_codex::app_server_client::{AppServerClient, ServerEvent};
use decodex_components::{NativePlugins, PluginAction, egm_component};
use decodex_core::{AccountOperationId, CredentialVersion};

struct SubscriptionVault(ImportedCredential);
impl CredentialVault for SubscriptionVault {
	fn project(
		&self,
		_: &AccountId,
		projection: &mut CredentialProjection<'_>,
	) -> Result<AccountIdentity, CredentialVaultError> {
		let bundle = &self.0.bundle;

		projection.authenticate_chatgpt(
			bundle.access_token(),
			self.0.provider.account_id(),
			bundle.plan_type(),
		)?;
		Ok(AccountIdentity::from_observation("chatgpt", bundle.provider_email(), true))
	}
}

fn start_child(binary: &OsStr, home: &Path) -> AttestedProcessChild {
	// Use the normal protected enrollment reader. No credentials are copied to
	// the isolated home or returned in test output, and no refresh token is used.
	let CredentialSource::Oauth(imported) =
		account_import::read_shared_codex_credential().expect("existing native subscription login")
	else {
		panic!("this fixture requires a native OAuth subscription login")
	};
	let account = AccountId::new("10000000-0000-4000-8000-000000000001").expect("fixture account");
	let operation =
		AccountOperationId::new("20000000-0000-4000-8000-000000000001").expect("fixture operation");
	let credential = imported
		.bundle
		.binding_for(
			&account,
			&operation,
			CredentialVersion::new(1).expect("version"),
			&imported.provider,
		)
		.expect("native credential binding");
	let profile = native_control_tests::attested_profile(binary, home);
	let callback = profile.generated.account_callback_profile_sha256().to_owned();
	let binding = AccountBinding {
		personal_access_token: None,
		account_id: account.clone(),
		expected_codex_home: home.join(".codex"),
		process_binding: Some(
			ProcessGenerationAccountBinding::new(1, credential, callback).expect("process binding"),
		),
		refresh_callback: None,
	};
	let capacity = RunnerCapacity::try_with_limit(1).expect("isolated capacity");
	let launch = AttestedAppServerLaunch::bind(
		profile,
		binding,
		Duration::from_secs(20),
		capacity.reserve(account, 1).expect("permit"),
	)
	.expect("native launch");
	let mut child = launch.spawn().expect("native child");

	child
		.initialize_agent_turns(&SubscriptionVault(*imported))
		.expect("process-scoped native login");
	assert!(!home.join(".codex/auth.json").exists(), "credentials stay in memory");
	child
}

async fn run_turn(
	client: &AppServerClient,
	events: &mut Receiver<ServerEvent>,
	thread: &str,
	input: Value,
	evidence: &Path,
) {
	let response = client
		.turn_start(json!({"threadId":thread,"input":input,"effort":"medium"}))
		.await
		.expect("native turn");
	let turn = response["turn"]["id"].as_str().expect("native turn identity");
	let mut observed = Vec::new();

	time::timeout(Duration::from_secs(600), async {
		loop {
			match events.recv().await.expect("native event stream") {
				ServerEvent::Notification { method, params } => {
					let completed = method == "turn/completed"
						&& params["threadId"] == thread
						&& params["turn"]["id"] == turn;

					observed.push(json!({"method":method,"params":params}));
					if completed {
						assert_eq!(params["turn"]["status"], "completed", "native workflow failed");
						break;
					}
				},
				ServerEvent::Request { method, .. } => panic!("fixture needs user input: {method}"),
				_ => {},
			}
		}
	})
	.await
	.expect("bounded EGM workflow");
	fs::write(evidence, serde_json::to_vec_pretty(&observed).expect("event evidence"))
		.expect("private event evidence");
}

fn collect_traces(directory: &Path, traces: &mut Vec<(PathBuf, Vec<Value>)>) {
	for entry in fs::read_dir(directory).expect("native trace directory") {
		let path = entry.expect("native trace entry").path();

		if path.is_dir() {
			collect_traces(&path, traces);
		} else if path.extension().is_some_and(|extension| extension == "jsonl") {
			let events = fs::read_to_string(&path)
				.expect("native trace")
				.split('\n')
				.filter(|line| !line.trim().is_empty())
				.map(|line| serde_json::from_str(line).expect("native trace JSON"))
				.collect();

			traces.push((path, events));
		}
	}
}

fn verify_traces(
	home: &Path,
	evidence: &Path,
	parent: &str,
	marker: &str,
	plugin: &Path,
	target: &Path,
) {
	let mut traces = Vec::new();

	collect_traces(&home.join(".codex/sessions"), &mut traces);
	let mut reviewers = 0;
	let mut fresh_spawns = 0;

	for (path, trace) in traces {
		fs::copy(&path, evidence.join(path.file_name().expect("trace name")))
			.expect("retained synthetic trace");
		let id = trace
			.iter()
			.find(|event| event["type"] == "session_meta")
			.expect("session identity")["payload"]["id"]
			.as_str()
			.expect("thread identity");

		if id == parent {
			for event in &trace {
				let payload = &event["payload"];

				if event["type"] == "response_item"
					&& payload["type"] == "function_call"
					&& payload["name"].as_str().is_some_and(|name| name.ends_with("spawn_agent"))
				{
					let arguments: Value = serde_json::from_str(
						payload["arguments"].as_str().expect("spawn arguments"),
					)
					.expect("native spawn JSON");

					assert!(
						arguments["fork_context"] == false || arguments["fork_turns"] == "none",
						"Goldfish must use fresh context"
					);
					fresh_spawns += 1;
				}
			}
			continue;
		}

		assert!(
			!serde_json::to_string(&trace).expect("child trace").contains(marker),
			"parent context leaked"
		);
		assert!(
			trace.iter().any(|event| event["payload"]["type"] == "task_complete"),
			"native child did not complete"
		);
		let reads = trace
			.iter()
			.filter_map(|event| {
				let item = &event["payload"]["item"];

				(item["type"] == "CommandExecution"
					&& item["status"] == "completed"
					&& item["exit_code"] == 0
					&& item["aggregated_output"]
						.as_str()
						.is_some_and(|text| !text.trim().is_empty()))
				.then(|| item.to_string())
			})
			.collect::<Vec<_>>();
		let persona = plugin.join("agents/zoom_out.md");

		if reads.iter().any(|item| item.contains(persona.to_str().expect("persona path"))) {
			for required in [
				persona,
				plugin.join("resources/document_standards/zoom-out-standard.md"),
				target.to_path_buf(),
			] {
				assert!(
					reads.iter().any(|item| item.contains(required.to_str().expect("source path"))),
					"missing successful read: {}",
					required.display()
				);
			}
			reviewers += 1;
		} else {
			assert!(
				trace.iter().any(|event| event["payload"]["type"] == "task_complete"
					&& event["payload"]["last_agent_message"]
						.as_str()
						.is_some_and(|text| text.trim() == "UNKNOWN")),
				"isolation probe must return its actual result"
			);
		}
	}

	assert!(
		fresh_spawns >= 2 && reviewers >= 1,
		"need the actual isolation probe and EGM reviewer"
	);
}

fn compare_original(source: &Path, installed: &Path) {
	for entry in fs::read_dir(source).expect("original EGM package") {
		let entry = entry.expect("original file");
		let target = installed.join(entry.file_name());

		if entry.path().is_dir() {
			compare_original(&entry.path(), &target);
		} else {
			assert_eq!(
				fs::read(entry.path()).expect("original bytes"),
				fs::read(&target).expect("installed bytes"),
				"modified original file: {}",
				target.display()
			);
		}
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires DECODEX_TEST_CODEX_BINARY, DECODEX_TEST_EGM_EVIDENCE and an existing native subscription; uses model quota"]
async fn isolated_original_egm_installs_and_completes_a_fresh_goldfish_review() {
	let binary = env::var_os("DECODEX_TEST_CODEX_BINARY").expect("qualified native binary");
	let evidence = PathBuf::from(
		env::var_os("DECODEX_TEST_EGM_EVIDENCE").expect("private evidence directory"),
	);
	let fixture = tempfile::tempdir().expect("isolated native home");
	let home = fixture.path().canonicalize().expect("native home");
	let codex_home = home.join(".codex");
	let work = home.join("work");

	fs::create_dir_all(&codex_home).expect("private Codex home");
	fs::create_dir_all(&work).expect("synthetic work");
	fs::create_dir_all(&evidence).expect("private evidence");
	fs::write(codex_home.join("config.toml"), "model_reasoning_effort=\"medium\"\n[features]\nmulti_agent=true\nremote_plugins=false\nhooks=false\nmemories=false\n[analytics]\nenabled=false\n").expect("isolated configuration");
	let native = NativePlugins::new(PathBuf::from(&binary), codex_home.clone());

	assert!(
		native
			.apply(&egm_component(), PluginAction::Install)
			.expect("original native EGM installation")
			.expect("installed EGM")
			.enabled
	);
	let launch_home = home.clone();
	let mut child = tokio::task::spawn_blocking(move || start_child(&binary, &launch_home))
		.await
		.expect("native launch worker");
	let (client, mut events) = child.retain_agent_connection().expect("existing app-server client");
	let skills = client
		.request("skills/list", json!({"cwds":[work],"forceReload":true}))
		.await
		.expect("fresh native skills");
	let skills = skills["data"][0]["skills"].as_array().expect("skill catalog");
	let egm: Vec<_> = skills.iter().filter(|skill| skill["pluginId"] == "egm@egm").collect();

	assert_eq!(egm.len(), 7, "complete original EGM skill suite");
	fs::write(
		evidence.join("skills.json"),
		serde_json::to_vec_pretty(&egm).expect("native skill metadata"),
	)
	.expect("skill evidence");
	let review = egm
		.iter()
		.find(|skill| {
			skill["path"].as_str().is_some_and(|path| {
				Path::new(path)
					.parent()
					.and_then(Path::file_name)
					.is_some_and(|name| name == "egm-review-loop")
			})
		})
		.expect("original review workflow");
	let skill_name = review["name"].as_str().expect("native skill name");
	let skill = PathBuf::from(review["path"].as_str().expect("discovered installed skill"));
	let plugin =
		skill.parent().and_then(Path::parent).and_then(Path::parent).expect("native plugin root");
	let original = codex_home.join(".tmp/marketplaces/egm");

	for directory in ["skills", "agents", "resources", "docs"] {
		compare_original(&original.join(directory), &plugin.join(directory));
	}
	let started = client.thread_start(json!({"cwd":work,"approvalPolicy":"never","sandbox":"workspace-write","persistExtendedHistory":true})).await.expect("isolated native thread");
	let thread = started["thread"]["id"].as_str().expect("native thread identity");
	let marker = format!("parent-only-{}", std::process::id());
	let probe = format!(
		"Native EGM isolation fixture. Parent-only marker: {marker}. Do not repeat, write or forward the marker. Read the installed EGM docs/codex.md. Use a real native fresh-context subagent with fork_context:false or fork_turns:none, following its actual tool schema. Its exact task is: 'Do not use tools. If a parent-only marker is visible in your conversation, return it verbatim. Otherwise return exactly UNKNOWN.' Wait for its actual completion; never simulate a child. Do not access memory, external services or conversation history."
	);

	run_turn(
		&client,
		&mut events,
		thread,
		json!([{"type":"text","text":probe}]),
		&evidence.join("isolation-events.json"),
	)
	.await;
	let target = work.join("design.md");
	let draft = "# Offline line counter\n\nGoal: Count nonblank UTF-8 lines in one local file with no network access. Use the Python standard library. Missing files and invalid UTF-8 must fail clearly.\n\n## Proposed design\nSend every file to a hosted database before counting. Add a generic multi-tenant scheduling engine for future customers. Build a custom database and background job queue before supporting the one-file command.\n";

	fs::write(&target, draft).expect("synthetic design");
	let prompt = format!(
		"Use the installed original egm-review-loop skill. This is one small noninteractive acceptance fixture; the synthetic user confirms this exact configuration up front: TargetFile={}, SemanticTarget=full file, Reviewers=[zoom_out], Standards={{zoom_out:zoom-out-standard}}, GoldfishContext=[]; EGM artifact directory: {}. Dispatch the real reviewer directly in a fresh native context, read the original persona, target and standard, wait for its actual findings, and execute original egm-resolve to write goldfish_review_report.md. The fixture author stops after this one review round: do not apply repairs, start another round, implement code or contact external services. Preserve the resolution decision gate in the final response. Do not retrieve memory or other conversations; do not alter the original plugin. Keep findings concise.",
		target.display(),
		work.display()
	);

	run_turn(
		&client,
		&mut events,
		thread,
		json!([{"type":"text","text":prompt},{"type":"skill","name":skill_name,"path":skill}]),
		&evidence.join("review-events.json"),
	)
	.await;
	assert_eq!(
		fs::read_to_string(&target).expect("original design"),
		draft,
		"review waits for approval"
	);
	let report =
		fs::read_to_string(work.join("goldfish_review_report.md")).expect("actual EGM report");

	assert!(report.len() > 100, "nontrivial original workflow artifact");
	fs::write(evidence.join("goldfish_review_report.md"), report).expect("retained report");
	client.close();
	tokio::task::spawn_blocking(move || child.shutdown())
		.await
		.expect("shutdown worker")
		.expect("native process cleanup");
	verify_traces(&home, &evidence, thread, &marker, plugin, &target);
	assert!(!codex_home.join("auth.json").exists(), "no credentials persisted");
	assert_eq!(
		native.apply(&egm_component(), PluginAction::Remove).expect("native uninstall"),
		None
	);
	assert!(!codex_home.join("plugins/cache/egm/egm").exists());
}
