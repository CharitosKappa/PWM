//! OPAQUE registration and login, sessions and devices (D26), end to end
//! through the HTTP API against a real Postgres.

mod common;

use axum::http::StatusCode;
use common::call;
use common::flows::*;
use pwm_crypto::{KdfParams, Secret};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn register_login_refresh_and_logout() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "  Alice@Example.com ").await;
    assert!(account.access().starts_with("pwma_"));
    assert_eq!(account.tokens["access_expires_in"], 900);
    assert_eq!(account.tokens["refresh_expires_in"], 90 * 24 * 3600);

    // Pre-login finds the account by its normalized email.
    let (status, pre) = call(
        &app,
        "POST",
        "/v1/auth/prelogin",
        None,
        Some(json!({ "email": "alice@example.com" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(pre["account_id"], json!(account.id));
    assert_eq!(unb64(&pre["salt"]), account.salt);
    assert_eq!(pre["kdf"]["memory_kib"], KdfParams::DEFAULT.memory_kib);

    let (status, me) = call(&app, "GET", "/v1/account", Some(account.access()), None).await;
    assert_eq!(status, StatusCode::OK, "{me}");
    assert_eq!(me["email"], "alice@example.com");
    // No billing on this server: every account is Premium (D6 §7).
    assert_eq!(me["plan"], "premium");
    assert_eq!(me["billing"], json!([]));
    assert_eq!(me["subscription"], json!(null));
    assert_eq!(unb64(&me["sealed_keyset"]), account.sealed_keyset);
    assert_eq!(unb64(&me["x25519_pub"]), account.keyset.x25519_public());

    // A second device logs in with the same credentials.
    let phone = new_id();
    let (status, phone_tokens) = login(&app, &account, &phone).await;
    assert_eq!(status, StatusCode::OK, "{phone_tokens}");
    let phone_access = phone_tokens["access_token"].as_str().unwrap();
    let (_, devices) = call(&app, "GET", "/v1/devices", Some(phone_access), None).await;
    let devices = devices.as_array().unwrap();
    assert_eq!(devices.len(), 2);
    let current: Vec<_> = devices.iter().filter(|d| d["current"] == true).collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0]["id"], json!(phone));
    let sealed = pwm_crypto::Envelope::from_bytes(&unb64(&current[0]["name_sealed"])).unwrap();
    assert_eq!(
        account
            .keyset
            .open_device_name(&sealed, account.id.as_bytes(), phone.as_bytes())
            .unwrap(),
        "Phone"
    );

    // Refresh replaces both tokens.
    let (status, renewed) = refresh(&app, account.refresh()).await;
    assert_eq!(status, StatusCode::OK, "{renewed}");
    let access = renewed["access_token"].as_str().unwrap();
    assert_ne!(access, account.access());
    assert_eq!(
        get_account(&app, account.access()).await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(get_account(&app, access).await, StatusCode::OK);

    // Logout ends the session; the other device stays signed in.
    let (status, _) = call(&app, "POST", "/v1/auth/logout", Some(access), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(get_account(&app, access).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        refresh(&app, renewed["refresh_token"].as_str().unwrap()).await,
        unauthorized()
    );
    assert_eq!(get_account(&app, phone_access).await, StatusCode::OK);
    db.drop().await;
}

#[tokio::test]
async fn unknown_emails_get_stable_fake_prelogin_answers() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "real@example.com").await;
    let ask = |email: &'static str| {
        let app = app.clone();
        async move {
            call(
                &app,
                "POST",
                "/v1/auth/prelogin",
                None,
                Some(json!({ "email": email })),
            )
            .await
        }
    };
    let (status, fake) = ask("nobody@example.com").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ask("NOBODY@example.com ").await.1, fake);
    assert_ne!(ask("other@example.com").await.1, fake);
    let (_, real) = ask("real@example.com").await;
    assert_eq!(real["account_id"], json!(account.id));
    // Same shape either way.
    let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
    assert_eq!(keys(&fake), keys(&real));
    assert_eq!(unb64(&fake["salt"]).len(), 16);
    assert_eq!(
        Uuid::parse_str(fake["account_id"].as_str().unwrap())
            .unwrap()
            .get_version_num(),
        4
    );
    db.drop().await;
}

#[tokio::test]
async fn every_login_failure_looks_the_same() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "a@example.com").await;
    let device = device_json(&account.id, &account.keyset, &new_id(), "X");

    // A finalization from another login attempt does not verify.
    let (_, login_a, _) = login_start(&app, &account).await;
    let (client_b, _, response_b) = login_start(&app, &account).await;
    let finalization_b = client_b
        .finish(&account.auth_secret, account.id.as_bytes(), &response_b)
        .unwrap();
    let finish = |login_id: Value, finalization: Vec<u8>| {
        let (app, device) = (app.clone(), device.clone());
        async move {
            call(
                &app,
                "POST",
                "/v1/auth/login/finish",
                None,
                Some(json!({ "login_id": login_id, "credential_finalization": b64(&finalization), "device": device })),
            )
            .await
        }
    };
    assert_eq!(
        finish(login_a, finalization_b.clone()).await,
        invalid_credentials()
    );
    // An unknown login id, and garbage.
    assert_eq!(
        finish(json!(new_id()), finalization_b).await,
        invalid_credentials()
    );
    let (_, login_c, _) = login_start(&app, &account).await;
    assert_eq!(finish(login_c, vec![0; 64]).await, invalid_credentials());

    // An unknown account gets a response of the same size, and the client
    // cannot finish it (as with a wrong password or Secret Key).
    let ghost = Account {
        id: new_id(),
        ..register(&app, "b@example.com").await
    };
    let (client, _, fake_response) = login_start(&app, &ghost).await;
    let (_, _, real_response) = login_start(&app, &account).await;
    assert_eq!(fake_response.len(), real_response.len());
    assert!(
        client
            .finish(&ghost.auth_secret, ghost.id.as_bytes(), &fake_response)
            .is_err()
    );
    let wrong = Account {
        auth_secret: Secret::new([9; 32]),
        ..register(&app, "c@example.com").await
    };
    let (client, _, response) = login_start(&app, &wrong).await;
    assert!(
        client
            .finish(&wrong.auth_secret, wrong.id.as_bytes(), &response)
            .is_err()
    );
    db.drop().await;
}

#[tokio::test]
async fn login_attempts_are_single_use_and_expire() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "a@example.com").await;
    let device = device_json(&account.id, &account.keyset, &account.device, "Laptop");
    let (client, login_id, response) = login_start(&app, &account).await;
    let finalization = client
        .finish(&account.auth_secret, account.id.as_bytes(), &response)
        .unwrap();
    let body = json!({ "login_id": login_id, "credential_finalization": b64(&finalization), "device": device });
    let (status, _) = call(
        &app,
        "POST",
        "/v1/auth/login/finish",
        None,
        Some(body.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        call(&app, "POST", "/v1/auth/login/finish", None, Some(body)).await,
        invalid_credentials()
    );

    let (client, login_id, response) = login_start(&app, &account).await;
    sqlx::query("UPDATE login_attempts SET expires_at = now() - interval '1 second'")
        .execute(&db.pool)
        .await
        .unwrap();
    let finalization = client
        .finish(&account.auth_secret, account.id.as_bytes(), &response)
        .unwrap();
    let body = json!({ "login_id": login_id, "credential_finalization": b64(&finalization),
                       "device": device_json(&account.id, &account.keyset, &account.device, "Laptop") });
    assert_eq!(
        call(&app, "POST", "/v1/auth/login/finish", None, Some(body)).await,
        invalid_credentials()
    );
    db.drop().await;
}

#[tokio::test]
async fn a_reused_refresh_token_ends_the_session() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "a@example.com").await;
    let (status, renewed) = refresh(&app, account.refresh()).await;
    assert_eq!(status, StatusCode::OK);
    // The replaced token comes back (a copy): the session ends for everyone.
    assert_eq!(refresh(&app, account.refresh()).await, unauthorized());
    assert_eq!(
        refresh(&app, renewed["refresh_token"].as_str().unwrap()).await,
        unauthorized()
    );
    assert_eq!(
        get_account(&app, renewed["access_token"].as_str().unwrap()).await,
        StatusCode::UNAUTHORIZED
    );
    // Logging in again works.
    assert_eq!(
        login(&app, &account, &account.device).await.0,
        StatusCode::OK
    );
    db.drop().await;
}

#[tokio::test]
async fn expired_or_malformed_tokens_are_refused() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "a@example.com").await;
    for bad in ["", "pwma_", "Bearer", account.refresh()] {
        assert_eq!(
            get_account(&app, bad).await,
            StatusCode::UNAUTHORIZED,
            "{bad}"
        );
    }
    let (status, body) = call(&app, "GET", "/v1/account", None, None).await;
    assert_eq!((status, body), unauthorized());
    assert_eq!(refresh(&app, account.access()).await, unauthorized());

    sqlx::query("UPDATE sessions SET access_expires_at = now() - interval '1 second'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        get_account(&app, account.access()).await,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE sessions SET refresh_expires_at = now() - interval '1 second'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(refresh(&app, account.refresh()).await, unauthorized());
    db.drop().await;
}

#[tokio::test]
async fn a_revoked_device_is_locked_out() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "a@example.com").await;
    let other = register(&app, "b@example.com").await;
    let phone = new_id();
    let (_, phone_tokens) = login(&app, &account, &phone).await;
    let phone_access = phone_tokens["access_token"].as_str().unwrap();

    // Another account's device, or an unknown one, is not found.
    let path = |id: &Uuid| format!("/v1/devices/{id}");
    assert_eq!(
        call(
            &app,
            "DELETE",
            &path(&other.device),
            Some(phone_access),
            None
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "DELETE", &path(&new_id()), Some(phone_access), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );

    // The phone revokes the laptop: its tokens stop working at once, and it
    // is told why, so it can clear its cache.
    let (status, _) = call(
        &app,
        "DELETE",
        &path(&account.device),
        Some(phone_access),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let revoked = (StatusCode::FORBIDDEN, json!({ "error": "device_revoked" }));
    assert_eq!(
        call(&app, "GET", "/v1/account", Some(account.access()), None).await,
        revoked
    );
    assert_eq!(refresh(&app, account.refresh()).await, revoked);
    // The revoked id cannot log in again; a new id can.
    assert_eq!(
        login(&app, &account, &account.device).await,
        (StatusCode::FORBIDDEN, json!({ "error": "device_revoked" }))
    );
    assert_eq!(login(&app, &account, &new_id()).await.0, StatusCode::OK);
    // Another account's device id is refused.
    assert_eq!(
        login(&app, &account, &other.device).await,
        (StatusCode::CONFLICT, json!({ "error": "device_conflict" }))
    );
    let (_, devices) = call(&app, "GET", "/v1/devices", Some(phone_access), None).await;
    let laptop = devices
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["id"] == json!(account.device))
        .unwrap();
    assert!(laptop["revoked_at"].is_i64());
    db.drop().await;
}

/// `PATCH /v1/devices/{id}` (D43 §6): only a sealed name of the padded size,
/// only for the account's own devices that are not revoked.
#[tokio::test]
async fn devices_are_renamed_with_a_sealed_name() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "a@example.com").await;
    let other = register(&app, "b@example.com").await;
    let phone = new_id();
    let (_, phone_tokens) = login(&app, &account, &phone).await;
    let phone_access = phone_tokens["access_token"].as_str().unwrap();
    let path = |id: &Uuid| format!("/v1/devices/{id}");
    let sealed = |device: &Uuid, name: &str| {
        let envelope = account
            .keyset
            .seal_device_name(account.id.as_bytes(), device.as_bytes(), name)
            .unwrap();
        json!({ "name_sealed": b64(&envelope.to_bytes()) })
    };

    // The phone names itself, then renames the laptop.
    for (device, name) in [(&phone, "Phone"), (&account.device, "Work laptop")] {
        let (status, body) = call(
            &app,
            "PATCH",
            &path(device),
            Some(phone_access),
            Some(sealed(device, name)),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }
    let (_, devices) = call(&app, "GET", "/v1/devices", Some(phone_access), None).await;
    for device in devices.as_array().unwrap() {
        let id: Uuid = device["id"].as_str().unwrap().parse().unwrap();
        let envelope = pwm_crypto::Envelope::from_bytes(&unb64(&device["name_sealed"])).unwrap();
        let name = account
            .keyset
            .open_device_name(&envelope, account.id.as_bytes(), id.as_bytes())
            .unwrap();
        assert_eq!(name, if id == phone { "Phone" } else { "Work laptop" });
    }

    // Anything but a v1 envelope of the padded size is refused.
    let invalid = (
        StatusCode::BAD_REQUEST,
        json!({ "error": "invalid_request" }),
    );
    let mut short = vec![1u8; 41];
    short[0] = 1;
    for body in [
        json!({ "name_sealed": b64(&short) }),
        json!({ "name_sealed": b64(&[0u8; 105]) }),
        json!({ "name_sealed": "not base64!" }),
        json!({}),
        json!({ "name_sealed": sealed(&phone, "x")["name_sealed"], "name": "x" }),
    ] {
        assert_eq!(
            call(&app, "PATCH", &path(&phone), Some(phone_access), Some(body)).await,
            invalid
        );
    }

    // Another account's device, an unknown one, or no session: refused.
    for device in [&other.device, &new_id()] {
        let (status, _) = call(
            &app,
            "PATCH",
            &path(device),
            Some(phone_access),
            Some(sealed(device, "x")),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    assert_eq!(
        call(
            &app,
            "PATCH",
            &path(&phone),
            None,
            Some(sealed(&phone, "x"))
        )
        .await,
        unauthorized()
    );

    // A revoked device keeps its last name.
    call(
        &app,
        "DELETE",
        &path(&account.device),
        Some(phone_access),
        None,
    )
    .await;
    assert_eq!(
        call(
            &app,
            "PATCH",
            &path(&account.device),
            Some(phone_access),
            Some(sealed(&account.device, "Old laptop")),
        )
        .await,
        (StatusCode::CONFLICT, json!({ "error": "device_revoked" }))
    );
    db.drop().await;
}

#[tokio::test]
async fn registration_is_validated() {
    let Some((db, app)) = setup().await else {
        return;
    };
    register(&app, "taken@example.com").await;
    let invalid = (
        StatusCode::BAD_REQUEST,
        json!({ "error": "invalid_request" }),
    );

    let (request, _) = registration(&app, "TAKEN@example.com").await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/auth/register/finish",
            None,
            Some(request)
        )
        .await,
        (StatusCode::CONFLICT, json!({ "error": "account_exists" }))
    );

    type Edit = fn(&mut Value);
    let edits: [Edit; 9] = [
        |r| r["email"] = json!("not an email"),
        |r| r["kdf"]["memory_kib"] = json!(8),
        |r| r["salt"] = json!(b64(&[1; 15])),
        |r| r["x25519_pub"] = json!(b64(&[1; 31])),
        |r| r["sealed_keyset"] = json!(b64(&[1; 10])),
        |r| r["registration_upload"] = json!(b64(&[1; 10])),
        |r| r["device"]["platform"] = json!("symbian"),
        |r| r["salt"] = json!("not base64!"),
        |r| r["unexpected"] = json!(1),
    ];
    for edit in edits {
        let (mut request, _) = registration(&app, "new@example.com").await;
        edit(&mut request);
        assert_eq!(
            call(
                &app,
                "POST",
                "/v1/auth/register/finish",
                None,
                Some(request.clone())
            )
            .await,
            invalid,
            "{request}"
        );
    }
    // A body that is not JSON gets the same plain error, without echoing it.
    let response = call(&app, "POST", "/v1/auth/prelogin", None, None).await;
    assert_eq!(response, invalid);
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/auth/register/start",
            None,
            Some(json!({ "account_id": "x" }))
        )
        .await,
        invalid
    );
    db.drop().await;
}
