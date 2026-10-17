"""Exercise native hosted Apps with cached discovery and a gated retry."""
import base64
import datetime
import json
import os
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from native_session import Session
n = Path(os.environ['DECODEX_QUALIFICATION_OUTPUT'])
initializes = []
rpc = []
bodies = []
observations = []
failures = 0
gate = threading.Event()
gate.set()
waiting = threading.Event()
discovered = threading.Event()
calls = []

class Backend(BaseHTTPRequestHandler):

    def log_message(self, *a):
        pass

    def respond(self, value, status=200):
        data = json.dumps(value).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        rpc.append({'path': self.path, 'method': 'GET'})
        if 'directory/list_workspace' in self.path:
            self.respond({'apps': [], 'nextToken': None})
        elif 'directory/list' in self.path:
            self.respond({'apps': [{'id': 'fixture_connector', 'name': 'Fixture', 'description': 'Isolated qualification'}], 'nextToken': None})
        elif 'installed' in self.path:
            self.respond({'plugins': [], 'pagination': {'next_page_token': None}})
        else:
            self.respond({}, 404)

    def do_DELETE(self):
        self.respond({})

    def do_POST(self):
        global failures
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        rpc.append({'path': self.path, 'method': body.get('method')})
        if body.get('method') == 'initialize':
            initializes.append(time.monotonic())
            if failures:
                failures -= 1
                self.respond({'error': 'isolated transient startup failure'}, 500)
                return
            if not gate.is_set():
                waiting.set()
            assert gate.wait(35), 'recovery gate deadline'
            result = {'protocolVersion': body['params']['protocolVersion'], 'capabilities': {'tools': {'listChanged': True}}, 'serverInfo': {'name': 'qualification-apps', 'version': '1'}}
        elif body.get('method') == 'tools/list':
            result = {'tools': [{'name': 'qualification_read', 'description': 'Read the isolated fixture.', 'inputSchema': {'type': 'object', 'properties': {}}, 'annotations': {'readOnlyHint': True}, '_meta': {'connector_id': 'fixture_connector', 'connector_name': 'Fixture', '_codex_apps': {'resource_uri': 'fixture://read', 'contains_mcp_source': True, 'connector_id': 'fixture_connector'}}}]}
        elif body.get('method') == 'tools/call':
            calls.append(body['params']['name'])
            result = {'content': [{'type': 'text', 'text': 'RECOVERED_NATIVE_APPS'}], 'isError': False}
        elif body.get('method') == 'notifications/initialized':
            self.respond({}, 202)
            return
        else:
            result = {}
        self.respond({'jsonrpc': '2.0', 'id': body.get('id'), 'result': result})

class Provider(BaseHTTPRequestHandler):

    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        bodies.append(body)
        index = len(bodies) - 1
        if index == 0:
            output = {'type': 'message', 'id': 'warm', 'role': 'assistant', 'content': [{'type': 'output_text', 'text': 'Warm fixture ready.'}]}
        elif index == 1:
            output = {'type': 'custom_tool_call', 'call_id': 'discover', 'name': 'exec', 'input': 'const t=ALL_TOOLS.find(t=>t.name.endsWith("qualification_read")); if(!t) throw new Error("cached Apps tool missing"); store("fixture-tool",t.name); text(t.name);'}
        elif index == 2:
            observations.extend((i for i in body['input'] if i.get('call_id') == 'discover' and i.get('type') == 'custom_tool_call_output'))
            discovered.set()
            output = {'type': 'custom_tool_call', 'call_id': 'invoke', 'name': 'exec', 'input': 'text(await tools[load("fixture-tool")]({}));'}
        else:
            observations.extend((i for i in body['input'] if i.get('call_id') == 'invoke' and i.get('type') == 'custom_tool_call_output'))
            output = {'type': 'message', 'id': 'done', 'role': 'assistant', 'content': [{'type': 'output_text', 'text': 'Recovery complete.'}]}
        if output.get('type') == 'custom_tool_call':
            for tool in list(body.get('tools', [])) + [t for i in body.get('input', []) if i.get('type') == 'additional_tools' for t in i.get('tools', [])]:
                if any((t.get('name') == 'exec' for t in tool.get('tools', []))):
                    output['namespace'] = tool['name']
        frames = [{'type': 'response.created', 'response': {'id': 'a' + str(index)}}, {'type': 'response.output_item.done', 'item': output}, {'type': 'response.completed', 'response': {'id': 'a' + str(index), 'usage': {'input_tokens': 1, 'output_tokens': 1, 'total_tokens': 2}}}]
        data = ''.join(('event: ' + f['type'] + '\ndata: ' + json.dumps(f) + '\n\n' for f in frames)).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)
backend = ThreadingHTTPServer(('127.0.0.1', 0), Backend)
provider = ThreadingHTTPServer(('127.0.0.1', 0), Provider)
for server in [backend, provider]:
    threading.Thread(target=server.serve_forever, daemon=True).start()
try:
    with tempfile.TemporaryDirectory(prefix='decodex-hosted-apps-') as home:
        Path(home, 'config.toml').write_text(f'model="gpt-5.6-sol"\nmodel_provider="fixture"\nchatgpt_base_url="http://127.0.0.1:{backend.server_port}"\ncli_auth_credentials_store="file"\n[features]\nenable_request_compression=false\napps=true\nremote_plugin=false\nplugins=false\n[analytics]\nenabled=false\n[model_providers.fixture]\nname="OpenAI"\nbase_url="http://127.0.0.1:{provider.server_port}"\nwire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n')
        encode = lambda v: base64.urlsafe_b64encode(json.dumps(v).encode()).decode().rstrip('=')
        jwt = encode({'alg': 'none', 'typ': 'JWT'}) + '.' + encode({'email': 'fixture@example.test', 'https://api.openai.com/auth': {'chatgpt_account_id': 'fixture-account', 'chatgpt_plan_type': 'pro'}}) + '.' + encode('fixture-signature')
        Path(home, 'auth.json').write_text(json.dumps({'auth_mode': 'chatgpt', 'tokens': {'id_token': jwt, 'access_token': 'isolated-fixture-access', 'refresh_token': 'isolated-fixture-refresh', 'account_id': 'fixture-account'}, 'last_refresh': datetime.datetime.now(datetime.timezone.utc).isoformat()}))
        session = Session(home)
        try:
            params = {'cwd': home, 'historyMode': 'paginated', 'approvalPolicy': 'never', 'sandbox': 'read-only'}
            warm = session.call('thread/start', params)['thread']['id']
            session.call('turn/start', {'threadId': warm, 'input': [{'type': 'text', 'text': 'Warm the isolated native Apps catalog.'}]})
            session.done()
            assert initializes, 'native hosted Apps did not initialize'
            failures = 2
            gate.clear()
            waiting.clear()
            cold = session.call('thread/start', params)['thread']['id']
            assert waiting.wait(15), 'native retry did not reach recovery gate'
            before = len(initializes)
            session.call('turn/start', {'threadId': cold, 'input': [{'type': 'text', 'text': 'Discover the cached fixture tool, then read it.'}]})
            assert discovered.wait(10), 'cached tool did not remain discoverable during recovery'
            assert not calls, 'Apps invocation must await the gated retry'
            assert len(initializes) == before, 'discovery spawned another startup retry'
            gate.set()
            session.done()
        finally:
            gate.set()
            session.close()
finally:
    for server in [backend, provider]:
        server.shutdown()
        server.server_close()
    (n / 'apps-recovery-observation.json').write_text(json.dumps({'initializes': len(initializes), 'rpc': rpc, 'observations': observations, 'calls': calls, 'provider_requests': len(bodies)}, indent=2) + '\n')
assert calls == ['qualification_read'], calls
assert 'RECOVERED_NATIVE_APPS' in json.dumps(observations), observations
(n / 'native-apps-recovery.json').write_text(json.dumps({'status': 'passed', 'binary': os.environ['QUALIFIED_CODEX_BINARY'], 'initializes': len(initializes), 'provider_requests': len(bodies), 'tool_calls': calls, 'cached_discovery_nonblocking': True, 'shared_retry_gate_observed': True, 'temporary_home_removed': True, 'scope': 'Actual packaged CLI and Code Mode helper; synthetic local hosted Apps and fake account. No provider credentials or production account access.', 'finished_at': datetime.datetime.now(datetime.timezone.utc).isoformat()}, indent=2) + '\n')
print('actual native Apps cached discovery and shared recovery passed')
