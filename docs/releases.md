# Preparing a Windows release

CI builds the Windows installers but leaves the uploadable release files unsigned. Sign them locally with the release key; do not upload the private key or its password file.

From the repository root, run:

```powershell
.\scripts\prepare-release.ps1 `
  -Version 0.3.0 `
  -X64Installer path\to\x64\installer.exe `
  -Arm64Installer path\to\arm64\installer.exe `
  -OutputDirectory .\release\0.3.0
```

The script checks that `-Version` matches `src-tauri/tauri.conf.json`, renames the installers, signs each one with the local release key, and produces `latest.json` and `SHA256SUMS`. It checks the generated signatures are base64 and that their trusted comments bind them to the supplied version. Upload the two renamed installers, their `.sig` files, `latest.json`, and `SHA256SUMS` to the GitHub release tagged `v<version>`. The `latest.json` URLs already point to that release. Review the generated release notes in `latest.json` before upload.

The signing key and password are local files under `private-vaults/release-signing/`. Keep an offline backup of both. They are required for every future updater release; losing either one prevents signing a compatible update. Do not place either file in CI, a GitHub release, an issue, or a repository commit.

Users check manually through **Help > Check for updates**. Updates are offline by default because Alve does not contact an update service until the user asks. The updater verifies the signature and requires the manifest version to match the signed version. It locks the vault before handing control to the Windows installer; open editors block installation until their work is resolved.

The installer is not Authenticode-signed. Windows SmartScreen may therefore show a warning even when the updater signature is valid. The updater signature verifies the downloaded update but does not replace Windows code-signing reputation checks.
