# Working POC

## Implemented

- English responsive browser UI backed by a local Python application.
- Human-readable nodes, exact typed facts, references, typed graph relations, and local search.
- Immutable revision history, archived nodes, persistent conflict heads, and explicit resolution.
- SQLite in memory, serialized to an AES-256-GCM authenticated encrypted file after successful mutations.
- Passphrase-derived keys using fixed scrypt parameters (`N=32768`, `r=8`, `p=1`), random salt, and fresh encryption nonces.
- Atomic encrypted writes; failed persistence rolls back the in-memory mutation.
- Loopback-only API with owner bearer sessions and separate per-connection AI tokens.
- Explicit AI node scopes, search/read/propose permissions, revocation, and owner-reviewed proposals.
- Local stdio MCP bridge exposing the usage contract and four memory tools.
- Encrypted graph/history bundles, restore into a new installation, and idempotent same-vault merge.

## Run and unlock

Install `requirements.txt` into Python 3.12+ and run:

```sh
python -m app --port 4765 --data-dir private-vaults/poc
```

Open `http://127.0.0.1:4765`. The data directory holds `memory.alve` and a process lock file. Do not run two instances against the same directory. An OS-level process lock prevents that overwrite scenario.

Create a vault with a strong passphrase of at least 12 characters. The POC uses this passphrase directly to derive the encryption key; it does not yet implement the proposed separate master/recovery-key hierarchy or biometric unlock. Alve cannot recover a lost passphrase.

Owner session credentials exist only in browser memory. Reloading requires entering the passphrase again and replaces the previous owner session. AI connection tokens remain device-local and are only usable while that vault is unlocked. Fifteen minutes without owner API activity locks the vault on the next API request. A manual Lock action locks immediately.

## Manual peer exchange

This POC demonstrates reconciliation without building network discovery or pairing:

1. On installation A, download an encrypted `.alve` bundle.
2. Transfer it yourself to installation B.
3. On a fresh B, use **Restore** and the bundle passphrase. B receives the same graph/vault identity, not A's AI credentials or proposals.
4. Edit on both installations while disconnected.
5. Export bundles and import them on the other installation.
6. Open any conflicting node, compare retained versions, and explicitly choose which content to retain. The resolution references all current heads; previous versions remain in history.

To simulate two installations on one computer, run a second process with `--port 4766 --data-dir private-vaults/peer`. The listener still stays on loopback. This is not LAN synchronization or a native mobile application.

Bundles contain complete confirmed graph history and relations; they are not efficient incremental transfers. AI grants, token hashes, and pending proposals are intentionally excluded. Relations are currently append-only. Archive replaces destructive deletion; tombstones and permanent erasure are not implemented.

## Recovery and exports

The encrypted bundle is a recoverable graph backup when paired with its passphrase. Store independent versions outside the active data directory. Synchronization is not backup. Plain JSON export is explicit and contains readable content; protect its destination yourself.

Restore only works on an installation without an existing vault. Import merges into the same vault without replacing its history; foreign-vault bundles are rejected. No existing data is silently overwritten by restore.

## Security boundaries and limitations

- This is encrypted snapshot persistence, **not SQLCipher**. The entire SQLite database is decrypted in process memory while open and encrypted on write. It is suitable for a bounded POC, not a large production database.
- Uses established cryptographic primitives through `cryptography`; the POC-specific file envelope and key lifecycle are not an audited standard.
- The file header authenticates the format and vault identity. Tampering is detected, but replay of an older valid snapshot is not.
- Fixed limits: 16 MiB per encrypted file, 5,000 revisions, 5,000 relations, 30 facts per node. These are explicit POC limits, not importance rankings.
- No full-disk/memory protection, secure memory erasure, hardware key custody, automatic OS backup controls, cryptographic peer identities, or key rotation. A compromised unlocked device can read the memory.
- References support titles and HTTP(S) links; no automatic URL fetching or attachment storage. Event nodes are records, not a complete calendar.
- No outgoing model calls, analytics, or external AI fallback. The MCP client can itself be cloud-hosted: any memory returned to that client may leave this computer. Choose the client accordingly.
- The local API rejects foreign hosts/origins and does not enable CORS. It is intentionally not exposed on Wi-Fi. Do not modify the bind address to bypass this boundary.

## Validation

Run `python -m unittest discover -s tests -v` for persistence, wrong-passphrase/tampering rejection, fault-injected atomic-write failures, exact fact validation, AI scope/authorization, proposal approval races, browser-session renewal, credential exclusion from restore, offline conflicts, repeat import, and MCP-to-API integration.

The browser smoke flow separately checks real rendering, editing, facts, relations, connection-token delivery, proposal review, downloads, lock/unlock, and narrow layouts. Tests use synthetic data. Real model inference and real phones have not been validated.
