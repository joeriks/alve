use alve_core::{api::Engine, crypto, vault::Vault};
use serde_json::{json, Value};
use std::fs;

const PASSWORD: &str = "synthetic acceptance passphrase";
fn request(e: &mut Engine, method: &str, path: &str, data: Value, token: &str) -> Value {
    e.request(method, path, &data, token).unwrap()
}

#[test]
fn update_lock_revokes_session_and_preserves_acknowledged_memory() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::new(dir.path().join("memory.alve")).unwrap();
    let token = request(
        &mut e,
        "POST",
        "/api/unlock",
        json!({"password":PASSWORD,"create":true}),
        "",
    )["token"]
        .as_str()
        .unwrap()
        .to_owned();
    request(
        &mut e,
        "POST",
        "/api/nodes",
        json!({"title":"Saved before update","body":"A persisted synthetic memory."}),
        &token,
    );
    e.lock_for_update();
    assert_eq!(e.vault.status()["unlocked"], false);
    assert!(e.request("GET", "/api/graph", &json!({}), &token).is_err());
    let reopened = request(
        &mut e,
        "POST",
        "/api/unlock",
        json!({"password":PASSWORD}),
        "",
    )["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(e.request("GET", "/api/graph", &json!({}), &token).is_err());
    let graph = request(&mut e, "GET", "/api/graph", json!({}), &reopened);
    assert_eq!(graph["nodes"][0]["title"], "Saved before update");
}

#[test]
fn encrypted_snapshot_tamper_and_failed_save_preserve_memory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.alve");
    let mut v = Vault::new(path.clone()).unwrap();
    v.unlock(PASSWORD, true).unwrap();
    let n = v
        .mutate(|v| v.add_node(&json!({"title":"Sensitive sample"}), None, None, "user"))
        .unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(!bytes.windows(16).any(|b| b == b"SQLite format 3\0"));
    // Make replacement fail after a valid mutation, preserving the previous snapshot.
    fs::rename(&path, dir.path().join("prior.alve")).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(v
        .mutate(|v| v.add_node(&json!({"title":"Unacknowledged"}), None, None, "user"))
        .is_err());
    assert_eq!(
        v.graph().unwrap()["nodes"][0]["revisionId"],
        n["revisionId"]
    );
    fs::remove_dir(&path).unwrap();
    fs::rename(dir.path().join("prior.alve"), &path).unwrap();
    v.lock();
    assert!(v.unlock("wrong password", false).is_err());
    assert_eq!(v.status()["unlocked"], false);
    let mut damaged = bytes.clone();
    *damaged.last_mut().unwrap() ^= 1;
    fs::write(&path, damaged).unwrap();
    assert!(v.unlock(PASSWORD, false).is_err());
    fs::write(&path, bytes).unwrap();
    v.unlock(PASSWORD, false).unwrap();
    assert_eq!(v.graph().unwrap()["nodes"].as_array().unwrap().len(), 1);
}

#[test]
fn concurrent_proposals_recheck_current_revision_and_revocation() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Engine::new(dir.path().join("memory.alve")).unwrap();
    let token = request(
        &mut e,
        "POST",
        "/api/unlock",
        json!({"password":PASSWORD,"create":true}),
        "",
    )["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let node = request(
        &mut e,
        "POST",
        "/api/nodes",
        json!({"title":"Original"}),
        &token,
    );
    let confirmation = json!({"concise":true,"accurateToSource":true,"structured":true,"userConfirmed":true,"sourceBasis":"user_statement","basis":"User said this.","uncertainties":""});
    let mut ids = Vec::new();
    for title in ["First", "Second"] {
        let preview = request(
            &mut e,
            "POST",
            "/api/ai/proposals/prepare",
            json!({"action":"update","nodeId":node["id"],"expectedRevision":node["revisionId"],"content":{"title":title,"type":"memory","kind":"record"}}),
            &token,
        );
        let proposal = request(
            &mut e,
            "POST",
            "/api/ai/proposals",
            json!({"reviewToken":preview["reviewToken"],"confirmation":confirmation}),
            &token,
        );
        ids.push(proposal["id"].as_str().unwrap().to_owned());
    }
    request(
        &mut e,
        "POST",
        &format!("/api/proposals/{}/approve", ids[0]),
        json!({}),
        &token,
    );
    assert_eq!(
        e.request(
            "POST",
            &format!("/api/proposals/{}/approve", ids[1]),
            &json!({}),
            &token
        )
        .unwrap_err()
        .status,
        409
    );
}

#[test]
fn malformed_bundle_restore_does_not_create_vault() {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.alve");
    let mut v = Vault::new(path.clone()).unwrap();
    let id = "12345678901234567890123456789012";
    let salt = [1u8; 16];
    let key = crypto::derive(PASSWORD, &salt).unwrap();
    let invalid =
        json!({"format":"alve-poc-1","vaultId":id,"revisions":[{"vaultId":id}],"relations":[]});
    let bundle = STANDARD.encode(
        crypto::envelope(
            crypto::BUNDLE,
            id,
            &salt,
            &key,
            invalid.to_string().as_bytes(),
        )
        .unwrap(),
    );
    assert!(v.restore(&bundle, PASSWORD).is_err());
    assert!(!path.exists());
    assert_eq!(v.status()["unlocked"], false);
}

#[test]
fn independent_instances_cannot_open_same_vault() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("memory.alve");
    let first = Vault::new(path.clone()).unwrap();
    assert!(Vault::new(path.clone()).is_err());
    drop(first);
    assert!(Vault::new(path).is_ok());
}
