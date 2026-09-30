# Memory graph

The graph organizes context. Each memory still needs a clear, human-readable meaning. Not every number or date becomes a node.

## Nodes

Initial node types are `memory`, `project`, `person`, `event`, `document`, and `conversation`. Curated memories are decisions, preferences, insights, commitments, or records. Conversations and source documents are working material, not automatically curated memory.

A memory has a summary heading and a concise paragraph or bullet list. One sentence to roughly half an A4 page is guidance, not a minimum or mandatory quota. Longer content should invite splitting or moving detail to a source.

## Facts

Typed facts belong to nodes. Dates have no implied time zone; instants and appointments have explicit time semantics. Money uses an exact decimal string and currency. Measurements use an exact decimal string and a unit. Unknown values are distinct from zero; precision and user confirmation are independent properties.

When a heading includes a fact, render it from its field rather than maintaining another literal copy. Fact placeholders must be interpreted as data, never evaluated as code. References identify the document, node revision, or conversation message supporting a fact.

## Relations

Begin with a small vocabulary:

| Relation | Meaning |
| --- | --- |
| `belongs_to` | A memory belongs to a project or area |
| `based_on` | A decision or fact is supported by a source |
| `fulfills` | A decision fulfills a requirement |
| `related_to` | A useful association without a stronger assertion |
| `supersedes` | A newer memory replaces an older one |
| `contradicts` | Two claims need reconciliation |

Relations carry their own provenance and confirmation state. An AI-proposed association is not a confirmed fact. Endpoints must be in the same vault in the initial model. Authorized graph traversal must not disclose hidden nodes or their existence.

## Separate records

- **Proposals:** suggested additions or updates, with their basis revision and approval status.
- **Revisions:** historical human content and fact values, retained according to a defined policy.
- **Attachments:** encrypted payloads and metadata. A content hash helps integrity/deduplication but must not be publicly exposed as a content fingerprint.
- **Calendar events:** all-day dates or timed start/end values, recurrence, exceptions, local reminders, and relations to memories. These need a fuller schema before calendar implementation.
- **Sync operations:** immutable operation IDs and causal references; not merely the `updatedAt` field of the content.

The illustrative structures in [schemas/memory.ts](../schemas/memory.ts) and [examples/memory-graph.json](../examples/memory-graph.json) are not runtime validation schemas or a complete synchronization protocol.

## Potential SQLite tables

`nodes`, `memory_content`, `facts`, `relations`, `references`, `attachments`, `proposals`, `revisions`, `sync_operations`, `peer_acknowledgements`, and `device_membership`.

Use transactions and indexes for graph traversal and local text search. Search indexes must remain inside protected storage. Future migrations need backup/rollback and mixed-version peer handling.
