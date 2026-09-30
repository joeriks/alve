# Alve product specification

**Alve — Your memory. Your control. Your choice of AI.**

Alve is a personal, portable memory graph for the physical computers and phones a person owns. It keeps useful knowledge available across devices while preserving the user's ownership of the data, keys, and AI choices. Its positioning is privacy-first and user-controlled; claims of uniqueness are not treated as proven facts.

## Product promise

- Memories live locally in an encrypted SQLite/SQLCipher database. The user owns the encryption keys; a cloud account and cloud service are not required.
- Devices can exchange data directly over a trusted LAN or a phone hotspot. Sync is peer-to-peer and user-initiated in the foreground.
- Alve stores curated, human-readable memories, not a dump of chats, files, or model transcripts.
- The default AI runs locally on the user's computer, with optional on-device AI where practical. Alve never silently falls back to an external model.
- The user can export their data in open formats and restore it from backups.

## Memory model

A memory is written for a person to understand without reopening the original source. It has a short summary heading and a concise paragraph or factual bullet list. A normal entry ranges from one sentence to roughly half an A4 page.

Memories preserve exact, typed facts when known: dates, amounts of money, quantities, names, locations, and identifiers. They can include source references and provenance that explains where a fact came from. Each item distinguishes a user-confirmed fact from an AI proposal awaiting review.

The graph links nodes with typed relations, for example a decision `belongs_to` a project or is `based_on` a source. Amounts and dates normally remain typed facts on a node. Relations, sources, and confirmation/proposal status help users inspect why an entry exists and revise it safely.

## Core workflows

1. **Capture and curate.** A user supplies a note, conversation excerpt, file reference, or instruction to their chosen AI. Alve proposes a compact memory and graph links. The user reviews, edits, accepts, or rejects it before it becomes confirmed.
2. **Recall.** A user searches or asks a question through Alve and sees relevant curated memories, their sources, and whether the answer relies on confirmed information or proposals.
3. **Reconcile.** When a new proposal overlaps an existing entry, Alve suggests a merge, deduplication, or conflict resolution. It does not silently delete memories; the user decides what is retained.
4. **Sync and recover.** A user explicitly connects two owned devices on a LAN or phone hotspot, monitors foreground sync, and can back up, restore, or export their data.

## AI integration

The primary integration surface is a local, user-controlled API that lets a selected model read permitted context and submit proposed memories. An optional MCP adapter may expose the same controlled capability to compatible tools. The Alve app may also connect directly to a model chosen by the user.

AI access is scoped by the user and designed around proposals rather than unreviewed writes. Models receive only the information necessary for an interaction. External models are optional and require an explicit user choice; no external provider is a hidden fallback.

## Boundaries for the first milestone

The first milestone is a prototype that communicates the product through documentation and UI. It is not a functioning security, cryptography, or synchronization implementation, and it must not imply that encrypted storage, key management, device pairing, conflict resolution, backup, or restore has been validated.

The MVP should target one desktop platform and one phone platform, but those platforms remain undecided. Platform choice follows prototype feedback and feasibility. The MVP scope includes local encrypted storage, user-owned key handling, foreground peer-to-peer sync, backup and restore, open export, reviewed AI proposals, and basic graph recall.

## MVP success criteria

- A new user can create, review, search, edit, export, back up, and restore curated memories without a cloud account.
- Two supported owned devices can complete a clearly visible, user-initiated foreground sync over LAN or phone hotspot.
- Conflicting or duplicate proposed memories are surfaced for review; no sync or AI path automatically deletes a memory.
- Users can tell which claims are confirmed, proposed by AI, and linked to a source.
- A user can select local AI and complete core capture and recall without an external model or network connection beyond local device communication.
- An exported dataset is documented and usable without depending on Alve.
