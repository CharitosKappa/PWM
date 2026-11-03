//! The frozen passkey vectors (`core/test-vectors/passkey.json`): the same
//! inputs must give exactly the same bytes, so a change to the formats is
//! caught. Named `vectors_*` for `just test-vectors`.

use pwm_crypto::Secret;
use pwm_passkey::{
    Caller, CreateRequest, GetRequest, authenticate, authenticate_hashed, register, register_hashed,
};
use serde_json::Value;

#[test]
fn vectors_passkey() {
    let text = include_str!("../../test-vectors/passkey.json");
    let doc: Value = serde_json::from_str(text).unwrap();
    for v in doc["vectors"].as_array().unwrap() {
        let caller = Caller {
            origin: v["origin"].as_str().unwrap(),
            top_origin: None,
        };
        let seed: [u8; 32] = hex::decode(v["key_seed"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let id: [u8; 16] = hex::decode(v["credential_id"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        let create = CreateRequest::from_json(v["create_request"].as_str().unwrap()).unwrap();
        let registration = register(caller, &create, &Secret::new(seed), id).unwrap();
        let response: Value = serde_json::from_str(&registration.response_json).unwrap();
        assert_eq!(response, v["registration"]);
        assert_eq!(
            hex::encode(registration.passkey.private_key.expose_secret()),
            v["private_key_pkcs8"].as_str().unwrap()
        );
        let get = GetRequest::from_json(v["get_request"].as_str().unwrap()).unwrap();
        let assertion: Value =
            serde_json::from_str(&authenticate(caller, &get, &registration.passkey).unwrap())
                .unwrap();
        assert_eq!(assertion, v["assertion"]);
    }
}

fn bytes(v: &Value) -> Vec<u8> {
    hex::decode(v.as_str().unwrap()).unwrap()
}

#[test]
fn vectors_passkey_hashed() {
    let text = include_str!("../../test-vectors/passkey_hashed.json");
    let doc: Value = serde_json::from_str(text).unwrap();
    for v in doc["vectors"].as_array().unwrap() {
        let rp_id = v["rp_id"].as_str().unwrap();
        let seed: [u8; 32] = bytes(&v["key_seed"]).try_into().unwrap();
        let id: [u8; 16] = bytes(&v["credential_id"]).try_into().unwrap();
        let registration = register_hashed(
            rp_id,
            &bytes(&v["user_handle"]),
            v["user_name"].as_str().unwrap(),
            &Secret::new(seed),
            id,
        )
        .unwrap();
        assert_eq!(
            registration.attestation_object,
            bytes(&v["attestation_object"])
        );
        assert_eq!(
            registration.passkey.private_key.expose_secret(),
            &bytes(&v["private_key_pkcs8"])
        );
        let assertion =
            authenticate_hashed(rp_id, &bytes(&v["client_data_hash"]), &registration.passkey)
                .unwrap();
        assert_eq!(
            assertion.authenticator_data,
            bytes(&v["authenticator_data"])
        );
        assert_eq!(assertion.signature, bytes(&v["signature"]));
    }
}
