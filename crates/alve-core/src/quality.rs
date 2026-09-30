use crate::{validation::node_content, Error, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, RngCore};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

struct Ticket {
    payload: Value,
    actor: String,
    expires: Instant,
}
#[derive(Default)]
pub struct QualityGate {
    tickets: HashMap<String, Ticket>,
}
impl QualityGate {
    pub fn clear(&mut self) {
        self.tickets.clear();
    }
    pub fn prepare(&mut self, data: &Value, actor: &str) -> Result<Value> {
        let payload = self.prepare_payload(data)?;
        let token = self.insert(payload.clone(), actor)?;
        Ok(
            json!({"status":"confirmation_required","reviewToken":token,"content":payload["content"],
            "action":payload["action"],"nodeId":payload.get("nodeId"),"expectedRevision":payload.get("expectedRevision"),"expiresInSeconds":600,
            "checks":{"lengthWithinLimit":true,"explicitCategory":true,"typedFactsValid":true,"factualTruthVerified":false},
            "instructions":[
                "For several related memories, collect all prepared previews and ask once for explicit confirmation of the complete set, including tags. Each submitted proposal must match its preview. Never include later or changed items in that confirmation.",
                "This is how the information will be stored. Show this to the user and request confirmation.",
                "Show the exact heading, content, categories, tags (or explicitly no tags), facts, references and qualifications. Request explicit user confirmation. Do not invent or automatically confirm user agreement. If the client cannot ask the user, stop here.",
                "Confirm concise: one useful memory, no transcript or repeated explanation. Confirm accurateToSource: faithfully preserve exact dates, amounts, units and qualifications; this is not proof of truth. Confirm structured: correct categories and typed hard data.",
                "State sourceBasis, a brief basis explanation and uncertainties. Mark inference and unknowns in the memory itself. User changes require a new preparation.",
                "Updates replace the complete node content. Read the current node and preserve fields unless the user requests removal. Submission creates only a proposal; owner approval in Alve remains required."
            ]}),
        )
    }
    fn prepare_payload(&self, data: &Value) -> Result<Value> {
        let object = data
            .as_object()
            .ok_or_else(|| Error::new(422, "Invalid proposal."))?;
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "action" | "content" | "nodeId" | "expectedRevision"
            )
        }) {
            return Err(Error::new(
                422,
                "Supply only action, content, nodeId, and expectedRevision.",
            ));
        }
        let content = &data["content"];
        if content.get("type").is_none() || content.get("kind").is_none() {
            return Err(Error::new(
                422,
                "Categorize content explicitly with type and kind.",
            ));
        }
        let clean = node_content(content)?;
        let title = clean["title"].as_str().unwrap_or_default();
        let body = clean["body"].as_str().unwrap_or_default();
        if title.chars().count() > 120
            || body.chars().count() > 2000
            || body.split_whitespace().count() > 300
        {
            return Err(Error::new(422,"Condense into one memory: heading at most 120 characters; body at most 300 words and 2,000 characters. Use references for longer material."));
        }
        let action = crate::validation::enum_field(
            data.as_object()
                .ok_or_else(|| Error::new(400, "Invalid proposal."))?,
            "action",
            "create",
        )?;
        if !["create", "update"].contains(&action) {
            return Err(Error::new(400, "Use create or update."));
        }
        if action == "create"
            && (object.contains_key("nodeId") || object.contains_key("expectedRevision"))
        {
            return Err(Error::new(
                422,
                "Create proposals must not include nodeId or expectedRevision.",
            ));
        }
        let mut payload = json!({"action":action,"content":clean});
        if action == "update" {
            for key in ["nodeId", "expectedRevision"] {
                let value = data[key]
                    .as_str()
                    .filter(|s| !s.trim().is_empty() && s.len() <= 100)
                    .ok_or_else(|| {
                        Error::new(400, "Update requires nodeId and expectedRevision.")
                    })?;
                payload[key] = json!(value);
            }
        }
        Ok(payload)
    }
    fn insert(&mut self, payload: Value, actor: &str) -> Result<String> {
        self.tickets.retain(|_, t| t.expires > Instant::now());
        if self.tickets.len() >= 128 {
            return Err(Error::new(429, "Too many active reviews. Wait for expiry."));
        }
        let mut random = [0u8; 32];
        OsRng.fill_bytes(&mut random);
        let token = URL_SAFE_NO_PAD.encode(random);
        self.tickets.insert(
            token.clone(),
            Ticket {
                payload,
                actor: actor.to_owned(),
                expires: Instant::now() + Duration::from_secs(600),
            },
        );
        Ok(token)
    }
    pub fn prepare_batch(&mut self, data: &Value, actor: &str) -> Result<Value> {
        let object = data
            .as_object()
            .ok_or_else(|| Error::new(422, "Supply proposals and groupTitle only."))?;
        if object.len() != 2
            || !object.contains_key("proposals")
            || !object.contains_key("groupTitle")
        {
            return Err(Error::new(422, "Supply proposals and groupTitle only."));
        }
        let proposals = data["proposals"]
            .as_array()
            .filter(|p| (2..=50).contains(&p.len()))
            .ok_or_else(|| Error::new(422, "Prepare 2 to 50 proposals."))?;
        let title = crate::validation::text(data.get("groupTitle"), "group title", 200, true)?;
        let mut payloads = Vec::new();
        let mut updates = std::collections::HashSet::new();
        for proposal in proposals {
            let payload = self.prepare_payload(proposal)?;
            if payload["action"] == "update" && !updates.insert(payload["nodeId"].clone()) {
                return Err(Error::new(
                    422,
                    "A batch cannot update the same memory twice.",
                ));
            }
            payloads.push(payload);
        }
        let mut random = [0u8; 16];
        OsRng.fill_bytes(&mut random);
        let batch_id = URL_SAFE_NO_PAD.encode(random);
        let batch = json!({"batchId":batch_id,"title":title,"proposals":payloads,"group":{"title":title,"type":"project","kind":"record","body":"Groups the selected memories.","tags":[],"facts":[],"references":[],"status":"active","relationType":"belongs_to","relationDirection":"member_to_group"}});
        let token = self.insert(json!({"batch":batch.clone()}), actor)?;
        Ok(
            json!({"status":"confirmation_required","reviewToken":token,"batch":batch,"checks":{"lengthWithinLimit":true,"explicitCategory":true,"typedFactsValid":true,"factualTruthVerified":false},"instructions":["Show the exact complete batch preview, including tags and the planned owner-created project group and belongs_to links, then request explicit confirmation."],"expiresInSeconds":600}),
        )
    }
    pub fn confirmed(&self, data: &Value, actor: &str) -> Result<(Value, Value)> {
        let obj = data
            .as_object()
            .ok_or_else(|| Error::new(422, "Use reviewToken and confirmation."))?;
        if obj.len() != 2 || !obj.contains_key("reviewToken") || !obj.contains_key("confirmation") {
            return Err(Error::new(422,"Prepare first; submit only reviewToken and confirmation. Changes require a new preview."));
        }
        let token = data["reviewToken"]
            .as_str()
            .ok_or_else(|| Error::new(422, "Invalid review token."))?;
        let ticket = self
            .tickets
            .get(token)
            .filter(|t| t.actor == actor && t.expires > Instant::now())
            .ok_or_else(|| Error::new(409, "Review unavailable or expired. Prepare again."))?;
        if ticket.payload.get("batch").is_some() {
            return Err(Error::new(422, "Use submit-batch for a prepared batch."));
        }
        let confirmation =
            self.validate_confirmation(&data["confirmation"], &[ticket.payload.clone()])?;
        Ok((ticket.payload.clone(), confirmation))
    }
    pub fn confirmed_batch(&self, data: &Value, actor: &str) -> Result<(Value, Value)> {
        let obj = data
            .as_object()
            .ok_or_else(|| Error::new(422, "Prepare a batch first."))?;
        if obj.len() != 2 || !obj.contains_key("reviewToken") || !obj.contains_key("confirmation") {
            return Err(Error::new(422, "Prepare a batch first."));
        }
        let token = data["reviewToken"]
            .as_str()
            .ok_or_else(|| Error::new(422, "Invalid review token."))?;
        let ticket = self
            .tickets
            .get(token)
            .filter(|t| t.actor == actor && t.expires > Instant::now())
            .ok_or_else(|| Error::new(409, "Review unavailable or expired. Prepare again."))?;
        let batch = ticket
            .payload
            .get("batch")
            .and_then(Value::as_object)
            .ok_or_else(|| Error::new(422, "Prepare a batch first."))?;
        let proposals = batch["proposals"]
            .as_array()
            .ok_or_else(|| Error::new(422, "Prepare a batch first."))?;
        let confirmation = self.validate_confirmation(&data["confirmation"], proposals)?;
        Ok((Value::Object(batch.clone()), confirmation))
    }
    fn validate_confirmation(&self, c: &Value, payloads: &[Value]) -> Result<Value> {
        if ["concise", "accurateToSource", "structured", "userConfirmed"]
            .iter()
            .any(|k| c[*k] != true)
        {
            return Err(Error::new(422,"Obtain explicit user confirmation of this preview and confirm concise, accurateToSource, structured and userConfirmed as true."));
        }
        let source = c["sourceBasis"]
            .as_str()
            .filter(|s| ["user_statement", "reference", "inference", "unknown"].contains(s))
            .ok_or_else(|| {
                Error::new(
                    422,
                    "State sourceBasis: user_statement, reference, inference or unknown.",
                )
            })?;
        let basis = c["basis"]
            .as_str()
            .filter(|s| !s.trim().is_empty() && s.chars().count() <= 500)
            .ok_or_else(|| {
                Error::new(
                    422,
                    "Supply a source basis explanation of at most 500 characters.",
                )
            })?;
        let uncertainties = c["uncertainties"]
            .as_str()
            .filter(|s| s.chars().count() <= 1000)
            .ok_or_else(|| Error::new(422, "Supply uncertainties, or an empty string."))?;
        if source == "reference"
            && payloads.iter().any(|p| {
                p["content"]["references"]
                    .as_array()
                    .map_or(true, |a| a.is_empty())
            })
        {
            return Err(Error::new(
                422,
                "Reference-based memories need source references. Prepare again.",
            ));
        }
        Ok(
            json!({"concise":true,"accurateToSource":true,"structured":true,"userConfirmed":true,"sourceBasis":source,"basis":basis.trim(),"uncertainties":uncertainties.trim()}),
        )
    }
    pub fn consume(&mut self, token: &str) {
        self.tickets.remove(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_bound_to_actor_and_payload() {
        let mut g = QualityGate::default();
        let p = g
            .prepare(
                &json!({"content":{"title":"Prefer mornings","type":"memory","kind":"preference"}}),
                "a",
            )
            .unwrap();
        let mut d = json!({"reviewToken":p["reviewToken"],"confirmation":{"concise":true,"accurateToSource":true,"structured":true,"userConfirmed":true,"sourceBasis":"user_statement","basis":"User said this.","uncertainties":""}});
        assert!(g.confirmed(&d, "b").is_err());
        assert!(g.confirmed(&d, "a").is_ok());
        d["content"] = json!({"title":"Changed"});
        assert!(g.confirmed(&d, "a").is_err());
        d.as_object_mut().unwrap().remove("content");
        d["confirmation"]["userConfirmed"] = json!(false);
        assert!(g.confirmed(&d, "a").is_err());
        g.consume(p["reviewToken"].as_str().unwrap());
        assert!(g.confirmed(&d, "a").is_err());
    }
}
