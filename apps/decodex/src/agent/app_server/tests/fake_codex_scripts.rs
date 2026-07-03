use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

use tempfile::TempDir;

pub(super) fn install_fake_codex_script(temp_dir: &TempDir, script: &str) -> PathBuf {
	let fake_bin_dir = temp_dir.path().join("fake-bin");
	let fake_codex_path = fake_bin_dir.join("codex");

	fs::create_dir_all(&fake_bin_dir).expect("fake bin directory should create");
	fs::write(&fake_codex_path, script).expect("fake codex script should write");

	let mut permissions =
		fs::metadata(&fake_codex_path).expect("fake codex metadata should read").permissions();

	#[cfg(unix)]
	PermissionsExt::set_mode(&mut permissions, 0o755);
	fs::set_permissions(&fake_codex_path, permissions)
		.expect("fake codex script should be executable");

	fake_bin_dir
}

pub(super) fn orphan_response_fake_codex_script() -> &'static str {
	r#"#!/usr/bin/env python3
import json
import os
import sys

def send(value):
    print(json.dumps(value), flush=True)

for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    message_id = message.get("id")
    params = message.get("params") or {}

    def reply(result):
        send({"id": message_id, "result": result})

    if method == "initialize":
        reply({
            "userAgent": "codex-cli 0.136.0",
            "codexHome": os.environ["CODEX_HOME"],
            "platformFamily": "unix",
            "platformOs": "macos"
        })
    elif method == "initialized":
        continue
    elif method == "config/read":
        reply({"config": {
            "model": "gpt-5.5",
            "model_provider": "openai",
            "approval_policy": {"type": "never"},
            "sandbox_mode": {"type": "dangerFullAccess"}
        }})
    elif method == "model/list":
        reply({"data": [{
            "id": "gpt-5.5",
            "model": "gpt-5.5",
            "displayName": "GPT-5.5",
            "isDefault": True,
            "hidden": False
        }], "nextCursor": None})
    elif method == "modelProvider/capabilities/read":
        reply({"imageGeneration": True, "namespaceTools": True, "webSearch": True})
    elif method == "skills/list":
        cwd = params.get("cwds", [""])[0]
        reply({"data": [{"cwd": cwd, "errors": [], "skills": [{
            "enabled": True,
            "name": "fake-skill",
            "scope": "user"
        }]}]})
    elif method == "plugin/list":
        reply({"marketplaces": [{"name": "fake", "plugins": [{
            "enabled": True,
            "id": "fake-plugin",
            "installed": True,
            "name": "Fake Plugin"
        }]}], "marketplaceLoadErrors": []})
    elif method == "mcpServerStatus/list":
        reply({"data": [], "nextCursor": None})
    elif method == "thread/start":
        cwd = params.get("cwd")
        reply({
            "thread": {"id": "thread-1"},
            "model": "gpt-5.5",
            "modelProvider": "openai",
            "serviceTier": None,
            "cwd": cwd,
            "instructionSources": [],
            "approvalPolicy": {"type": "never"},
            "approvalsReviewer": "user",
            "sandbox": {"type": "dangerFullAccess"},
            "reasoningEffort": None
        })
    elif method == "turn/start":
        reply({"turn": {"id": "turn-1", "status": "running", "error": None}})
        send({"method": "thread/status/changed", "params": {
            "threadId": "thread-1",
            "status": {"type": "active", "activeFlags": []}
        }})
        send({"method": "turn/started", "params": {
            "threadId": "thread-1",
            "turn": {"id": "turn-1", "status": "running", "error": None}
        }})
        send({"id": 999, "result": {"late": True}})
        send({"method": "item/completed", "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "item": {"type": "agentMessage", "text": "ORPHAN_OK"}
        }})
        send({"method": "turn/completed", "params": {
            "threadId": "thread-1",
            "turn": {"id": "turn-1", "status": "completed", "error": None}
        }})
    else:
        send({"id": message_id, "error": {
            "code": -32601,
            "message": "unexpected method " + str(method)
        }})
"#
}

pub(super) fn slow_thread_start_fake_codex_script() -> String {
	orphan_response_fake_codex_script().replace(
		"    elif method == \"thread/start\":\n        cwd = params.get(\"cwd\")",
		"    elif method == \"thread/start\":\n        import time\n        time.sleep(6)\n        cwd = params.get(\"cwd\")",
	)
}

pub(super) fn retrying_error_fake_codex_script() -> String {
	orphan_response_fake_codex_script().replace(
		"        send({\"id\": 999, \"result\": {\"late\": True}})",
		r#"        send({"method": "error", "params": {
            "threadId": "thread-1",
            "turnId": "turn-1",
            "willRetry": True,
            "error": {
                "message": "Reconnecting... 2/5",
                "codexErrorInfo": "transientNetworkError"
            }
        }})
        send({"id": 999, "result": {"late": True}})"#,
	)
}
