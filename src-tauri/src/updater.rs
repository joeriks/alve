use crate::SharedEngine;
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, State, WebviewWindow};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct UpdateState {
    operation: tokio::sync::Mutex<()>,
    pending: Mutex<Option<(Update, Instant)>>,
    pub installing: Arc<AtomicBool>,
}

fn validate_release(version: &str, download_url: &str, arch: &str) -> Result<(), String> {
    let parsed = semver::Version::parse(version).map_err(|_| "Invalid update version.")?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    if parsed <= current || !parsed.pre.is_empty() || !parsed.build.is_empty() {
        return Err("Only newer stable releases can be installed.".into());
    }
    if !matches!(arch, "x86_64" | "aarch64") {
        return Err("Updates are not available for this architecture.".into());
    }
    let expected = format!("https://github.com/joeriks/alve/releases/download/v{version}/Alve_{version}_windows-{arch}-setup.exe");
    if download_url != expected {
        return Err("The update is not an official Alve release asset.".into());
    }
    Ok(())
}

fn require_main(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Local main window required.".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, UpdateState>,
) -> Result<Value, String> {
    require_main(&window)?;
    let _operation = state
        .operation
        .try_lock()
        .map_err(|_| "An update operation is already running.")?;
    *state
        .pending
        .lock()
        .map_err(|_| "Update state is unavailable.")? = None;
    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "Could not initialize the update service.")?;
    let update = updater
        .check()
        .await
        .map_err(|_| "Could not check releases. Check your internet connection and try again.")?;
    let Some(update) = update else {
        return Ok(json!({"currentVersion":env!("CARGO_PKG_VERSION"),"available":false}));
    };
    validate_release(
        &update.version,
        update.download_url.as_str(),
        std::env::consts::ARCH,
    )?;
    let result = json!({"currentVersion":env!("CARGO_PKG_VERSION"),"available":true,"version":update.version,"notes":update.body});
    *state
        .pending
        .lock()
        .map_err(|_| "Update state is unavailable.")? = Some((update, Instant::now()));
    Ok(result)
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, UpdateState>,
    engine: State<'_, SharedEngine>,
    sync: State<'_, crate::sync::SyncState>,
    version: String,
) -> Result<(), String> {
    require_main(&window)?;
    let _operation = state
        .operation
        .try_lock()
        .map_err(|_| "An update operation is already running.")?;
    let (mut update, checked) = state
        .pending
        .lock()
        .map_err(|_| "Update state is unavailable.")?
        .take()
        .ok_or("Check for updates before installing.")?;
    if update.version != version || checked.elapsed() > Duration::from_secs(600) {
        return Err("This update confirmation expired. Check again.".into());
    }
    validate_release(
        &update.version,
        update.download_url.as_str(),
        std::env::consts::ARCH,
    )?;
    update.timeout = Some(Duration::from_secs(300));
    let mut downloaded = 0u64;
    let mut last_progress = Instant::now() - Duration::from_secs(1);
    let _ = app.emit(
        "alve-update-state",
        json!({"phase":"downloading","downloaded":0}),
    );
    // Tauri verifies the artifact signature AND its signed version before returning bytes.
    let bytes = update.download(|length, total| {
        downloaded += length as u64;
        if last_progress.elapsed() >= Duration::from_millis(150) {
            last_progress = Instant::now();
            let _ = app.emit("alve-update-state", json!({"phase":"downloading","downloaded":downloaded,"total":total}));
        }
    }, || {}).await.map_err(|_| "The update could not be downloaded or verified. Nothing was installed; check again to retry.")?;
    // Stop new owner requests, then drain any existing mutation under the shared mutex.
    // Each successful mutation already persisted; locking must precede Windows install's exit.
    sync.cancel_all();
    state.installing.store(true, Ordering::SeqCst);
    let shared = engine.inner().clone();
    let lock_result = tauri::async_runtime::spawn_blocking(move || {
        shared
            .lock()
            .map_err(|_| "Could not safely lock the vault.".to_string())?
            .lock_for_update();
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "Could not safely lock the vault.".to_string())
    .and_then(|r| r);
    if let Err(error) = lock_result {
        state.installing.store(false, Ordering::SeqCst);
        return Err(error);
    }
    let _ = app.emit("alve-update-state", json!({"phase":"locked"}));
    let _ = app.emit("alve-update-state", json!({"phase":"installing"}));
    let result = tauri::async_runtime::spawn_blocking(move || update.install(bytes)).await;
    // On Windows, successful install exits this process and restarts via NSIS.
    state.installing.store(false, Ordering::SeqCst);
    match result {
        Ok(Ok(())) => {
            app.restart();
        }
        _ => Err(
            "Installation failed. The vault remains safely locked; check again to retry.".into(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_exact_newer_official_architecture_assets_are_accepted() {
        let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
        let future = format!("{}.{}.{}", current.major, current.minor, current.patch + 1);
        let url = format!("https://github.com/joeriks/alve/releases/download/v{future}/Alve_{future}_windows-aarch64-setup.exe");
        assert!(validate_release(&future, &url, "aarch64").is_ok());
        for invalid in [
            url.replace("github.com", "evil.example"),
            url.replace("https:", "http:"),
            format!("{url}?token=x"),
            url.replace("aarch64", "x86_64"),
        ] {
            assert!(validate_release(&future, &invalid, "aarch64").is_err());
        }
        assert!(validate_release(env!("CARGO_PKG_VERSION"), &url, "aarch64").is_err());
        assert!(validate_release("0.2.0", &url, "aarch64").is_err());
        assert!(validate_release(&format!("{future}-beta.1"), &url, "aarch64").is_err());
    }
    #[test]
    fn production_configuration_requires_signed_versions_and_secure_transport() {
        let config: Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &config["plugins"]["updater"];
        assert_eq!(updater["requireSignedVersion"], true);
        assert_eq!(updater["allowDowngrades"], false);
        assert_eq!(
            updater["endpoints"],
            json!(["https://github.com/joeriks/alve/releases/latest/download/latest.json"])
        );
        assert!(updater.get("dangerousInsecureTransportProtocol").is_none());
    }
}
