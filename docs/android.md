# Android preview

Alve shares its Rust memory core and web UI between Windows and Android. Android
keeps the encrypted `memory.alve` snapshot in the app's private data directory.
It does not upload a copy. Automatic Android cloud backup and device-transfer
backup are disabled; use explicit encrypted exports instead.

## Installation and recovery

Install the signed APK on your Android phone. Android may ask you to allow APK
installation from the app used to open the file. Keep the original Android signing
key: later APKs must use the same key to update this installation.

On a fresh phone, restore an encrypted `.alve` bundle through the locked screen's
Menu. If the phone already contains this vault, unlock and import the bundle from
Storage, backup & sync. Import preserves revisions and conflicts; it does not
overwrite the destination. Never uninstall or clear app data to perform an update
unless you have independently verified a backup: those actions remove the local
working vault.

Android uses the system document picker for explicit exports. The phone's Back
button closes an open menu, returns to the previous Alve view, or locks the vault
at the workspace root. The app locks its vault when it moves into the background.
An explicitly opened document picker has a bounded 30-second grace period so a
quick selection can complete; longer selections may require unlocking and retrying.
Save edits before leaving the app; unsaved editor text is not a durable memory.

Windows updater installation is unavailable on Android. Android preview updates
are separately downloaded APKs. No app store account or cloud vault is required.

## AI and device exchange

Use the phone's foreground Wi-Fi exchange to move confirmed memories to the
computer, then connect the computer's AI client to its own scoped Alve connection.
The Windows MCP executable cannot run on Android. A compatible client on the same
phone may use its loopback HTTP API while the vault remains unlocked. Phone backup
and transfer bundles do not carry grants or pending AI proposals.

Read [storage and sync](storage-and-sync.md) for the two-direction exchange and its
limits. This preview has no background sync, automatic device discovery, biometric
unlocking or automatic AI wake-ups.

## Build and checks

Install Java, the Android SDK/NDK and the appropriate Rust Android targets as
described in the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).
Then run:

```text
npm ci
npm run android:init
npm run android:build
```

`scripts/prepare-android.mjs` reapplies the committed native overlays and explicit
backup policy after project generation. Generated Android files are ignored.
The Android Actions workflow builds the ARM64 release APK, builds an x86_64 debug
APK, and exercises the real WebView/native bridge on an emulator with synthetic
memory. The release APK is signed locally; private signing keys are not uploaded
to CI. Emulator checks do not establish behavior on every physical phone or Wi-Fi
hotspot: test export/recovery and two-direction transfer on the actual devices
before depending on them.

For test-script changes, the manual `Android emulator verification` workflow can
reuse the `alve-android-emulator` artifact from an earlier run. It refuses to run
when Rust sources, UI assets, dependencies or native build inputs differ from that
APK's source commit. This avoids recompiling an unchanged app while refining a
device test. Distribution APKs still come from the normal Android build workflow.
