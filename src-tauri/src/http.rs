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
const REJECTED: &str = "Request rejected.";

/// Validates the browser/loopback boundary before a request reaches the core.
/// An absent Origin is permitted for local CLI clients carrying a bearer token;
/// any supplied Origin must be one exact local origin.
fn validate_boundary(
    headers: &HeaderMap,
    method: &Method,
    peer: SocketAddr,
    port: u16,
) -> Result<(), StatusCode> {
    if peer.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST)
        || headers.get_all("host").iter().count() != 1
        || headers.get_all("authorization").iter().count() != 1
        || headers.get_all("origin").iter().count() > 1
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::FORBIDDEN)?;
    if host != format!("127.0.0.1:{port}") && host != format!("localhost:{port}") {
        return Err(StatusCode::FORBIDDEN);
    }
    if let Some(origin) = headers.get("origin") {
        let origin = origin.to_str().map_err(|_| StatusCode::FORBIDDEN)?;
        if origin != format!("http://127.0.0.1:{port}")
            && origin != format!("http://localhost:{port}")
        {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    if matches!(
        headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()),
        Some("cross-site") | Some("same-site")
    ) {
        return Err(StatusCode::FORBIDDEN);
    }
    if *method == Method::POST
        && headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or("").trim())
            != Some("application/json")
    {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }
    Ok(())
}
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
    let port = match SERVICE.get() {
        Some(address) => address.port(),
        None => return response(StatusCode::SERVICE_UNAVAILABLE, REJECTED),
    };
    if body.len() > 23 * 1024 * 1024 {
        return response(StatusCode::PAYLOAD_TOO_LARGE, REJECTED);
    }
    if let Err(status) = validate_boundary(&headers, &method, peer, port) {
        return response(
            status,
            if status == StatusCode::UNSUPPORTED_MEDIA_TYPE {
                "Use application/json."
            } else {
                REJECTED
            },
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{header, HeaderValue};

    const PORT: u16 = 4765;
    fn peer() -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, 41000))
    }
    fn local_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:4765"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer test-token"),
        );
        headers
    }
    fn valid(headers: &HeaderMap, method: Method, peer: SocketAddr) -> Result<(), StatusCode> {
        validate_boundary(headers, &method, peer, PORT)
    }

    #[test]
    fn permits_exact_local_cli_request() {
        assert_eq!(valid(&local_headers(), Method::GET, peer()), Ok(()));
    }

    #[test]
    fn rejects_foreign_and_null_origins() {
        for origin in ["https://evil.example", "null"] {
            let mut h = local_headers();
            h.insert(header::ORIGIN, HeaderValue::from_static(origin));
            assert_eq!(valid(&h, Method::GET, peer()), Err(StatusCode::FORBIDDEN));
        }
    }

    #[test]
    fn rejects_duplicate_authorization_and_host() {
        let mut auth = local_headers();
        auth.append(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer second"),
        );
        assert_eq!(
            valid(&auth, Method::GET, peer()),
            Err(StatusCode::FORBIDDEN)
        );
        let mut host = local_headers();
        host.append(header::HOST, HeaderValue::from_static("localhost:4765"));
        assert_eq!(
            valid(&host, Method::GET, peer()),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn rejects_foreign_host_peer_and_cross_site_fetch() {
        let mut host = local_headers();
        host.insert(header::HOST, HeaderValue::from_static("evil.example"));
        assert_eq!(
            valid(&host, Method::GET, peer()),
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            valid(
                &local_headers(),
                Method::GET,
                SocketAddr::from(([10, 0, 0, 8], 9000))
            ),
            Err(StatusCode::FORBIDDEN)
        );
        let mut fetch = local_headers();
        fetch.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
        assert_eq!(
            valid(&fetch, Method::GET, peer()),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn rejects_non_json_post_and_malformed_origin() {
        let h = local_headers();
        assert_eq!(
            valid(&h, Method::POST, peer()),
            Err(StatusCode::UNSUPPORTED_MEDIA_TYPE)
        );
        let mut malformed = local_headers();
        malformed.insert(header::ORIGIN, HeaderValue::from_bytes(b"\xff").unwrap());
        assert_eq!(
            valid(&malformed, Method::GET, peer()),
            Err(StatusCode::FORBIDDEN)
        );
    }
}
