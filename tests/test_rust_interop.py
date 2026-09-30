"""Exercise the compiled Rust API and Python snapshot/bundle compatibility."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from app.vault import Vault

DRIVER = Path(os.environ.get('ALVE_CORE_CHECK', 'target/debug/alve-core-check.exe'))
PASSWORD = 'synthetic interoperability passphrase'
CONFIRM = {'concise': True, 'accurateToSource': True, 'structured': True, 'userConfirmed': True,
           'sourceBasis': 'user_statement', 'basis': 'Synthetic owner statement.', 'uncertainties': ''}


@unittest.skipUnless(DRIVER.is_file(), 'Build the Rust acceptance driver first.')
class RustInterop(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.directory = Path(self.temp.name)
        self.processes = []
        self.p = self.start('rust')
        self.token = self.call(self.p, 'POST', '/api/unlock', {'password': PASSWORD, 'create': True})['token']

    def start(self, folder):
        p = subprocess.Popen([str(DRIVER.resolve()), str(self.directory / folder)], stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.processes.append(p)
        return p

    def call(self, p, method, path, body=None, token=None, expected=200):
        p.stdin.write(json.dumps({'method': method, 'path': path, 'body': body or {}, 'token': token or ''}) + '\n')
        p.stdin.flush()
        line = p.stdout.readline()
        self.assertTrue(line, 'Rust acceptance driver exited unexpectedly.')
        reply = json.loads(line)
        self.assertEqual(reply['status'], expected, reply['data'])
        return reply['data']

    def owner(self, method, path, body=None, expected=200):
        return self.call(self.p, method, path, body, self.token, expected)

    def tearDown(self):
        for p in self.processes:
            p.stdin.close()
            p.wait(timeout=10)
            p.stdout.close()
            p.stderr.close()
        self.temp.cleanup()

    def test_snapshot_round_trip_and_typed_values(self):
        facts = [{'key': 'cost', 'label': 'Cost', 'value': {'type': 'money', 'amount': '1200.50', 'currency': 'SEK'}},
                 {'key': 'flag', 'label': 'Flag', 'value': {'type': 'boolean', 'value': False}},
                 {'key': 'unknown', 'label': 'Unknown', 'value': None}]
        n = self.owner('POST', '/api/nodes', {'title': 'Portable memory', 'body': 'Synthetic interop text.', 'facts': facts})
        self.owner('POST', '/api/lock')
        # The Python reference must read the Rust-generated encrypted SQLite snapshot.
        v = Vault(self.directory / 'rust/memory.alve')
        try:
            v.unlock(PASSWORD)
            self.assertEqual(v.graph()['nodes'][0]['facts'], n['facts'])
            v.mutate(lambda: v.add_node({'title': 'Python edit', 'facts': facts}, n['id'], [n['revisionId']]))
        finally:
            v.lock()
        self.token = self.call(self.p, 'POST', '/api/unlock', {'password': PASSWORD})['token']
        self.assertEqual(self.owner('GET', '/api/graph')['nodes'][0]['title'], 'Python edit')

    def test_bundle_exchange_retains_conflicts_and_excludes_credentials(self):
        n = self.owner('POST', '/api/nodes', {'title': 'Original'})
        self.owner('POST', '/api/connections', {'name': 'Local client', 'nodeIds': [n['id']], 'permissions': ['read']})
        bundle = self.owner('POST', '/api/bundle')['bundle']
        peer = self.start('peer')
        token = self.call(peer, 'POST', '/api/restore', {'bundle': bundle, 'password': PASSWORD})['token']
        self.assertEqual(self.call(peer, 'GET', '/api/graph', token=token)['connections'], [])
        self.owner('PATCH', '/api/nodes/' + n['id'], {'title': 'Computer edit', 'expectedRevision': n['revisionId']})
        self.call(peer, 'PATCH', '/api/nodes/' + n['id'], {'title': 'Phone edit', 'expectedRevision': n['revisionId']}, token)
        incoming = self.call(peer, 'POST', '/api/bundle', {}, token)['bundle']
        merged = self.owner('POST', '/api/import', {'bundle': incoming, 'password': PASSWORD})
        self.assertEqual(merged['conflicts'], 1)
        self.assertEqual(self.owner('POST', '/api/import', {'bundle': incoming, 'password': PASSWORD})['addedRevisions'], 0)
        graph = self.owner('GET', '/api/graph')
        heads = graph['conflicts'][0]['revisionIds']
        self.owner('POST', '/api/conflicts/' + n['id'] + '/resolve', {'revisionIds': heads, 'content': {'title': 'Resolved'}})
        self.assertEqual(self.owner('GET', '/api/graph')['conflicts'], [])

    def test_scope_quality_confirmation_and_revoked_approval(self):
        node = self.owner('POST', '/api/nodes', {'title': 'Allowed', 'tags': ['budget']})
        hidden = self.owner('POST', '/api/nodes', {'title': 'Hidden', 'tags': ['budget']})
        connection = self.owner('POST', '/api/connections', {'name': 'Client', 'vaultAlias': 'personal-memory',
            'nodeIds': [node['id']], 'permissions': ['search', 'read', 'propose']})
        token = connection['token']
        result = self.call(self.p, 'GET', '/api/ai/search?tag=budget', token=token)
        self.assertEqual([n['id'] for n in result['nodes']], [node['id']])
        self.assertEqual(result['vaultAlias'], 'personal-memory')
        self.call(self.p, 'GET', '/api/ai/nodes/' + hidden['id'], token=token, expected=404)
        self.call(self.p, 'POST', '/api/nodes', {'title': 'Bypass'}, token, expected=403)
        self.call(self.p, 'POST', '/api/ai/proposals', {'content': {'title': 'Bypass'}}, token, expected=422)
        preview = self.call(self.p, 'POST', '/api/ai/proposals/prepare', {'content': {'title': 'Candidate', 'type': 'memory', 'kind': 'insight'}}, token)
        final = {'reviewToken': preview['reviewToken'], 'confirmation': CONFIRM}
        proposal = self.call(self.p, 'POST', '/api/ai/proposals', final, token)
        self.call(self.p, 'POST', '/api/ai/proposals', final, token, expected=409)
        self.owner('DELETE', '/api/connections/' + connection['connection']['id'])
        self.owner('POST', '/api/proposals/' + proposal['id'] + '/approve', {}, expected=403)
        self.assertEqual(len(self.owner('GET', '/api/graph')['nodes']), 2)
