use crate::SharedEngine;
use tauri::{Emitter, WebviewWindow};

#[cfg(target_os = "android")]
mod lifecycle {
    use std::sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        OnceLock,
    };
    use tauri::{AppHandle, Manager};
    static APP: OnceLock<AppHandle> = OnceLock::new();
    static FOREGROUND: AtomicBool = AtomicBool::new(true);
    static STOP_GENERATION: AtomicU64 = AtomicU64::new(0);

    pub fn register(app: &AppHandle) {
        let _ = APP.set(app.clone());
    }
    pub fn foreground() -> bool {
        FOREGROUND.load(Ordering::SeqCst)
    }
    fn lock() {
        if let Some(app) = APP.get() {
            app.state::<crate::sync::SyncState>().cancel_all();
            if let Some(window) = app.get_webview_window("main") {
                super::lock_on_background(&window, app.state::<crate::SharedEngine>().inner());
            }
        }
    }
    #[no_mangle]
    pub extern "system" fn Java_com_alve_local_MainActivity_alveOnResume(
        _env: jni::JNIEnv,
        _activity: jni::objects::JObject,
    ) {
        STOP_GENERATION.fetch_add(1, Ordering::SeqCst);
        FOREGROUND.store(true, Ordering::SeqCst);
    }
    #[no_mangle]
    pub extern "system" fn Java_com_alve_local_MainActivity_alveOnStop(
        _env: jni::JNIEnv,
        _activity: jni::objects::JObject,
        picker: jni::sys::jboolean,
    ) {
        FOREGROUND.store(false, Ordering::SeqCst);
        let generation = STOP_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(app) = APP.get() {
            app.state::<crate::sync::SyncState>().cancel_all();
        }
        if picker == 0 {
            lock();
            return;
        }
        // A document picker is an explicitly requested foreground operation.
        // Its grace period is bounded even if the user leaves that picker.
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            if STOP_GENERATION.load(Ordering::SeqCst) == generation && !foreground() {
                lock();
            }
        });
    }
}
#[cfg(target_os = "android")]
pub use lifecycle::{foreground as is_foreground, register};

/// Called when the Android activity loses focus. Taking the existing engine
/// mutex serializes this lock behind any in-flight mutation.
pub fn lock_on_background(window: &WebviewWindow, engine: &SharedEngine) {
    let Ok(mut engine) = engine.lock() else {
        return;
    };
    engine.lock_for_update();
    let _ = window.emit("alve-mobile-background-lock", ());
    let _ = window.emit("alve-vault-locked", ());
}

#[cfg(target_os = "android")]
pub fn write_saf_uri(window: &WebviewWindow, uri: &url::Url, bytes: Vec<u8>) -> Result<(), String> {
    use jni::objects::{JObject, JValue};
    use std::sync::mpsc;
    let uri = uri.as_str().to_owned();
    let (sent, received) = mpsc::channel();
    window
        .with_webview(move |webview| {
            webview.jni_handle().exec(move |env, _, webview| {
                let result = (|| -> jni::errors::Result<()> {
                    let context = env
                        .call_method(webview, "getContext", "()Landroid/content/Context;", &[])?
                        .l()?;
                    let resolver = env
                        .call_method(
                            context,
                            "getContentResolver",
                            "()Landroid/content/ContentResolver;",
                            &[],
                        )?
                        .l()?;
                    let text = env.new_string(uri)?;
                    let parsed = env
                        .call_static_method(
                            "android/net/Uri",
                            "parse",
                            "(Ljava/lang/String;)Landroid/net/Uri;",
                            &[JValue::Object(&JObject::from(text))],
                        )?
                        .l()?;
                    let stream = env
                        .call_method(
                            resolver,
                            "openOutputStream",
                            "(Landroid/net/Uri;)Ljava/io/OutputStream;",
                            &[JValue::Object(&parsed)],
                        )?
                        .l()?;
                    let array = env.byte_array_from_slice(&bytes)?;
                    env.call_method(
                        &stream,
                        "write",
                        "([B)V",
                        &[JValue::Object(&JObject::from(array))],
                    )?;
                    env.call_method(&stream, "close", "()V", &[])?;
                    Ok(())
                })();
                let _ = sent.send(
                    result
                        .map_err(|_| "Could not write the selected Android document.".to_string()),
                );
            });
        })
        .map_err(|_| "Could not access the Android webview.".to_string())?;
    received
        .recv_timeout(std::time::Duration::from_secs(15))
        .map_err(|_| "Android document writer did not respond.".to_string())?
}
