"""Verify an explicit catalog larger than 1 MiB reaches the native model inventory."""

import hashlib
import json
import os
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from native_session import Session


def qualify():
    model = {
        "slug": "large-catalog-fixture", "display_name": "Large catalog fixture",
        "description": "Synthetic catalog", "default_reasoning_level": "high",
        "supported_reasoning_levels": [{"effort": "high", "description": "High"}],
        "shell_type": "shell_command", "visibility": "list", "minimal_client_version": "0.1.0",
        "supported_in_api": True, "priority": 0, "support_verbosity": False,
        "default_verbosity": None, "apply_patch_tool_type": None,
        "truncation_policy": {"mode": "bytes", "limit": 10000},
        "supports_image_detail_original": False, "multi_agent_version": "v2",
        "context_window": 272000, "max_context_window": 272000,
        "experimental_supported_tools": [],
        "model_messages": {"instructions_template": "Synthetic fixture", "instructions_variables": None},
    }
    # Unknown catalog metadata does not inflate the public model/list response.
    payload = json.dumps({"models": [model], "fixturePadding": "x" * (1024 * 1024)}).encode()
    fetches = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            fetches.append(self.path.split("?")[0])
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="dx-large-catalog-") as home:
            Path(home, "config.toml").write_text(
                'model="large-catalog-fixture"\nmodel_provider="fixture"\n'
                '[features]\napi_key_model_discovery=true\n'
                '[model_providers.fixture]\nname="Fixture"\n'
                f'base_url="http://127.0.0.1:{server.server_port}"\n'
                f'model_catalog_url="http://127.0.0.1:{server.server_port}/catalog"\n'
                'wire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n'
                'experimental_bearer_token="synthetic-catalog-fixture"\n'
            )
            session = Session(home)
            try:
                models = session.call("model/list", {"includeHidden": True})["data"]
            finally:
                session.close()
            selected = [model["model"] for model in models if model["model"] == "large-catalog-fixture"]
            return {"catalog_bytes": len(payload), "catalog_sha256": hashlib.sha256(payload).hexdigest(),
                    "fetch_count": len(fetches), "selected_models": selected,
                    "passed": len(payload) > 1024 * 1024 and bool(fetches) and len(selected) == 1}
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    result = {"source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
              "cli_sha256": hashlib.sha256(Path(os.environ["QUALIFIED_CODEX_BINARY"]).read_bytes()).hexdigest(),
              **qualify()}
    Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"], "large-catalog-native.json").write_text(
        json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    assert result["passed"], "The selected model must survive an explicit catalog larger than 1 MiB."
