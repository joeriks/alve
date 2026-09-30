# Storage, backups, and synchronization

Alve currently stores one vault as an encrypted local snapshot. The native desktop app writes `memory.alve` under the operating system's Alve application-data directory by default. Set `ALVE_DATA_DIR` to choose another directory, for example a workspace directory for development. Alve keeps no hosted copy of the vault, and Alve does not provide automatic backups.

The snapshot is encrypted when it is written and decrypted only in the running application while the vault is unlocked. The passphrase protects the vault. If the passphrase is lost, Alve cannot recover the contents. Keep the passphrase and backups under your own control; do not put either in the repository or in an AI client's configuration.

## Your backup responsibility

The local snapshot is the working copy, not a backup. A device failure, accidental deletion, ransomware, or loss of the passphrase can make the vault unavailable. The operating system's application-data directory may be included in OS backup tools, but Alve does not configure, monitor, or guarantee those backups.

For an independent backup, export a versioned encrypted `.alve` bundle and keep copies on a separate device or offline medium. Keep more than one dated version when the history matters. Periodically verify that a copy can be restored into a fresh installation while the passphrase is available; having a file that has never been restored is not a recovery test. Protect any readable export separately: plain JSON is an explicit export, not an encrypted backup.

Bundles contain the confirmed memory graph, relations, and revision history. They intentionally exclude AI connection grants, token hashes, and pending proposals. A bundle therefore carries memory, not access to the source installation; issue new scoped AI connections after restoring it.

## Manual transfer and merge

Bundle exchange is currently a manual operation:

1. Export an encrypted bundle from installation A.
2. Transfer it yourself to installation B using a channel you control.
3. On a fresh B, use **Restore** with the bundle passphrase.
4. After either installation has changed, export from each side and import the other side's bundle as needed.

Restore is for a fresh installation without an existing vault. Import is the same-vault operation: it merges the bundle into the current vault, retains revision history and conflicts, and does not silently replace existing data. Review conflicting heads and resolve them explicitly. Foreign-vault bundles are rejected.

This exchange is useful for moving a vault or manually reconciling two offline installations. It is not network synchronization. There is currently no LAN or phone-hotspot sync and no native phone app. A future design may support explicit, foreground, paired peer-to-peer exchange with status shown per peer. A single global “freshness” indicator would not describe disconnected peers accurately.

## Synchronization is not backup

Synchronization is about exchanging changes between trusted installations. Backup is an independent, recoverable copy kept in case the working data or a synchronization decision is lost. Synchronization can copy an accidental edit or deletion, and a future offline peer will not know about a revocation until it receives an update. Keep independent encrypted backup versions even if peer synchronization is added later.

Do not overwrite an existing destination vault to migrate it. Restore to a fresh installation first, confirm the graph and exact values, and retain the source and independent backup until verification is complete. For reciprocal manual exchange, use export and import so both sides' retained history and conflicts can be reviewed.


## Recovery drill

Before relying on backups, export a dated encrypted bundle and restore it into a fresh, separate data directory. Do not restore over your working vault. Compare the vault ID, memory headings, tags, exact facts, relations and revision history, then lock and reopen the recovered vault. Changes made after the backup will not be recovered from that older file. AI grants and pending proposals are intentionally excluded; create new connections separately if needed.

The synthetic recovery acceptance tests exercise a separately stored dated bundle, exact monetary values, graph relations, revision history and reopen. They also check that wrong passwords and tampered bundles create no vault, and restoring over an existing vault is refused without changing it. Rust/Python interoperability tests restore a native batch group and its members into the Python reference implementation. These checks verify application behavior; they do not certify that your personal backup destination or password copy is usable.

Run `python -m unittest discover -s tests -p test_recovery.py -v` for the isolated drill. Build the native acceptance driver and run the full suite for cross-runtime recovery.
