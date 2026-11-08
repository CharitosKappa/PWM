//! The `pwm-sync` client (D28) against this server and a real Postgres, so
//! the HTTP contract between them is checked end to end.

mod common;

use axum::Router;
use common::flows::*;
use http_body_util::BodyExt;
use pwm_crypto::{AccountKeyset, Id, SymmetricKey};
use pwm_store::{SqliteStore, Store, SyncState};
use pwm_sync::{
    HttpRequest, HttpResponse, Step, SyncEngine, SyncEvent, SyncOutcome, SyncReport, Tokens,
};
use pwm_vault::{Item, ItemId, ItemKind};
use serde_json::Value;
use tower::ServiceExt;

type Engine = SyncEngine<SqliteStore>;

const T0: i64 = 1_700_000_000_000;

async fn send(app: &Router, request: &HttpRequest) -> HttpResponse {
    let mut builder = axum::http::Request::builder()
        .method(request.method.as_str())
        .uri(&request.path);
    for (name, value) in &request.headers {
        builder = builder.header(*name, value.as_str());
    }
    let body = request
        .body
        .clone()
        .map_or_else(axum::body::Body::empty, axum::body::Body::from);
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(n, v)| (n.to_string(), v.to_str().unwrap_or_default().to_owned()))
        .collect();
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    HttpResponse::new(status, headers, body)
}

async fn drive(app: &Router, engine: &mut Engine, mut step: Step) -> SyncOutcome {
    loop {
        step = match step {
            Step::Send(request) => engine.receive(send(app, &request).await, T0).unwrap(),
            Step::Wait { delay_ms } => panic!("the server failed; would wait {delay_ms} ms"),
            Step::Done(outcome) => return outcome,
        };
    }
}

async fn sync(app: &Router, engine: &mut Engine) -> SyncReport {
    let step = engine.start_sync().unwrap();
    match drive(app, engine, step).await {
        SyncOutcome::Synced(report) => report,
        other => panic!("sync did not finish: {other:?}"),
    }
}

/// A second copy of the account keyset, as another device unlocks it.
fn copy_keyset(account: &Account) -> AccountKeyset {
    let auk = SymmetricKey::generate().unwrap();
    let sealed = account.keyset.seal(&auk, account.id.as_bytes()).unwrap();
    AccountKeyset::open(&sealed, &auk, account.id.as_bytes()).unwrap()
}

fn engine(account: &Account, tokens: &Value) -> Engine {
    let tokens = Tokens::new(
        tokens["access_token"].as_str().unwrap().to_owned(),
        tokens["refresh_token"].as_str().unwrap().to_owned(),
    );
    SyncEngine::new(
        SqliteStore::open_in_memory().unwrap(),
        copy_keyset(account),
        tokens,
    )
    .unwrap()
}

fn add(engine: &mut Engine, vault: &Id, title: &str) -> ItemId {
    let id = ItemId::generate().unwrap();
    engine
        .save_item(vault, &id, &Item::new(ItemKind::Note, title, T0))
        .unwrap();
    id
}

fn edit(engine: &mut Engine, vault: &Id, id: &ItemId, title: &str, now: i64) {
    let mut item = engine.item(vault, id).unwrap().unwrap();
    let mut edited = item.clone();
    edited.title = title.to_owned();
    item.apply_edit(edited, now).unwrap();
    engine.save_item(vault, id, &item).unwrap();
}

fn titles(engine: &Engine, vault: &Id) -> Vec<String> {
    let mut titles: Vec<String> = engine
        .items(vault)
        .unwrap()
        .iter()
        .map(|(_, item)| item.title.clone())
        .collect();
    titles.sort();
    titles
}

#[tokio::test]
async fn two_devices_sync_through_the_server() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "alice@example.com").await;
    let mut laptop = engine(&account, &account.tokens);

    let (vault, step) = laptop.start_create_vault().unwrap();
    assert!(matches!(
        drive(&app, &mut laptop, step).await,
        SyncOutcome::Synced(_)
    ));
    let bank = add(&mut laptop, &vault, "Bank");
    let mail = add(&mut laptop, &vault, "Mail");
    let report = sync(&app, &mut laptop).await;
    assert_eq!(report.pushed, 2);

    let phone_id = new_id();
    let (_, phone_tokens) = login(&app, &account, &phone_id).await;
    let mut phone = engine(&account, &phone_tokens);
    let report = sync(&app, &mut phone).await;
    assert_eq!(report.pulled, 2);
    assert_eq!(titles(&phone, &vault), ["Bank", "Mail"]);

    // Offline edits on both; the phone's is later and stays current.
    edit(&mut laptop, &vault, &bank, "Bank (laptop)", T0 + 10);
    edit(&mut phone, &vault, &bank, "Bank (phone)", T0 + 20);
    sync(&app, &mut laptop).await;
    let report = sync(&app, &mut phone).await;
    assert_eq!(report.conflicts, 1);
    sync(&app, &mut laptop).await;
    for device in [&laptop, &phone] {
        let item = device.item(&vault, &bank).unwrap().unwrap();
        assert_eq!(item.title, "Bank (phone)");
        assert!(
            item.history()
                .iter()
                .any(|v| v.item.title == "Bank (laptop)")
        );
    }

    // A delete reaches the other device; editing it there keeps a copy.
    phone.delete_item(&vault, &mail).unwrap();
    edit(&mut laptop, &vault, &mail, "Mail (kept)", T0 + 30);
    sync(&app, &mut phone).await;
    sync(&app, &mut laptop).await;
    sync(&app, &mut phone).await;
    assert_eq!(titles(&laptop, &vault), ["Bank (phone)", "Mail (kept)"]);
    assert_eq!(titles(&phone, &vault), titles(&laptop, &vault));
    assert!(phone.store().pending_items().unwrap().is_empty());
    assert!(laptop.store().pending_items().unwrap().is_empty());
    let records = laptop.store().items(&vault).unwrap();
    assert!(records.iter().all(|r| r.state == SyncState::Synced));

    // An expired access token is refreshed and the run carries on.
    sqlx::query("UPDATE sessions SET access_expires_at = now() - interval '1 second'")
        .execute(&db.pool)
        .await
        .unwrap();
    add(&mut laptop, &vault, "After refresh");
    let report = sync(&app, &mut laptop).await;
    assert_eq!(report.pushed, 1);
    let events = laptop.take_events();
    assert!(matches!(events.as_slice(), [SyncEvent::TokensRefreshed(_)]));

    // The laptop revokes the phone; the phone's next sync wipes its cache.
    let SyncEvent::TokensRefreshed(tokens) = &events[0] else {
        unreachable!()
    };
    let (status, _) = common::call(
        &app,
        "DELETE",
        &format!("/v1/devices/{phone_id}"),
        Some(tokens.access()),
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    let step = phone.start_sync().unwrap();
    assert!(matches!(
        drive(&app, &mut phone, step).await,
        SyncOutcome::DeviceRevoked
    ));
    assert!(phone.store().vaults().unwrap().is_empty());
    db.drop().await;
}

#[tokio::test]
async fn a_device_behind_compacted_tombstones_pulls_again_and_drops_deletes() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "alice@example.com").await;
    let mut laptop = engine(&account, &account.tokens);
    let (vault, step) = laptop.start_create_vault().unwrap();
    assert!(matches!(
        drive(&app, &mut laptop, step).await,
        SyncOutcome::Synced(_)
    ));
    let bank = add(&mut laptop, &vault, "Bank");
    add(&mut laptop, &vault, "Mail");
    sync(&app, &mut laptop).await;
    let (_, phone_tokens) = login(&app, &account, &new_id()).await;
    let mut phone = engine(&account, &phone_tokens);
    sync(&app, &mut phone).await;

    // The laptop deletes Bank; half a year later the tombstone is compacted
    // before the phone syncs again (D76 §2).
    laptop.delete_item(&vault, &bank).unwrap();
    sync(&app, &mut laptop).await;
    sqlx::query("UPDATE tombstones SET deleted_at = now() - make_interval(days => $1::int)")
        .bind(pwm_server::TOMBSTONE_HORIZON_DAYS + 1)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(pwm_server::clean_up(&db.pool).await.unwrap().tombstones, 1);

    let report = sync(&app, &mut phone).await;
    assert!(report.resynced);
    assert_eq!(titles(&phone, &vault), ["Mail"]);
    assert_eq!(phone.store().item(&vault, &bank).unwrap(), None);
    // Nothing came back for the laptop either.
    let report = sync(&app, &mut laptop).await;
    assert!(!report.resynced);
    assert_eq!(titles(&laptop, &vault), ["Mail"]);
    db.drop().await;
}
