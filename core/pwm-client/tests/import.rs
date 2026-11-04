//! Importing into a vault (D51 §2, §8, §9): preview, save with new ids,
//! duplicates skipped, folders reused, items too large left out.

use pwm_client::{Client, MemoryStore, Store, Tokens, detect_import, import_columns};
use pwm_crypto::{KdfParams, SymmetricKey, create_account};
use pwm_store::{AccountRecord, VaultRecord};
use pwm_vault::ItemId;
use serde_json::Value;

const T0: i64 = 1_700_000_000_000;
const PASSWORD: &str = "correct horse battery staple 42";

fn unlocked() -> (Client<MemoryStore>, String) {
    let account_id = *ItemId::generate().unwrap().as_bytes();
    let new = create_account(PASSWORD, &account_id, KdfParams::MOBILE_FLOOR).unwrap();
    let mut store = MemoryStore::new();
    store
        .put_account(&AccountRecord {
            account_id,
            salt: new.salt,
            kdf_params: new.params,
            sealed_keyset: new.sealed_keyset,
        })
        .unwrap();
    let vault_id = *ItemId::generate().unwrap().as_bytes();
    let wrapped = new
        .keyset
        .wrap_vault_key(&SymmetricKey::generate().unwrap(), &vault_id)
        .unwrap();
    store
        .put_vault(&VaultRecord {
            vault_id,
            wrapped_key: wrapped,
            sync_cursor: 0,
        })
        .unwrap();
    let mut client = Client::new(store);
    let secret_key = new.secret_key.to_text().expose_secret().clone();
    client
        .unlock(PASSWORD, &secret_key, Tokens::new("a".into(), "r".into()))
        .unwrap();
    (client, ItemId::from_bytes(vault_id).to_string())
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

const LASTPASS: &[u8] = b"url,username,password,totp,extra,name,grouping,fav\n\
https://bank.example,ada,pw,,,Bank,Finance\\Banks,1\n\
https://mail.example,ada,pw2,,,Mail,,0\n";

#[test]
fn a_file_is_previewed_then_saved_with_new_ids() {
    let (mut client, vault) = unlocked();
    assert_eq!(
        detect_import("export.csv", LASTPASS),
        Some(("lastpass".to_owned(), false))
    );
    let preview = client
        .prepare_import(None, "lastpass", LASTPASS, None, None, None, T0)
        .unwrap();
    assert_eq!((preview.logins, preview.folders), (2, 2));
    assert!(titles(&client, &vault).is_empty(), "nothing is saved yet");

    let done = client.finish_import(T0).unwrap();
    assert_eq!(done, preview);
    assert_eq!(
        titles(&client, &vault),
        ["Bank", "Banks", "Finance", "Mail"]
    );
    // Queued for upload like any new item.
    assert_eq!(client.store().pending_items().unwrap().len(), 4);
    assert_eq!(client.finish_import(T0).unwrap_err().code(), "not_running");
}

#[test]
fn importing_the_same_file_again_adds_nothing() {
    let (mut client, vault) = unlocked();
    client
        .prepare_import(None, "lastpass", LASTPASS, None, None, None, T0)
        .unwrap();
    client.finish_import(T0).unwrap();
    let again = client
        .prepare_import(None, "lastpass", LASTPASS, None, None, None, T0)
        .unwrap();
    assert_eq!((again.logins, again.folders), (0, 0));
    assert_eq!(again.left_out, [("duplicates".to_owned(), 2)]);
    client.finish_import(T0).unwrap();
    assert_eq!(
        titles(&client, &vault).len(),
        4,
        "folders reused, logins skipped"
    );

    // A changed login comes in, into the folders already there.
    let changed = b"url,username,password,totp,extra,name,grouping,fav\n\
https://bank.example,ada,new-pw,,,Bank,Finance\\Banks,1\n";
    let preview = client
        .prepare_import(None, "lastpass", changed, None, None, None, T0)
        .unwrap();
    assert_eq!((preview.logins, preview.folders), (1, 0));
    client.finish_import(T0).unwrap();
    assert_eq!(
        titles(&client, &vault),
        ["Bank", "Bank", "Banks", "Finance", "Mail"]
    );
}

#[test]
fn an_item_too_large_to_sync_is_left_out() {
    let (mut client, vault) = unlocked();
    let huge = "x".repeat(300 * 1024);
    let file = format!("name,url,username,password,note\nBig,,,,{huge}\nSmall,,,,note\n");
    let preview = client
        .prepare_import(None, "csv", file.as_bytes(), None, None, None, T0)
        .unwrap();
    assert_eq!(preview.logins, 2);
    let done = client.finish_import(T0).unwrap();
    assert_eq!(done.logins, 1);
    assert_eq!(done.left_out, [("too_large".to_owned(), 1)]);
    assert_eq!(titles(&client, &vault), ["Small"]);
}

#[test]
fn refusals_carry_their_codes() {
    let (mut client, _) = unlocked();
    let encrypted = br#"{"encrypted":true,"passwordProtected":true,"salt":"s","kdfType":0,"kdfIterations":5000,"data":"2.x|y|z","items":[]}"#;
    let code =
        |r: Result<pwm_client::ImportPreview, pwm_client::ClientError>| r.unwrap_err().code();
    assert_eq!(
        code(client.prepare_import(None, "bitwarden", encrypted, None, None, None, T0)),
        "import_password_needed"
    );
    assert_eq!(
        code(client.prepare_import(None, "keeper", b"not json", None, None, None, T0)),
        "import_unreadable"
    );
    assert_eq!(
        code(client.prepare_import(None, "nope", LASTPASS, None, None, None, T0)),
        "import_unreadable"
    );
    assert_eq!(
        code(client.prepare_import(
            Some("not-an-id"),
            "lastpass",
            LASTPASS,
            None,
            None,
            None,
            T0
        )),
        "invalid_id"
    );

    // Locked: nothing to check against, so nothing is read.
    client
        .prepare_import(None, "lastpass", LASTPASS, None, None, None, T0)
        .unwrap();
    client.lock();
    assert_eq!(
        code(client.prepare_import(None, "lastpass", LASTPASS, None, None, None, T0)),
        "locked"
    );
}

#[test]
fn the_column_picker_gets_the_header_and_a_suggestion() {
    let columns = import_columns(
        b"Title,URL,Username,Password,Notes,OTPAuth\nBank,https://b.example,ada,pw,,\n",
    )
    .unwrap();
    assert_eq!(columns.rows, 1);
    assert_eq!(columns.suggested.totp, Some(5));
}
