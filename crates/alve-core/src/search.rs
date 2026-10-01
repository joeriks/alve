use crate::{validation::now, Error, Result};
use chrono::{DateTime, FixedOffset};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
pub type Query = BTreeMap<String, Vec<String>>;
fn one<'a>(q: &'a Query, k: &str, default: &'a str) -> Result<&'a str> {
    match q.get(k) {
        None => Ok(default),
        Some(a) if a.len() == 1 => Ok(&a[0]),
        _ => Err(Error::new(400, format!("Specify {k} once."))),
    }
}
fn timestamp(s: &str) -> Result<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(s).map_err(|_| {
        Error::new(
            400,
            "Dates require ISO timestamps with explicit timezone offsets.",
        )
    })
}
pub fn search(graph: &Value, q: &Query) -> Result<Value> {
    let allowed = [
        "q",
        "tag",
        "type",
        "kind",
        "updatedSince",
        "updatedBefore",
        "includeArchived",
        "sort",
        "limit",
        "offset",
    ];
    if q.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(Error::new(400, "Unknown search filter."));
    }
    let words = one(q, "q", "")?.trim().to_lowercase();
    if words.chars().count() > 1000 {
        return Err(Error::new(400, "Search query too long."));
    }
    let tags = q.get("tag").cloned().unwrap_or_default();
    if tags.len() > 20
        || tags
            .iter()
            .any(|s| s.trim().is_empty() || s.chars().count() > 60)
    {
        return Err(Error::new(
            400,
            "Use at most 20 nonempty tags of 60 characters.",
        ));
    }
    let tags: HashSet<_> = tags.iter().map(|s| s.trim().to_lowercase()).collect();
    let typ = one(q, "type", "")?;
    let kind = one(q, "kind", "")?;
    if !typ.is_empty() && !["memory", "project", "person", "event", "document"].contains(&typ)
        || !kind.is_empty()
            && !["decision", "preference", "insight", "commitment", "record"].contains(&kind)
    {
        return Err(Error::new(400, "Unsupported category filter."));
    }
    let s = one(q, "updatedSince", "")?;
    let b = one(q, "updatedBefore", "")?;
    let since = if s.is_empty() {
        None
    } else {
        Some(timestamp(s)?)
    };
    let before = if b.is_empty() {
        None
    } else {
        Some(timestamp(b)?)
    };
    if since.zip(before).is_some_and(|(s, b)| s >= b) {
        return Err(Error::new(400, "updatedSince must precede updatedBefore."));
    }
    let archived = one(q, "includeArchived", "false")?;
    if !["true", "false"].contains(&archived) {
        return Err(Error::new(400, "includeArchived must be true or false."));
    }
    let sort = one(q, "sort", "relevance")?;
    if !["relevance", "updated"].contains(&sort) {
        return Err(Error::new(400, "Sort must be relevance or updated."));
    }
    let limit: usize = one(q, "limit", "20")?
        .parse()
        .map_err(|_| Error::new(400, "Invalid limit."))?;
    let offset: usize = one(q, "offset", "0")?
        .parse()
        .map_err(|_| Error::new(400, "Invalid offset."))?;
    if !(1..=100).contains(&limit) || offset > 5000 {
        return Err(Error::new(400, "Limit must be 1–100 and offset 0–5000."));
    }
    let conflicts = graph["conflicts"]
        .as_array()
        .ok_or_else(|| Error::new(500, "Invalid graph."))?;
    let mut matches = Vec::new();
    for node in graph["nodes"]
        .as_array()
        .ok_or_else(|| Error::new(500, "Invalid graph."))?
    {
        let versions = conflicts
            .iter()
            .find(|c| c["nodeId"] == node["id"])
            .and_then(|c| c["versions"].as_array());
        let fallback = vec![node.clone()];
        let versions = versions.unwrap_or(&fallback);
        let mut best: Option<(i32, i64)> = None;
        for n in versions {
            let updated = timestamp(n["updatedAt"].as_str().unwrap_or(""))?;
            if (!typ.is_empty() && n["type"] != typ)
                || (!kind.is_empty() && n["kind"] != kind)
                || (archived == "false" && n["status"] == "archived")
                || since.is_some_and(|s| updated < s)
                || before.is_some_and(|b| updated >= b)
            {
                continue;
            }
            let ntags: HashSet<_> = n["tags"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_lowercase)
                .collect();
            if !tags.is_subset(&ntags) {
                continue;
            }
            let title = n["title"].as_str().unwrap_or("").to_lowercase();
            let body = n["body"].as_str().unwrap_or("").to_lowercase();
            let facts = n["facts"].to_string().to_lowercase();
            let refs = n["references"].to_string().to_lowercase();
            if !words.is_empty()
                && !title.contains(&words)
                && !body.contains(&words)
                && !ntags.iter().any(|t| t.contains(&words))
                && !facts.contains(&words)
                && !refs.contains(&words)
            {
                continue;
            }
            let score = if words.is_empty() || sort == "updated" {
                0
            } else if title == words {
                100
            } else if title.contains(&words) {
                80
            } else if ntags.iter().any(|t| t.contains(&words)) {
                60
            } else if body.contains(&words) {
                40
            } else {
                20
            };
            let stamp = updated.timestamp_micros();
            best = Some(match best {
                None => (score, stamp),
                Some((s, t)) => (s.max(score), t.max(stamp)),
            });
        }
        if let Some((score, updated)) = best {
            matches.push((score, updated, node.clone()));
        }
    }
    matches.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(a.2["id"].as_str().cmp(&b.2["id"].as_str()))
    });
    let nodes: Vec<_> = matches
        .iter()
        .skip(offset)
        .take(limit)
        .map(|a| a.2.clone())
        .collect();
    let conflicts: Vec<_> = conflicts
        .iter()
        .filter(|c| nodes.iter().any(|n| n["id"] == c["nodeId"]))
        .cloned()
        .collect();
    Ok(
        json!({"vaultId":graph["vaultId"],"nodes":nodes,"conflicts":conflicts,"nextOffset":if offset+limit<matches.len(){Some(offset+limit)}else{None},"asOf":now(),"sort":sort}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn node(id: &str, status: &str, date: &str) -> Value {
        json!({"id":id,"title":"Budget","body":"Concise memory","type":"memory","kind":"decision","status":status,"tags":["Work","Budget"],"facts":[],"references":[],"updatedAt":date})
    }
    fn query(pairs: &[(&str, &str)]) -> Query {
        let mut q = Query::new();
        for (key, value) in pairs {
            q.entry((*key).into()).or_default().push((*value).into());
        }
        q
    }
    #[test]
    fn filters_categories_tags_dates_and_archives_before_pagination() {
        let graph = json!({"vaultId":"v","nodes":[node("a","active","2026-09-29T12:00:00Z"),node("b","active","2026-09-30T12:00:00Z"),node("c","archived","2026-09-30T13:00:00Z")],"conflicts":[]});
        let filters = query(&[
            ("tag", "work"),
            ("tag", "BUDGET"),
            ("kind", "decision"),
            ("updatedSince", "2026-09-29T12:00:00Z"),
            ("updatedBefore", "2026-10-01T00:00:00Z"),
            ("limit", "1"),
        ]);
        let first = search(&graph, &filters).unwrap();
        assert_eq!(first["nodes"][0]["id"], "b");
        assert_eq!(first["nextOffset"], 1);
        let mut next = filters.clone();
        next.insert("offset".into(), vec!["1".into()]);
        assert_eq!(search(&graph, &next).unwrap()["nodes"][0]["id"], "a");
        let mut archive = filters;
        archive.insert("includeArchived".into(), vec!["true".into()]);
        assert_eq!(search(&graph, &archive).unwrap()["nodes"][0]["id"], "c");
    }
    #[test]
    fn search_finds_retained_conflict_versions_and_returns_the_conflict() {
        let a = node("a", "active", "2026-09-30T12:00:00Z");
        let mut b = a.clone();
        b["title"] = json!("Alternative decision");
        let graph =
            json!({"vaultId":"v","nodes":[a],"conflicts":[{"nodeId":"a","versions":[a,b]}]});
        let result = search(&graph, &query(&[("q", "Alternative")])).unwrap();
        assert_eq!(result["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(result["conflicts"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn ambiguous_or_invalid_filters_fail_instead_of_being_ignored() {
        let graph = json!({"vaultId":"v","nodes":[],"conflicts":[]});
        for pairs in [
            vec![("q", "one"), ("q", "two")],
            vec![("limit", "-1")],
            vec![("updatedSince", "2026-09-30")],
            vec![("unknown", "value")],
        ] {
            assert!(search(&graph, &query(&pairs)).is_err());
        }
    }
}
