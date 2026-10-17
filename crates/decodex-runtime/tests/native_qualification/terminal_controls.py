import datetime
import importlib.util
import json
import os
import re
import shlex
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
r = Path(os.environ['DECODEX_QUALIFICATION_OUTPUT'])
binary = Path(os.environ['QUALIFIED_CODEX_BINARY'])
env = os.environ.copy()
env.update(QUALIFIED_CODEX_BINARY=str(binary), QUALIFICATION_PROGRESS=str(r / 'incomplete-progress.json'))
old = Path(__file__).parent
with (r / 'incomplete-target.json').open('w') as output, (r / 'incomplete-target.stderr').open('w') as error:
    completed = subprocess.run(['python3', str(old / 'incomplete_response.py')], env=env, stdout=output, stderr=error)
assert completed.returncode == 0, 'Incomplete-response fixture failed to run'
data = json.loads((r / 'incomplete-target.json').read_text())
expected = [('max_output_tokens', False, 1, 'failed', 0), ('unknown_fixture_reason', False, 1, 'failed', 0), ('max_output_tokens', True, 2, 'failed', 0), ('content_filter', False, 3, 'failed', 2), ('interrupted', False, 2, 'completed', 0)]
assert len(data['cases']) == len(expected)
for case, expected_case in zip(data['cases'], expected):
    assert (case['reason'], case['compaction'], len(case['requests']), case['terminal_status'], case['retrying_errors']) == expected_case, case
    if case['reason'] in ('max_output_tokens', 'unknown_fixture_reason'):
        assert case['reason'] in case['terminal_error']['message']
        assert case['terminal_error']['codexErrorInfo'] == 'other'
(r / 'incomplete-regression.json').write_text(json.dumps({'status': 'passed', 'source_commit': os.environ['QUALIFIED_CODEX_SOURCE_COMMIT'], 'binary': str(binary), 'cases_passed': 5, 'transport': 'loopback SSE only'}, indent=2) + '\n')
print('Five terminal and retry controls passed')
