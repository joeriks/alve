//! Explicit, short-lived transfer of existing encrypted bundles on a private LAN.
//! This is not persistent pairing or authenticated TLS; see storage-and-sync.md.
use crate::{updater::UpdateState, SharedEngine};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State as HttpState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rand::RngCore;
use serde_json::{json, Value};
use std::{
    io::Read,
    net::Ipv4Addr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use tauri::{State, WebviewWindow};
use tokio::sync::oneshot;

const LIFETIME: Duration = Duration::from_secs(300);
const MAX_RESPONSE: usize = 24 * 1024 * 1024;

struct Offer {
    code: String,
    link: String,
    bundle: Option<String>,
    expires: Instant,
    fetched: bool,
    reported: bool,
    shutdown: Option<oneshot::Sender<()>>,
}

#[derive(Clone, Default)]
pub struct SyncState {
    offer: Arc<Mutex<Option<Offer>>>,
    generation: Arc<AtomicU64>,
}

impl SyncState {
    pub fn cancel_all(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut slot) = self.offer.lock() {
            if let Some(mut offer) = slot.take() {
                if let Some(stop) = offer.shutdown.take() {
                    let _ = stop.send(());
                }
            }
        }
    }
    fn status(&self) -> Value {
        let Ok(slot) = self.offer.lock() else {
            return json!({"phase":"unavailable"});
        };
        match slot.as_ref() {
            Some(offer) if offer.expires > Instant::now() => json!({
                "phase":if offer.fetched {"downloaded"} else {"offering"},
                "link":if offer.fetched {None} else {Some(&offer.link)},
                "expiresInSeconds":offer.expires.saturating_duration_since(Instant::now()).as_secs(),
                "peerReportedMerged":offer.reported,
            }),
            Some(_) => json!({"phase":"expired","peerReportedMerged":false}),
            None => json!({"phase":"idle","peerReportedMerged":false}),
        }
    }
}

#[derive(Clone)]
struct Transport {
    state: SyncState,
    engine: SharedEngine,
    owner: String,
    host: String,
    generation: u64,
}

fn require_window(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Local main window required.".into());
    }
    #[cfg(desktop)]
    if !window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(true) {
        return Err("Keep Alve visible during the transfer.".into());
    }
    #[cfg(target_os = "android")]
    if !crate::mobile::is_foreground() {
        return Err("Keep Alve in the foreground during the transfer.".into());
    }
    Ok(())
}

fn require_owner(engine: &SharedEngine, token: &str) -> Result<(), String> {
    engine
        .lock()
        .map_err(|_| "Local vault is busy.")?
        .vault
        .auth(token, true, None, None)
        .map(|_| ())
        .map_err(|e| e.message)
}

fn parse_link(link: &str) -> Result<(url::Url, String), String> {
    let mut url = url::Url::parse(link).map_err(|_| "Invalid transfer link.")?;
    let private = matches!(url.host(), Some(url::Host::Ipv4(ip)) if ip.is_private());
    if url.scheme() != "http"
        || !private
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_none()
        || url.path() != "/bundle"
        || url.query().is_some()
    {
        return Err("Use an Alve transfer link with a private Wi-Fi IPv4 address.".into());
    }
    let code = url.fragment().unwrap_or("").to_owned();
    if code.len() != 64 || !code.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("The transfer link has an invalid retrieval code.".into());
    }
    url.set_fragment(None);
    Ok((url, code))
}

fn validate_transport(transport: &Transport, headers: &HeaderMap) -> bool {
    if headers.get_all("host").iter().count() != 1
        || headers.get_all("authorization").iter().count() != 1
        || headers.contains_key("origin")
        || headers.contains_key("sec-fetch-site")
        || headers.get("x-alve-sync").and_then(|h| h.to_str().ok()) != Some("1")
    {
        return false;
    }
    if headers.get("host").and_then(|h| h.to_str().ok()) != Some(transport.host.as_str()) {
        return false;
    }
    if transport.generation != transport.state.generation.load(Ordering::SeqCst)
        || require_owner(&transport.engine, &transport.owner).is_err()
    {
        return false;
    }
    let supplied = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    let Ok(slot) = transport.state.offer.lock() else {
        return false;
    };
    slot.as_ref().is_some_and(|offer| {
        offer.expires > Instant::now() && supplied.as_bytes().ct_eq(offer.code.as_bytes()).into()
    })
}

fn rejected() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error":"Transfer rejected."})),
    )
        .into_response()
}

async fn bundle(HttpState(transport): HttpState<Transport>, headers: HeaderMap) -> Response {
    if !validate_transport(&transport, &headers) {
        return rejected();
    }
    let Ok(mut slot) = transport.state.offer.lock() else {
        return rejected();
    };
    let Some(offer) = slot.as_mut() else {
        return rejected();
    };
    if transport.generation != transport.state.generation.load(Ordering::SeqCst)
        || offer.fetched
        || offer.expires <= Instant::now()
    {
        return rejected();
    }
    let Some(data) = offer.bundle.take() else {
        return rejected();
    };
    offer.fetched = true;
    (
        [("cache-control", "no-store")],
        Json(json!({"bundle":data})),
    )
        .into_response()
}

async fn receipt(
    HttpState(transport): HttpState<Transport>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if body.len() > 128 || !validate_transport(&transport, &headers) {
        return rejected();
    }
    let Ok(data) = serde_json::from_slice::<Value>(&body) else {
        return rejected();
    };
    if data != json!({"merged":true}) {
        return rejected();
    }
    let Ok(mut slot) = transport.state.offer.lock() else {
        return rejected();
    };
    let Some(offer) = slot.as_mut() else {
        return rejected();
    };
    if transport.generation != transport.state.generation.load(Ordering::SeqCst)
        || !offer.fetched
        || offer.expires <= Instant::now()
    {
        return rejected();
    }
    offer.reported = true;
    Json(json!({"received":true})).into_response()
}

#[tauri::command]
pub async fn sync_start(
    window: WebviewWindow,
    state: State<'_, SyncState>,
    engine: State<'_, SharedEngine>,
    updates: State<'_, UpdateState>,
    address: String,
    token: String,
) -> Result<Value, String> {
    require_window(&window)?;
    if updates.installing.load(Ordering::SeqCst) {
        return Err("An update is installing.".into());
    }
    let ip = address
        .parse::<Ipv4Addr>()
        .map_err(|_| "Enter this device's private Wi-Fi IPv4 address.")?;
    if !ip.is_private() {
        return Err("Only private Wi-Fi IPv4 addresses can offer a transfer.".into());
    }
    let engine = engine.inner().clone();
    let source = engine.clone();
    let owner = token.clone();
    let encrypted = tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let e = source.lock().map_err(|_| "Local vault is busy.")?;
        e.vault
            .auth(&owner, true, None, None)
            .map_err(|e| e.message)?;
        e.vault.bundle().map_err(|e| e.message)?["bundle"]
            .as_str()
            .map(str::to_owned)
            .ok_or("Could not create the encrypted transfer.".into())
    })
    .await
    .map_err(|_| "Could not prepare the transfer.")??;
    let listener = tokio::net::TcpListener::bind((ip, 0))
        .await
        .map_err(|_| "Cannot listen on that address. Check this device's Wi-Fi IPv4 address.")?;
    let host = listener
        .local_addr()
        .map_err(|_| "Transfer address unavailable.")?
        .to_string();
    let state = state.inner().clone();
    state.cancel_all();
    let generation = state.generation.load(Ordering::SeqCst);
    let mut random = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut random);
    let code = random
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let link = format!("http://{host}/bundle#{code}");
    let (stop, stopped) = oneshot::channel();
    {
        let mut slot = state.offer.lock().map_err(|_| "Transfer state is busy.")?;
        *slot = Some(Offer {
            code,
            link,
            bundle: Some(encrypted),
            expires: Instant::now() + LIFETIME,
            fetched: false,
            reported: false,
            shutdown: Some(stop),
        });
    }
    let transport = Transport {
        state: state.clone(),
        engine,
        owner: token,
        host,
        generation,
    };
    let monitor = transport.clone();
    let app = Router::new()
        .route("/bundle", get(bundle))
        .route("/receipt", post(receipt))
        .layer(DefaultBodyLimit::max(128))
        .with_state(transport);
    tauri::async_runtime::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = stopped.await;
            })
            .await;
    });
    tauri::async_runtime::spawn(async move {
        let until = Instant::now() + LIFETIME;
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if generation != monitor.state.generation.load(Ordering::SeqCst) {
                break;
            }
            if Instant::now() >= until || require_owner(&monitor.engine, &monitor.owner).is_err() {
                monitor.state.cancel_all();
                break;
            }
        }
    });
    Ok(state.status())
}

#[tauri::command]
pub async fn sync_status(
    window: WebviewWindow,
    state: State<'_, SyncState>,
    engine: State<'_, SharedEngine>,
    token: String,
) -> Result<Value, String> {
    require_window(&window)?;
    require_owner(engine.inner(), &token)?;
    Ok(state.status())
}

#[tauri::command]
pub async fn sync_stop(window: WebviewWindow, state: State<'_, SyncState>) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Local main window required.".into());
    }
    state.cancel_all();
    Ok(())
}

#[tauri::command]
pub async fn sync_pull(
    window: WebviewWindow,
    state: State<'_, SyncState>,
    engine: State<'_, SharedEngine>,
    updates: State<'_, UpdateState>,
    link: String,
    password: String,
    token: String,
) -> Result<Value, String> {
    require_window(&window)?;
    require_owner(engine.inner(), &token)?;
    let (url, code) = parse_link(&link)?;
    let state = state.inner().clone();
    let generation = state.generation.load(Ordering::SeqCst);
    let engine = engine.inner().clone();
    let installing = updates.installing.clone();
    tauri::async_runtime::spawn_blocking(move|| -> Result<Value,String> {
        let client=reqwest::blocking::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(20)).build().map_err(|_|"Could not initialize the transfer client.")?;
        let response=client.get(url.clone()).header("X-Alve-Sync","1").bearer_auth(&code).send().map_err(|_|"Cannot reach the offering device. Keep both apps open and check Wi-Fi and firewall settings.")?;
        if response.status()!=reqwest::StatusCode::OK {return Err("Transfer rejected or already used. Create a new link.".into());}
        if response.content_length().is_some_and(|size|size>MAX_RESPONSE as u64) {return Err("Transfer is too large.".into());}
        let mut data=Vec::new();response.take(MAX_RESPONSE as u64+1).read_to_end(&mut data).map_err(|_|"Could not read the transfer.")?;
        if data.len()>MAX_RESPONSE {return Err("Transfer is too large.".into());}
        let data:Value=serde_json::from_slice(&data).map_err(|_|"Invalid transfer response.")?;
        let encrypted=data.get("bundle").and_then(Value::as_str).ok_or("Invalid encrypted bundle.")?;
        let summary={
            let mut e=engine.lock().map_err(|_|"Local vault is busy.")?;
            e.vault.auth(&token,true,None,None).map_err(|e|e.message)?;
            if generation!=state.generation.load(Ordering::SeqCst) || installing.load(Ordering::SeqCst) {return Err("Transfer canceled before import.".into());}
            #[cfg(target_os="android")]
            if !crate::mobile::is_foreground() {return Err("Transfer canceled while Alve was in the background.".into());}
            e.request("POST","/api/import",&json!({"bundle":encrypted,"password":password}),&token).map_err(|e|e.message)?
        };
        let mut receipt_url=url;receipt_url.set_path("/receipt");
        let receipt=client.post(receipt_url).header("X-Alve-Sync","1").header("Content-Type","application/json").bearer_auth(code).body("{\"merged\":true}").send().map(|r|r.status()==reqwest::StatusCode::OK).unwrap_or(false);
        Ok(json!({"merged":true,"receiptSent":receipt,"summary":summary}))
    }).await.map_err(|_|"Transfer operation failed.")?
}

#[cfg(test)]
mod tests {
    use super::*;
    use alve_core::api::Engine;
    use tempfile::TempDir;

    #[test]
    fn links_reject_public_hosts_credentials_ambiguous_paths_and_missing_codes() {
        let code = "a".repeat(64);
        for host in [
            "127.0.0.1",
            "169.254.1.1",
            "8.8.8.8",
            "example.com",
            "[::1]",
        ] {
            assert!(parse_link(&format!("http://{host}:4788/bundle#{code}")).is_err());
        }
        for link in [
            format!("https://192.168.1.2:4788/bundle#{code}"),
            format!("http://user@192.168.1.2:4788/bundle#{code}"),
            format!("http://192.168.1.2:4788/bundle?code=x#{code}"),
            format!("http://192.168.1.2/bundle#{code}"),
            "http://192.168.1.2:4788/bundle#short".into(),
        ] {
            assert!(parse_link(&link).is_err());
        }
        let (url, parsed) = parse_link(&format!("http://192.168.1.2:4788/bundle#{code}")).unwrap();
        assert_eq!(parsed, code);
        assert_eq!(url.fragment(), None);
    }

    #[test]
    fn encrypted_offer_is_single_use_requires_owner_and_receipt_and_stops_on_cancel() {
        let dir = TempDir::new().unwrap();
        let mut engine = Engine::new(dir.path().join("vault.alve")).unwrap();
        let token = engine
            .vault
            .unlock("Synthetic network transfer passphrase", true)
            .unwrap()["token"]
            .as_str()
            .unwrap()
            .to_owned();
        let encrypted = engine.vault.bundle().unwrap()["bundle"]
            .as_str()
            .unwrap()
            .to_owned();
        let state = SyncState::default();
        let code = "a".repeat(64);
        *state.offer.lock().unwrap() = Some(Offer {
            code: code.clone(),
            link: String::new(),
            bundle: Some(encrypted),
            expires: Instant::now() + LIFETIME,
            fetched: false,
            reported: false,
            shutdown: None,
        });
        let transport = Transport {
            state: state.clone(),
            engine: Arc::new(Mutex::new(engine)),
            owner: token,
            host: "192.168.1.2:4788".into(),
            generation: 0,
        };
        let mut headers = HeaderMap::new();
        headers.insert("host", "192.168.1.2:4788".parse().unwrap());
        headers.insert("x-alve-sync", "1".parse().unwrap());
        headers.insert("authorization", format!("Bearer {code}").parse().unwrap());
        assert!(validate_transport(&transport, &headers));
        let mut foreign = headers.clone();
        foreign.insert("origin", "http://192.168.1.2:4788".parse().unwrap());
        assert!(!validate_transport(&transport, &foreign));
        foreign = headers.clone();
        foreign.insert(
            "authorization",
            format!("Bearer {}", "b".repeat(64)).parse().unwrap(),
        );
        assert!(!validate_transport(&transport, &foreign));
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            assert_eq!(
                receipt(
                    HttpState(transport.clone()),
                    headers.clone(),
                    Bytes::from_static(b"{\"merged\":true}")
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );
            let response = bundle(HttpState(transport.clone()), headers.clone()).await;
            assert_eq!(response.status(), StatusCode::OK);
            let data = axum::body::to_bytes(response.into_body(), MAX_RESPONSE)
                .await
                .unwrap();
            let payload: Value = serde_json::from_slice(&data).unwrap();
            assert!(payload["bundle"].as_str().is_some());
            assert!(payload.get("token").is_none());
            assert_eq!(
                bundle(HttpState(transport.clone()), headers.clone())
                    .await
                    .status(),
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                receipt(
                    HttpState(transport.clone()),
                    headers.clone(),
                    Bytes::from_static(b"{\"merged\":true}")
                )
                .await
                .status(),
                StatusCode::OK
            );
        });
        assert_eq!(state.status()["peerReportedMerged"], true);
        transport.engine.lock().unwrap().vault.lock();
        assert!(!validate_transport(&transport, &headers));
        state.cancel_all();
        assert_eq!(state.status()["phase"], "idle");
        assert!(!validate_transport(&transport, &headers));
    }
}
