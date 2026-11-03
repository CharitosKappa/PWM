//! The app API (`pwm-client`, D31) end to end against this server and a
//! real Postgres: sign-up, first vault, items, sync, login on a second
//! device, unlock, and a snapshot reloaded offline.

mod common;

use axum::Router;
use common::flows::setup;
use http_body_util::BodyExt;
use pwm_client::{
    Client, ClientError, HttpRequest, HttpResponse, KdfParams, MemoryStore, Outcome, Platform,
    Step, SyncEvent, Tokens, new_id,
};
use pwm_vault::{Item, ItemId, ItemKind};
use serde_json::Value;
use tower::ServiceExt;

const T0: i64 = 1_700_000_000_000;
const EMAIL: &str = "Alice@Example.com";
const PASSWORD: &str = "correct horse battery staple 42";

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

/// Sends every request of a conversation until it is done.
async fn run(
    app: &Router,
    client: &mut Client<MemoryStore>,
    step: Result<Step, ClientError>,
) -> Result<Outcome, ClientError> {
    let mut step = step?;
    loop {
        step = match step {
            Step::Send(request) => client.receive(send(app, &request).await, T0)?,
            Step::Wait { delay_ms } => panic!("the server failed; would wait {delay_ms} ms"),
            Step::Done(outcome) => return Ok(outcome),
        };
    }
}

fn latest_tokens(client: &mut Client<MemoryStore>) -> Tokens {
    let events = client.take_events();
    let tokens = events
        .iter()
        .rev()
        .find_map(|e| match e {
            SyncEvent::TokensRefreshed(t) => Some(t),
            _ => None,
        })
        .expect("new tokens");
    Tokens::new(tokens.access().to_owned(), tokens.refresh().to_owned())
}

fn note(title: &str) -> (String, String) {
    let id = ItemId::generate().unwrap();
    let json = Item::new(ItemKind::Note, title, T0)
        .to_json_with_id(&id)
        .unwrap();
    (id.to_string(), json.expose_secret().clone())
}

fn titles(client: &Client<MemoryStore>, vault: &str) -> Vec<String> {
    let mut titles: Vec<String> = client
        .items(vault)
        .unwrap()
        .iter()
        .map(|j| {
            serde_json::from_str::<Value>(j.expose_secret()).unwrap()["title"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    titles.sort();
    titles
}

fn new_client() -> Client<MemoryStore> {
    let mut client = Client::new(MemoryStore::new());
    // The mobile floor keeps the test quick; the flow is the same.
    client.set_signup_params(KdfParams::MOBILE_FLOOR).unwrap();
    client
}

#[tokio::test]
async fn sign_up_sync_and_log_in_on_a_second_device() {
    let Some((db, app)) = setup().await else {
        return;
    };

    // Sign up, create the first vault, save an item, sync.
    let mut laptop = new_client();
    let laptop_id = new_id().unwrap();
    let step = laptop.start_signup(EMAIL, PASSWORD, &laptop_id, Platform::Linux, "Laptop");
    let Outcome::SignedUp { secret_key } = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("not signed up");
    };
    let secret_key = secret_key.expose_secret().clone();
    assert!(secret_key.starts_with("A1-"));
    assert!(laptop.is_unlocked());
    let laptop_tokens = latest_tokens(&mut laptop);
    // The first vault is created only if the account has none (D34).
    let step = laptop.start_ensure_vault();
    let Outcome::VaultCreated { vault_id: vault } = run(&app, &mut laptop, step).await.unwrap()
    else {
        panic!("no vault");
    };
    let (bank, json) = note("Bank");
    laptop.save_item(&vault, &json, T0).unwrap();
    let step = laptop.start_sync();
    let Outcome::Synced(report) = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("not synced");
    };
    assert_eq!(report.pushed, 1);

    // The same email cannot sign up twice.
    let mut other = new_client();
    let step = other.start_signup(EMAIL, PASSWORD, &new_id().unwrap(), Platform::Linux, "x");
    assert_eq!(
        run(&app, &mut other, step).await.unwrap_err().code(),
        "account_exists"
    );

    // Log in on a phone with the email, password and Secret Key.
    let mut phone = new_client();
    let phone_id = new_id().unwrap();
    let step = phone.start_login(
        EMAIL,
        PASSWORD,
        &secret_key,
        &phone_id,
        Platform::Android,
        "Phone",
    );
    assert!(matches!(
        run(&app, &mut phone, step).await.unwrap(),
        Outcome::LoggedIn
    ));
    let phone_tokens = latest_tokens(&mut phone);
    let step = phone.start_sync();
    run(&app, &mut phone, step).await.unwrap();
    assert_eq!(phone.vault_ids().unwrap(), std::slice::from_ref(&vault));
    // Running it again, on either device, creates nothing new.
    for client in [&mut laptop, &mut phone] {
        let step = client.start_ensure_vault();
        assert!(matches!(
            run(&app, client, step).await.unwrap(),
            Outcome::Synced(_)
        ));
        assert_eq!(client.vault_ids().unwrap(), std::slice::from_ref(&vault));
    }
    assert_eq!(titles(&phone, &vault), ["Bank"]);

    // Every login failure looks the same.
    let other_key = pwm_crypto::SecretKey::generate()
        .unwrap()
        .to_text()
        .expose_secret()
        .clone();
    for (email, password, key) in [
        (EMAIL, "wrong password entirely 99", secret_key.as_str()),
        (EMAIL, PASSWORD, other_key.as_str()),
        ("nobody@example.com", PASSWORD, secret_key.as_str()),
    ] {
        let mut client = new_client();
        let step = client.start_login(email, password, key, &new_id().unwrap(), Platform::Ios, "");
        assert_eq!(
            run(&app, &mut client, step).await.unwrap_err().code(),
            "invalid_credentials"
        );
        assert!(!client.is_unlocked());
    }

    // The phone edits; the laptop locks, unlocks offline and syncs it in.
    let edited = Item::new(ItemKind::Note, "Bank (new)", T0 + 5)
        .to_json_with_id(&bank.parse().unwrap())
        .unwrap();
    phone
        .save_item(&vault, edited.expose_secret(), T0 + 5)
        .unwrap();
    let step = phone.start_sync();
    run(&app, &mut phone, step).await.unwrap();
    laptop.lock();
    assert_eq!(laptop.items(&vault).unwrap_err().code(), "locked");
    assert_eq!(
        laptop
            .unlock(
                "not the password at all",
                &secret_key,
                Tokens::new("a".into(), "b".into())
            )
            .unwrap_err()
            .code(),
        "wrong_password"
    );
    laptop.unlock(PASSWORD, &secret_key, laptop_tokens).unwrap();
    let step = laptop.start_sync();
    run(&app, &mut laptop, step).await.unwrap();
    assert_eq!(titles(&laptop, &vault), ["Bank (new)"]);

    // The extension's way: snapshot the cache, load it later, unlock offline.
    let snapshot = phone.store().snapshot();
    let mut restored = Client::new(MemoryStore::load(&snapshot).unwrap());
    restored
        .unlock(PASSWORD, &secret_key, phone_tokens)
        .unwrap();
    assert_eq!(titles(&restored, &vault), ["Bank (new)"]);
    db.drop().await;
}

/// Devices from the app (D43 §6): a device names itself at login, any
/// device renames or revokes the others, and a name moved to another
/// device does not open.
#[tokio::test]
async fn devices_are_named_listed_renamed_and_revoked() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let mut laptop = new_client();
    let laptop_id = new_id().unwrap();
    let step = laptop.start_signup(EMAIL, PASSWORD, &laptop_id, Platform::Linux, "Laptop");
    let Outcome::SignedUp { secret_key } = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("not signed up");
    };
    let secret_key = secret_key.expose_secret().clone();
    let mut phone = new_client();
    let phone_id = new_id().unwrap();
    let step = phone.start_login(
        EMAIL,
        PASSWORD,
        &secret_key,
        &phone_id,
        Platform::Ios,
        "iPhone",
    );
    assert!(matches!(
        run(&app, &mut phone, step).await.unwrap(),
        Outcome::LoggedIn
    ));

    let step = phone.start_list_devices();
    let Outcome::Devices(devices) = run(&app, &mut phone, step).await.unwrap() else {
        panic!("no devices");
    };
    let names: Vec<(String, Option<String>, bool)> = devices
        .iter()
        .map(|d| (d.platform.clone(), d.name.clone(), d.current))
        .collect();
    assert_eq!(
        names,
        [
            ("linux".to_owned(), Some("Laptop".to_owned()), false),
            ("ios".to_owned(), Some("iPhone".to_owned()), true),
        ],
        "the phone named itself right after login"
    );
    assert!(
        devices
            .iter()
            .all(|d| d.revoked_at.is_none() && d.last_seen_at > 0)
    );

    // The phone renames the laptop; too long a name is refused on the device.
    let step = phone.start_rename_device(&laptop_id, "Work laptop");
    assert!(matches!(
        run(&app, &mut phone, step).await.unwrap(),
        Outcome::DeviceUpdated
    ));
    assert_eq!(
        phone
            .start_rename_device(&laptop_id, &"x".repeat(64))
            .unwrap_err()
            .code(),
        "invalid_device_name"
    );
    let step = phone.start_rename_device(&new_id().unwrap(), "Ghost");
    assert_eq!(
        run(&app, &mut phone, step).await.unwrap_err().code(),
        "unknown_device"
    );

    // A sealed name moved to another device's row does not open there.
    let laptop_tokens = latest_tokens(&mut laptop);
    let (_, rows) = common::call(
        &app,
        "GET",
        "/v1/devices",
        Some(laptop_tokens.access()),
        None,
    )
    .await;
    let laptop_name = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == laptop_id.as_str())
        .unwrap()["name_sealed"]
        .clone();
    let (status, _) = common::call(
        &app,
        "PATCH",
        &format!("/v1/devices/{phone_id}"),
        Some(laptop_tokens.access()),
        Some(serde_json::json!({ "name_sealed": laptop_name })),
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    let step = laptop.start_list_devices();
    let Outcome::Devices(devices) = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("no devices");
    };
    let named: Vec<Option<String>> = devices.iter().map(|d| d.name.clone()).collect();
    assert_eq!(named, [Some("Work laptop".to_owned()), None]);
    assert!(
        !format!("{devices:?}").contains("Work laptop"),
        "names are redacted in Debug"
    );

    // The laptop revokes the phone, which is signed out at its next sync.
    let step = laptop.start_revoke_device(&phone_id);
    assert!(matches!(
        run(&app, &mut laptop, step).await.unwrap(),
        Outcome::DeviceUpdated
    ));
    let step = laptop.start_list_devices();
    let Outcome::Devices(devices) = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("no devices");
    };
    assert!(devices[1].revoked_at.is_some());
    let step = phone.start_sync();
    assert!(matches!(
        run(&app, &mut phone, step).await.unwrap(),
        Outcome::DeviceRevoked
    ));
    db.drop().await;
}
