//! The app API (`pwm-client`, D31) end to end against this server and a
//! real Postgres: sign-up, first vault, items, sync, login on a second
//! device, unlock, and a snapshot reloaded offline.

mod common;

use axum::Router;
use common::driver::{T0, run, send};
use common::flows::setup;
use pwm_client::{
    Client, HttpRequest, KdfParams, MemoryStore, Outcome, Platform, SyncEvent, Tokens, new_id,
};
use pwm_vault::{Item, ItemId, ItemKind};
use serde_json::Value;

const EMAIL: &str = "Alice@Example.com";
const PASSWORD: &str = "correct horse battery staple 42";

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

/// D50 §1 and §5: a locked device pulls and uploads with its tokens only,
/// and after a refused refresh token it signs in again, keeping what it
/// had not sent.
#[tokio::test]
async fn a_locked_device_syncs_and_signs_in_again_keeping_unsent_changes() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let mut laptop = new_client();
    let step = laptop.start_signup(EMAIL, PASSWORD, &new_id().unwrap(), Platform::Macos, "Mac");
    let Outcome::SignedUp { secret_key } = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("not signed up");
    };
    let secret_key = secret_key.expose_secret().clone();
    let step = laptop.start_ensure_vault();
    let Outcome::VaultCreated { vault_id: vault } = run(&app, &mut laptop, step).await.unwrap()
    else {
        panic!("no vault");
    };

    let mut phone = new_client();
    let phone_id = new_id().unwrap();
    let step = phone.start_login(
        EMAIL,
        PASSWORD,
        &secret_key,
        &phone_id,
        Platform::Ios,
        "Phone",
    );
    run(&app, &mut phone, step).await.unwrap();
    let phone_tokens = latest_tokens(&mut phone);
    let step = phone.start_sync();
    run(&app, &mut phone, step).await.unwrap();

    // A passkey saved by the AutoFill extension, say, then the phone locks.
    phone.save_item(&vault, &note("Passkey").1, T0).unwrap();
    phone.lock();
    laptop.save_item(&vault, &note("Mail").1, T0).unwrap();
    let step = laptop.start_sync();
    run(&app, &mut laptop, step).await.unwrap();

    let tokens = Tokens::new(
        phone_tokens.access().to_owned(),
        phone_tokens.refresh().to_owned(),
    );
    let step = phone.start_locked_sync(tokens);
    let Outcome::Synced(report) = run(&app, &mut phone, step).await.unwrap() else {
        panic!("not synced");
    };
    assert_eq!((report.pulled, report.pushed), (1, 1));
    assert!(!phone.is_unlocked());
    // Its push token goes up while locked too (D50 §3).
    let tokens = Tokens::new(
        phone_tokens.access().to_owned(),
        phone_tokens.refresh().to_owned(),
    );
    let push = pwm_client::PushToken {
        app: "ios".to_owned(),
        token: "ab".repeat(32),
        sandbox: true,
    };
    let step = phone.start_push_token(tokens, Some(push));
    assert!(matches!(
        run(&app, &mut phone, step).await.unwrap(),
        Outcome::PushTokenSet
    ));
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM push_tokens")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
    let step = laptop.start_sync();
    run(&app, &mut laptop, step).await.unwrap();
    assert_eq!(titles(&laptop, &vault), ["Mail", "Passkey"]);
    let tokens = Tokens::new(
        phone_tokens.access().to_owned(),
        phone_tokens.refresh().to_owned(),
    );
    phone.unlock(PASSWORD, &secret_key, tokens).unwrap();
    assert_eq!(titles(&phone, &vault), ["Mail", "Passkey"]);

    // The session ends elsewhere; an edit made meanwhile is kept.
    let (status, _) = common::call(
        &app,
        "POST",
        "/v1/auth/logout",
        Some(phone_tokens.access()),
        None,
    )
    .await;
    assert_eq!(status, axum::http::StatusCode::NO_CONTENT);
    phone.save_item(&vault, &note("Offline").1, T0).unwrap();
    let step = phone.start_sync();
    assert!(matches!(
        run(&app, &mut phone, step).await.unwrap(),
        Outcome::SignedOut
    ));

    // Signing in again on the same device keeps the cache and sends it.
    let step = phone.start_login(EMAIL, PASSWORD, &secret_key, &phone_id, Platform::Ios, "");
    assert!(matches!(
        run(&app, &mut phone, step).await.unwrap(),
        Outcome::LoggedIn
    ));
    assert_eq!(titles(&phone, &vault), ["Mail", "Offline", "Passkey"]);
    let step = phone.start_sync();
    let Outcome::Synced(report) = run(&app, &mut phone, step).await.unwrap() else {
        panic!("not synced");
    };
    assert_eq!(report.pushed, 1);
    let step = laptop.start_sync();
    run(&app, &mut laptop, step).await.unwrap();
    assert_eq!(titles(&laptop, &vault), ["Mail", "Offline", "Passkey"]);
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

/// What the new device's step machine does with one answer.
async fn ask_step(
    app: &Router,
    ask: &mut pwm_client::ApprovalAsk,
    request: &HttpRequest,
) -> pwm_client::AskStep {
    ask.receive(&send(app, request).await).unwrap()
}

/// Approving a new device (D46) through the client against the real relay:
/// the new device asks, the signed-in one accepts, both show the same code,
/// and the Secret Key arrives; then a relay that swaps the keys is caught.
#[tokio::test]
async fn a_signed_in_device_approves_a_new_one() {
    use pwm_client::{ApprovalAsk, AskStep};
    let Some((db, app)) = setup().await else {
        return;
    };
    let mut laptop = new_client();
    let step = laptop.start_signup(
        EMAIL,
        PASSWORD,
        &new_id().unwrap(),
        Platform::Linux,
        "Laptop",
    );
    let Outcome::SignedUp { secret_key } = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("not signed up");
    };
    let secret_key = secret_key.expose_secret().clone();

    // N asks; nothing happens until E accepts.
    let (mut ask, create) = ApprovalAsk::start(EMAIL, Platform::Ios).unwrap();
    assert!(matches!(
        ask_step(&app, &mut ask, &create).await,
        AskStep::Wait { .. }
    ));
    let poll = ask.poll().unwrap();
    assert!(matches!(
        ask_step(&app, &mut ask, &poll).await,
        AskStep::Wait { .. }
    ));

    let step = laptop.start_list_approvals();
    let Outcome::Approvals(list) = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("no list");
    };
    assert_eq!(list.len(), 1);
    assert_eq!(
        (list[0].platform.as_str(), list[0].state.as_str()),
        ("ios", "pending")
    );
    assert!(list[0].code.is_none());
    let id = list[0].id.clone();
    let step = laptop.start_accept_approval(&id);
    assert!(matches!(
        run(&app, &mut laptop, step).await.unwrap(),
        Outcome::ApprovalUpdated
    ));
    // Completing before N revealed its key is refused on the device.
    assert_eq!(
        laptop
            .start_complete_approval(&id, &secret_key)
            .unwrap_err()
            .code(),
        "unknown_approval"
    );

    // N reveals, then shows the code.
    let poll = ask.poll().unwrap();
    let AskStep::Send(reveal) = ask_step(&app, &mut ask, &poll).await else {
        panic!("N did not reveal");
    };
    let AskStep::ShowCode { code: at_new } = ask_step(&app, &mut ask, &reveal).await else {
        panic!("no code at N");
    };
    let step = laptop.start_list_approvals();
    let Outcome::Approvals(list) = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("no list");
    };
    assert_eq!(
        list[0].code.as_deref(),
        Some(at_new.as_str()),
        "the same code on both screens"
    );

    // The user saw the codes match: E sends the Secret Key; N gets it once.
    let step = laptop.start_complete_approval(&id, &secret_key);
    assert!(matches!(
        run(&app, &mut laptop, step).await.unwrap(),
        Outcome::ApprovalUpdated
    ));
    let poll = ask.poll().unwrap();
    let AskStep::Approved { secret_key: got } = ask_step(&app, &mut ask, &poll).await else {
        panic!("not approved");
    };
    assert_eq!(got.expose_secret(), &secret_key);
    let poll = ask.poll().unwrap();
    assert!(
        matches!(ask_step(&app, &mut ask, &poll).await, AskStep::Ended),
        "handed out once"
    );

    // N logs in with it and the master password, as usual.
    let mut phone = new_client();
    let step = phone.start_login(
        EMAIL,
        PASSWORD,
        got.expose_secret(),
        &new_id().unwrap(),
        Platform::Ios,
        "iPhone",
    );
    assert!(matches!(
        run(&app, &mut phone, step).await.unwrap(),
        Outcome::LoggedIn
    ));

    // A relay that gives N its own key instead of E's: N's code differs.
    let (mut ask, create) = ApprovalAsk::start(EMAIL, Platform::Ios).unwrap();
    ask_step(&app, &mut ask, &create).await;
    let step = laptop.start_list_approvals();
    let Outcome::Approvals(list) = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("no list");
    };
    let id = list[0].id.clone();
    let step = laptop.start_accept_approval(&id);
    run(&app, &mut laptop, step).await.unwrap();
    let poll = ask.poll().unwrap();
    let mut answer = send(&app, &poll).await;
    let mut polled: Value = serde_json::from_slice(&answer.body).unwrap();
    let relay = pwm_crypto::ApprovalKeys::generate().unwrap();
    polled["existing_pub"] = Value::String(base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        relay.public(),
    ));
    answer.body = polled.to_string().into_bytes();
    let AskStep::Send(reveal) = ask.receive(&answer).unwrap() else {
        panic!("N did not reveal");
    };
    let AskStep::ShowCode { code: at_new } = ask_step(&app, &mut ask, &reveal).await else {
        panic!("no code at N");
    };
    let step = laptop.start_list_approvals();
    let Outcome::Approvals(list) = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("no list");
    };
    assert_ne!(
        list[0].code.as_deref(),
        Some(at_new.as_str()),
        "the user sees two codes"
    );
    // E's user denies; N's request ends.
    let step = laptop.start_deny_approval(&id);
    run(&app, &mut laptop, step).await.unwrap();
    let poll = ask.poll().unwrap();
    assert!(matches!(
        ask_step(&app, &mut ask, &poll).await,
        AskStep::Ended
    ));
    db.drop().await;
}

#[tokio::test]
async fn signing_out_ends_the_session_on_the_server() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let mut laptop = new_client();
    let step = laptop.start_signup(EMAIL, PASSWORD, &new_id().unwrap(), Platform::Macos, "Mac");
    let Outcome::SignedUp { secret_key } = run(&app, &mut laptop, step).await.unwrap() else {
        panic!("not signed up");
    };
    let secret_key = secret_key.expose_secret().clone();
    let laptop_tokens = latest_tokens(&mut laptop);
    let mut phone = new_client();
    let step = phone.start_login(
        EMAIL,
        PASSWORD,
        &secret_key,
        &new_id().unwrap(),
        Platform::Ios,
        "Phone",
    );
    run(&app, &mut phone, step).await.unwrap();
    let phone_tokens = latest_tokens(&mut phone);
    let copy = |t: &Tokens| Tokens::new(t.access().to_owned(), t.refresh().to_owned());
    let push = pwm_client::PushToken {
        app: "ios".to_owned(),
        token: "ab".repeat(32),
        sandbox: true,
    };
    let step = phone.start_push_token(copy(&phone_tokens), Some(push));
    run(&app, &mut phone, step).await.unwrap();

    // Locked, with the stored tokens (D62); then the device is wiped.
    phone.lock();
    let step = phone.start_sign_out(Some(copy(&phone_tokens)));
    assert!(matches!(
        run(&app, &mut phone, step).await.unwrap(),
        Outcome::LoggedOut
    ));
    phone.sign_out().unwrap();
    assert_eq!(
        common::flows::refresh(&app, phone_tokens.refresh()).await,
        common::flows::unauthorized()
    );
    assert_eq!(
        common::flows::get_account(&app, phone_tokens.access()).await,
        axum::http::StatusCode::UNAUTHORIZED
    );
    let pushes: i64 = sqlx::query_scalar("SELECT count(*) FROM push_tokens")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(pushes, 0);

    // The laptop stays signed in, then signs out unlocked.
    let step = laptop.start_sync();
    assert!(matches!(
        run(&app, &mut laptop, step).await.unwrap(),
        Outcome::Synced(_)
    ));
    let step = laptop.start_sign_out(None);
    assert!(matches!(
        run(&app, &mut laptop, step).await.unwrap(),
        Outcome::LoggedOut
    ));
    laptop.sign_out().unwrap();
    assert_eq!(
        common::flows::refresh(&app, laptop_tokens.refresh()).await,
        common::flows::unauthorized()
    );
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(sessions, 0);
    db.drop().await;
}
