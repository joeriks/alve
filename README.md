# Alve

**Your memory. Your control. Your choice of AI.**

Alve is a proposed personal, portable memory for computers and phones. It keeps the important parts of your thinking in a human-readable memory graph: decisions, preferences, commitments, insights, precise facts, and their sources.

Your memory lives on your devices. AI clients use a local, permission-controlled interface to retrieve relevant context and propose improvements. The memory remains yours when you change devices, applications, or AI providers.

## Current status

This repository now includes a working local proof of concept alongside the original specifications and UI sketch. The POC implements encrypted persistence, graph editing, typed facts, scoped AI access, reviewed proposals, an MCP bridge, and manual encrypted bundle exchange with retained conflicts.

It is not a production security implementation. Native phone apps, automatic LAN/hotspot transport, secure device pairing, SQLCipher integration, attachments, calendar recurrence/reminders, and app-initiated model inference remain future work.

## Run the POC

Requires Python 3.12 or newer with SQLite serialization support.

```sh
python -m venv .venv
```

Activate the virtual environment before installing or running, or invoke its Python executable directly. On Windows, that executable is `.venv/Scripts/python.exe`; on macOS/Linux it is `.venv/bin/python`.

```sh
python -m pip install -r requirements.txt
python -m app --data-dir private-vaults/poc
```

On Windows, after dependencies are available, `./start-alve.ps1` selects the local virtual environment or bundled Codex Python when available. It also accepts `-PythonPath`, `-Port`, and `-DataDirectory`.

Open **http://127.0.0.1:4765** and create a vault with a passphrase of at least 12 characters. The application listens only on this computer. No account or external network call is needed.

- Create and link concise memories with precise facts and references.
- Grant an AI connection an explicit selection of nodes and permissions.
- Review its proposals before they become confirmed memory.
- Download an encrypted bundle for backup or transfer to another installation.
- Restore on a fresh installation, or merge a same-vault bundle into an existing unlocked installation.

See [POC operation and limitations](docs/poc.md) and [AI client setup](docs/poc-ai.md). Run the acceptance tests with `python -m unittest discover -s tests -v`.

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

The original sketch above is separate from the working POC served by `python -m app`.

## Decisions still open

- First desktop and mobile operating systems, and application framework.
- Concrete synchronization protocol, cryptographic libraries, and key recovery format.
- Retention periods for working material, historical revisions, and deletion records.
- Which AI clients and local model runtimes to support first.

The specification is a starting point for implementation, not a security certification or a claim of unique market positioning.
