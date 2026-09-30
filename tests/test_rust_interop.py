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

    def test_agent_introduction_revision_and_recovery(self):
        draft = {'name': 'Project manager', 'mission': 'Follow Project A.', 'method': 'Check next steps.',
                 'escalation': 'Ask when blocked.', 'firstAssignment': 'Review current commitments.',
                 'phase': 'introduced', 'understanding': '', 'contextIds': [], 'reviewDate': '2026-10-05'}
        script = "const m=require('./app/static/agents.js');let s='';process.stdin.on('data',d=>s+=d);process.stdin.on('end',()=>process.stdout.write(JSON.stringify(m.content(JSON.parse(s)))));"
        def content():
            return json.loads(subprocess.run(['node','-e',script],input=json.dumps(draft),capture_output=True,text=True,check=True).stdout)
        project = self.owner('POST','/api/nodes',{'title':'Project A','type':'project'})
        grant = self.owner('POST','/api/connections',{'name':'Scoped AI','nodeIds':[project['id']], 'permissions':['read']})
        draft['contextIds'] = [project['id']]
        node = self.owner('POST','/api/nodes',content())
        draft.update(phase='ready',understanding='I follow Project A and ask before changes.')
        ready = self.owner('PATCH','/api/nodes/'+node['id'],{**content(),'expectedRevision':node['revisionId']})
        self.owner('PATCH','/api/nodes/'+node['id'],{**content(),'expectedRevision':node['revisionId']},expected=409)
        self.call(self.p,'GET','/api/ai/nodes/'+node['id'],token=grant['token'],expected=404)
        bundle = self.owner('POST','/api/bundle')['bundle']
        self.owner('POST','/api/lock')
        self.token = self.call(self.p,'POST','/api/unlock',{'password':PASSWORD})['token']
        self.assertEqual(next(n for n in self.owner('GET','/api/graph')['nodes'] if n['id']==node['id'])['facts'], ready['facts'])
        recovered = Vault(self.directory/'recovered-agent/memory.alve')
        try:
            recovered.restore(bundle,PASSWORD)
            self.assertEqual(recovered.heads()[node['id']][0]['facts'],ready['facts'])
            self.assertEqual(recovered.graph()['connections'],[])
        finally:
            recovered.lock()

    def test_prepared_batch_group_and_backup_restore_across_runtimes(self):
        proposals = [{'content': {'title': f'Responsibility {i}', 'body': 'Synthetic batch recovery.',
                                  'type': 'memory', 'kind': 'record', 'tags': ['responsibility']}} for i in range(5)]
        preview = self.owner('POST', '/api/ai/proposals/prepare-batch', {'proposals': proposals, 'groupTitle': 'My responsibilities'})
        submitted = self.owner('POST', '/api/ai/proposals/submit-batch', {'reviewToken': preview['reviewToken'], 'confirmation': CONFIRM})
        ids = [p['id'] for p in submitted['proposals']]
        self.owner('POST', '/api/proposals/review-batch', {'proposalIds': ids[:-1], 'action': 'approve'}, expected=409)
        self.owner('POST', '/api/proposals/' + ids[0] + '/approve', expected=409)
        approved = self.owner('POST', '/api/proposals/review-batch', {'proposalIds': ids, 'action': 'approve'})
        self.assertEqual(len(approved['relations']), 5)
        graph = self.owner('GET', '/api/graph')
        bundle = self.owner('POST', '/api/bundle')['bundle']
        v = Vault(self.directory / 'python-recovery/memory.alve')
        try:
            v.restore(bundle, PASSWORD)
            self.assertEqual(v.vault_id, graph['vaultId'])
            self.assertEqual({n['id']: n for n in v.graph()['nodes']}, {n['id']: n for n in graph['nodes']})
            self.assertEqual(v.graph()['relations'], graph['relations'])
            self.assertEqual(v.graph()['proposals'], [])
            self.assertEqual(v.graph()['connections'], [])
            v.lock(); v.unlock(PASSWORD)
            self.assertEqual(len(v.graph()['nodes']), 6)
        finally:
            v.lock()

    def test_agent_run_exact_report_approval_and_portable_resume(self):
        project=self.owner('POST','/api/nodes',{'title':'Project A','type':'project'})
        draft={'name':'Manager','mission':'Follow Project A.','method':'Review next steps.','escalation':'Ask when blocked.','firstAssignment':'Suggest next steps.','understanding':'I propose next steps and ask before changes.','phase':'ready','contextIds':[project['id']],'reviewDate':''}
        script="const m=require('./app/static/agents.js');let s='';process.stdin.on('data',d=>s+=d);process.stdin.on('end',()=>process.stdout.write(JSON.stringify(m.content(JSON.parse(s)))));"
        content=json.loads(subprocess.run(['node','-e',script],input=json.dumps(draft),capture_output=True,text=True,check=True).stdout)
        agent=self.owner('POST','/api/nodes',content)
        grant=self.owner('POST','/api/connections',{'name':'Run AI','nodeIds':[agent['id'],project['id']],'permissions':['read','run','propose']})
        def ai(method,path,body=None,expected=200):return self.call(self.p,method,path,body,grant['token'],expected)
        listing=ai('GET','/api/ai/agent-assignments');self.assertEqual(listing['assignments'][0]['status'],'due')
        body={'agentId':agent['id'],'requestId':'first-request'}
        run=ai('POST','/api/ai/agent-briefing',body)
        self.assertEqual(run,ai('POST','/api/ai/agent-briefing',body))
        self.assertEqual(run['assignment']['mission'],draft['mission'])
        report={'runId':run['run']['runId'],'report':{'workPerformed':'Reviewed Project A.','result':'Need a deadline.','uncertainties':'Deadline unknown.','remaining':'Ask user.','nextAction':'Ask for the deadline.','outcome':'partial','nextFollowUp':'2030-10-05T10:00:00+00:00','references':[]}}
        preview=ai('POST','/api/ai/agent-reports/prepare',report)
        confirmation={'reviewToken':preview['reviewToken'],'confirmation':CONFIRM}
        ai('POST','/api/ai/proposals',confirmation,expected=409)
        submitted=ai('POST','/api/ai/agent-reports/submit',confirmation)
        self.owner('POST','/api/proposals/'+submitted['proposal']['id']+'/approve',expected=409)
        approved=self.owner('POST','/api/agent-runs/'+run['run']['runId']+'/approve-report')
        self.assertEqual(approved['node']['body'],preview['content']['body'])
        self.assertEqual(approved['node']['facts'],preview['content']['facts'])
        resumed=ai('POST','/api/ai/agent-briefing',{'agentId':agent['id'],'requestId':'second-request'})
        self.assertEqual(resumed['lastReport']['id'],approved['node']['id'])
        # A Rust-started ledger can also be resumed through the reference implementation.
        self.owner('POST','/api/lock')
        v=Vault(self.directory/'rust/memory.alve')
        try:
            v.unlock(PASSWORD)
            from app.agents import AgentRuns
            service=AgentRuns(v)
            connection=v.auth(grant['token'])
            retry=v.mutate(lambda:service.briefing({'agentId':agent['id'],'requestId':'second-request'},connection))
            self.assertEqual(retry,{k:value for k,value in resumed.items() if k not in {'vaultId','vaultAlias'}})
            bundle=v.bundle()['bundle']
        finally:v.lock()
        restored=Vault(self.directory/'agent-recovered/memory.alve')
        try:
            restored.restore(bundle,PASSWORD)
            from app.agents import AgentRuns
            service=AgentRuns(restored)
            run=restored.mutate(lambda:service.briefing({'agentId':agent['id'],'requestId':'recovered'},None))
            self.assertEqual(run['lastReport']['id'],approved['node']['id'])
            self.assertEqual(len(service.load()),1)
        finally:restored.lock()

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
