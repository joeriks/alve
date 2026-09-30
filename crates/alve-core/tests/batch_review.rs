use alve_core::api::Engine;
use serde_json::{json, Value};
use tempfile::TempDir;

const PASSWORD: &str = "a test passphrase that is long enough";

fn engine() -> (TempDir, Engine, String) {
    let dir = TempDir::new().unwrap();
    let mut engine = Engine::new(dir.path().join("vault.alve")).unwrap();
    let owner = engine.vault.unlock(PASSWORD, true).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    (dir, engine, owner)
}

fn content(title: &str) -> Value {
    json!({"title":title,"body":"Body","type":"memory","kind":"record","tags":["kept"],"facts":[],"references":[]})
}

fn pending(engine: &mut Engine, title: &str, grant: Option<&Value>) -> String {
    engine
        .vault
        .mutate(|vault| vault.propose(&json!({"content":content(title)}), grant))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn five_approvals_can_create_explicit_owner_group() {
    let (_dir, mut engine, owner) = engine();
    let ids = (0..5)
        .map(|n| pending(&mut engine, &format!("memory {n}"), None))
        .collect::<Vec<_>>();
    let result = engine
        .request(
            "POST",
            "/api/proposals/review-batch",
            &json!({"proposalIds":ids,"action":"approve","groupTitle":"Import set"}),
            &owner,
        )
        .unwrap();
    assert_eq!(result["reviews"].as_array().unwrap().len(), 5);
    assert_eq!(result["group"]["type"], "project");
    assert_eq!(result["group"]["kind"], "record");
    assert_eq!(result["relations"].as_array().unwrap().len(), 5);
    for relation in result["relations"].as_array().unwrap() {
        assert_eq!(relation["type"], "belongs_to");
        assert_eq!(relation["toId"], result["group"]["id"]);
    }
    let graph = engine.vault.graph().unwrap();
    assert!(
        graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["tags"] == json!(["kept"]))
            .count()
            >= 5
    );
}

#[test]
fn stale_second_review_rolls_back_the_first() {
    let (_dir, mut engine, owner) = engine();
    let node = engine
        .vault
        .mutate(|vault| vault.add_node(&content("original"), None, None, "user"))
        .unwrap();
    let update = engine.vault.mutate(|vault| vault.propose(&json!({"action":"update","nodeId":node["id"],"expectedRevision":node["revisionId"],"content":content("update")}), None)).unwrap();
    engine
        .vault
        .mutate(|vault| {
            vault.add_node(
                &content("changed"),
                node["id"].as_str(),
                Some(&[node["revisionId"].as_str().unwrap().to_owned()]),
                "user",
            )
        })
        .unwrap();
    let create = pending(&mut engine, "other", None);
    let before = engine.vault.graph().unwrap();
    assert!(engine
        .request(
            "POST",
            "/api/proposals/review-batch",
            &json!({"proposalIds":[create,update["id"]],"action":"approve"}),
            &owner
        )
        .is_err());
    assert_eq!(engine.vault.graph().unwrap(), before);
}

#[test]
fn revoked_second_proposal_rolls_back_earlier_approval() {
    let (_dir, mut engine, owner) = engine();
    let first = pending(&mut engine, "first", None);
    let node = engine
        .vault
        .mutate(|vault| vault.add_node(&content("scope"), None, None, "user"))
        .unwrap();
    let connection = engine
        .vault
        .mutate(|vault| {
            vault.grant(&json!({"name":"AI","nodeIds":[node["id"]],"permissions":["propose"]}))
        })
        .unwrap();
    let grant = engine
        .vault
        .auth(
            connection["token"].as_str().unwrap(),
            false,
            Some("propose"),
            None,
        )
        .unwrap()
        .unwrap();
    let second = pending(&mut engine, "second", Some(&grant));
    engine
        .vault
        .mutate(|vault| vault.revoke(connection["connection"]["id"].as_str().unwrap()))
        .unwrap();
    let before = engine.vault.graph().unwrap();
    assert!(engine
        .request(
            "POST",
            "/api/proposals/review-batch",
            &json!({"proposalIds":[first,second],"action":"approve"}),
            &owner
        )
        .is_err());
    assert_eq!(engine.vault.graph().unwrap(), before);
}

#[test]
fn persistence_failure_rolls_back_reviews_and_grouping() {
    let (dir, mut engine, owner) = engine();
    let first = pending(&mut engine, "first", None);
    let second = pending(&mut engine, "second", None);
    let before = engine.vault.graph().unwrap();
    let vault_path = dir.path().join("vault.alve");
    let backup_path = dir.path().join("vault-backup.alve");
    std::fs::rename(&vault_path, &backup_path).unwrap();
    std::fs::create_dir(&vault_path).unwrap();
    let result = engine.request(
        "POST",
        "/api/proposals/review-batch",
        &json!({"proposalIds":[first,second],"action":"approve","groupTitle":"temporary group"}),
        &owner,
    );
    assert!(result.is_err());
    assert_eq!(engine.vault.graph().unwrap(), before);
    std::fs::remove_dir(&vault_path).unwrap();
    std::fs::rename(&backup_path, &vault_path).unwrap();
}

#[test]
fn duplicates_missing_and_ai_tokens_cannot_batch_review() {
    let (_dir, mut engine, owner) = engine();
    let id = pending(&mut engine, "one", None);
    assert!(engine
        .request(
            "POST",
            "/api/proposals/review-batch",
            &json!({"proposalIds":[id,id],"action":"approve"}),
            &owner
        )
        .is_err());
    assert!(engine
        .request(
            "POST",
            "/api/proposals/review-batch",
            &json!({"proposalIds":["missing"],"action":"approve"}),
            &owner
        )
        .is_err());
    let node = engine
        .vault
        .mutate(|vault| vault.add_node(&content("scope"), None, None, "user"))
        .unwrap();
    let connection = engine
        .vault
        .mutate(|vault| {
            vault.grant(&json!({"name":"AI","nodeIds":[node["id"]],"permissions":["propose"]}))
        })
        .unwrap();
    assert_eq!(
        engine
            .request(
                "POST",
                "/api/proposals/review-batch",
                &json!({"proposalIds":[id],"action":"reject"}),
                connection["token"].as_str().unwrap()
            )
            .unwrap_err()
            .status,
        403
    );
}

#[test]
fn malformed_batch_shapes_are_errors_not_panics() {
    let (_dir, mut engine, owner) = engine();
    for body in [
        json!([]),
        json!({"proposalIds":[],"action":"approve"}),
        json!({"proposalIds":["x"],"action":"delete"}),
        json!({"proposalIds":["x"],"action":"reject","groupTitle":"no"}),
    ] {
        assert!(engine
            .request("POST", "/api/proposals/review-batch", &body, &owner)
            .is_err());
    }
}
