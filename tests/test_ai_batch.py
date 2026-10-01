"""Exact AI batch previews, atomic submissions and owner review boundaries."""
import json
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch
from urllib.request import Request, urlopen
from urllib.error import HTTPError
from app.server import AlveServer
from app.vault import Vault, Problem

PASSWORD = 'synthetic AI batch passphrase'
CONFIRMATION = {'concise': True, 'accurateToSource': True, 'structured': True,
                'userConfirmed': True, 'sourceBasis': 'user_statement', 'basis': 'Synthetic user statement.', 'uncertainties': ''}

class AiBatchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.v = Vault(Path(self.temp.name) / 'memory.alve')
        self.owner = self.v.unlock(PASSWORD, True)['token']
        self.scope = self.v.mutate(lambda: self.v.add_node({'title': 'Scope'}))
        self.connection = self.v.mutate(lambda: self.v.grant({'name': 'Synthetic AI', 'nodeIds': [self.scope['id']], 'permissions': ['propose']}))
        self.server = AlveServer(('127.0.0.1', 0), self.v)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True); self.thread.start()

    def tearDown(self):
        self.server.shutdown(); self.server.server_close(); self.thread.join()
        self.v.lock(); self.temp.cleanup()

    def call(self, path, body, token=None):
        req = Request(f'http://127.0.0.1:{self.server.server_port}{path}', json.dumps(body).encode(),
                      headers={'Authorization': 'Bearer ' + (token or self.connection['token']), 'Content-Type': 'application/json'})
        try:
            with urlopen(req) as response: return response.status, json.load(response)
        except HTTPError as error: return error.code, json.load(error)

    def items(self, count=5):
        return [{'content': {'title': f'Responsibility {i}', 'body': 'Synthetic concise responsibility.',
                             'type': 'memory', 'kind': 'record', 'tags': ['responsibility']}} for i in range(count)]

    def prepare(self, items=None, token=None):
        status, result = self.call('/api/ai/proposals/prepare-batch', {'proposals': items or self.items(), 'groupTitle': 'My responsibilities'}, token)
        self.assertEqual(status, 200, result); return result

    def submit(self, preview, **extra):
        return self.call('/api/ai/proposals/submit-batch', {'reviewToken': preview['reviewToken'], 'confirmation': CONFIRMATION, **extra})

    def test_exact_group_persists_and_one_owner_approval_saves_all(self):
        preview = self.prepare()
        self.assertEqual(preview['batch']['group']['relationDirection'], 'member_to_group')
        self.assertEqual(len(self.v.graph()['nodes']), 1)
        status, out = self.submit(preview); self.assertEqual(status, 200, out)
        self.assertEqual(len(self.v.graph()['nodes']), 1)
        self.assertEqual({p['batchId'] for p in out['proposals']}, {preview['batch']['batchId']})
        ids = [p['id'] for p in out['proposals']]
        self.v.lock(); self.owner = self.v.unlock(PASSWORD)['token']
        status, saved = self.call('/api/proposals/review-batch', {'proposalIds': ids, 'action': 'approve'}, self.owner)
        self.assertEqual(status, 200, saved)
        self.assertEqual(len(saved['relations']), 5)
        for key in ('title', 'body', 'type', 'kind', 'tags', 'facts', 'references', 'status'):
            self.assertEqual(saved['group'][key], preview['batch']['group'][key])
        self.assertEqual(saved['group']['origin'], 'user')
        self.assertTrue(all(r['node']['tags'] == ['responsibility'] for r in saved['reviews']))
        self.assertEqual(self.v.rows('connections')[0]['nodeIds'], [self.scope['id']])

    def test_actor_replay_expiry_operation_separation_and_payload_changes(self):
        preview = self.prepare()
        body = {'reviewToken': preview['reviewToken'], 'confirmation': CONFIRMATION}
        self.assertEqual(self.call('/api/ai/proposals/submit-batch', body, self.owner)[0], 409)
        self.assertEqual(self.call('/api/ai/proposals', body)[0], 422)
        self.assertEqual(self.submit(preview, groupTitle='Changed')[0], 422)
        self.assertEqual(self.call('/api/ai/proposals/submit-batch', {**body, 'confirmation': {**CONFIRMATION, 'userConfirmed': False}})[0], 422)
        ticket = self.server.quality.tickets[preview['reviewToken']]
        expires = ticket['expires']; ticket['expires'] = 0
        self.assertEqual(self.submit(preview)[0], 409)
        ticket['expires'] = expires
        self.assertEqual(self.submit(preview)[0], 200)
        self.assertEqual(self.submit(preview)[0], 409)
        status, single = self.call('/api/ai/proposals/prepare', self.items(1)[0]); self.assertEqual(status, 200)
        self.assertEqual(self.submit(single)[0], 422)

    def test_partial_mixed_single_route_and_group_override_cannot_split_batch(self):
        _, out = self.submit(self.prepare()); ids = [p['id'] for p in out['proposals']]
        before = self.v.graph(); disk = self.v.path.read_bytes()
        for action in ('approve', 'reject'):
            self.assertEqual(self.call(f'/api/proposals/{ids[0]}/{action}', {}, self.owner)[0], 409)
            self.assertEqual(self.call('/api/proposals/review-batch', {'proposalIds': ids[:-1], 'action': action}, self.owner)[0], 409)
        self.assertEqual(self.call('/api/proposals/review-batch', {'proposalIds': ids, 'action': 'approve', 'groupTitle': 'Changed'}, self.owner)[0], 409)
        self.assertEqual(self.v.graph(), before); self.assertEqual(self.v.path.read_bytes(), disk)
        other = self.v.mutate(lambda: self.v.propose(self.items(1)[0], None))
        self.assertEqual(self.call('/api/proposals/review-batch', {'proposalIds': [*ids, other['id']], 'action': 'approve'}, self.owner)[0], 409)
        self.assertEqual(self.call('/api/proposals/review-batch', {'proposalIds': ids, 'action': 'reject'}, self.owner)[0], 200)
        self.assertEqual(len(self.v.graph()['nodes']), 1)

    def test_submission_disk_failure_preserves_token_for_retry(self):
        preview = self.prepare(); before = self.v.graph(); disk = self.v.path.read_bytes()
        with patch.object(self.v, 'persist', side_effect=OSError('Synthetic disk failure')):
            self.assertEqual(self.submit(preview)[0], 500)
        self.assertEqual(self.v.graph(), before); self.assertEqual(self.v.path.read_bytes(), disk)
        self.assertEqual(self.submit(preview)[0], 200)

    def test_stale_update_during_submit_or_review_rolls_back_entire_batch(self):
        update = {'action': 'update', 'nodeId': self.scope['id'], 'expectedRevision': self.scope['revisionId'], 'content': {**self.items(1)[0]['content'], 'title': 'Updated scope'}}
        preview = self.prepare([self.items(1)[0], update])
        self.v.mutate(lambda: self.v.add_node({**self.scope, 'title': 'Changed scope'}, self.scope['id'], [self.scope['revisionId']]))
        before = self.v.graph()
        self.assertEqual(self.submit(preview)[0], 409); self.assertEqual(self.v.graph(), before)
        current = self.v.heads()[self.scope['id']][0]; update['expectedRevision'] = current['revisionId']
        _, out = self.submit(self.prepare([self.items(1)[0], update])); ids = [p['id'] for p in out['proposals']]
        self.v.mutate(lambda: self.v.add_node({**current, 'title': 'Changed again'}, current['id'], [current['revisionId']]))
        before = self.v.graph(); disk = self.v.path.read_bytes()
        self.assertEqual(self.call('/api/proposals/review-batch', {'proposalIds': ids, 'action': 'approve'}, self.owner)[0], 409)
        self.assertEqual(self.v.graph(), before); self.assertEqual(self.v.path.read_bytes(), disk)

    def test_revocation_and_relation_capacity_block_whole_review(self):
        _, out = self.submit(self.prepare()); ids = [p['id'] for p in out['proposals']]
        before = self.v.graph()
        with patch('app.vault.MAX_RELATIONS', 0):
            self.assertEqual(self.call('/api/proposals/review-batch', {'proposalIds': ids, 'action': 'approve'}, self.owner)[0], 413)
        self.assertEqual(self.v.graph(), before)
        self.v.mutate(lambda: self.v.revoke(self.connection['connection']['id']))
        before = self.v.graph()
        self.assertEqual(self.call('/api/proposals/review-batch', {'proposalIds': ids, 'action': 'approve'}, self.owner)[0], 403)
        self.assertEqual(self.v.graph(), before)

    def test_prepare_validation_scope_and_owner_update(self):
        hidden = self.v.mutate(lambda: self.v.add_node({'title': 'Private'}))
        update = {'action': 'update', 'nodeId': hidden['id'], 'expectedRevision': hidden['revisionId'], 'content': self.items(1)[0]['content']}
        self.assertEqual(self.call('/api/ai/proposals/prepare-batch', {'proposals': [self.items(1)[0], update], 'groupTitle': 'Group'})[0], 404)
        self.prepare([self.items(1)[0], update], self.owner)
        for items in [[], self.items(1), self.items(51), [None, {}], [update, update]]:
            status, _ = self.call('/api/ai/proposals/prepare-batch', {'proposals': items, 'groupTitle': 'Group'}, self.owner)
            self.assertTrue(400 <= status < 500, status)

    def test_existing_memory_grouping_owner_only_and_atomic(self):
        second = self.v.mutate(lambda: self.v.add_node({'title': 'Second'}))
        body = {'nodeIds': [self.scope['id'], second['id']], 'groupTitle': 'Existing responsibilities'}
        before = self.v.graph(); disk = self.v.path.read_bytes()
        self.assertEqual(self.call('/api/nodes/group', body)[0], 403)
        with patch('app.vault.MAX_RELATIONS', 0): self.assertEqual(self.call('/api/nodes/group', body, self.owner)[0], 413)
        self.assertEqual(self.v.graph(), before); self.assertEqual(self.v.path.read_bytes(), disk)
        status, out = self.call('/api/nodes/group', body, self.owner)
        self.assertEqual(status, 200, out); self.assertEqual(len(out['relations']), 2)
        self.assertEqual(self.v.rows('connections')[0]['nodeIds'], [self.scope['id']])
        self.v.mutate(lambda: self.v.add_node({**second, 'status': 'archived'}, second['id'], [second['revisionId']]))
        before = self.v.graph()
        self.assertEqual(self.call('/api/nodes/group', body, self.owner)[0], 409)
        self.assertEqual(self.v.graph(), before)
