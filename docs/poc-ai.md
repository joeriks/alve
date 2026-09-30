# Connect an AI client to the POC

Alve does not require a particular model provider. A compatible AI client can call the local HTTP API or use the included stdio MCP adapter.

## Create a connection

1. Unlock Alve in its desktop window (or the Python reference browser UI).
2. Create at least one memory.
3. Open **Connections**, select exactly the nodes this client may access, and choose search/read/propose permissions.
4. Copy the one-time connection token. The stored grant contains a token hash, not the token itself.

The **Vaults** screen lists the current instance's vault and its active AI grants. A connection's `name` labels the AI client; `vaultAlias` is the name of the vault shown to that client. New grants default to `memory` unless the owner chooses another alias. Existing grants without an alias remain unnamed. Scoped API responses include `vaultId` and `vaultAlias` (null for an unnamed or owner session). These labels do not change node scope or permissions and do not verify the AI provider. The POC manages one vault per instance; other instances are not listed. Revoked connections are shown separately and do not count as active access.

Connections are issued on each installation separately. Restore and peer bundles do not copy them. Revocation blocks future calls and blocks approval of that connection's pending proposals; it cannot retract data already read.

## Native MCP companion

For the Tauri app, use the compiled `alve-mcp` executable. See [native AI setup](tauri.md#ai-clients) for the client configuration. No Python runtime is needed. Both adapters expose the tools below.

## Python reference MCP adapter

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

## Search memory

`search_memory` accepts optional text `query` (up to 1,000 characters; an empty query supports structured lookup), `tags` (up to 20 tags of up to 60 characters), `type`, `kind`, `updatedSince`, `updatedBefore`, `includeArchived`, `limit`, `offset`, and `sort`. Tags are repeated `tag` query parameters and all must match exactly, case-insensitively. Text matches case-insensitive substrings in titles, bodies, tags, fact labels and typed values, and reference titles. `type` and `kind` use the same categories as proposals.

`updatedSince` is inclusive and `updatedBefore` is exclusive; both must be offset-aware ISO 8601 timestamps. `includeArchived` is a boolean and defaults to `false`. `limit` defaults to 20 (1–100), `offset` defaults to 0 (0–5,000), and `sort` defaults to `relevance`; use `updated` for modification-time order. Relevance ranks title matches, then tags, then body and structured facts, then newer changes, with stable IDs breaking ties. Responses include `nextOffset` when another page is available, `asOf` in UTC, and the sort used.

Every returned node has a stable UUID `id` and a changing `revisionId`: use `id` to refer to the memory and `revisionId` for a specific version or update check. `updatedAt` records modification time. It is not an event date and does not prove that all peers are fresh; store event dates as typed date facts. Offset pagination observes a live local dataset, so rerun a search after edits or synchronization before relying on later pages.

Search runs only against nodes authorized for the connection. It uses no vector index, model, or outgoing call.

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

Owners can review up to 50 pending proposals atomically with `POST /api/proposals/review-batch`, using unique `proposalIds` and `action` `approve` or `reject`. An approval batch may explicitly supply `groupTitle` for two or more proposals; Alve then creates an owner-origin project and `belongs_to` relations. Any invalid, stale, revoked, or missing proposal rejects the whole batch without saving partial reviews or grouping.

The server must remain running and the vault unlocked. The bridge connects only to explicit localhost HTTP URLs and refuses redirects. There is no network discovery, remote MCP listener, or automatic provider configuration.

## Local-only usage

Use a local AI client/model if no content may leave your devices. A cloud AI client can upload the data returned by a local tool. Alve enforces API scope but cannot control what an authorized client does with returned information.

The POC tests the MCP-to-API read/proposal flow with synthetic requests. It does not install a model or claim that a specific third-party AI client has been tested.

## Reviewing related memories together

An AI client may prepare several related memories, show all exact previews (including tags or no tags), and ask for one explicit confirmation covering that complete set. Each proposal still uses its own bound review token; changed or later proposals need fresh confirmation.

In **Proposals**, select the memories to review and approve once. Optional shared grouping creates an owner-confirmed project node and `belongs_to` relations in the same atomic transaction. Tags are preserved exactly, not inferred from the AI conversation. Shared tags alone create no relation. The owner can inspect Tags / No tags before approval and search tags in the memory list. Existing saved memories are not modified by this workflow.
