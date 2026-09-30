"""Recovery drill using synthetic data and independently stored encrypted bundles."""
import base64
import tempfile
import unittest
from pathlib import Path
from app.vault import Vault, Problem

PASSWORD = 'synthetic recovery drill passphrase'

class RecoveryDrill(unittest.TestCase):
    def test_dated_backup_restores_history_graph_and_exact_facts(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = Vault(root / 'source' / 'memory.alve')
            recovered = Vault(root / 'fresh-install' / 'memory.alve')
            try:
                source.unlock(PASSWORD, True)
                node = source.mutate(lambda: source.add_node({'title': 'Budget', 'body': 'Synthetic exact budget.',
                    'type': 'memory', 'kind': 'record', 'tags': ['responsibility'],
                    'facts': [{'key': 'budget', 'label': 'Budget', 'precision': 'exact',
                               'value': {'type': 'money', 'amount': '1234.56', 'currency': 'SEK'}}]}))
                source.mutate(lambda: source.add_node({**node, 'body': 'Revised synthetic budget.'}, node['id'], [node['revisionId']]))
                group = source.mutate(lambda: source.add_node({'title': 'Responsibilities', 'type': 'project'}))
                source.mutate(lambda: source.add_relation({'fromId': node['id'], 'toId': group['id'], 'type': 'belongs_to'}))
                grant = source.mutate(lambda: source.grant({'name': 'Test AI', 'nodeIds': [node['id']], 'permissions': ['read']}))
                source.mutate(lambda: source.propose({'content': {'title': 'Unsaved suggestion'}}, None))
                snapshot = source.export()
                backup = root / 'offline-backups' / 'dated-backup.alve'
                backup.parent.mkdir()
                backup.write_bytes(base64.b64decode(source.bundle()['bundle']))
                source.mutate(lambda: source.add_node({'title': 'After the backup'}))
                source.lock()
                raw = backup.read_bytes()
                self.assertNotIn(b'Synthetic exact budget', raw)
                recovered.restore(base64.b64encode(raw).decode(), PASSWORD)
                self.assertEqual(recovered.export(), snapshot)
                self.assertEqual(len(recovered.rows('revisions')), 3)
                self.assertEqual(len(recovered.graph()['relations']), 1)
                self.assertEqual(recovered.graph()['proposals'], [])
                self.assertEqual(recovered.graph()['connections'], [])
                with self.assertRaises(Problem): recovered.auth(grant['token'])
                recovered.lock()
                recovered.unlock(PASSWORD)
                self.assertEqual(recovered.export(), snapshot)
            finally:
                source.lock(); recovered.lock()

    def test_wrong_password_tampering_and_existing_vault_preserve_storage(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, target = Vault(root / 'source.alve'), Vault(root / 'target.alve')
            try:
                source.unlock(PASSWORD, True)
                source.mutate(lambda: source.add_node({'title': 'Recover me'}))
                bundle = source.bundle()['bundle']
                raw = bytearray(base64.b64decode(bundle)); raw[-1] ^= 1
                for candidate, password in [(bundle, 'wrong password'), (base64.b64encode(raw).decode(), PASSWORD)]:
                    with self.assertRaises(Problem): target.restore(candidate, password)
                    self.assertFalse(target.path.exists())
                    self.assertIsNone(target.db)
                target.unlock(PASSWORD, True)
                target.mutate(lambda: target.add_node({'title': 'Do not overwrite'}))
                before = target.path.read_bytes()
                with self.assertRaises(Problem): target.restore(bundle, PASSWORD)
                self.assertEqual(target.path.read_bytes(), before)
                self.assertEqual(target.graph()['nodes'][0]['title'], 'Do not overwrite')
            finally:
                source.lock(); target.lock()
