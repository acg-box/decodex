import json, os, subprocess, tempfile, threading, queue, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
B = Path(os.environ['QUALIFIED_CODEX_BINARY'])
_EOF = object()

class Session:

    def __init__(self, home, args=()):
        self.stderr = tempfile.TemporaryFile(mode='w+t')
        self.home = home
        self.p = subprocess.Popen([str(B), 'app-server', *args], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.stderr, text=True, cwd=home, env={'HOME': home, 'CODEX_HOME': home, 'PATH': '/usr/bin:/bin:/usr/sbin:/sbin'})
        self.q = queue.Queue()
        self.events = []
        self.serial = 0

        def read():
            try:
                for line in self.p.stdout:
                    self.q.put(json.loads(line))
            finally:
                self.q.put(_EOF)
        threading.Thread(target=read, daemon=True).start()
        try:
            self.call('initialize', {'clientInfo': {'name': 'decodex_qualification', 'version': '0.1'}, 'capabilities': {'experimentalApi': True}})
            self.send({'method': 'initialized'})
        except BaseException:
            self.close(diagnostic=True)
            raise

    def send(self, v):
        self.p.stdin.write(json.dumps(v) + '\n')
        self.p.stdin.flush()

    def call(self, m, p):
        self.serial += 1
        i = self.serial
        self.send({'id': i, 'method': m, 'params': p})
        end = time.monotonic() + 30
        while time.monotonic() < end:
            v = self.q.get(timeout=max(0.01, end - time.monotonic()))
            if v is _EOF:
                self.stderr.seek(0)
                raise RuntimeError('App-server closed during ' + m + ': ' + self.stderr.read())
            if v.get('id') == i:
                if 'error' in v:
                    raise RuntimeError(v['error'])
                return v['result']
            self.events.append(v)
        raise TimeoutError(m)

    def done(self, count=1):
        completed = set()
        end = time.monotonic() + 35
        while len(completed) < count:
            v = self.events.pop(0) if self.events else self.q.get(timeout=max(0.01, end - time.monotonic()))
            if v is _EOF:
                raise RuntimeError('App-server closed before turn completion')
            if v.get('method') == 'turn/completed':
                assert v['params']['turn']['status'] == 'completed', v
                completed.add(v['params']['threadId'])
            if v.get('method') == 'error':
                raise RuntimeError(v)
            if 'id' in v and 'method' in v:
                raise RuntimeError(v['method'])
        return completed

    def close(self, diagnostic=False):
        before = self.p.poll()
        if before is None:
            self.p.terminate()
        try:
            self.p.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.p.kill()
            self.p.wait()
        if diagnostic:
            self.stderr.seek(0)
            text = self.stderr.read()
            root = Path(os.environ['DECODEX_QUALIFICATION_OUTPUT'])
            stem = 'session-init-failure-' + str(self.p.pid) + '-' + str(time.time_ns())
            (root / (stem + '.log')).write_text(text)
            (root / (stem + '.json')).write_text(json.dumps({'binary': str(B), 'pid': self.p.pid, 'exit_before_cleanup': before, 'exit_after_cleanup': self.p.returncode, 'home': self.home, 'limits': 'Initialization deadline stays 30 seconds; owned fixture cleanup and stderr capture only.'}, indent=2) + '\n')
        self.stderr.close()
