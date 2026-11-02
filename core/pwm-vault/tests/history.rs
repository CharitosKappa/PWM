//! Item history (last 10 versions) and 30-day trash (decision D19).

use pwm_crypto::{Id, Secret, SymmetricKey};
use pwm_vault::{
    HISTORY_LIMIT, Item, ItemId, ItemKind, Login, TRASH_RETENTION_MS, VaultError, decrypt_item,
    encrypt_item, trash_folder,
};

const VAULT: Id = [0xB1; 16];
const T0: i64 = 1_767_225_600_000;
const DAY: i64 = 24 * 60 * 60 * 1000;

fn login(password: &str) -> Item {
    let mut l = Login::default();
    l.username = "alice".into();
    l.password = password.into();
    Item::new(ItemKind::Login(l), "Bank", T0)
}

fn password(item: &Item) -> &str {
    match &item.kind {
        ItemKind::Login(l) => &l.password,
        _ => panic!("not a login"),
    }
}

fn edited(item: &Item, new_password: &str) -> Item {
    let mut copy = item.clone();
    if let ItemKind::Login(l) = &mut copy.kind {
        l.password = new_password.into();
    }
    copy
}

// --- History ---

#[test]
fn an_edit_keeps_the_previous_version() {
    let mut item = login("first");
    item.apply_edit(edited(&item, "second"), T0 + 1000).unwrap();

    assert_eq!(password(&item), "second");
    assert_eq!(item.updated_at, T0 + 1000);
    assert_eq!(item.created_at, T0);
    let history = item.history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].saved_at, T0 + 1000);
    assert_eq!(password(&history[0].item), "first");
    assert_eq!(history[0].item.updated_at, T0);
    assert!(history[0].item.history().is_empty());
}

#[test]
fn history_is_newest_first_and_capped_at_ten() {
    assert_eq!(HISTORY_LIMIT, 10);
    let mut item = login("p0");
    for n in 1..=12 {
        item.apply_edit(edited(&item, &format!("p{n}")), T0 + n * 1000)
            .unwrap();
    }
    assert_eq!(password(&item), "p12");
    let kept: Vec<&str> = item.history().iter().map(|v| password(&v.item)).collect();
    assert_eq!(
        kept,
        ["p11", "p10", "p9", "p8", "p7", "p6", "p5", "p4", "p3", "p2"]
    );
}

#[test]
fn favorite_only_and_identical_edits_add_no_version() {
    let mut item = login("same");
    let mut fav = item.clone();
    fav.favorite = true;
    item.apply_edit(fav, T0 + 1000).unwrap();
    assert!(item.favorite);
    assert_eq!(item.updated_at, T0 + 1000);
    assert!(item.history().is_empty());

    let same = item.clone();
    item.apply_edit(same, T0 + 2000).unwrap();
    assert!(item.history().is_empty());
}

#[test]
fn an_edit_cannot_rewrite_history_trash_state_or_creation_time() {
    let mut item = login("first");
    item.apply_edit(edited(&item, "second"), T0 + 1000).unwrap();
    item.trash(T0 + 1500).unwrap();

    let mut sneaky = Item::new(ItemKind::Note, "replaced", T0 + 9999);
    sneaky.trashed_at = None;
    item.apply_edit(sneaky, T0 + 2000).unwrap();

    assert_eq!(item.kind, ItemKind::Note);
    assert_eq!(item.created_at, T0);
    assert_eq!(item.trashed_at, Some(T0 + 1500));
    assert_eq!(item.history().len(), 2);
    assert_eq!(password(&item.history()[0].item), "second");
}

#[test]
fn conflicting_version_is_kept_in_history() {
    let mut item = login("ours");
    let theirs = login("theirs");
    item.keep_version(theirs, T0 + 500).unwrap();
    assert_eq!(password(&item), "ours");
    assert_eq!(item.history().len(), 1);
    assert_eq!(item.history()[0].saved_at, T0 + 500);
    assert_eq!(password(&item.history()[0].item), "theirs");
}

#[test]
fn a_conflict_keeps_the_losers_offline_versions_newest_first() {
    // Both devices start from "base"; the loser edited twice offline.
    let base = login("base");
    let mut winner = base.clone();
    winner
        .apply_edit(edited(&winner, "winner"), T0 + 30)
        .unwrap();
    let mut loser = base;
    loser
        .apply_edit(edited(&loser, "loser 1"), T0 + 10)
        .unwrap();
    loser
        .apply_edit(edited(&loser, "loser 2"), T0 + 20)
        .unwrap();

    winner.keep_conflicting(loser, T0 + 40).unwrap();
    let versions: Vec<(&str, i64)> = winner
        .history()
        .iter()
        .map(|v| (password(&v.item), v.saved_at))
        .collect();
    // "base" appears once; the loser's current and earlier edits are kept.
    assert_eq!(
        versions,
        [
            ("loser 2", T0 + 40),
            ("base", T0 + 30),
            ("loser 1", T0 + 20)
        ]
    );
    assert_eq!(password(&winner), "winner");

    // Still capped at ten.
    let mut big = login("v0");
    for n in 1..=HISTORY_LIMIT as i64 {
        big.apply_edit(edited(&big, &format!("v{n}")), T0 + n)
            .unwrap();
    }
    let mut other = login("x");
    other.apply_edit(edited(&other, "y"), T0 + 100).unwrap();
    big.keep_conflicting(other, T0 + 200).unwrap();
    assert_eq!(big.history().len(), HISTORY_LIMIT);
    assert_eq!(password(&big.history()[0].item), "y");
    assert_eq!(password(&big.history()[1].item), "x");
}

#[test]
fn restoring_a_version_keeps_the_current_one() {
    let mut item = login("first");
    item.apply_edit(edited(&item, "second"), T0 + 1000).unwrap();
    item.restore_version(0, T0 + 2000).unwrap();
    assert_eq!(password(&item), "first");
    assert_eq!(item.updated_at, T0 + 2000);
    let kept: Vec<&str> = item.history().iter().map(|v| password(&v.item)).collect();
    assert_eq!(kept, ["second", "first"]);
    assert!(matches!(
        item.restore_version(5, T0 + 3000),
        Err(VaultError::Invalid(_))
    ));
}

#[test]
fn read_only_items_cannot_be_edited_or_trashed() {
    let json = r#"{"v":2,"type":"note","title":"t","created_at":0,"updated_at":0}"#;
    let mut item = Item::from_json(json).unwrap();
    let edit = item.clone();
    assert!(matches!(
        item.apply_edit(edit, T0),
        Err(VaultError::ReadOnly)
    ));
    assert!(matches!(item.trash(T0), Err(VaultError::ReadOnly)));
    assert!(matches!(
        item.keep_version(login("x"), T0),
        Err(VaultError::ReadOnly)
    ));
}

#[test]
fn history_and_trash_survive_json_and_encryption() {
    let mut item = login("first");
    item.apply_edit(edited(&item, "second"), T0 + 1000).unwrap();
    item.apply_edit(edited(&item, "third"), T0 + 2000).unwrap();
    item.trash(T0 + 3000).unwrap();

    let back = Item::from_json(item.to_json().unwrap().expose_secret()).unwrap();
    assert_eq!(back, item);

    let key = SymmetricKey::from_secret(Secret::new([0x34; 32]));
    let id = ItemId::from_bytes([1; 16]);
    let env = encrypt_item(&key, &VAULT, &id, 3, &item).unwrap();
    assert_eq!(decrypt_item(&key, &VAULT, &id, 3, &env).unwrap(), item);
}

#[test]
fn fixture_with_history_and_trash() {
    let fixtures: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/items_v1.json")).unwrap();
    let item = Item::from_json(&fixtures[8].to_string()).unwrap();
    assert_eq!(password(&item), "third-password");
    assert_eq!(item.trashed_at, Some(1_767_484_800_000));
    let kept: Vec<&str> = item.history().iter().map(|v| password(&v.item)).collect();
    assert_eq!(kept, ["second-password", "first-password"]);
    let out = item.to_json().unwrap();
    assert!(out.expose_secret().contains("future_snapshot_field"));
}

#[test]
fn malformed_history_is_rejected() {
    let cases = [
        // Top-level item must not carry saved_at.
        r#"{"v":1,"type":"note","title":"t","created_at":0,"updated_at":0,"saved_at":1}"#,
        // History entries need saved_at.
        r#"{"v":1,"type":"note","title":"t","created_at":0,"updated_at":0,
            "history":[{"v":1,"type":"note","title":"t","created_at":0,"updated_at":0}]}"#,
        // History entries cannot nest history.
        r#"{"v":1,"type":"note","title":"t","created_at":0,"updated_at":0,
            "history":[{"saved_at":1,"v":1,"type":"note","title":"t","created_at":0,"updated_at":0,
            "history":[{"saved_at":2,"v":1,"type":"note","title":"t","created_at":0,"updated_at":0}]}]}"#,
    ];
    for json in cases {
        assert!(
            matches!(Item::from_json(json), Err(VaultError::InvalidItem)),
            "{json}"
        );
    }
}

// --- Trash ---

#[test]
fn trash_and_restore() {
    let mut item = login("pw");
    assert!(!item.is_trashed());
    item.trash(T0 + DAY).unwrap();
    assert!(item.is_trashed());
    assert_eq!(item.trashed_at, Some(T0 + DAY));
    assert!(item.history().is_empty(), "trashing is not an edit");
    item.restore_from_trash(T0 + 2 * DAY).unwrap();
    assert!(!item.is_trashed());
    assert_eq!(item.updated_at, T0 + 2 * DAY);
}

#[test]
fn purge_is_due_after_thirty_days() {
    assert_eq!(TRASH_RETENTION_MS, 30 * DAY);
    let mut item = login("pw");
    assert!(!item.is_due_for_purge(T0 + 100 * DAY), "not trashed");
    item.trash(T0).unwrap();
    assert!(!item.is_due_for_purge(T0 + 30 * DAY - 1));
    assert!(item.is_due_for_purge(T0 + 30 * DAY));
}

#[test]
fn trashing_a_folder_moves_its_items_out() {
    let folder_id = ItemId::from_bytes([7; 16]);
    let mut folder = Item::new(ItemKind::Folder(Default::default()), "Work", T0);
    let mut items = vec![login("a"), login("b"), login("c")];
    items[0].folder_id = Some(folder_id);
    items[2].folder_id = Some(folder_id);

    assert_eq!(
        trash_folder(&mut folder, &folder_id, &mut items, T0 + DAY).unwrap(),
        2
    );
    assert!(folder.is_trashed());
    assert!(items.iter().all(|i| i.folder_id.is_none()));

    folder.restore_from_trash(T0 + 2 * DAY).unwrap();
    assert!(
        items.iter().all(|i| i.folder_id.is_none()),
        "items stay out"
    );

    let mut not_a_folder = login("x");
    assert!(matches!(
        trash_folder(&mut not_a_folder, &folder_id, &mut items, T0),
        Err(VaultError::Invalid(_))
    ));
}
