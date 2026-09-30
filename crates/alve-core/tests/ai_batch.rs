use alve_core::api::Engine;
use serde_json::{json, Value};
use tempfile::TempDir;

fn fixture() -> (TempDir, Engine, String) {
    let dir = TempDir::new().unwrap();
    let mut engine = Engine::new(dir.path().join("memory.alve")).unwrap();
    let owner = engine
        .vault
        .unlock("synthetic batch passphrase", true)
        .unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    (dir, engine, owner)
}
fn items() -> Value {
    json!([{"content":{"title":"First","type":"memory","kind":"record","tags":["responsibility"]}},{"content":{"title":"Second","type":"memory","kind":"record","tags":["responsibility"]}}])
}
fn confirm(token: &Value) -> Value {
    json!({"reviewToken":token,"confirmation":{"concise":true,"accurateToSource":true,"structured":true,"userConfirmed":true,"sourceBasis":"user_statement","basis":"Synthetic statement.","uncertainties":""}})
}
fn prepare(e: &mut Engine, owner: &str) -> Value {
    e.request(
        "POST",
        "/api/ai/proposals/prepare-batch",
        &json!({"proposals":items(),"groupTitle":"Responsibilities"}),
        owner,
    )
    .unwrap()
}

#[test]
fn immutable_batch_requires_complete_owner_review_and_survives_reopen() {
    let (_dir, mut e, owner) = fixture();
    let preview = prepare(&mut e, &owner);
    let body = confirm(&preview["reviewToken"]);
    assert!(e
        .request("POST", "/api/ai/proposals", &body, &owner)
        .is_err());
    let out = e
        .request("POST", "/api/ai/proposals/submit-batch", &body, &owner)
        .unwrap();
    assert!(e
        .request("POST", "/api/ai/proposals/submit-batch", &body, &owner)
        .is_err());
    let ids = out["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].clone())
        .collect::<Vec<_>>();
    let before = e.vault.graph().unwrap();
    assert!(e
        .request(
            "POST",
            &format!("/api/proposals/{}/approve", ids[0].as_str().unwrap()),
            &json!({}),
            &owner
        )
        .is_err());
    assert!(e
        .request(
            "POST",
            "/api/proposals/review-batch",
            &json!({"proposalIds":[ids[0]],"action":"approve"}),
            &owner
        )
        .is_err());
    assert!(e
        .request(
            "POST",
            "/api/proposals/review-batch",
            &json!({"proposalIds":ids,"action":"approve","groupTitle":"Changed"}),
            &owner
        )
        .is_err());
    assert_eq!(before, e.vault.graph().unwrap());
    e.request("POST", "/api/lock", &json!({}), &owner).unwrap();
    let owner = e
        .request(
            "POST",
            "/api/unlock",
            &json!({"password":"synthetic batch passphrase"}),
            "",
        )
        .unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let approved = e
        .request(
            "POST",
            "/api/proposals/review-batch",
            &json!({"proposalIds":ids,"action":"approve"}),
            &owner,
        )
        .unwrap();
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
        assert_eq!(approved["group"][key], preview["batch"]["group"][key]);
    }
    assert_eq!(approved["relations"].as_array().unwrap().len(), 2);
    assert_eq!(approved["group"]["origin"], "user");
    assert_eq!(
        e.vault.graph().unwrap()["nodes"].as_array().unwrap().len(),
        3
    );
}

#[test]
fn actor_scope_revocation_and_grouping_do_not_expand_access() {
    let (_dir, mut e, owner) = fixture();
    let allowed = e
        .request("POST", "/api/nodes", &json!({"title":"Allowed"}), &owner)
        .unwrap();
    let hidden = e
        .request("POST", "/api/nodes", &json!({"title":"Hidden"}), &owner)
        .unwrap();
    let connection = e
        .request(
            "POST",
            "/api/connections",
            &json!({"name":"AI","nodeIds":[allowed["id"]],"permissions":["propose","read"]}),
            &owner,
        )
        .unwrap();
    let token = connection["token"].as_str().unwrap();
    let mut data = items();
    data[1] = json!({"action":"update","nodeId":hidden["id"],"expectedRevision":hidden["revisionId"],"content":{"title":"Hidden update","type":"memory","kind":"record"}});
    assert!(e
        .request(
            "POST",
            "/api/ai/proposals/prepare-batch",
            &json!({"proposals":data,"groupTitle":"Group"}),
            token
        )
        .is_err());
    let preview = prepare(&mut e, token);
    let body = confirm(&preview["reviewToken"]);
    assert!(e
        .request("POST", "/api/ai/proposals/submit-batch", &body, &owner)
        .is_err());
    let out = e
        .request("POST", "/api/ai/proposals/submit-batch", &body, token)
        .unwrap();
    let ids = out["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].clone())
        .collect::<Vec<_>>();
    e.request(
        "DELETE",
        &format!(
            "/api/connections/{}",
            connection["connection"]["id"].as_str().unwrap()
        ),
        &json!({}),
        &owner,
    )
    .unwrap();
    let before = e.vault.graph().unwrap();
    assert!(e
        .request(
            "POST",
            "/api/proposals/review-batch",
            &json!({"proposalIds":ids,"action":"approve"}),
            &owner
        )
        .is_err());
    assert_eq!(before, e.vault.graph().unwrap());
    let group_body = json!({"nodeIds":[allowed["id"],hidden["id"]],"groupTitle":"Owner group"});
    assert!(e
        .request("POST", "/api/nodes/group", &group_body, token)
        .is_err());
    let result = e
        .request("POST", "/api/nodes/group", &group_body, &owner)
        .unwrap();
    assert_eq!(result["relations"].as_array().unwrap().len(), 2);
    assert_eq!(
        e.vault.graph().unwrap()["connections"][0]["nodeIds"],
        json!([allowed["id"]])
    );
}

#[test]
fn stale_later_member_and_disk_failure_roll_back_entire_batch() {
    let (dir, mut e, owner) = fixture();
    let n = e
        .request("POST", "/api/nodes", &json!({"title":"Original"}), &owner)
        .unwrap();
    let mut data = items();
    data[1] = json!({"action":"update","nodeId":n["id"],"expectedRevision":n["revisionId"],"content":{"title":"Update","type":"memory","kind":"record"}});
    let preview = e
        .request(
            "POST",
            "/api/ai/proposals/prepare-batch",
            &json!({"proposals":data,"groupTitle":"Group"}),
            &owner,
        )
        .unwrap();
    let body = confirm(&preview["reviewToken"]);
    let path = dir.path().join("memory.alve");
    let backup = dir.path().join("fixture.alve");
    std::fs::rename(&path, &backup).unwrap();
    std::fs::create_dir(&path).unwrap();
    let before = e.vault.graph().unwrap();
    assert!(e
        .request("POST", "/api/ai/proposals/submit-batch", &body, &owner)
        .is_err());
    assert_eq!(before, e.vault.graph().unwrap());
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(&backup, &path).unwrap();
    let out = e
        .request("POST", "/api/ai/proposals/submit-batch", &body, &owner)
        .unwrap();
    let ids = out["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].clone())
        .collect::<Vec<_>>();
    e.request(
        "PATCH",
        &format!("/api/nodes/{}", n["id"].as_str().unwrap()),
        &json!({"title":"Owner edit","expectedRevision":n["revisionId"]}),
        &owner,
    )
    .unwrap();
    let before = e.vault.graph().unwrap();
    let disk = std::fs::read(&path).unwrap();
    assert!(e
        .request(
            "POST",
            "/api/proposals/review-batch",
            &json!({"proposalIds":ids,"action":"approve"}),
            &owner
        )
        .is_err());
    assert_eq!(before, e.vault.graph().unwrap());
    assert_eq!(disk, std::fs::read(&path).unwrap());
}
