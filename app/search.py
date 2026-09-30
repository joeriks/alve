"""Deterministic local search over an already permission-filtered graph."""
from datetime import datetime, timezone

from .vault import KINDS, TYPES, Problem, now, text


def timestamp(value, label):
    value = text(value, label, 80, True)
    try:
        result = datetime.fromisoformat(value.replace("Z", "+00:00"))
        if result.tzinfo is None:
            raise ValueError()
        return result.astimezone(timezone.utc)
    except ValueError:
        raise Problem(f"{label} must be an ISO datetime with an explicit timezone offset.") from None


def search(graph, query):
    if set(query) - {"q", "tag", "type", "kind", "updatedSince", "updatedBefore", "includeArchived", "sort", "limit", "offset"}:
        raise Problem("Unknown search filter. Use q, tag, type, kind, updatedSince, updatedBefore, includeArchived, sort, limit, or offset.")
    def single(key, default=None):
        values = query.get(key, [default])
        if len(values) != 1:
            raise Problem(f"Specify {key} once.")
        return values[0]

    q = text(single("q", ""), "search query", 1000).casefold()
    tags = query.get("tag", [])
    if len(tags) > 20:
        raise Problem("Use at most 20 tag filters.")
    tags = {text(t, "tag filter", 60, True).casefold() for t in tags}
    typ, kind = single("type"), single("kind")
    if typ is not None and typ not in TYPES or kind is not None and kind not in KINDS:
        raise Problem("Unsupported type or kind filter.")
    since, before = single("updatedSince"), single("updatedBefore")
    since = timestamp(since, "updatedSince") if since is not None else None
    before = timestamp(before, "updatedBefore") if before is not None else None
    if since and before and since >= before:
        raise Problem("updatedSince must be earlier than updatedBefore.")
    archived = single("includeArchived", "false")
    if archived not in {"true", "false"}:
        raise Problem("includeArchived must be true or false.")
    order = single("sort", "relevance")
    if order not in {"relevance", "updated"}:
        raise Problem("Sort must be relevance or updated.")
    try:
        limit, offset = int(single("limit", "20")), int(single("offset", "0"))
    except (ValueError, TypeError):
        raise Problem("Limit and offset must be integers.") from None
    if not 1 <= limit <= 100 or not 0 <= offset <= 5000:
        raise Problem("Limit must be 1–100 and offset 0–5000.")
    conflicts = {c["nodeId"]: c for c in graph["conflicts"]}
    matches = []
    for node in graph["nodes"]:
        versions = conflicts.get(node["id"], {}).get("versions") or [node]
        scores = []
        for version in versions:
            modified = timestamp(version["updatedAt"], "stored updatedAt")
            if (typ and version["type"] != typ or kind and version["kind"] != kind
                    or archived == "false" and version["status"] == "archived"
                    or since and modified < since or before and modified >= before):
                continue
            vtags = [t.casefold() for t in version["tags"]]
            if not tags.issubset(set(vtags)):
                continue
            title, body = version["title"].casefold(), version["body"].casefold()
            facts = " ".join(f["label"] + " " + " ".join(str(v) for v in (f["value"] or {}).values()) for f in version["facts"]).casefold()
            refs = " ".join(r["title"] + " " + r.get("url", "") for r in version["references"]).casefold()
            if q and not any(q in value for value in [title, body, facts, refs, *vtags]):
                continue
            score = (100 if q and q == title else 80 if q and q in title else
                     60 if q and any(q in t for t in vtags) else 40 if q and q in body else 20 if q else 0)
            scores.append((score, modified.timestamp()))
        if scores:
            score = max(s[0] for s in scores) if order == "relevance" else 0
            updated = max(s[1] for s in scores)
            matches.append((score, updated, node))
    matches.sort(key=lambda entry: (-entry[0], -entry[1], entry[2]["id"]))
    nodes = [entry[2] for entry in matches[offset:offset + limit]]
    selected = {n["id"] for n in nodes}
    return {"vaultId": graph["vaultId"], "nodes": nodes,
            "conflicts": [c for c in graph["conflicts"] if c["nodeId"] in selected],
            "nextOffset": offset + limit if offset + limit < len(matches) else None,
            "asOf": now(), "sort": order}
