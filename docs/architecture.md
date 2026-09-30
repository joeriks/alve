# Architecture proposal

Alve is a local-first application on each physical device. No mandatory central server or cloud account is part of the initial design.

```text
AI client ── local API / optional MCP adapter ──┐
                                              │
Alve UI ──────────────────────────────── application core
                                              │
                                  permission and proposal checks
                                              │
                                  memory graph / revisions
                                              │
                                SQLCipher + encrypted attachments
                                              │
                                   peer synchronization engine
                                              │
                              authenticated encrypted connection
                                              │
                                      another approved device
```

## Separation of responsibilities

1. **Application core:** validates types, resolves references, records provenance, handles proposals and revisions, and enforces permissions.
2. **Local storage:** a SQLCipher database per device and vault, plus encrypted attachments. The graph is a logical model stored in relational tables; a dedicated graph database is not required.
3. **Peer sync:** exchanges identified changes and attachment content between approved devices. It does not copy a live SQLite database between writers.
4. **AI gateway:** exposes scoped memory tools to AI clients. It never exposes the vault key or arbitrary SQL.
5. **Model connector:** lets the app start a conversation with a chosen local model. This is distinct from exposing tools to an external AI client.
6. **Backup/export:** creates independently recoverable encrypted backups and explicit open-format exports.

## Vaults and keys

A vault is a security boundary, containing graph data, sources, attachments, and synchronization history. Separate vaults may have separate keys and approved devices. Individual databases are implementation details, not automatically independent security boundaries.

Separate data-encryption keys, device identity keys, and user-held recovery material. A human password protects key material through an established password-based derivation scheme; it is not directly used as a database encryption key. Exact key wrapping and recovery formats require implementation review.

## Local P2P synchronization

- Pair devices explicitly with authenticated QR-based enrollment. Discovery is never authorization.
- Start with LAN or phone-hotspot connections and foreground synchronization.
- Each device works offline and records changes atomically with their local effects.
- Give operations unique IDs and causal revision information. Replaying the same operation must be safe.
- Exchange missing changes, acknowledge durable receipt, and resume interrupted transfers.
- Preserve concurrent human edits; do not select a winner solely by wall-clock time.
- Carry changes transitively: a phone can deliver a laptop's changes to another approved computer.
- Keep deletion records until a defined acknowledgement or re-enrollment policy makes compaction safe.

The protocol must be specified before implementation, including ordering, attachment verification, conflict handling, schema compatibility, membership changes, and interrupted transactions.

## Availability guarantees

Local memory remains available while the vault is unlocked even without a peer or model. A device only learns new changes when it reaches a peer that already has them. It cannot establish that it has the globally latest state while peers are unreachable.

Show local save status separately from per-peer synchronization status. Foreground synchronization is the initial reliable interaction; mobile background behavior depends on platform scheduling. Test hotspot routing and discovery on actual target devices before promising support.

## Portability

Export curated text to Markdown, complete typed graph content to versioned JSON, and calendar entries to ICS. Preserve IDs, relation types, source metadata, and attachments in a documented archive. Plaintext exports require an explicit user action and clear destination. Backups should retain encryption and key recovery information.

## References

- [SQLCipher design](https://www.zetetic.net/sqlcipher/design/)
- [MCP SDK overview](https://ts.sdk.modelcontextprotocol.io/)
- [Apple background task strategies](https://developer.apple.com/documentation/BackgroundTasks/choosing-background-strategies-for-your-app)
