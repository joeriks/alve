# Alve

**Your memory. Your control. Your choice of AI.**

Alve is a proposed personal, portable memory for computers and phones. It keeps the important parts of your thinking in a human-readable memory graph: decisions, preferences, commitments, insights, precise facts, and their sources.

Your memory lives on your devices. AI clients use a local, permission-controlled interface to retrieve relevant context and propose improvements. The memory remains yours when you change devices, applications, or AI providers.

## Current status

This repository contains the initial product specification, architecture proposal, illustrative data types, and an interactive UI sketch. **Encryption, peer-to-peer synchronization, authentication, model connections, and production storage are not implemented.** The UI uses fictional example data and simulated actions.

## Product principles

- Keep a curated memory, rather than automatically retaining every AI response.
- Write for humans: a meaningful summary heading, a concise paragraph or bullets, and references where useful.
- Store exact dates, money, and measurements as typed facts rather than duplicated prose.
- Connect memories through meaningful, traceable graph relations.
- Work offline with encrypted local storage and user-controlled keys.
- Synchronize directly between approved devices on a local network or phone hotspot.
- Let users control each AI connection's scope, lifetime, and permitted actions.
- Preserve open exports and independent, recoverable backups.

## Explore the design

- [Product and user experience](docs/product.md)
- [Architecture and security boundaries](docs/architecture.md)
- [Memory graph and data model](docs/data-model.md)
- [Local AI API and AI usage contract](docs/ai-interface.md)
- [Security and recovery](docs/security.md)
- [MVP roadmap and acceptance criteria](docs/roadmap.md)
- [Illustrative TypeScript structures](schemas/memory.ts)
- [Example memory graph](examples/memory-graph.json)

Open [the standalone UI sketch](ui/index.html) in a modern browser. It has desktop and phone views, searchable memories, structured facts, references, editing, and a simulated AI interaction. Changes exist only in browser memory and are lost when the page reloads. The editable source is [ui/memory-sketch.fragment.html](ui/memory-sketch.fragment.html).

## Decisions still open

- First desktop and mobile operating systems, and application framework.
- Concrete synchronization protocol, cryptographic libraries, and key recovery format.
- Retention periods for working material, historical revisions, and deletion records.
- Which AI clients and local model runtimes to support first.

The specification is a starting point for implementation, not a security certification or a claim of unique market positioning.
