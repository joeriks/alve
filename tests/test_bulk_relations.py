import json
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from app.server import AlveServer
from app.vault import Problem, Vault


class BulkRelationsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.vault = Vault(Path(self.temp.name) / "memory.alve")
        self.owner = self.vault.unlock("synthetic bulk relation passphrase", True)["token"]

    def tearDown(self):
        self.vault.lock()
        self.temp.cleanup()

    def node(self, title, **extra):
        return self.vault.mutate(lambda: self.vault.add_node({"title": title, **extra}))

    @staticmethod
    def request(sources, target):
        endpoints = [*sources, target]
        return {
            "nodeIds": [node["id"] for node in sources],
            "toId": target["id"],
            "type": "related_to",
            "expectedRevisions": {node["id"]: node["revisionId"] for node in endpoints},
        }

    def test_atomic_stale_batch_and_idempotency(self):
        first, second, target = (self.node(title) for title in ("first", "second", "target"))
        request = self.request([first, second], target)
        saved = self.vault.mutate(lambda: self.vault.add_relations_batch(request))
        self.assertEqual(len(saved["relations"]), 2)
        self.assertEqual(saved["skipped"], 0)
        repeated = self.vault.mutate(lambda: self.vault.add_relations_batch(request))
        self.assertEqual(repeated, {"relations": [], "skipped": 2})

        stale = self.node("stale")
        stale_request = self.request([first, stale], target)
        self.vault.mutate(lambda: self.vault.add_node({"title": "changed"}, stale["id"], [stale["revisionId"]]))
        before = self.vault.graph()
        with self.assertRaisesRegex(Problem, "changed") as error:
            self.vault.mutate(lambda: self.vault.add_relations_batch(stale_request))
        self.assertEqual(error.exception.status, 409)
        self.assertEqual(self.vault.graph(), before)

    def test_capacity_and_inactive_nodes_reject_without_partial_writes(self):
        first, second, target = (self.node(title) for title in ("first", "second", "target"))
        request = self.request([first, second], target)
        before = self.vault.graph()
        with patch("app.vault.MAX_RELATIONS", 1), self.assertRaises(Problem) as error:
            self.vault.mutate(lambda: self.vault.add_relations_batch(request))
        self.assertEqual(error.exception.status, 413)
        self.assertEqual(self.vault.graph(), before)

        archived = self.node("archived", status="archived")
        inactive = self.request([first], archived)
        with self.assertRaises(Problem) as error:
            self.vault.mutate(lambda: self.vault.add_relations_batch(inactive))
        self.assertEqual(error.exception.status, 409)

    def test_owner_route_accepts_batch_and_denies_connection_tokens(self):
        source, target = self.node("source"), self.node("target")
        request = self.request([source], target)
        connection = self.vault.mutate(lambda: self.vault.grant({
            "name": "limited", "nodeIds": [source["id"], target["id"]], "permissions": ["read"]}))
        server = AlveServer(("127.0.0.1", 0), self.vault)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()

        def post(token):
            payload = json.dumps(request).encode()
            http_request = Request(
                f"http://127.0.0.1:{server.server_port}/api/relations/batch", payload,
                headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"})
            try:
                with urlopen(http_request) as response:
                    return response.status
            except HTTPError as error:
                return error.code

        try:
            self.assertEqual(post(connection["token"]), 403)
            self.assertEqual(post(self.owner), 200)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

