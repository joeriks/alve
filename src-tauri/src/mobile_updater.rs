//! Android preview updates are separately installed APKs. Keep the same IPC
//! commands as desktop so the web UI can safely hide the updater controls.
use serde_json::{json, Value};
use std::sync::{atomic::AtomicBool, Arc};
use tauri::{AppHandle, State, WebviewWindow};

#[derive(Default)]
pub struct UpdateState {
    pub installing: Arc<AtomicBool>,
}

#[tauri::command]
pub async fn check_update(
    _app: AppHandle,
    _window: WebviewWindow,
    _state: State<'_, UpdateState>,
) -> Result<Value, String> {
    Ok(
        json!({"currentVersion":env!("CARGO_PKG_VERSION"),"available":false,"managedBy":"android_apk"}),
    )
}
#[tauri::command]
pub async fn install_update(
    _app: AppHandle,
    _window: WebviewWindow,
    _state: State<'_, UpdateState>,
    _engine: State<'_, crate::SharedEngine>,
    _version: String,
) -> Result<(), String> {
    Err("Install the separately downloaded Android APK to update Alve.".into())
}
