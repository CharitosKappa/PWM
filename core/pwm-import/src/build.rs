//! Turning foreign records into D18 items, with the same rules for every
//! format (D51 §3): empty titles, URLs, one-time codes, custom fields,
//! password history, the trash, folders, and the report.

use std::collections::BTreeMap;

use pwm_vault::{
    Card, CustomField, FieldKind, Folder, HISTORY_LIMIT, Identity, Item, ItemId, ItemKind, Login,
    LoginUrl, UrlMatch, normalize_totp,
};

use crate::{ImportError, MAX_ITEMS, Parsed};

/// Why something was left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LeftOut {
    /// Files attached to items, and document items.
    Attachments,
    SshKeys,
    /// Proton Pass email aliases.
    Aliases,
    /// Passkeys whose private key isn't in the export, or not ES256.
    Passkeys,
    /// Item types we have no place for.
    UnknownTypes,
    /// Records with nothing in them.
    Empty,
    /// Over the sync size limit even without history (D29); counted when
    /// saving.
    TooLarge,
    /// Identical to an item already in the vault; counted when saving.
    Duplicates,
}

impl LeftOut {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Attachments => "attachments",
            Self::SshKeys => "ssh_keys",
            Self::Aliases => "aliases",
            Self::Passkeys => "passkeys",
            Self::UnknownTypes => "unknown_types",
            Self::Empty => "empty",
            Self::TooLarge => "too_large",
            Self::Duplicates => "duplicates",
        }
    }
}

/// What an import brings in, and what it leaves out (with counts).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub logins: usize,
    pub notes: usize,
    pub cards: usize,
    pub identities: usize,
    pub folders: usize,
    /// Of the above, how many come in trashed (archived or deleted in the
    /// source).
    pub trashed: usize,
    pub left_out: BTreeMap<LeftOut, usize>,
}

impl Report {
    pub fn leave_out(&mut self, reason: LeftOut, count: usize) {
        if count > 0 {
            *self.left_out.entry(reason).or_default() += count;
        }
    }

    fn count(&mut self, item: &Item) {
        match item.kind {
            ItemKind::Login(_) => self.logins += 1,
            ItemKind::Note => self.notes += 1,
            ItemKind::Card(_) => self.cards += 1,
            ItemKind::Identity(_) => self.identities += 1,
            ItemKind::Folder(_) => self.folders += 1,
            ItemKind::Unknown(_) => {}
        }
        if item.trashed_at.is_some() {
            self.trashed += 1;
        }
    }

    /// One fewer of the item's kind (left out when saving).
    pub fn uncount(&mut self, item: &Item) {
        let n = match item.kind {
            ItemKind::Login(_) => &mut self.logins,
            ItemKind::Note => &mut self.notes,
            ItemKind::Card(_) => &mut self.cards,
            ItemKind::Identity(_) => &mut self.identities,
            ItemKind::Folder(_) => &mut self.folders,
            ItemKind::Unknown(_) => return,
        };
        *n = n.saturating_sub(1);
        if item.trashed_at.is_some() {
            self.trashed = self.trashed.saturating_sub(1);
        }
    }
}

/// Collects folders and items for one import.
pub(crate) struct Builder {
    pub now_ms: i64,
    /// Folder path (names joined by `\u{0}`) to its id and item.
    folders: Vec<(String, ItemId, Item)>,
    items: Vec<(ItemId, Item)>,
    pub report: Report,
}

/// Nested folders deeper than this are cut (as the CSV export does).
const MAX_FOLDER_DEPTH: usize = 16;

impl Builder {
    pub fn new(now_ms: i64) -> Self {
        Self {
            now_ms,
            folders: Vec::new(),
            items: Vec::new(),
            report: Report::default(),
        }
    }

    /// The folder at `path` (outermost first), made with its parents as
    /// needed. Empty names are skipped; an empty path is no folder.
    pub fn folder<S: AsRef<str>>(&mut self, path: &[S]) -> Result<Option<ItemId>, ImportError> {
        let mut parent: Option<ItemId> = None;
        let mut key = String::new();
        for name in path
            .iter()
            .map(|n| n.as_ref().trim())
            .filter(|n| !n.is_empty())
            .take(MAX_FOLDER_DEPTH)
        {
            if !key.is_empty() {
                key.push('\u{0}');
            }
            key.push_str(name);
            if let Some((_, id, _)) = self.folders.iter().find(|(k, _, _)| *k == key) {
                parent = Some(*id);
                continue;
            }
            self.check_room()?;
            let mut folder = Folder::default();
            folder.parent_id = parent;
            let item = Item::new(ItemKind::Folder(folder), name, self.now_ms);
            let id = ItemId::generate()?;
            self.report.count(&item);
            self.folders.push((key.clone(), id, item));
            parent = Some(id);
        }
        Ok(parent)
    }

    /// A folder from a path written with `separator` (`a/b`, `a\b`).
    pub fn folder_path(
        &mut self,
        path: &str,
        separator: char,
    ) -> Result<Option<ItemId>, ImportError> {
        let parts: Vec<&str> = path.split(separator).collect();
        self.folder(&parts)
    }

    fn check_room(&self) -> Result<(), ImportError> {
        if self.folders.len() + self.items.len() >= MAX_ITEMS {
            return Err(ImportError::TooLarge);
        }
        Ok(())
    }

    /// Adds a finished item: an empty title becomes the first URL's host,
    /// or "Untitled".
    pub fn push(&mut self, mut item: Item) -> Result<(), ImportError> {
        self.check_room()?;
        if item.title.trim().is_empty() {
            item.title = fallback_title(&item);
        }
        self.report.count(&item);
        self.items.push((ItemId::generate()?, item));
        Ok(())
    }

    /// Counts an item added by the caller with its own id (our exports).
    pub fn report_item(&mut self, item: &Item) {
        self.report.count(item);
    }

    pub fn leave_out(&mut self, reason: LeftOut) {
        self.report.leave_out(reason, 1);
    }

    pub fn finish(self) -> Parsed {
        let mut items: Vec<(ItemId, Item)> = self
            .folders
            .into_iter()
            .map(|(_, id, item)| (id, item))
            .collect();
        items.extend(self.items);
        Parsed {
            items,
            report: self.report,
        }
    }
}

fn fallback_title(item: &Item) -> String {
    if let ItemKind::Login(login) = &item.kind
        && let Some(host) = login.urls.iter().find_map(|u| {
            url::Url::parse(u.url.trim())
                .ok()?
                .host_str()
                .map(str::to_owned)
        })
    {
        return host;
    }
    "Untitled".to_owned()
}

// --- Making items ---

pub fn new_item(kind: ItemKind, title: &str, now_ms: i64) -> Item {
    Item::new(kind, title.trim(), now_ms)
}

pub fn login_kind() -> ItemKind {
    ItemKind::Login(Login::default())
}

pub fn card_kind() -> ItemKind {
    ItemKind::Card(Card::default())
}

pub fn identity_kind() -> ItemKind {
    ItemKind::Identity(Identity::default())
}

pub fn login(item: &mut Item) -> Option<&mut Login> {
    match &mut item.kind {
        ItemKind::Login(login) => Some(login),
        _ => None,
    }
}

pub fn card(item: &mut Item) -> Option<&mut Card> {
    match &mut item.kind {
        ItemKind::Card(card) => Some(card),
        _ => None,
    }
}

pub fn identity(item: &mut Item) -> Option<&mut Identity> {
    match &mut item.kind {
        ItemKind::Identity(identity) => Some(identity),
        _ => None,
    }
}

/// Adds a login URL; blank ones and duplicates are skipped.
pub fn add_url(item: &mut Item, url: &str, mode: UrlMatch) {
    let url = url.trim();
    let Some(login) = login(item) else { return };
    if url.is_empty() || login.urls.iter().any(|u| u.url == url) {
        return;
    }
    let mut entry = LoginUrl::default();
    entry.url = url.to_owned();
    entry.r#match = mode;
    login.urls.push(entry);
}

/// The one-time code, through D23's check. One that is refused (or an item
/// that is not a login) keeps the value as a hidden field, so nothing is
/// lost.
pub fn set_totp(item: &mut Item, value: &str) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    let title = item.title.clone();
    if let Some(login) = login(item)
        && login.totp.is_none()
        && let Ok(normalized) = normalize_totp(value, &title)
    {
        login.totp = Some(normalized.expose_secret().clone());
        return;
    }
    add_field(item, "One-time code", value, FieldKind::Hidden);
}

/// A custom field; empty values are skipped.
pub fn add_field(item: &mut Item, name: &str, value: &str, kind: FieldKind) {
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    let mut field = CustomField::default();
    field.name = name.trim().to_owned();
    field.value = value.to_owned();
    field.kind = kind;
    item.custom_fields.push(field);
}

/// Appends a line to the notes.
pub fn add_note(item: &mut Item, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    if !item.notes.is_empty() {
        item.notes.push('\n');
    }
    item.notes.push_str(text);
}

/// Earlier passwords as D19 history, newest first and at most 10:
/// `(password, when it was replaced, Unix ms)`.
pub fn add_password_history(item: &mut Item, mut old: Vec<(String, i64)>) {
    if !matches!(item.kind, ItemKind::Login(_)) {
        return;
    }
    old.retain(|(p, _)| !p.is_empty());
    // Oldest first, so the newest is pushed last and ends up first.
    old.sort_by_key(|(_, at)| *at);
    let start = old.len().saturating_sub(HISTORY_LIMIT);
    for (password, at) in old.into_iter().skip(start) {
        let mut version = item.clone();
        if let Some(login) = login(&mut version) {
            login.password = password;
        }
        version.updated_at = at;
        // Only an item from a newer schema refuses, and ours never is.
        let _ = item.keep_version(version, at);
    }
}

/// Marks the item trashed (archived or deleted in the source).
pub fn trash(item: &mut Item, now_ms: i64) {
    let _ = item.trash(now_ms);
}

/// A card expiry month and year from text like `05/27`, `5/2027`,
/// `2027-05`, `202705` or `January,2027`.
pub fn expiry(text: &str) -> (Option<u8>, Option<u16>) {
    let text = text.trim();
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    let year = |y: &str| -> Option<u16> {
        let y: u16 = y.parse().ok()?;
        Some(if y < 100 { 2000 + y } else { y })
    };
    let month = |m: &str| -> Option<u8> { m.parse().ok().filter(|m| (1..=12).contains(m)) };
    if let Some((a, b)) = text.split_once(['/', '-', ',', ' ', '.']) {
        let (a, b) = (a.trim(), b.trim());
        if let Some(m) = month_name(a) {
            return (Some(m), year(b));
        }
        if a.len() == 4 {
            return (month(b), year(a));
        }
        return (month(a), year(b));
    }
    match digits.len() {
        6 if text.len() == 6 => (month(&digits[4..]), year(&digits[..4])),
        4 => (month(&digits[..2]), year(&digits[2..])),
        _ => (None, None),
    }
}

fn month_name(text: &str) -> Option<u8> {
    const NAMES: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let lower = text.to_ascii_lowercase();
    NAMES
        .iter()
        .position(|n| lower.starts_with(n))
        .and_then(|i| u8::try_from(i + 1).ok())
}

/// Seconds or milliseconds since 1970 to milliseconds (sources use both).
pub fn ms_from(time: i64) -> i64 {
    if time > 0 && time < 100_000_000_000 {
        time.saturating_mul(1000)
    } else {
        time
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_reads_the_usual_forms() {
        assert_eq!(expiry("05/27"), (Some(5), Some(2027)));
        assert_eq!(expiry("5/2027"), (Some(5), Some(2027)));
        assert_eq!(expiry("2027-05"), (Some(5), Some(2027)));
        assert_eq!(expiry("202705"), (Some(5), Some(2027)));
        assert_eq!(expiry("January,2027"), (Some(1), Some(2027)));
        assert_eq!(expiry("13/27"), (None, Some(2027)));
        assert_eq!(expiry(""), (None, None));
    }

    #[test]
    fn folders_are_made_once_with_their_parents() {
        let mut b = Builder::new(1);
        let a = b.folder(&["Work", "Banks"]).unwrap().unwrap();
        let again = b.folder_path("Work/Banks", '/').unwrap().unwrap();
        assert_eq!(a, again);
        assert_eq!(b.folder(&["", " "]).unwrap(), None);
        let parsed = b.finish();
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.report.folders, 2);
        let ItemKind::Folder(banks) = &parsed.items[1].1.kind else {
            panic!("not a folder")
        };
        assert_eq!(banks.parent_id, Some(parsed.items[0].0));
    }

    #[test]
    fn password_history_is_newest_first_and_at_most_ten() {
        let mut item = new_item(login_kind(), "x", 100);
        login(&mut item).unwrap().password = "current".into();
        let old: Vec<(String, i64)> = (0..12).map(|i| (format!("p{i}"), i)).collect();
        add_password_history(&mut item, old);
        let history = item.history();
        assert_eq!(history.len(), HISTORY_LIMIT);
        let first = match &history[0].item.kind {
            ItemKind::Login(l) => l.password.clone(),
            _ => String::new(),
        };
        assert_eq!(first, "p11");
    }

    #[test]
    fn a_refused_one_time_code_is_kept_as_a_hidden_field() {
        let mut item = new_item(login_kind(), "x", 1);
        set_totp(&mut item, "not a secret!");
        assert!(login(&mut item).unwrap().totp.is_none());
        assert_eq!(item.custom_fields.len(), 1);
        assert_eq!(item.custom_fields[0].kind, FieldKind::Hidden);
        let mut item = new_item(login_kind(), "x", 1);
        set_totp(&mut item, "JBSWY3DPEHPK3PXP");
        assert!(login(&mut item).unwrap().totp.is_some());
    }
}
