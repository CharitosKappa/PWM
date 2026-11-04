//! Syncing while locked (D50 §1): the engine runs with tokens only, stores
//! what needs no key as ciphertext, uploads what is queued, and leaves the
//! rest for the next unlocked sync.

mod common;

use common::*;
use pwm_store::{SqliteStore, Store, SyncState};
use pwm_sync::{SyncEngine, SyncError, SyncOutcome};

const T0: i64 = 1_700_000_000_000;

/// The same cache, without the keys.
fn lock(engine: Engine, server: &mut FakeServer) -> Engine {
    SyncEngine::locked(engine.into_store(), server.new_device()).unwrap()
}

fn unlock(engine: Engine, account: &Account, server: &mut FakeServer) -> Engine {
    SyncEngine::new(engine.into_store(), account.keyset(), server.new_device()).unwrap()
}

fn cursor(engine: &Engine) -> u64 {
    engine.store().vaults().unwrap()[0].sync_cursor
}

#[test]
fn a_locked_device_stores_new_changed_and_deleted_items() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut laptop = account.device(&mut server);
    let vault = create_vault(&mut laptop, &mut server);
    let bank = add(&mut laptop, &vault, "Bank", T0);
    let mail = add(&mut laptop, &vault, "Mail", T0);
    sync(&mut laptop, &mut server, T0);

    let mut phone = account.device(&mut server);
    sync(&mut phone, &mut server, T0);
    let mut phone = lock(phone, &mut server);

    edit(&mut laptop, &vault, &bank, "Bank (new)", T0 + 1);
    laptop.delete_item(&vault, &mail).unwrap();
    add(&mut laptop, &vault, "Shop", T0 + 2);
    sync(&mut laptop, &mut server, T0);

    let report = sync(&mut phone, &mut server, T0);
    assert_eq!(report.pulled, 3);
    assert_eq!(report.conflicts, 0);
    assert_eq!(state(&phone, &vault, &bank), Some((2, SyncState::Synced)));
    assert_eq!(state(&phone, &vault, &mail), None);
    assert_eq!(cursor(&phone), server.vault(&vault).seq);

    let phone = unlock(phone, &account, &mut server);
    assert_eq!(titles(&phone, &vault), ["Bank (new)", "Shop"]);
}

#[test]
fn a_locked_device_cannot_read_or_change_items() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut device = account.device(&mut server);
    let vault = create_vault(&mut device, &mut server);
    let id = add(&mut device, &vault, "Bank", T0);
    let mut device = lock(device, &mut server);

    assert!(matches!(device.items(&vault), Err(SyncError::Locked)));
    assert!(matches!(device.item(&vault, &id), Err(SyncError::Locked)));
    assert!(matches!(
        device.save_item(&vault, &id, &note("x", T0)),
        Err(SyncError::Locked)
    ));
    assert!(matches!(
        device.delete_item(&vault, &id),
        Err(SyncError::Locked)
    ));
    assert!(matches!(device.purge_trash(T0), Err(SyncError::Locked)));
    assert!(matches!(
        device.start_create_vault(),
        Err(SyncError::Locked)
    ));
    assert!(matches!(device.keyset(), Err(SyncError::Locked)));
}

#[test]
fn a_locked_device_uploads_what_is_queued() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut phone = account.device(&mut server);
    let vault = create_vault(&mut phone, &mut server);
    // Made by the AutoFill extension, say, and not yet sent.
    let passkey = add(&mut phone, &vault, "passkey", T0);
    let mut phone = lock(phone, &mut server);

    let report = sync(&mut phone, &mut server, T0);
    assert_eq!(report.pushed, 1);
    assert_eq!(
        state(&phone, &vault, &passkey),
        Some((1, SyncState::Synced))
    );

    let mut laptop = account.device(&mut server);
    sync(&mut laptop, &mut server, T0);
    assert_eq!(titles(&laptop, &vault), ["passkey"]);
}

#[test]
fn a_change_on_both_sides_waits_for_unlock() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut laptop = account.device(&mut server);
    let vault = create_vault(&mut laptop, &mut server);
    let bank = add(&mut laptop, &vault, "Bank", T0);
    sync(&mut laptop, &mut server, T0);

    let mut phone = account.device(&mut server);
    sync(&mut phone, &mut server, T0);
    edit(&mut phone, &vault, &bank, "Bank (phone)", T0 + 1);
    let before = cursor(&phone);
    let mut phone = lock(phone, &mut server);

    edit(&mut laptop, &vault, &bank, "Bank (laptop)", T0 + 2);
    let shop = add(&mut laptop, &vault, "Shop", T0 + 3);
    sync(&mut laptop, &mut server, T0);

    // Shop is stored; Bank needs the key, on both the pull and the push.
    let report = sync(&mut phone, &mut server, T0);
    assert_eq!((report.conflicts, report.failed, report.pushed), (0, 0, 0));
    assert_eq!(state(&phone, &vault, &shop), Some((1, SyncState::Synced)));
    assert_eq!(state(&phone, &vault, &bank), Some((2, SyncState::Modified)));
    assert_eq!(cursor(&phone), before, "pulled again after unlock");
    assert_eq!(server.vault(&vault).items[bank.as_bytes()].0, 2);

    // A second locked run changes nothing.
    sync(&mut phone, &mut server, T0);
    assert_eq!(state(&phone, &vault, &bank), Some((2, SyncState::Modified)));

    let mut phone = unlock(phone, &account, &mut server);
    let report = sync(&mut phone, &mut server, T0);
    assert_eq!(report.conflicts, 1);
    assert_eq!(cursor(&phone), server.vault(&vault).seq);
    let item = phone.item(&vault, &bank).unwrap().unwrap();
    assert_eq!(item.title, "Bank (laptop)");
    assert!(history_titles(&item).contains(&"Bank (phone)".to_owned()));
    assert_eq!(titles(&phone, &vault), ["Bank (laptop)", "Shop"]);
}

#[test]
fn a_delete_over_an_unsent_edit_waits_for_unlock() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut laptop = account.device(&mut server);
    let vault = create_vault(&mut laptop, &mut server);
    let bank = add(&mut laptop, &vault, "Bank", T0);
    sync(&mut laptop, &mut server, T0);

    let mut phone = account.device(&mut server);
    sync(&mut phone, &mut server, T0);
    edit(&mut phone, &vault, &bank, "Bank (phone)", T0 + 1);
    let mut phone = lock(phone, &mut server);

    laptop.delete_item(&vault, &bank).unwrap();
    sync(&mut laptop, &mut server, T0);

    sync(&mut phone, &mut server, T0);
    assert_eq!(state(&phone, &vault, &bank), Some((2, SyncState::Modified)));

    // Unlocked, the edit is kept as a new item (D28 §7).
    let mut phone = unlock(phone, &account, &mut server);
    sync(&mut phone, &mut server, T0);
    assert_eq!(state(&phone, &vault, &bank), None);
    assert_eq!(titles(&phone, &vault), ["Bank (phone)"]);
}

#[test]
fn a_new_vault_is_stored_while_locked() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut laptop = account.device(&mut server);
    create_vault(&mut laptop, &mut server);
    let mut phone = account.device(&mut server);
    sync(&mut phone, &mut server, T0);
    let mut phone = lock(phone, &mut server);

    let second = create_vault(&mut laptop, &mut server);
    add(&mut laptop, &second, "Work", T0);
    sync(&mut laptop, &mut server, T0);

    let report = sync(&mut phone, &mut server, T0);
    assert_eq!(report.pulled, 1);
    assert_eq!(phone.store().vaults().unwrap().len(), 2);
    let phone = unlock(phone, &account, &mut server);
    assert_eq!(titles(&phone, &second), ["Work"]);
}

#[test]
fn a_restored_server_waits_for_unlock() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut phone = account.device(&mut server);
    let vault = create_vault(&mut phone, &mut server);
    let bank = add(&mut phone, &vault, "Bank", T0);
    sync(&mut phone, &mut server, T0);

    // The server loses Bank in a restore.
    server.vaults.get_mut(&vault).unwrap().items.clear();
    server.mark_restored();
    let mut phone = lock(phone, &mut server);
    let report = sync(&mut phone, &mut server, T0);
    assert!(!report.resynced);
    assert_eq!(state(&phone, &vault, &bank), Some((1, SyncState::Synced)));

    // Unlocked, the restore is handled and Bank goes up again.
    let mut phone = unlock(phone, &account, &mut server);
    let report = sync(&mut phone, &mut server, T0);
    assert!(report.resynced);
    assert!(server.vault(&vault).items.contains_key(bank.as_bytes()));
}

#[test]
fn a_locked_device_that_was_revoked_clears_its_cache() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut phone = account.device(&mut server);
    let vault = create_vault(&mut phone, &mut server);
    add(&mut phone, &vault, "Bank", T0);
    let store = phone.into_store();
    let tokens = server.new_device();
    server.revoke(&tokens);
    let mut phone = SyncEngine::locked(store, tokens).unwrap();

    let step = phone.start_sync().unwrap();
    assert!(matches!(
        drive(&mut phone, &mut server, step, T0),
        Ran::Done(SyncOutcome::DeviceRevoked)
    ));
    assert!(phone.store().vaults().unwrap().is_empty());
}

#[test]
fn a_locked_device_signed_out_keeps_its_unsent_changes() {
    let mut server = FakeServer::new();
    let account = Account::new();
    let mut phone = account.device(&mut server);
    let vault = create_vault(&mut phone, &mut server);
    let id = add(&mut phone, &vault, "Bank", T0);
    let mut phone = lock(phone, &mut server);
    server.end_sessions();

    let step = phone.start_sync().unwrap();
    assert!(matches!(
        drive(&mut phone, &mut server, step, T0),
        Ran::Done(SyncOutcome::SignedOut)
    ));
    assert_eq!(state(&phone, &vault, &id), Some((1, SyncState::Modified)));
}

#[test]
fn an_empty_locked_cache_syncs_nothing() {
    let mut server = FakeServer::new();
    let mut device =
        SyncEngine::locked(SqliteStore::open_in_memory().unwrap(), server.new_device()).unwrap();
    let report = sync(&mut device, &mut server, T0);
    assert_eq!((report.pulled, report.pushed), (0, 0));
}
