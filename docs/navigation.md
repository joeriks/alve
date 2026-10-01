# Navigation and relation handling

Back restores the previous view, query, status/tag filters, list length and scroll position. Browser history stores only opaque session/entry identifiers; memory content and queries stay in app memory and are cleared on lock. Back to results opens the relevant memory or agent list. These lists have separate search queries.

Selections remain in the unlocked session while switching views. Done and Clear selection explicitly clear them. The compact sticky selection bar opens Actions when needed. Active memories are shown by default; Filters supports archived/all memories and groups only. Groups show their member count and visible member list. Save keeps the edited memory open.

## Unlink and undo

A relation's Actions menu offers Unlink with an explicit endpoint/type preview. Undo unlink restores the most recently removed relation while the vault remains unlocked. Memories are retained. Creating relations requires distinct active non-conflicting endpoints and avoids active identical links.

The owner-only API is DELETE `/api/relations/{id}` and POST `/api/relations/{id}/restore`. Unlink retains a permanent tombstone for the original relation ID. Graph and AI reads hide it; encrypted bundles and exports retain it. Merge applies deletion-wins for the same relation ID and rejects changes to its immutable content. Undo creates a new relation ID, or reuses an already-active identical link, preserving the original tombstone.

All devices exchanging these bundles must use a version with tombstone support. Older app builds may display removed links or reject changed same-ID imports. A new independent relation ID is a new link; deletion-wins does not prohibit deliberately recreating a connection. Restore can fail if an endpoint became archived or conflicted. No hard deletion or tombstone pruning is performed.
