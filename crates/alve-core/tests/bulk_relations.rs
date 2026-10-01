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

fn node(engine: &mut Engine, title: &str) -> Value {
    engine
        .vault
        .mutate(|vault| {
            vault.add_node(
                &json!({"title":title,"body":"Body","type":"memory","kind":"record","tags":[],"facts":[],"references":[]}),
                None,
                None,
                "user",
            )
        })
        .unwrap()
}

fn batch(sources: &[&Value], target: &Value) -> Value {
    let mut expected = serde_json::Map::new();
    for endpoint in sources.iter().copied().chain(std::iter::once(target)) {
        expected.insert(
            endpoint["id"].as_str().unwrap().to_owned(),
            endpoint["revisionId"].clone(),
        );
    }
    json!({"nodeIds":sources.iter().map(|node|node["id"].clone()).collect::<Vec<_>>(),"toId":target["id"],"type":"related_to","expectedRevisions":expected})
}

#[test]
fn bulk_relations_are_atomic_stale_safe_and_idempotent() {
    let (_dir, mut engine, owner) = engine();
    let first = node(&mut engine, "first");
    let second = node(&mut engine, "second");
    let target = node(&mut engine, "target");
    let request = batch(&[&first, &second], &target);
    let created = engine
        .request("POST", "/api/relations/batch", &request, &owner)
        .unwrap();
    assert_eq!(created["relations"].as_array().unwrap().len(), 2);
    assert_eq!(created["skipped"], 0);
    let repeated = engine
        .request("POST", "/api/relations/batch", &request, &owner)
        .unwrap();
    assert_eq!(repeated["relations"].as_array().unwrap().len(), 0);
    assert_eq!(repeated["skipped"], 2);

    let stale = node(&mut engine, "stale");
    let stale_request = batch(&[&first, &stale], &target);
    engine
        .vault
        .mutate(|vault| {
            vault.add_node(
                &json!({"title":"changed","body":"Body","type":"memory","kind":"record","tags":[],"facts":[],"references":[]}),
                stale["id"].as_str(),
                Some(&[stale["revisionId"].as_str().unwrap().to_owned()]),
                "user",
            )
        })
        .unwrap();
    let before = engine.vault.graph().unwrap();
    assert_eq!(
        engine
            .request("POST", "/api/relations/batch", &stale_request, &owner)
            .unwrap_err()
            .status,
        409
    );
    assert_eq!(engine.vault.graph().unwrap(), before);
}

#[test]
fn bulk_relations_require_owner_and_exact_active_revisions() {
    let (_dir, mut engine, owner) = engine();
    let source = node(&mut engine, "source");
    let target = node(&mut engine, "target");
    let request = batch(&[&source], &target);
    let connection = engine
        .vault
        .mutate(|vault| {
            vault.grant(&json!({"name":"limited","nodeIds":[source["id"],target["id"]],"permissions":["read"]}))
        })
        .unwrap();
    assert_eq!(
        engine
            .request(
                "POST",
                "/api/relations/batch",
                &request,
                connection["token"].as_str().unwrap(),
            )
            .unwrap_err()
            .status,
        403
    );
    let mut malformed = request.clone();
    malformed["expectedRevisions"]
        .as_object_mut()
        .unwrap()
        .remove(source["id"].as_str().unwrap());
    assert_eq!(
        engine
            .request("POST", "/api/relations/batch", &malformed, &owner)
            .unwrap_err()
            .status,
        400
    );
}
