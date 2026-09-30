"""Agent content uses existing storage without granting AI permissions."""
import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from app.vault import Vault, Problem


class AgentStorage(unittest.TestCase):
    def test_introduction_confirmation_reopen_and_recovery_preserve_scope(self):
        draft = dict(name='Project manager', mission='Keep Project A moving.',
                     method='Review commitments.', escalation='Ask when blocked.',
                     firstAssignment='Suggest the next step.', understanding='',
                     phase='introduced', contextIds=[], reviewDate='2026-10-05')
        def content(value):
            script = "const m=require('./app/static/agents.js');let s='';process.stdin.on('data',d=>s+=d);process.stdin.on('end',()=>process.stdout.write(JSON.stringify(m.content(JSON.parse(s)))));"
            return json.loads(subprocess.run(['node', '-e', script], input=json.dumps(value),
                                            capture_output=True, text=True, check=True).stdout)
        with tempfile.TemporaryDirectory() as directory:
            source, restored = Vault(Path(directory)/'source.alve'), Vault(Path(directory)/'restore.alve')
            try:
                source.unlock('synthetic agent storage passphrase', True)
                project = source.mutate(lambda: source.add_node({'title':'Project A','type':'project'}))
                grant = source.mutate(lambda: source.grant({'name':'Existing AI','nodeIds':[project['id']], 'permissions':['read']}))
                draft['contextIds'] = [project['id']]
                introduced = source.mutate(lambda: source.add_node(content(draft)))
                # Creating an agent does not change an existing grant.
                self.assertEqual(source.auth(grant['token'])['nodeIds'], [project['id']])
                draft.update(phase='ready', understanding='I follow Project A and request approval.')
                ready = source.mutate(lambda: source.add_node(content(draft), introduced['id'], [introduced['revisionId']]))
                before = source.path.read_bytes()
                with self.assertRaises(Problem):
                    source.mutate(lambda: source.add_node(content(draft), introduced['id'], [introduced['revisionId']]))
                self.assertEqual(source.path.read_bytes(), before)
                bundle = source.bundle()['bundle']
                source.lock(); source.unlock('synthetic agent storage passphrase')
                self.assertEqual(source.heads()[ready['id']][0]['facts'],ready['facts'])
                restored.restore(bundle,'synthetic agent storage passphrase')
                self.assertEqual(restored.heads()[ready['id']][0]['facts'],ready['facts'])
                self.assertEqual(restored.graph()['connections'],[])
            finally:
                source.lock(); restored.lock()
