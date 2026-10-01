import base64
import copy
import json
import tempfile
import threading
import unittest
from pathlib import Path
from unittest.mock import patch
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from app.server import AlveServer
from app.vault import BUNDLE, Problem, Vault, canonical, envelope


PASSWORD = "synthetic relation lifecycle passphrase"


class RelationLifecycleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.vault = Vault(Path(self.temp.name) / "source.alve")
        self.owner = self.vault.unlock(PASSWORD, True)["token"]
        self.nodes = [self.vault.mutate(lambda title=title: self.vault.add_node({"title": title}))
                      for title in ("source", "target")]
        self.request = {"fromId": self.nodes[0]["id"], "toId": self.nodes[1]["id"], "type": "related_to"}
        self.relation = self.vault.mutate(lambda: self.vault.add_relation(self.request))
        self.old = self.vault.bundle()["bundle"]

    def tearDown(self):
        self.vault.lock()
        self.temp.cleanup()

    def encoded(self, payload):
        return base64.b64encode(envelope(BUNDLE, self.vault.vault_id, self.vault.salt,
                                        self.vault.key, canonical(payload).encode())).decode()

    def test_delete_wins_both_merge_orders_replay_restore_and_persistence(self):
        deleted = self.vault.mutate(lambda: self.vault.delete_relation(self.relation["id"]))
        self.assertTrue(deleted["deleted"])
        self.assertEqual(self.vault.mutate(lambda: self.vault.delete_relation(self.relation["id"])), deleted)
        removed = self.vault.bundle()["bundle"]
        self.assertEqual(self.vault.graph()["relations"], [])
        self.assertEqual(self.vault.visible({"nodeIds": [n["id"] for n in self.nodes]})["relations"], [])
        self.vault.mutate(lambda: self.vault.merge(self.old, PASSWORD))
        self.assertEqual(self.vault.rows("relations"), [deleted])
        for index, (first, second) in enumerate(((self.old, removed), (removed, self.old))):
            peer = Vault(Path(self.temp.name) / f"peer-{index}.alve")
            try:
                peer.restore(first, PASSWORD)
                peer.mutate(lambda: peer.merge(second, PASSWORD))
                self.assertEqual(peer.graph()["relations"], [])
                self.assertEqual(peer.rows("relations"), [deleted])
                peer.lock()
                peer.unlock(PASSWORD)
                self.assertEqual(peer.rows("relations"), [deleted])
            finally:
                peer.lock()
        restored = self.vault.mutate(lambda: self.vault.restore_relation(deleted["id"]))
        self.assertNotEqual(restored["id"], deleted["id"])
        self.assertEqual(restored["restoredFrom"], deleted["id"])
        self.assertEqual(self.vault.mutate(lambda: self.vault.restore_relation(deleted["id"])), restored)
        self.vault.mutate(lambda: self.vault.merge(removed, PASSWORD))
        self.vault.mutate(lambda: self.vault.merge(self.old, PASSWORD))
        self.vault.lock()
        self.vault.unlock(PASSWORD)
        self.assertEqual(self.vault.graph()["relations"], [restored])
        self.assertEqual(len(self.vault.rows("relations")), 2)

    def test_merge_rejects_invalid_tombstones_and_same_id_content_changes_atomically(self):
        original = self.vault.export()
        for fields in ({"deleted": True}, {"deleted": False, "deletedAt": "2026-01-01T00:00:00Z"},
                       {"deleted": "true", "deletedAt": "2026-01-01T00:00:00Z"},
                       {"deleted": True, "deletedAt": "not a timestamp"},
                       {"deleted": True, "deletedAt": "2026-01-01T00:00:00Z", "type": "belongs_to"},
                       {"deleted": True, "deletedAt": "2026-01-01T00:00:00Z", "origin": "import"}):
            candidate = copy.deepcopy(original)
            candidate["relations"][0].update(fields)
            with self.assertRaises(Problem):
                self.vault.mutate(lambda: self.vault.merge(self.encoded(candidate), PASSWORD))
            self.assertEqual(self.vault.export(), original)
        # Duplicate IDs within a bundle must not overwrite each other or lose tombstones.
        for reverse in (False, True):
            candidate = copy.deepcopy(original)
            deleted = {**self.relation, "deleted": True, "deletedAt": "2026-01-01T00:00:00Z"}
            candidate["relations"] = [self.relation, deleted][:: -1 if reverse else 1]
            self.vault.mutate(lambda: self.vault.merge(self.encoded(candidate), PASSWORD))
            self.assertEqual(self.vault.rows("relations"), [deleted])
        candidate["relations"] = [deleted, {**self.relation, "type": "belongs_to"}, self.relation]
        before = self.vault.export()
        with self.assertRaises(Problem):
            self.vault.mutate(lambda: self.vault.merge(self.encoded(candidate), PASSWORD))
        self.assertEqual(self.vault.export(), before)

    def test_concurrent_deletions_converge_and_failed_save_rolls_back(self):
        with patch("app.vault.now", return_value="2026-02-01T00:00:00Z"):
            deleted = self.vault.mutate(lambda: self.vault.delete_relation(self.relation["id"]))
        earlier = copy.deepcopy(self.vault.export())
        earlier["relations"][0]["deletedAt"] = "2026-01-01T00:00:00Z"
        later = copy.deepcopy(earlier)
        later["relations"][0]["deletedAt"] = "2026-03-01T00:00:00Z"
        for payload in (earlier, later, earlier):
            self.vault.mutate(lambda: self.vault.merge(self.encoded(payload), PASSWORD))
        self.assertEqual(self.vault.rows("relations")[0]["deletedAt"], "2026-01-01T00:00:00Z")
        before = self.vault.export()
        with patch.object(self.vault, "persist", side_effect=OSError("synthetic save failure")):
            with self.assertRaises(OSError):
                self.vault.mutate(lambda: self.vault.restore_relation(deleted["id"]))
        self.assertEqual(self.vault.export(), before)

    def test_single_add_and_restore_reject_inactive_conflicted_and_self_endpoints(self):
        self.assertEqual(self.vault.mutate(lambda: self.vault.add_relation(self.request)), self.relation)
        with self.assertRaises(Problem):
            self.vault.mutate(lambda: self.vault.add_relation({**self.request, "toId": self.request["fromId"]}))
        self.vault.mutate(lambda: self.vault.delete_relation(self.relation["id"]))
        source = self.nodes[0]
        self.vault.mutate(lambda: self.vault.add_node({**source, "status": "archived"}, source["id"], [source["revisionId"]]))
        for action in (lambda: self.vault.add_relation(self.request), lambda: self.vault.restore_relation(self.relation["id"])):
            with self.assertRaises(Problem) as error:
                self.vault.mutate(action)
            self.assertEqual(error.exception.status, 409)
        # A concurrent active edit still leaves two heads, so creating a link must fail.
        peer = Vault(Path(self.temp.name) / "conflict.alve")
        try:
            peer.restore(self.old, PASSWORD)
            peer.mutate(lambda: peer.add_node({**source, "title": "concurrent"}, source["id"], [source["revisionId"]]))
            self.vault.mutate(lambda: self.vault.merge(peer.bundle()["bundle"], PASSWORD))
        finally:
            peer.lock()
        with self.assertRaises(Problem) as error:
            self.vault.mutate(lambda: self.vault.add_relation(self.request))
        self.assertEqual(error.exception.status, 409)

    def test_owner_routes_deny_connection_and_hide_deleted_link_from_ai(self):
        connection = self.vault.mutate(lambda: self.vault.grant({"name": "limited", "nodeIds": [n["id"] for n in self.nodes], "permissions": ["read"]}))
        server = AlveServer(("127.0.0.1", 0), self.vault)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        def request(method, path, token):
            req = Request(f"http://127.0.0.1:{server.server_port}{path}", b"{}" if method == "POST" else None,
                          headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"}, method=method)
            try:
                with urlopen(req) as response:
                    return response.status, json.load(response)
            except HTTPError as error:
                return error.code, json.load(error)
        path = f"/api/relations/{self.relation['id']}"
        try:
            self.assertEqual(request("DELETE", path, connection["token"])[0], 403)
            self.assertEqual(request("DELETE", path, self.owner)[0], 200)
            status, body = request("GET", f"/api/ai/nodes/{self.nodes[0]['id']}/relations", connection["token"])
            self.assertEqual((status, body["relations"]), (200, []))
            self.assertEqual(request("POST", path + "/restore", connection["token"])[0], 403)
            status, body = request("POST", path + "/restore", self.owner)
            self.assertEqual(status, 200)
            self.assertNotEqual(body["id"], self.relation["id"])
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_batch_can_relink_removed_relation_and_undo_reuses_active_duplicate(self):
        with self.assertRaises(Problem) as error:
            self.vault.mutate(lambda: self.vault.restore_relation(self.relation["id"]))
        self.assertEqual(error.exception.status, 409)
        for action in (lambda: self.vault.delete_relation("missing"), lambda: self.vault.restore_relation("missing")):
            with self.assertRaises(Problem) as error:
                self.vault.mutate(action)
            self.assertEqual(error.exception.status, 404)
        self.vault.mutate(lambda: self.vault.delete_relation(self.relation["id"]))
        request = {"nodeIds": [self.nodes[0]["id"]], "toId": self.nodes[1]["id"], "type": "related_to",
                   "expectedRevisions": {n["id"]: n["revisionId"] for n in self.nodes}}
        batch = self.vault.mutate(lambda: self.vault.add_relations_batch(request))
        self.assertEqual(batch["skipped"], 0)
        active = batch["relations"][0]
        self.assertNotEqual(active["id"], self.relation["id"])
        self.assertEqual(self.vault.mutate(lambda: self.vault.restore_relation(self.relation["id"])), active)
        self.assertEqual(self.vault.graph()["relations"], [active])
        self.assertEqual(len(self.vault.rows("relations")), 2)
