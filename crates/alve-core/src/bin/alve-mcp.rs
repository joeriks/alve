//! Local stdio MCP bridge. No Python interpreter or vault passphrase required.
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Read, Write},
    time::Duration,
};

fn api(path: &str, body: Option<&Value>) -> std::result::Result<Value, String> {
    let base = std::env::var("ALVE_URL").unwrap_or_else(|_| "http://127.0.0.1:4765".into());
    let parsed = url::Url::parse(base.trim_end_matches('/')).map_err(|_| "Invalid ALVE_URL.")?;
    if parsed.scheme() != "http"
        || !matches!(parsed.host_str(), Some("127.0.0.1" | "localhost"))
        || parsed.port().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("ALVE_URL must be an explicit localhost HTTP URL with a port.".into());
    }
    let token =
        std::env::var("ALVE_TOKEN").map_err(|_| "Set ALVE_TOKEN to an Alve connection token.")?;
    if token.is_empty() {
        return Err("Set ALVE_TOKEN to an Alve connection token.".into());
    }
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .no_proxy()
        .build()
        .map_err(|_| "Could not create local client.")?;
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let response = match body {
        Some(data) => client.post(url).json(data),
        None => client.get(url),
    }
    .bearer_auth(token)
    .send()
    .map_err(|_| "Could not reach the local Alve app.")?;
    let status = response.status();
    let value: Value = response.json().map_err(|_| "Invalid local response.")?;
    if !status.is_success() {
        return Err(format!(
            "Alve API {}: {}",
            status.as_u16(),
            value["error"].as_str().unwrap_or("Request denied.")
        ));
    }
    Ok(value)
}
fn call(name: &str, args: &Value) -> std::result::Result<Value, String> {
    match name {
        "search_memory" => {
            let a = args.as_object().ok_or("Arguments must be an object.")?;
            validate_search_arguments(a)?;
            let mut query = url::form_urlencoded::Serializer::new(String::new());
            for (k, v) in a {
                if k == "tags" {
                    for tag in v.as_array().ok_or("Tags must be an array.")? {
                        query.append_pair("tag", tag.as_str().ok_or("Tags must be strings.")?);
                    }
                } else {
                    let key = if k == "query" { "q" } else { k };
                    let value = match v {
                        Value::String(s) => s.clone(),
                        Value::Bool(b) => b.to_string(),
                        Value::Number(n) => n.to_string(),
                        _ => return Err("Invalid search filter.".into()),
                    };
                    query.append_pair(key, &value);
                }
            }
            api(&format!("/api/ai/search?{}", query.finish()), None)
        }
        "read_node" | "get_relations" => {
            let id = args["node_id"]
                .as_str()
                .filter(|s| {
                    !s.is_empty()
                        && s.len() <= 100
                        && s.bytes()
                            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                })
                .ok_or("Invalid node_id.")?;
            api(
                &format!(
                    "/api/ai/nodes/{id}{}",
                    if name == "get_relations" {
                        "/relations"
                    } else {
                        ""
                    }
                ),
                None,
            )
        }
        "prepare_memory" => api("/api/ai/proposals/prepare", Some(args)),
        "prepare_memory_batch" => api("/api/ai/proposals/prepare-batch", Some(args)),
        "propose_memory" => api("/api/ai/proposals", Some(args)),
        "propose_memory_batch" => api("/api/ai/proposals/submit-batch", Some(args)),
        _ => Err("Unknown tool.".into()),
    }
}

fn validate_search_arguments(
    args: &serde_json::Map<String, Value>,
) -> std::result::Result<(), String> {
    const KEYS: &[&str] = &[
        "query",
        "tags",
        "type",
        "kind",
        "updatedSince",
        "updatedBefore",
        "includeArchived",
        "limit",
        "offset",
        "sort",
    ];
    if args.keys().any(|key| !KEYS.contains(&key.as_str())) {
        return Err("Unknown search argument.".into());
    }
    if let Some(value) = args.get("query") {
        if value
            .as_str()
            .filter(|text| text.chars().count() <= 1000)
            .is_none()
        {
            return Err("query must be a string of at most 1000 characters.".into());
        }
    }
    if let Some(value) = args.get("tags") {
        let tags = value.as_array().ok_or("tags must be an array.")?;
        if tags.len() > 20
            || tags.iter().any(|tag| {
                tag.as_str()
                    .filter(|text| !text.is_empty() && text.chars().count() <= 60)
                    .is_none()
            })
        {
            return Err(
                "tags must contain at most 20 nonempty strings of at most 60 characters.".into(),
            );
        }
    }
    for (key, allowed) in [
        (
            "type",
            &["memory", "project", "person", "event", "document"][..],
        ),
        (
            "kind",
            &["decision", "preference", "insight", "commitment", "record"][..],
        ),
        ("sort", &["relevance", "updated"][..]),
    ] {
        if let Some(value) = args.get(key) {
            if value
                .as_str()
                .filter(|text| allowed.contains(text))
                .is_none()
            {
                return Err(format!("Invalid search {key}."));
            }
        }
    }
    for key in ["updatedSince", "updatedBefore"] {
        if let Some(value) = args.get(key) {
            if value.as_str().is_none() {
                return Err(format!("{key} must be an ISO 8601 timestamp."));
            }
        }
    }
    if let Some(value) = args.get("includeArchived") {
        if !value.is_boolean() {
            return Err("includeArchived must be a boolean.".into());
        }
    }
    for (key, min, max) in [("limit", 1, 100), ("offset", 0, 5000)] {
        if let Some(value) = args.get(key) {
            if value
                .as_i64()
                .filter(|number| *number >= min && *number <= max)
                .is_none()
            {
                return Err(format!("{key} must be an integer between {min} and {max}."));
            }
        }
    }
    Ok(())
}
fn handle(m: Value) -> Option<Value> {
    if m.get("id").is_none() {
        return None;
    }
    let id = m["id"].clone();
    if m["jsonrpc"] != "2.0" || !m.is_object() {
        return Some(
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"Invalid JSON-RPC request"}}),
        );
    }
    let result=match m["method"].as_str().unwrap_or("") {
        "initialize"=>Ok(json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{},"resources":{}},"serverInfo":{"name":"alve-local-memory","version":"0.2.0"},"instructions":"Read alve://usage. Prepare memory, show the exact preview to the user, request explicit confirmation, then propose. Owner approval in Alve is required."})),
        "ping"=>Ok(json!({})),
        "tools/list"=>Ok(json!({"tools":serde_json::from_str::<Value>(include_str!("../mcp-tools.json")).unwrap()})),
        "resources/list"=>Ok(json!({"resources":[{"uri":"alve://usage","name":"Alve usage contract","mimeType":"application/json"}]})),
        "resources/read" if m["params"]["uri"]=="alve://usage"=>api("/api/ai/contract",None).map(|v|json!({"contents":[{"uri":"alve://usage","mimeType":"application/json","text":v.to_string()}]})),
        "tools/call"=>{
            let args=m["params"].get("arguments").cloned().unwrap_or(json!({}));
            Ok(match call(m["params"]["name"].as_str().unwrap_or(""),&args){Ok(v)=>json!({"content":[{"type":"text","text":v.to_string()}],"isError":false}),Err(e)=>json!({"content":[{"type":"text","text":e}],"isError":true})})
        },
        _=>return Some(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}}))
    };
    Some(match result {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err(e) => json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":e}}),
    })
}
fn main() {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let mut line = Vec::new();
        match input
            .by_ref()
            .take(256 * 1024 + 1)
            .read_until(b'\n', &mut line)
        {
            Ok(0) | Err(_) => break,
            _ => {}
        }
        if line.len() > 256 * 1024 {
            break;
        }
        let value = match serde_json::from_slice(&line) {
            Ok(v) => handle(v),
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            ),
        };
        if let Some(value) = value {
            if writeln!(output, "{value}")
                .and_then(|_| output.flush())
                .is_err()
            {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_returns_bounded_protocol_contract() {
        let response =
            handle(json!({"jsonrpc":"2.0","id":7,"method":"initialize","params":{}})).unwrap();
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(
            response["result"]["serverInfo"]["name"],
            "alve-local-memory"
        );
    }

    #[test]
    fn tools_list_exposes_exactly_seven_tools() {
        let response =
            handle(json!({"jsonrpc":"2.0","id":8,"method":"tools/list","params":{}})).unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 7);
        assert_eq!(tools[0]["name"], "search_memory");
        assert_eq!(tools[6]["name"], "propose_memory_batch");
    }

    #[test]
    fn malformed_json_rpc_returns_invalid_request() {
        let response = handle(json!({"jsonrpc":"1.0","id":"bad","method":"ping"})).unwrap();
        assert_eq!(response["error"]["code"], -32600);
        assert_eq!(response["id"], "bad");
    }

    #[test]
    fn search_schema_rejects_unknown_and_wrong_scalar_types() {
        let unknown = serde_json::from_value(json!({"unexpected":true})).unwrap();
        assert!(validate_search_arguments(&unknown).is_err());
        let string_boolean = serde_json::from_value(json!({"includeArchived":"true"})).unwrap();
        assert!(validate_search_arguments(&string_boolean).is_err());
        let negative_limit = serde_json::from_value(json!({"limit":-1})).unwrap();
        assert!(validate_search_arguments(&negative_limit).is_err());
        let negative_offset = serde_json::from_value(json!({"offset":-1})).unwrap();
        assert!(validate_search_arguments(&negative_offset).is_err());
    }
}
