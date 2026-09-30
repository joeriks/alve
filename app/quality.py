"""Ephemeral, connection-bound AI review handshake; never a truth verifier."""
import secrets
import time

from .vault import Problem, node_content, text


class QualityGate:
    def __init__(self):
        self.tickets = {}

    def clear(self):
        self.tickets.clear()

    def prepare(self, data, connection_id):
        content = data.get("content")
        if not isinstance(content, dict) or "type" not in content or "kind" not in content:
            raise Problem("Categorize content explicitly: type (memory, project, person, event, document) and kind (decision, preference, insight, commitment, record).", 422)
        clean = node_content(content)
        if len(clean["title"]) > 120 or len(clean["body"]) > 2000 or len(clean["body"].split()) > 300:
            raise Problem("Condense this into one memory: title at most 120 characters; body at most 300 words and 2,000 characters. Split unrelated topics and use references for longer material.", 422)
        action = data.get("action", "create")
        if not isinstance(action, str) or action not in {"create", "update"}:
            raise Problem("Use create or update.")
        payload = {"action": action, "content": clean}
        if action == "update":
            payload["nodeId"] = text(data.get("nodeId"), "node ID", 100, True)
            payload["expectedRevision"] = text(data.get("expectedRevision"), "expected revision", 100, True)
        current = time.monotonic()
        self.tickets = {k: v for k, v in self.tickets.items() if v["expires"] > current}
        if len(self.tickets) >= 128:
            raise Problem("Too many active reviews. Wait for a review to expire.", 429)
        token = secrets.token_urlsafe(32)
        self.tickets[token] = {"payload": payload, "connectionId": connection_id, "expires": current + 600}
        return {"status": "confirmation_required", "reviewToken": token, "content": clean,
                "action": action, "nodeId": payload.get("nodeId"), "expectedRevision": payload.get("expectedRevision"),
                "expiresInSeconds": 600,
                "checks": {"lengthWithinLimit": True, "explicitCategory": True, "typedFactsValid": True,
                           "factualTruthVerified": False},
                "instructions": [
                    'For several related memories, collect all prepared previews and ask once for explicit confirmation of the complete set, including tags. Each submitted proposal must match its preview. Never include later or changed items in that confirmation.',
                    "This is how the information will be stored. Show this to the user and request confirmation.",
                    "This is how the information will be stored. Show the exact preview, including categories, exact tags (or no tags), facts, references and any qualifications, to the user and ask: Is this concise, correctly represented, and categorized appropriately?",
                    "Request explicit user confirmation. Do not submit until the user agrees. If the client cannot ask the user, stop here. User changes require a new preparation and confirmation.",
                    "For update, this preview replaces the complete node content. Read the existing node first and preserve facts, references, and other fields unless the user explicitly requests their removal.",
                    "Review this exact prepared content before confirming. Do not automatically confirm.",
                    "Confirm concise: one useful memory, a summary heading, no transcript or repeated explanation.",
                    "Confirm accurateToSource: faithfully represents its stated basis; preserve exact numbers, dates, units and qualifications. This is not proof of factual truth.",
                    "Confirm structured: category is appropriate and hard data uses typed facts. Use references where needed.",
                    "Supply sourceBasis, a short basis explanation, and uncertainties. Mark inference and unknowns explicitly in the memory itself.",
                    "If any check fails, revise and prepare again. Submission creates only an owner-reviewed proposal.",
                ]}

    def confirmed(self, data, connection_id):
        if set(data) != {"reviewToken", "confirmation"}:
            raise Problem("Prepare first, then submit only reviewToken and confirmation. Changed content requires another preparation.", 422)
        token = text(data.get("reviewToken"), "review token", 100, True)
        ticket = self.tickets.get(token)
        if not ticket or ticket["connectionId"] != connection_id or ticket["expires"] <= time.monotonic():
            raise Problem("Review is unavailable or expired. Prepare again.", 409)
        if "batch" in ticket["payload"]:
            raise Problem("Use submit-batch for a prepared batch.", 422)
        c = data.get("confirmation")
        if not isinstance(c, dict) or any(c.get(k) is not True for k in ("concise", "accurateToSource", "structured", "userConfirmed")):
            raise Problem("Obtain explicit user confirmation of this exact preview, then confirm userConfirmed, concise, accurateToSource, and structured as true. Otherwise revise and prepare again.", 422)
        source = c.get("sourceBasis")
        if not isinstance(source, str) or source not in {"user_statement", "reference", "inference", "unknown"}:
            raise Problem("State sourceBasis: user_statement, reference, inference, or unknown.", 422)
        confirmation = {"concise": True, "accurateToSource": True, "structured": True, "userConfirmed": True,
                        "sourceBasis": source, "basis": text(c.get("basis"), "source basis explanation", 500, True),
                        "uncertainties": text(c.get("uncertainties"), "uncertainties", 1000)}
        if source == "reference" and not ticket["payload"]["content"]["references"]:
            raise Problem("Reference-based memories need a source reference in the preview. Prepare again with references.", 422)
        return ticket["payload"], confirmation

    def prepare_batch(self, data, connection_id):
        if not isinstance(data, dict) or set(data) != {"proposals", "groupTitle"}:
            raise Problem("Supply proposals and groupTitle only.", 422)
        proposals = data.get("proposals")
        if not isinstance(proposals, list) or not 2 <= len(proposals) <= 50:
            raise Problem("Prepare 2 to 50 proposals.", 422)
        title = text(data.get("groupTitle"), "group title", 200, True)
        payloads, updates = [], set()
        for proposal in proposals:
            if not isinstance(proposal, dict):
                raise Problem("Each batch proposal must be an object.", 422)
            preview = self.prepare(proposal, connection_id)
            payload = self.tickets.pop(preview["reviewToken"])["payload"]
            if payload["action"] == "update":
                if payload["nodeId"] in updates:
                    raise Problem("A batch cannot update the same memory twice.", 422)
                updates.add(payload["nodeId"])
            payloads.append(payload)
        batch_id = secrets.token_hex(16)
        current = time.monotonic()
        token = secrets.token_urlsafe(32)
        batch = {"batchId": batch_id, "title": title, "proposals": payloads,
                 "group": {"title": title, "type": "project", "kind": "record",
                           "body": "Groups the selected memories.", "tags": [], "facts": [],
                           "references": [], "status": "active", "relationType": "belongs_to",
                           "relationDirection": "member_to_group"}}
        self.tickets[token] = {"payload": {"batch": batch}, "connectionId": connection_id,
                               "expires": current + 600}
        return {"status": "confirmation_required", "reviewToken": token, "batch": batch,
                "checks": {"lengthWithinLimit": True, "explicitCategory": True,
                           "typedFactsValid": True, "factualTruthVerified": False},
                "instructions": ["Show the exact complete batch preview, including tags and the planned owner-created project group and belongs_to links, then request explicit confirmation."],
                "expiresInSeconds": 600}

    def confirmed_batch(self, data, connection_id):
        if not isinstance(data, dict) or set(data) != {"reviewToken", "confirmation"}:
            raise Problem("Prepare a batch first.", 422)
        token = text(data.get("reviewToken"), "review token", 100, True)
        ticket = self.tickets.get(token)
        if not ticket or ticket["connectionId"] != connection_id or ticket["expires"] <= time.monotonic():
            raise Problem("Review is unavailable or expired. Prepare again.", 409)
        batch = ticket["payload"].get("batch")
        if not isinstance(batch, dict):
            raise Problem("Prepare a batch first.", 422)
        c = data.get("confirmation")
        if not isinstance(c, dict) or any(c.get(k) is not True for k in ("concise", "accurateToSource", "structured", "userConfirmed")):
            raise Problem("Obtain explicit user confirmation of this preview.", 422)
        source = c.get("sourceBasis")
        if not isinstance(source, str) or source not in {"user_statement", "reference", "inference", "unknown"}:
            raise Problem("State sourceBasis: user_statement, reference, inference, or unknown.", 422)
        for payload in batch["proposals"]:
            if source == "reference" and not payload["content"]["references"]:
                raise Problem("Reference-based memories need source references in every preview.", 422)
        confirmation = {"concise": True, "accurateToSource": True, "structured": True, "userConfirmed": True,
                        "sourceBasis": source, "basis": text(c.get("basis"), "source basis explanation", 500, True),
                        "uncertainties": text(c.get("uncertainties"), "uncertainties", 1000)}
        return batch, confirmation

    def consume(self, token):
        self.tickets.pop(token, None)
