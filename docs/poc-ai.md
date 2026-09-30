# Connect an AI client to the POC

Alve does not require a particular model provider. A compatible AI client can call the local HTTP API or use the included stdio MCP adapter.

## Create a connection

1. Unlock Alve in the browser.
2. Create at least one memory.
3. Open **Connections**, select exactly the nodes this client may access, and choose search/read/propose permissions.
4. Copy the one-time connection token. The stored grant contains a token hash, not the token itself.

Connections are issued on each installation separately. Restore and peer bundles do not copy them. Revocation blocks future calls and blocks approval of that connection's pending proposals; it cannot retract data already read.

## MCP stdio adapter

Configure your AI client to launch Python with the absolute path to `app/mcp_bridge.py`. Set its environment to contain:

```json
{
  "ALVE_URL": "http://127.0.0.1:4765",
  "ALVE_TOKEN": "YOUR_SCOPED_CONNECTION_TOKEN"
}
```

An illustrative MCP client entry is:

```json
{
  "mcpServers": {
    "alve": {
      "command": "python",
      "args": ["/absolute/path/to/alve/app/mcp_bridge.py"],
      "env": {
        "ALVE_URL": "http://127.0.0.1:4765",
        "ALVE_TOKEN": "YOUR_SCOPED_CONNECTION_TOKEN"
      }
    }
  }
}
```

Client configuration formats vary. Keep real configuration containing the token outside the repository and use the client's secret store if available. The bridge requires only Python's standard library; the application server additionally requires `cryptography`.

The bridge supports the POC's bounded MCP `2025-11-25` stdio subset: initialization, ping, tool listing/calls, and the `alve://usage` resource. It is not a claim of complete MCP compatibility with every client. Read the usage resource before using memory tools.

Available tools:

| Tool | Permission | Effect |
| --- | --- | --- |
| `search_memory` | search | Search within explicit allowed nodes |
| `read_node` | read | Read a node and retained conflicts |
| `get_relations` | read | Return edges only when both endpoints are allowed |
| `propose_memory` | propose | Submit a create/update proposal for owner review |

AI cannot grant itself permissions, directly create confirmed nodes, export the vault, or obtain the encryption key. Scope does not expand automatically through graph edges. New approved memories require a new connection grant if the client needs to read them; editing scopes is future work.

Example proposal arguments:

```json
{
  "action": "create",
  "content": {
    "title": "Keep the main point visible",
    "body": "Use a concise heading and only necessary details. Keep supporting evidence in references.",
    "type": "memory",
    "kind": "preference",
    "tags": ["writing"]
  }
}
```

Updates also require `nodeId` and `expectedRevision`. Owner approval rechecks that revision and the submitting connection's permission. A proposal is not confirmed memory until approval succeeds.

## HTTP API

Use `Authorization: Bearer YOUR_SCOPED_CONNECTION_TOKEN`. GET `/api/ai/contract` describes the API and writing rules. Other endpoints are listed in [app/server.py](../app/server.py). Responses are not cached; foreign browser origins are rejected, while authenticated local non-browser clients can call without an Origin header.

The server must remain running and the vault unlocked. The bridge connects only to explicit localhost HTTP URLs and refuses redirects. There is no network discovery, remote MCP listener, or automatic provider configuration.

## Local-only usage

Use a local AI client/model if no content may leave your devices. A cloud AI client can upload the data returned by a local tool. Alve enforces API scope but cannot control what an authorized client does with returned information.

The POC tests the MCP-to-API read/proposal flow with synthetic requests. It does not install a model or claim that a specific third-party AI client has been tested.
