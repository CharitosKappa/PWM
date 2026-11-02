//! Save and update offers (D36 §4, §6): when the client offers to save or
//! update a login after a sign-in, and what saving and updating write.

use pwm_client::{Client, ClientError, MemoryStore, SaveOffer, Store, Tokens, new_id};
use pwm_crypto::{KdfParams, SymmetricKey, create_account};
use pwm_store::{AccountRecord, VaultRecord};
use pwm_vault::{Item, ItemId, ItemKind};
use serde_json::Value;

const T0: i64 = 1_700_000_000_000;
const PASSWORD: &str = "correct horse battery staple 42";

/// An unlocked client with one empty vault; returns it and the vault id.
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
    let secret_key = new.secret_key.to_text().expose_secret().clone();
    let mut client = Client::new(store);
    client
        .unlock(
            PASSWORD,
            &secret_key,
            Tokens::new("pwma_x".to_owned(), "pwmr_x".to_owned()),
        )
        .unwrap();
    (client, ItemId::from_bytes(vault_id).to_string())
}

fn item(client: &Client<MemoryStore>, vault: &str, id: &str) -> Value {
    let json = client.item(vault, id).unwrap().unwrap();
    serde_json::from_str(json.expose_secret()).unwrap()
}

fn ids(offer: &SaveOffer) -> Vec<String> {
    match offer {
        SaveOffer::Update(candidates) => candidates.iter().map(|c| c.item_id.clone()).collect(),
        other => panic!("not an update: {other:?}"),
    }
}

fn code<T: std::fmt::Debug>(result: Result<T, ClientError>) -> &'static str {
    result.unwrap_err().code()
}

const PAGE: &str = "https://www.example.com/login?next=/home";

#[test]
fn a_new_site_is_offered_saving_and_the_saved_login_fills() {
    let (mut client, vault) = unlocked();
    let offer = client
        .save_offer(PAGE, None, "ada", "s3cret", None)
        .unwrap();
    assert_eq!(offer, SaveOffer::Save);

    let id = client
        .save_login(&vault, PAGE, "", " ada ", "s3cret", T0)
        .unwrap();
    let saved = item(&client, &vault, &id);
    assert_eq!(saved["type"], "login");
    // The host without `www.` when no title is given; the page's origin only.
    assert_eq!(saved["title"], "example.com");
    assert_eq!(saved["login"]["username"], "ada");
    assert_eq!(saved["login"]["password"], "s3cret");
    assert_eq!(saved["login"]["urls"][0]["url"], "https://www.example.com");
    assert_eq!(saved["login"]["urls"][0]["match"], "domain");
    assert_eq!(saved["created_at"], T0);

    let found = client.autofill_matches(PAGE, None).unwrap();
    assert_eq!(found[0].item_id, id);
    assert_eq!(found[0].decision, "fill");

    // Saved as is: nothing more to offer, whatever the username's case or spaces.
    for username in ["ada", "ADA", " ada"] {
        assert_eq!(
            client
                .save_offer(PAGE, None, username, "s3cret", None)
                .unwrap(),
            SaveOffer::Nothing
        );
    }
}

#[test]
fn a_given_title_is_kept() {
    let (mut client, vault) = unlocked();
    let id = client
        .save_login(&vault, PAGE, "Work mail", "ada", "s3cret", T0)
        .unwrap();
    assert_eq!(item(&client, &vault, &id)["title"], "Work mail");
}

#[test]
fn a_new_password_for_a_saved_username_is_offered_as_an_update() {
    let (mut client, vault) = unlocked();
    let id = client
        .save_login(&vault, PAGE, "", "ada", "old-password", T0)
        .unwrap();
    let offer = client
        .save_offer("https://example.com/", None, "ada", "new-password", None)
        .unwrap();
    assert_eq!(ids(&offer), std::slice::from_ref(&id));
    let SaveOffer::Update(candidates) = &offer else {
        unreachable!()
    };
    assert_eq!(candidates[0].vault_id, vault);
    assert_eq!(candidates[0].title, "example.com");
    assert_eq!(candidates[0].username, "ada");

    client
        .update_password(&vault, &id, "new-password", T0 + 1000)
        .unwrap();
    let updated = item(&client, &vault, &id);
    assert_eq!(updated["login"]["password"], "new-password");
    assert_eq!(updated["title"], "example.com");
    assert_eq!(
        updated["login"]["urls"][0]["url"],
        "https://www.example.com"
    );
    assert_eq!(updated["updated_at"], T0 + 1000);
    // The old password is kept in history (D19).
    assert_eq!(updated["history"][0]["login"]["password"], "old-password");
    assert_eq!(
        client
            .save_offer(PAGE, None, "ada", "new-password", None)
            .unwrap(),
        SaveOffer::Nothing
    );
}

#[test]
fn another_username_on_a_saved_site_is_a_new_login() {
    let (mut client, vault) = unlocked();
    client
        .save_login(&vault, PAGE, "", "ada", "s3cret", T0)
        .unwrap();
    assert_eq!(
        client
            .save_offer(PAGE, None, "bob", "s3cret", None)
            .unwrap(),
        SaveOffer::Save
    );
}

#[test]
fn a_change_password_form_updates_the_login_with_the_old_password() {
    let (mut client, vault) = unlocked();
    let id = client
        .save_login(&vault, PAGE, "", "ada", "old-password", T0)
        .unwrap();
    client
        .save_login(&vault, PAGE, "", "bob", "bobs-password", T0)
        .unwrap();
    // No username on the form: the old password says which login it is.
    let offer = client
        .save_offer(PAGE, None, "", "old-password", Some("new-password"))
        .unwrap();
    assert_eq!(ids(&offer), std::slice::from_ref(&id));
    // With the username too.
    let offer = client
        .save_offer(PAGE, None, "ada", "old-password", Some("new-password"))
        .unwrap();
    assert_eq!(ids(&offer), std::slice::from_ref(&id));
    // Already changed: nothing.
    client
        .update_password(&vault, &id, "new-password", T0 + 1)
        .unwrap();
    assert_eq!(
        client
            .save_offer(PAGE, None, "", "old-password", Some("new-password"))
            .unwrap(),
        SaveOffer::Nothing
    );
}

#[test]
fn a_sign_up_form_with_only_a_new_password_is_offered_saving() {
    let (client, _) = unlocked();
    assert_eq!(
        client
            .save_offer(PAGE, None, "ada", "", Some("brand-new"))
            .unwrap(),
        SaveOffer::Save
    );
}

#[test]
fn several_logins_with_that_username_are_all_candidates() {
    let (mut client, vault) = unlocked();
    let a = client
        .save_login(&vault, PAGE, "Personal", "ada", "one", T0)
        .unwrap();
    let b = client
        .save_login(
            &vault,
            "https://shop.example.com/",
            "Shop",
            "ada",
            "two",
            T0,
        )
        .unwrap();
    let mut found = ids(&client.save_offer(PAGE, None, "ada", "three", None).unwrap());
    found.sort();
    let mut expected = vec![a, b];
    expected.sort();
    assert_eq!(found, expected);
}

#[test]
fn no_offer_for_empty_passwords_frames_of_another_site_or_non_web_pages() {
    let (client, _) = unlocked();
    assert_eq!(
        client.save_offer(PAGE, None, "ada", "", None).unwrap(),
        SaveOffer::Nothing
    );
    assert_eq!(
        client.save_offer(PAGE, None, "ada", "", Some("")).unwrap(),
        SaveOffer::Nothing
    );
    assert_eq!(
        client
            .save_offer(PAGE, Some("https://evil.example/frame"), "ada", "x", None)
            .unwrap(),
        SaveOffer::Nothing
    );
    assert_eq!(
        client
            .save_offer("chrome://settings/", None, "ada", "x", None)
            .unwrap(),
        SaveOffer::Nothing
    );
    // A frame of the same site is fine.
    assert_eq!(
        client
            .save_offer(PAGE, Some("https://login.example.com/f"), "ada", "x", None)
            .unwrap(),
        SaveOffer::Save
    );
    // http pages may save; the extension shows a warning.
    assert_eq!(
        client
            .save_offer("http://plain.example/", None, "ada", "x", None)
            .unwrap(),
        SaveOffer::Save
    );
}

#[test]
fn bad_input_and_a_locked_vault_are_errors() {
    let (mut client, vault) = unlocked();
    assert_eq!(
        code(client.save_offer("not a url", None, "a", "b", None)),
        "invalid_url"
    );
    assert_eq!(
        code(client.save_login(&vault, "chrome://x/", "", "a", "b", T0)),
        "invalid_url"
    );
    assert_eq!(
        code(client.save_login(&vault, PAGE, "", "a", "", T0)),
        "invalid_item"
    );
    let note = new_id().unwrap();
    let json = Item::new(ItemKind::Note, "n", T0)
        .to_json_with_id(&note.parse().unwrap())
        .unwrap();
    client.save_item(&vault, json.expose_secret(), T0).unwrap();
    assert_eq!(
        code(client.update_password(&vault, &note, "x", T0)),
        "invalid_item"
    );
    assert_eq!(
        code(client.update_password(&vault, &new_id().unwrap(), "x", T0)),
        "unknown_item"
    );
    client.lock();
    assert_eq!(
        code(client.save_offer(PAGE, None, "a", "b", None)),
        "locked"
    );
    assert_eq!(
        code(client.save_login(&vault, PAGE, "", "a", "b", T0)),
        "locked"
    );
}
