"""Qualify native Project identity, review routing and private reviewer observations offline."""
import hashlib
import json
import os
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from native_session import B, Session


def qualify():
    bodies = []
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            bodies.append(body)
            serial = len(bodies)
            if serial == 1:
                item = {'type': 'function_call', 'name': 'exec_command', 'call_id': 'review-fixture', 'arguments': json.dumps({'cmd': 'exit 0', 'sandbox_permissions': 'require_escalated', 'justification': 'Synthetic review fixture'})}
            else:
                text = '{"outcome":"allow"}' if serial == 2 else 'Synthetic review finished.'
                item = {'type': 'message', 'role': 'assistant', 'id': f'output-{serial}', 'content': [{'type': 'output_text', 'text': text}]}
            frames = [{'type': 'response.created', 'response': {'id': f'response-{serial}'}}, {'type': 'response.output_item.done', 'output_index': 0, 'item': item}, {'type': 'response.completed', 'response': {'id': f'response-{serial}', 'usage': {'input_tokens': 10, 'output_tokens': 5, 'total_tokens': 15}}}]
            data = ''.join('event: ' + frame['type'] + '\ndata: ' + json.dumps(frame) + '\n\n' for frame in frames).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='decodex-project-guardian-') as home:
            Path(home, 'config.toml').write_text('model="gpt-5.6-sol"\nmodel_provider="fixture"\n[features]\nguardian_approval=true\nguardian_recorded_items=true\ncontent_item_kinds=true\nenable_request_compression=false\n[model_providers.fixture]\nname="OpenAI"\n' + f'base_url="http://127.0.0.1:{server.server_port}"\n' + 'wire_api="responses"\nrequires_openai_auth=false\nsupports_websockets=false\n')
            session = Session(home)
            try:
                project = session.call('project/create', {'name': 'Community Market', 'roots': [{'path': home}], 'idempotencyKey': 'project-contract-fixture'})['project']
                duplicate = session.call('project/create', {'name': 'Community Market', 'roots': [{'path': home}], 'idempotencyKey': 'project-contract-fixture'})['project']
                assert duplicate['id'] == project['id']
                started = session.call('thread/start', {'cwd': home, 'historyMode': 'paginated', 'projectId': project['id'], 'approvalPolicy': 'on-request', 'approvalsReviewer': 'user', 'sandbox': 'read-only', 'experimentalRawEvents': True})
                parent = started['thread']['id']
                assert started['thread']['projectId'] == project['id']
                assert started['sandbox']['type'] == 'readOnly'
                session.call('thread/settings/update', {'threadId': parent, 'approvalsReviewer': 'auto_review'})
                session.call('turn/start', {'threadId': parent, 'input': [{'type': 'text', 'text': 'Exercise the isolated reviewer. Do not publish anything.'}]})
                reviews, private = [], []
                while True:
                    event = session.events.pop(0) if session.events else session.q.get(timeout=30)
                    method, params = event.get('method'), event.get('params', {})
                    if method == 'error':
                        raise AssertionError(event)
                    if method in ('item/autoApprovalReview/started', 'item/autoApprovalReview/completed'):
                        reviews.append(params)
                    if method == 'rawResponseItem/completed' and params['threadId'] != parent:
                        private.append(params)
                    if method == 'turn/completed' and params['threadId'] == parent:
                        assert params['turn']['status'] == 'completed', event
                        break
                assert reviews and private, {'reviews': reviews, 'private': private}
                stored = session.call('thread/read', {'threadId': parent, 'includeTurns': True})
                assert '"outcome":"allow"' not in json.dumps(stored)
                identities = []
                for record in private:
                    try:
                        read = session.call('thread/read', {'threadId': record['threadId'], 'includeTurns': False})
                        identities.append({'thread_id': record['threadId'], 'turn_id': record['turnId'], 'item_type': record['item']['type'], 'native_thread_source': read['thread'].get('source'), 'native_parent': read['thread'].get('parentThreadId')})
                    except RuntimeError:
                        identities.append({'thread_id': record['threadId'], 'turn_id': record['turnId'], 'item_type': record['item']['type'], 'native_thread_source': None})
                return {'passed': True, 'project_idempotency': True, 'private_identity_observations': identities, 'review_fields': sorted(reviews[-1]), 'private_fields': sorted(private[0]), 'binary_sha256': hashlib.sha256(B.read_bytes()).hexdigest(), 'limits': 'Exact packaged native CLI, temporary homes and loopback synthetic provider. No real account, live provider, installation or app acceptance.'}
            finally:
                session.close()
    finally:
        server.shutdown()

if __name__ == '__main__':
    result = qualify()
    Path(os.environ['DECODEX_QUALIFICATION_OUTPUT'], 'project_guardian.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))
