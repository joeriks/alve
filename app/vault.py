"""Encrypted in-memory SQLite POC. This is not SQLCipher or a production vault."""
from __future__ import annotations

import base64
import hashlib
import json
import os
import secrets
import sqlite3
import tempfile
import threading
from collections import deque
from datetime import date, datetime, timezone
from decimal import Decimal, InvalidOperation
from pathlib import Path
from uuid import uuid4
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from cryptography.exceptions import InvalidTag
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.scrypt import Scrypt

MAX_ENVELOPE = 16 * 1024 * 1024
MAX_REVISIONS = 5000
MAX_RELATIONS = 5000
SNAPSHOT = b"ALVEPOC1"
BUNDLE = b"ALVEBND1"
KINDS = {"decision", "preference", "insight", "commitment", "record"}
TYPES = {"memory", "project", "person", "event", "document"}
RELATIONS = {"belongs_to", "based_on", "related_to", "supersedes", "contradicts", "fulfills"}


class Problem(Exception):
    def __init__(self, message: str, status: int = 400):
        self.status = status
        super().__init__(message)


def canonical(value) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)


def now() -> str:
    return datetime.now(timezone.utc).isoformat()


def text(value, label, maximum=4000, required=False):
    if not isinstance(value, str) or len(value) > maximum or (required and not value.strip()):
        raise Problem(f"Invalid {label}.")
    return value.strip()


def derive(password: str, salt: bytes) -> bytes:
    text(password, "passphrase", 1024, True)
    return Scrypt(salt=salt, length=32, n=2**15, r=8, p=1).derive(password.encode("utf-8"))


def envelope(magic: bytes, vault_id: str, salt: bytes, key: bytes, data: bytes) -> bytes:
    nonce = os.urandom(12)
    header = magic + bytes.fromhex(vault_id) + salt + nonce
    result = header + AESGCM(key).encrypt(nonce, data, header)
    if len(result) > MAX_ENVELOPE:
        raise Problem("POC vault limit is 16 MiB.", 413)
    return result


def decrypt(raw: bytes, magic: bytes, password: str):
    if not 68 <= len(raw) <= MAX_ENVELOPE or raw[:8] != magic:
        raise Problem("Unsupported or oversized encrypted file.")
    vault_id, salt, nonce = raw[8:24].hex(), raw[24:40], raw[40:52]
    key = derive(password, salt)
    try:
        data = AESGCM(key).decrypt(nonce, raw[52:], raw[:52])
    except InvalidTag:
        raise Problem("Incorrect passphrase or damaged encrypted file.", 401) from None
    return vault_id, salt, key, data


def atomic_write(path: Path, raw: bytes):
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, filename = tempfile.mkstemp(prefix=".alve-encrypted-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(raw)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(filename, path)
    finally:
        if os.path.exists(filename):
            os.unlink(filename)


def connection():
    db = sqlite3.connect(":memory:", check_same_thread=False)
    db.execute("PRAGMA temp_store=MEMORY")
    db.execute("PRAGMA journal_mode=MEMORY")
    return db


def validate_facts(facts):
    if not isinstance(facts, list) or len(facts) > 30:
        raise Problem("Use at most 30 facts per node.")
    result, keys = [], set()
    for fact in facts:
        if not isinstance(fact, dict):
            raise Problem("Invalid fact.")
        key = text(fact.get("key"), "fact key", 80, True)
        if key in keys:
            raise Problem("Fact keys must be unique within a node.")
        keys.add(key)
        label = text(fact.get("label"), "fact label", 120, True)
        value = fact.get("value")
        precision = fact.get("precision", "exact")
        if not isinstance(precision, str) or precision not in {"exact", "approximate", "estimated"}:
            raise Problem("Invalid fact precision.")
        clean = None
        if value is not None:
            if not isinstance(value, dict):
                raise Problem("Invalid fact value.")
            kind = value.get("type")
            if not isinstance(kind, str):
                raise Problem("Invalid fact type.")
            if kind == "text":
                clean = {"type": kind, "value": text(value.get("value"), "text fact", 1000)}
            elif kind == "boolean" and isinstance(value.get("value"), bool):
                clean = {"type": kind, "value": value["value"]}
            elif kind == "date":
                val = text(value.get("value"), "date", 10, True)
                try:
                    if date.fromisoformat(val).isoformat() != val:
                        raise ValueError()
                except ValueError:
                    raise Problem("Dates must be valid YYYY-MM-DD values.") from None
                clean = {"type": kind, "value": val}
            elif kind == "datetime":
                val = text(value.get("value"), "datetime", 80, True)
                tz = text(value.get("timeZone"), "time zone", 80, True)
                try:
                    if datetime.fromisoformat(val.replace("Z", "+00:00")).tzinfo is None:
                        raise ValueError()
                    ZoneInfo(tz)
                except (ValueError, ZoneInfoNotFoundError):
                    raise Problem("Datetime needs an explicit offset and a valid time zone.") from None
                clean = {"type": kind, "value": val, "timeZone": tz}
            elif kind in {"money", "quantity"}:
                amount = text(value.get("amount"), "decimal amount", 80, True)
                try:
                    number = Decimal(amount)
                    if not number.is_finite() or "e" in amount.lower():
                        raise InvalidOperation()
                except InvalidOperation:
                    raise Problem("Amounts must be finite decimal strings without exponent notation.") from None
                clean = {"type": kind, "amount": amount}
                if kind == "money":
                    currency = text(value.get("currency"), "currency", 3, True)
                    if len(currency) != 3 or not currency.isascii() or not currency.isalpha() or currency != currency.upper():
                        raise Problem("Currency must be a three-letter uppercase code.")
                    clean["currency"] = currency
                else:
                    clean["unit"] = text(value.get("unit"), "unit", 40, True)
            else:
                raise Problem("Unsupported fact type.")
        result.append({"key": key, "label": label, "value": clean, "precision": precision})
    return result


def node_content(data):
    if not isinstance(data, dict):
        raise Problem("Node content must be an object.")
    kind, typ = data.get("kind", "record"), data.get("type", "memory")
    if not isinstance(kind, str) or not isinstance(typ, str) or kind not in KINDS or typ not in TYPES:
        raise Problem("Unsupported node type or memory kind.")
    tags = data.get("tags", [])
    if not isinstance(tags, list) or len(tags) > 20:
        raise Problem("Invalid tags.")
    refs = data.get("references", [])
    if not isinstance(refs, list) or len(refs) > 20:
        raise Problem("Invalid references.")
    clean_refs = []
    for ref in refs:
        if not isinstance(ref, dict):
            raise Problem("Invalid reference.")
        item = {"title": text(ref.get("title"), "reference title", 200, True)}
        if ref.get("url"):
            url = text(ref["url"], "reference URL", 2000, True)
            if not url.startswith(("https://", "http://")):
                raise Problem("Reference URLs must use HTTP or HTTPS.")
            item["url"] = url
        clean_refs.append(item)
    status = data.get("status", "active")
    if not isinstance(status, str) or status not in {"active", "archived"}:
        raise Problem("Invalid node status.")
    return {"title": text(data.get("title"), "summary heading", 200, True),
            "body": text(data.get("body", ""), "memory body", 6000),
            "type": typ, "kind": kind, "tags": [text(t, "tag", 60, True) for t in tags],
            "facts": validate_facts(data.get("facts", [])), "references": clean_refs, "status": status}


class Vault:
    def __init__(self, path):
        self.path = Path(path)
        self.mutex = threading.RLock()
        self.db = None
        self.key = None
        self.salt = None
        self.vault_id = None
        self.admin = None

    def status(self):
        return {"exists": self.path.exists(), "unlocked": self.db is not None}

    def require(self):
        if self.db is None:
            raise Problem("Unlock the vault first.", 423)

    def unlock(self, password, create=False):
        if self.db is not None:
            # A browser reload loses its in-memory owner token. Require the passphrase
            # again rather than handing out access merely because the process is open.
            decrypt(self.path.read_bytes(), SNAPSHOT, password)
            self.admin = secrets.token_urlsafe(32)
            return {"token": self.admin, "vaultId": self.vault_id}
        if self.path.exists():
            if self.path.stat().st_size > MAX_ENVELOPE:
                raise Problem("POC vault limit is 16 MiB.", 413)
            raw = self.path.read_bytes()
            vault_id, salt, key, serialized = decrypt(raw, SNAPSHOT, password)
            db = connection()
            try:
                db.deserialize(serialized)
                db.execute("PRAGMA temp_store=MEMORY")
                if db.execute("PRAGMA integrity_check").fetchone()[0] != "ok":
                    raise Problem("Vault database integrity check failed.")
                if db.execute("SELECT value FROM meta WHERE key='vaultId'").fetchone()[0] != vault_id:
                    raise Problem("Vault identity mismatch.")
                if db.execute("SELECT value FROM meta WHERE key='format'").fetchone()[0] != "alve-poc-1":
                    raise Problem("Unsupported vault schema.")
            except Exception:
                db.close()
                raise
            self.db, self.vault_id, self.salt, self.key = db, vault_id, salt, key
        else:
            if not create:
                raise Problem("Create a vault first.", 404)
            if len(text(password, "passphrase", 1024, True)) < 12:
                raise Problem("Use a passphrase of at least 12 characters.")
            self.vault_id, self.salt = uuid4().hex, os.urandom(16)
            self.key = derive(password, self.salt)
            self.db = connection()
            self.db.executescript("""
                CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                CREATE TABLE revisions (id TEXT PRIMARY KEY, node_id TEXT NOT NULL, payload TEXT NOT NULL);
                CREATE INDEX revisions_node ON revisions(node_id);
                CREATE TABLE relations (id TEXT PRIMARY KEY, payload TEXT NOT NULL);
                CREATE TABLE proposals (id TEXT PRIMARY KEY, payload TEXT NOT NULL);
                CREATE TABLE connections (id TEXT PRIMARY KEY, payload TEXT NOT NULL);
            """)
            self.db.executemany("INSERT INTO meta VALUES (?,?)", [("vaultId", self.vault_id), ("format", "alve-poc-1")])
            self.db.commit()
            try:
                self.persist()
            except Exception:
                self.lock()
                raise
        self.admin = secrets.token_urlsafe(32)
        return {"token": self.admin, "vaultId": self.vault_id}

    def lock(self):
        if self.db is not None:
            self.db.close()
        self.db = self.key = self.salt = self.vault_id = self.admin = None

    def persist(self):
        self.require()
        atomic_write(self.path, envelope(SNAPSHOT, self.vault_id, self.salt, self.key, self.db.serialize()))

    def mutate(self, action):
        self.require()
        before = self.db.serialize()
        try:
            result = action()
            self.db.commit()
            self.persist()
            return result
        except Exception:
            self.db.rollback()
            self.db.deserialize(before)
            self.db.execute("PRAGMA temp_store=MEMORY")
            raise

    def rows(self, table):
        self.require()
        if table not in {"revisions", "relations", "proposals", "connections"}:
            raise Problem("Invalid table.")
        return [json.loads(row[0]) for row in self.db.execute(f"SELECT payload FROM {table} ORDER BY id")]

    def heads(self):
        rows = self.rows("revisions")
        parents = {p for row in rows for p in row["parents"]}
        result = {}
        for row in rows:
            if row["revisionId"] not in parents:
                result.setdefault(row["id"], []).append(row)
        return result

    def graph(self):
        heads = self.heads()
        return {"vaultId": self.vault_id, "nodes": [v[0] for v in heads.values()],
                "relations": self.rows("relations"),
                "conflicts": [{"nodeId": k, "revisionIds": [x["revisionId"] for x in v], "versions": v}
                              for k, v in heads.items() if len(v) > 1],
                "proposals": self.rows("proposals"),
                "connections": [{k: v for k, v in c.items() if k != "tokenHash"} for c in self.rows("connections")]}

    def auth(self, token, admin=False, permission=None, node_id=None):
        self.require()
        if not isinstance(token, str) or not token:
            raise Problem("Authentication required.", 401)
        if self.admin and secrets.compare_digest(token, self.admin):
            return None
        if admin:
            raise Problem("Owner authorization required.", 403)
        hashed = hashlib.sha256(token.encode()).hexdigest()
        for c in self.rows("connections"):
            if secrets.compare_digest(hashed, c["tokenHash"]) and not c["revoked"]:
                if permission and permission not in c["permissions"]:
                    raise Problem("This connection does not have that permission.", 403)
                if node_id is not None and node_id not in c["nodeIds"]:
                    raise Problem("Node is unavailable to this connection.", 404)
                return c
        raise Problem("Invalid or revoked connection token.", 401)

    def visible(self, grant):
        graph = self.graph()
        if grant is None:
            return graph
        allowed = set(grant["nodeIds"])
        return {"vaultId": self.vault_id,
                "nodes": [n for n in graph["nodes"] if n["id"] in allowed],
                "relations": [r for r in graph["relations"] if r["fromId"] in allowed and r["toId"] in allowed],
                "conflicts": [c for c in graph["conflicts"] if c["nodeId"] in allowed]}

    def add_node(self, data, node_id=None, parents=None, origin="user"):
        content = node_content(data)
        heads = self.heads()
        if node_id is None:
            node_id, parents, created = uuid4().hex, [], now()
        else:
            current = heads.get(node_id)
            if not current:
                raise Problem("Node not found.", 404)
            expected = {r["revisionId"] for r in current}
            if set(parents or []) != expected:
                raise Problem("The node changed or has a conflict. Review all current versions.", 409)
            created = current[0]["createdAt"]
        node = {**content, "id": node_id, "vaultId": self.vault_id, "revisionId": uuid4().hex,
                "parents": parents, "createdAt": created, "updatedAt": now(), "origin": origin,
                "verification": "user_confirmed"}
        self.db.execute("INSERT INTO revisions VALUES (?,?,?)", (node["revisionId"], node_id, canonical(node)))
        if self.db.execute("SELECT count(*) FROM revisions").fetchone()[0] > MAX_REVISIONS:
            raise Problem("POC revision limit reached.", 413)
        return node

    def add_relation(self, data):
        if self.db.execute("SELECT count(*) FROM relations").fetchone()[0] >= MAX_RELATIONS:
            raise Problem("POC relation limit reached.", 413)
        heads = self.heads()
        if data.get("fromId") not in heads or data.get("toId") not in heads:
            raise Problem("Relation endpoints must exist.")
        if not isinstance(data.get("type"), str) or data.get("type") not in RELATIONS:
            raise Problem("Unsupported relation type.")
        relation = {"id": uuid4().hex, "fromId": data["fromId"], "toId": data["toId"],
                    "type": data["type"], "vaultId": self.vault_id, "origin": "user",
                    "verification": "user_confirmed", "createdAt": now()}
        self.db.execute("INSERT INTO relations VALUES (?,?)", (relation["id"], canonical(relation)))
        return relation

    def grant(self, data):
        name = text(data.get("name"), "connection name", 100, True)
        alias = text(data.get("vaultAlias", "memory"), "vault alias", 100, True)
        ids, permissions = data.get("nodeIds"), data.get("permissions")
        if not isinstance(ids, list) or not ids or len(ids) > 500 or any(not isinstance(i, str) or i not in self.heads() for i in ids):
            raise Problem("Select existing nodes for this connection.")
        if not isinstance(permissions, list) or not permissions or any(not isinstance(p, str) or p not in {"search", "read", "propose"} for p in permissions):
            raise Problem("Only search, read, and propose permissions are supported.")
        token = secrets.token_urlsafe(32)
        conn = {"id": uuid4().hex, "name": name, "vaultAlias": alias, "nodeIds": sorted(set(ids)),
                "permissions": sorted(set(permissions)), "revoked": False, "createdAt": now(),
                "tokenHash": hashlib.sha256(token.encode()).hexdigest()}
        self.db.execute("INSERT INTO connections VALUES (?,?)", (conn["id"], canonical(conn)))
        return {"token": token, "connection": {k: v for k, v in conn.items() if k != "tokenHash"}}

    def revoke(self, identifier):
        row = self.db.execute("SELECT payload FROM connections WHERE id=?", (identifier,)).fetchone()
        if not row:
            raise Problem("Connection not found.", 404)
        c = json.loads(row[0])
        c["revoked"] = True
        self.db.execute("UPDATE connections SET payload=? WHERE id=?", (canonical(c), identifier))
        return {"revoked": True}

    def propose(self, data, grant):
        if not isinstance(data.get("action", "create"), str) or data.get("action", "create") not in {"create", "update"}:
            raise Problem("Only memory create/update proposals are supported.")
        action = data.get("action", "create")
        content = node_content(data.get("content", {}))
        node_id = data.get("nodeId")
        if action == "update":
            if grant is not None and node_id not in grant["nodeIds"]:
                raise Problem("Node is unavailable to this connection.", 404)
            current = self.heads().get(node_id, [])
            if len(current) != 1 or current[0]["revisionId"] != data.get("expectedRevision"):
                raise Problem("Read the current, non-conflicting revision before proposing an update.", 409)
        proposal = {"id": uuid4().hex, "action": action, "status": "pending", "content": content,
                    "nodeId": node_id if action == "update" else None,
                    "expectedRevision": data.get("expectedRevision") if action == "update" else None,
                    "connectionId": grant["id"] if grant is not None else None, "createdAt": now()}
        self.db.execute("INSERT INTO proposals VALUES (?,?)", (proposal["id"], canonical(proposal)))
        return proposal

    def review(self, identifier, approve):
        row = self.db.execute("SELECT payload FROM proposals WHERE id=?", (identifier,)).fetchone()
        if not row:
            raise Problem("Proposal not found.", 404)
        proposal = json.loads(row[0])
        if proposal["status"] != "pending":
            raise Problem("This proposal has already been reviewed.", 409)
        result = None
        if approve:
            if proposal["connectionId"]:
                conn = self.db.execute("SELECT payload FROM connections WHERE id=?", (proposal["connectionId"],)).fetchone()
                grant = json.loads(conn[0]) if conn else None
                if not grant or grant["revoked"] or "propose" not in grant["permissions"]:
                    raise Problem("The proposal's connection is no longer authorized.", 403)
                if proposal["action"] == "update" and proposal["nodeId"] not in grant["nodeIds"]:
                    raise Problem("Proposal target is no longer in scope.", 403)
            result = self.add_node(proposal["content"], proposal["nodeId"],
                                   [proposal["expectedRevision"]] if proposal["nodeId"] else None, "ai_proposal")
        proposal["status"] = "approved" if approve else "rejected"
        self.db.execute("UPDATE proposals SET payload=? WHERE id=?", (canonical(proposal), identifier))
        return {"proposal": proposal, "node": result}

    def export(self):
        return {"format": "alve-poc-1", "vaultId": self.vault_id,
                "revisions": self.rows("revisions"), "relations": self.rows("relations")}

    def bundle(self):
        raw = envelope(BUNDLE, self.vault_id, self.salt, self.key, canonical(self.export()).encode())
        return {"bundle": base64.b64encode(raw).decode("ascii")}

    def merge(self, bundle, password):
        if not isinstance(bundle, str) or len(bundle) > MAX_ENVELOPE * 4 // 3 + 8:
            raise Problem("Invalid bundle size.", 413)
        try:
            raw = base64.b64decode(bundle, validate=True)
        except ValueError:
            raise Problem("Invalid bundle encoding.") from None
        vault_id, _, _, plain = decrypt(raw, BUNDLE, password)
        if vault_id != self.vault_id:
            raise Problem("This bundle belongs to a different vault.", 409)
        try:
            data = json.loads(plain)
        except (ValueError, UnicodeDecodeError):
            raise Problem("Invalid bundle contents.") from None
        if not isinstance(data, dict) or data.get("format") != "alve-poc-1" or data.get("vaultId") != vault_id:
            raise Problem("Unsupported bundle schema.")
        revisions, relations = data.get("revisions"), data.get("relations")
        if not isinstance(revisions, list) or not isinstance(relations, list) or len(revisions) > MAX_REVISIONS or len(relations) > MAX_RELATIONS:
            raise Problem("Invalid bundle records.")
        known = {r["revisionId"]: r for r in self.rows("revisions")}
        incoming = {}
        for r in revisions:
            if not isinstance(r, dict) or r.get("vaultId") != vault_id:
                raise Problem("Invalid revision vault.")
            rid = text(r.get("revisionId"), "revision ID", 64, True)
            text(r.get("id"), "node ID", 64, True)
            node_content(r)
            parents = r.get("parents")
            if not isinstance(parents, list) or len(parents) > MAX_REVISIONS or any(not isinstance(p, str) for p in parents) or len(set(parents)) != len(parents):
                raise Problem("Invalid revision parents.")
            if r.get("verification") != "user_confirmed" or r.get("origin") not in {"user", "import", "ai_proposal"}:
                raise Problem("Invalid revision provenance.")
            for field in ("createdAt", "updatedAt"):
                val = text(r.get(field), field, 80, True)
                try:
                    if datetime.fromisoformat(val.replace("Z", "+00:00")).tzinfo is None:
                        raise ValueError()
                except ValueError:
                    raise Problem("Invalid revision timestamp.") from None
            existing = known.get(rid) or incoming.get(rid)
            if existing and canonical(existing) != canonical(r):
                raise Problem("A revision ID was reused with different content.", 409)
            incoming[rid] = r
        combined = {**known, **incoming}
        if len(combined) > MAX_REVISIONS:
            raise Problem("POC revision limit reached.", 413)
        roots = {}
        for r in combined.values():
            if not r["parents"]:
                if r["id"] in roots and roots[r["id"]] != r["revisionId"]:
                    raise Problem("Multiple independent roots use the same node ID.", 409)
                roots[r["id"]] = r["revisionId"]
            for p in r["parents"]:
                if p not in combined or combined[p]["id"] != r["id"]:
                    raise Problem("Bundle is missing a parent revision or crosses node boundaries.", 409)
        indegree = {rid: len(r["parents"]) for rid, r in combined.items()}
        children = {}
        for rid, r in combined.items():
            for p in r["parents"]:
                children.setdefault(p, []).append(rid)
        queue = deque(rid for rid, count in indegree.items() if count == 0)
        visited = 0
        while queue:
            visited += 1
            for child in children.get(queue.popleft(), []):
                indegree[child] -= 1
                if indegree[child] == 0:
                    queue.append(child)
        if visited != len(combined):
            raise Problem("Revision history contains a cycle.", 409)
        node_ids = {r["id"] for r in combined.values()}
        existing_edges = {r["id"]: r for r in self.rows("relations")}
        edge_additions = {}
        for r in relations:
            if not isinstance(r, dict) or r.get("vaultId") != vault_id or r.get("type") not in RELATIONS:
                raise Problem("Invalid relation.")
            eid = text(r.get("id"), "relation ID", 64, True)
            if r.get("fromId") not in node_ids or r.get("toId") not in node_ids:
                raise Problem("Relation endpoint is missing.")
            if r.get("verification") != "user_confirmed" or r.get("origin") not in {"user", "import", "ai_proposal"}:
                raise Problem("Invalid relation provenance.")
            existing = existing_edges.get(eid) or edge_additions.get(eid)
            if existing and canonical(existing) != canonical(r):
                raise Problem("A relation ID was reused with different content.", 409)
            edge_additions[eid] = r
        added = 0
        for rid, r in incoming.items():
            if rid not in known:
                self.db.execute("INSERT INTO revisions VALUES (?,?,?)", (rid, r["id"], canonical(r)))
                added += 1
        edges_added = 0
        if len(set(existing_edges) | set(edge_additions)) > MAX_RELATIONS:
            raise Problem("POC relation limit reached.", 413)
        for eid, r in edge_additions.items():
            if eid not in existing_edges:
                self.db.execute("INSERT INTO relations VALUES (?,?)", (eid, canonical(r)))
                edges_added += 1
        return {"addedRevisions": added, "addedRelations": edges_added, "conflicts": len(self.graph()["conflicts"])}

    def restore(self, bundle, password):
        if self.db is not None or self.path.exists():
            raise Problem("Restore requires a new installation with no existing vault.", 409)
        if not isinstance(bundle, str) or len(bundle) > MAX_ENVELOPE * 4 // 3 + 8:
            raise Problem("Invalid bundle size.", 413)
        try:
            raw = base64.b64decode(bundle, validate=True)
        except ValueError:
            raise Problem("Invalid bundle encoding.") from None
        vault_id, salt, key, _ = decrypt(raw, BUNDLE, password)
        # Build in memory first; a malformed import must not create or overwrite a vault.
        self.db = connection()
        self.vault_id, self.salt, self.key = vault_id, salt, key
        self.db.executescript("""
            CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE revisions (id TEXT PRIMARY KEY, node_id TEXT NOT NULL, payload TEXT NOT NULL);
            CREATE INDEX revisions_node ON revisions(node_id);
            CREATE TABLE relations (id TEXT PRIMARY KEY, payload TEXT NOT NULL);
            CREATE TABLE proposals (id TEXT PRIMARY KEY, payload TEXT NOT NULL);
            CREATE TABLE connections (id TEXT PRIMARY KEY, payload TEXT NOT NULL);
        """)
        self.db.executemany("INSERT INTO meta VALUES (?,?)", [("vaultId", vault_id), ("format", "alve-poc-1")])
        try:
            self.merge(bundle, password)
            self.db.commit()
            self.persist()
        except Exception:
            self.lock()
            raise
        self.admin = secrets.token_urlsafe(32)
        return {"token": self.admin, "vaultId": vault_id}
