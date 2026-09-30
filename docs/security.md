# Security and recovery requirements

These are requirements for implementation, not protections supplied by this repository's UI sketch.

## Threat model

Protect against a stolen locked device, unauthorized local/network API callers, spoofed peers, accidental concurrent edits/deletions, malicious imported instructions, and unwanted AI context disclosure. Encryption at rest cannot protect plaintext from a compromised or unlocked device running authorized code.

## Required controls

- Use established encryption and mutually authenticated transports; do not invent cryptography.
- Encrypt database content, attachment payloads, and recoverable backups. Review temporary files, previews, search indexes, clipboard use, logs, and OS backups for plaintext leakage.
- Separate device identities, encryption keys, and recovery material. Store device-held secrets in the platform's secure facilities where available.
- Pair devices explicitly. Discovery advertisements are not evidence of trust. Define who may enroll or revoke peers; a sync credential must not automatically grant administration.
- Keep API grants scoped and revocable. Deny unknown callers, arbitrary SQL, and arbitrary file reads.
- Keep model instructions out of the authorization boundary. Treat source content as untrusted and initially keep AI tools limited to reads and proposals.
- Prevent model-generated content from triggering external image/network requests or executing markup/code by default.
- Show what memory goes to a model and where that model runs. Never silently fall back to an external provider.
- Define notification and lock-screen privacy so reminders need not reveal sensitive text.

## Recovery

Users need both a backup and valid recovery material. Provide a portable encrypted backup format that can be restored without the original device. A password used for everyday unlock and random recovery key are different concepts and must be explained clearly.

Changing an unlock password should re-protect appropriate key material rather than implicitly reusing it as every encryption key. Recovery key rotation and data key rotation are separate operations. Select and review the exact key hierarchy before implementation.

Restore into an isolated vault/new device identity first. Do not automatically reactivate an old peer membership list. Test restoration on a new device, not just a copy on the source machine.

If all usable keys and recovery material are lost, encrypted content may be unrecoverable. A revoked device can retain previously obtained data, and offline peers cannot know about a revocation until they receive it.

## Sync integrity and retention

Use atomic local changes, idempotent operation replay, authenticated peer identities, and content integrity checks. A timestamp is not a sufficient conflict-resolution protocol. Retain conflicting revisions and deletion markers under an explicit safe compaction policy.

Synchronization copies mistakes too. Maintain independent, encrypted, versioned backups. Distinguish archiving, recoverable deletion, and permanent deletion; historical copies and backups make immediate erasure everywhere impossible to promise.

## Acceptance gates

Before calling the implementation robust, verify unauthorized API calls and graph traversal, spoofed pairing, duplicate/reordered/interrupted sync, concurrent edit/delete with incorrect clocks, old-peer re-entry, lost-device membership handling, schema upgrade failures, corrupted/truncated transfers, and recovery on a fresh device.

Use malicious source text to test attempted vault export and permission expansion. Network inspection must confirm that local-only operation makes no external context or analytics calls. Confirm mobile OS backup behavior on actual platforms.

## References

- [SQLCipher design](https://www.zetetic.net/sqlcipher/design/)
- [Syncthing security principles](https://docs.syncthing.net/users/security.html)
- [OWASP prompt injection prevention](https://cheatsheetseries.owasp.org/cheatsheets/LLM_Prompt_Injection_Prevention_Cheat_Sheet.html)
