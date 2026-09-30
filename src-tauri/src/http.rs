use crate::SharedEngine;
use axum::{
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use serde_json::{json, Value};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::OnceLock,
};

static SERVICE: OnceLock<SocketAddr> = OnceLock::new();
pub fn service_info() -> Value {
    json!({"httpEndpoint": SERVICE.get().map(|a| format!("http://{a}")), "aiOnly": true})
}

pub async fn start(engine: SharedEngine) {
    let requested = std::env::var("ALVE_API_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4765);
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, requested));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(_) => tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))
            .await
            .expect("localhost listener"),
    };
    let actual = listener.local_addr().expect("listener address");
    let _ = SERVICE.set(actual);
    let app = Router::new()
        .route("/api/ai/{*path}", any(ai))
        .layer(DefaultBodyLimit::max(256 * 1024))
        .with_state(engine);
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("localhost service");
}

async fn ai(
    State(engine): State<SharedEngine>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    method: Method,
    headers: HeaderMap,
    uri: axum::http::Uri,
    body: Bytes,
) -> Response {
    if headers.get_all("host").iter().count() != 1
        || headers.get_all("authorization").iter().count() != 1
        || headers.get_all("origin").iter().count() > 1
    {
        return response(StatusCode::FORBIDDEN, "Request rejected.");
    }
    if method == Method::POST
        && headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or("").trim())
            != Some("application/json")
    {
        return response(StatusCode::UNSUPPORTED_MEDIA_TYPE, "Use application/json.");
    }
    let origin = headers.get("origin").and_then(|v| v.to_str().ok());
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let allowed_host = SERVICE
        .get()
        .map(|a| {
            [
                format!("127.0.0.1:{}", a.port()),
                format!("localhost:{}", a.port()),
            ]
            .contains(&host.to_string())
        })
        .unwrap_or(false);
    let origin_ok = origin
        .map(|o| {
            SERVICE
                .get()
                .map(|a| {
                    o == format!("http://127.0.0.1:{}", a.port())
                        || o == format!("http://localhost:{}", a.port())
                })
                .unwrap_or(false)
        })
        .unwrap_or(true);
    let fetch_ok = !matches!(
        headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()),
        Some("cross-site") | Some("same-site")
    );
    if peer.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST)
        || !allowed_host
        || !origin_ok
        || !fetch_ok
        || body.len() > 23 * 1024 * 1024
    {
        return response(StatusCode::FORBIDDEN, "Request rejected.");
    }
    let token = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let data = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => return response(StatusCode::BAD_REQUEST, "Invalid JSON data."),
        }
    };
    let path = uri
        .path_and_query()
        .map(|x| x.as_str().to_owned())
        .unwrap_or_else(|| "/".to_owned());
    let method = method.as_str().to_owned();
    let token = token.to_owned();
    let result = tokio::task::spawn_blocking(move || {
        engine
            .lock()
            .map_err(|_| alve_core::Error::new(503, "Local vault is busy."))
            .and_then(|mut e| e.request(&method, &path, &data, &token))
    })
    .await;
    match result {
        Ok(Ok(value)) => json_response(StatusCode::OK, value),
        Ok(Err(e)) => json_response(
            StatusCode::from_u16(e.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            json!({"error":e.message,"status":e.status}),
        ),
        Err(_) => response(StatusCode::INTERNAL_SERVER_ERROR, "Local request failed."),
    }
}
fn response(status: StatusCode, message: &str) -> Response {
    json_response(status, json!({"error":message,"status":status.as_u16()}))
}
fn json_response(status: StatusCode, value: Value) -> Response {
    (
        status,
        [
            ("cache-control", "no-store"),
            ("content-type", "application/json"),
        ],
        axum::Json(value),
    )
        .into_response()
}
