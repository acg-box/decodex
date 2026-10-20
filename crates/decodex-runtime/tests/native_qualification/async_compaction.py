"""A failed background request must allow foreground compaction at the next checkpoint."""

import json
import os
from pathlib import Path
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from native_session import Session


def main():
    requests = []
    request_lock = threading.Lock()

    class Provider(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            with request_lock:
                serial = len(requests)
                requests.append(body)
            identity = f'async-compaction-{serial}'
            tokens = [70_000, 3_000, 91_000, 3_000, 3_000][serial]
            if serial == 2:
                output = {'type': 'function_call', 'name': 'list_agents',
                          'namespace': 'collaboration', 'call_id': 'checkpoint', 'arguments': '{}'}
            elif serial == 3:
                output = {'type': 'compaction', 'encrypted_content': 'FOREGROUND_CHECKPOINT'}
            else:
                output = {'type': 'message', 'role': 'assistant', 'id': identity,
                          'content': [{'type': 'output_text', 'text': 'Invalid checkpoint' if serial == 1 else 'Done'}]}
            frames = [
                {'type': 'response.created', 'response': {'id': identity}},
                {'type': 'response.output_item.done', 'item': output},
                {'type': 'response.completed', 'response': {'id': identity, 'usage': {
                    'input_tokens': tokens, 'output_tokens': 0, 'total_tokens': tokens}}},
            ]
            payload = ''.join(f'data: {json.dumps(frame)}\n\n' for frame in frames).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(payload)))
            self.send_header('x-codex-turn-state', 'fixture-model-pin')
            self.end_headers()
            self.wfile.write(payload)

    server = ThreadingHTTPServer(('127.0.0.1', 0), Provider)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='dx-async-compaction-', dir='/tmp') as home:
            Path(home, 'config.toml').write_text(f'''
model = "gpt-6-astra"
model_provider = "fixture"
model_context_window = 100000
model_auto_compact_token_limit = 90000
cli_auth_credentials_store = "file"
[features]
async_compaction = true
token_budget = false
enable_request_compression = false
code_mode = false
multi_agent = true
multi_agent_v2 = true
[model_providers.fixture]
name = "OpenAI"
base_url = "http://127.0.0.1:{server.server_port}/v1"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
stream_max_retries = 0
request_max_retries = 0
[model_providers.fixture.capabilities]
remote_compaction = "v2"
''')
            session = Session(home, environment={'RUST_LOG': 'codex_core::session::async_compaction=debug'})
            try:
                thread = session.call('thread/start', {'cwd': home, 'historyMode': 'paginated',
                    'approvalPolicy': 'never', 'sandbox': 'read-only'})['thread']['id']
                session.call('turn/start', {'threadId': thread, 'input': [{'type': 'text', 'text': 'Remember this request.'}]})
                session.done()
                # Observe the native failure before releasing the next checkpoint. This is
                # diagnostic synchronization, separate from the behavioral assertions below.
                deadline = time.monotonic() + 15
                while b'discarding unsuccessful background compaction' not in os.pread(
                        session.stderr.fileno(), os.fstat(session.stderr.fileno()).st_size, 0):
                    assert time.monotonic() < deadline, 'Native background failure was not observed.'
                    time.sleep(0.01)
                assert len(requests) == 2, requests
                session.call('turn/start', {'threadId': thread, 'input': [{'type': 'text', 'text': 'Continue after the failed background request.'}]})
                session.done()
                assert len(requests) == 5, requests
                assert any(item.get('type') == 'compaction' and
                           item.get('encrypted_content') == 'FOREGROUND_CHECKPOINT'
                           for item in requests[-1]['input']), requests[-1]
                session.call('thread/read', {'threadId': thread, 'includeTurns': False})
                print(json.dumps({'status': 'passed', 'requests': len(requests),
                    'behavior': 'Closed background receiver falls back to a usable foreground checkpoint.'}))
            except BaseException:
                diagnostic = os.pread(session.stderr.fileno(), os.fstat(session.stderr.fileno()).st_size, 0)
                Path(os.environ['DECODEX_QUALIFICATION_OUTPUT'], 'async-compaction-failure.log').write_bytes(diagnostic)
                print(json.dumps({'status': 'failed', 'requests': len(requests),
                    'closed_receiver_panic': b'called after complete' in diagnostic}))
                raise
            finally:
                session.close()
    finally:
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    main()
