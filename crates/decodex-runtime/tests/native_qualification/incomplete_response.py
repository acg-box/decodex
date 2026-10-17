"""Exercise terminal provider responses through the selected native app-server."""
import json
import os
import queue
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from native_session import Session

def terminal(session, thread):
    events = []
    deadline = time.monotonic() + float(os.environ.get('QUALIFICATION_TIMEOUT', '15'))
    while time.monotonic() < deadline:
        try:
            item = session.events.pop(0) if session.events else session.q.get(timeout=max(0.01, deadline - time.monotonic()))
        except queue.Empty:
            return (None, events)
        events.append(item)
        if item.get('method') == 'turn/completed' and item['params']['threadId'] == thread:
            return (item['params']['turn'], events)
        if 'method' in item and 'id' in item:
            raise AssertionError('Unexpected request: ' + item['method'])
    return (None, events)

def case(reason, compact):
    requests = []

    class Handler(BaseHTTPRequestHandler):

        def log_message(self, *args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            serial = len(requests)
            requests.append({'path': self.path, 'input_types': [x.get('type') for x in body.get('input', [])[:16]], 'input_item_count': len(body.get('input', []))})
            response_id = 'fixture-' + str(serial)
            frames = [{'type': 'response.created', 'response': {'id': response_id}}]
            if compact and serial == 0 or (reason == 'interrupted' and serial > 0):
                frames += [{'type': 'response.output_item.done', 'item': {'type': 'message', 'id': 'initial', 'role': 'assistant', 'content': [{'type': 'output_text', 'text': 'Initial reply.'}]}}, {'type': 'response.completed', 'response': {'id': response_id, 'usage': {'input_tokens': 250000 if compact else 100, 'output_tokens': 0, 'total_tokens': 250000 if compact else 100}}}]
            else:
                frames += [{'type': 'response.output_item.done', 'item': {'type': 'message', 'id': 'partial-' + str(serial), 'role': 'assistant', 'content': [{'type': 'output_text', 'text': 'Partial reply.'}]}}, {'type': 'response.incomplete', 'response': {'id': response_id, 'incomplete_details': {'reason': reason}}}]
            data = ''.join(('event: ' + f['type'] + '\ndata: ' + json.dumps(f) + '\n\n' for f in frames)).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='decodex-incomplete-') as home:
            Path(home, 'config.toml').write_text(f'model="gpt-5.6-sol"\nmodel_provider="fixture"\nmodel_auto_compact_token_limit=200000\n[features]\nenable_request_compression=false\n[model_providers.fixture]\nname="OpenAI"\nbase_url="http://127.0.0.1:{server.server_port}"\nwire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\nstream_max_retries=2\nrequest_max_retries=0\n')
            session = Session(home)
            all_events = []
            try:
                tid = session.call('thread/start', {'cwd': home, 'historyMode': 'paginated', 'approvalPolicy': 'never', 'sandbox': 'read-only'})['thread']['id']
                for index in range(2 if compact else 1):
                    session.call('turn/start', {'threadId': tid, 'input': [{'type': 'text', 'text': 'Continue fixture ' + str(index)}]})
                    turn, events = terminal(session, tid)
                    all_events += events
            finally:
                session.close()
    finally:
        server.shutdown()
        server.server_close()
    errors = [x['params'] for x in all_events if x.get('method') == 'error']
    return {'reason': reason, 'compaction': compact, 'requests': requests, 'terminal_status': turn['status'] if turn else 'observation_timeout', 'terminal_error': turn.get('error') if turn else None, 'errors': errors, 'retrying_errors': sum((x.get('willRetry') is True for x in errors)), 'assistant_items': sum((x.get('method') == 'item/completed' and x.get('params', {}).get('item', {}).get('type') == 'agentMessage' for x in all_events))}
if __name__ == '__main__':
    result = {'binary': os.environ['QUALIFIED_CODEX_BINARY'], 'cases': []}
    progress = Path(os.environ['DECODEX_QUALIFICATION_OUTPUT']) / 'incomplete-progress.json'
    for reason, compact in [('max_output_tokens', False), ('unknown_fixture_reason', False), ('max_output_tokens', True), ('content_filter', False), ('interrupted', False)]:
        result['cases'].append(case(reason, compact))
        progress.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))
