"""HTTP lifecycle, scoped handoffs, approval and failure recovery with synthetic data."""
import json
import tempfile
import threading
import unittest
from datetime import timedelta
from pathlib import Path
from unittest.mock import patch
from urllib.request import Request, urlopen
from urllib.error import HTTPError
from app.server import AlveServer
from app.vault import Vault, canonical
from app.agents import utc

CONFIRM={'concise':True,'accurateToSource':True,'structured':True,'userConfirmed':True,'sourceBasis':'inference','basis':'Synthetic reported work, not verified project facts.','uncertainties':'Reported by AI.'}


class AgentRunTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.v=Vault(Path(self.temp.name)/'memory.alve')
        self.owner=self.v.unlock('synthetic agent runs passphrase',True)['token']
        self.a=self.v.mutate(lambda:self.v.add_node({'title':'Project A','type':'project','body':'Private context A.'}))
        self.b=self.v.mutate(lambda:self.v.add_node({'title':'Project B','type':'project','body':'Private context B.'}))
        facts=[{'key':'agent_'+k,'label':k,'value':{'type':'text','value':v}} for k,v in {'mission':'Follow projects.','method':'Review next steps.','escalation':'Ask when blocked.','firstAssignment':'Suggest next steps.','understanding':'I review projects and ask before changes.','phase':'ready','reviewDate':'','context_1':self.a['id']}.items()]
        self.agent=self.v.mutate(lambda:self.v.add_node({'title':'Manager','type':'memory','kind':'record','tags':['alve-agent'],'facts':facts}))
        self.conn=self.grant([self.agent['id'],self.a['id']])
        self.server=AlveServer(('127.0.0.1',0),self.v);self.thread=threading.Thread(target=self.server.serve_forever,daemon=True);self.thread.start()

    def tearDown(self):
        self.server.shutdown();self.server.server_close();self.thread.join();self.v.lock();self.temp.cleanup()

    def grant(self,ids,perms=None):
        return self.v.mutate(lambda:self.v.grant({'name':'Synthetic AI','nodeIds':ids,'permissions':perms or ['read','run','propose']}))

    def call(self,path,body=None,token=None):
        req=Request(f'http://127.0.0.1:{self.server.server_port}{path}',json.dumps(body).encode() if body is not None else None,headers={'Authorization':'Bearer '+(token or self.conn['token']),'Content-Type':'application/json'})
        try:
            with urlopen(req) as r:return r.status,json.load(r)
        except HTTPError as e:return e.code,json.load(e)

    def start(self,request='request-one',token=None):
        st,r=self.call('/api/ai/agent-briefing',{'agentId':self.agent['id'],'requestId':request},token)
        self.assertEqual(st,200,r);return r

    def report(self,run):
        return {'runId':run['run']['runId'],'report':{'workPerformed':'Reviewed the selected project.','result':'A next step is needed.','uncertainties':'Deadline is unknown.','remaining':'Ask for the deadline.','nextAction':'Ask the user for the deadline.','outcome':'partial','nextFollowUp':(utc()+timedelta(days=1)).isoformat(),'references':[]}}

    def prepare(self,run):
        st,p=self.call('/api/ai/agent-reports/prepare',self.report(run));self.assertEqual(st,200,p);return p

    def submit(self,p):
        return self.call('/api/ai/agent-reports/submit',{'reviewToken':p['reviewToken'],'confirmation':CONFIRM})

    def review(self,run,approve=True):
        return self.call('/api/agent-runs/'+run['run']['runId']+('/approve-report' if approve else '/reject-report'),{},self.owner)

    def expire(self,run):
        def action():
            runs=self.server.agents.load();next(r for r in runs if r['runId']==run['run']['runId'])['expiresAt']=(utc()-timedelta(seconds=1)).isoformat();self.server.agents.save(runs)
        self.v.mutate(action)

    def test_lifecycle_exact_scope_idempotency_owner_review_and_portable_handoff(self):
        st,listing=self.call('/api/ai/agent-assignments');self.assertEqual(st,200);self.assertEqual(listing['assignments'][0]['status'],'due')
        run=self.start();self.assertNotIn('Private context B',json.dumps(run));self.assertTrue(any('Before ending' in i for i in run['instructions']))
        self.assertEqual(self.start()['run']['runId'],run['run']['runId'])
        self.assertEqual(self.call('/api/ai/agent-briefing',{'agentId':self.agent['id'],'requestId':'different'})[0],409)
        p=self.prepare(run);self.assertEqual(len(self.v.graph()['proposals']),0)
        st,out=self.submit(p);self.assertEqual(st,200,out);self.assertEqual(out['status'],'pending_report')
        pid=out['proposal']['id']
        self.assertEqual(self.call('/api/proposals/'+pid+'/approve',{},self.owner)[0],409)
        self.assertEqual(self.call('/api/proposals/review-batch',{'proposalIds':[pid],'action':'approve'},self.owner)[0],409)
        self.expire(run);st,saved=self.review(run);self.assertEqual(st,200,saved)
        self.assertEqual(saved['node']['facts'],p['content']['facts'])
        self.assertEqual(self.v.rows('connections')[0]['nodeIds'],sorted([self.agent['id'],self.a['id']]))
        # Exact-node read is still denied, while explicit run permission allows the approved scoped handoff.
        self.assertEqual(self.call('/api/ai/nodes/'+saved['node']['id'])[0],404)
        nxt=self.start('next-run');self.assertEqual(nxt['lastReport']['id'],saved['node']['id'])
        bundle=self.v.bundle()['bundle'];other=Vault(Path(self.temp.name)/'restored/memory.alve')
        try:
            other.restore(bundle,'synthetic agent runs passphrase')
            self.assertEqual(other.db.execute("SELECT value FROM meta WHERE key='agentRuns'").fetchone(),None)
            self.assertEqual(other.graph()['connections'],[])
            from app.agents import AgentRuns
            restored=AgentRuns(other)
            resumed=other.mutate(lambda:restored.briefing({'agentId':self.agent['id'],'requestId':'restored-run'},None))
            self.assertEqual(resumed['lastReport']['id'],saved['node']['id'])
        finally:other.lock()

    def test_report_and_generic_ticket_domains_actor_and_single_submission(self):
        run=self.start();p=self.prepare(run);body={'reviewToken':p['reviewToken'],'confirmation':CONFIRM}
        self.assertEqual(self.call('/api/ai/proposals',body)[0],422)
        self.assertEqual(self.call('/api/ai/proposals/submit-batch',body)[0],422)
        other=self.grant([self.agent['id'],self.a['id']])
        self.assertEqual(self.call('/api/ai/agent-reports/submit',body,other['token'])[0],409)
        self.assertEqual(self.call('/api/ai/agent-runs/'+run['run']['runId'],token=other['token'])[0],404)
        st,generic=self.call('/api/ai/proposals/prepare',{'content':{'title':'Plain memory','type':'memory','kind':'record'}});self.assertEqual(st,200)
        self.assertEqual(self.call('/api/ai/agent-reports/submit',{'reviewToken':generic['reviewToken'],'confirmation':CONFIRM})[0],422)
        self.assertEqual(self.call('/api/ai/proposals/prepare',{'content':p['content']})[0],422)
        second=self.prepare(run);self.assertEqual(self.submit(p)[0],200);self.assertEqual(self.submit(second)[0],409);self.assertEqual(len(self.v.graph()['proposals']),1)

    def test_permissions_scopes_revocation_and_pending_rejection(self):
        for perms in [['read','propose'],['run','propose'],['run','read']]:
            c=self.grant([self.agent['id'],self.a['id']],perms)
            self.assertEqual(self.call('/api/ai/agent-briefing',{'agentId':self.agent['id'],'requestId':str(perms)},c['token'])[0],403)
        c=self.grant([self.agent['id']]);self.assertEqual(self.call('/api/ai/agent-briefing',{'agentId':self.agent['id'],'requestId':'scope'},c['token'])[0],403)
        run=self.start();p=self.prepare(run);self.assertEqual(self.submit(p)[0],200)
        self.v.mutate(lambda:self.v.revoke(self.conn['connection']['id']))
        self.assertEqual(self.review(run)[0],403);self.assertEqual(self.review(run,False)[0],200)

    def test_expiry_restart_and_stale_introduction(self):
        run=self.start();p=self.prepare(run);self.expire(run)
        self.assertEqual(self.submit(p)[0],409)
        self.assertEqual(self.call('/api/ai/agent-runs/'+run['run']['runId'])[1]['status'],'expired')
        self.v.lock();self.owner=self.v.unlock('synthetic agent runs passphrase')['token']
        new=self.start('resumed');p=self.prepare(new)
        self.v.mutate(lambda:self.v.add_node({**self.agent,'body':'New introduction.'},self.agent['id'],[self.agent['revisionId']]))
        self.assertEqual(self.submit(p)[0],409)

    def test_disk_failures_preserve_run_preview_and_approval_retry(self):
        run=self.start();p=self.prepare(run);before=self.v.path.read_bytes()
        with patch.object(self.v,'persist',side_effect=OSError('Synthetic failure')):self.assertEqual(self.submit(p)[0],500)
        self.assertEqual(self.v.path.read_bytes(),before);self.assertEqual(self.server.agents.load()[0]['state'],'running');self.assertEqual(self.v.rows('proposals'),[])
        self.assertEqual(self.submit(p)[0],200)
        with patch.object(self.v,'persist',side_effect=OSError('Synthetic failure')):self.assertEqual(self.review(run)[0],500)
        self.assertEqual(self.server.agents.load()[0]['state'],'pending_report');self.assertEqual(self.review(run)[0],200)

    def test_old_a_context_report_is_not_laundered_into_b_only_handoff(self):
        run=self.start();self.assertEqual(self.submit(self.prepare(run))[0],200);self.assertEqual(self.review(run)[0],200)
        newfacts=[f if f['key']!='agent_context_1' else {**f,'value':{'type':'text','value':self.b['id']}} for f in self.agent['facts']]
        self.agent=self.v.mutate(lambda:self.v.add_node({**self.agent,'facts':newfacts},self.agent['id'],[self.agent['revisionId']]))
        self.conn=self.grant([self.agent['id'],self.b['id']])
        new=self.start('b-run');self.assertIsNone(new['lastReport']);self.assertNotIn('Private context A',json.dumps(new))

    def test_validation_and_abandonment(self):
        run=self.start()
        for key,value in [('outcome',[]),('nextFollowUp','2026-10-05'),('result','x'*301)]:
            data=self.report(run);data['report'][key]=value;self.assertEqual(self.call('/api/ai/agent-reports/prepare',data)[0],422 if key!='result' else 400)
        self.assertEqual(self.call('/api/agent-runs/'+run['run']['runId']+'/abandon',{},self.owner)[0],200)
        self.assertEqual(self.call('/api/ai/agent-reports/prepare',self.report(run))[0],409)

    def test_scope_removed_after_prepare_and_source_changed_after_submit(self):
        run=self.start();p=self.prepare(run)
        original=self.v.rows('connections')[0]
        narrower={**original,'nodeIds':[self.agent['id']]}
        self.v.mutate(lambda:self.v.db.execute('UPDATE connections SET payload=? WHERE id=?',(canonical(narrower),original['id'])))
        self.assertEqual(self.submit(p)[0],403)
        self.v.mutate(lambda:self.v.db.execute('UPDATE connections SET payload=? WHERE id=?',(canonical(original),original['id'])))
        self.assertEqual(self.submit(p)[0],200)
        self.v.mutate(lambda:self.v.add_node({**self.a,'body':'Changed current context.'},self.a['id'],[self.a['revisionId']]))
        self.assertEqual(self.review(run)[0],409)
        self.assertEqual(self.review(run,False)[0],200)

    def test_interrupted_start_and_reference_confirmation(self):
        with patch.object(self.v,'persist',side_effect=OSError('Synthetic start failure')):
            self.assertEqual(self.call('/api/ai/agent-briefing',{'agentId':self.agent['id'],'requestId':'retry'})[0],500)
        self.assertEqual(self.server.agents.load(),[])
        run=self.start('retry');r=self.report(run);r['report']['references']=[{'title':'Synthetic source','url':'https://example.com'}]
        st,p=self.call('/api/ai/agent-reports/prepare',r);self.assertEqual(st,200,p)
        confirmation={**CONFIRM,'sourceBasis':'reference'}
        self.assertEqual(self.call('/api/ai/agent-reports/submit',{'reviewToken':p['reviewToken'],'confirmation':confirmation})[0],200)

    def test_due_status_after_approved_handoff_and_source_edit(self):
        run=self.start();self.assertEqual(self.submit(self.prepare(run))[0],200);self.assertEqual(self.review(run)[0],200)
        st,l=self.call('/api/ai/agent-assignments');self.assertEqual(st,200);self.assertEqual(l['assignments'][0]['status'],'not_due')
        self.v.mutate(lambda:self.v.add_node({**self.a,'body':'New confirmed project facts.'},self.a['id'],[self.a['revisionId']]))
        st,l=self.call('/api/ai/agent-assignments');self.assertEqual(st,200);self.assertEqual(l['assignments'][0]['status'],'due')
