use chrono::{DateTime, NaiveDate};
use chrono_tz::Tz;

use serde_json::{json, Map, Value};

use crate::{Error, Result};

pub const KINDS: &[&str] = &["decision", "preference", "insight", "commitment", "record"];
pub const TYPES: &[&str] = &["memory", "project", "person", "event", "document"];
pub const RELATIONS: &[&str] = &[
    "belongs_to",
    "based_on",
    "related_to",
    "supersedes",
    "contradicts",
    "fulfills",
];

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn text(value: Option<&Value>, label: &str, maximum: usize, required: bool) -> Result<String> {
    let raw = value
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new(400, format!("Invalid {label}.")))?;
    let trimmed = raw.trim();
    if raw.chars().count() > maximum || (required && trimmed.is_empty()) {
        return Err(Error::new(400, format!("Invalid {label}.")));
    }
    Ok(trimmed.to_owned())
}

pub fn node_content(data: &Value) -> Result<Value> {
    let object = data
        .as_object()
        .ok_or_else(|| Error::new(400, "Node content must be an object."))?;
    let kind = enum_field(object, "kind", "record")?;
    let typ = enum_field(object, "type", "memory")?;
    if !KINDS.contains(&kind) || !TYPES.contains(&typ) {
        return Err(Error::new(400, "Unsupported node type or memory kind."));
    }
    let tags = array(object.get("tags"), "Invalid tags.")?;
    if tags.len() > 20 {
        return Err(Error::new(400, "Invalid tags."));
    }
    let refs = array(object.get("references"), "Invalid references.")?;
    if refs.len() > 20 {
        return Err(Error::new(400, "Invalid references."));
    }
    let status = enum_field(object, "status", "active")?;
    if !["active", "archived"].contains(&status) {
        return Err(Error::new(400, "Invalid node status."));
    }
    let clean_tags = tags
        .iter()
        .map(|tag| text(Some(tag), "tag", 60, true).map(Value::String))
        .collect::<Result<Vec<_>>>()?;
    let clean_refs = refs
        .iter()
        .map(clean_reference)
        .collect::<Result<Vec<_>>>()?;
    let facts = validate_facts(object.get("facts"))?;
    Ok(
        json!({"title":text(object.get("title"),"summary heading",200,true)?,"body":text_or_empty(object.get("body"),"memory body",6000)?,"type":typ,"kind":kind,"tags":clean_tags,"facts":facts,"references":clean_refs,"status":status}),
    )
}

fn array<'a>(value: Option<&'a Value>, message: &str) -> Result<&'a Vec<Value>> {
    match value {
        None => Ok(&EMPTY),
        Some(Value::Array(values)) => Ok(values),
        _ => Err(Error::new(400, message)),
    }
}
static EMPTY: Vec<Value> = Vec::new();

pub fn enum_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    default: &'a str,
) -> Result<&'a str> {
    match object.get(key) {
        None => Ok(default),
        Some(Value::String(value)) => Ok(value),
        _ => Err(Error::new(400, format!("Invalid {key}."))),
    }
}

fn text_or_empty(value: Option<&Value>, label: &str, maximum: usize) -> Result<String> {
    match value {
        None => Ok(String::new()),
        Some(_) => text(value, label, maximum, false),
    }
}

fn clean_reference(reference: &Value) -> Result<Value> {
    let object = reference
        .as_object()
        .ok_or_else(|| Error::new(400, "Invalid reference."))?;
    let mut result = Map::new();
    result.insert(
        "title".into(),
        Value::String(text(object.get("title"), "reference title", 200, true)?),
    );
    if let Some(url) = object.get("url") {
        if !url.is_null() && url != "" {
            let clean = text(Some(url), "reference URL", 2000, true)?;
            if !clean.starts_with("http://") && !clean.starts_with("https://") {
                return Err(Error::new(400, "Reference URLs must use HTTP or HTTPS."));
            }
            result.insert("url".into(), Value::String(clean));
        }
    }
    Ok(Value::Object(result))
}

pub fn validate_facts(value: Option<&Value>) -> Result<Value> {
    let facts = array(value, "Use at most 30 facts per node.")?;
    if facts.len() > 30 {
        return Err(Error::new(400, "Use at most 30 facts per node."));
    }
    let mut keys = std::collections::HashSet::new();
    let mut clean = Vec::new();
    for fact in facts {
        let object = fact
            .as_object()
            .ok_or_else(|| Error::new(400, "Invalid fact."))?;
        let key = text(object.get("key"), "fact key", 80, true)?;
        if !keys.insert(key.clone()) {
            return Err(Error::new(400, "Fact keys must be unique within a node."));
        }
        let label = text(object.get("label"), "fact label", 120, true)?;
        let precision = enum_field(object, "precision", "exact")?;
        if !["exact", "approximate", "estimated"].contains(&precision) {
            return Err(Error::new(400, "Invalid fact precision."));
        }
        let fact_value = match object.get("value") {
            None | Some(Value::Null) => Value::Null,
            Some(value) => clean_fact_value(value)?,
        };
        clean.push(json!({"key":key,"label":label,"value":fact_value,"precision":precision}));
    }
    Ok(Value::Array(clean))
}

fn clean_fact_value(value: &Value) -> Result<Value> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::new(400, "Invalid fact value."))?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::new(400, "Invalid fact type."))?;
    match kind {
        "text" => {
            Ok(json!({"type":"text","value":text(object.get("value"),"text fact",1000,false)?}))
        }
        "boolean" => match object.get("value").and_then(Value::as_bool) {
            Some(value) => Ok(json!({"type":"boolean","value":value})),
            None => Err(Error::new(400, "Invalid fact value.")),
        },
        "date" => {
            let date = text(object.get("value"), "date", 10, true)?;
            if NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                .map(|x| x.to_string() != date)
                .unwrap_or(true)
            {
                return Err(Error::new(400, "Dates must be valid YYYY-MM-DD values."));
            }
            Ok(json!({"type":"date","value":date}))
        }
        "datetime" => {
            let value = text(object.get("value"), "datetime", 80, true)?;
            let zone = text(object.get("timeZone"), "time zone", 80, true)?;
            if DateTime::parse_from_rfc3339(&value).is_err() || zone.parse::<Tz>().is_err() {
                return Err(Error::new(
                    400,
                    "Datetime needs an explicit offset and a valid time zone.",
                ));
            }
            Ok(json!({"type":"datetime","value":value,"timeZone":zone}))
        }
        "money" | "quantity" => {
            let amount = text(object.get("amount"), "decimal amount", 80, true)?;
            if !decimal_string(&amount) {
                return Err(Error::new(
                    400,
                    "Amounts must be finite decimal strings without exponent notation.",
                ));
            }
            if kind == "money" {
                let currency = text(object.get("currency"), "currency", 3, true)?;
                if currency.len() != 3
                    || !currency.is_ascii()
                    || !currency.chars().all(|x| x.is_ascii_uppercase())
                {
                    return Err(Error::new(
                        400,
                        "Currency must be a three-letter uppercase code.",
                    ));
                }
                Ok(json!({"type":"money","amount":amount,"currency":currency}))
            } else {
                Ok(
                    json!({"type":"quantity","amount":amount,"unit":text(object.get("unit"),"unit",40,true)?}),
                )
            }
        }
        _ => Err(Error::new(400, "Unsupported fact type.")),
    }
}

fn decimal_string(amount: &str) -> bool {
    let s = amount.strip_prefix(['+', '-']).unwrap_or(amount);
    let mut dots = 0;
    let mut digits = 0;
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits += 1
        } else if c == '.' {
            dots += 1
        } else {
            return false;
        }
    }
    digits > 0 && dots <= 1
}
