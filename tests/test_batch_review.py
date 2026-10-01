"""Atomic owner review of explicitly selected proposals."""
import json
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch
from urllib.request import Request, urlopen
from urllib.error import HTTPError
from app.vault import Vault, Problem
from app.server import AlveServer

class BatchTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.v = Vault(Path(self.temp.name) / 'memory.alve')
        self.owner = self.v.unlock('synthetic batch test passphrase', True)['token']
    def tearDown(self):
        self.v.lock()
        self.temp.cleanup()
    def propose(self, title='Responsibility', grant=None, **extra):
        return self.v.mutate(lambda: self.v.propose({'content': {'title':title,'body':'Synthetic task','type':'memory','kind':'record','tags':['ansvarsområde']},**extra}, grant))['id']
    def review(self, ids, action='approve', **extra):
        return self.v.mutate(lambda: self.v.review_batch({'proposalIds':ids,'action':action,**extra}))
    def test_five_tagged_memories_and_explicit_group_persist(self):
        ids=[self.propose(str(i)) for i in range(5)]
        out=self.review(ids, groupTitle='Responsibilities')
        self.assertEqual(len(out['reviews']),5)
        self.assertEqual(len(out['relations']),5)
        self.assertEqual(out['group']['origin'],'user')
        self.assertEqual({r['fromId'] for r in out['relations']},{r['node']['id'] for r in out['reviews']})
        for r in out['relations']:
            self.assertEqual(r['type'],'belongs_to')
            self.assertEqual(r['toId'],out['group']['id'])
        self.v.lock();self.v.unlock('synthetic batch test passphrase')
        self.assertEqual(len(self.v.graph()['nodes']),6)
        self.assertEqual(len(self.v.graph()['relations']),5)
        for n in self.v.graph()['nodes']:
            if n['id']!=out['group']['id']: self.assertEqual(n['tags'],['ansvarsområde'])
    def test_no_implicit_relations_and_batch_rejection(self):
        self.review([self.propose('a'), self.propose('b')])
        self.assertEqual(self.v.graph()['relations'],[])
        self.assertIsNone(self.review([self.propose('c'),self.propose('d')], 'reject')['group'])
        self.assertEqual(len(self.v.graph()['nodes']),2)
    def test_stale_second_proposal_rolls_back_first(self):
        node=self.v.mutate(lambda:self.v.add_node({'title':'Original'}))
        second=self.propose('Updated',action='update',nodeId=node['id'],expectedRevision=node['revisionId'])
        self.v.mutate(lambda:self.v.add_node({'title':'Changed'},node['id'],[node['revisionId']]))
        first=self.propose('First')
        before=self.v.graph();disk=self.v.path.read_bytes()
        with self.assertRaises(Problem):self.review([first,second],groupTitle='Group')
        self.assertEqual(self.v.graph(),before);self.assertEqual(self.v.path.read_bytes(),disk)
    def test_revoked_second_proposal_rolls_back_first(self):
        node=self.v.mutate(lambda:self.v.add_node({'title':'Scope'}))
        conn=self.v.mutate(lambda:self.v.grant({'name':'AI','nodeIds':[node['id']],'permissions':['propose']}))
        grant=self.v.auth(conn['token'], permission='propose')
        first=self.propose('First');second=self.propose('Second',grant)
        self.v.mutate(lambda:self.v.revoke(grant['id']))
        before=self.v.graph()
        with self.assertRaises(Problem): self.review([first,second],groupTitle='Group')
        self.assertEqual(self.v.graph(),before)
    def test_invalid_or_missing_ids_and_shapes_do_not_write(self):
        first=self.propose();before=self.v.graph()
        for data in [{'proposalIds':[first,first],'action':'approve'}, {'proposalIds':[first,'missing'],'action':'approve'}, {'proposalIds':[],'action':'approve'}, {'proposalIds':[first]*51,'action':'approve'}, {'proposalIds':[first],'action':[]}, {'proposalIds':[first],'action':{}}, {'proposalIds':[first],'action':'approve','groupTitle':None}, {'proposalIds':[first],'action':'approve','unknown':True}]:
            with self.subTest(data=data),self.assertRaises(Problem):self.v.mutate(lambda:self.v.review_batch(data))
            self.assertEqual(self.v.graph(),before)
    def test_persistence_and_relation_failure_roll_back_entire_batch(self):
        ids=[self.propose('a'),self.propose('b')];before=self.v.graph();disk=self.v.path.read_bytes()
        with patch.object(self.v,'persist',side_effect=OSError('Disk full')),self.assertRaises(OSError):self.review(ids,groupTitle='Group')
        self.assertEqual(self.v.graph(),before);self.assertEqual(self.v.path.read_bytes(),disk)
        with patch('app.vault.MAX_RELATIONS',0),self.assertRaises(Problem):self.review(ids,groupTitle='Group')
        self.assertEqual(self.v.graph(),before)
    def test_owner_route_denies_scoped_ai_token(self):
        node=self.v.mutate(lambda:self.v.add_node({'title':'Scope'}))
        conn=self.v.mutate(lambda:self.v.grant({'name':'AI','nodeIds':[node['id']],'permissions':['propose']}))
        identifier=self.propose();server=AlveServer(('127.0.0.1',0),self.v)
        thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
        def request(token):
            req=Request(f'http://127.0.0.1:{server.server_port}/api/proposals/review-batch',json.dumps({'proposalIds':[identifier],'action':'approve'}).encode(),headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
            try:
                with urlopen(req) as response:return response.status
            except HTTPError as error:return error.code
        try:
            self.assertEqual(request(conn['token']),403)
            self.assertEqual(request(self.owner),200)
        finally:server.shutdown();server.server_close();thread.join()
