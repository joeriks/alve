# Native Alve desktop POC

The Tauri application bundles the existing interface and executes storage, validation, graph operations, search, AI proposals, encryption, and bundle reconciliation in Rust. It launches no Python process and uses Tauri IPC for owner operations. Its separate loopback HTTP listener serves only authenticated, scoped AI routes.

## Build and run

Install Node.js, stable Rust, and the [platform prerequisites](https://v2.tauri.app/start/prerequisites/), then run `npm ci` and `npm run dev`. For release: `npm run build -- --no-bundle`. A Windows build produces `target/release/alve.exe`; `cargo build -p alve-core --bin alve-mcp --release --locked` produces the native MCP companion.

The app uses the OS application-data directory by default. For a workspace-local vault on Windows:

```powershell
$env:ALVE_DATA_DIR = Join-Path $PWD 'private-vaults/native'
$env:ALVE_API_PORT = '4785'
npm run dev
```

`start-alve.ps1` runs a compiled executable with a chosen data directory and preferred port. It does not compile on each launch. Its default workspace vault is `private-vaults/native`, separate from the Python POC's vault.

The app opens a locked screen first. Key derivation and owner API operations run on a worker thread. No model, graph layout or background synchronization is required before the locked screen appears. The whole-database snapshot design still limits scaling; this port does not introduce SQLCipher or promise a measured startup time.

## Existing data and backups

The Rust core preserves the `ALVEPOC1` encrypted snapshot and `ALVEBND1` bundle formats, including AES-256-GCM, scrypt parameters, and immutable revision history. Compatibility checks exercise Python reading Rust snapshots, Rust reading Python edits, and backup/restore/merge across installations.

For migration, download an encrypted bundle from the Python POC and restore it in a fresh native vault. Confirm the graph and exact values, and keep the old vault and an independent backup until verification is complete. Restored bundles exclude AI connections and pending proposals; issue fresh scoped connections in the native app. Alternatively, with the source app stopped, copy its `memory.alve` into a fresh native data directory and unlock with the same passphrase. Never overwrite an existing destination vault to migrate. A direct snapshot copy also preserves local connection grants and pending proposals; revoke copied grants and issue new tokens when moving to another device. Prefer a bundle when those local credentials should be excluded.

Both runtimes use `.process-lock` in the data directory to prevent simultaneous writes. A locked vault still holds the process lock while its application is running. Synchronization and independent backups remain separate operations.

## AI clients

Create a connection in the native UI. Open **Connect an AI** for setup instructions and the actual localhost endpoint, especially if the preferred port was unavailable. Configure the native `alve-mcp` executable with `ALVE_URL` and `ALVE_TOKEN`. No vault passphrase is passed to the AI client.

```json
{
  "mcpServers": {
    "alve": {
      "command": "/absolute/path/to/alve-mcp",
      "env": {
        "ALVE_URL": "http://127.0.0.1:4785",
        "ALVE_TOKEN": "YOUR_SCOPED_CONNECTION_TOKEN"
      }
    }
  }
}
```

Windows uses `alve-mcp.exe`. Keep real tokens outside the repository. The bridge exposes the same five tools and exact-preview/user-confirmation handshake described in [AI setup](poc-ai.md). The native HTTP listener rejects owner routes, foreign origins and hosts; it is not a LAN service.

## Validation and remaining scope

`cargo test -p alve-core` covers core failure paths. `cargo build -p alve-core --bins` builds the acceptance driver and MCP bridge. `python -m unittest discover -s tests -v` additionally exercises the Python reference and cross-runtime compatibility when the Rust driver is present. Python is required only for these reference tests, not for native runtime.

Native CI builds Windows x64 and ARM64 executables and runs both Rust and interoperability checks. Android has a separate APK/emulator workflow and shares the Rust core; see [Android preview](android.md). Build artifacts must be signed locally before distribution. Automatic peer discovery/pairing, incremental LAN transport, multiple vaults in one instance, recovery keys, key rotation, and full calendar behavior remain outside this preview.

## Native smoke verification

On Windows ARM64, a compiled release executable was exercised in its actual WebView2 window using synthetic data. Creating a memory with an exact money fact, scoped native MCP search, prepare/confirm/owner approval, encrypted bundle export, HTTP origin and owner-route rejection, and lock/reopen all passed. Python/Rust snapshot and bundle interoperability also passed locally on both x64 and ARM64 executables.

One instrumented ARM64 launch reached the locked screen in about 0.43 seconds. This is an observed local sample with WebView debugging enabled and cached system resources, not a startup guarantee or a mobile-device measurement.
