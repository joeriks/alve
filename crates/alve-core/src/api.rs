use crate::{
    quality::QualityGate,
    search::{self, Query},
    vault::Vault,
    Error, Result,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct Engine {
    pub vault: Vault,
    quality: QualityGate,
    last_owner: Instant,
    failed_unlocks: u32,
    unlock_after: Instant,
}
impl Engine {
    pub fn new(path: PathBuf) -> Result<Self> {
        Ok(Self {
            vault: Vault::new(path)?,
            quality: QualityGate::default(),
            last_owner: Instant::now(),
            failed_unlocks: 0,
            unlock_after: Instant::now(),
        })
    }
    pub fn lock_for_update(&mut self) {
        self.vault.lock();
        self.quality.clear();
    }
    pub fn request(
        &mut self,
        method: &str,
        path: &str,
        data: &Value,
        token: &str,
    ) -> Result<Value> {
        if self.last_owner.elapsed() > Duration::from_secs(900) {
            self.vault.lock();
            self.quality.clear();
        }
        if ["POST", "PATCH"].contains(&method) && !data.is_object() {
            return Err(Error::new(400, "Request body must be an object."));
        }
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        if method == "GET" && path == "/api/status" {
            return Ok(self.vault.status());
        }
        if method == "POST" && ["/api/unlock", "/api/restore"].contains(&path) {
            if Instant::now() < self.unlock_after {
                return Err(Error::new(
                    429,
                    "Too many unlock attempts. Wait before retrying.",
                ));
            }
            let password = data["password"]
                .as_str()
                .ok_or_else(|| Error::new(400, "Supply a passphrase."))?;
            let result = if path == "/api/unlock" {
                self.vault.unlock(password, data["create"] == true)
            } else {
                self.vault
                    .restore(data["bundle"].as_str().unwrap_or(""), password)
            };
            match result {
                Ok(v) => {
                    self.quality.clear();
                    self.last_owner = Instant::now();
                    self.failed_unlocks = 0;
                    self.unlock_after = Instant::now();
                    return Ok(v);
                }
                Err(e) => {
                    if e.status == 401 {
                        self.failed_unlocks = self.failed_unlocks.saturating_add(1);
                        self.unlock_after = Instant::now()
                            + Duration::from_secs((self.failed_unlocks as u64 * 2).min(30));
                    }
                    return Err(e);
                }
            }
        }
        if path.starts_with("/api/ai/") {
            return self.ai(method, path, query, data, token);
        }
        self.vault.auth(token, true, None, None)?;
        self.last_owner = Instant::now();
        let parts: Vec<_> = path.trim_matches('/').split('/').collect();
        match (method, path) {
            ("GET", "/api/graph") => self.vault.graph(),
            ("GET", "/api/export") => self.vault.export(),
            ("POST", "/api/lock") => {
                self.vault.lock();
                self.quality.clear();
                Ok(json!({"locked":true}))
            }
            ("POST", "/api/bundle" | "/api/backup" | "/api/sync/export") => self.vault.bundle(),
            ("POST", "/api/import") => self.vault.mutate(|v| {
                v.merge(
                    data["bundle"].as_str().unwrap_or(""),
                    data["password"].as_str().unwrap_or(""),
                )
            }),
            ("POST", "/api/nodes") => self.vault.mutate(|v| v.add_node(data, None, None, "user")),
            ("POST", "/api/relations") => self.vault.mutate(|v| v.add_relation(data)),
            ("POST", "/api/nodes/group") => self.vault.mutate(|v| v.group_nodes(data)),
            ("POST", "/api/connections") => self.vault.mutate(|v| v.grant(data)),
            ("POST", "/api/proposals/review-batch") => self.vault.mutate(|v| v.review_batch(data)),
            _ => {
                if method == "PATCH" && parts.len() == 3 && parts[..2] == ["api", "nodes"] {
                    let heads = self.vault.heads()?;
                    let current = heads
                        .get(parts[2])
                        .filter(|a| a.len() == 1 && a[0]["revisionId"] == data["expectedRevision"])
                        .ok_or_else(|| {
                            Error::new(409, "The node changed or has conflicting versions.")
                        })?;
                    let mut content = current[0].clone();
                    content
                        .as_object_mut()
                        .ok_or_else(|| Error::new(500, "Invalid node."))?
                        .extend(data.as_object().unwrap().clone());
                    let parents = vec![data["expectedRevision"].as_str().unwrap_or("").to_owned()];
                    return self
                        .vault
                        .mutate(|v| v.add_node(&content, Some(parts[2]), Some(&parents), "user"));
                }
                if method == "DELETE" && parts.len() == 3 && parts[..2] == ["api", "connections"] {
                    return self.vault.mutate(|v| v.revoke(parts[2]));
                }
                if method == "POST"
                    && parts.len() == 4
                    && parts[..2] == ["api", "proposals"]
                    && ["approve", "reject"].contains(&parts[3])
                {
                    return self
                        .vault
                        .mutate(|v| v.review(parts[2], parts[3] == "approve"));
                }
                if method == "POST"
                    && parts.len() == 4
                    && parts[..2] == ["api", "conflicts"]
                    && parts[3] == "resolve"
                {
                    let parents: Vec<String> = data["revisionIds"]
                        .as_array()
                        .filter(|a| !a.is_empty())
                        .ok_or_else(|| Error::new(400, "Specify all conflicting revisions."))?
                        .iter()
                        .map(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .ok_or_else(|| Error::new(400, "Invalid revision ID."))
                        })
                        .collect::<Result<_>>()?;
                    return self.vault.mutate(|v| {
                        v.add_node(&data["content"], Some(parts[2]), Some(&parents), "user")
                    });
                }
                Err(Error::new(404, "Endpoint not found."))
            }
        }
    }
    fn ai(
        &mut self,
        method: &str,
        path: &str,
        query: &str,
        data: &Value,
        token: &str,
    ) -> Result<Value> {
        let permission = if path == "/api/ai/search" {
            Some("search")
        } else if path.starts_with("/api/ai/nodes/") {
            Some("read")
        } else if path.starts_with("/api/ai/proposals") {
            Some("propose")
        } else {
            None
        };
        let grant = self.vault.auth(token, false, permission, None)?;
        let mut result = if method == "GET" && path == "/api/ai/contract" {
            let mut c: Value = serde_json::from_str(include_str!("contract.json"))?;
            c["version"] = json!("alve-rust-1");
            c
        } else if method == "GET" && path == "/api/ai/search" {
            let mut q = Query::new();
            for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
                q.entry(k.into_owned()).or_default().push(v.into_owned());
            }
            search::search(&self.vault.visible(grant.as_ref())?, &q)?
        } else if method == "GET" && path.starts_with("/api/ai/nodes/") {
            let parts: Vec<_> = path.trim_matches('/').split('/').collect();
            if parts.len() != 4 && !(parts.len() == 5 && parts[4] == "relations") {
                return Err(Error::new(404, "Endpoint not found."));
            }
            let id = parts[3];
            self.vault.auth(token, false, Some("read"), Some(id))?;
            let graph = self.vault.visible(grant.as_ref())?;
            let n = graph["nodes"]
                .as_array()
                .and_then(|a| a.iter().find(|n| n["id"] == id))
                .ok_or_else(|| Error::new(404, "Node not found."))?;
            if parts.len() == 5 {
                json!({"relations":graph["relations"].as_array().unwrap().iter().filter(|r|r["fromId"]==id||r["toId"]==id).collect::<Vec<_>>()})
            } else {
                json!({"node":n,"conflicts":graph["conflicts"].as_array().unwrap().iter().filter(|c|c["nodeId"]==id).collect::<Vec<_>>()})
            }
        } else if method == "POST" && path == "/api/ai/proposals/prepare" {
            if data["action"] == "update" {
                let id = data["nodeId"]
                    .as_str()
                    .ok_or_else(|| Error::new(400, "Supply nodeId."))?;
                self.vault.auth(token, false, Some("propose"), Some(id))?;
                let heads = self.vault.heads()?;
                if !heads
                    .get(id)
                    .is_some_and(|a| a.len() == 1 && a[0]["revisionId"] == data["expectedRevision"])
                {
                    return Err(Error::new(
                        409,
                        "Read the current, non-conflicting revision before preparing an update.",
                    ));
                }
            }
            let actor = grant
                .as_ref()
                .and_then(|g| g["id"].as_str())
                .unwrap_or(token);
            self.quality.prepare(data, actor)?
        } else if method == "POST" && path == "/api/ai/proposals/prepare-batch" {
            let proposals = data["proposals"]
                .as_array()
                .ok_or_else(|| Error::new(422, "Prepare 2 to 50 proposals."))?;
            if !(2..=50).contains(&proposals.len()) {
                return Err(Error::new(422, "Prepare 2 to 50 proposals."));
            }
            let mut update_ids = std::collections::HashSet::new();
            for proposal in proposals {
                let item = proposal
                    .as_object()
                    .ok_or_else(|| Error::new(422, "Invalid batch proposal fields."))?;
                if item.keys().any(|key| {
                    !matches!(
                        key.as_str(),
                        "action" | "nodeId" | "expectedRevision" | "content"
                    )
                }) {
                    return Err(Error::new(422, "Invalid batch proposal fields."));
                }
                if proposal["action"] == "update" {
                    let id = proposal["nodeId"]
                        .as_str()
                        .ok_or_else(|| Error::new(422, "Supply nodeId."))?;
                    if !update_ids.insert(id) {
                        return Err(Error::new(422, "Use a unique node ID for each update."));
                    }
                }
            }
            let heads = self.vault.heads()?;
            for proposal in proposals {
                if proposal["action"] == "update" {
                    let id = proposal["nodeId"]
                        .as_str()
                        .ok_or_else(|| Error::new(400, "Supply nodeId."))?;
                    self.vault.auth(token, false, Some("propose"), Some(id))?;
                    if !heads.get(id).is_some_and(|a| {
                        a.len() == 1 && a[0]["revisionId"] == proposal["expectedRevision"]
                    }) {
                        return Err(Error::new(409, "Read the current, non-conflicting revision before preparing an update."));
                    }
                }
            }
            let actor = grant
                .as_ref()
                .and_then(|g| g["id"].as_str())
                .unwrap_or(token);
            self.quality.prepare_batch(data, actor)?
        } else if method == "POST" && path == "/api/ai/proposals" {
            let actor = grant
                .as_ref()
                .and_then(|g| g["id"].as_str())
                .unwrap_or(token);
            let (payload, confirmation) = self.quality.confirmed(data, actor)?;
            let result = self.vault.mutate(|v| {
                let mut p = v.propose(&payload, grant.as_ref())?;
                let id = p["id"]
                    .as_str()
                    .ok_or_else(|| Error::new(500, "Invalid proposal."))?;
                v.record_quality(id, &confirmation)?;
                p["qualityConfirmation"] = confirmation.clone();
                Ok(p)
            })?;
            self.quality
                .consume(data["reviewToken"].as_str().unwrap_or(""));
            result
        } else if method == "POST" && path == "/api/ai/proposals/submit-batch" {
            let actor = grant
                .as_ref()
                .and_then(|g| g["id"].as_str())
                .unwrap_or(token);
            let (batch, confirmation) = self.quality.confirmed_batch(data, actor)?;
            let result = self
                .vault
                .mutate(|v| v.propose_batch(&batch, grant.as_ref(), &confirmation))?;
            self.quality
                .consume(data["reviewToken"].as_str().unwrap_or(""));
            result
        } else {
            return Err(Error::new(404, "Endpoint not found."));
        };
        result["vaultId"] = self.vault.graph()?["vaultId"].clone();
        result["vaultAlias"] = grant
            .as_ref()
            .and_then(|g| g.get("vaultAlias"))
            .cloned()
            .unwrap_or(Value::Null);
        Ok(result)
    }
}
