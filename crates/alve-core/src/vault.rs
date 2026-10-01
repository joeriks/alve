use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD, Engine};
use fs2::FileExt;
use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroize;

use crate::crypto::{self, BUNDLE, MAX_ENVELOPE, SNAPSHOT};
use crate::validation::{node_content, now, text, RELATIONS};
use crate::{Error, Result};

pub const MAX_REVISIONS: usize = 5000;
pub const MAX_RELATIONS: usize = 5000;

pub struct Vault {
    path: PathBuf,
    _lock: File,
    db: Option<Connection>,
    key: Option<[u8; 32]>,
    salt: Option<[u8; 16]>,
    vault_id: Option<String>,
    admin: Option<String>,
}
impl Vault {
    pub fn new(path: PathBuf) -> Result<Self> {
        let lock_path = path
            .parent()
            .ok_or_else(|| Error::new(400, "Vault path has no parent."))?
            .join(".process-lock");
        std::fs::create_dir_all(lock_path.parent().unwrap())?;
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(lock_path)?;
        lock.try_lock_exclusive()
            .map_err(|_| Error::new(409, "This vault is already open in another instance."))?;
        Ok(Self {
            path,
            _lock: lock,
            db: None,
            key: None,
            salt: None,
            vault_id: None,
            admin: None,
        })
    }
    pub fn status(&self) -> Value {
        json!({"exists":self.path.exists(),"unlocked":self.db.is_some()})
    }
    fn require(&self) -> Result<&Connection> {
        self.db
            .as_ref()
            .ok_or_else(|| Error::new(423, "Unlock the vault first."))
    }
    fn id(&self) -> Result<&str> {
        self.vault_id
            .as_deref()
            .ok_or_else(|| Error::new(423, "Unlock the vault first."))
    }
    pub fn unlock(&mut self, password: &str, create: bool) -> Result<Value> {
        if self.db.is_some() {
            if std::fs::metadata(&self.path)?.len() > MAX_ENVELOPE as u64 {
                return Err(Error::new(413, "POC vault limit is 16 MiB."));
            }
            let raw = std::fs::read(&self.path)?;
            crypto::decrypt(&raw, SNAPSHOT, password)?;
            self.admin = Some(token());
            return Ok(json!({"token":self.admin,"vaultId":self.vault_id}));
        }
        if self.path.exists() {
            if std::fs::metadata(&self.path)?.len() > MAX_ENVELOPE as u64 {
                return Err(Error::new(413, "POC vault limit is 16 MiB."));
            }
            let raw = std::fs::read(&self.path)?;
            let (id, salt, key, bytes) = crypto::decrypt(&raw, SNAPSHOT, password)?;
            let mut db = Connection::open_in_memory()?;
            db.deserialize_read_exact("main", bytes.as_slice(), bytes.len(), false)?;
            db.execute_batch("PRAGMA temp_store=MEMORY;PRAGMA journal_mode=MEMORY;")?;
            if db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))? != "ok" {
                return Err(Error::new(400, "Vault database integrity check failed."));
            }
            let stored: String =
                db.query_row("SELECT value FROM meta WHERE key='vaultId'", [], |r| {
                    r.get(0)
                })?;
            if stored != id {
                return Err(Error::new(400, "Vault identity mismatch."));
            }
            let format: String =
                db.query_row("SELECT value FROM meta WHERE key='format'", [], |r| {
                    r.get(0)
                })?;
            if format != "alve-poc-1" {
                return Err(Error::new(400, "Unsupported vault schema."));
            }
            self.db = Some(db);
            self.vault_id = Some(id);
            self.salt = Some(salt);
            self.key = Some(key);
        } else {
            if !create {
                return Err(Error::new(404, "Create a vault first."));
            }
            if text(
                Some(&Value::String(password.into())),
                "passphrase",
                1024,
                true,
            )?
            .chars()
            .count()
                < 12
            {
                return Err(Error::new(
                    400,
                    "Use a passphrase of at least 12 characters.",
                ));
            }
            let id = Uuid::new_v4().simple().to_string();
            let mut salt = [0; 16];
            rand::rngs::OsRng.fill_bytes(&mut salt);
            let key = crypto::derive(password, &salt)?;
            let db = Connection::open_in_memory()?;
            init(&db, &id)?;
            self.db = Some(db);
            self.vault_id = Some(id);
            self.salt = Some(salt);
            self.key = Some(key);
            if let Err(e) = self.persist() {
                self.lock();
                return Err(e);
            }
        }
        self.admin = Some(token());
        Ok(json!({"token":self.admin,"vaultId":self.vault_id}))
    }
    pub fn lock(&mut self) {
        self.db = None;
        if let Some(mut key) = self.key.take() {
            key.zeroize()
        }
        self.salt = None;
        self.vault_id = None;
        self.admin = None;
    }
    fn persist(&self) -> Result<()> {
        let db = self.require()?;
        let bytes = db.serialize("main")?.to_vec();
        let raw = crypto::envelope(
            SNAPSHOT,
            self.id()?,
            self.salt.as_ref().unwrap(),
            self.key.as_ref().unwrap(),
            &bytes,
        )?;
        crypto::atomic_write(&self.path, &raw)
    }
    pub fn mutate<T, F>(&mut self, action: F) -> Result<T>
    where
        F: FnOnce(&mut Self) -> Result<T>,
    {
        let before = self.require()?.serialize("main")?.to_vec();
        let result = action(self);
        match result {
            Ok(value) => {
                if let Err(error) = self.persist() {
                    if let Err(rollback_error) = self.reload(&before) {
                        self.lock();
                        return Err(rollback_error);
                    }
                    return Err(error);
                }
                Ok(value)
            }
            Err(error) => {
                if let Err(rollback_error) = self.reload(&before) {
                    self.lock();
                    return Err(rollback_error);
                }
                Err(error)
            }
        }
    }
    fn reload(&mut self, bytes: &[u8]) -> Result<()> {
        let mut db = Connection::open_in_memory()?;
        db.deserialize_read_exact("main", bytes, bytes.len(), false)?;
        db.execute_batch("PRAGMA temp_store=MEMORY;PRAGMA journal_mode=MEMORY;")?;
        self.db = Some(db);
        Ok(())
    }
    fn rows(&self, table: &str) -> Result<Vec<Value>> {
        if !["revisions", "relations", "proposals", "connections"].contains(&table) {
            return Err(Error::new(400, "Invalid table."));
        }
        let db = self.require()?;
        let mut stmt = db.prepare(&format!("SELECT payload FROM {table} ORDER BY id"))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .map(|row| serde_json::from_str(&row?).map_err(Into::into))
            .collect();
        rows
    }
    pub fn heads(&self) -> Result<BTreeMap<String, Vec<Value>>> {
        let rows = self.rows("revisions")?;
        let parents: HashSet<String> = rows
            .iter()
            .flat_map(|r| {
                r["parents"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
            })
            .collect();
        let mut out = BTreeMap::new();
        for row in rows {
            if !parents.contains(row["revisionId"].as_str().unwrap_or("")) {
                out.entry(row["id"].as_str().unwrap_or("").into())
                    .or_insert_with(Vec::new)
                    .push(row)
            }
        }
        Ok(out)
    }
    pub fn graph(&self) -> Result<Value> {
        let heads = self.heads()?;
        let conflicts=heads.iter().filter(|(_,v)|v.len()>1).map(|(id,v)|json!({"nodeId":id,"revisionIds":v.iter().map(|x|x["revisionId"].clone()).collect::<Vec<_>>(),"versions":v})).collect::<Vec<_>>();
        let connections = self
            .rows("connections")?
            .into_iter()
            .map(|mut c| {
                c.as_object_mut().map(|x| x.remove("tokenHash"));
                c
            })
            .collect::<Vec<_>>();
        Ok(
            json!({"vaultId":self.id()?,"nodes":heads.values().filter_map(|x|x.first()).collect::<Vec<_>>(),"relations":self.rows("relations")?.into_iter().filter(|r| r["deleted"] != true).collect::<Vec<_>>(),"conflicts":conflicts,"proposals":self.rows("proposals")?,"connections":connections}),
        )
    }
    pub fn auth(
        &self,
        token_value: &str,
        admin: bool,
        permission: Option<&str>,
        node_id: Option<&str>,
    ) -> Result<Option<Value>> {
        self.require()?;
        if token_value.is_empty() {
            return Err(Error::new(401, "Authentication required."));
        }
        if self
            .admin
            .as_deref()
            .map(|x| x.as_bytes().ct_eq(token_value.as_bytes()).into())
            .unwrap_or(false)
        {
            return Ok(None);
        }
        if admin {
            return Err(Error::new(403, "Owner authorization required."));
        }
        let hash = sha(token_value);
        for c in self.rows("connections")? {
            if !c["revoked"].as_bool().unwrap_or(false)
                && c["tokenHash"]
                    .as_str()
                    .map(|x| x.as_bytes().ct_eq(hash.as_bytes()).into())
                    .unwrap_or(false)
            {
                if permission
                    .map(|p| {
                        !c["permissions"]
                            .as_array()
                            .map(|x| x.iter().any(|v| v.as_str() == Some(p)))
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
                {
                    return Err(Error::new(
                        403,
                        "This connection does not have that permission.",
                    ));
                }
                if node_id
                    .map(|id| {
                        !c["nodeIds"]
                            .as_array()
                            .map(|x| x.iter().any(|v| v.as_str() == Some(id)))
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
                {
                    return Err(Error::new(404, "Node is unavailable to this connection."));
                }
                return Ok(Some(c));
            }
        }
        Err(Error::new(401, "Invalid or revoked connection token."))
    }
    pub fn visible(&self, grant: Option<&Value>) -> Result<Value> {
        let mut graph = self.graph()?;
        if let Some(grant) = grant {
            let allowed: HashSet<&str> = grant["nodeIds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            for key in ["nodes", "conflicts"] {
                if let Some(rows) = graph[key].as_array_mut() {
                    rows.retain(|r| {
                        allowed.contains(
                            r[if key == "nodes" { "id" } else { "nodeId" }]
                                .as_str()
                                .unwrap_or(""),
                        )
                    });
                }
            }
            if let Some(rows) = graph["relations"].as_array_mut() {
                rows.retain(|r| {
                    allowed.contains(r["fromId"].as_str().unwrap_or(""))
                        && allowed.contains(r["toId"].as_str().unwrap_or(""))
                });
            }
            graph.as_object_mut().unwrap().remove("proposals");
            graph.as_object_mut().unwrap().remove("connections");
        }
        Ok(graph)
    }
    pub fn add_node(
        &mut self,
        data: &Value,
        node_id: Option<&str>,
        parents: Option<&[String]>,
        origin: &str,
    ) -> Result<Value> {
        let content = node_content(data)?;
        let heads = self.heads()?;
        let (id, parent_list, created) = match node_id {
            None => (Uuid::new_v4().simple().to_string(), Vec::new(), now()),
            Some(id) => {
                let current = heads
                    .get(id)
                    .ok_or_else(|| Error::new(404, "Node not found."))?;
                let expected: BTreeSet<String> = current
                    .iter()
                    .filter_map(|x| x["revisionId"].as_str().map(str::to_owned))
                    .collect();
                let got: BTreeSet<String> = parents.unwrap_or(&[]).iter().cloned().collect();
                if expected != got {
                    return Err(Error::new(
                        409,
                        "The node changed or has a conflict. Review all current versions.",
                    ));
                }
                (
                    id.to_owned(),
                    parents.unwrap_or(&[]).to_vec(),
                    current[0]["createdAt"].as_str().unwrap_or("").to_owned(),
                )
            }
        };
        let node = json!({"id":id,"vaultId":self.id()?,"revisionId":Uuid::new_v4().simple().to_string(),"parents":parent_list,"createdAt":created,"updatedAt":now(),"origin":origin,"verification":"user_confirmed","title":content["title"],"body":content["body"],"type":content["type"],"kind":content["kind"],"tags":content["tags"],"facts":content["facts"],"references":content["references"],"status":content["status"]});
        let db = self.require()?;
        db.execute(
            "INSERT INTO revisions VALUES (?,?,?)",
            params![
                node["revisionId"].as_str(),
                node["id"].as_str(),
                canonical(&node)
            ],
        )?;
        let count: i64 = db.query_row("SELECT count(*) FROM revisions", [], |r| r.get(0))?;
        if count as usize > MAX_REVISIONS {
            return Err(Error::new(413, "POC revision limit reached."));
        }
        Ok(node)
    }
    pub fn add_relation(&mut self, data: &Value) -> Result<Value> {
        let heads = self.heads()?;
        let from = data["fromId"].as_str().unwrap_or("");
        let to = data["toId"].as_str().unwrap_or("");
        if !heads.contains_key(from) || !heads.contains_key(to) {
            return Err(Error::new(400, "Relation endpoints must exist."));
        }
        if from == to {
            return Err(Error::new(400, "A relation cannot link a node to itself."));
        }
        if [from, to].iter().any(|id| {
            let current = &heads[*id];
            current.len() != 1 || current[0]["status"] != "active"
        }) {
            return Err(Error::new(
                409,
                "A node is inactive or has conflicting versions.",
            ));
        }
        let typ = data["type"].as_str().unwrap_or("");
        if !RELATIONS.contains(&typ) {
            return Err(Error::new(400, "Unsupported relation type."));
        }
        if let Some(existing) = self.rows("relations")?.into_iter().find(|r| {
            r["deleted"] != true && r["fromId"] == from && r["toId"] == to && r["type"] == typ
        }) {
            return Ok(existing);
        }
        let count: i64 = self
            .require()?
            .query_row("SELECT count(*) FROM relations", [], |r| r.get(0))?;
        if count as usize >= MAX_RELATIONS {
            return Err(Error::new(413, "POC relation limit reached."));
        }
        let relation = json!({"id":Uuid::new_v4().simple().to_string(),"fromId":from,"toId":to,"type":typ,"vaultId":self.id()?,"origin":"user","verification":"user_confirmed","createdAt":now()});
        self.require()?.execute(
            "INSERT INTO relations VALUES (?,?)",
            params![relation["id"].as_str(), canonical(&relation)],
        )?;
        Ok(relation)
    }
    pub fn delete_relation(&mut self, id: &str) -> Result<Value> {
        let mut relation = self
            .rows("relations")?
            .into_iter()
            .find(|r| r["id"] == id)
            .ok_or_else(|| Error::new(404, "Relation not found."))?;
        if relation["deleted"] != true {
            relation["deleted"] = json!(true);
            relation["deletedAt"] = json!(now());
            self.require()?.execute(
                "UPDATE relations SET payload=? WHERE id=?",
                params![canonical(&relation), id],
            )?;
        }
        Ok(relation)
    }
    pub fn restore_relation(&mut self, id: &str) -> Result<Value> {
        let records = self.rows("relations")?;
        let relation = records
            .iter()
            .find(|r| r["id"] == id)
            .ok_or_else(|| Error::new(404, "Relation not found."))?;
        if relation["deleted"] != true {
            return Err(Error::new(409, "Only a removed relation can be restored."));
        }
        let mut restored = self.add_relation(relation)?;
        if !records.iter().any(|r| r["id"] == restored["id"]) {
            restored["restoredFrom"] = json!(id);
            self.require()?.execute(
                "UPDATE relations SET payload=? WHERE id=?",
                params![canonical(&restored), restored["id"].as_str()],
            )?;
        }
        Ok(restored)
    }
    pub fn add_relations_batch(&mut self, data: &Value) -> Result<Value> {
        let object = data
            .as_object()
            .ok_or_else(|| Error::new(400, "Invalid relation batch."))?;
        if object.len() != 4
            || !["nodeIds", "toId", "type", "expectedRevisions"]
                .iter()
                .all(|key| object.contains_key(*key))
        {
            return Err(Error::new(400, "Invalid relation batch."));
        }
        let ids = data["nodeIds"]
            .as_array()
            .ok_or_else(|| Error::new(400, "Select 1 to 50 unique source nodes."))?;
        if ids.is_empty() || ids.len() > 50 {
            return Err(Error::new(400, "Select 1 to 50 unique source nodes."));
        }
        let sources = ids
            .iter()
            .map(|id| {
                id.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| Error::new(400, "Select 1 to 50 unique source nodes."))
            })
            .collect::<Result<Vec<_>>>()?;
        if sources.iter().collect::<HashSet<_>>().len() != sources.len() {
            return Err(Error::new(400, "Select 1 to 50 unique source nodes."));
        }
        let target = data["toId"]
            .as_str()
            .ok_or_else(|| Error::new(400, "Supply a target node."))?;
        if sources.iter().any(|id| id == target) {
            return Err(Error::new(400, "The target cannot also be a source node."));
        }
        let typ = data["type"]
            .as_str()
            .filter(|typ| RELATIONS.contains(typ))
            .ok_or_else(|| Error::new(400, "Unsupported relation type."))?;
        let expected = data["expectedRevisions"]
            .as_object()
            .ok_or_else(|| Error::new(400, "Supply expected revisions for every endpoint."))?;
        let endpoint_ids = sources
            .iter()
            .cloned()
            .chain(std::iter::once(target.to_owned()))
            .collect::<HashSet<_>>();
        if expected.len() != endpoint_ids.len()
            || endpoint_ids
                .iter()
                .any(|id| !expected.get(id).is_some_and(Value::is_string))
        {
            return Err(Error::new(
                400,
                "Supply expected revisions for every endpoint.",
            ));
        }
        let heads = self.heads()?;
        for id in &endpoint_ids {
            if !heads.contains_key(id) {
                return Err(Error::new(400, "Relation endpoints must exist."));
            }
            let revision = expected.get(id).unwrap();
            let current = heads.get(id).filter(|rows| {
                rows.len() == 1
                    && rows[0].get("status").and_then(Value::as_str) == Some("active")
                    && rows[0].get("revisionId") == Some(revision)
            });
            if current.is_none() {
                return Err(Error::new(
                    409,
                    "A node changed, is inactive, or has conflicting versions.",
                ));
            }
        }
        let existing = self
            .rows("relations")?
            .into_iter()
            .filter(|relation| {
                relation["deleted"] != true && relation["toId"] == target && relation["type"] == typ
            })
            .filter_map(|relation| relation["fromId"].as_str().map(str::to_owned))
            .collect::<HashSet<_>>();
        let additions = sources
            .iter()
            .filter(|source| !existing.contains(*source))
            .cloned()
            .collect::<Vec<_>>();
        let count: i64 = self
            .require()?
            .query_row("SELECT count(*) FROM relations", [], |r| r.get(0))?;
        if count as usize + additions.len() > MAX_RELATIONS {
            return Err(Error::new(413, "POC relation limit reached."));
        }
        let relations = additions
            .iter()
            .map(|source| self.add_relation(&json!({"fromId":source,"toId":target,"type":typ})))
            .collect::<Result<Vec<_>>>()?;
        Ok(json!({"relations":relations,"skipped":sources.len() - additions.len()}))
    }
    pub fn grant(&mut self, data: &Value) -> Result<Value> {
        let name = text(data.get("name"), "connection name", 100, true)?;
        let alias = text(
            data.get("vaultAlias")
                .or(Some(&Value::String("memory".into()))),
            "vault alias",
            100,
            true,
        )?;
        let ids = data["nodeIds"]
            .as_array()
            .ok_or_else(|| Error::new(400, "Select existing nodes for this connection."))?;
        let heads = self.heads()?;
        if ids.is_empty()
            || ids.len() > 500
            || ids
                .iter()
                .any(|x| x.as_str().map(|id| !heads.contains_key(id)).unwrap_or(true))
        {
            return Err(Error::new(
                400,
                "Select existing nodes for this connection.",
            ));
        }
        let perms = data["permissions"].as_array().ok_or_else(|| {
            Error::new(
                400,
                "Only search, read, propose, and run permissions are supported.",
            )
        })?;
        if perms.is_empty()
            || perms
                .iter()
                .any(|x| !matches!(x.as_str(), Some("search" | "read" | "propose" | "run")))
        {
            return Err(Error::new(
                400,
                "Only search, read, propose, and run permissions are supported.",
            ));
        }
        let token_value = token();
        let mut node_ids = ids.iter().filter_map(Value::as_str).collect::<Vec<_>>();
        node_ids.sort();
        node_ids.dedup();
        let mut permissions = perms.iter().filter_map(Value::as_str).collect::<Vec<_>>();
        permissions.sort();
        permissions.dedup();
        let conn = json!({"id":Uuid::new_v4().simple().to_string(),"name":name,"vaultAlias":alias,"nodeIds":node_ids,"permissions":permissions,"revoked":false,"createdAt":now(),"tokenHash":sha(&token_value)});
        self.require()?.execute(
            "INSERT INTO connections VALUES (?,?)",
            params![conn["id"].as_str(), canonical(&conn)],
        )?;
        let mut public = conn.clone();
        public.as_object_mut().unwrap().remove("tokenHash");
        Ok(json!({"token":token_value,"connection":public}))
    }
    pub fn revoke(&mut self, id: &str) -> Result<Value> {
        let raw: Option<String> = self
            .require()?
            .query_row(
                "SELECT payload FROM connections WHERE id=?",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let mut conn: Value =
            serde_json::from_str(&raw.ok_or_else(|| Error::new(404, "Connection not found."))?)?;
        conn["revoked"] = Value::Bool(true);
        self.require()?.execute(
            "UPDATE connections SET payload=? WHERE id=?",
            params![canonical(&conn), id],
        )?;
        Ok(json!({"revoked":true}))
    }
    pub(crate) fn meta(&self, key: &str) -> Result<Option<Value>> {
        let raw: Option<String> = self
            .require()?
            .query_row("SELECT value FROM meta WHERE key=?", params![key], |r| {
                r.get(0)
            })
            .optional()?;
        raw.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    pub(crate) fn set_meta(&mut self, key: &str, value: &Value) -> Result<()> {
        self.require()?.execute("INSERT INTO meta(key,value) VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, canonical(value)])?;
        Ok(())
    }
    pub(crate) fn require_agent_insert(&mut self, proposal: &Value) -> Result<()> {
        self.require()?.execute(
            "INSERT INTO proposals VALUES (?,?)",
            params![proposal["id"].as_str(), canonical(proposal)],
        )?;
        Ok(())
    }
    pub(crate) fn proposal(&self, id: &str) -> Result<Value> {
        let raw: Option<String> = self
            .require()?
            .query_row(
                "SELECT payload FROM proposals WHERE id=?",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        serde_json::from_str(&raw.ok_or_else(|| Error::new(404, "Proposal not found."))?)
            .map_err(Into::into)
    }
    pub(crate) fn connection(&self, id: &str) -> Result<Value> {
        self.rows("connections")?
            .into_iter()
            .find(|c| c["id"] == id)
            .ok_or_else(|| Error::new(403, "The report connection is no longer authorized."))
    }
    pub(crate) fn review_agent_inner(&mut self, id: &str, approve: bool) -> Result<Value> {
        self.review_inner(id, approve)
    }
    pub fn propose(&mut self, data: &Value, grant: Option<&Value>) -> Result<Value> {
        let action = crate::validation::enum_field(
            data.as_object()
                .ok_or_else(|| Error::new(400, "Invalid proposal."))?,
            "action",
            "create",
        )?;
        if !["create", "update"].contains(&action) {
            return Err(Error::new(
                400,
                "Only memory create/update proposals are supported.",
            ));
        }
        let content = node_content(data.get("content").unwrap_or(&Value::Null))?;
        if content["tags"]
            .as_array()
            .map(|a| a.iter().any(|x| x == "alve-agent-report"))
            .unwrap_or(false)
            || content["facts"]
                .as_array()
                .map(|a| {
                    a.iter().any(|f| {
                        matches!(
                            f["key"].as_str(),
                            Some("agent_report_version" | "agent_run_id")
                        )
                    })
                })
                .unwrap_or(false)
        {
            return Err(Error::new(
                403,
                "Agent reports require the dedicated handoff flow.",
            ));
        }
        let node_id = data.get("nodeId").and_then(Value::as_str);
        if action == "update" {
            let id = node_id.unwrap_or("");
            if grant
                .map(|g| {
                    !g["nodeIds"]
                        .as_array()
                        .map(|a| a.iter().any(|x| x.as_str() == Some(id)))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
            {
                return Err(Error::new(404, "Node is unavailable to this connection."));
            }
            let current = self.heads()?.get(id).cloned().unwrap_or_default();
            if current.len() != 1
                || current[0]["revisionId"].as_str()
                    != data.get("expectedRevision").and_then(Value::as_str)
            {
                return Err(Error::new(
                    409,
                    "Read the current, non-conflicting revision before proposing an update.",
                ));
            }
        }
        let p = json!({"id":Uuid::new_v4().simple().to_string(),"action":action,"status":"pending","content":content,"nodeId":if action=="update"{Value::String(node_id.unwrap_or("").into())}else{Value::Null},"expectedRevision":if action=="update"{data.get("expectedRevision").cloned().unwrap_or(Value::Null)}else{Value::Null},"connectionId":grant.and_then(|g|g["id"].as_str()).map(Value::from).unwrap_or(Value::Null),"createdAt":now()});
        self.require()?.execute(
            "INSERT INTO proposals VALUES (?,?)",
            params![p["id"].as_str(), canonical(&p)],
        )?;
        Ok(p)
    }
    pub fn propose_batch(
        &mut self,
        batch: &Value,
        grant: Option<&Value>,
        confirmation: &Value,
    ) -> Result<Value> {
        let proposals = batch["proposals"]
            .as_array()
            .filter(|p| (2..=50).contains(&p.len()))
            .ok_or_else(|| Error::new(422, "Invalid prepared batch."))?;
        let batch_id = batch["batchId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 100)
            .ok_or_else(|| Error::new(422, "Invalid prepared batch."))?;
        let title = text(batch.get("title"), "group title", 200, true)?;
        let group = batch
            .get("group")
            .filter(|g| g.is_object())
            .ok_or_else(|| Error::new(422, "Invalid prepared batch."))?
            .clone();
        let expected = json!({"title":title,"type":"project","kind":"record","body":"Groups the selected memories.","tags":[],"facts":[],"references":[],"status":"active","relationType":"belongs_to","relationDirection":"member_to_group"});
        if group != expected {
            return Err(Error::new(422, "Invalid prepared batch."));
        }
        let mut result = Vec::new();
        for (index, payload) in proposals.iter().enumerate() {
            let mut proposal = self.propose(payload, grant)?;
            proposal["batchId"] = json!(batch_id);
            proposal["batchTitle"] = json!(title);
            proposal["batchIndex"] = json!(index);
            proposal["groupIntent"] = group.clone();
            proposal["qualityConfirmation"] = confirmation.clone();
            self.require()?.execute(
                "UPDATE proposals SET payload=? WHERE id=?",
                params![canonical(&proposal), proposal["id"].as_str()],
            )?;
            result.push(proposal);
        }
        Ok(json!({"batchId":batch_id,"proposals":result,"group":group}))
    }
    pub fn review(&mut self, id: &str, approve: bool) -> Result<Value> {
        let raw: Option<String> = self
            .require()?
            .query_row(
                "SELECT payload FROM proposals WHERE id=?",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let proposal: Value =
            serde_json::from_str(&raw.ok_or_else(|| Error::new(404, "Proposal not found."))?)?;
        if proposal.get("agentRunId").is_some() {
            return Err(Error::new(
                409,
                "Review agent reports with the dedicated report action.",
            ));
        }
        if proposal.get("batchId").is_some() {
            return Err(Error::new(
                409,
                "Review the complete prepared batch together.",
            ));
        }
        self.review_inner(id, approve)
    }
    fn review_inner(&mut self, id: &str, approve: bool) -> Result<Value> {
        let raw: Option<String> = self
            .require()?
            .query_row(
                "SELECT payload FROM proposals WHERE id=?",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        let mut p: Value =
            serde_json::from_str(&raw.ok_or_else(|| Error::new(404, "Proposal not found."))?)?;
        if p["status"] != "pending" {
            return Err(Error::new(409, "This proposal has already been reviewed."));
        }
        if approve {
            if let Some(connection_id) = p["connectionId"].as_str() {
                let raw: Option<String> = self
                    .require()?
                    .query_row(
                        "SELECT payload FROM connections WHERE id=?",
                        params![connection_id],
                        |r| r.get(0),
                    )
                    .optional()?;
                let grant: Value = serde_json::from_str(&raw.ok_or_else(|| {
                    Error::new(403, "The proposal's connection is no longer authorized.")
                })?)?;
                if grant["revoked"].as_bool().unwrap_or(true)
                    || !grant["permissions"]
                        .as_array()
                        .map(|a| a.iter().any(|v| v.as_str() == Some("propose")))
                        .unwrap_or(false)
                {
                    return Err(Error::new(
                        403,
                        "The proposal's connection is no longer authorized.",
                    ));
                }
                if p["action"] == "update"
                    && !grant["nodeIds"]
                        .as_array()
                        .map(|a| a.iter().any(|v| v == &p["nodeId"]))
                        .unwrap_or(false)
                {
                    return Err(Error::new(403, "Proposal target is no longer in scope."));
                }
            }
        }
        let parents = p["expectedRevision"]
            .as_str()
            .map(|value| vec![value.to_owned()]);
        let node = if approve {
            Some(self.add_node(
                &p["content"],
                p["nodeId"].as_str(),
                parents.as_deref(),
                "ai_proposal",
            )?)
        } else {
            None
        };
        p["status"] = Value::String(if approve { "approved" } else { "rejected" }.into());
        self.require()?.execute(
            "UPDATE proposals SET payload=? WHERE id=?",
            params![canonical(&p), id],
        )?;
        Ok(json!({"proposal":p,"node":node}))
    }
    pub fn review_batch(&mut self, data: &Value) -> Result<Value> {
        let object = data
            .as_object()
            .ok_or_else(|| Error::new(400, "Invalid batch review request."))?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "proposalIds" | "action" | "groupTitle"))
        {
            return Err(Error::new(400, "Invalid batch review request."));
        }
        let ids = data["proposalIds"].as_array().ok_or_else(|| {
            Error::new(
                400,
                "Use 1 to 50 unique proposal IDs and approve or reject.",
            )
        })?;
        let action = data["action"]
            .as_str()
            .filter(|x| matches!(*x, "approve" | "reject"))
            .ok_or_else(|| {
                Error::new(
                    400,
                    "Use 1 to 50 unique proposal IDs and approve or reject.",
                )
            })?;
        if ids.is_empty() || ids.len() > 50 {
            return Err(Error::new(
                400,
                "Use 1 to 50 unique proposal IDs and approve or reject.",
            ));
        }
        let ids = ids
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .filter(|x| !x.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        Error::new(
                            400,
                            "Use 1 to 50 unique proposal IDs and approve or reject.",
                        )
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        if ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len() {
            return Err(Error::new(
                400,
                "Use 1 to 50 unique proposal IDs and approve or reject.",
            ));
        }
        let mut group = match data.get("groupTitle") {
            None => None,
            Some(value) => {
                if action != "approve" || ids.len() < 2 {
                    return Err(Error::new(
                        400,
                        "A group is available only when approving at least two proposals.",
                    ));
                }
                Some(text(Some(value), "group title", 200, true)?)
            }
        };
        let mut selected = Vec::new();
        for id in &ids {
            let raw: Option<String> = self
                .require()?
                .query_row(
                    "SELECT payload FROM proposals WHERE id=?",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?;
            let value: Value = serde_json::from_str(&raw.ok_or_else(|| {
                Error::new(409, "A selected proposal is missing or already reviewed.")
            })?)?;
            if value["status"] != "pending" {
                return Err(Error::new(
                    409,
                    "A selected proposal is missing or already reviewed.",
                ));
            }
            selected.push(value);
        }
        let batch_ids: std::collections::HashSet<Option<String>> = selected
            .iter()
            .map(|p| p.get("batchId").and_then(Value::as_str).map(str::to_owned))
            .collect();
        if selected.iter().any(|p| p.get("agentRunId").is_some()) {
            return Err(Error::new(
                409,
                "Review agent reports with the dedicated report action.",
            ));
        }
        if batch_ids.iter().any(Option::is_some)
            && (batch_ids.len() != 1 || batch_ids.contains(&None))
        {
            return Err(Error::new(
                409,
                "Review one complete prepared batch at a time.",
            ));
        }
        let mut prepared_intent = None;
        if let Some(Some(batch_id)) = batch_ids.iter().next() {
            let members = self
                .rows("proposals")?
                .into_iter()
                .filter(|p| p["batchId"].as_str() == Some(batch_id.as_str()))
                .collect::<Vec<_>>();
            if members.iter().any(|p| p["status"] != "pending")
                || members.len() != ids.len()
                || members
                    .iter()
                    .any(|p| !ids.iter().any(|id| p["id"].as_str() == Some(id.as_str())))
            {
                return Err(Error::new(409, "Select the complete prepared batch."));
            }
            let intent = selected[0]
                .get("groupIntent")
                .cloned()
                .ok_or_else(|| Error::new(409, "Prepared batch group intent is inconsistent."))?;
            if selected
                .iter()
                .any(|p| p.get("groupIntent") != Some(&intent))
            {
                return Err(Error::new(
                    409,
                    "Prepared batch group intent is inconsistent.",
                ));
            }
            if group.is_some() {
                return Err(Error::new(
                    409,
                    "Prepared batches use their exact previewed group.",
                ));
            }
            if intent["relationType"] != "belongs_to"
                || intent["relationDirection"] != "member_to_group"
            {
                return Err(Error::new(
                    409,
                    "Prepared batch group intent is inconsistent.",
                ));
            }
            if action == "approve" {
                group = Some(
                    intent["title"]
                        .as_str()
                        .ok_or_else(|| {
                            Error::new(409, "Prepared batch group intent is inconsistent.")
                        })?
                        .to_owned(),
                );
                prepared_intent = Some(intent);
            }
        }
        let reviews = ids
            .iter()
            .map(|id| self.review_inner(id, action == "approve"))
            .collect::<Result<Vec<_>>>()?;
        let mut relations = Vec::new();
        let group_node = if let Some(title) = group {
            let mut content = prepared_intent.unwrap_or_else(|| {
                json!({"title":title,"body":"Groups the selected memories.","type":"project","kind":"record","tags":[],"facts":[],"references":[]})
            });
            if let Some(object) = content.as_object_mut() {
                object.remove("relationType");
                object.remove("relationDirection");
            }
            let node = self.add_node(&content, None, None, "user")?;
            for review in &reviews {
                let member = &review["node"];
                relations.push(self.add_relation(
                    &json!({"fromId":member["id"],"toId":node["id"],"type":"belongs_to"}),
                )?);
            }
            Some(node)
        } else {
            None
        };
        Ok(json!({"reviews":reviews,"group":group_node,"relations":relations}))
    }
    pub fn group_nodes(&mut self, data: &Value) -> Result<Value> {
        let object = data
            .as_object()
            .ok_or_else(|| Error::new(400, "Supply nodeIds and groupTitle only."))?;
        if object.len() != 2
            || !object.contains_key("nodeIds")
            || !object.contains_key("groupTitle")
        {
            return Err(Error::new(400, "Supply nodeIds and groupTitle only."));
        }
        let ids = data["nodeIds"]
            .as_array()
            .ok_or_else(|| Error::new(400, "Select 2 to 50 unique memory IDs."))?;
        if ids.len() < 2 || ids.len() > 50 {
            return Err(Error::new(400, "Select 2 to 50 unique memory IDs."));
        }
        let ids = ids
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned)
                    .ok_or_else(|| Error::new(400, "Select 2 to 50 unique memory IDs."))
            })
            .collect::<Result<Vec<_>>>()?;
        if ids.iter().collect::<std::collections::HashSet<_>>().len() != ids.len() {
            return Err(Error::new(400, "Select 2 to 50 unique memory IDs."));
        }
        let heads = self.heads()?;
        if ids.iter().any(|id| {
            heads
                .get(id)
                .map(|v| v.len() != 1 || v[0]["status"] == "archived")
                .unwrap_or(true)
        }) {
            return Err(Error::new(
                409,
                "All selected memories must be active and non-conflicting.",
            ));
        }
        let group=self.add_node(&json!({"title":text(data.get("groupTitle"),"group title",200,true)?,"body":"Groups the selected memories.","type":"project","kind":"record","tags":[],"facts":[],"references":[]}),None,None,"user")?;
        let relations = ids
            .iter()
            .map(|id| {
                self.add_relation(&json!({"fromId":id,"toId":group["id"],"type":"belongs_to"}))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(json!({"group":group,"relations":relations}))
    }
    pub fn record_quality(&mut self, id: &str, quality: &Value) -> Result<()> {
        let raw: String = self.require()?.query_row(
            "SELECT payload FROM proposals WHERE id=?",
            params![id],
            |r| r.get(0),
        )?;
        let mut p: Value = serde_json::from_str(&raw)?;
        p["qualityConfirmation"] = quality.clone();
        self.require()?.execute(
            "UPDATE proposals SET payload=? WHERE id=?",
            params![canonical(&p), id],
        )?;
        Ok(())
    }
    pub fn export(&self) -> Result<Value> {
        Ok(
            json!({"format":"alve-poc-1","vaultId":self.id()?,"revisions":self.rows("revisions")?,"relations":self.rows("relations")?}),
        )
    }
    pub fn bundle(&self) -> Result<Value> {
        let raw = crypto::envelope(
            BUNDLE,
            self.id()?,
            self.salt.as_ref().unwrap(),
            self.key.as_ref().unwrap(),
            canonical(&self.export()?).as_bytes(),
        )?;
        Ok(json!({"bundle":STANDARD.encode(raw)}))
    }
    pub fn merge(&mut self, bundle: &str, password: &str) -> Result<Value> {
        let (vault_id, _, _, data) = decode_bundle(bundle, password)?;
        if vault_id != self.id()? {
            return Err(Error::new(409, "This bundle belongs to a different vault."));
        }
        let revisions = data["revisions"]
            .as_array()
            .ok_or_else(|| Error::new(400, "Invalid bundle records."))?;
        let relations = data["relations"]
            .as_array()
            .ok_or_else(|| Error::new(400, "Invalid bundle records."))?;
        if revisions.len() > MAX_REVISIONS || relations.len() > MAX_RELATIONS {
            return Err(Error::new(400, "Invalid bundle records."));
        }
        let known = self
            .rows("revisions")?
            .into_iter()
            .map(|r| (r["revisionId"].as_str().unwrap_or("").to_owned(), r))
            .collect::<HashMap<_, _>>();
        let mut all = known.clone();
        let mut incoming = HashMap::new();
        for revision in revisions {
            validate_revision(revision, &vault_id)?;
            let id = revision["revisionId"].as_str().unwrap().to_owned();
            if let Some(existing) = all.get(&id).or(incoming.get(&id)) {
                if existing != revision {
                    return Err(Error::new(
                        409,
                        "A revision ID was reused with different content.",
                    ));
                }
            }
            incoming.insert(id, revision.clone());
        }
        for (id, value) in &incoming {
            all.entry(id.clone()).or_insert_with(|| value.clone());
        }
        if all.len() > MAX_REVISIONS {
            return Err(Error::new(413, "POC revision limit reached."));
        }
        validate_history(&all)?;
        let existing_edges = self
            .rows("relations")?
            .into_iter()
            .map(|r| (r["id"].as_str().unwrap_or("").to_owned(), r))
            .collect::<HashMap<_, _>>();
        let node_ids = all
            .values()
            .filter_map(|r| r["id"].as_str())
            .collect::<HashSet<_>>();
        let mut additions = HashMap::new();
        for relation in relations {
            validate_relation(relation, &vault_id, &node_ids)?;
            let id = relation["id"].as_str().unwrap().to_owned();
            // Check accumulated incoming state first: repeated IDs must not bypass validation.
            let merged = if let Some(existing) = additions.get(&id).or(existing_edges.get(&id)) {
                merge_relation(existing, relation)?
            } else {
                relation.clone()
            };
            additions.insert(id, merged);
        }
        if existing_edges.len()
            + additions
                .keys()
                .filter(|id| !existing_edges.contains_key(*id))
                .count()
            > MAX_RELATIONS
        {
            return Err(Error::new(413, "POC relation limit reached."));
        }
        let db = self.require()?;
        let mut added_revisions = 0;
        for (id, revision) in incoming {
            if !known.contains_key(&id) {
                db.execute(
                    "INSERT INTO revisions VALUES (?,?,?)",
                    params![id, revision["id"].as_str(), canonical(&revision)],
                )?;
                added_revisions += 1;
            }
        }
        let mut added_relations = 0;
        for (id, relation) in additions {
            if !existing_edges.contains_key(&id) {
                db.execute(
                    "INSERT INTO relations VALUES (?,?)",
                    params![id, canonical(&relation)],
                )?;
                added_relations += 1;
            } else if existing_edges.get(&id) != Some(&relation) {
                db.execute(
                    "UPDATE relations SET payload=? WHERE id=?",
                    params![canonical(&relation), id],
                )?;
            }
        }
        Ok(
            json!({"addedRevisions":added_revisions,"addedRelations":added_relations,"conflicts":self.graph()?["conflicts"].as_array().map(|x|x.len()).unwrap_or(0)}),
        )
    }
    pub fn restore(&mut self, bundle: &str, password: &str) -> Result<Value> {
        if self.db.is_some() || self.path.exists() {
            return Err(Error::new(
                409,
                "Restore requires a new installation with no existing vault.",
            ));
        }
        let (id, salt, key, _) = decode_bundle(bundle, password)?;
        let db = Connection::open_in_memory()?;
        init(&db, &id)?;
        self.db = Some(db);
        self.vault_id = Some(id);
        self.salt = Some(salt);
        self.key = Some(key);
        let result = self.merge(bundle, password);
        match result {
            Ok(_) => {
                if let Err(e) = self.persist() {
                    self.lock();
                    return Err(e);
                }
                self.admin = Some(token());
                Ok(json!({"token":self.admin,"vaultId":self.vault_id}))
            }
            Err(error) => {
                self.lock();
                Err(error)
            }
        }
    }
}
fn init(db: &Connection, id: &str) -> Result<()> {
    db.execute_batch("PRAGMA temp_store=MEMORY;PRAGMA journal_mode=MEMORY;")?;
    db.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY,value TEXT NOT NULL);CREATE TABLE revisions (id TEXT PRIMARY KEY,node_id TEXT NOT NULL,payload TEXT NOT NULL);CREATE INDEX revisions_node ON revisions(node_id);CREATE TABLE relations (id TEXT PRIMARY KEY,payload TEXT NOT NULL);CREATE TABLE proposals (id TEXT PRIMARY KEY,payload TEXT NOT NULL);CREATE TABLE connections (id TEXT PRIMARY KEY,payload TEXT NOT NULL);")?;
    db.execute("INSERT INTO meta VALUES (?,?)", params!["vaultId", id])?;
    db.execute(
        "INSERT INTO meta VALUES (?,?)",
        params!["format", "alve-poc-1"],
    )?;
    Ok(())
}
fn token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    STANDARD.encode(bytes)
}
fn sha(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn canonical(value: &Value) -> String {
    serde_json::to_string(value).expect("JSON Value serializes")
}
impl Drop for Vault {
    fn drop(&mut self) {
        self.lock();
    }
}
fn decode_bundle(bundle: &str, password: &str) -> Result<(String, [u8; 16], [u8; 32], Value)> {
    if bundle.len() > MAX_ENVELOPE * 4 / 3 + 8 {
        return Err(Error::new(413, "Invalid bundle size."));
    }
    let raw = STANDARD
        .decode(bundle)
        .map_err(|_| Error::new(400, "Invalid bundle encoding."))?;
    let (id, salt, key, plain) = crypto::decrypt(&raw, BUNDLE, password)?;
    let data: Value =
        serde_json::from_slice(&plain).map_err(|_| Error::new(400, "Invalid bundle contents."))?;
    if data["format"] != "alve-poc-1" || data["vaultId"].as_str() != Some(&id) {
        return Err(Error::new(400, "Unsupported bundle schema."));
    }
    Ok((id, salt, key, data))
}
fn validate_revision(r: &Value, vault_id: &str) -> Result<()> {
    let object = r
        .as_object()
        .ok_or_else(|| Error::new(400, "Invalid revision vault."))?;
    if r["vaultId"].as_str() != Some(vault_id) {
        return Err(Error::new(400, "Invalid revision vault."));
    }
    text(object.get("revisionId"), "revision ID", 64, true)?;
    text(object.get("id"), "node ID", 64, true)?;
    node_content(r)?;
    let parents = r["parents"]
        .as_array()
        .ok_or_else(|| Error::new(400, "Invalid revision parents."))?;
    let mut seen = HashSet::new();
    if parents.len() > MAX_REVISIONS
        || parents
            .iter()
            .any(|p| p.as_str().map(|x| !seen.insert(x)).unwrap_or(true))
    {
        return Err(Error::new(400, "Invalid revision parents."));
    }
    if !matches!(
        r["origin"].as_str(),
        Some("user" | "import" | "ai_proposal")
    ) || r["verification"] != "user_confirmed"
    {
        return Err(Error::new(400, "Invalid revision provenance."));
    }
    for key in ["createdAt", "updatedAt"] {
        let value = text(object.get(key), key, 80, true)?;
        if chrono::DateTime::parse_from_rfc3339(&value).is_err() {
            return Err(Error::new(400, "Invalid revision timestamp."));
        }
    }
    Ok(())
}
fn validate_history(all: &HashMap<String, Value>) -> Result<()> {
    let mut roots = HashMap::new();
    let mut degree = HashMap::new();
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for (id, r) in all {
        let parents = r["parents"].as_array().unwrap();
        if parents.is_empty() {
            let node = r["id"].as_str().unwrap();
            if roots.insert(node, id).is_some() {
                return Err(Error::new(
                    409,
                    "Multiple independent roots use the same node ID.",
                ));
            }
        }
        degree.insert(id.clone(), parents.len());
        for parent in parents {
            let parent = parent.as_str().unwrap();
            if all.get(parent).map(|x| x["id"] != r["id"]).unwrap_or(true) {
                return Err(Error::new(
                    409,
                    "Bundle is missing a parent revision or crosses node boundaries.",
                ));
            }
            children.entry(parent.into()).or_default().push(id.clone());
        }
    }
    let mut queue = degree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(id, _)| id.clone())
        .collect::<VecDeque<_>>();
    let mut count = 0;
    while let Some(id) = queue.pop_front() {
        count += 1;
        for child in children.get(&id).into_iter().flatten() {
            let degree = degree.get_mut(child).unwrap();
            *degree -= 1;
            if *degree == 0 {
                queue.push_back(child.clone())
            }
        }
    }
    if count != all.len() {
        return Err(Error::new(409, "Revision history contains a cycle."));
    }
    Ok(())
}
fn validate_relation(r: &Value, vault_id: &str, nodes: &HashSet<&str>) -> Result<()> {
    if r["vaultId"].as_str() != Some(vault_id)
        || !RELATIONS.contains(&r["type"].as_str().unwrap_or(""))
    {
        return Err(Error::new(400, "Invalid relation."));
    }
    text(r.get("id"), "relation ID", 64, true)?;
    if !nodes.contains(r["fromId"].as_str().unwrap_or(""))
        || !nodes.contains(r["toId"].as_str().unwrap_or(""))
    {
        return Err(Error::new(400, "Relation endpoint is missing."));
    }
    if !matches!(
        r["origin"].as_str(),
        Some("user" | "import" | "ai_proposal")
    ) || r["verification"] != "user_confirmed"
    {
        return Err(Error::new(400, "Invalid relation provenance."));
    }
    // Legacy active records omit both fields. A deletion is irreversible for this ID.
    if r.get("deleted").is_some() || r.get("deletedAt").is_some() {
        if r["deleted"] != true {
            return Err(Error::new(400, "Invalid relation tombstone."));
        }
        let timestamp = text(r.get("deletedAt"), "relation deletion timestamp", 80, true)?;
        if chrono::DateTime::parse_from_rfc3339(&timestamp).is_err() {
            return Err(Error::new(400, "Invalid relation deletion timestamp."));
        }
    }
    if r.get("restoredFrom").is_some() {
        text(r.get("restoredFrom"), "restored relation ID", 64, true)?;
    }
    Ok(())
}

fn merge_relation(existing: &Value, incoming: &Value) -> Result<Value> {
    let base = |value: &Value| {
        let mut value = value.clone();
        let object = value.as_object_mut().unwrap();
        object.remove("deleted");
        object.remove("deletedAt");
        value
    };
    if base(existing) != base(incoming) {
        return Err(Error::new(
            409,
            "A relation ID was reused with different content.",
        ));
    }
    // Lexical minimum is deterministic across runtimes, time zones, and merge order.
    Ok([existing, incoming]
        .into_iter()
        .filter(|r| r["deleted"] == true)
        .min_by(|a, b| a["deletedAt"].as_str().cmp(&b["deletedAt"].as_str()))
        .unwrap_or(incoming)
        .clone())
}
