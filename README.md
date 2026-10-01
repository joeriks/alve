# Alve

**Your memory. Your control. Your choice of AI.**

Alve is a proposed personal, portable memory for computers and phones. It keeps the important parts of your thinking in a human-readable memory graph: decisions, preferences, commitments, insights, precise facts, and their sources.

Your memory lives on your devices. AI clients use a local, permission-controlled interface to retrieve relevant context and propose improvements. The memory remains yours when you change devices, applications, or AI providers.

## Current status

The desktop POC runs in Tauri 2 with a Rust core and a bundled HTML/CSS/JavaScript interface. It implements encrypted persistence, graph editing, typed facts, scoped AI access, reviewed proposals, and manual encrypted bundle exchange with retained conflicts. A native `alve-mcp` stdio companion connects AI clients to the local Rust API. The Python POC remains as a reference implementation and interoperability test fixture.

It is not a production security implementation. Native phone apps, automatic LAN/hotspot transport, secure device pairing, SQLCipher integration, attachments, calendar recurrence/reminders, and app-initiated model inference remain future work.

## Run the native desktop app

Download the Windows x64 or ARM64 installer from [GitHub Releases](https://github.com/joeriks/alve/releases/latest). The installer includes the native MCP companion. In the desktop app, choose **Help → Check for updates…** to check manually, then **Install update and restart** to install a newer signed release. Checking requires an internet connection; local memory use does not. See [release and update operation](docs/releases.md).

Building requires Node.js, Rust, and the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/). On Windows, install the MSVC C++ build tools and WebView2. The compiled app and native MCP companion do not require Python, Node.js, or Cargo at runtime.

```sh
npm ci
npm run dev
```

Build a release executable with `npm run build -- --no-bundle`. On Windows it is `target/release/alve.exe`. Build the MCP companion with `cargo build -p alve-core --bin alve-mcp --release --locked`.

The UI opens directly in the desktop window. The locked screen does not decrypt the vault or start a model. Unlock performs the passphrase derivation on a worker thread; app startup and unlocking are separate operations. Startup latency must be measured on real devices; no one-second guarantee is claimed.

The default vault is in the operating system's Alve application-data directory. `ALVE_DATA_DIR` selects another directory; `ALVE_API_PORT` selects the loopback AI port. The native app exposes only scoped AI routes over HTTP; owner operations use local Tauri IPC. **AI contract** shows the active HTTP endpoint. If the preferred port is busy, the app chooses an available loopback port.

See [native operation and migration](docs/tauri.md) and [storage, backups, and synchronization](docs/storage-and-sync.md). Native phone builds and automatic LAN synchronization remain future work.

## Run the Python reference POC

Requires Python 3.12 or newer with SQLite serialization support.

```sh
python -m venv .venv
```

Activate the virtual environment before installing or running, or invoke its Python executable directly. On Windows, that executable is `.venv/Scripts/python.exe`; on macOS/Linux it is `.venv/bin/python`.

```sh
python -m pip install -r requirements.txt
python -m app --data-dir private-vaults/poc
```

On Windows, `./start-python-poc.ps1` selects the local virtual environment or bundled Codex Python when available. It accepts `-PythonPath`, `-Port`, and `-DataDirectory`. `./start-alve.ps1` launches the compiled native app.

Open **http://127.0.0.1:4765** and create a vault with a passphrase of at least 12 characters. The application listens only on this computer. No account or external network call is needed.

- Create and link concise memories with precise facts and references.
- Grant an AI connection an explicit selection of nodes and permissions.
- Review exact AI proposal groups once before their memories and group become confirmed memory.
- Browse clickable tags and group existing memories from the menu.
- Introduce project agents, confirm their understanding, and hand off a first assignment with selected context. See [agent onboarding](docs/agents.md). AI execution remains in your chosen external client.
- Let an explicitly authorized AI list assignments, retrieve a scoped briefing and return an exact reviewed handoff for the next run. Local leases track interrupted work; Alve does not start models or schedule AI runs.
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

- Mobile operating systems and native phone integration. Desktop uses Tauri 2 and Rust.
- Concrete synchronization protocol, cryptographic libraries, and key recovery format.
- Retention periods for working material, historical revisions, and deletion records.
- Which AI clients and local model runtimes to support first.

The specification is a starting point for implementation, not a security certification or a claim of unique market positioning.

### Organizing existing memories

Choose **Select memories** above the list, or **Menu > Select & organize memories**. Click rows or checkboxes; search and tag filters retain the selection. **Select visible** selects only displayed rows, and **Clear selection** clears the full selection. Review the selected titles before saving.

Choose **Create a new group**, **Add to an existing memory or project** (a `belongs_to` link), or **Create relations** with a chosen direction and type. Existing memory text, tags and AI grants are preserved. Bulk links are owner-only, checked against the endpoint revisions and saved atomically; existing identical links are skipped. Up to 50 sources can be linked at once.
