mod http;

use base64::Engine as _;
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

pub type SharedEngine = Arc<Mutex<alve_core::api::Engine>>;

fn error_message(error: alve_core::Error) -> String {
    error.message
}

#[tauri::command]
async fn alve_request(
    window: tauri::WebviewWindow,
    state: State<'_, SharedEngine>,
    method: String,
    path: String,
    body: Option<Value>,
    token: Option<String>,
) -> Result<Value, String> {
    if window.label() != "main" {
        return Err("Local main window required.".into());
    }
    let engine = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut engine = engine
            .lock()
            .map_err(|_| "Local vault is busy.".to_string())?;
        engine
            .request(
                &method,
                &path,
                &body.unwrap_or_else(|| serde_json::json!({})),
                token.as_deref().unwrap_or(""),
            )
            .map_err(error_message)
    })
    .await
    .map_err(|_| "Local vault request failed.".to_string())?
}

#[tauri::command]
async fn service_info() -> Value {
    http::service_info()
}

#[tauri::command]
async fn save_export(
    app: AppHandle,
    content: String,
    base64: bool,
    suggested_name: String,
    token: String,
    state: State<'_, SharedEngine>,
    window: tauri::WebviewWindow,
) -> Result<bool, String> {
    if window.label() != "main" {
        return Err("Local main window required.".into());
    }
    if content.len() > 32 * 1024 * 1024 {
        return Err("Export is too large.".into());
    }
    state
        .lock()
        .map_err(|_| "Local vault is busy.")?
        .vault
        .auth(&token, true, None, None)
        .map_err(error_message)?;
    let bytes = if base64 {
        base64::engine::general_purpose::STANDARD
            .decode(content)
            .map_err(|_| "Invalid export data.")?
    } else {
        content.into_bytes()
    };
    let name = suggested_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect::<String>();
    let path = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name(if name.is_empty() {
                "alve-export"
            } else {
                &name
            })
            .blocking_save_file()
    })
    .await
    .map_err(|_| "Could not select export destination.")?;
    let Some(path) = path else { return Ok(false) };
    let path = path
        .as_path()
        .ok_or("Selected destination is not a local path.")?
        .to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::write(path, bytes).map_err(|_| "Could not save export.".to_string())
    })
    .await
    .map_err(|_| "Could not save export.".to_string())??;
    Ok(true)
}

#[tauri::command]
fn open_reference(app: AppHandle, window: tauri::WebviewWindow, url: String) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Local main window required.".into());
    }
    let parsed = url::Url::parse(&url).map_err(|_| "Invalid reference URL.")?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("References must use HTTP or HTTPS.".into());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| "Could not open reference.".into())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = std::env::var_os("ALVE_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    app.path()
                        .app_data_dir()
                        .expect("application data directory")
                });
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let engine =
                alve_core::api::Engine::new(dir.join("memory.alve")).map_err(|e| e.message)?;
            let shared = Arc::new(Mutex::new(engine));
            app.manage(shared.clone());
            tauri::async_runtime::spawn(http::start(shared));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            alve_request,
            service_info,
            save_export,
            open_reference
        ])
        .run(tauri::generate_context!())
        .expect("error while running Alve");
}
