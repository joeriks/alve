"""Acceptance tests for real POC boundaries, persistence, and reconciliation."""
import base64
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import patch
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from app.server import AlveServer
from app.vault import BUNDLE, Problem, Vault, canonical, envelope

PASSWORD = "a long test passphrase only"


class VaultTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.v = Vault(Path(self.temp.name) / "memory.alve")
        self.v.unlock(PASSWORD, True)

    def tearDown(self):
        self.v.lock()
        self.temp.cleanup()

    def add(self, title="Private meeting decision"):
        return self.v.mutate(lambda: self.v.add_node({"title": title, "body": "Exact sensitive sample text."}))

    def test_snapshot_encrypted_and_reopens(self):
        node = self.add()
        raw = self.v.path.read_bytes()
        self.assertNotIn(b"Private meeting decision", raw)
        self.assertNotIn(b"SQLite format", raw)
        self.v.lock()
        with self.assertRaises(Problem):
            self.v.unlock("wrong passphrase")
        self.assertIsNone(self.v.db)
        self.v.unlock(PASSWORD)
        self.assertEqual(self.v.graph()["nodes"][0]["revisionId"], node["revisionId"])

    def test_browser_reload_requires_password_and_rotates_owner_session(self):
        old = self.v.admin
        with self.assertRaises(Problem):
            self.v.unlock("wrong password")
        self.assertEqual(self.v.admin, old)
        result = self.v.unlock(PASSWORD)
        self.assertNotEqual(result["token"], old)
        with self.assertRaises(Problem):
            self.v.auth(old, admin=True)

    def test_tampered_snapshot_rejected(self):
        raw = bytearray(self.v.path.read_bytes())
        raw[-1] ^= 1
        self.v.lock()
        self.v.path.write_bytes(raw)
        with self.assertRaises(Problem):
            self.v.unlock(PASSWORD)
        self.assertIsNone(self.v.db)

    def test_failed_disk_save_rolls_back(self):
        node = self.add()
        before = self.v.path.read_bytes()
        with patch("app.vault.os.replace", side_effect=OSError("Injected disk failure")):
            with self.assertRaises(OSError):
                self.v.mutate(lambda: self.v.add_node({"title": "Unacknowledged edit"}, node["id"], [node["revisionId"]]))
        self.assertEqual(self.v.graph()["nodes"][0]["title"], node["title"])
        self.assertEqual(self.v.path.read_bytes(), before)
        self.assertEqual(list(Path(self.temp.name).glob(".alve-encrypted-*")), [])

    def test_fact_validation_and_exact_money(self):
        n = self.v.mutate(lambda: self.v.add_node({"title": "Premium", "facts": [
            {"key": "premium", "label": "Premium", "value": {"type": "money", "amount": "3450.00", "currency": "SEK"}}]}))
        self.assertEqual(n["facts"][0]["value"]["amount"], "3450.00")
        with self.assertRaises(Problem):
            self.v.mutate(lambda: self.v.add_node({"title": "Invalid date", "facts": [
                {"key": "date", "label": "Date", "value": {"type": "date", "value": "2026-02-30"}}]}))

    def test_scope_hides_nodes_and_edges(self):
        a, b = self.add("Allowed"), self.add("Hidden")
        self.v.mutate(lambda: self.v.add_relation({"fromId": a["id"], "toId": b["id"], "type": "related_to"}))
        c = self.v.mutate(lambda: self.v.grant({"name": "Restricted", "nodeIds": [a["id"]], "permissions": ["read"]}))
        grant = self.v.auth(c["token"], permission="read")
        visible = self.v.visible(grant)
        self.assertEqual([n["id"] for n in visible["nodes"]], [a["id"]])
        self.assertEqual(visible["relations"], [])
        with self.assertRaises(Problem):
            self.v.auth(c["token"], permission="read", node_id=b["id"])
        with self.assertRaises(Problem):
            self.v.auth(c["token"], permission="search")

    def test_revocation_blocks_proposal_approval(self):
        node = self.add()
        c = self.v.mutate(lambda: self.v.grant({"name": "AI", "nodeIds": [node["id"]], "permissions": ["propose"]}))
        grant = self.v.auth(c["token"], permission="propose")
        p = self.v.mutate(lambda: self.v.propose({"content": {"title": "Proposed memory"}}, grant))
        self.v.mutate(lambda: self.v.revoke(c["connection"]["id"]))
        with self.assertRaises(Problem):
            self.v.auth(c["token"])
        with self.assertRaises(Problem):
            self.v.mutate(lambda: self.v.review(p["id"], True))
        self.assertEqual(self.v.rows("proposals")[0]["status"], "pending")

    def test_proposal_requires_review_and_current_revision(self):
        node = self.add()
        p = self.v.mutate(lambda: self.v.propose({"action": "update", "nodeId": node["id"],
            "expectedRevision": node["revisionId"], "content": {"title": "AI suggestion"}}, None))
        self.assertEqual(self.v.graph()["nodes"][0]["title"], node["title"])
        self.v.mutate(lambda: self.v.add_node({"title": "Owner edit"}, node["id"], [node["revisionId"]]))
        with self.assertRaises(Problem):
            self.v.mutate(lambda: self.v.review(p["id"], True))
        self.assertEqual(self.v.graph()["nodes"][0]["title"], "Owner edit")

    def test_backup_restores_without_ai_credentials(self):
        node = self.add()
        c = self.v.mutate(lambda: self.v.grant({"name": "AI", "nodeIds": [node["id"]], "permissions": ["read"]}))
        bundle = self.v.bundle()["bundle"]
        peer = Vault(Path(self.temp.name) / "peer.alve")
        try:
            peer.restore(bundle, PASSWORD)
            self.assertEqual(peer.vault_id, self.v.vault_id)
            self.assertEqual(peer.graph()["nodes"][0]["title"], node["title"])
            self.assertEqual(peer.rows("connections"), [])
            with self.assertRaises(Problem):
                peer.auth(c["token"])
            with self.assertRaises(Problem):
                peer.restore(bundle, PASSWORD)
        finally:
            peer.lock()

    def test_offline_conflict_idempotent_merge_and_resolution(self):
        original = self.add()
        peer = Vault(Path(self.temp.name) / "peer.alve")
        try:
            peer.restore(self.v.bundle()["bundle"], PASSWORD)
            self.v.mutate(lambda: self.v.add_node({"title": "Laptop edit"}, original["id"], [original["revisionId"]]))
            peer.mutate(lambda: peer.add_node({"title": "Phone archive", "status": "archived"}, original["id"], [original["revisionId"]]))
            peer_bundle, local_bundle = peer.bundle()["bundle"], self.v.bundle()["bundle"]
            self.v.mutate(lambda: self.v.merge(peer_bundle, PASSWORD))
            peer.mutate(lambda: peer.merge(local_bundle, PASSWORD))
            self.assertEqual(self.v.export(), peer.export())
            self.assertEqual(len(self.v.graph()["conflicts"]), 1)
            again = self.v.mutate(lambda: self.v.merge(peer_bundle, PASSWORD))
            self.assertEqual(again["addedRevisions"], 0)
            heads = [n["revisionId"] for n in self.v.heads()[original["id"]]]
            self.v.mutate(lambda: self.v.add_node({"title": "Resolved decision"}, original["id"], heads))
            peer.mutate(lambda: peer.merge(self.v.bundle()["bundle"], PASSWORD))
            self.assertEqual(peer.graph()["conflicts"], [])
            self.assertEqual(len(peer.rows("revisions")), 4)
        finally:
            peer.lock()

    def test_divergent_revision_and_missing_parent_rejected(self):
        self.add()
        data = self.v.export()
        data["revisions"][0]["title"] = "Reused ID with different content"
        raw = envelope(BUNDLE, self.v.vault_id, self.v.salt, self.v.key, canonical(data).encode())
        before = self.v.export()
        with self.assertRaises(Problem):
            self.v.mutate(lambda: self.v.merge(base64.b64encode(raw).decode(), PASSWORD))
        self.assertEqual(self.v.export(), before)
        data = self.v.export()
        data["revisions"][0]["revisionId"] = "new-revision"
        data["revisions"][0]["parents"] = ["missing-parent"]
        raw = envelope(BUNDLE, self.v.vault_id, self.v.salt, self.v.key, canonical(data).encode())
        with self.assertRaises(Problem):
            self.v.mutate(lambda: self.v.merge(base64.b64encode(raw).decode(), PASSWORD))

    def test_relation_capacity_preserves_recoverability(self):
        a, b = self.add("A"), self.add("B")
        peer = Vault(Path(self.temp.name) / "peer.alve")
        try:
            peer.restore(self.v.bundle()["bundle"], PASSWORD)
            with patch("app.vault.MAX_RELATIONS", 1):
                self.v.mutate(lambda: self.v.add_relation({"fromId": a["id"], "toId": b["id"], "type": "related_to"}))
                peer.mutate(lambda: peer.add_relation({"fromId": b["id"], "toId": a["id"], "type": "based_on"}))
                before = self.v.path.read_bytes()
                with self.assertRaises(Problem):
                    self.v.mutate(lambda: self.v.add_relation({"fromId": a["id"], "toId": b["id"], "type": "fulfills"}))
                with self.assertRaises(Problem):
                    self.v.mutate(lambda: self.v.merge(peer.bundle()["bundle"], PASSWORD))
                self.assertEqual(self.v.path.read_bytes(), before)
                self.assertEqual(len(self.v.rows("relations")), 1)
        finally:
            peer.lock()

    def test_large_conflict_resolution_round_trips(self):
        original = self.add()
        data = self.v.export()
        for i in range(101):
            r = dict(original, revisionId=f"branch-{i}", parents=[original["revisionId"]], title=f"Branch {i}")
            data["revisions"].append(r)
        raw = envelope(BUNDLE, self.v.vault_id, self.v.salt, self.v.key, canonical(data).encode())
        self.v.mutate(lambda: self.v.merge(base64.b64encode(raw).decode(), PASSWORD))
        heads = [n["revisionId"] for n in self.v.heads()[original["id"]]]
        self.v.mutate(lambda: self.v.add_node({"title": "Merged many versions"}, original["id"], heads))
        peer = Vault(Path(self.temp.name) / "peer.alve")
        try:
            peer.restore(self.v.bundle()["bundle"], PASSWORD)
            self.assertEqual(peer.graph()["conflicts"], [])
            self.assertEqual(peer.graph()["nodes"][0]["title"], "Merged many versions")
        finally:
            peer.lock()


class HTTPTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.v = Vault(Path(self.temp.name) / "memory.alve")
        self.server = AlveServer(("127.0.0.1", 0), self.v)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.base = f"http://127.0.0.1:{self.server.server_port}"
        _, result = self.request("/api/unlock", {"password": PASSWORD, "create": True})
        self.admin = result["token"]

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
        self.v.lock()
        self.temp.cleanup()

    def request(self, path, data=None, token=None, method=None, headers=None):
        h = {"X-Alve-Request": "local", **(headers or {})}
        if token:
            h["Authorization"] = "Bearer " + token
        if data is not None:
            h["Content-Type"] = "application/json"
        req = Request(self.base + path, json.dumps(data).encode() if data is not None else None,
                      headers=h, method=method)
        try:
            with urlopen(req, timeout=10) as response:
                return response.status, json.loads(response.read())
        except HTTPError as exc:
            return exc.code, json.loads(exc.read())

    def test_origin_host_and_auth_boundaries(self):
        self.assertEqual(self.request("/api/graph")[0], 401)
        self.assertEqual(self.request("/api/graph", token=self.admin, headers={"Origin": "https://evil.example"})[0], 403)
        self.assertEqual(self.request("/api/graph", token=self.admin, headers={"Origin": "null"})[0], 403)
        self.assertEqual(self.request("/api/graph", token=self.admin, headers={"Host": "evil.example"})[0], 403)
        self.assertEqual(self.request("/api/graph", token=self.admin)[0], 200)
        self.request("/api/lock", {}, self.admin)
        self.assertEqual(self.request("/api/graph", token=self.admin)[0], 423)
        _, result = self.request("/api/unlock", {"password": PASSWORD})
        self.assertNotEqual(result["token"], self.admin)
        self.assertEqual(self.request("/api/graph", token=self.admin)[0], 403)

    def test_ai_end_to_end_scope_and_approval(self):
        _, node = self.request("/api/nodes", {"title": "Allowed fact"}, self.admin)
        _, hidden = self.request("/api/nodes", {"title": "Hidden fact"}, self.admin)
        self.request("/api/relations", {"fromId": node["id"], "toId": hidden["id"], "type": "related_to"}, self.admin)
        _, c = self.request("/api/connections", {"name": "Test AI", "nodeIds": [node["id"]], "permissions": ["search", "read", "propose"]}, self.admin)
        _, result = self.request("/api/ai/search?q=fact", token=c["token"])
        self.assertEqual([n["id"] for n in result["nodes"]], [node["id"]])
        self.assertEqual(self.request("/api/ai/nodes/" + hidden["id"], token=c["token"])[0], 404)
        _, result = self.request("/api/ai/nodes/" + node["id"] + "/relations", token=c["token"])
        self.assertEqual(result["relations"], [])
        self.assertEqual(self.request("/api/nodes", {"title": "AI direct write"}, c["token"])[0], 403)
        _, proposal = self.request("/api/ai/proposals", {"content": {"title": "Candidate", "body": "Reviewed memory."}}, c["token"])
        _, before = self.request("/api/graph", token=self.admin)
        self.assertEqual(len(before["nodes"]), 2)
        self.assertEqual(self.request("/api/proposals/" + proposal["id"] + "/approve", {}, self.admin)[0], 200)
        _, after = self.request("/api/graph", token=self.admin)
        self.assertEqual(len(after["nodes"]), 3)
        self.assertEqual(self.request("/api/connections/" + c["connection"]["id"], token=self.admin, method="DELETE")[0], 200)
        self.assertEqual(self.request("/api/ai/search", token=c["token"])[0], 401)

    def test_concurrent_update_approvals(self):
        _, node = self.request("/api/nodes", {"title": "Original"}, self.admin)
        pids = []
        for title in ("One", "Two"):
            _, proposal = self.request("/api/ai/proposals", {"action": "update", "nodeId": node["id"],
                "expectedRevision": node["revisionId"], "content": {"title": title}}, self.admin)
            pids.append(proposal["id"])
        with ThreadPoolExecutor(max_workers=2) as pool:
            results = list(pool.map(lambda pid: self.request("/api/proposals/" + pid + "/approve", {}, self.admin)[0], pids))
        self.assertEqual(sorted(results), [200, 409])

    def test_stdio_mcp_reads_and_proposes_with_scoped_token(self):
        _, node = self.request("/api/nodes", {"title": "MCP memory"}, self.admin)
        _, c = self.request("/api/connections", {"name": "MCP", "nodeIds": [node["id"]], "permissions": ["search", "read", "propose"]}, self.admin)
        messages = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25"}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
            {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "search_memory", "arguments": {"query": "MCP"}}},
            {"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "propose_memory", "arguments": {"content": {"title": "MCP proposal"}}}},
        ]
        run = subprocess.run([sys.executable, "-m", "app.mcp_bridge"], input="\n".join(json.dumps(m) for m in messages) + "\n",
                             capture_output=True, text=True, timeout=20,
                             env={**os.environ, "ALVE_URL": self.base, "ALVE_TOKEN": c["token"]})
        self.assertEqual(run.returncode, 0)
        results = [json.loads(line) for line in run.stdout.splitlines()]
        self.assertEqual(len(results), 4)
        search = json.loads(results[2]["result"]["content"][0]["text"])
        self.assertEqual(search["nodes"][0]["id"], node["id"])
        self.assertEqual(len(self.v.graph()["nodes"]), 1)
        self.assertEqual(self.v.graph()["proposals"][0]["content"]["title"], "MCP proposal")


if __name__ == "__main__":
    unittest.main()
