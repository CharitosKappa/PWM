//! OPAQUE login, session tokens and device names (decision D26).

use pwm_crypto::{
    AccountKeyset, CryptoError, OpaqueLogin, OpaqueRegistration, Secret, ServerSecrets, TokenKind,
    generate_id, generate_token, opaque_record, token_hash,
};

fn secret(byte: u8) -> Secret<[u8; 32]> {
    Secret::new([byte; 32])
}

/// Registers `auth_secret` for `account_id` and returns the stored record.
fn register(
    server: &ServerSecrets,
    account_id: &[u8; 16],
    auth_secret: &Secret<[u8; 32]>,
) -> Vec<u8> {
    let (client, request) = OpaqueRegistration::start(auth_secret).unwrap();
    let response = server.register_respond(account_id, &request).unwrap();
    let upload = client.finish(auth_secret, account_id, &response).unwrap();
    opaque_record(&upload).unwrap()
}

/// Runs a login; `Ok` only if both sides accept.
fn login(
    server: &ServerSecrets,
    record: Option<&[u8]>,
    account_id: &[u8; 16],
    auth_secret: &Secret<[u8; 32]>,
) -> Result<(), CryptoError> {
    let (client, request) = OpaqueLogin::start(auth_secret)?;
    let (state, response) = server.login_respond(record, account_id, &request)?;
    let finalization = client.finish(auth_secret, account_id, &response)?;
    server.login_finish(&state, &finalization, account_id)
}

#[test]
fn register_then_login() {
    let server = ServerSecrets::generate().unwrap();
    let id = generate_id().unwrap();
    let record = register(&server, &id, &secret(1));
    login(&server, Some(&record), &id, &secret(1)).unwrap();
}

#[test]
fn wrong_auth_secret_fails() {
    let server = ServerSecrets::generate().unwrap();
    let id = generate_id().unwrap();
    let record = register(&server, &id, &secret(1));
    assert_eq!(
        login(&server, Some(&record), &id, &secret(2)),
        Err(CryptoError::AuthenticationFailed)
    );
}

#[test]
fn record_is_bound_to_the_account_id() {
    let server = ServerSecrets::generate().unwrap();
    let id = generate_id().unwrap();
    let other = generate_id().unwrap();
    let record = register(&server, &id, &secret(1));
    assert_eq!(
        login(&server, Some(&record), &other, &secret(1)),
        Err(CryptoError::AuthenticationFailed)
    );
}

#[test]
fn record_is_bound_to_the_server() {
    let server = ServerSecrets::generate().unwrap();
    let other_server = ServerSecrets::generate().unwrap();
    let id = generate_id().unwrap();
    let record = register(&server, &id, &secret(1));
    assert_eq!(
        login(&other_server, Some(&record), &id, &secret(1)),
        Err(CryptoError::AuthenticationFailed)
    );
}

#[test]
fn unknown_account_gets_a_response_of_the_same_size_and_fails() {
    let server = ServerSecrets::generate().unwrap();
    let id = generate_id().unwrap();
    let record = register(&server, &id, &secret(1));

    let (_, request) = OpaqueLogin::start(&secret(1)).unwrap();
    let (_, real) = server.login_respond(Some(&record), &id, &request).unwrap();
    let unknown = generate_id().unwrap();
    let (_, fake) = server.login_respond(None, &unknown, &request).unwrap();
    assert_eq!(real.len(), fake.len());
    assert_eq!(
        login(&server, None, &unknown, &secret(1)),
        Err(CryptoError::AuthenticationFailed)
    );
}

#[test]
fn malformed_messages_are_rejected() {
    let server = ServerSecrets::generate().unwrap();
    let id = generate_id().unwrap();
    assert_eq!(
        server.register_respond(&id, &[0u8; 5]),
        Err(CryptoError::InvalidMessage)
    );
    assert_eq!(opaque_record(&[1u8; 7]), Err(CryptoError::InvalidMessage));
    assert!(matches!(
        server.login_respond(Some(&[0u8; 3]), &id, &[0u8; 3]),
        Err(CryptoError::InvalidMessage)
    ));
    assert_eq!(
        server.login_finish(&[0u8; 9], &[0u8; 9], &id),
        Err(CryptoError::InvalidMessage)
    );
}

#[test]
fn server_secrets_round_trip_and_reject_other_blobs() {
    let server = ServerSecrets::generate().unwrap();
    let bytes = server.to_bytes();
    assert_eq!(bytes.expose_secret()[0], 1);
    let restored = ServerSecrets::from_bytes(bytes.expose_secret()).unwrap();

    // A record made under the original logs in under the restored secrets.
    let id = generate_id().unwrap();
    let record = register(&server, &id, &secret(3));
    login(&restored, Some(&record), &id, &secret(3)).unwrap();
    assert_eq!(
        server.prelogin_fake("a@example.com"),
        restored.prelogin_fake("a@example.com")
    );

    let mut other_version = bytes.expose_secret().clone();
    other_version[0] = 2;
    assert_eq!(
        ServerSecrets::from_bytes(&other_version).err(),
        Some(CryptoError::UnsupportedVersion(2))
    );
    let short = &bytes.expose_secret()[..40];
    assert!(ServerSecrets::from_bytes(short).is_err());
    assert!(!format!("{server:?}").contains(&format!("{:?}", &bytes.expose_secret()[1..5])));
}

#[test]
fn prelogin_fakes_are_stable_per_email_and_look_like_uuid_v4() {
    let server = ServerSecrets::generate().unwrap();
    let (id, salt) = server.prelogin_fake("a@example.com");
    assert_eq!(server.prelogin_fake("a@example.com"), (id, salt));
    assert_ne!(server.prelogin_fake("b@example.com").0, id);
    assert_ne!(&id[..], &salt[..]);
    assert_eq!(id[6] >> 4, 4);
    assert_eq!(id[8] >> 6, 0b10);
    // Another server answers differently for the same email.
    let other = ServerSecrets::generate().unwrap();
    assert_ne!(other.prelogin_fake("a@example.com"), (id, salt));
}

#[test]
fn tokens_have_their_prefix_and_hash_only_for_their_kind() {
    let access = generate_token(TokenKind::Access).unwrap();
    let refresh = generate_token(TokenKind::Refresh).unwrap();
    let access = access.expose_secret();
    let refresh = refresh.expose_secret();
    assert!(
        access.starts_with("pwma_") && access.len() == 5 + 43,
        "{access}"
    );
    assert!(refresh.starts_with("pwmr_") && refresh.len() == 5 + 43);
    assert_ne!(
        generate_token(TokenKind::Access).unwrap().expose_secret(),
        access
    );

    let hash = token_hash(TokenKind::Access, access).unwrap();
    assert_eq!(token_hash(TokenKind::Access, access), Some(hash));
    assert_eq!(token_hash(TokenKind::Refresh, access), None);
    assert_ne!(token_hash(TokenKind::Refresh, refresh), Some(hash));
}

#[test]
fn malformed_tokens_have_no_hash() {
    let token = generate_token(TokenKind::Access).unwrap();
    let token = token.expose_secret();
    for bad in [
        "",
        "pwma_",
        &token[..token.len() - 1],
        &format!("{token}A"),
        &format!("{}+{}", &token[..10], &token[11..]),
        &token.replacen("pwma_", "PWMA_", 1),
    ] {
        assert_eq!(token_hash(TokenKind::Access, bad), None, "{bad}");
    }
}

#[test]
fn device_name_round_trip_has_a_fixed_size() {
    let keyset = AccountKeyset::generate().unwrap();
    let account = generate_id().unwrap();
    let device = generate_id().unwrap();
    let short = keyset.seal_device_name(&account, &device, "Phone").unwrap();
    let long = keyset
        .seal_device_name(&account, &device, &"é".repeat(31))
        .unwrap();
    assert_eq!(short.to_bytes().len(), long.to_bytes().len());
    assert_eq!(
        keyset.open_device_name(&short, &account, &device).unwrap(),
        "Phone"
    );
    assert_eq!(
        keyset.open_device_name(&long, &account, &device).unwrap(),
        "é".repeat(31)
    );
    assert_eq!(
        keyset.seal_device_name(&account, &device, "").map(|_| ()),
        Ok(())
    );
}

#[test]
fn device_name_is_bound_and_limited() {
    let keyset = AccountKeyset::generate().unwrap();
    let account = generate_id().unwrap();
    let device = generate_id().unwrap();
    let other = generate_id().unwrap();
    let sealed = keyset
        .seal_device_name(&account, &device, "Laptop")
        .unwrap();
    assert_eq!(
        keyset.open_device_name(&sealed, &account, &other),
        Err(CryptoError::DecryptionFailed)
    );
    assert_eq!(
        keyset.open_device_name(&sealed, &other, &device),
        Err(CryptoError::DecryptionFailed)
    );
    assert_eq!(
        keyset
            .seal_device_name(&account, &device, &"x".repeat(64))
            .map(|_| ()),
        Err(CryptoError::DeviceNameRejected)
    );
    assert_eq!(
        keyset
            .seal_device_name(&account, &device, "bad\u{7}name")
            .map(|_| ()),
        Err(CryptoError::DeviceNameRejected)
    );
}

/// The stored login state is sealed: it carries the MAC the client must
/// send, so reading it must not let anyone finish the login.
#[test]
fn login_state_is_sealed_and_bound_to_the_account_and_server() {
    let server = ServerSecrets::generate().unwrap();
    let id = generate_id().unwrap();
    let record = register(&server, &id, &secret(1));
    let (client, request) = OpaqueLogin::start(&secret(1)).unwrap();
    let (state, response) = server.login_respond(Some(&record), &id, &request).unwrap();
    let finalization = client.finish(&secret(1), &id, &response).unwrap();

    let other_id = generate_id().unwrap();
    assert_eq!(
        server.login_finish(&state, &finalization, &other_id),
        Err(CryptoError::AuthenticationFailed)
    );
    let other_server = ServerSecrets::generate().unwrap();
    assert_eq!(
        other_server.login_finish(&state, &finalization, &id),
        Err(CryptoError::AuthenticationFailed)
    );
    let mut tampered = state.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert_eq!(
        server.login_finish(&tampered, &finalization, &id),
        Err(CryptoError::AuthenticationFailed)
    );
    server.login_finish(&state, &finalization, &id).unwrap();
}
