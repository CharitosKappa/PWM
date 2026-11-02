//! Vaults, item writes with revision preconditions, and delta sync (D27),
//! end to end through the HTTP API against a real Postgres.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use axum::Router;
use axum::http::StatusCode;
use common::flows::*;
use common::{Reply, call, send};
use pwm_crypto::{Aad, Envelope, SymmetricKey, open, seal};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Item record format version bound into the item AAD (D11, D18).
const ITEM_VERSION: u8 = 1;

struct Vault {
    id: Uuid,
    key: SymmetricKey,
}

async fn create_vault(app: &Router, account: &Account) -> Vault {
    let id = new_id();
    let key = SymmetricKey::generate().unwrap();
    let wrapped = account.keyset.wrap_vault_key(&key, id.as_bytes()).unwrap();
    let (status, body) = call(
        app,
        "POST",
        "/v1/vaults",
        Some(account.access()),
        Some(json!({ "id": id, "wrapped_key": b64(&wrapped.to_bytes()) })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    Vault { id, key }
}

/// The item envelope a client uploads for `revision`, as base64url.
fn sealed(vault: &Vault, item: &Uuid, revision: u64, text: &str) -> String {
    let aad = Aad::item(ITEM_VERSION, vault.id.as_bytes(), item.as_bytes(), revision);
    b64(&seal(&vault.key, text.as_bytes(), &aad).unwrap().to_bytes())
}

fn opened(vault: &Vault, item: &Uuid, revision: u64, blob: &Value) -> String {
    let aad = Aad::item(ITEM_VERSION, vault.id.as_bytes(), item.as_bytes(), revision);
    let envelope = Envelope::from_bytes(&unb64(blob)).unwrap();
    String::from_utf8(
        open(&vault.key, &envelope, &aad)
            .unwrap()
            .expose_secret()
            .clone(),
    )
    .unwrap()
}

fn item_path(vault: &Uuid, item: &Uuid) -> String {
    format!("/v1/vaults/{vault}/items/{item}")
}

/// `PUT` with raw precondition headers.
async fn put_raw(
    app: &Router,
    token: &str,
    vault: &Uuid,
    item: &Uuid,
    headers: &[(&str, &str)],
    body: Value,
) -> Reply {
    send(
        app,
        "PUT",
        &item_path(vault, item),
        Some(token),
        headers,
        Some(body),
    )
    .await
}

/// Creates `item` at revision 1.
async fn create(app: &Router, token: &str, vault: &Vault, item: &Uuid, text: &str) -> Reply {
    let body = json!({ "revision": "1", "blob": sealed(vault, item, 1, text) });
    put_raw(app, token, &vault.id, item, &[("if-none-match", "*")], body).await
}

/// Replaces revision `current` of `item` with `current + 1`.
async fn update(
    app: &Router,
    token: &str,
    vault: &Vault,
    item: &Uuid,
    current: u64,
    text: &str,
) -> Reply {
    let next = current + 1;
    let body = json!({ "revision": next.to_string(), "blob": sealed(vault, item, next, text) });
    let etag = format!("\"{current}\"");
    put_raw(app, token, &vault.id, item, &[("if-match", &etag)], body).await
}

async fn delete(app: &Router, token: &str, vault: &Uuid, item: &Uuid, current: u64) -> Reply {
    let etag = format!("\"{current}\"");
    send(
        app,
        "DELETE",
        &item_path(vault, item),
        Some(token),
        &[("if-match", &etag)],
        None,
    )
    .await
}

async fn sync(app: &Router, token: &str, vault: &Uuid, query: &str) -> (StatusCode, Value) {
    call(
        app,
        "GET",
        &format!("/v1/vaults/{vault}/items{query}"),
        Some(token),
        None,
    )
    .await
}

async fn vault_seq(pool: &PgPool, vault: &Uuid) -> i64 {
    sqlx::query_scalar("SELECT seq FROM vaults WHERE id = $1")
        .bind(vault)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Adds `member` to `vault` the way the sharing milestone will.
async fn add_member(pool: &PgPool, vault: &Uuid, member: &Account, owner: &Account, role: &str) {
    sqlx::query(
        "INSERT INTO vault_keys (vault_id, account_id, role, wrap, wrapped_key, shared_by) \
         VALUES ($1, $2, $3, 'share', $4, $5)",
    )
    .bind(vault)
    .bind(member.id)
    .bind(role)
    .bind(vec![7u8; 41])
    .bind(owner.id)
    .execute(pool)
    .await
    .unwrap();
}

fn error(code: &str) -> Value {
    json!({ "error": code })
}

#[tokio::test]
async fn vaults_are_created_and_listed() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let bob = register(&app, "bob@example.com").await;

    let (status, body) = call(&app, "GET", "/v1/vaults", Some(alice.access()), None).await;
    assert_eq!((status, body), (StatusCode::OK, json!([])));

    let vault = create_vault(&app, &alice).await;
    let (status, list) = call(&app, "GET", "/v1/vaults", Some(alice.access()), None).await;
    assert_eq!(status, StatusCode::OK);
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 1);
    let entry = &list[0];
    assert_eq!(entry["id"], json!(vault.id));
    assert_eq!(entry["role"], "owner");
    assert_eq!(entry["wrap"], "own");
    assert_eq!(entry["shared_by"], Value::Null);
    assert_eq!(entry["seq"], "0");
    let wrapped = Envelope::from_bytes(&unb64(&entry["wrapped_key"])).unwrap();
    let key = alice
        .keyset
        .unwrap_vault_key(&wrapped, vault.id.as_bytes())
        .unwrap();
    // The unwrapped key is the vault key: what one seals, the other opens.
    let probe = Aad::item(ITEM_VERSION, vault.id.as_bytes(), &[0; 16], 1);
    let sealed = seal(&key, b"probe", &probe).unwrap();
    assert!(open(&vault.key, &sealed, &probe).is_ok());

    // Bob sees none of Alice's vaults.
    let (_, list) = call(&app, "GET", "/v1/vaults", Some(bob.access()), None).await;
    assert_eq!(list, json!([]));

    // The id is taken, even for another account.
    let again = json!({ "id": vault.id, "wrapped_key": b64(&[1u8; 41]) });
    let (status, body) = call(&app, "POST", "/v1/vaults", Some(bob.access()), Some(again)).await;
    assert_eq!(
        (status, body),
        (StatusCode::CONFLICT, error("vault_exists"))
    );

    for bad in [
        json!({ "id": new_id(), "wrapped_key": b64(&[1u8; 40]) }),
        json!({ "id": new_id(), "wrapped_key": "not base64!" }),
        json!({ "id": "not-a-uuid", "wrapped_key": b64(&[1u8; 41]) }),
        json!({ "id": new_id() }),
    ] {
        let (status, body) =
            call(&app, "POST", "/v1/vaults", Some(alice.access()), Some(bad)).await;
        assert_eq!(
            (status, body),
            (StatusCode::BAD_REQUEST, error("invalid_request"))
        );
    }

    let (status, _) = call(&app, "GET", "/v1/vaults", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    db.drop().await;
}

#[tokio::test]
async fn items_are_created_updated_deleted_and_synced() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let token = alice.access();
    let vault = create_vault(&app, &alice).await;
    let (a, b) = (new_id(), new_id());

    let reply = create(&app, token, &vault, &a, "a1").await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    assert_eq!(reply.body, json!({ "revision": "1", "seq": "1" }));
    assert_eq!(reply.headers["etag"], "\"1\"");

    let reply = create(&app, token, &vault, &b, "b1").await;
    assert_eq!(reply.body, json!({ "revision": "1", "seq": "2" }));

    let reply = update(&app, token, &vault, &a, 1, "a2").await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body, json!({ "revision": "2", "seq": "3" }));
    assert_eq!(reply.headers["etag"], "\"2\"");

    let reply = delete(&app, token, &vault.id, &b, 1).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body, json!({ "revision": "1", "seq": "4" }));

    let (status, page) = sync(&app, token, &vault.id, "").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["more"], false);
    assert_eq!(page["next"], "4");
    let changes = page["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0]["id"], json!(a));
    assert_eq!(changes[0]["revision"], "2");
    assert_eq!(changes[0]["seq"], "3");
    assert_eq!(opened(&vault, &a, 2, &changes[0]["blob"]), "a2");
    assert_eq!(
        changes[1],
        json!({ "id": b, "revision": "1", "seq": "4", "deleted": true })
    );

    // Nothing new since the last change.
    let (_, page) = sync(&app, token, &vault.id, "?since=4").await;
    assert_eq!(page, json!({ "changes": [], "next": "4", "more": false }));
    let (_, page) = sync(&app, token, &vault.id, "?since=3").await;
    assert_eq!(page["changes"].as_array().unwrap().len(), 1);

    let (_, list) = call(&app, "GET", "/v1/vaults", Some(token), None).await;
    assert_eq!(list[0]["seq"], "4");

    // The same item id in another vault is a different item.
    let other = create_vault(&app, &alice).await;
    let reply = create(&app, token, &other, &a, "other").await;
    assert_eq!(reply.body, json!({ "revision": "1", "seq": "1" }));
    db.drop().await;
}

#[tokio::test]
async fn writes_need_a_matching_precondition() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let token = alice.access();
    let vault = create_vault(&app, &alice).await;
    let item = new_id();
    let v1 = json!({ "revision": "1", "blob": sealed(&vault, &item, 1, "x") });

    // No precondition: the server never overwrites blindly.
    let reply = put_raw(&app, token, &vault.id, &item, &[], v1.clone()).await;
    assert_eq!(
        (reply.status, reply.body),
        (
            StatusCode::PRECONDITION_REQUIRED,
            error("precondition_required")
        )
    );
    // Updating an item that does not exist.
    let reply = update(&app, token, &vault, &item, 1, "x").await;
    assert_eq!(
        (reply.status, reply.body),
        (StatusCode::NOT_FOUND, error("not_found"))
    );
    // A create must be revision 1.
    let v2 = json!({ "revision": "2", "blob": sealed(&vault, &item, 2, "x") });
    let reply = put_raw(&app, token, &vault.id, &item, &[("if-none-match", "*")], v2).await;
    assert_eq!(
        (reply.status, reply.body),
        (StatusCode::BAD_REQUEST, error("invalid_request"))
    );

    assert_eq!(
        create(&app, token, &vault, &item, "x1").await.status,
        StatusCode::CREATED
    );

    // Creating it again returns the current item.
    let reply = create(&app, token, &vault, &item, "again").await;
    assert_eq!(reply.status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(reply.body["error"], "revision_conflict");
    assert_eq!(reply.body["current"]["revision"], "1");
    assert_eq!(reply.body["current"]["seq"], "1");
    assert_eq!(
        opened(&vault, &item, 1, &reply.body["current"]["blob"]),
        "x1"
    );

    assert_eq!(
        update(&app, token, &vault, &item, 1, "x2").await.status,
        StatusCode::OK
    );

    // A stale write gets the current item, so the client can keep both.
    let reply = update(&app, token, &vault, &item, 1, "stale").await;
    assert_eq!(reply.status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(reply.body["error"], "revision_conflict");
    assert_eq!(reply.body["current"]["revision"], "2");
    assert_eq!(reply.body["current"]["seq"], "2");
    assert_eq!(
        opened(&vault, &item, 2, &reply.body["current"]["blob"]),
        "x2"
    );

    // The new revision must be the matched one plus one.
    let skip = json!({ "revision": "4", "blob": sealed(&vault, &item, 4, "x") });
    let reply = put_raw(
        &app,
        token,
        &vault.id,
        &item,
        &[("if-match", "\"2\"")],
        skip,
    )
    .await;
    assert_eq!(
        (reply.status, reply.body),
        (StatusCode::BAD_REQUEST, error("invalid_request"))
    );

    // Malformed preconditions and bodies.
    let v3 = json!({ "revision": "3", "blob": sealed(&vault, &item, 3, "x") });
    for headers in [
        &[("if-match", "2")][..],
        &[("if-match", "W/\"2\"")],
        &[("if-match", "*")],
        &[("if-match", "\"02\"")],
        &[("if-match", "\"2\", \"3\"")],
        &[("if-none-match", "\"2\"")],
        &[("if-match", "\"2\""), ("if-none-match", "*")],
    ] {
        let reply = put_raw(&app, token, &vault.id, &item, headers, v3.clone()).await;
        assert_eq!(
            (reply.status, reply.body),
            (StatusCode::BAD_REQUEST, error("invalid_request")),
            "{headers:?}"
        );
    }
    for body in [
        json!({ "revision": "03", "blob": sealed(&vault, &item, 3, "x") }),
        json!({ "revision": 3, "blob": sealed(&vault, &item, 3, "x") }),
        json!({ "revision": "3", "blob": b64(&[1u8; 40]) }),
        json!({ "revision": "3", "blob": b64(&[9u8; 41]) }),
        json!({ "revision": "3" }),
        json!({ "revision": "18446744073709551616", "blob": sealed(&vault, &item, 3, "x") }),
    ] {
        let reply = put_raw(
            &app,
            token,
            &vault.id,
            &item,
            &[("if-match", "\"2\"")],
            body,
        )
        .await;
        assert_eq!(
            (reply.status, reply.body),
            (StatusCode::BAD_REQUEST, error("invalid_request"))
        );
    }
    let reply = put_raw(
        &app,
        token,
        &vault.id,
        &item,
        &[("if-match", "\"2\"")],
        v3.clone(),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);

    // Deleting needs If-Match too.
    let path = item_path(&vault.id, &item);
    let reply = send(&app, "DELETE", &path, Some(token), &[], None).await;
    assert_eq!(
        (reply.status, reply.body),
        (
            StatusCode::PRECONDITION_REQUIRED,
            error("precondition_required")
        )
    );
    let reply = send(
        &app,
        "DELETE",
        &path,
        Some(token),
        &[("if-none-match", "*")],
        None,
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
    let reply = delete(&app, token, &vault.id, &item, 2).await;
    assert_eq!(reply.status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(reply.body["current"]["revision"], "3");
    assert_eq!(
        delete(&app, token, &vault.id, &new_id(), 1).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        delete(&app, token, &vault.id, &item, 3).await.status,
        StatusCode::OK
    );

    // A deleted id is never reused.
    let reply = delete(&app, token, &vault.id, &item, 3).await;
    assert_eq!(
        (reply.status, reply.body),
        (StatusCode::CONFLICT, error("item_deleted"))
    );
    let reply = create(&app, token, &vault, &item, "reborn").await;
    assert_eq!(
        (reply.status, reply.body),
        (StatusCode::CONFLICT, error("item_deleted"))
    );
    let reply = update(&app, token, &vault, &item, 3, "reborn").await;
    assert_eq!(
        (reply.status, reply.body),
        (StatusCode::CONFLICT, error("item_deleted"))
    );

    // Bad ids in the path.
    let reply = send(
        &app,
        "DELETE",
        &format!("/v1/vaults/{}/items/nope", vault.id),
        Some(token),
        &[("if-match", "\"1\"")],
        None,
    )
    .await;
    assert_eq!(
        (reply.status, reply.body),
        (StatusCode::BAD_REQUEST, error("invalid_request"))
    );

    // Only the four successful writes used a change number.
    assert_eq!(vault_seq(&db.pool, &vault.id).await, 4);
    db.drop().await;
}

#[tokio::test]
async fn viewers_read_and_outsiders_get_not_found() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let viewer = register(&app, "viewer@example.com").await;
    let editor = register(&app, "editor@example.com").await;
    let outsider = register(&app, "outsider@example.com").await;
    let vault = create_vault(&app, &alice).await;
    let item = new_id();
    create(&app, alice.access(), &vault, &item, "shared").await;
    add_member(&db.pool, &vault.id, &viewer, &alice, "viewer").await;
    add_member(&db.pool, &vault.id, &editor, &alice, "editor").await;

    // A viewer reads but cannot write.
    let (status, page) = sync(&app, viewer.access(), &vault.id, "").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        opened(&vault, &item, 1, &page["changes"][0]["blob"]),
        "shared"
    );
    let (_, list) = call(&app, "GET", "/v1/vaults", Some(viewer.access()), None).await;
    assert_eq!(list[0]["role"], "viewer");
    assert_eq!(list[0]["wrap"], "share");
    assert_eq!(list[0]["shared_by"], json!(alice.id));
    let read_only = (StatusCode::FORBIDDEN, error("read_only"));
    let reply = update(&app, viewer.access(), &vault, &item, 1, "no").await;
    assert_eq!((reply.status, reply.body), read_only.clone());
    let reply = create(&app, viewer.access(), &vault, &new_id(), "no").await;
    assert_eq!((reply.status, reply.body), read_only.clone());
    let reply = delete(&app, viewer.access(), &vault.id, &item, 1).await;
    assert_eq!((reply.status, reply.body), read_only);

    // An editor writes.
    let reply = update(&app, editor.access(), &vault, &item, 1, "edited").await;
    assert_eq!(reply.status, StatusCode::OK);

    // A vault you are not in looks exactly like one that does not exist.
    let not_found = (StatusCode::NOT_FOUND, error("not_found"));
    for vault_id in [vault.id, new_id()] {
        let (status, body) = sync(&app, outsider.access(), &vault_id, "").await;
        assert_eq!((status, body), not_found.clone());
        let body = json!({ "revision": "3", "blob": sealed(&vault, &item, 3, "x") });
        let reply = put_raw(
            &app,
            outsider.access(),
            &vault_id,
            &item,
            &[("if-match", "\"2\"")],
            body,
        )
        .await;
        assert_eq!((reply.status, reply.body), not_found.clone());
        let reply = delete(&app, outsider.access(), &vault_id, &item, 2).await;
        assert_eq!((reply.status, reply.body), not_found.clone());
    }
    let (status, _) = sync(&app, "pwma_nope", &vault.id, "").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    db.drop().await;
}

#[tokio::test]
async fn sync_pages_through_items_and_tombstones() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let token = alice.access();
    let vault = create_vault(&app, &alice).await;

    // Expected end state: id -> (revision, deleted).
    let mut expected = BTreeMap::new();
    let ids: Vec<Uuid> = (0..7).map(|_| new_id()).collect();
    for id in &ids {
        create(&app, token, &vault, id, "v1").await;
        expected.insert(*id, (1u64, false));
    }
    for id in &ids[..3] {
        assert_eq!(
            delete(&app, token, &vault.id, id, 1).await.status,
            StatusCode::OK
        );
        expected.insert(*id, (1, true));
    }
    assert_eq!(
        update(&app, token, &vault, &ids[5], 1, "v2").await.status,
        StatusCode::OK
    );
    expected.insert(ids[5], (2, false));
    let total = vault_seq(&db.pool, &vault.id).await;
    assert_eq!(total, 11);

    let mut since = "0".to_owned();
    let mut seen = BTreeMap::new();
    let mut last_seq = 0;
    let mut pages = 0;
    loop {
        let (status, page) = sync(&app, token, &vault.id, &format!("?since={since}&limit=3")).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        pages += 1;
        let changes = page["changes"].as_array().unwrap();
        assert!(changes.len() <= 3);
        for change in changes {
            let seq: i64 = change["seq"].as_str().unwrap().parse().unwrap();
            assert!(seq > last_seq, "changes come in seq order");
            last_seq = seq;
            let id: Uuid = serde_json::from_value(change["id"].clone()).unwrap();
            let revision: u64 = change["revision"].as_str().unwrap().parse().unwrap();
            let deleted = change.get("deleted") == Some(&json!(true));
            assert_eq!(deleted, change.get("blob").is_none());
            assert!(
                seen.insert(id, (revision, deleted)).is_none(),
                "each id once"
            );
        }
        since = page["next"].as_str().unwrap().to_owned();
        if page["more"] == false {
            break;
        }
        assert_eq!(changes.len(), 3, "a page that has more is full");
        assert_eq!(since, last_seq.to_string());
    }
    assert_eq!(pages, 3);
    assert_eq!(seen, expected);
    assert_eq!(since, total.to_string());

    // Defaults, limits and bad parameters.
    let (status, page) = sync(&app, token, &vault.id, "?limit=1000").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["changes"].as_array().unwrap().len(), 7);
    for query in [
        "?limit=0",
        "?limit=1001",
        "?limit=x",
        "?since=-1",
        "?since=abc",
        "?since=01",
        "?since=1&since=2",
        "?since=9223372036854775808",
    ] {
        let (status, body) = sync(&app, token, &vault.id, query).await;
        assert_eq!(
            (status, body),
            (StatusCode::BAD_REQUEST, error("invalid_request")),
            "{query}"
        );
    }

    // A client ahead of the server (e.g. after a restore) is told the
    // current seq, which is lower than what it sent.
    let (_, page) = sync(&app, token, &vault.id, "?since=50").await;
    assert_eq!(page, json!({ "changes": [], "next": "11", "more": false }));
    db.drop().await;
}

#[tokio::test]
async fn concurrent_writers_get_gapless_seqs() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let token = alice.access().to_owned();
    let vault = std::sync::Arc::new(create_vault(&app, &alice).await);
    const WRITERS: i64 = 24;

    let mut tasks = Vec::new();
    for _ in 0..WRITERS {
        let (app, token, vault) = (app.clone(), token.clone(), vault.clone());
        tasks.push(tokio::spawn(async move {
            let reply = create(&app, &token, &vault, &new_id(), "x").await;
            assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
            reply.body["seq"].as_str().unwrap().parse::<i64>().unwrap()
        }));
    }
    // A reader syncing meanwhile only ever sees a gapless prefix.
    let reader = {
        let (app, token, vault_id) = (app.clone(), token.clone(), vault.id);
        tokio::spawn(async move {
            for _ in 0..20 {
                let (status, page) = sync(&app, &token, &vault_id, "").await;
                assert_eq!(status, StatusCode::OK);
                let seqs: Vec<i64> = page["changes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| c["seq"].as_str().unwrap().parse().unwrap())
                    .collect();
                let next: i64 = page["next"].as_str().unwrap().parse().unwrap();
                assert_eq!(seqs, (1..=next).collect::<Vec<_>>());
            }
        })
    };
    let mut seqs = BTreeSet::new();
    for task in tasks {
        assert!(seqs.insert(task.await.unwrap()), "no repeated seq");
    }
    reader.await.unwrap();
    assert_eq!(seqs, (1..=WRITERS).collect());
    assert_eq!(vault_seq(&db.pool, &vault.id).await, WRITERS);
    db.drop().await;
}

#[tokio::test]
async fn concurrent_writes_from_one_revision_have_one_winner() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let alice = register(&app, "alice@example.com").await;
    let token = alice.access().to_owned();
    let vault = std::sync::Arc::new(create_vault(&app, &alice).await);
    let item = new_id();
    const RACERS: usize = 12;

    // Racing creates of one id.
    let mut tasks = Vec::new();
    for n in 0..RACERS {
        let (app, token, vault) = (app.clone(), token.clone(), vault.clone());
        tasks.push(tokio::spawn(async move {
            create(&app, &token, &vault, &item, &format!("create {n}")).await
        }));
    }
    let mut replies = Vec::new();
    for task in tasks {
        replies.push(task.await.unwrap());
    }
    let created: Vec<_> = replies
        .iter()
        .filter(|r| r.status == StatusCode::CREATED)
        .collect();
    assert_eq!(created.len(), 1);
    for reply in replies.iter().filter(|r| r.status != StatusCode::CREATED) {
        assert_eq!(
            reply.status,
            StatusCode::PRECONDITION_FAILED,
            "{}",
            reply.body
        );
        assert_eq!(reply.body["current"]["revision"], "1");
    }

    // Racing updates from revision 1.
    let mut tasks = Vec::new();
    for n in 0..RACERS {
        let (app, token, vault) = (app.clone(), token.clone(), vault.clone());
        tasks.push(tokio::spawn(async move {
            let text = format!("update {n}");
            (update(&app, &token, &vault, &item, 1, &text).await, text)
        }));
    }
    let mut results = Vec::new();
    for task in tasks {
        results.push(task.await.unwrap());
    }
    let winners: Vec<_> = results
        .iter()
        .filter(|(r, _)| r.status == StatusCode::OK)
        .collect();
    assert_eq!(winners.len(), 1);
    let winning_text = &winners[0].1;
    for (reply, _) in results.iter().filter(|(r, _)| r.status != StatusCode::OK) {
        assert_eq!(
            reply.status,
            StatusCode::PRECONDITION_FAILED,
            "{}",
            reply.body
        );
        assert_eq!(reply.body["current"]["revision"], "2");
        assert_eq!(
            &opened(&vault, &item, 2, &reply.body["current"]["blob"]),
            winning_text
        );
    }

    // Racing deletes of revision 2.
    let mut tasks = Vec::new();
    for _ in 0..RACERS {
        let (app, token, vault_id) = (app.clone(), token.clone(), vault.id);
        tasks.push(tokio::spawn(async move {
            delete(&app, &token, &vault_id, &item, 2).await.status
        }));
    }
    let mut statuses = Vec::new();
    for task in tasks {
        statuses.push(task.await.unwrap());
    }
    assert_eq!(statuses.iter().filter(|s| **s == StatusCode::OK).count(), 1);
    assert!(
        statuses
            .iter()
            .all(|s| *s == StatusCode::OK || *s == StatusCode::CONFLICT)
    );
    assert_eq!(vault_seq(&db.pool, &vault.id).await, 3);
    db.drop().await;
}
