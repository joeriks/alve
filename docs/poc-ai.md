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
| `prepare_memory` | propose | Validate a complete create/update candidate and issue a short-lived review token; stores nothing |
| `propose_memory` | propose | Submit a prepared candidate with explicit human-confirmation and quality attestations for owner review |

AI cannot grant itself permissions, directly create confirmed nodes, export the vault, or obtain the encryption key. Scope does not expand automatically through graph edges. New approved memories require a new connection grant if the client needs to read them; editing scopes is future work.

AI proposals use a mandatory two-step quality handshake. First call `prepare_memory` with the complete candidate. Its content must include a meaningful title (at most 120 characters), a body (at most 2,000 characters and 300 whitespace-separated words), explicit `type` (`memory`, `project`, `person`, `event`, or `document`), and `kind` (`decision`, `preference`, `insight`, `commitment`, or `record`). It also supports `tags`, typed `facts`, `references`, and `status`. Decimal money and quantity amounts are strings, and dates remain `YYYY-MM-DD`, so their values are not rounded or reformatted.

`prepare_memory` returns `confirmation_required`, the candidate content, checks, instructions, and a review token valid for 600 seconds. It neither saves a node nor creates a pending proposal. The AI must show the user the exact returned preview — including categories, facts, references, and status — and state:

> This is how the information will be stored. Show this to the user and request confirmation.

It must obtain the user's explicit confirmation before it can call `propose_memory`. For updates, include `nodeId` and `expectedRevision`. The review token is bound to the exact prepared payload and connection, is single-use, and cannot be used after changing content.

Example prepare arguments:

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

After explicit user confirmation, call `propose_memory` with only the returned `reviewToken` and a confirmation object. `concise`, `accurateToSource`, `structured`, and `userConfirmed` must all be `true`; state the source basis as `user_statement`, `reference`, `inference`, or `unknown`, with a short nonempty `basis` and any `uncertainties`.

```json
{
  "reviewToken": "TOKEN_RETURNED_BY_PREPARE",
  "confirmation": {
    "concise": true,
    "accurateToSource": true,
    "structured": true,
    "userConfirmed": true,
    "sourceBasis": "user_statement",
    "basis": "The user explicitly asked to retain this writing preference.",
    "uncertainties": ""
  }
}
```

`userConfirmed` is an AI-reported attestation; it does not independently prove what the user saw or said, and Alve does not claim autonomous AI truth verification. The internal owner approval remains mandatory. Owner approval rechecks the update revision and the submitting connection's permission; a proposal is never confirmed memory until approval succeeds. API and MCP errors include safe validation details so the client can correct a proposal, without disclosing connection tokens or request payloads.

## HTTP API

Use `Authorization: Bearer YOUR_SCOPED_CONNECTION_TOKEN`. GET `/api/ai/contract` describes the API and writing rules. Use `POST /api/ai/proposals/prepare` with the complete candidate, then `POST /api/ai/proposals` with only the review token and confirmation. Other endpoints are listed in [app/server.py](../app/server.py). Responses are not cached; foreign browser origins are rejected, while authenticated local non-browser clients can call without an Origin header.

The server must remain running and the vault unlocked. The bridge connects only to explicit localhost HTTP URLs and refuses redirects. There is no network discovery, remote MCP listener, or automatic provider configuration.

## Local-only usage

Use a local AI client/model if no content may leave your devices. A cloud AI client can upload the data returned by a local tool. Alve enforces API scope but cannot control what an authorized client does with returned information.

The POC tests the MCP-to-API read/proposal flow with synthetic requests. It does not install a model or claim that a specific third-party AI client has been tested.
