"""Record reported incomplete spending without replacing the live context baseline."""
import datetime
import json
import os
import queue
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from native_session import Session
r = Path(os.environ['DECODEX_QUALIFICATION_OUTPUT'])

def terminal(session, tid):
    events = []
    end = time.monotonic() + 35
    while time.monotonic() < end:
        event = session.events.pop(0) if session.events else session.q.get(timeout=max(0.01, end - time.monotonic()))
        events.append(event)
        if event.get('method') == 'turn/completed' and event['params']['threadId'] == tid:
            return (event['params']['turn'], events)
        assert not ('method' in event and 'id' in event), event
    raise TimeoutError('turn did not terminate')

def case(reported):
    requests = []

    class Handler(BaseHTTPRequestHandler):

        def log_message(self, *args):
            pass

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            index = len(requests)
            requests.append(body)
            rid = 'usage-' + str(index)
            frames = [{'type': 'response.created', 'response': {'id': rid}}]
            if index == 1:
                response = {'id': rid, 'incomplete_details': {'reason': 'max_output_tokens'}}
                if reported == 'valid':
                    response['usage'] = {'input_tokens': 150000, 'output_tokens': 10, 'total_tokens': 150010}
                if reported == 'malformed':
                    response['usage'] = {'malformed': True}
                frames.append({'type': 'response.incomplete', 'response': response})
            else:
                frames.append({'type': 'response.output_item.done', 'item': {'type': 'message', 'id': 'm-' + rid, 'role': 'assistant', 'content': [{'type': 'output_text', 'text': 'Fixture reply.'}]}})
                usage = {'input_tokens': 20000 if index == 0 else 1, 'output_tokens': 2 if index == 0 else 1, 'total_tokens': 20002 if index == 0 else 2}
                frames.append({'type': 'response.completed', 'response': {'id': rid, 'usage': usage}})
            data = ''.join(('event: ' + f['type'] + '\ndata: ' + json.dumps(f) + '\n\n' for f in frames)).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    snapshots = []
    try:
        with tempfile.TemporaryDirectory(prefix='decodex-incomplete-spending-') as home:
            Path(home, 'config.toml').write_text(f'model="gpt-5.6-sol"\nmodel_provider="fixture"\nmodel_auto_compact_token_limit=100000\nmodel_post_turn_compact_threshold_percent=0\n[features]\nenable_request_compression=false\ntoken_budget=false\n[analytics]\nenabled=false\n[model_providers.fixture]\nname="OpenAI"\nbase_url="http://127.0.0.1:{server.server_port}"\nwire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\nstream_max_retries=2\nrequest_max_retries=0\n')
            session = Session(home)
            try:
                tid = session.call('thread/start', {'cwd': home, 'historyMode': 'paginated', 'approvalPolicy': 'never', 'sandbox': 'read-only'})['thread']['id']
                all_events = []
                for index in range(3):
                    session.call('turn/start', {'threadId': tid, 'input': [{'type': 'text', 'text': 'Usage fixture ' + str(index)}]})
                    turn, events = terminal(session, tid)
                    all_events.extend(events)
                    notifications = [e['params'] for e in events if e.get('method') == 'thread/tokenUsage/updated']
                    snapshots.append({'turn_status': turn['status'], 'usage_notifications': notifications})
            finally:
                session.close()
    finally:
        server.shutdown()
        server.server_close()
    return {'reported': reported, 'requests': len(requests), 'snapshots': snapshots, 'compactions': sum((e.get('method') == 'item/started' and e.get('params', {}).get('item', {}).get('type') == 'contextCompaction' for e in all_events)), 'retrying_errors': sum((e.get('method') == 'error' and e.get('params', {}).get('willRetry') is True for e in all_events))}
phase = 'after'
result = {'phase': phase, 'binary': os.environ['QUALIFIED_CODEX_BINARY'], 'cases': [], 'scope': 'Actual packaged stdio app-server, synthetic SSE usage, no live provider or installed-app acceptance.'}
for reported in ['valid', 'missing', 'malformed']:
    result['cases'].append(case(reported))
    (r / ('incomplete-usage-' + phase + '.json')).write_text(json.dumps(result, indent=2) + '\n')
for item in result['cases']:
    assert item['requests'] == 3 and item['retrying_errors'] == 0 and (item['compactions'] == 0), item
    assert item['snapshots'][1]['turn_status'] == 'failed', item
    first = item['snapshots'][0]['usage_notifications'][-1]['tokenUsage']
    final = item['snapshots'][2]['usage_notifications'][-1]['tokenUsage']
    expected = 170014 if phase == 'after' and item['reported'] == 'valid' else 20004
    assert final['total']['totalTokens'] == expected, (expected, final)
    if phase == 'after' and item['reported'] == 'valid':
        incomplete = item['snapshots'][1]['usage_notifications'][-1]['tokenUsage']
        assert incomplete['total']['totalTokens'] == 170012, incomplete
        assert incomplete['last'] == first['last'], (first, incomplete)
result.update(status='passed', finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), temporary_homes_removed=True)
(r / ('incomplete-usage-' + phase + '.json')).write_text(json.dumps(result, indent=2) + '\n')
print(phase, 'incomplete usage cases passed; valid, missing, malformed; no extra compaction or retry')
