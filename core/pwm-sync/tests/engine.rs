//! The sync engine (D28) against an in-memory server, one rule per test.

mod common;

use common::*;
use pwm_crypto::{Aad, Envelope, seal};
use pwm_store::{Store, SyncState};
use pwm_sync::{
    BACKOFF_CAP_MS, HttpResponse, Step, SyncEngine, SyncEvent, SyncOutcome, backoff_delay_ms,
};
use pwm_vault::{Item, ItemId};

const T0: i64 = 1_700_000_000_000;

#[test]
fn items_reach_a_second_device() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut laptop = account.device(&mut server);
    let vault = create_vault(&mut laptop, &mut server);
    let a = add(&mut laptop, &vault, "Bank", T0);
    add(&mut laptop, &vault, "Mail", T0);

    let report = sync(&mut laptop, &mut server, T0);
    assert_eq!(report.pushed, 2);
    assert_eq!(state(&laptop, &vault, &a), Some((1, SyncState::Synced)));
    assert_eq!(server.vault(&vault).seq, 2);

    let mut phone = account.device(&mut server);
    let report = sync(&mut phone, &mut server, T0);
    assert_eq!(report.pulled, 2);
    assert_eq!(titles(&phone, &vault), ["Bank", "Mail"]);
    assert_eq!(
        phone.item(&vault, &a).unwrap(),
        laptop.item(&vault, &a).unwrap()
    );
    assert_eq!(phone.store().vaults().unwrap()[0].sync_cursor, 2);

    // Nothing to do the second time.
    let report = sync(&mut phone, &mut server, T0);
    assert_eq!((report.pulled, report.pushed), (0, 0));
}

#[test]
fn local_changes_follow_the_revision_rules() {
    let mut server = FakeServer::new();
    let mut device = Account::new().device(&mut server);
    let vault = create_vault(&mut device, &mut server);

    // New: revision 1, uploaded with If-None-Match.
    let id = add(&mut device, &vault, "v1", T0);
    assert_eq!(state(&device, &vault, &id), Some((1, SyncState::Modified)));
    edit(&mut device, &vault, &id, "v1b", T0 + 1);
    assert_eq!(state(&device, &vault, &id), Some((1, SyncState::Modified)));
    sync(&mut device, &mut server, T0);
    assert!(
        server
            .log
            .iter()
            .any(|l| l.starts_with("PUT") && l.ends_with("if-none-match=*"))
    );

    // An edit is sealed at base + 1; edits before upload keep that revision.
    edit(&mut device, &vault, &id, "v2", T0 + 2);
    assert_eq!(state(&device, &vault, &id), Some((2, SyncState::Modified)));
    edit(&mut device, &vault, &id, "v2b", T0 + 3);
    assert_eq!(state(&device, &vault, &id), Some((2, SyncState::Modified)));
    sync(&mut device, &mut server, T0);
    assert!(server.log.last().unwrap().ends_with("if-match=\"1\""));
    assert_eq!(state(&device, &vault, &id), Some((2, SyncState::Synced)));

    // An unsent edit then a delete: the delete targets the server's revision.
    edit(&mut device, &vault, &id, "v3", T0 + 4);
    device.delete_item(&vault, &id).unwrap();
    assert_eq!(state(&device, &vault, &id), Some((2, SyncState::Deleted)));
    assert_eq!(device.item(&vault, &id).unwrap(), None);
    assert!(device.save_item(&vault, &id, &note("again", T0)).is_err());
    sync(&mut device, &mut server, T0);
    assert!(server.log.last().unwrap().starts_with("DELETE"));
    assert!(server.log.last().unwrap().ends_with("if-match=\"2\""));
    assert_eq!(state(&device, &vault, &id), None);
    assert_eq!(server.vault(&vault).tombstones[id.as_bytes()].0, 2);

    // Deleting an item that was never confirmed still tells the server,
    // which may have it (D50 §4). Here it doesn't: the 404 removes it.
    let local = add(&mut device, &vault, "draft", T0);
    device.delete_item(&vault, &local).unwrap();
    assert_eq!(
        state(&device, &vault, &local),
        Some((1, SyncState::Deleted))
    );
    assert_eq!(device.item(&vault, &local).unwrap(), None);
    sync(&mut device, &mut server, T0);
    assert!(server.log.last().unwrap().starts_with("DELETE"));
    assert!(server.log.last().unwrap().ends_with("if-match=\"1\""));
    assert_eq!(state(&device, &vault, &local), None);
    assert!(
        !server
            .vault(&vault)
            .tombstones
            .contains_key(local.as_bytes())
    );
}

#[test]
fn a_delete_after_a_lost_upload_answer_stays_deleted() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut device = account.device(&mut server);
    let vault = create_vault(&mut device, &mut server);
    let id = add(&mut device, &vault, "draft", T0);

    // The upload lands but the answer is lost; then the user deletes it.
    server.fault("PUT", Fault::LoseResponse);
    let step = device.start_sync().unwrap();
    assert!(matches!(
        drive(&mut device, &mut server, step, T0),
        Ran::Wait(_)
    ));
    assert!(server.vault(&vault).items.contains_key(id.as_bytes()));
    device.delete_item(&vault, &id).unwrap();

    // The pull brings our upload back, which doesn't revive it; the push
    // deletes it on the server.
    sync(&mut device, &mut server, T0);
    assert_eq!(state(&device, &vault, &id), None);
    assert!(titles(&device, &vault).is_empty());
    assert!(!server.vault(&vault).items.contains_key(id.as_bytes()));
    assert_eq!(server.vault(&vault).tombstones[id.as_bytes()].0, 1);

    let mut other = account.device(&mut server);
    sync(&mut other, &mut server, T0);
    assert!(titles(&other, &vault).is_empty());
}

#[test]
fn a_delete_of_an_unconfirmed_item_another_device_edited_keeps_the_edit() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut device = account.device(&mut server);
    let vault = create_vault(&mut device, &mut server);
    let id = add(&mut device, &vault, "draft", T0);
    server.fault("PUT", Fault::LoseResponse);
    let step = device.start_sync().unwrap();
    drive(&mut device, &mut server, step, T0);

    // Another device pulls it and edits it before this one hears back.
    let mut other = account.device(&mut server);
    sync(&mut other, &mut server, T0);
    edit(&mut other, &vault, &id, "draft (edited)", T0 + 1);
    sync(&mut other, &mut server, T0);

    device.delete_item(&vault, &id).unwrap();
    sync(&mut device, &mut server, T0);
    assert_eq!(titles(&device, &vault), ["draft (edited)"]);
}

#[test]
fn concurrent_edits_keep_both_versions() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut a = account.device(&mut server);
    let vault = create_vault(&mut a, &mut server);
    let id = add(&mut a, &vault, "base", T0);
    sync(&mut a, &mut server, T0);
    let mut b = account.device(&mut server);
    sync(&mut b, &mut server, T0);

    // Both edit offline; B's edit is later.
    edit(&mut a, &vault, &id, "from A", T0 + 10);
    edit(&mut b, &vault, &id, "from B", T0 + 20);
    sync(&mut a, &mut server, T0 + 30);
    let report = sync(&mut b, &mut server, T0 + 40);
    assert_eq!(report.conflicts, 1);
    sync(&mut a, &mut server, T0 + 50);

    for device in [&a, &b] {
        let item = device.item(&vault, &id).unwrap().unwrap();
        assert_eq!(item.title, "from B");
        let history = history_titles(&item);
        assert!(history.contains(&"from A".to_owned()), "{history:?}");
        assert!(history.contains(&"base".to_owned()), "{history:?}");
        assert_eq!(state(device, &vault, &id), Some((3, SyncState::Synced)));
    }
    assert_eq!(titles(&a, &vault), ["from B"]);

    // A tie keeps the server's version current.
    edit(&mut a, &vault, &id, "tie A", T0 + 60);
    edit(&mut b, &vault, &id, "tie B", T0 + 60);
    sync(&mut a, &mut server, T0 + 70);
    sync(&mut b, &mut server, T0 + 70);
    let item = b.item(&vault, &id).unwrap().unwrap();
    assert_eq!(item.title, "tie A");
    assert!(history_titles(&item).contains(&"tie B".to_owned()));
}

#[test]
fn a_conflict_found_while_pushing_is_merged_and_retried() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut a = account.device(&mut server);
    let vault = create_vault(&mut a, &mut server);
    let id = add(&mut a, &vault, "base", T0);
    sync(&mut a, &mut server, T0);
    let mut b = account.device(&mut server);
    sync(&mut b, &mut server, T0);

    // B has pulled and is about to upload when A's edit lands.
    edit(&mut b, &vault, &id, "from B", T0 + 20);
    let put = run_until(&mut b, &mut server, "PUT", T0);
    edit(&mut a, &vault, &id, "from A", T0 + 10);
    sync(&mut a, &mut server, T0);
    let response = server.handle(&put).unwrap();
    assert_eq!(response.status, 412);
    let step = b.receive(response, T0 + 30).unwrap();
    let Ran::Done(SyncOutcome::Synced(report)) = drive(&mut b, &mut server, step, T0 + 30) else {
        panic!("B did not finish");
    };
    assert_eq!(report.conflicts, 1);
    let item = b.item(&vault, &id).unwrap().unwrap();
    assert_eq!(item.title, "from B");
    assert!(history_titles(&item).contains(&"from A".to_owned()));
    assert_eq!(state(&b, &vault, &id), Some((3, SyncState::Synced)));
}

#[test]
fn a_lost_response_is_recognised_as_our_own_write() {
    let mut server = FakeServer::new();
    let mut device = Account::new().device(&mut server);
    let vault = create_vault(&mut device, &mut server);
    let id = add(&mut device, &vault, "one", T0);

    // The upload lands but the answer is lost.
    server.fault("PUT", Fault::LoseResponse);
    let step = device.start_sync().unwrap();
    assert!(matches!(
        drive(&mut device, &mut server, step, T0),
        Ran::Wait(_)
    ));
    assert_eq!(state(&device, &vault, &id), Some((1, SyncState::Modified)));
    assert_eq!(server.vault(&vault).items.len(), 1);

    // The next run pulls it back and sees the same ciphertext: no conflict.
    let report = sync(&mut device, &mut server, T0);
    assert_eq!(report.conflicts, 0);
    assert_eq!(state(&device, &vault, &id), Some((1, SyncState::Synced)));
    assert!(
        device
            .item(&vault, &id)
            .unwrap()
            .unwrap()
            .history()
            .is_empty()
    );
    assert_eq!(titles(&device, &vault), ["one"]);

    // An update lands, its answer is lost and a resend of the same request
    // gets 412 with our own ciphertext: also not a conflict.
    edit(&mut device, &vault, &id, "two", T0 + 1);
    let put = run_until(&mut device, &mut server, "PUT", T0);
    server.handle(&put).unwrap();
    let response = server.handle(&put).unwrap();
    assert_eq!(response.status, 412);
    let step = device.receive(response, T0).unwrap();
    let Ran::Done(SyncOutcome::Synced(report)) = drive(&mut device, &mut server, step, T0) else {
        panic!("did not finish");
    };
    assert_eq!(report.conflicts, 0);
    assert_eq!(state(&device, &vault, &id), Some((2, SyncState::Synced)));
    assert_eq!(server.vault(&vault).seq, 2);
}

#[test]
fn an_edit_of_a_deleted_item_is_kept_as_a_new_item() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut a = account.device(&mut server);
    let vault = create_vault(&mut a, &mut server);
    let id = add(&mut a, &vault, "base", T0);
    sync(&mut a, &mut server, T0);
    let mut b = account.device(&mut server);
    sync(&mut b, &mut server, T0);

    // Found while pulling (tombstone).
    a.delete_item(&vault, &id).unwrap();
    sync(&mut a, &mut server, T0);
    edit(&mut b, &vault, &id, "edited on B", T0 + 5);
    sync(&mut b, &mut server, T0 + 10);
    let items = b.items(&vault).unwrap();
    assert_eq!(items.len(), 1);
    let (copy, item) = &items[0];
    assert_ne!(copy, &id);
    assert_eq!(item.title, "edited on B");
    assert_eq!(state(&b, &vault, copy), Some((1, SyncState::Synced)));
    assert_eq!(state(&b, &vault, &id), None);
    sync(&mut a, &mut server, T0 + 20);
    assert_eq!(titles(&a, &vault), ["edited on B"]);

    // Found while pushing (409 item_deleted).
    let id = *copy;
    edit(&mut b, &vault, &id, "second edit", T0 + 30);
    let put = run_until(&mut b, &mut server, "PUT", T0);
    a.delete_item(&vault, &id).unwrap();
    sync(&mut a, &mut server, T0);
    let response = server.handle(&put).unwrap();
    assert_eq!(response.status, 409);
    let step = b.receive(response, T0 + 40).unwrap();
    assert!(matches!(
        drive(&mut b, &mut server, step, T0 + 40),
        Ran::Done(_)
    ));
    sync(&mut a, &mut server, T0 + 50);
    for device in [&a, &b] {
        assert_eq!(titles(device, &vault), ["second edit"]);
        assert_eq!(state(device, &vault, &id), None);
    }
}

#[test]
fn a_delete_never_removes_a_newer_edit() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut a = account.device(&mut server);
    let vault = create_vault(&mut a, &mut server);
    let id = add(&mut a, &vault, "base", T0);
    sync(&mut a, &mut server, T0);
    let mut b = account.device(&mut server);
    sync(&mut b, &mut server, T0);

    // Found while pulling.
    edit(&mut a, &vault, &id, "newer", T0 + 5);
    sync(&mut a, &mut server, T0);
    b.delete_item(&vault, &id).unwrap();
    sync(&mut b, &mut server, T0);
    // Settled while pulling: the stale delete is never sent.
    assert!(!server.log.iter().any(|l| l.starts_with("DELETE")));
    assert_eq!(titles(&b, &vault), ["newer"]);
    assert_eq!(state(&b, &vault, &id), Some((2, SyncState::Synced)));

    // Found while pushing (412 on the DELETE).
    b.delete_item(&vault, &id).unwrap();
    let delete = run_until(&mut b, &mut server, "DELETE", T0);
    edit(&mut a, &vault, &id, "newest", T0 + 9);
    sync(&mut a, &mut server, T0);
    let response = server.handle(&delete).unwrap();
    assert_eq!(response.status, 412);
    let step = b.receive(response, T0).unwrap();
    assert!(matches!(drive(&mut b, &mut server, step, T0), Ran::Done(_)));
    assert_eq!(titles(&b, &vault), ["newest"]);
    assert_eq!(state(&b, &vault, &id), Some((3, SyncState::Synced)));

    // Deleted on both sides.
    a.delete_item(&vault, &id).unwrap();
    b.delete_item(&vault, &id).unwrap();
    sync(&mut a, &mut server, T0);
    sync(&mut b, &mut server, T0);
    assert_eq!(state(&b, &vault, &id), None);
    assert!(titles(&b, &vault).is_empty());
}

/// Seals `json` as an item record v1 (D18) by hand, for versions the
/// current core cannot write.
fn seal_raw(
    key: &pwm_crypto::SymmetricKey,
    vault: &pwm_crypto::Id,
    id: &ItemId,
    revision: u64,
    json: &str,
) -> Vec<u8> {
    let mut plain = json.as_bytes().to_vec();
    plain.push(0x80);
    plain.resize(plain.len().div_ceil(256) * 256, 0);
    let aad = Aad::item(1, vault, id.as_bytes(), revision);
    seal(key, &plain, &aad).unwrap().to_bytes()
}

#[test]
fn versions_that_cannot_be_merged_are_kept_side_by_side() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut a = account.device(&mut server);
    let vault = create_vault(&mut a, &mut server);
    let id = add(&mut a, &vault, "base", T0);
    sync(&mut a, &mut server, T0);

    // A newer client rewrote the item in a schema this core reads only.
    let wrapped = Envelope::from_bytes(&server.vault(&vault).wrapped_key).unwrap();
    let key = &account.keyset().unwrap_vault_key(&wrapped, &vault).unwrap();
    let json = r#"{"v":2,"type":"note","title":"from the future","created_at":0,"updated_at":0}"#;
    let blob = seal_raw(key, &vault, &id, 2, json);
    let fake = server.vaults.get_mut(&vault).unwrap();
    fake.seq += 1;
    fake.items.insert(*id.as_bytes(), (2, fake.seq, blob));

    edit(&mut a, &vault, &id, "local edit", T0 + 99_999_999);
    let report = sync(&mut a, &mut server, T0);
    assert_eq!(report.conflicts, 1);
    let server_side = a.item(&vault, &id).unwrap().unwrap();
    assert_eq!(server_side.title, "from the future");
    assert!(!server_side.is_editable());
    assert_eq!(state(&a, &vault, &id), Some((2, SyncState::Synced)));
    assert_eq!(titles(&a, &vault), ["from the future", "local edit"]);
    assert_eq!(server.vault(&vault).items.len(), 2);
}

#[test]
fn failures_back_off_and_recover() {
    let mut server = FakeServer::new();
    let mut device = Account::new().device(&mut server);
    let vault = create_vault(&mut device, &mut server);
    add(&mut device, &vault, "x", T0);

    server.fault("GET /v1/vaults", Fault::Status(503, None));
    let step = device.start_sync().unwrap();
    let Ran::Wait(first) = drive(&mut device, &mut server, step, T0) else {
        panic!("expected a wait");
    };
    assert!(first <= 1_000);
    assert!(!device.is_running());

    server.fault("GET /v1/vaults", Fault::Network);
    let step = device.start_sync().unwrap();
    let Ran::Wait(second) = drive(&mut device, &mut server, step, T0) else {
        panic!("expected a wait");
    };
    assert!(second <= 2_000);

    // Retry-After is a floor.
    server.fault("PUT", Fault::Status(429, Some("30")));
    let step = device.start_sync().unwrap();
    let Ran::Wait(third) = drive(&mut device, &mut server, step, T0) else {
        panic!("expected a wait");
    };
    assert!((30_000..=34_000).contains(&third), "{third}");

    let report = sync(&mut device, &mut server, T0);
    assert_eq!(report.pushed, 1);

    // The schedule: full jitter under a doubling cap.
    assert_eq!(backoff_delay_ms(0, 0.999_999, None), 999);
    assert_eq!(backoff_delay_ms(3, 0.5, None), 4_000);
    assert_eq!(backoff_delay_ms(30, 1.0, None), BACKOFF_CAP_MS);
    assert_eq!(backoff_delay_ms(0, 0.0, Some(7_000)), 7_000);
}

#[test]
fn a_rejected_item_stays_queued_and_others_still_sync() {
    let mut server = FakeServer::new();
    let mut device = Account::new().device(&mut server);
    let vault = create_vault(&mut device, &mut server);
    let first = add(&mut device, &vault, "first", T0);
    let second = add(&mut device, &vault, "second", T0);

    let path_of = |id: &ItemId| format!("PUT /v1/vaults/{}/items/{id}", ItemId::from_bytes(vault));
    server.fault(&path_of(&first), Fault::Status(413, None));
    let report = sync(&mut device, &mut server, T0);
    assert_eq!((report.pushed, report.failed), (1, 1));
    assert_eq!(
        state(&device, &vault, &first),
        Some((1, SyncState::Modified))
    );
    assert_eq!(
        state(&device, &vault, &second),
        Some((1, SyncState::Synced))
    );
    let events = device.take_events();
    assert!(events.iter().any(|e| matches!(e,
        SyncEvent::ItemFailed { item_id, status: 413, .. } if *item_id == first)));

    sync(&mut device, &mut server, T0);
    assert_eq!(state(&device, &vault, &first), Some((1, SyncState::Synced)));
}

#[test]
fn an_expired_access_token_is_refreshed_once() {
    let mut server = FakeServer::new();
    let mut device = Account::new().device(&mut server);
    let vault = create_vault(&mut device, &mut server);
    let id = add(&mut device, &vault, "x", T0);

    server.fault("PUT", Fault::ExpireAccess);
    let report = sync(&mut device, &mut server, T0);
    assert_eq!(report.pushed, 1);
    let events = device.take_events();
    let refreshed: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            SyncEvent::TokensRefreshed(tokens) => Some(tokens),
            _ => None,
        })
        .collect();
    assert_eq!(refreshed.len(), 1);
    assert!(refreshed[0].access().starts_with("pwma_"));
    assert!(format!("{events:?}").contains("REDACTED"));
    assert!(!format!("{events:?}").contains(refreshed[0].access()));

    // When the refresh token is gone too, the device is signed out and
    // keeps its unsent changes.
    edit(&mut device, &vault, &id, "unsent", T0 + 1);
    server.end_sessions();
    let step = device.start_sync().unwrap();
    assert!(matches!(
        drive(&mut device, &mut server, step, T0),
        Ran::Done(SyncOutcome::SignedOut)
    ));
    assert_eq!(state(&device, &vault, &id), Some((2, SyncState::Modified)));
}

#[test]
fn a_revoked_device_clears_its_cache() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let tokens = server.new_device();
    let mut device = SyncEngine::new(
        pwm_store::SqliteStore::open_in_memory().unwrap(),
        account.keyset(),
        pwm_sync::Tokens::new(tokens.access().to_owned(), tokens.refresh().to_owned()),
    )
    .unwrap();
    let vault = create_vault(&mut device, &mut server);
    add(&mut device, &vault, "secret", T0);
    sync(&mut device, &mut server, T0);
    add(&mut device, &vault, "unsent", T0);

    server.revoke(&tokens);
    let step = device.start_sync().unwrap();
    assert!(matches!(
        drive(&mut device, &mut server, step, T0),
        Ran::Done(SyncOutcome::DeviceRevoked)
    ));
    assert!(device.store().vaults().unwrap().is_empty());
    assert!(device.store().pending_items().unwrap().is_empty());

    // Also when it is found out while refreshing an expired token.
    let tokens = server.new_device();
    let mut other = SyncEngine::new(
        pwm_store::SqliteStore::open_in_memory().unwrap(),
        account.keyset(),
        pwm_sync::Tokens::new(tokens.access().to_owned(), tokens.refresh().to_owned()),
    )
    .unwrap();
    sync(&mut other, &mut server, T0);
    assert_eq!(other.store().vaults().unwrap().len(), 1);
    server.revoke(&tokens);
    server.fault("GET /v1/vaults", Fault::ExpireAccess);
    let step = other.start_sync().unwrap();
    assert!(matches!(
        drive(&mut other, &mut server, step, T0),
        Ran::Done(SyncOutcome::DeviceRevoked)
    ));
    assert!(other.store().vaults().unwrap().is_empty());
}

#[test]
fn a_crash_between_pages_repeats_only_that_page() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut writer = account.device(&mut server);
    let vault = create_vault(&mut writer, &mut server);
    for n in 0..5 {
        add(&mut writer, &vault, &format!("item {n}"), T0);
    }
    sync(&mut writer, &mut server, T0);

    let mut reader = account.device(&mut server);
    reader.set_page_limit(2);
    // The second page is requested, then the app dies.
    let mut step = reader.start_sync().unwrap();
    let mut pages = 0;
    loop {
        let Step::Send(request) = step else {
            panic!("unexpected {step:?}")
        };
        if request.path.contains("/items") {
            pages += 1;
            if pages == 2 {
                break;
            }
        }
        step = reader
            .receive(server.handle(&request).unwrap(), T0)
            .unwrap();
    }
    let store = reader.into_store();
    assert_eq!(store.vaults().unwrap()[0].sync_cursor, 2);

    let mut reader = SyncEngine::new(store, account.keyset(), server.new_device()).unwrap();
    reader.set_page_limit(2);
    let report = sync(&mut reader, &mut server, T0);
    assert_eq!(report.pulled, 3);
    assert_eq!(titles(&reader, &vault).len(), 5);
}

#[test]
fn a_server_restored_from_backup_gets_newer_items_back() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut device = account.device(&mut server);
    let vault = create_vault(&mut device, &mut server);
    let kept = add(&mut device, &vault, "in the backup", T0);
    sync(&mut device, &mut server, T0);
    let backup = server.vaults.clone();
    let lost = add(&mut device, &vault, "after the backup", T0);
    edit(
        &mut device,
        &vault,
        &kept,
        "edited after the backup",
        T0 + 1,
    );
    sync(&mut device, &mut server, T0);
    assert_eq!(server.vault(&vault).seq, 3);

    server.vaults = backup;
    let report = sync(&mut device, &mut server, T0);
    assert!(report.resynced);
    // The lost item went up again as new; the edit is merged onto the
    // backup's version, which is older, so the edit stays current.
    assert_eq!(state(&device, &vault, &lost), Some((1, SyncState::Synced)));
    assert_eq!(server.vault(&vault).items.len(), 2);
    let mut other = account.device(&mut server);
    sync(&mut other, &mut server, T0);
    assert_eq!(
        titles(&other, &vault),
        ["after the backup", "edited after the backup"]
    );
}

#[test]
fn saving_while_an_upload_is_in_flight_keeps_the_new_edit() {
    let mut server = FakeServer::new();
    let mut device = Account::new().device(&mut server);
    let vault = create_vault(&mut device, &mut server);
    let id = add(&mut device, &vault, "sent", T0);
    let put = run_until(&mut device, &mut server, "PUT", T0);
    edit(&mut device, &vault, &id, "typed meanwhile", T0 + 1);
    let response = server.handle(&put).unwrap();
    let step = device.receive(response, T0).unwrap();
    assert!(matches!(
        drive(&mut device, &mut server, step, T0),
        Ran::Done(_)
    ));
    assert_eq!(state(&device, &vault, &id), Some((2, SyncState::Modified)));
    sync(&mut device, &mut server, T0);
    assert_eq!(state(&device, &vault, &id), Some((2, SyncState::Synced)));
    assert_eq!(
        device.item(&vault, &id).unwrap().unwrap().title,
        "typed meanwhile"
    );
}

#[test]
fn trash_due_for_purge_is_deleted_on_the_server() {
    let mut server = FakeServer::new();
    let mut device = Account::new().device(&mut server);
    let vault = create_vault(&mut device, &mut server);
    let old = add(&mut device, &vault, "old", T0);
    let fresh = add(&mut device, &vault, "fresh", T0);
    for (id, at) in [(old, T0), (fresh, T0 + pwm_vault::TRASH_RETENTION_MS)] {
        let mut item: Item = device.item(&vault, &id).unwrap().unwrap();
        item.trash(at).unwrap();
        device.save_item(&vault, &id, &item).unwrap();
    }
    sync(&mut device, &mut server, T0);
    let purged = device
        .purge_trash(T0 + pwm_vault::TRASH_RETENTION_MS)
        .unwrap();
    assert_eq!(purged, 1);
    sync(&mut device, &mut server, T0);
    assert!(server.vault(&vault).tombstones.contains_key(old.as_bytes()));
    assert!(server.vault(&vault).items.contains_key(fresh.as_bytes()));
}

#[test]
fn vaults_gone_from_the_server_are_removed() {
    let mut server = FakeServer::new();
    let mut device = Account::new().device(&mut server);
    let keep = create_vault(&mut device, &mut server);
    let gone = create_vault(&mut device, &mut server);
    add(&mut device, &gone, "x", T0);
    sync(&mut device, &mut server, T0);
    server.vaults.remove(&gone);
    sync(&mut device, &mut server, T0);
    let ids: Vec<_> = device
        .store()
        .vaults()
        .unwrap()
        .iter()
        .map(|v| v.vault_id)
        .collect();
    assert_eq!(ids, [keep]);
}

#[test]
fn only_one_run_at_a_time() {
    let mut server = FakeServer::new();
    let mut device = Account::new().device(&mut server);
    let _ = create_vault(&mut device, &mut server);
    let step = device.start_sync().unwrap();
    assert!(device.is_running());
    assert!(device.start_sync().is_err());
    assert!(device.start_create_vault().is_err());
    device.cancel();
    assert!(!device.is_running());
    assert!(
        device
            .receive(HttpResponse::new(200, Vec::new(), Vec::new()), T0)
            .is_err()
    );
    drop(step);
    sync(&mut device, &mut server, T0);
}

#[test]
fn requests_and_responses_never_print_secrets() {
    let mut server = FakeServer::new();
    let mut device = Account::new().device(&mut server);
    let step = device.start_sync().unwrap();
    let Step::Send(request) = &step else { panic!() };
    let printed = format!("{step:?} {request:?}");
    assert!(
        request
            .header("authorization")
            .unwrap()
            .starts_with("Bearer pwma_")
    );
    assert!(!printed.contains("pwma_"), "{printed}");
    let response = HttpResponse::new(200, Vec::new(), br#"{"refresh_token":"pwmr_x"}"#.to_vec());
    assert!(!format!("{response:?}").contains("pwmr_x"));
}

#[test]
fn an_item_too_large_to_sync_is_refused_when_saved() {
    let mut server = FakeServer::new();
    let mut device = Account::new().device(&mut server);
    let vault = create_vault(&mut device, &mut server);
    let mut item = note("big", T0);
    item.notes = "n".repeat(pwm_vault::MAX_SEALED_ITEM_BYTES);
    let id = ItemId::generate().unwrap();
    assert!(matches!(
        device.save_item(&vault, &id, &item),
        Err(pwm_sync::SyncError::Vault(pwm_vault::VaultError::TooLarge))
    ));
    assert_eq!(state(&device, &vault, &id), None);
}

#[test]
fn a_restore_the_seqs_cannot_show_is_caught_by_the_epoch() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut laptop = account.device(&mut server);
    let vault = create_vault(&mut laptop, &mut server);
    add(&mut laptop, &vault, "in the backup", T0);
    sync(&mut laptop, &mut server, T0);
    let epoch = laptop.store().server_epoch().unwrap();
    assert_eq!(epoch, Some(server.epoch), "the first epoch is recorded");
    let backup = server.vaults.clone();
    let lost = add(&mut laptop, &vault, "after the backup", T0);
    sync(&mut laptop, &mut server, T0);
    assert_eq!(laptop.store().vaults().unwrap()[0].sync_cursor, 2);

    // Restored, marked, and then another device writes past seq 2 before
    // the laptop syncs again: next >= since, so seqs alone look normal.
    server.vaults = backup;
    server.mark_restored();
    let mut phone = account.device(&mut server);
    sync(&mut phone, &mut server, T0);
    add(&mut phone, &vault, "phone 1", T0);
    add(&mut phone, &vault, "phone 2", T0);
    sync(&mut phone, &mut server, T0);
    assert_eq!(server.vault(&vault).seq, 3);

    let report = sync(&mut laptop, &mut server, T0);
    assert!(report.resynced);
    assert_eq!(laptop.store().server_epoch().unwrap(), Some(server.epoch));
    assert_eq!(state(&laptop, &vault, &lost), Some((1, SyncState::Synced)));
    let all = ["after the backup", "in the backup", "phone 1", "phone 2"];
    assert_eq!(titles(&laptop, &vault), all);
    sync(&mut phone, &mut server, T0);
    assert_eq!(titles(&phone, &vault), all);

    // The next sync is an ordinary one.
    assert!(!sync(&mut laptop, &mut server, T0).resynced);
}

#[test]
fn a_device_behind_compacted_tombstones_drops_what_was_deleted() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut a = account.device(&mut server);
    let vault = create_vault(&mut a, &mut server);
    let kept = add(&mut a, &vault, "kept", T0);
    let deleted = add(&mut a, &vault, "deleted", T0);
    let edited = add(&mut a, &vault, "deleted on A", T0);
    sync(&mut a, &mut server, T0);
    let mut b = account.device(&mut server);
    sync(&mut b, &mut server, T0);

    // While B is away, A deletes two items; B edits one of them and adds one.
    a.delete_item(&vault, &deleted).unwrap();
    a.delete_item(&vault, &edited).unwrap();
    sync(&mut a, &mut server, T0);
    edit(&mut b, &vault, &edited, "edited on B", T0 + 5);
    let added = add(&mut b, &vault, "new on B", T0 + 5);
    // Then the server compacts those tombstones.
    server.compact(&vault);

    server.log.clear();
    let report = sync(&mut b, &mut server, T0 + 10);
    assert!(report.resynced);
    let gets: Vec<&String> = server
        .log
        .iter()
        .filter(|l| l.starts_with("GET /v1/vaults/"))
        .collect();
    assert!(gets.len() >= 2 && gets[1].contains("since=0"), "{gets:?}");
    // The deletes reach B, nothing deleted comes back, and B's work stays:
    // the edit as a new item, the new item uploaded.
    assert_eq!(state(&b, &vault, &deleted), None);
    assert_eq!(state(&b, &vault, &edited), None);
    assert_eq!(state(&b, &vault, &kept), Some((1, SyncState::Synced)));
    assert_eq!(state(&b, &vault, &added), Some((1, SyncState::Synced)));
    assert_eq!(titles(&b, &vault), ["edited on B", "kept", "new on B"]);
    let on_server = &server.vault(&vault).items;
    assert!(!on_server.contains_key(deleted.as_bytes()));
    assert!(!on_server.contains_key(edited.as_bytes()));
    assert_eq!(on_server.len(), 3);

    // A device past the compacted deletes just syncs on.
    let report = sync(&mut a, &mut server, T0 + 20);
    assert!(!report.resynced);
    assert_eq!(titles(&a, &vault), ["edited on B", "kept", "new on B"]);
}
