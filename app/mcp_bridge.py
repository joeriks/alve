"""Small stdio MCP adapter for the POC's scoped localhost API.

Credentials come from ALVE_TOKEN; no vault passphrase is exposed to an AI client.
"""
import json
import os
import sys
from urllib.error import HTTPError
from urllib.parse import quote, urlencode, urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

PROTOCOL = "2025-11-25"
MAX_LINE = 256 * 1024


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *_args, **_kwargs):
        return None


def api(path, body=None):
    base = os.environ.get("ALVE_URL", "http://127.0.0.1:4765").rstrip("/")
    parsed = urlsplit(base)
    if (parsed.scheme != "http" or parsed.hostname not in {"127.0.0.1", "localhost"}
            or not parsed.port or parsed.username or parsed.password or parsed.path or parsed.query or parsed.fragment):
        raise ValueError("ALVE_URL must be an explicit localhost HTTP address with a port.")
    token = os.environ.get("ALVE_TOKEN", "")
    if not token:
        raise ValueError("Set ALVE_TOKEN to a scoped connection token from the Alve UI.")
    headers = {"Authorization": "Bearer " + token}
    if body is not None:
        headers["Content-Type"] = "application/json"
    request = Request(base + path, json.dumps(body).encode() if body is not None else None, headers)
    try:
        with build_opener(NoRedirect).open(request, timeout=15) as response:
            return json.load(response)
    except HTTPError as exc:
        try:
            error = json.loads(exc.read(4096)).get("error", "API request denied")
        except ValueError:
            error = "API request denied"
        raise ValueError(f"Alve API {exc.code}: {error}") from None


TOOLS = [
    {"name": "search_memory", "description": "Search allowed human-readable memories. Results may be stale while peers are offline.",
     "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}, "limit": {"type": "integer", "minimum": 1, "maximum": 100}}, "required": ["query"]}},
    {"name": "read_node", "description": "Read an allowed memory node and any conflicting versions.",
     "inputSchema": {"type": "object", "properties": {"node_id": {"type": "string"}}, "required": ["node_id"]}},
    {"name": "get_relations", "description": "Read only relations whose endpoints are both allowed.",
     "inputSchema": {"type": "object", "properties": {"node_id": {"type": "string"}}, "required": ["node_id"]}},
    {"name": "propose_memory", "description": "Submit a concise memory for owner review. This does not create confirmed memory.",
     "inputSchema": {"type": "object", "properties": {
         "action": {"type": "string", "enum": ["create", "update"]},
         "nodeId": {"type": "string"}, "expectedRevision": {"type": "string"},
         "content": {"type": "object", "properties": {"title": {"type": "string"}, "body": {"type": "string"}}, "required": ["title"]}},
         "required": ["content"]}},
]


def handle(message):
    if not isinstance(message, dict) or message.get("jsonrpc") != "2.0":
        return {"jsonrpc": "2.0", "id": None, "error": {"code": -32600, "message": "Invalid JSON-RPC request"}}
    identifier = message.get("id")
    if "id" not in message:
        return None
    method = message.get("method")
    params = message.get("params", {})
    if not isinstance(params, dict):
        return {"jsonrpc": "2.0", "id": identifier, "error": {"code": -32602, "message": "Invalid parameters"}}
    try:
        if method == "initialize":
            result = {"protocolVersion": PROTOCOL, "capabilities": {"tools": {}, "resources": {}},
                      "serverInfo": {"name": "alve-local-memory-poc", "version": "0.1.0"},
                      "instructions": "Read alve://usage before using memory. All writes are owner-reviewed proposals."}
        elif method == "ping":
            result = {}
        elif method == "tools/list":
            result = {"tools": TOOLS}
        elif method == "resources/list":
            result = {"resources": [{"uri": "alve://usage", "name": "Alve usage contract", "mimeType": "application/json"}]}
        elif method == "resources/read":
            if params.get("uri") != "alve://usage":
                raise ValueError("Unknown resource")
            result = {"contents": [{"uri": "alve://usage", "mimeType": "application/json", "text": json.dumps(api("/api/ai/contract"))}]}
        elif method == "tools/call":
            name, args = params.get("name"), params.get("arguments", {})
            if not isinstance(args, dict):
                raise ValueError("Tool arguments must be an object")
            try:
                if name == "search_memory":
                    data = api("/api/ai/search?" + urlencode({"q": args.get("query", ""), "limit": args.get("limit", 20)}))
                elif name in {"read_node", "get_relations"}:
                    node_id = args.get("node_id")
                    if not isinstance(node_id, str) or not node_id:
                        raise ValueError("node_id is required")
                    data = api("/api/ai/nodes/" + quote(node_id, safe="") + ("/relations" if name == "get_relations" else ""))
                elif name == "propose_memory":
                    data = api("/api/ai/proposals", args)
                else:
                    raise ValueError("Unknown tool")
                result = {"content": [{"type": "text", "text": json.dumps(data, ensure_ascii=False)}], "isError": False}
            except (ValueError, OSError):
                result = {"content": [{"type": "text", "text": "The local memory request failed or was denied. Check the vault, connection permissions, and input."}], "isError": True}
        else:
            return {"jsonrpc": "2.0", "id": identifier, "error": {"code": -32601, "message": "Method not found"}}
        return {"jsonrpc": "2.0", "id": identifier, "result": result}
    except (ValueError, OSError):
        return {"jsonrpc": "2.0", "id": identifier, "error": {"code": -32602, "message": "Request failed; check local access and parameters"}}


def main():
    while True:
        line = sys.stdin.buffer.readline(MAX_LINE + 1)
        if not line:
            break
        if len(line) > MAX_LINE:
            # Terminate rather than interpreting the tail of an oversized request.
            break
        try:
            result = handle(json.loads(line))
        except (ValueError, UnicodeDecodeError):
            result = {"jsonrpc": "2.0", "id": None, "error": {"code": -32700, "message": "Parse error"}}
        if result is not None:
            sys.stdout.write(json.dumps(result, ensure_ascii=False) + "\n")
            sys.stdout.flush()


if __name__ == "__main__":
    main()
