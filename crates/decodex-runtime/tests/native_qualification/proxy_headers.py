import os, tempfile, subprocess, threading, time, json, socket, http.client, hashlib, datetime
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
n = Path(os.environ['DECODEX_QUALIFICATION_OUTPUT'])
b = Path(os.environ['QUALIFIED_CODEX_BINARY'])
observed = []

class Handler(BaseHTTPRequestHandler):

    def log_message(self, *a):
        pass

    def do_POST(self):
        self.rfile.read(int(self.headers.get('Content-Length', '0')))
        observed.append({'host': self.headers.get_all('Host'), 'authorization': self.headers.get_all('Authorization'), 'probe': self.headers.get_all('X-Probe')})
        data = b'{"ok":true}'
        self.send_response(200)
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)
server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
try:
    with tempfile.TemporaryDirectory(prefix='decodex-native-proxy-') as home:
        info = Path(home) / 'server-info.json'
        p = subprocess.Popen([str(b), 'responses-api-proxy', '--server-info', str(info), '--http-shutdown', '--upstream-url', f'http://127.0.0.1:{server.server_port}/v1/responses'], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, env={'HOME': home, 'CODEX_HOME': home, 'PATH': '/usr/bin:/bin:/usr/sbin:/sbin'})
        try:
            p.stdin.write(b'isolated-fixture-key\n')
            p.stdin.close()
            end = time.monotonic() + 15
            while not info.exists():
                assert p.poll() is None, 'fixture proxy exited'
                assert time.monotonic() < end, 'fixture startup timeout'
                time.sleep(0.05)
            port = json.loads(info.read_text())['port']
            c = http.client.HTTPConnection('127.0.0.1', port, timeout=10)
            c.request('POST', '/v1/responses', body=b'{"input":[]}', headers={'Host': 'incoming.invalid', 'Authorization': 'Bearer incoming-fixture', 'X-Probe': 'preserved'})
            resp = c.getresponse()
            assert resp.status == 200
            assert json.loads(resp.read()) == {'ok': True}
            c.close()
            assert observed == [{'host': [f'127.0.0.1:{server.server_port}'], 'authorization': ['Bearer isolated-fixture-key'], 'probe': ['preserved']}], 'proxy must replace caller authority and authorization while retaining ordinary headers'
            with socket.create_connection(('127.0.0.1', port), timeout=10) as conn:
                conn.sendall(b'POST /v1/responses HTTP/1.1\r\nHost: incoming.invalid\r\nHost: alternate.invalid\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}')
                reply = conn.recv(4096)
            assert b'200' in reply.split(b'\r\n', 1)[0], reply[:100]
            assert len(observed) == 2
            assert observed[1]['host'] == [f'127.0.0.1:{server.server_port}']
            assert observed[1]['authorization'] == ['Bearer isolated-fixture-key']
            c = http.client.HTTPConnection('127.0.0.1', port, timeout=10)
            c.request('GET', '/shutdown')
            c.getresponse().read()
            c.close()
            p.wait(timeout=5)
        finally:
            if p.poll() is None:
                p.terminate()
                p.wait(timeout=5)
finally:
    server.shutdown()
    server.server_close()
(n / 'proxy-host-qualification.json').write_text(json.dumps({'status': 'passed', 'source_commit': os.environ['QUALIFIED_CODEX_SOURCE_COMMIT'], 'binary_sha256': hashlib.sha256(b.read_bytes()).hexdigest(), 'outgoing_host_uses_configured_upstream': True, 'caller_authorization_replaced': True, 'ordinary_header_preserved': True, 'duplicate_host_normalized_to_configured_upstream': True, 'scope': 'Exact packaged CLI, loopback upstream, synthetic stdin authentication and system-temporary home. Owned child and server closed; no live provider or app installation.', 'finished_at': datetime.datetime.now(datetime.timezone.utc).isoformat()}, indent=2) + '\n')
print('Native proxy Host and authorization regression passed')
