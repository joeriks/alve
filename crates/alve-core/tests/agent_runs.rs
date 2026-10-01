use alve_core::api::Engine;
use chrono::{Duration, Utc};
use serde_json::{json, Value};
use tempfile::TempDir;

const PASSWORD: &str = "synthetic agent workflow passphrase";

struct Fixture {
    _dir: TempDir,
    e: Engine,
    owner: String,
    token: String,
    connection: Value,
    agent: Value,
    a: Value,
    b: Value,
}

fn agent_content(context: &Value) -> Value {
    let mut facts = vec![];
    for (key, value) in [
        ("mission", "Review commitments."),
        ("method", "Read the selected context."),
        ("escalation", "Ask before acting."),
        ("firstAssignment", "Suggest the next step."),
        (
            "understanding",
            "I report uncertainty and request approval.",
        ),
        ("phase", "ready"),
        ("reviewDate", ""),
    ] {
        facts.push(
            json!({"key":format!("agent_{key}"),"label":key,"value":{"type":"text","value":value}}),
        );
    }
    facts.push(json!({"key":"agent_context_1","label":"Context","value":{"type":"text","value":context["id"]}}));
    json!({"title":"Project manager","type":"memory","kind":"record","tags":["alve-agent"],"facts":facts})
}

impl Fixture {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let mut e = Engine::new(dir.path().join("memory.alve")).unwrap();
        let owner = e.vault.unlock(PASSWORD, true).unwrap()["token"]
            .as_str()
            .unwrap()
            .to_owned();
        let a = e
            .request(
                "POST",
                "/api/nodes",
                &json!({"title":"Project A","body":"Private A","type":"project"}),
                &owner,
            )
            .unwrap();
        let b = e
            .request(
                "POST",
                "/api/nodes",
                &json!({"title":"Project B","body":"Private B","type":"project"}),
                &owner,
            )
            .unwrap();
        let agent = e
            .request("POST", "/api/nodes", &agent_content(&a), &owner)
            .unwrap();
        let connection = e.request("POST", "/api/connections", &json!({"name":"AI","nodeIds":[agent["id"],a["id"]],"permissions":["read","run","propose"]}), &owner).unwrap();
        let token = connection["token"].as_str().unwrap().to_owned();
        Self {
            _dir: dir,
            e,
            owner,
            token,
            connection,
            agent,
            a,
            b,
        }
    }
    fn start(&mut self, request: &str) -> Value {
        self.e
            .request(
                "POST",
                "/api/ai/agent-briefing",
                &json!({"agentId":self.agent["id"],"requestId":request}),
                &self.token,
            )
            .unwrap()
    }
    fn prepare(&mut self, run: &Value) -> Value {
        self.e.request("POST", "/api/ai/agent-reports/prepare", &json!({"runId":run["run"]["runId"],"report":{"workPerformed":"Reviewed context.","result":"Deadline remains unknown.","nextAction":"Ask the owner.","outcome":"partial","nextFollowUp":(Utc::now()+Duration::days(1)).to_rfc3339(),"references":[{"title":"Synthetic source","url":"https://example.com/source"}]}}), &self.token).unwrap()
    }
    fn status(&mut self) -> Value {
        self.e
            .request("GET", "/api/ai/agent-assignments", &json!({}), &self.token)
            .unwrap()["assignments"][0]["status"]
            .clone()
    }
    fn review(&mut self, run: &Value, action: &str) -> alve_core::Result<Value> {
        self.e.request(
            "POST",
            &format!(
                "/api/agent-runs/{}/{}",
                run["run"]["runId"].as_str().unwrap(),
                action
            ),
            &json!({}),
            &self.owner,
        )
    }
    fn patch_agent(&mut self, content: Value) {
        let mut data = content;
        data["expectedRevision"] = self.agent["revisionId"].clone();
        self.agent = self
            .e
            .request(
                "PATCH",
                &format!("/api/nodes/{}", self.agent["id"].as_str().unwrap()),
                &data,
                &self.owner,
            )
            .unwrap();
    }
}

fn confirm(preview: &Value) -> Value {
    json!({"reviewToken":preview["reviewToken"],"confirmation":{"concise":true,"accurateToSource":true,"structured":true,"userConfirmed":true,"sourceBasis":"reference","basis":"Synthetic reference, no verified project outcome.","uncertainties":"Deadline unknown."}})
}

#[test]
fn exact_preview_owner_review_portable_handoff_and_persisted_retry() {
    let mut f = Fixture::new();
    assert_eq!(f.status(), "due");
    let run = f.start("first");
    assert_eq!(f.status(), "running");
    assert_eq!(f.start("first"), run);
    assert!(!run.to_string().contains("Private B"));
    f.e.request("POST", "/api/lock", &json!({}), &f.owner)
        .unwrap();
    f.owner =
        f.e.request("POST", "/api/unlock", &json!({"password":PASSWORD}), "")
            .unwrap()["token"]
            .as_str()
            .unwrap()
            .to_owned();
    assert_eq!(f.start("first"), run);
    let preview = f.prepare(&run);
    assert!(preview["content"].is_object());
    let body = confirm(&preview);
    assert!(f
        .e
        .request("POST", "/api/ai/proposals", &body, &f.token)
        .is_err());
    let pending =
        f.e.request("POST", "/api/ai/agent-reports/submit", &body, &f.token)
            .unwrap();
    assert_eq!(pending["proposal"]["content"], preview["content"]);
    assert_eq!(f.status(), "pending_report");
    let pid = pending["proposal"]["id"].as_str().unwrap();
    assert!(f
        .e
        .request(
            "POST",
            &format!("/api/proposals/{pid}/approve"),
            &json!({}),
            &f.owner
        )
        .is_err());
    assert!(f
        .e
        .request(
            "POST",
            "/api/proposals/review-batch",
            &json!({"proposalIds":[pid],"action":"approve"}),
            &f.owner
        )
        .is_err());
    let saved = f.review(&run, "approve-report").unwrap();
    for key in [
        "title",
        "body",
        "type",
        "kind",
        "tags",
        "facts",
        "references",
        "status",
    ] {
        assert_eq!(saved["node"][key], preview["content"][key]);
    }
    assert_eq!(f.status(), "not_due");
    f.e.request("PATCH", &format!("/api/nodes/{}",f.a["id"].as_str().unwrap()), &json!({"expectedRevision":f.a["revisionId"],"body":"A source changed after the approved handoff."}), &f.owner).unwrap();
    assert_eq!(f.status(), "due");
    let next = f.start("next");
    assert_eq!(next["lastReport"]["id"], saved["node"]["id"]);
    assert!(f
        .e
        .request(
            "GET",
            &format!("/api/ai/nodes/{}", saved["node"]["id"].as_str().unwrap()),
            &json!({}),
            &f.token
        )
        .is_err());

    let bundle = f.e.vault.bundle().unwrap();
    let dir = TempDir::new().unwrap();
    let mut restored = Engine::new(dir.path().join("restored.alve")).unwrap();
    let owner = restored
        .vault
        .restore(bundle["bundle"].as_str().unwrap(), PASSWORD)
        .unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        restored
            .request("GET", "/api/agent-runs", &json!({}), &owner)
            .unwrap()["runs"],
        json!([])
    );
    let briefing = restored
        .request(
            "POST",
            "/api/ai/agent-briefing",
            &json!({"agentId":f.agent["id"],"requestId":"restored"}),
            &owner,
        )
        .unwrap();
    assert_eq!(briefing["lastReport"]["id"], saved["node"]["id"]);
}

#[test]
fn two_tickets_and_abandoned_runs_cannot_duplicate_or_revive_reports() {
    let mut f = Fixture::new();
    let run = f.start("first");
    let one = f.prepare(&run);
    let two = f.prepare(&run);
    f.e.request(
        "POST",
        "/api/ai/agent-reports/submit",
        &confirm(&one),
        &f.token,
    )
    .unwrap();
    let before = f.e.vault.graph().unwrap();
    assert!(f
        .e
        .request(
            "POST",
            "/api/ai/agent-reports/submit",
            &confirm(&two),
            &f.token
        )
        .is_err());
    assert_eq!(f.e.vault.graph().unwrap(), before);
    f.review(&run, "reject-report").unwrap();
    let second = f.start("second");
    let preview = f.prepare(&second);
    f.review(&second, "abandon").unwrap();
    let before = f.e.vault.graph().unwrap();
    assert!(f
        .e
        .request(
            "POST",
            "/api/ai/agent-reports/submit",
            &confirm(&preview),
            &f.token
        )
        .is_err());
    assert_eq!(f.e.vault.graph().unwrap(), before);
    assert_eq!(f.status(), "due");
}

#[test]
fn missing_scope_actor_and_permissions_are_denied_and_revocation_blocks_approval() {
    let mut f = Fixture::new();
    for (ids, perms) in [
        (json!([f.agent["id"]]), json!(["read", "run", "propose"])),
        (
            json!([f.agent["id"], f.a["id"]]),
            json!(["read", "propose"]),
        ),
        (json!([f.agent["id"], f.a["id"]]), json!(["run", "propose"])),
        (json!([f.agent["id"], f.a["id"]]), json!(["read", "run"])),
    ] {
        let conn =
            f.e.request(
                "POST",
                "/api/connections",
                &json!({"name":"Restricted","nodeIds":ids,"permissions":perms}),
                &f.owner,
            )
            .unwrap();
        assert!(f
            .e
            .request(
                "POST",
                "/api/ai/agent-briefing",
                &json!({"agentId":f.agent["id"],"requestId":"denied"}),
                conn["token"].as_str().unwrap()
            )
            .is_err());
    }
    let run = f.start("first");
    let preview = f.prepare(&run);
    assert!(f
        .e
        .request(
            "POST",
            "/api/ai/agent-reports/submit",
            &confirm(&preview),
            &f.owner
        )
        .is_err());
    let generic = f.e.request("POST", "/api/ai/proposals/prepare", &json!({"content":{"title":"Generic","type":"memory","kind":"record","references":[{"title":"Source"}]}}), &f.token).unwrap();
    assert!(f
        .e
        .request(
            "POST",
            "/api/ai/agent-reports/submit",
            &confirm(&generic),
            &f.token
        )
        .is_err());
    f.e.request(
        "POST",
        "/api/ai/agent-reports/submit",
        &confirm(&preview),
        &f.token,
    )
    .unwrap();
    f.e.request(
        "DELETE",
        &format!(
            "/api/connections/{}",
            f.connection["connection"]["id"].as_str().unwrap()
        ),
        &json!({}),
        &f.owner,
    )
    .unwrap();
    let before = f.e.vault.graph().unwrap();
    assert!(f.review(&run, "approve-report").is_err());
    assert_eq!(f.e.vault.graph().unwrap(), before);
    f.review(&run, "reject-report").unwrap();
}

#[test]
fn context_revision_changes_block_replay_submission_and_approval() {
    let mut f = Fixture::new();
    let run = f.start("first");
    let preview = f.prepare(&run);
    f.e.request(
        "PATCH",
        &format!("/api/nodes/{}", f.a["id"].as_str().unwrap()),
        &json!({"expectedRevision":f.a["revisionId"],"body":"Updated source"}),
        &f.owner,
    )
    .unwrap();
    assert!(f
        .e
        .request(
            "POST",
            "/api/ai/agent-briefing",
            &json!({"agentId":f.agent["id"],"requestId":"first"}),
            &f.token
        )
        .is_err());
    assert!(f
        .e
        .request(
            "POST",
            "/api/ai/agent-reports/submit",
            &confirm(&preview),
            &f.token
        )
        .is_err());
    f.review(&run, "abandon").unwrap();
    let next = f.start("next");
    let preview = f.prepare(&next);
    f.e.request(
        "POST",
        "/api/ai/agent-reports/submit",
        &confirm(&preview),
        &f.token,
    )
    .unwrap();
    let current = f.e.vault.heads().unwrap()[f.a["id"].as_str().unwrap()][0].clone();
    f.e.request(
        "PATCH",
        &format!("/api/nodes/{}", f.a["id"].as_str().unwrap()),
        &json!({"expectedRevision":current["revisionId"],"body":"Changed again"}),
        &f.owner,
    )
    .unwrap();
    assert!(f.review(&next, "approve-report").is_err());
    f.review(&next, "reject-report").unwrap();
}

#[test]
fn a_handoff_cannot_launder_context_into_a_b_only_assignment() {
    let mut f = Fixture::new();
    let run = f.start("first");
    let preview = f.prepare(&run);
    f.e.request(
        "POST",
        "/api/ai/agent-reports/submit",
        &confirm(&preview),
        &f.token,
    )
    .unwrap();
    f.review(&run, "approve-report").unwrap();
    f.patch_agent(agent_content(&f.b));
    let conn = f.e.request("POST", "/api/connections", &json!({"name":"B only","nodeIds":[f.agent["id"],f.b["id"]],"permissions":["read","run","propose"]}), &f.owner).unwrap();
    f.token = conn["token"].as_str().unwrap().to_owned();
    let next = f.start("b-only");
    assert!(next["lastReport"].is_null());
    assert!(!next.to_string().contains("Private A"));
    let preview = f.prepare(&next);
    let context = preview["content"]["facts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["key"] == "agent_context_1")
        .unwrap();
    let decoded: Value = serde_json::from_str(context["value"]["value"].as_str().unwrap()).unwrap();
    assert_eq!(decoded["id"], f.b["id"]);
}

#[test]
fn typed_followup_dates_and_incomplete_introductions_have_real_statuses() {
    let mut f = Fixture::new();
    let mut content = agent_content(&f.a);
    let date = (Utc::now() + Duration::days(2))
        .format("%Y-%m-%d")
        .to_string();
    for fact in content["facts"].as_array_mut().unwrap() {
        if fact["key"] == "agent_reviewDate" {
            fact["value"] = json!({"type":"date","value":date});
        }
    }
    f.patch_agent(content.clone());
    assert_eq!(f.status(), "not_due");
    for fact in content["facts"].as_array_mut().unwrap() {
        if fact["key"] == "agent_understanding" {
            fact["value"]["value"] = json!("");
        }
    }
    f.patch_agent(content);
    assert_eq!(f.status(), "introduced");
}
