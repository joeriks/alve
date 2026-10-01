use alve_core::{api::Engine, crypto, vault::Vault};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use tempfile::TempDir;

const PASSWORD: &str = "synthetic relation lifecycle passphrase";

fn fixture() -> (TempDir, Engine, String, Value, Value, Value) {
    let dir = TempDir::new().unwrap();
    let mut engine = Engine::new(dir.path().join("source.alve")).unwrap();
    let owner = engine.vault.unlock(PASSWORD, true).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut node = |title| {
        engine
            .vault
            .mutate(|v| v.add_node(&json!({"title":title}), None, None, "user"))
            .unwrap()
    };
    let source = node("source");
    let target = node("target");
    let relation = engine
        .request(
            "POST",
            "/api/relations",
            &json!({"fromId":source["id"],"toId":target["id"],"type":"related_to"}),
            &owner,
        )
        .unwrap();
    (dir, engine, owner, source, target, relation)
}

fn bundle(vault: &Vault) -> String {
    vault.bundle().unwrap()["bundle"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn encoded(payload: &Value) -> String {
    let salt = [23u8; 16];
    let key = crypto::derive(PASSWORD, &salt).unwrap();
    STANDARD.encode(
        crypto::envelope(
            crypto::BUNDLE,
            payload["vaultId"].as_str().unwrap(),
            &salt,
            &key,
            &serde_json::to_vec(payload).unwrap(),
        )
        .unwrap(),
    )
}

#[test]
fn deletion_wins_both_orders_old_replay_and_undo_survive_reopening() {
    let (dir, mut engine, owner, _source, _target, relation) = fixture();
    let old = bundle(&engine.vault);
    let path = format!("/api/relations/{}", relation["id"].as_str().unwrap());
    let deleted = engine.request("DELETE", &path, &json!({}), &owner).unwrap();
    assert_eq!(deleted["deleted"], true);
    assert_eq!(
        engine.request("DELETE", &path, &json!({}), &owner).unwrap(),
        deleted
    );
    let removed = bundle(&engine.vault);
    engine.vault.mutate(|v| v.merge(&old, PASSWORD)).unwrap();
    assert_eq!(engine.vault.graph().unwrap()["relations"], json!([]));
    assert_eq!(
        engine.vault.export().unwrap()["relations"],
        json!([deleted.clone()])
    );
    for (index, (first, second)) in [(&old, &removed), (&removed, &old)].into_iter().enumerate() {
        let mut peer = Vault::new(dir.path().join(format!("peer-{index}.alve"))).unwrap();
        peer.restore(first, PASSWORD).unwrap();
        peer.mutate(|v| v.merge(second, PASSWORD)).unwrap();
        assert_eq!(peer.graph().unwrap()["relations"], json!([]));
        assert_eq!(
            peer.export().unwrap()["relations"],
            json!([deleted.clone()])
        );
        peer.lock();
        peer.unlock(PASSWORD, false).unwrap();
        assert_eq!(
            peer.export().unwrap()["relations"],
            json!([deleted.clone()])
        );
    }
    let restored = engine
        .request("POST", &format!("{path}/restore"), &json!({}), &owner)
        .unwrap();
    assert_ne!(restored["id"], relation["id"]);
    assert_eq!(restored["restoredFrom"], relation["id"]);
    assert_eq!(
        engine
            .request("POST", &format!("{path}/restore"), &json!({}), &owner)
            .unwrap(),
        restored
    );
    for replay in [&old, &removed] {
        engine.vault.mutate(|v| v.merge(replay, PASSWORD)).unwrap();
    }
    engine.vault.lock();
    engine.vault.unlock(PASSWORD, false).unwrap();
    assert_eq!(
        engine.vault.graph().unwrap()["relations"],
        json!([restored])
    );
    assert_eq!(
        engine.vault.export().unwrap()["relations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn malformed_tombstones_and_same_id_replacement_are_rejected_atomically() {
    let (_dir, mut engine, _owner, _source, _target, relation) = fixture();
    let original = engine.vault.export().unwrap();
    for fields in [
        json!({"deleted":true}),
        json!({"deleted":false,"deletedAt":"2026-01-01T00:00:00Z"}),
        json!({"deleted":"true","deletedAt":"2026-01-01T00:00:00Z"}),
        json!({"deleted":true,"deletedAt":"invalid"}),
        json!({"deleted":true,"deletedAt":"2026-01-01T00:00:00Z","type":"belongs_to"}),
        json!({"deleted":true,"deletedAt":"2026-01-01T00:00:00Z","origin":"import"}),
    ] {
        let mut candidate = original.clone();
        candidate["relations"][0]
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        assert!(engine
            .vault
            .mutate(|v| v.merge(&encoded(&candidate), PASSWORD))
            .is_err());
        assert_eq!(engine.vault.export().unwrap(), original);
    }
    let mut deleted = relation.clone();
    deleted["deleted"] = json!(true);
    deleted["deletedAt"] = json!("2026-01-01T00:00:00Z");
    for records in [json!([relation, deleted]), json!([deleted, relation])] {
        let mut candidate = original.clone();
        candidate["relations"] = records;
        engine
            .vault
            .mutate(|v| v.merge(&encoded(&candidate), PASSWORD))
            .unwrap();
        assert_eq!(
            engine.vault.export().unwrap()["relations"],
            json!([deleted.clone()])
        );
    }
    let before = engine.vault.export().unwrap();
    let mut tampered = relation.clone();
    tampered["type"] = json!("belongs_to");
    let mut candidate = original;
    candidate["relations"] = json!([deleted, tampered, relation]);
    assert!(engine
        .vault
        .mutate(|v| v.merge(&encoded(&candidate), PASSWORD))
        .is_err());
    assert_eq!(engine.vault.export().unwrap(), before);
}

#[test]
fn concurrent_deletions_converge_and_save_failure_rolls_back_undo() {
    let (dir, mut engine, _owner, _source, _target, relation) = fixture();
    let mut early = engine.vault.export().unwrap();
    early["relations"][0]["deleted"] = json!(true);
    early["relations"][0]["deletedAt"] = json!("2026-01-01T00:00:00Z");
    let mut late = early.clone();
    late["relations"][0]["deletedAt"] = json!("2026-02-01T00:00:00Z");
    for records in [&late, &early, &late] {
        engine
            .vault
            .mutate(|v| v.merge(&encoded(records), PASSWORD))
            .unwrap();
    }
    assert_eq!(engine.vault.export().unwrap(), early);
    let path = dir.path().join("source.alve");
    std::fs::rename(&path, dir.path().join("saved.alve")).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(engine
        .vault
        .mutate(|v| v.restore_relation(relation["id"].as_str().unwrap()))
        .is_err());
    assert_eq!(engine.vault.export().unwrap(), early);
    std::fs::remove_dir(&path).unwrap();
    std::fs::rename(dir.path().join("saved.alve"), &path).unwrap();
    engine.vault.lock();
    engine.vault.unlock(PASSWORD, false).unwrap();
    assert_eq!(engine.vault.export().unwrap(), early);
}

#[test]
fn single_creation_and_restore_validate_active_nonconflicted_distinct_endpoints() {
    let (dir, mut engine, owner, source, target, relation) = fixture();
    let request = json!({"fromId":source["id"],"toId":target["id"],"type":"related_to"});
    assert_eq!(
        engine
            .request("POST", "/api/relations", &request, &owner)
            .unwrap(),
        relation
    );
    assert_eq!(
        engine
            .request(
                "POST",
                "/api/relations",
                &json!({"fromId":source["id"],"toId":source["id"],"type":"related_to"}),
                &owner
            )
            .unwrap_err()
            .status,
        400
    );
    let old = bundle(&engine.vault);
    engine
        .vault
        .mutate(|v| v.delete_relation(relation["id"].as_str().unwrap()))
        .unwrap();
    let parents = [source["revisionId"].as_str().unwrap().to_owned()];
    let mut archived = source.clone();
    archived["status"] = json!("archived");
    engine
        .vault
        .mutate(|v| v.add_node(&archived, source["id"].as_str(), Some(&parents), "user"))
        .unwrap();
    assert_eq!(
        engine
            .request("POST", "/api/relations", &request, &owner)
            .unwrap_err()
            .status,
        409
    );
    assert_eq!(
        engine
            .vault
            .mutate(|v| v.restore_relation(relation["id"].as_str().unwrap()))
            .unwrap_err()
            .status,
        409
    );
    let mut peer = Vault::new(dir.path().join("conflict.alve")).unwrap();
    peer.restore(&old, PASSWORD).unwrap();
    let mut active = source.clone();
    active["title"] = json!("concurrent");
    peer.mutate(|v| v.add_node(&active, source["id"].as_str(), Some(&parents), "user"))
        .unwrap();
    engine
        .vault
        .mutate(|v| v.merge(&bundle(&peer), PASSWORD))
        .unwrap();
    assert_eq!(
        engine
            .request("POST", "/api/relations", &request, &owner)
            .unwrap_err()
            .status,
        409
    );
}

#[test]
fn relation_lifecycle_routes_require_owner_and_hide_deleted_edges_from_ai() {
    let (_dir, mut engine, owner, source, target, relation) = fixture();
    let connection = engine.vault.mutate(|v| v.grant(&json!({"name":"limited","nodeIds":[source["id"],target["id"]],"permissions":["read"]}))).unwrap();
    let token = connection["token"].as_str().unwrap();
    let path = format!("/api/relations/{}", relation["id"].as_str().unwrap());
    assert_eq!(
        engine
            .request("DELETE", &path, &json!({}), token)
            .unwrap_err()
            .status,
        403
    );
    engine.request("DELETE", &path, &json!({}), &owner).unwrap();
    let visible = engine
        .request(
            "GET",
            &format!("/api/ai/nodes/{}/relations", source["id"].as_str().unwrap()),
            &json!({}),
            token,
        )
        .unwrap();
    assert_eq!(visible["relations"], json!([]));
    assert_eq!(
        engine
            .request("POST", &format!("{path}/restore"), &json!({}), token)
            .unwrap_err()
            .status,
        403
    );
    let restored = engine
        .request("POST", &format!("{path}/restore"), &json!({}), &owner)
        .unwrap();
    assert_ne!(restored["id"], relation["id"]);
}

#[test]
fn batch_relinks_removed_edge_and_undo_reuses_active_duplicate() {
    let (_dir, mut engine, owner, source, target, relation) = fixture();
    let id = relation["id"].as_str().unwrap();
    assert_eq!(
        engine
            .vault
            .mutate(|v| v.restore_relation(id))
            .unwrap_err()
            .status,
        409
    );
    assert_eq!(
        engine
            .vault
            .mutate(|v| v.restore_relation("missing"))
            .unwrap_err()
            .status,
        404
    );
    assert_eq!(
        engine
            .vault
            .mutate(|v| v.delete_relation("missing"))
            .unwrap_err()
            .status,
        404
    );
    engine.vault.mutate(|v| v.delete_relation(id)).unwrap();
    let mut expected = serde_json::Map::new();
    for endpoint in [&source, &target] {
        expected.insert(
            endpoint["id"].as_str().unwrap().to_owned(),
            endpoint["revisionId"].clone(),
        );
    }
    let batch = engine.request("POST", "/api/relations/batch", &json!({"nodeIds":[source["id"]],"toId":target["id"],"type":"related_to","expectedRevisions":expected}), &owner).unwrap();
    assert_eq!(batch["skipped"], 0);
    let active = &batch["relations"][0];
    assert_ne!(active["id"], relation["id"]);
    assert_eq!(
        engine.vault.mutate(|v| v.restore_relation(id)).unwrap(),
        *active
    );
    assert_eq!(engine.vault.graph().unwrap()["relations"], json!([active]));
    assert_eq!(
        engine.vault.export().unwrap()["relations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
