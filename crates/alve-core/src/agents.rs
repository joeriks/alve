//! Device-local agent lease ledger.  It intentionally lives in encrypted meta
//! rather than in exported records: reports are ordinary portable memories,
//! while a lease is not a distributed lock.
use crate::{
    validation::{node_content, now, text},
    vault::Vault,
    Error, Result,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

const KEY: &str = "agentRuns";
fn ledger(v: &Vault) -> Result<Vec<Value>> {
    match v
        .meta(KEY)
        .map_err(|_| Error::new(409, "Local agent run history is invalid."))?
    {
        None => Ok(Vec::new()),
        Some(Value::Array(rows))
            if rows.iter().all(|r| {
                r.is_object()
                    && [
                        "runId",
                        "agentId",
                        "agentRevision",
                        "actor",
                        "requestId",
                        "state",
                        "startedAt",
                        "expiresAt",
                    ]
                    .iter()
                    .all(|k| r[*k].as_str().map(|s| !s.is_empty()).unwrap_or(false))
                    && r["context"]
                        .as_array()
                        .map(|cs| {
                            cs.iter().all(|c| {
                                ["id", "revisionId"]
                                    .iter()
                                    .all(|k| c[*k].as_str().map(|s| !s.is_empty()).unwrap_or(false))
                            })
                        })
                        .unwrap_or(false)
            }) =>
        {
            Ok(rows)
        }
        Some(_) => Err(Error::new(409, "Local agent run history is invalid.")),
    }
}
fn save(v: &mut Vault, rows: &[Value]) -> Result<()> {
    v.set_meta(KEY, &Value::Array(rows.to_vec()))
}
fn fact(n: &Value, key: &str) -> Option<String> {
    let f = n["facts"].as_array()?.iter().find(|f| f["key"] == key)?;
    if f["value"]["type"] != "text" {
        return None;
    }
    f["value"]["value"].as_str().map(str::to_owned)
}
fn datetime_fact(n: &Value, key: &str) -> Option<String> {
    let f = n["facts"].as_array()?.iter().find(|f| f["key"] == key)?;
    if f["value"]["type"] != "datetime" {
        return None;
    }
    let value = f["value"]["value"].as_str()?;
    DateTime::parse_from_rfc3339(value).ok()?;
    Some(value.to_owned())
}
fn introduction(v: &Vault, id: &str) -> Result<Value> {
    let n = v.heads()?.get(id).cloned().unwrap_or_default();
    if n.len() != 1
        || n[0]["status"] == "archived"
        || !n[0]["tags"]
            .as_array()
            .map(|a| a.iter().any(|x| x == "alve-agent"))
            .unwrap_or(false)
    {
        return Err(Error::new(409, "Agent is unavailable."));
    }
    if n[0]["type"] != "memory" || n[0]["kind"] != "record" {
        return Err(Error::new(409, "Agent metadata is invalid."));
    }
    for key in [
        "agent_mission",
        "agent_method",
        "agent_escalation",
        "agent_firstAssignment",
        "agent_understanding",
        "agent_phase",
    ] {
        if fact(&n[0], key).is_none() {
            return Err(Error::new(409, "Agent metadata is incomplete."));
        }
    }
    if !matches!(
        fact(&n[0], "agent_phase").as_deref(),
        Some("ready" | "introduced")
    ) {
        return Err(Error::new(409, "Agent metadata is invalid."));
    }
    review_date(&n[0])?;
    Ok(n[0].clone())
}
fn agent(v: &Vault, id: &str) -> Result<Value> {
    let n = introduction(v, id)?;
    if fact(&n, "agent_phase").as_deref() != Some("ready")
        || fact(&n, "agent_understanding")
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
    {
        return Err(Error::new(409, "Agent is not ready."));
    }
    Ok(n)
}
fn review_date(n: &Value) -> Result<Option<String>> {
    let item = n["facts"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|f| f["key"] == "agent_reviewDate");
    match item {
        None => Ok(None),
        Some(f) if f["value"]["value"] == "" => Ok(None),
        Some(f) => {
            let raw = f["value"]["value"].as_str().unwrap_or("");
            if !matches!(f["value"]["type"].as_str(), Some("date" | "text"))
                || NaiveDate::parse_from_str(raw, "%Y-%m-%d")
                    .map(|d| d.to_string() != raw)
                    .unwrap_or(true)
            {
                return Err(Error::new(409, "Agent follow-up date is invalid."));
            }
            Ok(Some(format!("{raw}T00:00:00+00:00")))
        }
    }
}
fn contexts(a: &Value) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for f in a["facts"].as_array().into_iter().flatten() {
        let key = f["key"].as_str().unwrap_or("");
        if let Some(suffix) = key.strip_prefix("agent_context_") {
            if suffix.is_empty()
                || !suffix.bytes().all(|c| c.is_ascii_digit())
                || f["value"]["type"] != "text"
            {
                return Err(Error::new(409, "Agent context metadata is invalid."));
            }
            let id = f["value"]["value"]
                .as_str()
                .filter(|x| !x.trim().is_empty())
                .ok_or_else(|| Error::new(409, "Agent context metadata is invalid."))?;
            if !ids.insert(id) {
                return Err(Error::new(409, "Agent context metadata is invalid."));
            }
            out.push(json!({"id":id}));
        }
    }
    if out.len() > 20 {
        return Err(Error::new(409, "Agent context metadata is invalid."));
    }
    Ok(out)
}
fn frozen(v: &Vault, a: &Value) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    let heads = v.heads()?;
    for c in contexts(a)? {
        let id = c["id"].as_str().unwrap();
        let n = heads.get(id).cloned().unwrap_or_default();
        if n.len() != 1 || n[0]["status"] == "archived" {
            return Err(Error::new(409, "Agent context is unavailable."));
        }
        out.push(json!({"id":id,"revisionId":n[0]["revisionId"]}));
    }
    Ok(out)
}
fn current_frozen(v: &Vault, run: &Value) -> Result<()> {
    let a = agent(v, run["agentId"].as_str().unwrap_or(""))?;
    if a["revisionId"] != run["agentRevision"] {
        return Err(Error::new(409, "Agent changed; start a new assignment."));
    }
    let heads = v.heads()?;
    for c in run["context"].as_array().into_iter().flatten() {
        let n = heads
            .get(c["id"].as_str().unwrap_or(""))
            .cloned()
            .unwrap_or_default();
        if n.len() != 1 || n[0]["status"] == "archived" || n[0]["revisionId"] != c["revisionId"] {
            return Err(Error::new(
                409,
                "Assignment context changed or is unavailable.",
            ));
        }
    }
    Ok(())
}
fn actor(grant: Option<&Value>, _token: &str) -> String {
    grant
        .and_then(|g| g["id"].as_str())
        .unwrap_or("owner")
        .to_owned()
}
fn scope(grant: Option<&Value>, agent: &Value, context: &[Value]) -> Result<()> {
    if let Some(g) = grant {
        let ids = g["nodeIds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<std::collections::HashSet<_>>();
        if !ids.contains(agent["id"].as_str().unwrap_or(""))
            || context
                .iter()
                .any(|c| !ids.contains(c["id"].as_str().unwrap_or("")))
        {
            return Err(Error::new(
                404,
                "Agent assignment is unavailable to this connection.",
            ));
        }
    }
    Ok(())
}
fn assignment(a: &Value, context: &[Value]) -> Value {
    let date = review_date(a)
        .ok()
        .flatten()
        .map(|s| s[..10].to_owned())
        .unwrap_or_default();
    json!({"agentId":a["id"],"name":a["title"],"title":a["title"],"mission":fact(a,"agent_mission"),"method":fact(a,"agent_method"),"escalation":fact(a,"agent_escalation"),"firstAssignment":fact(a,"agent_firstAssignment"),"understanding":fact(a,"agent_understanding"),"phase":fact(a,"agent_phase"),"reviewDate":date,"contextIds":context.iter().map(|x|x["id"].clone()).collect::<Vec<_>>()})
}
fn public_run(r: &Value) -> Value {
    let mut out = r.clone();
    if let Some(o) = out.as_object_mut() {
        for key in ["actor", "requestId", "briefing"] {
            o.remove(key);
        }
    }
    out
}
// Report provenance is data carried by an owner-approved node, not a local lease.
fn report_context(n: &Value) -> Option<Vec<Value>> {
    let mut result = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for f in n["facts"].as_array()? {
        if let Some(suffix) = f["key"].as_str()?.strip_prefix("agent_context_") {
            if suffix.is_empty()
                || !suffix.bytes().all(|c| c.is_ascii_digit())
                || f["value"]["type"] != "text"
            {
                return None;
            }
            let c: Value = serde_json::from_str(f["value"]["value"].as_str()?).ok()?;
            if c.as_object()?.len() != 2
                || c["id"].as_str()?.is_empty()
                || c["revisionId"].as_str()?.is_empty()
                || !ids.insert(c["id"].as_str()?.to_owned())
            {
                return None;
            }
            result.push(c);
        }
    }
    if result.len() > 20 {
        return None;
    }
    Some(result)
}
fn last_report(v: &Vault, agent_id: &str, context: &[Value]) -> Option<Value> {
    let allowed = context
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect::<std::collections::HashSet<_>>();
    v.heads()
        .ok()?
        .values()
        .filter(|hs| hs.len() == 1)
        .filter_map(|hs| hs.first())
        .filter(|n| {
            n["type"] == "memory"
                && n["kind"] == "record"
                && n["status"] == "active"
                && n["tags"]
                    .as_array()
                    .map(|a| a.iter().any(|t| t == "alve-agent-report"))
                    .unwrap_or(false)
                && fact(n, "agent_report_version").as_deref() == Some("1")
                && fact(n, "agent_id").as_deref() == Some(agent_id)
                && ["agent_run_id", "agent_revision", "agent_nextAction"]
                    .iter()
                    .all(|key| fact(n, key).map(|s| !s.is_empty()).unwrap_or(false))
                && fact(n, "agent_remaining").is_some()
                && fact(n, "agent_outcome")
                    .map(|s| ["completed", "partial", "blocked"].contains(&s.as_str()))
                    .unwrap_or(false)
                && ["agent_startedAt", "agent_reportedAt", "agent_nextFollowUp"]
                    .iter()
                    .all(|key| datetime_fact(n, key).is_some())
                && report_context(n)
                    .map(|cs| {
                        cs.iter()
                            .all(|c| allowed.contains(c["id"].as_str().unwrap_or("")))
                    })
                    .unwrap_or(false)
        })
        // Compare instants, not strings: equivalent ISO timestamps can use different offsets.
        .max_by_key(|n| {
            DateTime::parse_from_rfc3339(&datetime_fact(n, "agent_reportedAt").unwrap()).unwrap()
        })
        .cloned()
}
fn effective(r: &Value) -> &str {
    if r["state"] == "running" && !active(r) {
        "expired"
    } else {
        r["state"].as_str().unwrap_or("blocked")
    }
}
fn revision_map(context: &[Value]) -> std::collections::BTreeMap<String, String> {
    context
        .iter()
        .map(|c| {
            (
                c["id"].as_str().unwrap_or("").to_owned(),
                c["revisionId"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect()
}
fn active(r: &Value) -> bool {
    r["state"] == "running"
        && DateTime::parse_from_rfc3339(r["expiresAt"].as_str().unwrap_or(""))
            .map(|x| x.with_timezone(&Utc) > Utc::now())
            .unwrap_or(false)
}
pub fn list(v: &Vault, grant: Option<&Value>, limit: usize, offset: usize) -> Result<Value> {
    if !(1..=100).contains(&limit) || offset > 5000 {
        return Err(Error::new(422, "Use limit 1 to 100 and offset 0 to 5000."));
    }
    let runs = ledger(v)?;
    let mut rows = Vec::new();
    for n in v.heads()?.values().filter_map(|hs| hs.first()) {
        if n["status"] == "archived"
            || !n["tags"]
                .as_array()
                .map(|ts| ts.iter().any(|t| t == "alve-agent"))
                .unwrap_or(false)
            || scope(grant, n, &[]).is_err()
        {
            continue;
        }
        let mut row = json!({"agentId":n["id"],"title":n["title"],"status":"blocked","dueReason":"Introduction or complete context needs owner review.","nextFollowUp":Value::Null});
        let eligible = (|| -> Result<(Value, Vec<Value>)> {
            let a = introduction(v, n["id"].as_str().unwrap_or(""))?;
            let context = frozen(v, &a)?;
            scope(grant, &a, &context)?;
            Ok((a, context))
        })();
        if let Ok((a, context)) = eligible {
            if fact(&a, "agent_phase").as_deref() != Some("ready")
                || fact(&a, "agent_understanding")
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
            {
                row["status"] = json!("introduced");
                row["dueReason"] = json!("Confirm the agent understanding first.");
            } else {
                let last = last_report(v, a["id"].as_str().unwrap_or(""), &context);
                let due = match &last {
                    Some(r) => datetime_fact(r, "agent_nextFollowUp"),
                    None => review_date(&a)?,
                };
                let changed = last
                    .as_ref()
                    .map(|r| {
                        fact(r, "agent_revision").as_deref() != a["revisionId"].as_str()
                            || revision_map(&report_context(r).unwrap()) != revision_map(&context)
                    })
                    .unwrap_or(false);
                let is_due = changed
                    || due
                        .as_ref()
                        .map(|s| {
                            DateTime::parse_from_rfc3339(s)
                                .map(|t| t.with_timezone(&Utc) <= Utc::now())
                                .unwrap_or(true)
                        })
                        .unwrap_or(true);
                row["status"] = json!(if is_due { "due" } else { "not_due" });
                row["dueReason"] = json!(if changed {
                    "Source information changed."
                } else if due.is_some() {
                    "Follow-up is due."
                } else {
                    "No approved handoff yet."
                });
                row["nextFollowUp"] = json!(due);
                if let Some(r) = runs.iter().rev().find(|r| r["agentId"] == a["id"]) {
                    match effective(r) {
                        "running" => {
                            row["status"] = json!("running");
                            row["dueReason"] = json!("A local run is active.");
                        }
                        "pending_report" => {
                            row["status"] = json!("pending_report");
                            row["dueReason"] = json!("Owner report review is required.");
                        }
                        "expired" | "abandoned" | "rejected" => {
                            row["status"] = json!(if effective(r) == "expired" {
                                "expired"
                            } else {
                                "due"
                            });
                            row["dueReason"] = json!(
                                "Previous run was not approved. Resume the last approved handoff."
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
        rows.push(row);
    }
    rows.sort_by_key(|r| {
        (
            r["status"] == "not_due",
            r["title"].as_str().unwrap_or("").to_lowercase(),
            r["agentId"].as_str().unwrap_or("").to_owned(),
        )
    });
    let total = rows.len();
    let next = if offset + limit < total {
        Some(offset + limit)
    } else {
        None
    };
    Ok(
        json!({"assignments":rows.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),"limit":limit,"offset":offset,"total":total,"nextOffset":next,"instructions":["Choose a due assignment, then call get_agent_briefing with a fresh requestId. Reuse that requestId when retrying.","Before ending, prepare, confirm, and submit a handoff including partial progress."],"nextTools":["get_agent_briefing","get_agent_run"]}),
    )
}
pub fn briefing(v: &mut Vault, grant: Option<&Value>, token: &str, data: &Value) -> Result<Value> {
    if data
        .as_object()
        .map(|o| o.len() != 2 || !o.contains_key("agentId") || !o.contains_key("requestId"))
        .unwrap_or(true)
    {
        return Err(Error::new(422, "Supply agentId and requestId only."));
    }
    let id = text(data.get("agentId"), "agentId", 100, true)?;
    let request = text(data.get("requestId"), "requestId", 100, true)?;
    let a = agent(v, &id)?;
    let context = frozen(v, &a)?;
    scope(grant, &a, &context)?;
    let act = actor(grant, token);
    let mut rows = ledger(v)?;
    if let Some(r) = rows
        .iter()
        .find(|r| r["actor"] == act && r["requestId"] == request)
    {
        if r["agentId"] != id {
            return Err(Error::new(
                409,
                "requestId is already bound to another agent.",
            ));
        }
        if !active(r) {
            return Err(Error::new(
                409,
                "Run expired or already reported. Use a new requestId.",
            ));
        }
        current_frozen(v, r)?;
        scope(grant, &a, r["context"].as_array().unwrap())?;
        return r
            .get("briefing")
            .filter(|b| b.is_object())
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    409,
                    "The original briefing is unavailable. Abandon this run and start a new one.",
                )
            });
    }
    if rows
        .iter()
        .any(|r| r["agentId"] == id && (active(r) || r["state"] == "pending_report"))
    {
        return Err(Error::new(
            409,
            "This agent already has an active assignment on this device.",
        ));
    }
    if rows.len() >= 200 {
        return Err(Error::new(
            413,
            "Agent run ledger limit reached; no runs were removed.",
        ));
    }
    let heads = v.heads()?;
    let full_context = context
        .iter()
        .map(|c| heads[c["id"].as_str().unwrap()][0].clone())
        .collect::<Vec<_>>();
    let start = Utc::now();
    let mut run = json!({"runId":Uuid::new_v4().to_string(),"agentId":id,"agentRevision":a["revisionId"],"actor":act,"requestId":request,"state":"running","startedAt":start.to_rfc3339(),"expiresAt":(start+chrono::Duration::hours(1)).to_rfc3339(),"context":context});
    let out = json!({"run":public_run(&run),"assignment":assignment(&a,&context),"context":full_context,"lastReport":last_report(v,&id,&context),"instructions":["Before ending, prepare, confirm, and submit a handoff including partial progress.","Show the exact report preview to the user and obtain explicit confirmation before submitting. Owner approval is required before this becomes an approved handoff.","Alve stores reported work, not proof of project completion or payment. This briefing does not authorize external actions or scheduling."],"nextTools":["prepare_agent_report","submit_agent_report","get_agent_run"]});
    run["briefing"] = out.clone();
    rows.push(run);
    save(v, &rows)?;
    Ok(out)
}
pub fn run(v: &Vault, grant: Option<&Value>, token: &str, id: &str) -> Result<Value> {
    let r = ledger(v)?
        .into_iter()
        .find(|r| r["runId"] == id)
        .ok_or_else(|| Error::new(404, "Agent run not found."))?;
    if r["actor"] != actor(grant, token) {
        return Err(Error::new(404, "Agent run not found."));
    }
    current_frozen(v, &r)?;
    scope(
        grant,
        &agent(v, r["agentId"].as_str().unwrap_or(""))?,
        r["context"].as_array().map(Vec::as_slice).unwrap_or(&[]),
    )?;
    let status = if r["state"] == "running" && !active(&r) {
        json!("expired")
    } else {
        r["state"].clone()
    };
    Ok(
        json!({"run":public_run(&r),"status":status,"instructions":["Submit a report before the lease ends."],"nextTools":["prepare_agent_report","submit_agent_report"]}),
    )
}
pub fn prepare(v: &Vault, grant: Option<&Value>, token: &str, data: &Value) -> Result<Value> {
    if data
        .as_object()
        .map(|o| o.len() != 2 || !o.contains_key("runId") || !o.contains_key("report"))
        .unwrap_or(true)
    {
        return Err(Error::new(422, "Supply runId and a structured report."));
    }
    let id = text(data.get("runId"), "runId", 100, true)?;
    let r = ledger(v)?
        .into_iter()
        .find(|x| x["runId"] == id)
        .ok_or_else(|| Error::new(404, "Agent run not found."))?;
    if r["actor"] != actor(grant, token) || !active(&r) {
        return Err(Error::new(409, "Agent lease is no longer active."));
    }
    current_frozen(v, &r)?;
    scope(
        grant,
        &agent(v, r["agentId"].as_str().unwrap_or(""))?,
        r["context"].as_array().map(Vec::as_slice).unwrap_or(&[]),
    )?;
    let report = data
        .get("report")
        .ok_or_else(|| Error::new(400, "Invalid report."))?;
    if report
        .as_object()
        .map(|o| {
            o.keys().any(|k| {
                !matches!(
                    k.as_str(),
                    "workPerformed"
                        | "result"
                        | "nextAction"
                        | "uncertainties"
                        | "remaining"
                        | "outcome"
                        | "nextFollowUp"
                        | "references"
                )
            })
        })
        .unwrap_or(true)
    {
        return Err(Error::new(422, "Invalid report fields."));
    }
    let required = ["workPerformed", "result", "nextAction"];
    for k in required {
        text(report.get(k), k, 300, true)?;
    }
    let wp = text(report.get("workPerformed"), "workPerformed", 300, true)?;
    let result = text(report.get("result"), "result", 300, true)?;
    let uncertainties = text(
        report
            .get("uncertainties")
            .or(Some(&Value::String("".into()))),
        "uncertainties",
        300,
        false,
    )?;
    let remaining = text(
        report.get("remaining").or(Some(&Value::String("".into()))),
        "remaining",
        300,
        false,
    )?;
    let next = text(report.get("nextAction"), "nextAction", 300, true)?;
    let outcome = report["outcome"]
        .as_str()
        .filter(|x| ["completed", "partial", "blocked"].contains(x))
        .ok_or_else(|| Error::new(400, "Invalid report outcome."))?;
    let follow = DateTime::parse_from_rfc3339(report["nextFollowUp"].as_str().unwrap_or(""))
        .map_err(|_| Error::new(400, "nextFollowUp must include a UTC offset."))?
        .with_timezone(&Utc)
        .to_rfc3339();
    let refs = match report.get("references") {
        None => Vec::new(),
        Some(Value::Array(a)) => a.clone(),
        Some(_) => return Err(Error::new(422, "Invalid references.")),
    };
    if refs.len() > 20 {
        return Err(Error::new(400, "Invalid references."));
    }
    let body = format!(
        "Work performed\n{}\n\nResult\n{}\n\nUncertainties\n{}\n\nRemaining\n{}\n\nNext action\n{}",
        wp, result, uncertainties, remaining, next
    );
    if body.chars().count() > 2000 || body.split_whitespace().count() > 300 {
        return Err(Error::new(400, "Report is too long."));
    }
    let a = agent(v, r["agentId"].as_str().unwrap_or(""))?;
    let mut facts = vec![
        tf("agent_report_version", json!("1")),
        tf("agent_id", r["agentId"].clone()),
        tf("agent_run_id", r["runId"].clone()),
        tf("agent_revision", r["agentRevision"].clone()),
        tf("agent_outcome", json!(outcome)),
        df("agent_startedAt", &r["startedAt"]),
        df("agent_reportedAt", &json!(now())),
        df("agent_nextFollowUp", &json!(follow)),
        tf("agent_nextAction", json!(next)),
        tf("agent_remaining", json!(remaining)),
    ];
    for (i, c) in r["context"].as_array().into_iter().flatten().enumerate() {
        facts.push(tf(
            &format!("agent_context_{}", i + 1),
            json!(c.to_string()),
        ))
    }
    let title = format!("Handoff: {}", a["title"].as_str().unwrap_or("Agent"))
        .chars()
        .take(120)
        .collect::<String>();
    let content = node_content(
        &json!({"title":title,"body":body,"type":"memory","kind":"record","tags":["alve-agent-report"],"facts":facts,"references":refs,"status":"active"}),
    )?;
    Ok(json!({"runId":r["runId"],"content":content}))
}
fn tf(key: &str, value: Value) -> Value {
    json!({"key":key,"label":key,"value":{"type":"text","value":value}})
}
fn df(key: &str, value: &Value) -> Value {
    json!({"key":key,"label":key,"value":{"type":"datetime","value":value,"timeZone":"UTC"}})
}
pub fn submit(
    v: &mut Vault,
    grant: Option<&Value>,
    token: &str,
    payload: &Value,
    confirmation: &Value,
) -> Result<Value> {
    let run_id = payload["runId"]
        .as_str()
        .ok_or_else(|| Error::new(409, "Invalid prepared report."))?;
    let r = ledger(v)?
        .into_iter()
        .find(|r| r["runId"] == run_id)
        .ok_or_else(|| Error::new(404, "Agent run not found."))?;
    if r["actor"] != actor(grant, token) || !active(&r) {
        return Err(Error::new(409, "Agent lease is no longer active."));
    }
    current_frozen(v, &r)?;
    scope(
        grant,
        &agent(v, r["agentId"].as_str().unwrap_or(""))?,
        r["context"].as_array().map(Vec::as_slice).unwrap_or(&[]),
    )?;
    let content = payload["content"].clone();
    if fact(&content, "agent_run_id").as_deref() != Some(run_id)
        || fact(&content, "agent_revision") != r["agentRevision"].as_str().map(str::to_owned)
        || fact(&content, "agent_id") != r["agentId"].as_str().map(str::to_owned)
        || report_context(&content).map(|cs| revision_map(&cs))
            != Some(revision_map(r["context"].as_array().unwrap()))
    {
        return Err(Error::new(
            409,
            "Prepared report no longer matches this assignment.",
        ));
    }
    // bypass generic reserved-marker guard only from this frozen, dedicated path.
    let p = json!({"id":Uuid::new_v4().simple().to_string(),"action":"create","status":"pending","content":content,"nodeId":Value::Null,"expectedRevision":Value::Null,"connectionId":grant.and_then(|g|g["id"].as_str()).map(Value::from).unwrap_or(Value::Null),"createdAt":now(),"agentRunId":r["runId"],"qualityConfirmation":confirmation});
    v.require_agent_insert(&p)?;
    let mut rows = ledger(v)?;
    let item = rows
        .iter_mut()
        .find(|x| x["runId"] == r["runId"])
        .ok_or_else(|| Error::new(404, "Agent run not found."))?;
    item["state"] = json!("pending_report");
    item["proposalId"] = p["id"].clone();
    save(v, &rows)?;
    Ok(p)
}
pub fn review(v: &mut Vault, run_id: &str, approve: bool) -> Result<Value> {
    let mut rows = ledger(v)?;
    let index = rows
        .iter()
        .position(|r| r["runId"] == run_id)
        .ok_or_else(|| Error::new(404, "Agent run not found."))?;
    let r = rows[index].clone();
    if r["state"] != "pending_report" {
        return Err(Error::new(409, "This agent run has no pending report."));
    }
    let proposal_id = r["proposalId"]
        .as_str()
        .ok_or_else(|| Error::new(409, "Invalid agent report."))?;
    let p = v.proposal(proposal_id)?;
    if p["agentRunId"] != r["runId"] {
        return Err(Error::new(409, "Invalid agent report."));
    }
    if approve {
        if p["connectionId"].as_str().unwrap_or("owner") != r["actor"].as_str().unwrap_or("")
            || fact(&p["content"], "agent_id") != r["agentId"].as_str().map(str::to_owned)
            || fact(&p["content"], "agent_run_id") != r["runId"].as_str().map(str::to_owned)
            || fact(&p["content"], "agent_revision")
                != r["agentRevision"].as_str().map(str::to_owned)
            || report_context(&p["content"]).map(|cs| revision_map(&cs))
                != Some(revision_map(r["context"].as_array().unwrap()))
        {
            return Err(Error::new(409, "Run/proposal provenance is invalid."));
        }
        current_frozen(v, &r)?;
        if let Some(connection_id) = p["connectionId"].as_str() {
            let g = v.connection(connection_id)?;
            if g["revoked"] == true
                || ["read", "run", "propose"].iter().any(|need| {
                    !g["permissions"]
                        .as_array()
                        .map(|x| x.iter().any(|p| p.as_str() == Some(need)))
                        .unwrap_or(false)
                })
            {
                return Err(Error::new(
                    403,
                    "The report connection is no longer authorized.",
                ));
            }
            scope(
                Some(&g),
                &agent(v, r["agentId"].as_str().unwrap_or(""))?,
                r["context"].as_array().map(Vec::as_slice).unwrap_or(&[]),
            )?;
        }
    }
    let out = v.review_agent_inner(proposal_id, approve)?;
    rows[index]["state"] = json!(if approve { "approved" } else { "rejected" });
    if approve {
        rows[index]["reportNodeId"] = out["node"]["id"].clone();
        rows[index]["reportRevision"] = out["node"]["revisionId"].clone();
    }
    save(v, &rows)?;
    Ok(
        json!({"runId":run_id,"status":if approve{"approved"}else{"rejected"},"proposal":out["proposal"],"node":out["node"]}),
    )
}
pub fn abandon(v: &mut Vault, run_id: &str) -> Result<Value> {
    let mut rows = ledger(v)?;
    let r = rows
        .iter_mut()
        .find(|r| r["runId"] == run_id)
        .ok_or_else(|| Error::new(404, "Agent run not found."))?;
    if r["state"] != "running" {
        return Err(Error::new(
            409,
            "Only a running agent run can be abandoned.",
        ));
    }
    r["state"] = json!("abandoned");
    save(v, &rows)?;
    Ok(json!({"abandoned":true}))
}
/// Owner-only history cleanup; callers persist this through Vault::mutate.
/// Portable report nodes and all pending/running work remain untouched.
pub fn prune(v: &mut Vault) -> Result<Value> {
    let mut rows = ledger(v)?;
    let before = rows.len();
    rows.retain(|r| {
        !matches!(
            effective(r),
            "approved" | "rejected" | "abandoned" | "expired"
        )
    });
    let removed = before - rows.len();
    save(v, &rows)?;
    Ok(json!({"removed":removed}))
}
pub fn owner_list(v: &Vault) -> Result<Value> {
    let rows = ledger(v)?;
    let out = rows
        .into_iter()
        .map(|r| {
            let status = if r["state"] == "running" && !active(&r) {
                json!("expired")
            } else {
                r["state"].clone()
            };
            let proposal = r["proposalId"]
                .as_str()
                .and_then(|id| v.proposal(id).ok())
                .filter(|p| p["status"] == "pending");
            let mut out = public_run(&r);
            out["status"] = status;
            out["proposal"] = json!(proposal);
            out
        })
        .collect::<Vec<_>>();
    Ok(json!({"runs":out}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Engine;

    fn fixture() -> (tempfile::TempDir, Engine, String, Value) {
        let dir = tempfile::TempDir::new().unwrap();
        let mut e = Engine::new(dir.path().join("agents.alve")).unwrap();
        let owner = e
            .vault
            .unlock("synthetic lease test passphrase", true)
            .unwrap()["token"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut facts = Vec::new();
        for (key, value) in [
            ("mission", "Review"),
            ("method", "Read"),
            ("escalation", "Ask"),
            ("firstAssignment", "Check"),
            ("understanding", "I understand"),
            ("phase", "ready"),
        ] {
            facts.push(tf(&format!("agent_{key}"), json!(value)));
        }
        let a = e.request("POST", "/api/nodes", &json!({"title":"Agent","type":"memory","kind":"record","tags":["alve-agent"],"facts":facts}), &owner).unwrap();
        (dir, e, owner, a)
    }

    fn expire(e: &mut Engine, id: &Value) {
        e.vault
            .mutate(|v| {
                let mut runs = ledger(v)?;
                runs.iter_mut().find(|r| &r["runId"] == id).unwrap()["expiresAt"] =
                    json!((Utc::now() - chrono::Duration::seconds(1)).to_rfc3339());
                save(v, &runs)
            })
            .unwrap();
    }

    #[test]
    fn malformed_ledger_is_not_silently_replaced() {
        let (_dir, mut e, owner, a) = fixture();
        e.vault
            .mutate(|v| v.set_meta(KEY, &json!({"unexpected":"history"})))
            .unwrap();
        let before = e.vault.meta(KEY).unwrap();
        let error = e
            .request(
                "POST",
                "/api/ai/agent-briefing",
                &json!({"agentId":a["id"],"requestId":"first"}),
                &owner,
            )
            .unwrap_err();
        assert_eq!(error.status, 409);
        assert_eq!(e.vault.meta(KEY).unwrap(), before);
    }

    #[test]
    fn expired_run_cannot_replay_but_pending_report_can_be_approved() {
        let (_dir, mut e, owner, a) = fixture();
        let body = json!({"agentId":a["id"],"requestId":"first"});
        let first = e
            .request("POST", "/api/ai/agent-briefing", &body, &owner)
            .unwrap();
        expire(&mut e, &first["run"]["runId"]);
        assert_eq!(
            e.request("POST", "/api/ai/agent-briefing", &body, &owner)
                .unwrap_err()
                .status,
            409
        );
        assert_eq!(
            e.request("GET", "/api/ai/agent-assignments", &json!({}), &owner)
                .unwrap()["assignments"][0]["status"],
            "expired"
        );
        let second = e
            .request(
                "POST",
                "/api/ai/agent-briefing",
                &json!({"agentId":a["id"],"requestId":"second"}),
                &owner,
            )
            .unwrap();
        let preview = e.request("POST", "/api/ai/agent-reports/prepare", &json!({"runId":second["run"]["runId"],"report":{"workPerformed":"Read","result":"Unknown","nextAction":"Ask","outcome":"partial","nextFollowUp":(Utc::now()+chrono::Duration::days(1)).to_rfc3339()}}), &owner).unwrap();
        e.request("POST", "/api/ai/agent-reports/submit", &json!({"reviewToken":preview["reviewToken"],"confirmation":{"concise":true,"accurateToSource":true,"structured":true,"userConfirmed":true,"sourceBasis":"inference","basis":"Synthetic","uncertainties":"Unknown"}}), &owner).unwrap();
        expire(&mut e, &second["run"]["runId"]);
        e.request(
            "POST",
            &format!(
                "/api/agent-runs/{}/approve-report",
                second["run"]["runId"].as_str().unwrap()
            ),
            &json!({}),
            &owner,
        )
        .unwrap();
        assert_eq!(
            e.request("GET", "/api/ai/agent-assignments", &json!({}), &owner)
                .unwrap()["assignments"][0]["status"],
            "not_due"
        );
        let third = e
            .request(
                "POST",
                "/api/ai/agent-briefing",
                &json!({"agentId":a["id"],"requestId":"third"}),
                &owner,
            )
            .unwrap();
        let nodes = e.vault.graph().unwrap()["nodes"].clone();
        assert_eq!(e.vault.mutate(prune).unwrap()["removed"], 2);
        assert_eq!(e.vault.graph().unwrap()["nodes"], nodes);
        let remaining = ledger(&e.vault).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0]["runId"], third["run"]["runId"]);
    }

    #[test]
    fn failed_mutation_rolls_back_lease_state() {
        let (_dir, mut e, owner, a) = fixture();
        let run = e
            .request(
                "POST",
                "/api/ai/agent-briefing",
                &json!({"agentId":a["id"],"requestId":"first"}),
                &owner,
            )
            .unwrap();
        let before = ledger(&e.vault).unwrap();
        let failed: Result<()> = e.vault.mutate(|v| {
            abandon(v, run["run"]["runId"].as_str().unwrap())?;
            Err(Error::new(413, "Synthetic persistence boundary failure"))
        });
        assert!(failed.is_err());
        assert_eq!(ledger(&e.vault).unwrap(), before);
    }
}
